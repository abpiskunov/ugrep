//! policy-scan: small anti-gaming scan of a change (merge-base of BASE and HEAD .. work tree).

use crate::util::*;
use regex::Regex;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

pub const HELP: &str = "\
cpp2rust-tools policy-scan - small anti-gaming scan of a change (merge-base of BASE and HEAD .. work tree).

  policy-scan --base origin/main [--plan migration/validation-plan.yaml]
              [--forbid-todo PATH ...]   # final-state: no todo!()/unimplemented!() in these files or dirs
              [--no-tap PATH ...]        # final-state: no tap or trace marker left in these files or dirs
              [--out policy.json]

Findings (each one fails the scan):
  new_allow            added #[allow(...)] / #![allow(...)] / #[expect(...)] in Rust
  static_mut           added `static mut`
  test_disabled        added #[ignore], DISABLED_ test name or GTEST_SKIP
  test_deleted         deleted file that looks like a test
  unsafe_outside_ffi   added `unsafe` outside the plan's policy.unsafe_allowed globs
  frozen_modified      a file under policy.frozen (corpus, recorded traces, goldens) was changed other
                       than by appending lines, or deleted (adding is fine), and is not listed in
                       policy.approved_rebaselines
  judge_and_judged     one change touches both policy.protected (plan, masks, harness, tap, tools,
                       CI workflow, frozen files) and policy.implementation (ported code)
  todo_remaining       (--forbid-todo) todo!() or unimplemented!() still present
  tap_remaining        (--no-tap) CPP2RUST_TAP / cpp2rust-tap / cpp2rust_tap / CPP2RUST_TRACE still present
Needs git. Exit: 0 pass, 1 findings, 2 usage error or git failure.";

pub const CLASSES: [&str; 9] = [
    "new_allow",
    "static_mut",
    "test_disabled",
    "test_deleted",
    "unsafe_outside_ffi",
    "frozen_modified",
    "judge_and_judged",
    "todo_remaining",
    "tap_remaining",
];

static NEW_ALLOW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#!?\[\s*(allow|expect)\s*\(|cfg_attr\([^)]*\ballow\s*\(").unwrap());
static STATIC_MUT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bstatic\s+mut\b").unwrap());
static TEST_DISABLED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#\[\s*ignore\b|\bDISABLED_\w+|\bGTEST_SKIP\s*\(").unwrap());
static UNSAFE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bunsafe\b").unwrap());
static TODO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(todo|unimplemented)!\s*\(").unwrap());
static TAP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)CPP2RUST_TAP|cpp2rust[-_]tap|CPP2RUST_TRACE|cpp2rust[-_]shadow").unwrap());
static TEST_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(^|/)(tests?|testdata)/|(_test|_tests|Test|_unittest)\.[a-z+]+$|(^|/)test_[^/]+$").unwrap());
static COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"//.*$").unwrap());

