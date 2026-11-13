//! depgraph: dependency graph and port order for a C/C++ tree.

use crate::util::*;
use regex::Regex;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::Duration;

pub const HELP: &str = "\
cpp2rust-tools depgraph - dependency graph and port order for a C/C++ tree.

Input: compile_commands.json (preferred) or a source root. Files are grouped into
components (by directory depth or a mapping file); edges come from resolved #include
lines and, with --nm, from undefined/defined symbols of the compiled objects (call and
data references). Strongly connected components are merged; levels are topological,
leaves first. Level 0 is ported first.

Usage:
  depgraph --compdb build/compile_commands.json --root . [--depth 2] [--map comp.json]
           [--nm [--nm-tool llvm-nm]] [--out migration/depgraph.json]
  depgraph --src-root src --root .            # no compile database (weaker)

  --depth N    directory depth that defines a component (default 2)
  --map FILE   JSON list of [glob, component] pairs; first match wins
  --nm         add symbol edges from compiled objects (--nm-tool, default nm)
Exit: 0 written, 2 usage or input error.";

static INC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)^\s*#\s*include\s*([<"])([^>"]+)[>"]"#).unwrap());
const SRC_EXT: [&str; 5] = [".c", ".cc", ".cpp", ".cxx", ".c++"];

/// A translation unit: include directories and the object file, if known.
pub struct Tu {
    pub dirs: Vec<PathBuf>,
    pub obj: Option<PathBuf>,
}

/// Translation units in compile-database order (the order decides which include directories a
/// header inherits, exactly as before).
pub type Tus = Vec<(PathBuf, Tu)>;

fn loc(text: &str) -> usize {
    splitlines(text, false)
        .iter()
        .filter(|l| !l.trim().is_empty() && !l.trim().starts_with("//"))
        .count()
}

/// POSIX shell-style splitting (Python shlex.split posix=True).
fn split_posix(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut have = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                have = true;
                for d in chars.by_ref() {
                    if d == '\'' {
                        break;
                    }
                    cur.push(d);
                }
            }
            '"' => {
                have = true;
                while let Some(d) = chars.next() {
                    match d {
                        '"' => break,
                        '\\' if matches!(chars.peek(), Some('"' | '\\' | '$' | '`' | '\n')) => cur.push(chars.next().unwrap()),
                        d => cur.push(d),
                    }
                }
            }
            '\\' => {
                have = true;
                if let Some(d) = chars.next() {
                    cur.push(d);
                }
            }
            c if c.is_whitespace() => {
                if have {
                    out.push(std::mem::take(&mut cur));
                    have = false;
                }
            }
            c => {
                have = true;
                cur.push(c)
            }
        }
    }
    if have {
        out.push(cur);
    }
    out
}

/// Windows command-line splitting (CommandLineToArgvW rules).
fn split_windows(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut inq, mut have, mut bs) = (false, false, 0usize);
    for c in s.chars() {
        match c {
            '\\' => {
                bs += 1;
                have = true;
            }
            '"' => {
                cur.push_str(&"\\".repeat(bs / 2));
                if bs % 2 == 1 {
                    cur.push('"');
                } else {
                    inq = !inq;
                }
                bs = 0;
                have = true;
            }
            c if c.is_whitespace() && !inq => {
                cur.push_str(&"\\".repeat(bs));
                bs = 0;
                if have {
                    out.push(std::mem::take(&mut cur));
                    have = false;
                }
            }
            c => {
                cur.push_str(&"\\".repeat(bs));
                bs = 0;
                cur.push(c);
                have = true;
            }
        }
    }
    cur.push_str(&"\\".repeat(bs));
    if have {
        out.push(cur);
    }
    out
}

fn cmd_args(entry: &Value) -> Vec<String> {
    if let Some(Value::Array(a)) = entry.get("arguments") {
        return a.iter().map(py_str).collect();
    }
    let c = entry.get("command").and_then(Value::as_str).unwrap_or("");
    if cfg!(windows) { split_windows(c) } else { split_posix(c) }
}

