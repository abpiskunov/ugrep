//! gate-check: run a gate's checks from the accepted plan and compare results to its thresholds.

use crate::util::*;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const HELP: &str = "\
cpp2rust-tools gate-check - run a gate's checks from the accepted plan and compare results to the
plan's thresholds. The same command is run by the agent, by Validating and by CI on the commit.

  gate-check --gate unit --target core_parse.header [--plan migration/validation-plan.yaml]
             [--out migration/out] [--base REF] [--dry-run]
  gate-check --changed origin/master   # every gate whose unit record, catalog or corpus changed

A gate in the plan:
  gates:
    unit:
      scope: unit                  # unit | component
      checks:
        - name: rust-tests
          run: \"cargo nextest run -p {rust_crate} --profile ci\"     # exit code must be 0
          timeout: 1800
          expect:
            - {junit: \"target/nextest/ci/junit.xml\", max_failures: 0, min_tests: 1}
            - {json: \"{out}/seam.json\", field: divergent_cases, op: \"==\", value: 0}
Placeholders: {out} {target} {unit} {component} {migration} {tools} {base} and every scalar field
of the unit record (migration/units/<unit>.yaml) and of the plan's components.<component> entry.
{tools} is this program (so a check runs `{tools} trace-diff ...` without another build).
{base} is the ref the change is judged against: --base, else the --changed base, else
CPP2RUST_BASE, else the remote's default branch (origin/HEAD, then origin/main, origin/master).
Write a literal brace in `run` as {{ or }}. Commands run in `sh -c` (Windows: `cmd /d /s /c`).
Result: <out>/gates/<gate>/<target>/report.json and one log per check. A check never passes by
being skipped: a missing command, a missing result file or an empty test run fails.
Exit: 0 pass, 1 fail or blocked, 2 configuration error.";

/// Placeholder values for `{name}` in `run` and in expectation paths.
pub type Fmt = BTreeMap<String, String>;

pub fn format(template: &str, fmt: &Fmt) -> R<String> {
    let mut out = String::new();
    let mut it = template.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '{' if it.peek() == Some(&'{') => {
                it.next();
                out.push('{');
            }
            '}' if it.peek() == Some(&'}') => {
                it.next();
                out.push('}');
            }
            '{' => {
                let name: String = it.by_ref().take_while(|c| *c != '}').collect();
                match fmt.get(&name) {
                    Some(v) => out.push_str(v),
                    None if name == "base" => {
                        return Err(bad_input(
                            "gate-check: {base} has no value: no origin/HEAD, origin/main or origin/master; \
                             pass --base REF or set CPP2RUST_BASE",
                        ));
                    }
                    None => return Err(bad_input(format!("gate-check: unknown placeholder {{{name}}}"))),
                }
            }
            '}' => return Err(bad_input("gate-check: single '}' in a command; write }} for a literal brace")),
            c => out.push(c),
        }
    }
    Ok(out)
}

fn scalars(v: &Value, fmt: &mut Fmt) {
    if let Value::Object(o) = v {
        for (k, x) in o {
            match x {
                Value::String(s) => {
                    fmt.insert(k.clone(), s.clone());
                }
                Value::Number(n) => {
                    fmt.insert(k.clone(), n.to_string());
                }
                Value::Bool(b) => {
                    fmt.insert(k.clone(), py_repr(&json!(b)));
                }
                _ => {}
            }
        }
    }
}

pub struct Junit {
    pub tests: u64,
    pub failures: u64,
    pub errors: u64,
    pub skipped: u64,
    pub ids: HashSet<String>,
    pub failed: HashSet<String>,
}

