//! run-cases: run surface cases against ONE binary and write observations as trace events.

use crate::util::*;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub const HELP: &str = "\
cpp2rust-tools run-cases - run surface cases against ONE binary and write observations as
cpp2rust-trace/1 events, so the same trace-diff compares C++ with Rust (and C++ with itself).

  run-cases --bin build/cpp/tool --impl cpp --cases migration/corpus/cli/cases.jsonl
            --out out/cli.cpp.jsonl [--env-matrix matrix.json] [--repeat 5] [--timeout 20]

A case (one JSON object per line; inputs only, never expected values):
  {\"id\": \"help\", \"args\": [\"--help\"], \"stdin\": \"\", \"env\": {\"X\": \"1\"},
   \"files\": {\"in/a.txt\": \"text\", \"in/b.bin\": {\"hex\": \"00ff\"}}, \"timeout\": 10}
Each case runs in a fresh directory with a minimal environment (HOME and TMPDIR point into it;
on Windows also USERPROFILE, APPDATA, LOCALAPPDATA, TEMP and TMP). Its path in stdout and
stderr is written as $WORK.
--env-matrix: JSON list of objects, e.g. [{\"LC_ALL\":\"C\",\"TZ\":\"UTC\"},{\"LC_ALL\":\"tr_TR.UTF-8\"}];
  every case runs once per row; the case id becomes \"<id>@<row index>\".
--repeat N: run each case N times and fail (exit 3) if the observations differ between runs
  of this one binary: such a case is not deterministic and is not admitted to the corpus.
Events per case: call(main) -> stdout -> stderr -> fs (files left behind) -> exit.
Exit: 0 recorded, 2 usage or input error, 3 a case is not deterministic.";

#[cfg(unix)]
fn signal_name(n: i32) -> String {
    let linux = [
        (1, "SIGHUP"),
        (2, "SIGINT"),
        (3, "SIGQUIT"),
        (4, "SIGILL"),
        (5, "SIGTRAP"),
        (6, "SIGABRT"),
        (7, "SIGBUS"),
        (8, "SIGFPE"),
        (9, "SIGKILL"),
        (10, "SIGUSR1"),
        (11, "SIGSEGV"),
        (12, "SIGUSR2"),
        (13, "SIGPIPE"),
        (14, "SIGALRM"),
        (15, "SIGTERM"),
        (24, "SIGXCPU"),
        (25, "SIGXFSZ"),
        (31, "SIGSYS"),
    ];
    let macos = [
        (7, "SIGEMT"),
        (10, "SIGBUS"),
        (12, "SIGSYS"),
        (24, "SIGXCPU"),
        (25, "SIGXFSZ"),
        (30, "SIGUSR1"),
        (31, "SIGUSR2"),
    ];
    if cfg!(target_os = "macos") {
        if let Some((_, s)) = macos.iter().find(|(k, _)| *k == n) {
            return s.to_string();
        }
    }
    linux
        .iter()
        .find(|(k, _)| *k == n)
        .map(|(_, s)| s.to_string())
        .unwrap_or_else(|| n.to_string())
}

fn category(c: &Captured) -> Value {
    if c.timed_out {
        return json!({"category": "timeout"});
    }
    let st = c.status.as_ref().expect("finished");
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = st.signal() {
            return json!({"category": "signal", "signal": signal_name(sig)});
        }
    }
    let code = exit_code(st);
    if code == 0 {
        json!({"category": "ok", "code": 0})
    } else {
        json!({"category": "error", "code": code})
    }
}

/// Reduced final-state view: relative path -> type, size, sha256, permission bits.
fn snapshot(root: &Path) -> Map<String, Value> {
    let mut out = Map::new();
    fn rec(d: &Path, root: &Path, out: &mut Map<String, Value>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        if entries.is_empty() && d != root {
            out.insert(rel_slash(d, root) + "/", json!({"type": "dir"}));
        }
        for e in entries {
            let p = e.path();
            let Ok(md) = std::fs::symlink_metadata(&p) else { continue };
            let rel = rel_slash(&p, root);
            if md.file_type().is_symlink() {
                let to = std::fs::read_link(&p).map(|t| t.to_string_lossy().into_owned()).unwrap_or_default();
                out.insert(rel, json!({"type": "link", "to": to}));
            } else if md.is_dir() {
                rec(&p, root, out);
            } else {
                let mut h = Sha256::default();
                if let Ok(bytes) = std::fs::read(&p) {
                    h.update(&bytes);
                }
                #[cfg(unix)]
                let mode = {
                    use std::os::unix::fs::PermissionsExt;
                    json!(format!("0o{:o}", md.permissions().mode() & 0o7777))
                };
                #[cfg(not(unix))]
                let mode = Value::Null;
                out.insert(rel, json!({"type": "file", "size": md.len(), "sha256": h.hex(), "mode": mode}));
            }
        }
    }
    rec(root, root, &mut out);
    out
}

/// Bytes as lines when valid UTF-8 (line endings kept visible), else hex.
fn text(b: &[u8]) -> Value {
    match std::str::from_utf8(b) {
        Ok(s) => json!({"lines": splitlines(s, true)}),
        Err(_) => json!({"hex": b.iter().map(|x| format!("{x:02x}")).collect::<String>()}),
    }
}

fn replace_bytes(hay: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    if needle.is_empty() {
        return hay.to_vec();
    }
    let mut out = Vec::with_capacity(hay.len());
    let mut i = 0;
    while i < hay.len() {
        if hay[i..].starts_with(needle) {
            out.extend_from_slice(with);
            i += needle.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

fn hex_bytes(s: &str) -> R<Vec<u8>> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if s.len() % 2 != 0 {
        return Err(bad_input(format!("run-cases: odd-length hex {s:?}")));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| bad_input(format!("run-cases: bad hex {s:?}"))))
        .collect()
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn scratch() -> R<PathBuf> {
    for _ in 0..100 {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!("c2r-{}-{n}-{nanos:x}", std::process::id()));
        if std::fs::create_dir(&p).is_ok() {
            return Ok(p);
        }
    }
    Err(bad_input("run-cases: cannot create a scratch directory"))
}

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

type Obs = Vec<(&'static str, Value)>;

fn run_one(binary: &Path, case: &Value, envrow: Option<&Value>, timeout: f64) -> R<Obs> {
    let work = Scratch(scratch()?);
    let (home, tmp, cwd) = (work.0.join(".home"), work.0.join(".tmp"), work.0.join("w"));
    for p in [&home, &tmp, &cwd] {
        std::fs::create_dir_all(p).map_err(|e| bad_input(format!("run-cases: {e}")))?;
    }
    if let Some(Value::Object(files)) = case.get("files") {
        for (rel, content) in files {
            let p = cwd.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).map_err(|e| bad_input(format!("run-cases: {e}")))?;
            }
            let bytes = match content {
                Value::Object(o) => hex_bytes(o.get("hex").and_then(Value::as_str).unwrap_or(""))?,
                other => py_str(other).into_bytes(),
            };
            std::fs::write(&p, bytes).map_err(|e| bad_input(format!("run-cases: {}: {e}", p.display())))?;
        }
    }
    let s = |p: &Path| p.to_string_lossy().into_owned();
    let mut env: Vec<(String, String)> = vec![
        ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
        ("HOME".into(), s(&home)),
        ("TMPDIR".into(), s(&tmp)),
        ("TMP".into(), s(&tmp)),
        ("TEMP".into(), s(&tmp)),
        ("USERPROFILE".into(), s(&home)),
        ("SystemRoot".into(), std::env::var("SystemRoot").unwrap_or_default()),
    ];
    if cfg!(windows) {
        // the per-user folders follow USERPROFILE; the rest is how this machine runs programs
        for (k, rel) in [("APPDATA", r"AppData\Roaming"), ("LOCALAPPDATA", r"AppData\Local")] {
            let p = home.join(rel);
            std::fs::create_dir_all(&p).map_err(|e| bad_input(format!("run-cases: {e}")))?;
            env.push((k.into(), s(&p)));
        }
        for k in ["SystemDrive", "windir", "ComSpec", "PATHEXT"] {
            env.push((k.into(), std::env::var(k).unwrap_or_default()));
        }
    }
    let mut set = |k: &str, v: String| match env.iter_mut().find(|(x, _)| x == k || (cfg!(windows) && x.eq_ignore_ascii_case(k))) {
        Some(slot) => slot.1 = v,
        None => env.push((k.to_string(), v)),
    };
    for src in [envrow, case.get("env")].into_iter().flatten() {
        if let Value::Object(o) = src {
            for (k, v) in o {
                set(k, py_str(v));
            }
        }
    }
    let args: Vec<String> = arr(case.get("args")).iter().map(py_str).collect();
    let mut cmd = Command::new(binary);
    cmd.args(&args).current_dir(&cwd).env_clear();
    for (k, v) in env.iter().filter(|(_, v)| !v.is_empty()) {
        cmd.env(k, v);
    }
    let stdin = case
        .get("stdin")
        .map(py_str)
        .filter(|_| truthy(case.get("stdin")))
        .unwrap_or_default();
    let t = case.get("timeout").and_then(Value::as_f64).unwrap_or(timeout);
    let cap = run_capture(&mut cmd, stdin.as_bytes(), Duration::from_secs_f64(t.max(0.0)))
        .map_err(|e| bad_input(format!("run-cases: cannot run {}: {e}", binary.display())))?;
    // the scratch path is run-specific: template it so it is not noise, in every spelling a
    // program may print it (resolved, which differs for a short 8.3 or symlinked temp dir, and
    // with `/` separators on Windows)
    let mut forms = vec![work.0.to_string_lossy().into_owned()];
    if let Ok(real) = std::fs::canonicalize(&work.0) {
        let real = real.to_string_lossy().into_owned();
        forms.push(real.strip_prefix(r"\\?\").map(String::from).unwrap_or(real));
    }
    if cfg!(windows) {
        let slashed: Vec<String> = forms.iter().map(|f| f.replace('\\', "/")).collect();
        forms.extend(slashed);
    }
    forms.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    forms.dedup();
    let sub = |b: &[u8]| forms.iter().fold(b.to_vec(), |acc, w| replace_bytes(&acc, w.as_bytes(), b"$WORK"));
    Ok(vec![
        (
            "call",
            json!({"args": {"argv": case.get("args").cloned().unwrap_or(json!([])), "env": envrow.cloned().unwrap_or(json!({}))}}),
        ),
        ("stdout", json!({"ret": text(&sub(&cap.stdout))})),
        ("stderr", json!({"ret": text(&sub(&cap.stderr))})),
        ("fs", json!({"state": snapshot(&cwd)})),
        ("exit", json!({"ret": category(&cap)})),
    ])
}

/// Run every case (once per environment row, `repeat` times each) and write the trace.
/// Returns (case runs, flaky case ids).
pub fn run(
    binary: &Path,
    imp: &str,
    cases: &[Value],
    matrix: Option<&[Value]>,
    repeat: usize,
    timeout: f64,
    out: &Path,
) -> R<(usize, Vec<String>)> {
    let mut f = std::fs::File::create(out).map_err(|e| bad_input(format!("run-cases: {}: {e}", out.display())))?;
    let (mut flaky, mut n) = (Vec::new(), 0);
    let rows: Vec<Option<&Value>> = match matrix {
        Some(m) if !m.is_empty() => m.iter().map(Some).collect(),
        _ => vec![None],
    };
    for case in cases {
        let id = py_str(case.get("id").unwrap_or(&Value::Null));
        for (ri, row) in rows.iter().enumerate() {
            let cid = if matrix.is_none() { id.clone() } else { format!("{id}@{ri}") };
            let obs = run_one(binary, case, *row, timeout)?;
            for _ in 1..repeat.max(1) {
                if run_one(binary, case, *row, timeout)? != obs {
                    flaky.push(cid.clone());
                    break;
                }
            }
            for (seq, (kind, fields)) in obs.into_iter().enumerate() {
                let mut e = json!({"impl": imp, "case": cid, "thread": 0, "seq": seq, "kind": kind, "fn": "main"});
                for (k, v) in fields.as_object().unwrap() {
                    e[k] = v.clone();
                }
                writeln!(f, "{}", canon(&e)).map_err(|e| bad_input(format!("run-cases: {e}")))?;
            }
            n += 1;
        }
    }
    Ok((n, flaky))
}

pub fn main(argv: &[String]) -> R<i32> {
    let spec = [
        ("--bin", Kind::Value),
        ("--impl", Kind::Value),
        ("--cases", Kind::Value),
        ("--out", Kind::Value),
        ("--env-matrix", Kind::Value),
        ("--repeat", Kind::Value),
        ("--timeout", Kind::Value),
    ];
    let a = Args::parse(argv, &spec)?;
    let (Some(bin), Some(imp), Some(cases_path), Some(out)) = (a.get("--bin"), a.get("--impl"), a.get("--cases"), a.get("--out")) else {
        return usage("--bin --impl --cases --out are required");
    };
    if imp != "cpp" && imp != "rust" {
        return usage("--impl must be cpp or rust");
    }
    let text = read_text(cases_path).map_err(|e| bad_input(format!("{cases_path}: {e}")))?;
    let mut cases = Vec::new();
    for (i, l) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        cases.push(serde_json::from_str::<Value>(l).map_err(|e| bad_input(format!("{cases_path}:{}: not JSON: {e}", i + 1)))?);
    }
    let mut ids = HashSet::new();
    for c in &cases {
        if c.get("id").is_none() {
            return Err(bad_input("run-cases: a case without 'id'"));
        }
        if !ids.insert(py_str(&c["id"])) {
            return Err(bad_input("run-cases: duplicate case ids"));
        }
    }
    let matrix = match a.get("--env-matrix") {
        Some(m) => Some(
            load_json_file(m)?
                .as_array()
                .cloned()
                .ok_or_else(|| bad_input(format!("{m}: expected a JSON list")))?,
        ),
        None => None,
    };
    let (n, flaky) = run(
        &norm(bin),
        imp,
        &cases,
        matrix.as_deref(),
        a.num("--repeat", 1usize)?,
        a.num("--timeout", 20.0f64)?,
        Path::new(out),
    )?;
    println!("ran {n} case runs, flaky: {}", flaky.len());
    for c in &flaky {
        println!("FLAKY {c}");
    }
    Ok(if flaky.is_empty() { 0 } else { 3 })
}
