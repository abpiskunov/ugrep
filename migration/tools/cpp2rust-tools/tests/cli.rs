//! End-to-end tests through the built binary: run-cases and gate-check run child processes
//! (the binary's hidden `__fixture` command), and every subcommand's exit codes are checked.

use cpp2rust_tools::gate_check::{Ctx, run_gate};
use cpp2rust_tools::run_cases::run;
use cpp2rust_tools::testutil::TempDir;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_cpp2rust-tools");

fn tool(args: &[&str], cwd: &Path) -> (i32, String, String) {
    let o = Command::new(BIN).args(args).current_dir(cwd).output().unwrap();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into(),
        String::from_utf8_lossy(&o.stderr).into(),
    )
}

/// A wrapper that runs `cpp2rust-tools __fixture <mode>` as if it were a product binary.
#[cfg(windows)]
fn fixture_bin(d: &TempDir, mode: &str) -> PathBuf {
    let p = d.path().join(format!("{mode}.cmd"));
    std::fs::write(&p, format!("@\"{BIN}\" __fixture {mode} %*\r\n")).unwrap();
    p
}

/// A wrapper that runs `cpp2rust-tools __fixture <mode>` as if it were a product binary.
#[cfg(unix)]
fn fixture_bin(d: &TempDir, mode: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = d.path().join(mode);
    std::fs::write(&p, format!("#!/bin/sh\nexec \"{BIN}\" __fixture {mode} \"$@\"\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[test]
fn run_cases_records_and_finds_nondeterminism() {
    let d = TempDir::new("runcases");
    let echo = fixture_bin(&d, "echo");
    let cases = vec![
        json!({"id": "ok", "args": ["a"], "stdin": "abc"}),
        json!({"id": "bad", "args": ["--fail"]}),
    ];
    let out = d.path().join("t.jsonl");
    let matrix = vec![json!({"LC_ALL": "C"}), json!({"LC_ALL": "tr_TR.UTF-8"})];
    let (n, flaky) = run(&echo, "cpp", &cases, Some(&matrix), 2, 20.0, &out).unwrap();
    let ev: Vec<Value> = std::fs::read_to_string(&out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!((n, flaky.len(), ev.len()), (4, 0, 20));
    let exit = |case: &str| ev.iter().find(|e| e["kind"] == "exit" && e["case"] == case).unwrap()["ret"].clone();
    assert_eq!(exit("ok@0"), json!({"category": "ok", "code": 0}));
    assert_eq!(exit("bad@1"), json!({"category": "error", "code": 3}));
    let fs = &ev.iter().find(|e| e["kind"] == "fs" && e["case"] == "ok@0").unwrap()["state"];
    assert_eq!(fs["out.txt"]["size"], 3);
    let stdout = &ev.iter().find(|e| e["kind"] == "stdout" && e["case"] == "ok@1").unwrap()["ret"]["lines"][0];
    assert!(stdout.as_str().unwrap().contains("tr_TR.UTF-8"), "{stdout}");
    // a nondeterministic binary is reported as flaky
    let rnd = fixture_bin(&d, "random");
    let (_, flaky) = run(&rnd, "cpp", &[json!({"id": "r"})], None, 3, 20.0, &out).unwrap();
    assert_eq!(flaky, vec!["r"]);
}

#[test]
fn run_cases_cli_exit_codes() {
    let d = TempDir::new("runcases-cli");
    let rnd = fixture_bin(&d, "random");
    d.write("cases.jsonl", "{\"id\": \"r\"}\n");
    let (rc, out, _) = tool(
        &[
            "run-cases",
            "--bin",
            rnd.to_str().unwrap(),
            "--impl",
            "cpp",
            "--cases",
            "cases.jsonl",
            "--out",
            "o.jsonl",
            "--repeat",
            "2",
        ],
        d.path(),
    );
    assert_eq!(rc, 3, "{out}");
    assert!(out.contains("FLAKY r"));
    d.write("dup.jsonl", "{\"id\": \"r\"}\n{\"id\": \"r\"}\n");
    let (rc, _, err) = tool(
        &[
            "run-cases",
            "--bin",
            rnd.to_str().unwrap(),
            "--impl",
            "cpp",
            "--cases",
            "dup.jsonl",
            "--out",
            "o.jsonl",
        ],
        d.path(),
    );
    assert_eq!(rc, 2, "{err}");
    assert_eq!(tool(&["run-cases", "--impl", "go"], d.path()).0, 2);
}

#[test]
fn run_cases_templates_the_work_dir_in_every_spelling() {
    let d = TempDir::new("runcases-work");
    let out = d.path().join("t.jsonl");
    // a program printing its directory the portable way (`/`) is still deterministic
    let pwd = fixture_bin(&d, "pwd");
    let (_, flaky) = run(&pwd, "cpp", &[json!({"id": "p"})], None, 2, 20.0, &out).unwrap();
    assert_eq!(flaky, Vec::<String>::new());
    let ev: Vec<Value> = std::fs::read_to_string(&out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let stdout = |ev: &[Value]| ev.iter().find(|e| e["kind"] == "stdout").unwrap()["ret"]["lines"].clone();
    assert_eq!(stdout(&ev), json!(["$WORK/w\n"]));
    if cfg!(windows) {
        // the per-user folders are in the scratch directory too, and programs can start others
        let env = fixture_bin(&d, "env");
        let case = json!({"id": "e", "args": ["APPDATA", "LOCALAPPDATA", "TEMP", "SystemRoot", "ComSpec", "PATHEXT"]});
        run(&env, "cpp", &[case], None, 1, 20.0, &out).unwrap();
        let ev: Vec<Value> = std::fs::read_to_string(&out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let lines: Vec<String> = stdout(&ev)
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l.as_str().unwrap().to_string())
            .collect();
        assert_eq!(lines[0], "APPDATA=$WORK\\.home\\AppData\\Roaming\n", "{lines:?}");
        assert_eq!(lines[1], "LOCALAPPDATA=$WORK\\.home\\AppData\\Local\n");
        assert_eq!(lines[2], "TEMP=$WORK\\.tmp\n");
        assert!(
            lines[3..]
                .iter()
                .all(|l| l.trim_end().split_once('=').is_some_and(|(_, v)| !v.is_empty())),
            "{lines:?}"
        );
    }
}

fn gate_plan() -> Value {
    json!({"components": {"core": {"rust_crate": "rust/core"}},
           "gates": {
             "unit": {"scope": "unit", "checks": [
                {"name": "tests", "run": "{tools} __fixture junit {out}/j.xml t1 t2",
                 "expect": [{"junit": "{out}/j.xml", "max_failures": 0, "min_tests": 2}]},
                {"name": "seam", "run": "{tools} __fixture seam {out}/s.json {div}",
                 "expect": [{"json": "{out}/s.json", "field": "divergent_cases", "op": "==", "value": 0},
                            {"json": "{out}/s.json", "field": "s.0.n", "op": ">=", "value": 5}]}]},
             "broken": {"scope": "component", "checks": [{"name": "nope", "run": "definitely-not-a-command-xyz"}]},
             "empty": {"scope": "component", "checks": [{"name": "none", "run": "{tools} __fixture exit 0",
                       "expect": [{"junit": "{out}/missing.xml"}]}]},
             "slow": {"scope": "component", "checks": [{"name": "sleep", "run": "{tools} __fixture sleep 5", "timeout": 0.3}]}}})
}

#[test]
fn gate_check_pass_fail_blocked_and_missing_results() {
    let d = TempDir::new("gate");
    d.write(
        "migration/units/core.a.yaml",
        "unit: core.a\ncomponent: core\ndiv: 0\nfunctions: [f]\n",
    );
    d.write("migration/units/core.b.yaml", "unit: core.b\ncomponent: core\ndiv: 2\n");
    let ctx = |dry| Ctx {
        out_root: d.path().join("out"),
        mig: d.path().join("migration"),
        cwd: d.path().to_path_buf(),
        tools: BIN.to_string(),
        base: None,
        dry,
    };
    let plan = gate_plan();
    assert_eq!(run_gate(&plan, "unit", "core.a", &ctx(false)).unwrap()["result"], "pass");
    let r = run_gate(&plan, "unit", "core.b", &ctx(false)).unwrap();
    assert_eq!(r["result"], "fail");
    assert_eq!(r["checks"][0]["status"], "pass");
    assert_eq!(r["checks"][1]["status"], "fail");
    assert_eq!(run_gate(&plan, "broken", "core", &ctx(false)).unwrap()["result"], "blocked"); // never a pass
    assert_eq!(run_gate(&plan, "empty", "core", &ctx(false)).unwrap()["result"], "fail"); // missing result file
    assert_eq!(run_gate(&plan, "unit", "core.a", &ctx(true)).unwrap()["result"], "dry-run");
    let r = run_gate(&plan, "slow", "core", &ctx(false)).unwrap();
    assert_eq!(
        (r["result"].as_str(), r["checks"][0]["exit"].as_str()),
        (Some("fail"), Some("timeout"))
    );
    assert!(d.path().join("out/gates/unit/core.a/report.json").is_file());
    assert_eq!(run_gate(&plan, "nope", "core", &ctx(false)).unwrap_err().0, 2);
    assert_eq!(run_gate(&plan, "unit", "core.zz", &ctx(false)).unwrap_err().0, 2);
}

/// cmd.exe does not end the command it started when it is killed: the timeout ends both.
#[cfg(windows)]
#[test]
fn gate_check_timeout_ends_the_whole_command() {
    let d = TempDir::new("gate-tree");
    let plan = json!({"components": {"core": {}}, "gates": {"slow": {"scope": "component", "checks": [
        {"name": "sleep", "run": "{tools} __fixture sleep 1.5 {out}/late.txt", "timeout": 0.3}]}}});
    let ctx = Ctx {
        out_root: d.path().join("out"),
        mig: d.path().join("migration"),
        cwd: d.path().to_path_buf(),
        tools: BIN.to_string(),
        base: None,
        dry: false,
    };
    let r = run_gate(&plan, "slow", "core", &ctx).unwrap();
    assert_eq!(r["checks"][0]["exit"], "timeout");
    std::thread::sleep(std::time::Duration::from_secs(3));
    assert!(
        !d.path().join("out/gates/slow/core/late.txt").exists(),
        "the command outlived its timeout"
    );
}

#[test]
fn gate_check_cli_quotes_its_own_path_for_the_shell() {
    let d = TempDir::new("gate-quote");
    let dir = d.path().join("t&o(1)");
    std::fs::create_dir_all(&dir).unwrap();
    let copy = dir.join(Path::new(BIN).file_name().unwrap());
    std::fs::copy(BIN, &copy).unwrap();
    d.write("migration/units/core.a.yaml", "unit: core.a\ncomponent: core\ndiv: 0\n");
    let yaml = "components: {core: {}}\ngates:\n  unit:\n    scope: unit\n    checks:\n      - name: seam\n        run: \"{tools} __fixture seam {out}/s.json {div}\"\n        expect: [{json: \"{out}/s.json\", field: divergent_cases, op: \"==\", value: 0}]\n";
    d.write("migration/validation-plan.yaml", yaml);
    let o = Command::new(&copy)
        .args(["gate-check", "--gate", "unit", "--target", "core.a"])
        .current_dir(d.path())
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{out}{}", String::from_utf8_lossy(&o.stderr));
    assert!(out.contains("GATE unit core.a: PASS"), "{out}");
}

#[test]
fn gate_check_cli_uses_its_own_binary_for_tools() {
    let d = TempDir::new("gate-cli");
    d.write(
        "migration/units/core.a.yaml",
        "unit: core.a\ncomponent: core\ndiv: 0\nfunctions: [f]\n",
    );
    let yaml = "components: {core: {rust_crate: rust/core}}\ngates:\n  unit:\n    scope: unit\n    checks:\n      - name: seam\n        run: \"{tools} __fixture seam {out}/s.json {div}\"\n        expect: [{json: \"{out}/s.json\", field: divergent_cases, op: \"==\", value: 0}]\n";
    d.write("migration/validation-plan.yaml", yaml);
    let (rc, out, err) = tool(&["gate-check", "--gate", "unit", "--target", "core.a"], d.path());
    assert_eq!(rc, 0, "{out}{err}");
    assert!(out.contains("GATE unit core.a: PASS"), "{out}");
    let (rc, _, _) = tool(&["gate-check"], d.path());
    assert_eq!(rc, 2);
}

#[test]
fn trace_diff_cli_exit_codes() {
    let d = TempDir::new("tracediff-cli");
    let a = "{\"case\":\"c\",\"seq\":0,\"kind\":\"ret\",\"fn\":\"f\",\"ret\":1}\n";
    d.write("a.jsonl", a);
    d.write("b.jsonl", &a.replace("\"ret\":1", "\"ret\":2"));
    d.write("empty.jsonl", "");
    d.write("bad.jsonl", "not json\n");
    d.write(
        "unapproved.json",
        "{\"masks\": [{\"fn\": \"f\", \"path\": \"ret\", \"approved\": null}]}",
    );
    d.write(
        "approved.json",
        "{\"masks\": [{\"fn\": \"f\", \"path\": \"ret\", \"approved\": \"DQ-1\"}]}",
    );
    let run = |args: &[&str]| tool(args, d.path());
    assert_eq!(run(&["trace-diff", "diff", "--a", "a.jsonl", "--b", "a.jsonl"]).0, 0);
    let (rc, out, _) = run(&["trace-diff", "diff", "--a", "a.jsonl", "--b", "b.jsonl", "--out", "s.json"]);
    assert_eq!(rc, 1);
    assert!(out.contains("FIRST DIVERGENCE case=c") && out.contains("\"path\":\"ret\""), "{out}");
    let s: Value = serde_json::from_str(&std::fs::read_to_string(d.path().join("s.json")).unwrap()).unwrap();
    assert_eq!(s["divergent_cases"], 1);
    assert_eq!(s["unapproved_masks_allowed"], false);
    assert_eq!(
        run(&["trace-diff", "diff", "--a", "a.jsonl", "--b", "b.jsonl", "--masks", "approved.json"]).0,
        0
    );
    assert_eq!(
        run(&[
            "trace-diff",
            "diff",
            "--a",
            "a.jsonl",
            "--b",
            "b.jsonl",
            "--masks",
            "unapproved.json"
        ])
        .0,
        2
    );
    assert_eq!(
        run(&[
            "trace-diff",
            "diff",
            "--a",
            "a.jsonl",
            "--b",
            "b.jsonl",
            "--masks",
            "unapproved.json",
            "--allow-unapproved"
        ])
        .0,
        0
    );
    assert_eq!(run(&["trace-diff", "diff", "--a", "empty.jsonl", "--b", "a.jsonl"]).0, 2); // an empty trace never passes
    assert_eq!(run(&["trace-diff", "diff", "--a", "bad.jsonl", "--b", "a.jsonl"]).0, 2);
    assert_eq!(run(&["trace-diff", "diff", "--a", "a.jsonl"]).0, 2);
    let (rc, out, _) = run(&["trace-diff", "noise", "--a", "a.jsonl", "--b", "b.jsonl", "--out", "m.json"]);
    assert_eq!((rc, out.trim()), (0, "proposed masks: 1, unstable sequences: 0"));
    let (rc, out, _) = run(&["trace-diff", "--help"]);
    assert_eq!(rc, 0);
    assert!(out.contains("trace-diff diff"));
}

#[test]
fn catalog_lint_linked_check_policy_scan_cli() {
    let d = TempDir::new("misc-cli");
    d.write(
        "cat.yaml",
        "component: core\nbehaviors:\n  - {id: B-1, status: observed, evidence: [\"case:nope\"]}\n",
    );
    d.write("ok.yaml", "component: core\nbehaviors:\n  - {id: B-1, status: hypothesis}\n");
    let (rc, out, _) = tool(
        &["catalog-lint", "--catalog", "cat.yaml", "--migration", "m", "--out", "c.json"],
        d.path(),
    );
    assert_eq!(rc, 1, "{out}");
    assert!(out.contains("ERROR E1 B-1") && out.contains("catalog core: FAIL;"), "{out}");
    let (rc, out, _) = tool(&["catalog-lint", "--catalog", "ok.yaml", "--migration", "m"], d.path());
    assert_eq!(rc, 0, "{out}");
    assert!(out.contains("'hypothesis': 1") && out.contains("saturated False"), "{out}");
    assert_eq!(tool(&["catalog-lint"], d.path()).0, 2);
    assert_eq!(tool(&["linked-check", "--map", "x.map"], d.path()).0, 2); // looks for nothing
    let (rc, out, _) = tool(&["linked-check", "--map", "x.map", "--forbid", "tap", "--require", "lib"], d.path());
    assert_eq!(rc, 1, "{out}");
    assert!(out.contains("map_missing"));
    assert_eq!(tool(&["policy-scan"], d.path()).0, 2);
    assert_eq!(tool(&["nonsense"], d.path()).0, 2);
    assert_eq!(tool(&["--help"], d.path()).0, 0);
}

#[test]
fn depgraph_cli_writes_json() {
    let d = TempDir::new("depgraph-cli");
    d.write("src/a/x.cc", "#include \"b/y.h\"\n");
    d.write("src/b/y.h", "int y();\n");
    let (rc, out, err) = tool(
        &["depgraph", "--src-root", "src", "--root", ".", "--out", "dg/depgraph.json"],
        d.path(),
    );
    assert_eq!(rc, 0, "{err}");
    assert!(out.starts_with("level  loc     component"), "{out}");
    assert!(out.contains("\n0      1       src/b\n"), "{out}");
    let g: Value = serde_json::from_str(&std::fs::read_to_string(d.path().join("dg/depgraph.json")).unwrap()).unwrap();
    assert_eq!(g["port_order"], json!(["src/b", "src/a"]));
    assert_eq!(tool(&["depgraph"], d.path()).0, 2);
}