pub fn junit(path: &str) -> Result<Junit, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("OSError: {e}"))?;
    let doc = roxmltree::Document::parse(strip_bom(&text)).map_err(|e| format!("ParseError: {e}"))?;
    let root = doc.root_element();
    let suites: Vec<roxmltree::Node> = if root.has_tag_name("testsuite") {
        vec![root]
    } else {
        root.descendants().filter(|n| n.has_tag_name("testsuite")).collect()
    };
    let mut j = Junit {
        tests: 0,
        failures: 0,
        errors: 0,
        skipped: 0,
        ids: HashSet::new(),
        failed: HashSet::new(),
    };
    for s in suites {
        for c in s.children().filter(|n| n.has_tag_name("testcase")) {
            let tid = match c.attribute("classname").filter(|s| !s.is_empty()) {
                Some(cl) => format!("{cl}.{}", c.attribute("name").unwrap_or("")),
                None => c.attribute("name").unwrap_or("").to_string(),
            };
            j.ids.insert(tid.clone());
            j.tests += 1;
            let child = |t: &str| c.children().any(|n| n.has_tag_name(t));
            if child("failure") {
                j.failures += 1;
                j.failed.insert(tid);
            } else if child("error") {
                j.errors += 1;
                j.failed.insert(tid);
            } else if child("skipped") || matches!(c.attribute("status"), Some("notrun" | "disabled")) {
                j.skipped += 1;
            }
        }
    }
    Ok(j)
}

fn dig<'a>(doc: &'a Value, field: &str) -> Result<&'a Value, String> {
    let mut cur = doc;
    for part in field.split('.') {
        cur = match cur {
            Value::Array(a) => {
                let i: i64 = part.parse().map_err(|_| format!("ValueError: invalid index {part:?}"))?;
                let idx = if i < 0 { a.len() as i64 + i } else { i };
                a.get(usize::try_from(idx).map_err(|_| "IndexError: list index out of range".to_string())?)
                    .ok_or("IndexError: list index out of range")?
            }
            Value::Object(o) => o.get(part).ok_or_else(|| format!("KeyError: {}", py_repr(&json!(part))))?,
            other => return Err(format!("TypeError: cannot index {} with {part:?}", py_repr(other))),
        };
    }
    Ok(cur)
}

/// Python-like comparison: numbers (and booleans) numerically, strings as strings.
fn compare(op: &str, a: &Value, b: &Value) -> Result<bool, String> {
    let num = |v: &Value| match v {
        Value::Bool(x) => Some(if *x { 1.0 } else { 0.0 }),
        Value::Number(n) => n.as_f64(),
        _ => None,
    };
    let ord = match (num(a), num(b), a, b) {
        (Some(x), Some(y), _, _) => x.partial_cmp(&y),
        (_, _, Value::String(x), Value::String(y)) => Some(x.cmp(y)),
        _ => None,
    };
    use std::cmp::Ordering::*;
    Ok(match (op, ord) {
        ("==", Some(o)) => o == Equal,
        ("!=", Some(o)) => o != Equal,
        ("==", None) => a == b,
        ("!=", None) => a != b,
        ("<=", Some(o)) => o != Greater,
        (">=", Some(o)) => o != Less,
        ("<", Some(o)) => o == Less,
        (">", Some(o)) => o == Greater,
        (op, None) if ["<=", ">=", "<", ">"].contains(&op) => {
            return Err(format!("TypeError: '{op}' not supported between {} and {}", py_repr(a), py_repr(b)));
        }
        (op, _) => return Err(format!("KeyError: {}", py_repr(&json!(op)))),
    })
}

fn num_or(x: &Value, k: &str, default: u64) -> u64 {
    x.get(k).and_then(Value::as_u64).unwrap_or(default)
}