fn include_dirs(args: &[String], cwd: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        for flag in ["-I", "/I", "-isystem", "-iquote", "-external:I", "/external:I"] {
            if a == flag && i + 1 < args.len() {
                dirs.push(norm(cwd.join(&args[i + 1])));
                i += 1;
                break;
            }
            if a.starts_with(flag) && a.len() > flag.len() && ["-I", "/I", "-external:I", "/external:I"].contains(&flag) {
                dirs.push(norm(cwd.join(&a[flag.len()..])));
                break;
            }
        }
        i += 1;
    }
    dirs
}

fn under(f: &Path, root: &Path) -> bool {
    strip_root(f, root).is_some_and(|r| !r.as_os_str().is_empty())
}

/// One spelling per file: on Windows `C:\Src\a.h` and `c:/src/A.h` (from different include
/// directories or compile database entries) are the same node; the first spelling seen wins.
#[derive(Default)]
struct Spellings(HashMap<String, PathBuf>);

impl Spellings {
    fn of(&mut self, p: PathBuf) -> PathBuf {
        self.0.entry(path_key(&p)).or_insert(p).clone()
    }
}

pub fn load_tus(compdb: Option<&Path>, src_root: Option<&Path>, root: &Path) -> R<Tus> {
    let mut tus: Tus = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut put = |tus: &mut Tus, f: PathBuf, tu: Tu| match index.get(&path_key(&f)) {
        Some(&i) => tus[i].1 = tu,
        None => {
            index.insert(path_key(&f), tus.len());
            tus.push((f, tu));
        }
    };
    if let Some(db) = compdb {
        let doc = load_json_file(&db.to_string_lossy())?;
        let entries = doc
            .as_array()
            .ok_or_else(|| bad_input(format!("{}: not a JSON list", db.display())))?;
        for e in entries {
            let cwd = PathBuf::from(e.get("directory").and_then(Value::as_str).unwrap_or("."));
            let file = e
                .get("file")
                .and_then(Value::as_str)
                .ok_or_else(|| bad_input("compile database entry without 'file'"))?;
            let f = norm(cwd.join(file));
            let args = cmd_args(e);
            let mut obj = e.get("output").and_then(Value::as_str).map(String::from);
            if obj.is_none() {
                for (i, a) in args.iter().enumerate() {
                    if a == "-o" && i + 1 < args.len() {
                        obj = Some(args[i + 1].clone());
                    } else if let Some(o) = a.strip_prefix("/Fo") {
                        obj = Some(o.to_string());
                    }
                }
            }
            let tu = Tu {
                dirs: include_dirs(&args, &cwd),
                obj: obj.map(|o| norm(cwd.join(o))),
            };
            put(&mut tus, f, tu);
        }
    } else if let Some(src) = src_root {
        let srcn = norm(src);
        for rel in walk_files(src, src) {
            if SRC_EXT.iter().any(|e| rel.to_lowercase().ends_with(e)) {
                put(
                    &mut tus,
                    norm(src.join(&rel)),
                    Tu {
                        dirs: vec![srcn.clone()],
                        obj: None,
                    },
                );
            }
        }
    }
    tus.retain(|(f, _)| under(f, root));
    Ok(tus)
}

/// Resolve includes transitively. Only first-party files (under root) become nodes.
fn file_graph(tus: &Tus, root: &Path) -> (BTreeMap<PathBuf, BTreeSet<PathBuf>>, HashMap<PathBuf, String>) {
    let mut edges: BTreeMap<PathBuf, BTreeSet<PathBuf>> = BTreeMap::new();
    let mut texts = HashMap::new();
    let mut spell = Spellings::default();
    for (f, _) in tus {
        spell.of(f.clone());
    }
    let mut owner: HashMap<PathBuf, Vec<PathBuf>> = tus.iter().map(|(f, t)| (f.clone(), t.dirs.clone())).collect();
    let mut todo: Vec<PathBuf> = tus.iter().map(|(f, _)| f.clone()).collect();
    while let Some(f) = todo.pop() {
        if edges.contains_key(&f) {
            continue;
        }
        let text = read_text(&f).unwrap_or_default();
        let dirs = owner.get(&f).cloned().unwrap_or_default();
        let mut out = BTreeSet::new();
        for cap in INC.captures_iter(&text) {
            let (kind, name) = (&cap[1], &cap[2]);
            let mut cands: Vec<PathBuf> = Vec::new();
            if kind == "\"" {
                cands.push(f.parent().unwrap_or(Path::new("")).join(name));
            }
            cands.extend(dirs.iter().map(|d| d.join(name)));
            for c in cands {
                let c = norm(c);
                if c.is_file() {
                    let c = spell.of(c);
                    if under(&c, root) {
                        out.insert(c.clone());
                        owner.entry(c.clone()).or_insert_with(|| dirs.clone());
                        todo.push(c);
                    }
                    break;
                }
            }
        }
        texts.insert(f.clone(), text);
        edges.insert(f, out);
    }
    (edges, texts)
}

