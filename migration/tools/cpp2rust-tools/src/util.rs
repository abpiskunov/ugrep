//! Shared helpers: errors and exit codes, argument parsing, JSON and YAML, globs, paths,
//! processes with timeouts, SHA-256 and UTC time. Standard library plus the crate's few deps.

use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------------------------
// Errors. Exit codes are the same for every subcommand:
//   0 pass, 1 check failed (divergence, finding, failing gate), 2 usage or input error,
//   3 run-cases only: a case is not deterministic.
// ---------------------------------------------------------------------------------------------

#[derive(Debug)]
pub struct Fatal(pub i32, pub String);

pub type R<T> = Result<T, Fatal>;

pub fn usage<T>(msg: impl Into<String>) -> R<T> {
    Err(Fatal(2, msg.into()))
}

pub fn bad_input(msg: impl Into<String>) -> Fatal {
    Fatal(2, msg.into())
}

// ---------------------------------------------------------------------------------------------
// Arguments: long options only, `--name value`, `--name=value`, multi-value options that take
// every following argument up to the next option (like argparse nargs="*"), flags, positionals.
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Value,
    Multi,
    Flag,
}

#[derive(Default, Debug)]
pub struct Args {
    values: HashMap<String, Vec<String>>,
    flags: HashSet<String>,
    pub positionals: Vec<String>,
}

fn looks_like_option(a: &str) -> bool {
    a.starts_with('-') && a.len() > 1 && a[1..].parse::<f64>().is_err()
}

impl Args {
    /// Parse `argv` (without the program and subcommand) against `spec`. `--help` is reported
    /// as `Err(Fatal(0, ""))` so the caller prints its help text.
    pub fn parse(argv: &[String], spec: &[(&str, Kind)]) -> R<Args> {
        let mut a = Args::default();
        let mut i = 0;
        while i < argv.len() {
            let arg = &argv[i];
            if arg == "--help" || arg == "-h" {
                return Err(Fatal(0, String::new()));
            }
            if !looks_like_option(arg) {
                a.positionals.push(arg.clone());
                i += 1;
                continue;
            }
            let (name, inline) = match arg.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (arg.clone(), None),
            };
            let kind = spec
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, k)| *k)
                .ok_or_else(|| bad_input(format!("unrecognized argument: {name}")))?;
            match kind {
                Kind::Flag => {
                    if inline.is_some() {
                        return usage(format!("{name} takes no value"));
                    }
                    a.flags.insert(name);
                }
                Kind::Value => {
                    let v = match inline {
                        Some(v) => v,
                        None => {
                            i += 1;
                            match argv.get(i) {
                                Some(v) if !looks_like_option(v) => v.clone(),
                                _ => return usage(format!("{name} expects one argument")),
                            }
                        }
                    };
                    a.values.insert(name, vec![v]);
                }
                Kind::Multi => {
                    let entry = a.values.entry(name).or_default();
                    if let Some(v) = inline {
                        entry.push(v);
                    }
                    while i + 1 < argv.len() && !looks_like_option(&argv[i + 1]) {
                        i += 1;
                        entry.push(argv[i].clone());
                    }
                }
            }
            i += 1;
        }
        Ok(a)
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).and_then(|v| v.last()).map(|s| s.as_str())
    }

    pub fn all(&self, name: &str) -> Vec<String> {
        self.values.get(name).cloned().unwrap_or_default()
    }

    pub fn flag(&self, name: &str) -> bool {
        self.flags.contains(name)
    }

    pub fn num<T: std::str::FromStr>(&self, name: &str, default: T) -> R<T> {
        match self.get(name) {
            None => Ok(default),
            Some(s) => s.parse().map_err(|_| bad_input(format!("{name}: invalid value {s:?}"))),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// JSON and YAML
// ---------------------------------------------------------------------------------------------

/// JSON with one-space indentation and sorted keys (what the reports have always used).
pub fn pretty(v: &Value) -> String {
    fn rec(v: &Value, depth: usize, out: &mut String) {
        let pad = |d: usize| " ".repeat(d);
        match v {
            Value::Array(a) if !a.is_empty() => {
                out.push_str("[\n");
                for (i, x) in a.iter().enumerate() {
                    out.push_str(&pad(depth + 1));
                    rec(x, depth + 1, out);
                    out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
                }
                out.push_str(&pad(depth));
                out.push(']');
            }
            Value::Object(o) if !o.is_empty() => {
                out.push_str("{\n");
                for (i, (k, x)) in o.iter().enumerate() {
                    out.push_str(&pad(depth + 1));
                    out.push_str(&canon(&Value::String(k.clone())));
                    out.push_str(": ");
                    rec(x, depth + 1, out);
                    out.push_str(if i + 1 < o.len() { ",\n" } else { "\n" });
                }
                out.push_str(&pad(depth));
                out.push('}');
            }
            other => out.push_str(&canon(other)),
        }
    }
    let mut s = String::new();
    rec(v, 0, &mut s);
    s
}

/// Compact canonical JSON: sorted keys, no spaces.
pub fn canon(v: &Value) -> String {
    serde_json::to_string(v).expect("serializing a Value cannot fail")
}

pub fn write_json(path: &str, v: &Value) -> R<()> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| bad_input(format!("{path}: {e}")))?;
        }
    }
    std::fs::write(path, pretty(v) + "\n").map_err(|e| bad_input(format!("{path}: {e}")))
}