/// Evaluate one expectation. Returns (ok, message).
pub fn check_expect(x: &Value, fmt: &Fmt) -> R<(bool, String)> {
    let path_of = |k: &str| -> R<Option<String>> { x.get(k).map(|p| format(&py_str(p), fmt)).transpose() };
    let inner = || -> R<Result<(bool, String), String>> {
        if let Some(path) = path_of("junit")? {
            let j = match junit(&path) {
                Ok(j) => j,
                Err(e) => return Ok(Err(e)),
            };
            let bad = j.failures + j.errors;
            let mut msgs = vec![format!("{path}: {} tests, {bad} failed, {} skipped", j.tests, j.skipped)];
            let mut ok =
                bad <= num_or(x, "max_failures", 0) && j.tests >= num_or(x, "min_tests", 1) && j.skipped <= num_or(x, "max_skipped", 0);
            if let Some(ids) = path_of("must_include")? {
                // every baseline test id still exists and passes
                let want = match read_text(&ids) {
                    Ok(t) => t,
                    Err(e) => return Ok(Err(format!("OSError: {e}"))),
                };
                let gone: Vec<Value> = want
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && (!j.ids.contains(*l) || j.failed.contains(*l)))
                    .map(|l| json!(l))
                    .collect();
                if !gone.is_empty() {
                    ok = false;
                    msgs.push(format!(
                        "{} baseline test ids missing or failing, e.g. {}",
                        gone.len(),
                        py_repr(&Value::Array(gone[..gone.len().min(3)].to_vec()))
                    ));
                }
            }
            return Ok(Ok((ok, msgs.join("; "))));
        }
        if let Some(path) = path_of("json")? {
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => return Ok(Err(format!("OSError: {e}"))),
            };
            let doc: Value = match serde_json::from_str(strip_bom(&text)) {
                Ok(d) => d,
                Err(e) => return Ok(Err(format!("ValueError: {e}"))),
            };
            let field = x.get("field").map(py_str).unwrap_or_default();
            let val = match dig(&doc, &field) {
                Ok(v) => v,
                Err(e) => return Ok(Err(e)),
            };
            let op = x.get("op").and_then(Value::as_str).unwrap_or("==");
            let want = x.get("value").cloned().unwrap_or(Value::Null);
            let ok = match compare(op, val, &want) {
                Ok(b) => b,
                Err(e) => return Ok(Err(e)),
            };
            return Ok(Ok((
                ok,
                format!("{path}: {field} = {} (need {op} {})", py_repr(val), py_repr(&want)),
            )));
        }
        if let Some(path) = path_of("exists")? {
            let ok = std::fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false);
            return Ok(Ok((ok, format!("{path} exists and is not empty"))));
        }
        Ok(Ok((false, format!("unknown expectation {}", py_repr(x)))))
    };
    Ok(match inner()? {
        Ok(r) => r,
        Err(e) => (false, format!("cannot evaluate {}: {e}", py_repr(x))),
    })
}

pub struct Ctx {
    pub out_root: PathBuf,
    pub mig: PathBuf,
    pub cwd: PathBuf,
    pub tools: String,
    pub base: Option<String>,
    pub dry: bool,
}

/// The ref `{base}` names: `explicit` (`--base`, or the `--changed` base), else `CPP2RUST_BASE`,
/// else the remote's default branch. Plans must not hard-code `origin/main`: a repository's
/// default branch may be `master`, and a sandbox clone has `origin/<default>` but no `origin/HEAD`.
pub fn resolve_base(explicit: Option<&str>, cwd: &Path) -> Option<String> {
    let env = std::env::var("CPP2RUST_BASE").ok();
    if let Some(b) = explicit.or(env.as_deref()).filter(|b| !b.is_empty()) {
        return Some(b.to_string());
    }
    let head = git_quiet(&["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"], cwd);
    if !head.is_empty() {
        return Some(head);
    }
    ["origin/main", "origin/master"]
        .into_iter()
        .find(|r| !git_quiet(&["rev-parse", "--verify", "--quiet", &format!("{r}^{{commit}}")], cwd).is_empty())
        .map(str::to_string)
}

fn blocked_code(rc: i64) -> bool {
    // sh: 126 not executable, 127 not found; cmd.exe: 9009 not recognized
    rc == 126 || rc == 127 || (cfg!(windows) && rc == 9009)
}