/// Symbol-level edges between translation units: undefined in A, defined in B.
fn nm_edges(tus: &Tus, tool: &str) -> BTreeMap<PathBuf, BTreeMap<PathBuf, BTreeSet<String>>> {
    let mut defined: HashMap<String, PathBuf> = HashMap::new();
    let mut undefined: Vec<(PathBuf, BTreeSet<String>)> = Vec::new();
    for (f, tu) in tus {
        let Some(obj) = &tu.obj else { continue };
        if !obj.is_file() {
            continue;
        }
        let Ok(out) = run_capture(Command::new(tool).args(["-P", "-g"]).arg(obj), b"", Duration::from_secs(60)) else {
            continue;
        };
        let mut und = BTreeSet::new();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }
            let (sym, typ) = (parts[0], parts[1]);
            if typ == "U" {
                und.insert(sym.to_string());
            } else if typ.len() == 1 && "TDBRSV".contains(&typ.to_uppercase()) && !"wv".contains(typ) {
                defined.entry(sym.to_string()).or_insert_with(|| f.clone());
            }
        }
        if !und.is_empty() {
            undefined.push((f.clone(), und));
        }
    }
    let mut edges: BTreeMap<PathBuf, BTreeMap<PathBuf, BTreeSet<String>>> = BTreeMap::new();
    for (f, syms) in undefined {
        for s in syms {
            if let Some(g) = defined.get(&s) {
                if *g != f {
                    edges.entry(f.clone()).or_default().entry(g.clone()).or_default().insert(s);
                }
            }
        }
    }
    edges
}

fn component_of(f: &Path, root: &Path, depth: usize, mapping: &[(String, String)]) -> String {
    let rel = rel_slash(f, root);
    for (pat, comp) in mapping {
        if fnmatch(&rel, pat) {
            return comp.clone();
        }
    }
    let parts: Vec<&str> = rel.split('/').collect();
    let dirs = &parts[..parts.len().saturating_sub(1)];
    let c = dirs[..depth.min(dirs.len())].join("/");
    if c.is_empty() { ".".into() } else { c }
}

/// Tarjan's algorithm, iterative; SCCs come out sinks first (reverse topological order).
fn tarjan(nodes: &[String], succ: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
    let empty = BTreeSet::new();
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut low: HashMap<&str, usize> = HashMap::new();
    let mut on: BTreeSet<&str> = BTreeSet::new();
    let mut stack: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    let mut n = 0;
    for root in nodes {
        if index.contains_key(root.as_str()) {
            continue;
        }
        let mut work: Vec<(&str, Vec<&str>, usize)> = Vec::new();
        let start = |v: &str| succ.get(v).unwrap_or(&empty).iter().map(String::as_str).collect::<Vec<_>>();
        index.insert(root, n);
        low.insert(root, n);
        n += 1;
        stack.push(root);
        on.insert(root);
        work.push((root, start(root), 0));
        while let Some(top) = work.last_mut() {
            let v = top.0;
            if top.2 < top.1.len() {
                let w = top.1[top.2];
                top.2 += 1;
                if !index.contains_key(w) {
                    index.insert(w, n);
                    low.insert(w, n);
                    n += 1;
                    stack.push(w);
                    on.insert(w);
                    work.push((w, start(w), 0));
                } else if on.contains(w) {
                    let lw = index[w];
                    let lv = low.get_mut(v).unwrap();
                    *lv = (*lv).min(lw);
                }
                continue;
            }
            work.pop();
            if let Some(parent) = work.last() {
                let lv = low[v];
                let lp = low.get_mut(parent.0).unwrap();
                *lp = (*lp).min(lv);
            }
            if low[v] == index[v] {
                let mut scc = Vec::new();
                loop {
                    let w = stack.pop().unwrap();
                    on.remove(w);
                    scc.push(w.to_string());
                    if w == v {
                        break;
                    }
                }
                scc.sort();
                out.push(scc);
            }
        }
    }
    out
}