/// A file as text (invalid UTF-8 replaced), without the UTF-8 byte order mark that Windows
/// editors and PowerShell 5 write.
pub fn read_text(path: impl AsRef<Path>) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(strip_bom(&String::from_utf8_lossy(&bytes)).to_string())
}

pub fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

pub fn load_json_file(path: &str) -> R<Value> {
    let text = read_text(path).map_err(|e| bad_input(format!("{path}: {e}")))?;
    serde_json::from_str(&text).map_err(|e| bad_input(format!("{path}: not JSON: {e}")))
}

/// Parse YAML (1.2 core schema) into a JSON value. An empty document is `{}`.
pub fn yaml_to_json(text: &str) -> Result<Value, String> {
    let docs = yaml_rust2::YamlLoader::load_from_str(text).map_err(|e| e.to_string())?;
    Ok(match docs.into_iter().next() {
        None => Value::Object(Map::new()),
        Some(y) => match conv_yaml(y) {
            Value::Null => Value::Object(Map::new()),
            v => v,
        },
    })
}

fn conv_yaml(y: yaml_rust2::Yaml) -> Value {
    use yaml_rust2::Yaml;
    match y {
        Yaml::Real(s) => s
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or(Value::String(s)),
        Yaml::Integer(i) => Value::from(i),
        Yaml::String(s) => Value::String(s),
        Yaml::Boolean(b) => Value::Bool(b),
        Yaml::Array(a) => Value::Array(a.into_iter().map(conv_yaml).collect()),
        Yaml::Hash(h) => {
            let mut m = Map::new();
            for (k, v) in h {
                let key = match k {
                    Yaml::String(s) | Yaml::Real(s) => s,
                    Yaml::Integer(i) => i.to_string(),
                    Yaml::Boolean(b) => b.to_string(),
                    _ => "null".to_string(),
                };
                m.insert(key, conv_yaml(v));
            }
            Value::Object(m)
        }
        Yaml::Alias(_) | Yaml::Null | Yaml::BadValue => Value::Null,
    }
}

pub fn load_yaml_file(path: &str) -> R<Value> {
    let text = read_text(path).map_err(|e| bad_input(format!("{path}: {e}")))?;
    yaml_to_json(&text).map_err(|e| bad_input(format!("{path}: not YAML: {e}")))
}

/// `obj.get(key)` that treats null like absent (Python's `d.get(k) or default`).
pub fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v.get(key) {
        Some(Value::Null) | None => None,
        Some(x) => Some(x),
    }
}

/// Python truthiness of a JSON value.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

pub fn arr(v: Option<&Value>) -> Vec<Value> {
    match v {
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    }
}

pub fn str_list(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a.iter().map(py_str).collect(),
        _ => Vec::new(),
    }
}

/// Python `str()` of a value: strings raw, everything else as its repr.
pub fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => py_repr(other),
    }
}