pub fn run_gate(plan: &Value, gate: &str, target: &str, ctx: &Ctx) -> R<Value> {
    let g = get(plan, "gates")
        .and_then(|gs| get(gs, gate))
        .ok_or_else(|| bad_input(format!("gate-check: plan has no gate '{gate}'")))?;
    let mut fmt = Fmt::new();
    let comp = if get(g, "scope").and_then(Value::as_str).unwrap_or("unit") == "unit" {
        let rec_path = ctx.mig.join("units").join(format!("{target}.yaml"));
        let rec = load_yaml_file(&rec_path.to_string_lossy())?;
        scalars(&rec, &mut fmt);
        fmt.insert("unit".into(), target.into());
        fmt.insert("functions".into(), str_list(get(&rec, "functions")).join(","));
        rec.get("component").map(py_str).unwrap_or_default()
    } else {
        target.to_string()
    };
    let cdef = get(plan, "components")
        .and_then(|c| c.get(&comp))
        .ok_or_else(|| bad_input(format!("gate-check: component '{comp}' is not in the plan")))?;
    scalars(cdef, &mut fmt);
    let out = ctx.out_root.join("gates").join(gate).join(target.replace('/', "_"));
    if !ctx.dry {
        let _ = std::fs::remove_dir_all(&out); // no result file of an earlier run may be read
    }
    std::fs::create_dir_all(&out).map_err(|e| bad_input(format!("gate-check: {}: {e}", out.display())))?;
    let s = |p: &Path| p.to_string_lossy().into_owned();
    fmt.insert("component".into(), comp.clone());
    fmt.insert("target".into(), target.into());
    fmt.insert("out".into(), s(&out));
    fmt.insert("migration".into(), s(&ctx.mig));
    fmt.insert("tools".into(), ctx.tools.clone());
    if let Some(base) = &ctx.base {
        fmt.insert("base".into(), base.clone());
    }
    let checks = arr(g.get("checks"));
    if checks.is_empty() {
        return Err(bad_input(format!("gate-check: gate '{gate}' has no checks")));
    }
    let mut report = Map::new();
    report.insert("schema".into(), json!("cpp-rust-gate/1"));
    report.insert("gate".into(), json!(gate));
    report.insert("target".into(), json!(target));
    report.insert("component".into(), json!(comp));
    report.insert("commit".into(), json!(git_quiet(&["rev-parse", "HEAD"], &ctx.cwd)));
    report.insert(
        "dirty".into(),
        json!(!git_quiet(&["status", "--porcelain", "-uno"], &ctx.cwd).is_empty()),
    );
    report.insert("started".into(), json!(utc_now()));
    let mut results = Vec::new();
    for c in checks {
        let name = c.get("name").map(py_str).unwrap_or_default();
        let cmd = format(&c.get("run").map(py_str).unwrap_or_default(), &fmt)?;
        let mut res = json!({"name": name, "run": cmd, "status": "pass", "details": []});
        if ctx.dry {
            res["status"] = json!("dry-run");
            println!("DRY  {name:<22} {cmd}");
            results.push(res);
            continue;
        }
        let log = out.join(format!("{name}.log"));
        let t0 = Instant::now();
        let timeout = c.get("timeout").and_then(Value::as_f64).unwrap_or(3600.0);
        let rc = run_logged(&cmd, &ctx.cwd, &log, Duration::from_secs_f64(timeout.max(0.0)))
            .map_err(|e| bad_input(format!("gate-check: cannot run check {name}: {e}")))?;
        let mut details: Vec<String> = Vec::new();
        res["exit"] = rc.map_or(json!("timeout"), |c| json!(c));
        res["seconds"] = json!((t0.elapsed().as_secs_f64() * 10.0).round() / 10.0);
        res["log"] = json!(s(&log));
        if rc != Some(0) {
            let blocked = rc.is_some_and(blocked_code);
            res["status"] = json!(if blocked { "blocked" } else { "fail" });
            let shown = rc.map_or("timeout".to_string(), |c| c.to_string());
            details.push(format!(
                "command exit {shown}{}",
                if blocked {
                    " (command not found: environment gap, not a pass)"
                } else {
                    ""
                }
            ));
        }
        for x in arr(c.get("expect")) {
            let (ok, msg) = check_expect(&x, &fmt)?;
            details.push(format!("{}{msg}", if ok { "ok: " } else { "FAIL: " }));
            if !ok && res["status"] == "pass" {
                res["status"] = json!("fail");
            }
        }
        println!(
            "{:<5} {name:<22} {}",
            py_str(&res["status"]).to_uppercase(),
            truncate_chars(&details.join("; "), 300)
        );
        res["details"] = json!(details);
        results.push(res);
    }
    let statuses: HashSet<String> = results.iter().map(|r| py_str(&r["status"])).collect();
    let result = if ctx.dry {
        "dry-run"
    } else if statuses.len() == 1 && statuses.contains("pass") {
        "pass"
    } else if statuses.contains("blocked") && !statuses.contains("fail") {
        "blocked"
    } else {
        "fail"
    };
    report.insert("checks".into(), Value::Array(results));
    report.insert("result".into(), json!(result));
    let report = Value::Object(report);
    let rp = out.join("report.json");
    write_json(&s(&rp), &report)?;
    println!("GATE {gate} {target}: {}  ({})", result.to_uppercase(), rp.display());
    Ok(report)
}