pub fn build(tus: &Tus, root: &Path, depth: usize, mapping: &[(String, String)], nm_tool: Option<&str>) -> Value {
    let (fedges, texts) = file_graph(tus, root);
    // On Windows one directory may be spelled in several letter cases; it is one component,
    // named as the compile database's first file in it spells it.
    let mut names: HashMap<String, String> = HashMap::new();
    let mut name = |c: String| {
        let key = if cfg!(windows) { c.to_lowercase() } else { c.clone() };
        names.entry(key).or_insert(c).clone()
    };
    for (f, _) in tus.iter().filter(|(f, _)| fedges.contains_key(f)) {
        name(component_of(f, root, depth, mapping));
    }
    let comp: BTreeMap<&PathBuf, String> = fedges.keys().map(|f| (f, name(component_of(f, root, depth, mapping)))).collect();
    let mut cedges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut why: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut add = |a: &str, b: &str, reason: String| {
        if a != b {
            cedges.entry(a.to_string()).or_default().insert(b.to_string());
            let w = why.entry(format!("{a} -> {b}")).or_default();
            if w.len() < 5 {
                w.push(reason);
            }
        }
    };
    for (f, tos) in &fedges {
        for t in tos {
            // a header's component depends on what it includes; a source too
            add(
                &comp[f],
                &comp[t],
                format!("include {} -> {}", rel_slash(f, root), rel_slash(t, root)),
            );
        }
    }
    if let Some(tool) = nm_tool {
        for (f, tos) in nm_edges(tus, tool) {
            for (t, syms) in tos {
                if let (Some(a), Some(b)) = (comp.get(&f), comp.get(&t)) {
                    add(a, b, format!("symbol {}", syms.iter().next().unwrap()));
                }
            }
        }
    }
    let comps: Vec<String> = comp.values().cloned().collect::<BTreeSet<_>>().into_iter().collect();
    let sccs = tarjan(&comps, &cedges);
    let group: HashMap<&str, usize> = sccs
        .iter()
        .enumerate()
        .flat_map(|(i, s)| s.iter().map(move |c| (c.as_str(), i)))
        .collect();
    // Tarjan emits sinks first, so every dependency's level is known when it is needed.
    let mut level = vec![0i64; sccs.len()];
    for i in 0..sccs.len() {
        let deps: BTreeSet<usize> = sccs[i]
            .iter()
            .flat_map(|c| cedges.get(c).into_iter().flatten())
            .map(|d| group[d.as_str()])
            .filter(|&g| g != i)
            .collect();
        level[i] = 1 + deps.iter().map(|&d| level[d]).max().unwrap_or(-1);
    }
    let dependents = |c: &str| comps.iter().filter(|d| cedges.get(*d).is_some_and(|s| s.contains(c))).count();
    let mut info = serde_json::Map::new();
    for c in &comps {
        let files: Vec<&PathBuf> = comp.iter().filter(|(_, v)| *v == c).map(|(f, _)| *f).collect();
        let mut rels: Vec<String> = files.iter().map(|f| rel_slash(f, root)).collect();
        rels.sort();
        let g = group[c.as_str()];
        info.insert(
            c.clone(),
            json!({
                "level": level[g],
                "files": rels,
                "loc": files.iter().map(|f| loc(texts.get(*f).map(String::as_str).unwrap_or(""))).sum::<usize>(),
                "depends_on": cedges.get(c).cloned().unwrap_or_default(),
                "cycle_with": sccs[g].iter().filter(|x| *x != c).collect::<Vec<_>>(),
            }),
        );
    }
    let mut order = comps.clone();
    order.sort_by_key(|c| (level[group[c.as_str()]], std::cmp::Reverse(dependents(c)), c.clone()));
    let mut edges_from = vec!["include"];
    if nm_tool.is_some() {
        edges_from.push("symbol");
    }
    json!({
        "schema": "cpp-rust-depgraph/1",
        "root": root.to_string_lossy(),
        "edges_from": edges_from,
        "translation_units": tus.len(),
        "components": info,
        "port_order": order,
        "cycles": sccs.iter().filter(|s| s.len() > 1).collect::<Vec<_>>(),
        "edge_evidence": why,
    })
}