/// Python `repr()` of a JSON value, used where messages show values (`'a'`, `None`, `[1, 2]`).
pub fn py_repr(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            let q = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
            let mut out = String::from(q);
            for c in s.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if c == q => {
                        out.push('\\');
                        out.push(c)
                    }
                    c => out.push(c),
                }
            }
            out.push(q);
            out
        }
        Value::Array(a) => format!("[{}]", a.iter().map(py_repr).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| format!("{}: {}", py_repr(&Value::String(k.clone())), py_repr(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Python `str.splitlines(keepends)` line boundaries.
pub fn splitlines(s: &str, keepends: bool) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut start = 0;
    let mut i = 0;
    while i < chars.len() {
        let (pos, c) = chars[i];
        let is_break = matches!(
            c,
            '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}'
        );
        if is_break {
            let mut end = pos + c.len_utf8();
            if c == '\r' && i + 1 < chars.len() && chars[i + 1].1 == '\n' {
                end += 1;
                i += 1;
            }
            out.push(if keepends {
                s[start..end].to_string()
            } else {
                s[start..pos].to_string()
            });
            start = end;
        }
        i += 1;
    }
    if start < s.len() {
        out.push(s[start..].to_string());
    }
    out
}

pub fn truncate_chars(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((i, _)) => s[..i].to_string(),
        None => s.to_string(),
    }
}

// ---------------------------------------------------------------------------------------------
// Globs
// ---------------------------------------------------------------------------------------------

/// Python `fnmatch.fnmatch` for file system paths: `*` matches any run of characters (also `/`),
/// `?` one character, `[seq]` and `[!seq]` a class. Case-insensitive on Windows, like Python's
/// `normcase`. Names that are not file system paths use [`fnmatchcase`].
pub fn fnmatch(name: &str, pat: &str) -> bool {
    if cfg!(windows) {
        let n = name.to_lowercase().replace('\\', "/");
        let p = pat.to_lowercase().replace('\\', "/");
        return wild(&n.chars().collect::<Vec<_>>(), &p.chars().collect::<Vec<_>>());
    }
    fnmatchcase(name, pat)
}

/// Python `fnmatch.fnmatchcase`: the same on every platform. For function names, value paths
/// and git paths, which are case-sensitive even on Windows.
pub fn fnmatchcase(name: &str, pat: &str) -> bool {
    wild(&name.chars().collect::<Vec<_>>(), &pat.chars().collect::<Vec<_>>())
}

/// Match a class at `p[0] == '['`. Returns (matched, pattern length) or None if not a class.
fn class(c: char, p: &[char]) -> Option<(bool, usize)> {
    let mut j = 1;
    let neg = p.get(j) == Some(&'!');
    if neg {
        j += 1;
    }
    let first = j;
    let mut items = Vec::new();
    while j < p.len() && (p[j] != ']' || j == first) {
        if j + 2 < p.len() && p[j + 1] == '-' && p[j + 2] != ']' {
            items.push((p[j], p[j + 2]));
            j += 3;
        } else {
            items.push((p[j], p[j]));
            j += 1;
        }
    }
    if j >= p.len() {
        return None; // unclosed: '[' is literal
    }
    let hit = items.iter().any(|(lo, hi)| *lo <= c && c <= *hi);
    Some((hit != neg, j + 1))
}

fn wild(n: &[char], p: &[char]) -> bool {
    let (mut ni, mut pi) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() {
            match p[pi] {
                '*' => {
                    star = Some((pi, ni));
                    pi += 1;
                    continue;
                }
                '?' => {
                    ni += 1;
                    pi += 1;
                    continue;
                }
                '[' => {
                    if let Some((ok, len)) = class(n[ni], &p[pi..]) {
                        if ok {
                            ni += 1;
                            pi += len;
                            continue;
                        }
                    } else if n[ni] == '[' {
                        ni += 1;
                        pi += 1;
                        continue;
                    }
                }
                c if c == n[ni] => {
                    ni += 1;
                    pi += 1;
                    continue;
                }
                _ => {}
            }
        }
        match star {
            Some((sp, sn)) => {
                pi = sp + 1;
                ni = sn + 1;
                star = Some((sp, sn + 1));
            }
            None => return false,
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Any glob matches a file system path ([`fnmatch`]).
pub fn match_any(path: &str, globs: &[String]) -> bool {
    globs.iter().any(|g| fnmatch(path, g))
}

/// Any glob matches a git path ([`fnmatchcase`]): the same result on every platform. Git always
/// separates with `/`, so a plan glob written with `\` on Windows is read with `/`; letter case
/// still matters.
pub fn match_any_case(path: &str, globs: &[String]) -> bool {
    globs.iter().any(|g| fnmatchcase(path, &g.replace('\\', "/")))
}

fn has_magic(s: &str) -> bool {
    s.contains(['*', '?', '['])
}

/// Python `glob.glob(os.path.join(root, pattern), recursive=True)`: `*` stays inside one path
/// segment, `**` matches any number of directories, names starting with `.` need a pattern
/// starting with `.`.
pub fn glob_paths(root: &str, pattern: &str) -> Vec<String> {
    let full = if Path::new(pattern).is_absolute() {
        pattern.to_string()
    } else {
        join_str(root, pattern)
    };
    let norm = full.replace('\\', "/");
    let (base, rest) = if let Some(stripped) = norm.strip_prefix('/') {
        ("/".to_string(), stripped.to_string())
    } else if norm.len() > 2 && norm.as_bytes()[1] == b':' {
        (norm[..3].to_string(), norm[3..].to_string())
    } else {
        (String::new(), norm.clone())
    };
    let segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    let mut out = Vec::new();
    glob_rec(&base, &segs, &mut out);
    out.sort();
    out.dedup();
    out
}

fn join_str(a: &str, b: &str) -> String {
    if a.is_empty() {
        b.to_string()
    } else if a.ends_with('/') || a.ends_with('\\') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

fn glob_rec(dir: &str, segs: &[&str], out: &mut Vec<String>) {
    let Some((seg, rest)) = segs.split_first() else {
        if !dir.is_empty() && Path::new(dir).exists() {
            out.push(dir.to_string());
        }
        return;
    };
    let list_dir = if dir.is_empty() { "." } else { dir };
    if *seg == "**" {
        glob_rec(dir, rest, out); // zero directories
        if let Ok(rd) = std::fs::read_dir(list_dir) {
            let mut names: Vec<_> = rd
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            for name in names.into_iter().filter(|n| !n.starts_with('.')) {
                glob_rec(&join_str(dir, &name), segs, out);
            }
        }
    } else if !has_magic(seg) {
        let next = join_str(dir, seg);
        if rest.is_empty() {
            if Path::new(&next).exists() {
                out.push(next);
            }
        } else if Path::new(&next).is_dir() {
            glob_rec(&next, rest, out);
        }
    } else if let Ok(rd) = std::fs::read_dir(list_dir) {
        let mut names: Vec<_> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        for name in names {
            if name.starts_with('.') && !seg.starts_with('.') {
                continue;
            }
            if fnmatch(&name, seg) {
                let next = join_str(dir, &name);
                if rest.is_empty() {
                    out.push(next);
                } else if Path::new(&next).is_dir() {
                    glob_rec(&next, rest, out);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------------------------

/// Python `os.path.normpath(os.path.abspath(p))`: absolute and lexically normalized; symbolic
/// links are not resolved.
pub fn norm(p: impl AsRef<Path>) -> PathBuf {
    let p = p.as_ref();
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `p` relative to `root`, component by component; None when `p` is not below `root`. Windows
/// paths compare case-insensitively (`C:\Src` is `c:\src`), as the file system does.
pub fn strip_root(p: &Path, root: &Path) -> Option<PathBuf> {
    let same = |a: &Component, b: &Component| {
        if cfg!(windows) {
            a.as_os_str().to_string_lossy().to_lowercase() == b.as_os_str().to_string_lossy().to_lowercase()
        } else {
            a == b
        }
    };
    let mut rest = p.components();
    for r in root.components() {
        match rest.next() {
            Some(c) if same(&c, &r) => {}
            _ => return None,
        }
    }
    Some(rest.as_path().to_path_buf())
}

/// A path as a map key: on Windows letter case and `/` versus `\` do not tell paths apart.
pub fn path_key(p: &Path) -> String {
    let s = p.to_string_lossy();
    if cfg!(windows) {
        s.replace('/', "\\").to_lowercase()
    } else {
        s.into_owned()
    }
}

/// Path of `p` relative to `root` with `/` separators (or `p` itself when outside `root`).
pub fn rel_slash(p: &Path, root: &Path) -> String {
    match strip_root(p, root) {
        Some(r) => r
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
        None if cfg!(windows) => p.to_string_lossy().replace('\\', "/"),
        None => p.to_string_lossy().into_owned(),
    }
}

/// Every file below `dir`, sorted, as paths relative to `base` with `/` separators.
pub fn walk_files(dir: &Path, base: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn rec(d: &Path, base: &Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => rec(&p, base, out),
                Ok(_) => out.push(rel_slash(&p, base)),
                Err(_) => {}
            }
        }
    }
    rec(dir, base, &mut out);
    out
}

// ---------------------------------------------------------------------------------------------
// Processes
// ---------------------------------------------------------------------------------------------

pub struct Captured {
    pub status: Option<ExitStatus>,
    pub timed_out: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

fn reader(mut r: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = r.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    rx
}

/// The processes a child starts, so a timeout ends all of them. On Windows a command runs under
/// `cmd.exe`, and killing cmd.exe alone leaves the command itself running (writing its log and
/// result files after the gate was decided): the child goes into a job object and the whole job
/// is ended. The child is added right after it starts, long before it can start anything itself.
/// Elsewhere the child alone is killed (`sh -c` runs a single command in its own process).
struct Tree {
    #[cfg(windows)]
    job: Option<job::Job>,
}

impl Tree {
    fn of(child: &std::process::Child) -> Tree {
        #[cfg(windows)]
        {
            Tree {
                job: job::Job::with(child),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = child;
            Tree {}
        }
    }

    fn kill(&self, child: &mut std::process::Child) {
        #[cfg(windows)]
        if let Some(j) = &self.job {
            j.terminate();
        }
        let _ = child.kill();
    }
}

#[cfg(windows)]
mod job {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    type Handle = *mut c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
        fn TerminateJobObject(job: Handle, exit_code: u32) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    pub struct Job(Handle);

    impl Job {
        /// A new job holding `child` (and so every process it starts), or None when the
        /// system refuses (then only the child can be killed).
        pub fn with(child: &std::process::Child) -> Option<Job> {
            // SAFETY: Win32 calls with valid arguments; the job handle is owned by the result,
            // the process handle stays owned by `child`, which outlives this call.
            unsafe {
                let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if h.is_null() {
                    return None;
                }
                if AssignProcessToJobObject(h, child.as_raw_handle()) == 0 {
                    CloseHandle(h);
                    return None;
                }
                Some(Job(h))
            }
        }

        pub fn terminate(&self) {
            // SAFETY: the handle is open until drop.
            unsafe {
                TerminateJobObject(self.0, 1);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: the handle is open and closed only here. Closing it does not end the
            // processes (no kill-on-close limit is set).
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

fn wait_timeout(child: &mut std::process::Child, tree: &Tree, timeout: Duration) -> std::io::Result<Option<ExitStatus>> {
    let start = Instant::now();
    loop {
        if let Some(st) = child.try_wait()? {
            return Ok(Some(st));
        }
        if start.elapsed() >= timeout {
            tree.kill(child);
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Run with `input` on stdin, capture stdout and stderr, kill (with everything it started)
/// after `timeout`.
pub fn run_capture(cmd: &mut Command, input: &[u8], timeout: Duration) -> std::io::Result<Captured> {
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let tree = Tree::of(&child);
    let mut stdin = child.stdin.take().expect("piped");
    let data = input.to_vec();
    std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
    });
    let out = reader(child.stdout.take().expect("piped"));
    let err = reader(child.stderr.take().expect("piped"));
    let status = wait_timeout(&mut child, &tree, timeout)?;
    let grace = if status.is_some() {
        Duration::from_secs(3600)
    } else {
        Duration::from_secs(2)
    };
    Ok(Captured {
        timed_out: status.is_none(),
        status,
        stdout: out.recv_timeout(grace).unwrap_or_default(),
        stderr: err.recv_timeout(grace).unwrap_or_default(),
    })
}

/// A command line run by the platform shell (`sh -c` or `cmd /d /s /c`), like Python's shell=True.
pub fn shell(cmdline: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new(std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()));
        c.raw_arg(format!("/d /s /c \"{}\"", cmd_line(cmdline)));
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("/bin/sh");
        c.arg("-c").arg(cmdline);
        c
    }
}

/// What `cmd /c` runs for `cmdline`. cmd.exe exits with 1, not 9009, when a command is not
/// found: 9009 is only its ERRORLEVEL. So the command runs as a block followed by: exit 0 on
/// success, else exit with ERRORLEVEL (9009 when not found, the program's code otherwise), or
/// with 1 when a failure left ERRORLEVEL at 0 (a failed redirection), so a failure never turns
/// into a pass. A line the block would change runs unchanged (a missing command then exits 1):
/// an open quote, a paren or caret outside quotes (`for ... do`, `if ... (...)`, `echo (x)`),
/// a line break, a `rem` (it would comment out the closing paren), a trailing operator, or a`n/// `%` outside quotes (cmd expands it before parsing, and `%PATH%` holds `Program Files (x86)`).
#[cfg(windows)]
fn cmd_line(cmdline: &str) -> String {
    let t = cmdline.trim();
    let mut quoted = false;
    let mut word = String::new();
    let mut ok = !t.is_empty() && !t.ends_with(['&', '|', '<', '>']);
    for c in t.chars().chain([' ']) {
        match c {
            '\r' | '\n' => ok = false,
            '"' => quoted = !quoted,
            _ if quoted => {}
            '(' | ')' | '^' | '%' => ok = false,
            c if c.is_whitespace() || "&|<>".contains(c) => {
                let w = word.trim_start_matches('@');
                if w.eq_ignore_ascii_case("rem") || w.starts_with("::") {
                    ok = false;
                }
                word.clear();
            }
            c => word.push(c),
        }
    }
    if ok && !quoted {
        format!("({t})&&exit /b 0&if errorlevel 1 (exit /b) else if not errorlevel 0 (exit /b) else exit /b 1")
    } else {
        cmdline.to_string()
    }
}

/// `s` as one word for [`shell`]: quoted when it holds a character the shell would act on
/// (a path with a space, `&` or parentheses).
pub fn shell_quote(s: &str) -> String {
    if cfg!(windows) {
        // the characters `cmd /?` lists as needing quotes; a Windows path cannot hold `"`
        if s.is_empty() || s.contains(|c: char| c.is_whitespace() || "&()[]{}^=;!'+,`~<>|".contains(c)) {
            return format!("\"{s}\"");
        }
        s.to_string()
    } else if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "@%+=:,./_-".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// Run a shell command with stdout and stderr appended to `log`. Returns the exit code, or None
/// on timeout. A process ended by a signal reports 128 + signal, as a shell does.
pub fn run_logged(cmdline: &str, cwd: &Path, log: &Path, timeout: Duration) -> std::io::Result<Option<i64>> {
    let f = std::fs::File::create(log)?;
    let f2 = f.try_clone()?;
    let mut child = shell(cmdline).current_dir(cwd).stdin(Stdio::null()).stdout(f).stderr(f2).spawn()?;
    let tree = Tree::of(&child);
    Ok(wait_timeout(&mut child, &tree, timeout)?.map(|st| exit_code(&st)))
}

pub fn exit_code(st: &ExitStatus) -> i64 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = st.signal() {
            return 128 + sig as i64;
        }
    }
    match st.code() {
        #[cfg(windows)]
        Some(c) => c as u32 as i64,
        #[cfg(not(windows))]
        Some(c) => c as i64,
        None => -1,
    }
}

/// Run git in `cwd`; stdout trimmed, or "" when git cannot run or fails.
pub fn git_quiet(args: &[&str], cwd: &Path) -> String {
    match Command::new("git").args(args).current_dir(cwd).output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => String::new(),
    }
}

// ---------------------------------------------------------------------------------------------
// SHA-256 and time
// ---------------------------------------------------------------------------------------------

pub struct Sha256 {
    h: [u32; 8],
    buf: Vec<u8>,
    len: u64,
}

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
    0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
    0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
    0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
    0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Default for Sha256 {
    fn default() -> Self {
        Sha256 {
            h: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            buf: Vec::with_capacity(64),
            len: 0,
        }
    }
}

impl Sha256 {
    fn block(&mut self, b: &[u8]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b2, mut c, mut d, mut e, mut f, mut g, mut h] = self.h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b2) ^ (a & c) ^ (b2 & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b2;
            b2 = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in self.h.iter_mut().zip([a, b2, c, d, e, f, g, h]) {
            *x = x.wrapping_add(y);
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        self.len += data.len() as u64;
        let mut data = data;
        if !self.buf.is_empty() {
            let take = (64 - self.buf.len()).min(data.len());
            self.buf.extend_from_slice(&data[..take]);
            data = &data[take..];
            if self.buf.len() == 64 {
                let b = std::mem::take(&mut self.buf);
                self.block(&b);
            }
        }
        while data.len() >= 64 {
            self.block(&data[..64]);
            data = &data[64..];
        }
        self.buf.extend_from_slice(data);
    }

    pub fn hex(mut self) -> String {
        let bits = self.len.wrapping_mul(8);
        let mut tail = std::mem::take(&mut self.buf);
        tail.push(0x80);
        while tail.len() % 64 != 56 {
            tail.push(0);
        }
        tail.extend_from_slice(&bits.to_be_bytes());
        for chunk in tail.chunks(64) {
            self.block(chunk);
        }
        self.h.iter().map(|x| format!("{x:08x}")).collect()
    }
}

/// Current UTC time as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // civil from days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_vectors() {
        let mut h = Sha256::default();
        h.update(b"abc");
        assert_eq!(h.hex(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let mut h = Sha256::default();
        for _ in 0..1000 {
            h.update(b"a");
        }
        let mut one = Sha256::default();
        one.update(&[b'a'; 1000]);
        assert_eq!(h.hex(), one.hex());
        assert_eq!(
            Sha256::default().hex(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn fnmatch_like_python() {
        assert!(fnmatch("rust/core/src/lib.rs", "rust/*/src/*"));
        assert!(fnmatch("migration/corpus/a/b.jsonl", "migration/corpus/*"));
        assert!(!fnmatch("migration/corpusx", "migration/corpus/*"));
        assert!(fnmatch("a.c", "?.[ch]") && !fnmatch("a.o", "?.[!o]") && fnmatch("[x", "[x"));
        assert!(fnmatch("", "*") && !fnmatch("", "?"));
    }

    #[test]
    fn git_path_globs_accept_backslashes_and_keep_case() {
        let globs = vec![r"migration\corpus\*".to_string(), "src/Core/*.cc".to_string()];
        assert!(match_any_case("migration/corpus/cli/cases.jsonl", &globs));
        assert!(match_any_case("src/Core/a.cc", &globs));
        assert!(!match_any_case("src/core/a.cc", &globs));
    }

    #[test]
    fn args_and_repr() {
        let argv: Vec<String> = ["--a", "1", "--m", "x", "y", "--f", "pos", "--m=z"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let a = Args::parse(&argv, &[("--a", Kind::Value), ("--m", Kind::Multi), ("--f", Kind::Flag)]).unwrap();
        assert_eq!(a.get("--a"), Some("1"));
        assert!(a.flag("--f"));
        assert_eq!(a.all("--m"), vec!["x", "y", "z"]);
        assert_eq!(a.positionals, vec!["pos"]);
        assert!(Args::parse(&["--nope".to_string()], &[]).is_err());
        assert_eq!(py_repr(&serde_json::json!(["a", null, true, 1.5])), "['a', None, True, 1.5]");
        assert_eq!(splitlines("a\r\nb\rc\n", true), vec!["a\r\n", "b\r", "c\n"]);
        assert_eq!(utc_now().len(), 20);
    }

    #[test]
    fn yaml_flow_and_block() {
        let v = yaml_to_json("a: {x: 1, y: [p, q]}\nb: >-\n  one\n  two\nc: 1.5\nd: null\n").unwrap();
        assert_eq!(v["a"]["y"][1], "q");
        assert_eq!(v["b"], "one two");
        assert_eq!(v["c"], 1.5);
        assert!(v["d"].is_null());
        assert_eq!(yaml_to_json("").unwrap(), serde_json::json!({}));
    }

    #[test]
    fn files_with_bom_and_crlf() {
        let d = crate::testutil::TempDir::new("bom");
        d.write(
            "p.yaml",
            "\u{feff}a: 1\r\nb: [x, y]\r\nc: >-\r\n  one\r\n  two\r\nd: |\r\n  l1\r\n  l2\r\n",
        );
        d.write("p.json", "\u{feff}{\"a\": 1}\r\n");
        let p = |n: &str| d.path().join(n).to_string_lossy().into_owned();
        let v = load_yaml_file(&p("p.yaml")).unwrap();
        assert_eq!(v, serde_json::json!({"a": 1, "b": ["x", "y"], "c": "one two", "d": "l1\nl2\n"}));
        assert_eq!(load_json_file(&p("p.json")).unwrap()["a"], 1);
    }

    #[test]
    fn relative_paths() {
        let root = norm("proj");
        assert_eq!(rel_slash(&root.join("src").join("a.cc"), &root), "src/a.cc");
        assert_eq!(rel_slash(&root, &root), "");
        assert!(strip_root(&norm("projx/a"), &root).is_none());
        assert_eq!(rel_slash(Path::new("./rust/x.rs"), Path::new(".")), "rust/x.rs");
        if cfg!(windows) {
            // the file system ignores letter case, and so must the comparison
            let upper = PathBuf::from(root.to_string_lossy().to_uppercase());
            assert_eq!(rel_slash(&upper.join("Src").join("a.cc"), &root), "Src/a.cc");
            assert_eq!(path_key(Path::new("C:/A/b")), path_key(Path::new(r"c:\a\B")));
        }
    }

    #[test]
    fn quoting_for_the_shell() {
        assert_eq!(shell_quote("/usr/bin/tool"), "/usr/bin/tool");
        if cfg!(windows) {
            assert_eq!(shell_quote(r"C:\tools\x.exe"), r"C:\tools\x.exe");
            assert_eq!(shell_quote(r"C:\R&D (x86)\x.exe"), r#""C:\R&D (x86)\x.exe""#);
            assert_eq!(shell_quote(r"C:\a&b\x.exe"), r#""C:\a&b\x.exe""#);
        } else {
            assert_eq!(shell_quote("/a b/it's $x"), r"'/a b/it'\''s $x'");
            assert_eq!(shell_quote("/a&b/x"), "'/a&b/x'");
        }
    }

    /// cmd.exe: a command that is not found exits 9009, every other exit code and every
    /// failure is kept, and lines with parens, loops or comments run unchanged.
    #[cfg(windows)]
    #[test]
    fn windows_shell_exit_codes() {
        let d = crate::testutil::TempDir::new("cmd");
        let log = d.path().join("x.log");
        let run = |c: &str| run_logged(c, d.path(), &log, Duration::from_secs(30)).unwrap().unwrap();
        let logged = || read_text(&log).unwrap();
        assert_eq!(run("definitely-not-a-command-xyz --flag \"a b\""), 9009);
        assert_eq!(run("echo a && definitely-not-a-command-xyz"), 9009);
        assert_eq!(run("cmd /d /c exit 0"), 0);
        assert_eq!(run("cmd /d /c exit 5"), 5);
        assert_eq!(run("cmd /d /c exit 300"), 300);
        assert_eq!(run("cmd /d /c exit -1073741819"), 0xC000_0005); // unsigned, like the OS
        assert_eq!(run("cmd /d /c exit 0 & cmd /d /c exit 4"), 4);
        assert_eq!(run("definitely-not-a-command-xyz || echo fallback"), 0);
        assert_eq!(run("echo x > missing\\dir\\f"), 1); // a failed redirection is a failure
        assert_eq!(run("echo \"quoted & (stuff)\""), 0);
        assert!(logged().contains("\"quoted & (stuff)\""));
        assert_eq!(run("for %i in (1 2 3) do @echo item%i"), 0);
        assert!(logged().contains("item3"), "{}", logged()); // the loop is not cut short
        assert_eq!(run("echo (paren)"), 0);
        assert_eq!(run("echo a & rem b"), 0);
        assert_eq!(run("exit 7"), 7);
        // an unquoted %VAR% may expand to a paren (Program Files (x86)): the line runs unchanged
        assert!(std::env::var("ProgramFiles(x86)").is_ok_and(|v| v.contains(')')));
        assert_eq!(run("set P=%ProgramFiles(x86)%;y&& echo ran-ok"), 0);
        assert!(logged().contains("ran-ok"), "{}", logged());
        assert_eq!(run("echo %ProgramFiles(x86)% & exit 5"), 5);
    }
}