/// Gates due for a change: unit record -> unit or switch; a component's `switch_files` ->
/// switch; catalog, corpus or masks -> the behavior gate of the component's stage; C++
/// sources or headers -> baseline.
///
/// The behavior gate follows the component's stage, read from what the change touches,
/// because every stage changes the catalog or corpus and only the last can pass
/// `behavior-ready`:
/// - no catalog yet (Foundation recording the cases that exist today): no behavior gate;
/// - the approvals record changed, `decisions.md` or the approved masks `masks/<c>.json`
///   (Behavior approvals): `behavior-ready`;
/// - otherwise (a Behavior task: catalog, corpus, harness, proposed masks): `behavior-proposed`.
pub fn changed_targets(base: &str, plan: &Value, mig: &Path, cwd: &Path) -> R<Vec<(String, String)>> {
    let names: Vec<String> = git_quiet(&["diff", "--name-only", &format!("{base}...HEAD")], cwd)
        .lines()
        .map(String::from)
        .collect();
    let rel = rel_slash(&norm(mig), &norm(cwd));
    let comps = get(plan, "components").cloned().unwrap_or(json!({}));
    let mut todo: Vec<(String, String)> = Vec::new();
    for n in &names {
        if n.starts_with(&format!("{rel}/units/")) && n.ends_with(".yaml") && cwd.join(n).exists() {
            let rec = load_yaml_file(&cwd.join(n).to_string_lossy())?;
            let unit = Path::new(n)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            match rec.get("status").and_then(Value::as_str) {
                Some("ported") => todo.push(("unit".into(), unit)),
                Some("switched" | "cleaned") => todo.push(("switch".into(), rec.get("component").map(py_str).unwrap_or_default())),
                _ => {}
            }
        }
        for kind in ["behavior", "corpus", "masks"] {
            if let Some(rest) = n.strip_prefix(&format!("{rel}/{kind}/")) {
                let first = rest.split('/').next().unwrap_or("");
                let comp = first.rsplit_once('.').map_or(first, |(a, _)| a);
                if comps.get(comp).is_some() {
                    if let Some(gate) = behavior_gate(&norm(mig), comp, &names, &rel) {
                        todo.push((gate.into(), comp.into()));
                    }
                }
            }
        }
    }
    if let Value::Object(cs) = &comps {
        for (comp, cdef) in cs {
            let globs: Vec<String> = ["cpp_sources", "cpp_headers"].iter().flat_map(|k| str_list(get(cdef, k))).collect();
            if names.iter().any(|n| match_any_case(n, &globs)) {
                todo.push(("baseline".into(), comp.clone()));
            }
            // The file that holds the build switch decides what ships. A change to it re-runs
            // the switch gate of every component that ships as Rust: one with a `switched` or
            // `cleaned` unit, or whose default the file now sets to rust (`switch_rust`, a regex
            // over the file) even if Switch over forgot its unit records. A component still at
            // cpp is the baseline's business, however often the shared file names it.
            let switch_files = str_list(get(cdef, "switch_files"));
            let touched: Vec<&String> = names.iter().filter(|n| match_any_case(n, &switch_files)).collect();
            if !touched.is_empty() {
                let switched_unit = component_has_switched_unit(&cwd.join(&rel).join("units"), comp)?;
                let defaults_to_rust = match get(cdef, "switch_rust").and_then(Value::as_str) {
                    Some(pattern) => {
                        let re = Regex::new(pattern).map_err(|e| bad_input(format!("gate-check: {comp}.switch_rust: {e}")))?;
                        touched
                            .iter()
                            .any(|n| std::fs::read_to_string(cwd.join(n)).is_ok_and(|t| re.is_match(&t)))
                    }
                    None => false,
                };
                if switched_unit || defaults_to_rust {
                    todo.push(("switch".into(), comp.clone()));
                }
            }
        }
    }
    let gates = get(plan, "gates").cloned().unwrap_or(json!({}));
    let mut seen = HashSet::new();
    Ok(todo
        .into_iter()
        .filter(|t| gates.get(&t.0).is_some() && seen.insert(t.clone()))
        .collect())
}