pub fn main(argv: &[String]) -> R<i32> {
    let spec = [
        ("--compdb", Kind::Value),
        ("--src-root", Kind::Value),
        ("--root", Kind::Value),
        ("--depth", Kind::Value),
        ("--map", Kind::Value),
        ("--nm", Kind::Flag),
        ("--nm-tool", Kind::Value),
        ("--out", Kind::Value),
    ];
    let a = Args::parse(argv, &spec)?;
    if a.get("--compdb").is_none() && a.get("--src-root").is_none() {
        return usage("give --compdb or --src-root");
    }
    let root = norm(a.get("--root").unwrap_or("."));
    let mapping: Vec<(String, String)> = match a.get("--map") {
        None => Vec::new(),
        Some(m) => {
            let v = load_json_file(m)?;
            v.as_array()
                .ok_or_else(|| bad_input(format!("{m}: expected a list of [glob, component] pairs")))?
                .iter()
                .map(|p| (py_str(&p[0]), py_str(&p[1])))
                .collect()
        }
    };
    let tus = load_tus(a.get("--compdb").map(Path::new), a.get("--src-root").map(Path::new), &root)?;
    let nm_tool = if a.flag("--nm") {
        Some(a.get("--nm-tool").unwrap_or("nm"))
    } else {
        None
    };
    let g = build(&tus, &root, a.num("--depth", 2usize)?, &mapping, nm_tool);
    if let Some(out) = a.get("--out") {
        write_json(out, &g)?;
    }
    println!("level  loc     component (depends on)");
    for c in g["port_order"].as_array().unwrap() {
        let c = c.as_str().unwrap();
        let i = &g["components"][c];
        let deps: Vec<String> = i["depends_on"].as_array().unwrap().iter().map(py_str).collect();
        let cyc: Vec<String> = i["cycle_with"].as_array().unwrap().iter().map(py_str).collect();
        println!(
            "{:<6} {:<7} {}{}{}",
            i["level"].as_i64().unwrap_or(0),
            i["loc"].as_u64().unwrap_or(0),
            c,
            if deps.is_empty() {
                String::new()
            } else {
                format!("  <- {}", deps.join(", "))
            },
            if cyc.is_empty() {
                String::new()
            } else {
                format!("  [cycle: {}]", cyc.join(", "))
            }
        );
    }
    if a.get("--out").is_none() {
        println!("{}", pretty(&g));
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> (crate::testutil::TempDir, PathBuf) {
        let d = crate::testutil::TempDir::new("depgraph");
        let files = [
            ("src/util/str.h", "int len(const char*);\n"),
            ("src/util/str.cc", "#include \"str.h\"\nint len(const char*s){return 0;}\n"),
            ("src/core/parse.h", "#include \"util/str.h\"\nint parse();\n"),
            (
                "src/core/parse.cc",
                "#include \"core/parse.h\"\n#include \"core/emit.h\"\nint parse(){return 1;}\n",
            ),
            ("src/core/emit.h", "int emit();\n"),
            (
                "src/core/emit.cc",
                "#include \"core/emit.h\"\n#include \"io/out.h\"\nint emit(){return 2;}\n",
            ),
            ("src/io/out.h", "void out();\n"),
            ("src/io/out.cc", "#include \"io/out.h\"\n#include \"core/parse.h\"\nvoid out(){}\n"),
            (
                "src/app/main.cc",
                "#include \"core/parse.h\"\n#include <vector>\nint main(){return parse();}\n",
            ),
        ];
        let root = norm(d.path());
        let mut db = Vec::new();
        for (rel, text) in files {
            d.write(rel, text);
            if rel.ends_with(".cc") {
                db.push(json!({"directory": root.to_string_lossy(), "file": rel, "command": format!("c++ -Isrc -c {rel} -o {rel}.o")}));
            }
        }
        d.write("cc.json", &canon(&Value::Array(db)));
        (d, root)
    }

    #[test]
    fn levels_cycles_and_order() {
        let (_d, root) = tree();
        let g = build(&load_tus(Some(&root.join("cc.json")), None, &root).unwrap(), &root, 2, &[], None);
        let c = &g["components"];
        assert_eq!(c["src/util"]["level"], 0, "{c}");
        assert_eq!(c["src/core"]["level"], 1);
        assert_eq!(c["src/io"]["level"], 1); // cycle core <-> io
        assert_eq!(g["cycles"], json!([["src/core", "src/io"]]));
        assert_eq!(c["src/app"]["level"], 2);
        assert_eq!(g["port_order"][0], "src/util");
        assert_eq!(g["port_order"][3], "src/app");
        assert_eq!(g["translation_units"], 5);
    }

    #[test]
    fn without_compile_database() {
        let (_d, root) = tree();
        let g = build(&load_tus(None, Some(&root.join("src")), &root).unwrap(), &root, 2, &[], None);
        assert_eq!(g["components"]["src/util"]["level"], 0);
        assert_eq!(g["port_order"].as_array().unwrap().last().unwrap(), "src/app");
    }

    #[test]
    fn mapping_and_splitting() {
        let (_d, root) = tree();
        let map = vec![
            ("src/core/*".to_string(), "core".to_string()),
            ("src/io/*".to_string(), "core".to_string()),
        ];
        let g = build(&load_tus(Some(&root.join("cc.json")), None, &root).unwrap(), &root, 2, &map, None);
        assert_eq!(g["cycles"], json!([]));
        assert_eq!(g["components"]["core"]["level"], 1);
        assert_eq!(split_posix(r#"c++ -I"a b" -DX='1 2' x\ y"#), vec!["c++", "-Ia b", "-DX=1 2", "x y"]);
        assert_eq!(
            split_windows(r#"cl /I"C:\a b" /Fo"o\x.obj" C:\s.cc"#),
            vec!["cl", r"/IC:\a b", r"/Foo\x.obj", r"C:\s.cc"]
        );
    }

    #[test]
    fn source_with_bom() {
        let d = crate::testutil::TempDir::new("depgraph-bom");
        d.write("src/a/x.cc", "\u{feff}#include \"b/y.h\"\r\nint x();\r\n");
        d.write("src/b/y.h", "int y();\n");
        let root = norm(d.path());
        let g = build(&load_tus(None, Some(&root.join("src")), &root).unwrap(), &root, 2, &[], None);
        assert_eq!(g["components"]["src/a"]["depends_on"], json!(["src/b"]), "{g}");
    }

    /// A Windows compile database spells paths as the build saw them: other letter case, `/`,
    /// cl.exe flags. They are still the files under --root, and each file is one node.
    #[cfg(windows)]
    #[test]
    fn windows_paths_in_any_case() {
        let (d, root) = tree();
        let up = root.to_string_lossy().to_uppercase();
        let low = root.to_string_lossy().to_lowercase();
        let db: Vec<Value> = [
            "src/util/str.cc",
            "src/core/parse.cc",
            "src/core/emit.cc",
            "src/io/out.cc",
            "src/app/main.cc",
        ]
        .iter()
        .map(|rel| {
            json!({"directory": up.replace('\\', "/"), "file": rel,
                       "command": format!("cl.exe /nologo /TP -I{up}\\SRC \"/I{low}\\src\" /c {rel} /Fo{rel}.obj")})
        })
        .collect();
        d.write("cc-win.json", &canon(&Value::Array(db)));
        let tus = load_tus(Some(&root.join("cc-win.json")), None, &root).unwrap();
        let g = build(&tus, &root, 2, &[], None);
        let c = &g["components"];
        assert_eq!(g["translation_units"], 5, "{g}");
        assert_eq!(g["port_order"], json!(["src/util", "src/core", "src/io", "src/app"]), "{g}");
        assert_eq!(c["src/util"]["files"].as_array().unwrap().len(), 2, "{c}"); // str.h once
        assert_eq!(c["src/core"]["level"], 1);
        assert_eq!(g["cycles"], json!([["src/core", "src/io"]]));
        assert_eq!(c["src/app"]["level"], 2);
    }
}