fn git(args: &[&str], cwd: &Path) -> R<String> {
    let out = Command::new("git")
        .args(["-c", "core.quotepath=off"])
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| bad_input(format!("policy-scan: cannot run git: {e}")))?;
    if !out.status.success() {
        return Err(bad_input(format!(
            "policy-scan: git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn strip_comment(line: &str) -> String {
    COMMENT.replace(line, "").into_owned()
}

/// Status letter per changed path, and the added lines per path.
type Change = (BTreeMap<String, char>, BTreeMap<String, Vec<String>>);

/// ({path: status}, {path: [added lines]}) against the merge-base, including untracked files.
fn change(base: &str, cwd: &Path) -> R<Change> {
    let mb = git(&["merge-base", base, "HEAD"], cwd)?.trim().to_string();
    let mut status = BTreeMap::new();
    let mut added: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in git(&["diff", "--name-status", "--no-renames", &mb], cwd)?.lines() {
        if let Some((st, path)) = line.split_once('\t') {
            status.insert(path.to_string(), st.chars().next().unwrap_or('M'));
        }
    }
    let mut cur: Option<String> = None;
    for line in git(&["diff", "--unified=0", "--no-color", "--no-renames", &mb], cwd)?.lines() {
        if let Some(rest) = line.strip_prefix("+++ ") {
            // git ends the name with a tab when it holds a space (for GNU patch)
            cur = rest.strip_prefix("b/").map(|p| p.trim_end_matches('\t').to_string());
        } else if line.starts_with("+++") {
            continue;
        } else if let (Some(body), Some(c)) = (line.strip_prefix('+'), &cur) {
            added.entry(c.clone()).or_default().push(body.to_string());
        }
    }
    for path in git(&["ls-files", "--others", "--exclude-standard"], cwd)?.lines() {
        status.insert(path.to_string(), 'A');
        if let Ok(t) = read_text(cwd.join(path)) {
            added.insert(path.to_string(), splitlines(&t, false));
        }
    }
    Ok((status, added))
}

/// Whether a modified frozen file only gained lines at its end: its content at the
/// merge-base is an unchanged prefix of the work tree's (line endings normalized). Recorded
/// expectations are append-only, so this is the one modification that is not a rebaseline.
fn appended_only(base: &str, path: &str, cwd: &Path) -> R<bool> {
    let mb = git(&["merge-base", base, "HEAD"], cwd)?.trim().to_string();
    let old = git(&["show", &format!("{mb}:{path}")], cwd)?.replace("\r\n", "\n");
    let Ok(now) = read_text(cwd.join(path)) else {
        return Ok(false);
    };
    let now = now.replace("\r\n", "\n");
    let Some(rest) = now.strip_prefix(old.as_str()) else {
        return Ok(false);
    };
    // A last line without its newline may only be completed, not extended.
    Ok(old.is_empty() || old.ends_with('\n') || rest.is_empty() || rest.starts_with('\n'))
}

fn walk(paths: &[String], cwd: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for p in paths {
        let full = cwd.join(p);
        if full.is_file() {
            out.push(if cfg!(windows) { p.replace('\\', "/") } else { p.clone() });
        } else if full.is_dir() {
            out.extend(walk_files(&full, cwd));
        }
    }
    out
}

pub fn scan(base: &str, policy: &Value, cwd: &Path, forbid_todo: &[String], no_tap: &[String]) -> R<Value> {
    let (status, added) = change(base, cwd)?;
    let globs = |k: &str| str_list(get(policy, k));
    let mut f: Vec<Value> = Vec::new();
    let mut add = |cls: &str, path: &str, detail: &str| f.push(json!({"class": cls, "path": path, "detail": truncate_chars(detail, 160)}));
    for (path, lines) in &added {
        // The vendored tools crate is a judge file reviewed as a whole; its help text, test fixtures
        // and Windows process code are not product code and would trip every product-code rule.
        if path.starts_with("migration/tools/") {
            continue;
        }
        for l in lines {
            let code = strip_comment(l);
            if path.ends_with(".rs") {
                if NEW_ALLOW.is_match(&code) {
                    add("new_allow", path, l.trim());
                }
                if STATIC_MUT.is_match(&code) {
                    add("static_mut", path, l.trim());
                }
                if UNSAFE.is_match(&code) && !match_any_case(path, &globs("unsafe_allowed")) {
                    add("unsafe_outside_ffi", path, l.trim());
                }
            }
            if TEST_DISABLED.is_match(&code) {
                add("test_disabled", path, l.trim());
            }
        }
    }
    for (path, st) in &status {
        if *st == 'D' && TEST_FILE.is_match(path) {
            add("test_deleted", path, "");
        }
        let frozen = match_any_case(path, &globs("frozen")) && !match_any_case(path, &globs("approved_rebaselines"));
        if frozen && (*st == 'D' || (*st == 'M' && !appended_only(base, path, cwd)?)) {
            add(
                "frozen_modified",
                path,
                "recorded expectations are append-only; a rebaseline needs an approved plan entry",
            );
        }
    }
    let judge: Vec<&String> = status
        .keys()
        .filter(|p| match_any_case(p, &globs("protected")) || match_any_case(p, &globs("frozen")))
        .collect();
    let implementation: Vec<&String> = status
        .keys()
        .filter(|p| match_any_case(p, &globs("implementation")) && !judge.contains(p))
        .collect();
    if let (Some(j), Some(i)) = (judge.first(), implementation.first()) {
        add(
            "judge_and_judged",
            j,
            &format!("also changes implementation, e.g. {i}; split into two changes"),
        );
    }
    for p in walk(forbid_todo, cwd) {
        if p.ends_with(".rs") {
            for (i, l) in read_text(cwd.join(&p)).unwrap_or_default().lines().enumerate() {
                if TODO.is_match(&strip_comment(l)) {
                    add("todo_remaining", &p, &format!("line {}", i + 1));
                }
            }
        }
    }
    for p in walk(no_tap, cwd) {
        if let Some(i) = read_text(cwd.join(&p)).unwrap_or_default().lines().position(|l| TAP.is_match(l)) {
            add("tap_remaining", &p, &format!("line {}", i + 1));
        }
    }
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for x in &f {
        *counts.entry(py_str(&x["class"])).or_default() += 1;
    }
    let result = if f.is_empty() { "pass" } else { "fail" };
    Ok(json!({"schema": "cpp-rust-policy/1", "base": base, "files_changed": status.len(),
              "judge_files": judge.iter().take(50).collect::<Vec<_>>(), "findings": f, "counts": counts, "result": result}))
}

pub fn main(argv: &[String]) -> R<i32> {
    let spec = [
        ("--base", Kind::Value),
        ("--plan", Kind::Value),
        ("--forbid-todo", Kind::Multi),
        ("--no-tap", Kind::Multi),
        ("--out", Kind::Value),
    ];
    let a = Args::parse(argv, &spec)?;
    let Some(base) = a.get("--base") else {
        return usage("--base is required");
    };
    let policy = match a.get("--plan") {
        Some(p) => get(&load_yaml_file(p)?, "policy").cloned().unwrap_or(json!({})),
        None => json!({}),
    };
    let res = scan(base, &policy, Path::new("."), &a.all("--forbid-todo"), &a.all("--no-tap"))?;
    let findings = res["findings"].as_array().unwrap();
    for x in findings {
        println!(
            "FINDING {:<20} {}  {}",
            py_str(&x["class"]),
            py_str(&x["path"]),
            py_str(&x["detail"])
        );
    }
    println!(
        "policy-scan: {} ({} files changed, {} findings)",
        py_str(&res["result"]).to_uppercase(),
        res["files_changed"],
        findings.len()
    );
    if let Some(out) = a.get("--out") {
        write_json(out, &res)?;
    }
    Ok(if findings.is_empty() { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;
    use std::collections::BTreeSet;

    fn sh(d: &Path, args: &[&str]) {
        let st = Command::new(args[0]).args(&args[1..]).current_dir(d).output().unwrap();
        assert!(st.status.success(), "{args:?}: {}", String::from_utf8_lossy(&st.stderr));
    }

    fn repo(tag: &str) -> TempDir {
        let t = TempDir::new(tag);
        let d = t.path();
        sh(d, &["git", "init", "-q"]);
        sh(d, &["git", "checkout", "-q", "-b", "main"]);
        sh(d, &["git", "config", "user.email", "t@example.invalid"]);
        sh(d, &["git", "config", "user.name", "t"]);
        sh(d, &["git", "config", "commit.gpgsign", "false"]);
        t
    }

    #[test]
    fn clean_port_passes_and_gaming_is_found() {
        let t = repo("policy");
        let d = t.path();
        t.write("rust/core/src/lib.rs", "pub fn f() -> i32 { todo!() }\n");
        t.write("rust/core/src/ffi.rs", "// bridge\n");
        t.write("migration/corpus/core/seam/a.trace.jsonl", "{}\n");
        t.write("migration/corpus/core/seam/c.trace.jsonl", "{\"case\": 1}\n");
        t.write("tests/parse_test.cc", "TEST(P, A) {}\n");
        t.write("migration/validation-plan.yaml", "x: 1\n");
        sh(d, &["git", "add", "-A"]);
        sh(d, &["git", "commit", "-qm", "base"]);
        sh(d, &["git", "branch", "base"]);
        let mut pol = json!({"protected": ["migration/validation-plan.yaml", "migration/harness/*", "migration/tools/*"],
                             "frozen": ["migration/corpus/*"], "implementation": ["rust/*/src/*"],
                             "unsafe_allowed": ["rust/*/src/ffi.rs"], "approved_rebaselines": []});
        let todo = vec!["rust/core/src".to_string()];
        // a clean port: body written, unsafe only in ffi
        t.write("rust/core/src/lib.rs", "pub fn f() -> i32 { 1 } // no unsafe here\n");
        t.write("rust/core/src/ffi.rs", "pub unsafe fn g() {}\n");
        let r = scan("base", &pol, d, &todo, &todo).unwrap();
        assert_eq!(r["result"], "pass", "{r}");
        // gaming attempts
        t.write("rust/core/src/lib.rs", "#[allow(clippy::all)]\npub fn f() -> i32 { unsafe { X } }\nstatic mut X: i32 = 0;\npub fn h() { unimplemented!() }\n#[cfg(feature = \"cpp2rust-tap\")] mod tap;\n");
        t.write("migration/corpus/core/seam/a.trace.jsonl", "{\"edited\": true}\n");
        t.write("migration/corpus/core/seam/b.trace.jsonl", "{}\n"); // adding is fine
        t.write("migration/corpus/core/seam/c.trace.jsonl", "{\"case\": 1}\r\n{\"case\": 2}\n"); // appending is fine
        sh(d, &["git", "rm", "-q", "tests/parse_test.cc"]);
        t.write("tests/other_test.cc", "TEST(P, DISABLED_B) {}\n");
        let r = scan("base", &pol, d, &todo, &todo).unwrap();
        let got: BTreeSet<String> = r["counts"].as_object().unwrap().keys().cloned().collect();
        let want: BTreeSet<String> = CLASSES.iter().map(|s| s.to_string()).collect();
        assert_eq!(got, want);
        assert_eq!(r["counts"]["frozen_modified"], 1);
        pol["approved_rebaselines"] = json!(["migration/corpus/core/seam/a.trace.jsonl"]);
        assert!(scan("base", &pol, d, &[], &[]).unwrap()["counts"].get("frozen_modified").is_none());
        assert!(scan("no-such-branch", &pol, d, &[], &[]).is_err());
    }

    #[test]
    fn the_vendored_tools_crate_is_not_scanned_as_product_code() {
        let t = repo("policy-tools");
        let d = t.path();
        t.write("README.md", "x\n");
        sh(d, &["git", "add", "-A"]);
        sh(d, &["git", "commit", "-qm", "base"]);
        let pol = json!({"protected": ["migration/tools/*"], "implementation": ["rust/*/src/*"]});
        // installing the tools: help text, fixtures and Windows process code are not product code
        t.write(
            "migration/tools/cpp2rust-tools/src/util.rs",
            "#[allow(dead_code)]\nfn k() { unsafe { x() } }\nstatic mut N: i32 = 0;\nconst H: &str = \"todo!()\";\n",
        );
        let r = scan("HEAD", &pol, d, &[], &[]).unwrap();
        assert_eq!(r["result"], "pass", "{r}");
        // the same lines in product code are still found
        t.write("rust/core/src/lib.rs", "#[allow(dead_code)]\npub fn f() {}\n");
        let r = scan("HEAD", &pol, d, &[], &[]).unwrap();
        assert_eq!(r["counts"]["new_allow"], 1, "{r}");
    }
    #[test]
    fn paths_with_spaces_and_letter_case() {
        let t = repo("policy-paths");
        let d = t.path();
        t.write("rust/core/src/with space.rs", "pub fn f() {}\n");
        t.write("rust/core/src/FFI.rs", "// bridge\n");
        sh(d, &["git", "add", "-A"]);
        sh(d, &["git", "commit", "-qm", "base"]);
        t.write("rust/core/src/with space.rs", "#[allow(dead_code)]\npub fn f() {}\n");
        t.write("rust/core/src/FFI.rs", "pub unsafe fn g() {}\n");
        let pol = json!({"unsafe_allowed": ["rust/*/src/ffi.rs"]});
        let r = scan("HEAD", &pol, d, &[], &[]).unwrap();
        let paths: BTreeSet<(String, String)> = r["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| (py_str(&f["class"]), py_str(&f["path"])))
            .collect();
        let want: BTreeSet<(String, String)> = [
            ("new_allow", "rust/core/src/with space.rs"),
            // git paths are case-sensitive: the glob names ffi.rs, not FFI.rs (as on Linux CI)
            ("unsafe_outside_ffi", "rust/core/src/FFI.rs"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        assert_eq!(paths, want);
    }
}