/// Whether a unit record of `comp` in `units` is `switched` or `cleaned`.
fn component_has_switched_unit(units: &Path, comp: &str) -> R<bool> {
    let Ok(entries) = std::fs::read_dir(units) else {
        return Ok(false);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        let rec = load_yaml_file(&path.to_string_lossy())?;
        if rec.get("component").map(py_str).as_deref() == Some(comp)
            && matches!(rec.get("status").and_then(Value::as_str), Some("switched" | "cleaned"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The behavior gate a change to `comp`'s catalog, corpus or masks is held to (see
/// [`changed_targets`]), or `None` before the component has a catalog. `changed` are the
/// changed paths, relative to the repository; `rel` is the migration folder among them.
fn behavior_gate(mig: &Path, comp: &str, changed: &[String], rel: &str) -> Option<&'static str> {
    if !mig.join("behavior").join(format!("{comp}.yaml")).exists() {
        return None;
    }
    let approvals = [format!("{rel}/decisions.md"), format!("{rel}/masks/{comp}.json")];
    Some(if changed.iter().any(|n| approvals.contains(n)) {
        "behavior-ready"
    } else {
        "behavior-proposed"
    })
}

pub fn main(argv: &[String]) -> R<i32> {
    let spec = [
        ("--plan", Kind::Value),
        ("--gate", Kind::Value),
        ("--target", Kind::Value),
        ("--changed", Kind::Value),
        ("--out", Kind::Value),
        ("--base", Kind::Value),
        ("--dry-run", Kind::Flag),
    ];
    let a = Args::parse(argv, &spec)?;
    let plan_path = a.get("--plan").unwrap_or("migration/validation-plan.yaml");
    let plan = load_yaml_file(plan_path)?;
    let mig = norm(plan_path).parent().map(Path::to_path_buf).unwrap_or_default();
    let tools = std::env::current_exe()
        .map(|p| shell_quote(&p.to_string_lossy()))
        .unwrap_or_else(|_| "cpp2rust-tools".into());
    let ctx = Ctx {
        out_root: PathBuf::from(a.get("--out").unwrap_or("migration/out")),
        mig: mig.clone(),
        cwd: PathBuf::from("."),
        tools,
        base: resolve_base(a.get("--base").or(a.get("--changed")), Path::new(".")),
        dry: a.flag("--dry-run"),
    };
    let todo = if let Some(base) = a.get("--changed") {
        let t = changed_targets(base, &plan, &mig, Path::new("."))?;
        if t.is_empty() {
            println!("gate-check: no gate is due for this change");
            return Ok(0);
        }
        t
    } else if let (Some(g), Some(t)) = (a.get("--gate"), a.get("--target")) {
        vec![(g.to_string(), t.to_string())]
    } else {
        return usage("give --gate and --target, or --changed BASE");
    };
    let mut all_ok = true;
    for (g, t) in todo {
        let r = run_gate(&plan, &g, &t, &ctx)?;
        all_ok &= matches!(r["result"].as_str(), Some("pass" | "dry-run"));
    }
    Ok(if all_ok { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn behavior_gate_follows_the_component_stage() {
        let d = TempDir::new("bgate");
        let mig = d.path();
        let changed = |paths: &[&str]| paths.iter().map(|p| p.to_string()).collect::<Vec<_>>();
        // Foundation: starter cases recorded, no catalog yet.
        d.write("corpus/utf8/cases.jsonl", "{}\n");
        assert_eq!(
            behavior_gate(mig, "utf8", &changed(&["migration/corpus/utf8/cases.jsonl"]), "migration"),
            None
        );

        // Behavior: catalog, corpus and proposed masks.
        d.write("behavior/utf8.yaml", "behaviors: []\n");
        let behavior = changed(&["migration/behavior/utf8.yaml", "migration/masks/utf8.proposed.json"]);
        assert_eq!(behavior_gate(mig, "utf8", &behavior, "migration"), Some("behavior-proposed"));

        // Behavior approvals: the decisions record or the approved masks changed.
        let approvals = changed(&["migration/behavior/utf8.yaml", "migration/decisions.md"]);
        assert_eq!(behavior_gate(mig, "utf8", &approvals, "migration"), Some("behavior-ready"));
        let masks = changed(&["migration/masks/utf8.json"]);
        assert_eq!(behavior_gate(mig, "utf8", &masks, "migration"), Some("behavior-ready"));
    }
    #[test]
    fn placeholders_and_braces() {
        let mut f = Fmt::new();
        f.insert("out".into(), "o".into());
        assert_eq!(format("{out}/x {{y}}", &f).unwrap(), "o/x {y}");
        assert_eq!(format("{nope}", &f).unwrap_err().0, 2);
        assert!(format("--base {base}", &f).unwrap_err().1.contains("--base REF"));
    }

    /// The Switch over task may forget to mark its units `switched`; the build file still decides.
    #[test]
    fn a_build_switch_change_is_due_for_the_switch_gate() {
        let d = TempDir::new("gswitch");
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git").args(args).current_dir(d.path()).output().unwrap();
            assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
        };
        let commit = |m: &str| {
            git(&[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                m,
            ])
        };
        git(&["init", "-q"]);
        let props = |leaves: &str, extra: &str| format!("<LeavesImpl>{leaves}</LeavesImpl>\n<CoreImpl>cpp</CoreImpl>\n{extra}");
        let plan = json!({"components": {
                              "leaves": {"switch_files": ["msvc/rust.props"], "switch_rust": "<LeavesImpl>rust<"},
                              "core": {"switch_files": ["msvc/rust.props"], "switch_rust": "<CoreImpl>rust<"},
                              "other": {}},
                          "gates": {"switch": {}}});
        let mig = d.path().join("migration");
        d.write("msvc/rust.props", &props("cpp", ""));
        git(&["add", "-A"]);
        commit("base");
        git(&["branch", "base"]);

        // Switch over flipped the default but forgot its unit record.
        d.write("msvc/rust.props", &props("rust", ""));
        git(&["add", "-A"]);
        commit("switch");
        let leaves = vec![("switch".to_string(), "leaves".to_string())];
        assert_eq!(changed_targets("base", &plan, &mig, d.path()).unwrap(), leaves);

        // A later seam edits a line naming every component: only the one shipping as Rust is due.
        git(&["branch", "-f", "base"]);
        d.write("migration/units/glob.yaml", "unit: glob\ncomponent: leaves\nstatus: switched\n");
        d.write("msvc/rust.props", &props("rust", "<Link>LeavesImpl CoreImpl</Link>\n"));
        git(&["add", "-A"]);
        commit("seam");
        assert_eq!(
            changed_targets("base", &plan, &mig, d.path()).unwrap(),
            leaves,
            "a cpp component is the baseline's"
        );
    }

    /// A sandbox clone of a `master` repository has `origin/master` and no `origin/HEAD`.
    #[test]
    fn base_follows_the_remote_default_branch() {
        let d = TempDir::new("gbase");
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git").args(args).current_dir(d.path()).output().unwrap();
            assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
        };
        git(&["init", "-q"]);
        git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "c",
        ]);
        assert_eq!(resolve_base(Some("origin/rel"), d.path()).as_deref(), Some("origin/rel"));
        if std::env::var_os("CPP2RUST_BASE").is_none() {
            assert_eq!(resolve_base(None, d.path()), None);
            git(&["update-ref", "refs/remotes/origin/master", "HEAD"]);
            assert_eq!(resolve_base(None, d.path()).as_deref(), Some("origin/master"));
            git(&["update-ref", "refs/remotes/origin/trunk", "HEAD"]);
            git(&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/trunk"]);
            assert_eq!(resolve_base(None, d.path()).as_deref(), Some("origin/trunk"));
        }
    }

    #[test]
    fn junit_and_must_include() {
        let d = TempDir::new("junit");
        d.write(
            "f.xml",
            r#"<testsuites><testsuite><testcase name="x"><failure/></testcase><testcase name="y"/></testsuite></testsuites>"#,
        );
        d.write("ids.txt", "y\nz\n");
        let p = |n: &str| d.path().join(n).to_string_lossy().into_owned();
        let (ok, msg) = check_expect(
            &json!({"junit": p("f.xml"), "max_failures": 1, "must_include": p("ids.txt")}),
            &Fmt::new(),
        )
        .unwrap();
        assert!(!ok && msg.contains("baseline") && msg.contains("['z']"), "{msg}");
        let (ok, _) = check_expect(&json!({"junit": p("f.xml"), "max_failures": 1}), &Fmt::new()).unwrap();
        assert!(ok);
        let (ok, msg) = check_expect(&json!({"junit": p("missing.xml")}), &Fmt::new()).unwrap();
        assert!(!ok && msg.starts_with("cannot evaluate"), "{msg}");
    }

    #[test]
    fn json_fields_and_ops() {
        let d = TempDir::new("jsonexp");
        d.write("s.json", r#"{"divergent_cases": 0, "s": [{"n": 7}], "saturated": true, "p": 85.5}"#);
        let p = d.path().join("s.json").to_string_lossy().into_owned();
        let e = |field: &str, op: &str, value: Value| {
            check_expect(&json!({"json": p, "field": field, "op": op, "value": value}), &Fmt::new())
                .unwrap()
                .0
        };
        assert!(e("divergent_cases", "==", json!(0)));
        assert!(e("s.0.n", ">=", json!(5)) && !e("s.0.n", ">", json!(7)));
        assert!(e("saturated", "==", json!(true)) && e("p", ">=", json!(85)));
        assert!(!e("missing", "==", json!(0)) && !e("s.3.n", "==", json!(0)));
        assert!(!check_expect(&json!({"exists": p.clone() + ".none"}), &Fmt::new()).unwrap().0);
    }

    #[test]
    fn result_files_with_bom_and_crlf() {
        let d = TempDir::new("bomexp");
        d.write("s.json", "\u{feff}{\"divergent_cases\": 0}\r\n");
        d.write(
            "j.xml",
            "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<testsuite>\r\n<testcase name=\"x\"/>\r\n</testsuite>\r\n",
        );
        d.write("ids.txt", "x\r\n");
        let p = |n: &str| d.path().join(n).to_string_lossy().into_owned();
        let (ok, msg) = check_expect(&json!({"json": p("s.json"), "field": "divergent_cases", "value": 0}), &Fmt::new()).unwrap();
        assert!(ok, "{msg}");
        let (ok, msg) = check_expect(&json!({"junit": p("j.xml"), "must_include": p("ids.txt")}), &Fmt::new()).unwrap();
        assert!(ok, "{msg}");
    }
}
