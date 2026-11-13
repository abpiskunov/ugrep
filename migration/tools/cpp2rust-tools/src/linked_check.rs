//! linked-check: prove from linker maps which implementation of a component a binary holds.

use crate::util::*;
use regex::Regex;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub const HELP: &str = "\
cpp2rust-tools linked-check - prove from linker maps which implementation of a component a binary holds.

  linked-check --map build/rust/tool.map [--map ...]
               --forbid-sources \"src/parse/*.cc\" [...]   # legacy C++ of the component
               [--forbid REGEX ...]                      # e.g. cpp2rust_tap, cpp2rust_shadow
               --require REGEX [...]                     # e.g. the Rust archive or a facade symbol
               [--root .] [--out linked.json]

Works on GNU ld / lld maps (-Wl,-Map=FILE) and MSVC / lld-link maps (/MAP:FILE), because it only
looks for object-file names: a legacy source src/parse/lexer.cc shows up as lexer.cc.o, lexer.o,
lexer.obj or lexer.cc.obj. Inlining cannot hide an object file. Fails when a forbidden object or
pattern is present, when a required pattern is missing, or when a map is missing or empty.
Patterns use Rust regex syntax. Exit: 0 pass, 1 findings, 2 usage error.";

/// Matches `lexer.cc.o | lexer.o | lexer.obj | lexer.cc.obj`, not preceded by a name character.
pub struct ObjectPattern(Regex);

impl ObjectPattern {
    pub fn new(source: &str) -> ObjectPattern {
        let base = source.rsplit(['/', '\\']).next().unwrap_or(source);
        let (stem, ext) = match base.rfind('.') {
            Some(i) if i > 0 => (&base[..i], &base[i..]),
            _ => (base, ""),
        };
        let ext_part = if ext.is_empty() {
            String::new()
        } else {
            format!("(?:{})?", regex::escape(ext))
        };
        ObjectPattern(Regex::new(&format!(r"(?i){}{}\.(?:o|obj)\b", regex::escape(stem), ext_part)).unwrap())
    }

    /// First match whose preceding character is not `[A-Za-z0-9_.-]` (the old look-behind).
    pub fn find<'t>(&self, text: &'t str) -> Option<&'t str> {
        let mut pos = 0;
        while let Some(m) = self.0.find_at(text, pos) {
            let prev = text[..m.start()].chars().next_back();
            if !prev.is_some_and(|c| c.is_ascii_alphanumeric() || "_.-".contains(c)) {
                return Some(m.as_str());
            }
            pos = m.start() + text[m.start()..].chars().next().map_or(1, char::len_utf8);
        }
        None
    }
}

pub fn check(maps: &[String], forbid_sources: &[String], forbid: &[String], require: &[String], root: &str) -> R<Value> {
    let mut findings = Vec::new();
    let mut seen_required = vec![false; require.len()];
    let mut sources = BTreeSet::new();
    for pat in forbid_sources {
        let hits = glob_paths(root, pat);
        if hits.is_empty() {
            sources.insert(pat.clone()); // a deleted file still names a forbidden object
        } else {
            sources.extend(hits);
        }
    }
    let src_rx: Vec<(String, ObjectPattern)> = sources.into_iter().map(|s| (s.clone(), ObjectPattern::new(&s))).collect();
    let compile = |p: &String| Regex::new(p).map_err(|e| bad_input(format!("linked-check: pattern {p:?}: {e}")));
    let fb_rx: Vec<(String, Regex)> = forbid.iter().map(|f| Ok((f.clone(), compile(f)?))).collect::<R<_>>()?;
    let rq_rx: Vec<Regex> = require.iter().map(compile).collect::<R<_>>()?;
    for m in maps {
        let text = match read_text(m) {
            Ok(t) => t,
            Err(e) => {
                findings.push(json!({"class": "map_missing", "map": m, "detail": e.to_string()}));
                continue;
            }
        };
        if text.trim().chars().count() < 50 {
            findings.push(json!({"class": "map_empty", "map": m}));
            continue;
        }
        for (src, rx) in &src_rx {
            if let Some(hit) = rx.find(&text) {
                findings.push(json!({"class": "legacy_object_linked", "map": m, "source": src, "object": hit}));
            }
        }
        for (f, rx) in &fb_rx {
            if let Some(hit) = rx.find(&text) {
                findings.push(json!({"class": "forbidden_pattern", "map": m, "pattern": f, "text": hit.as_str()}));
            }
        }
        for (i, rx) in rq_rx.iter().enumerate() {
            if rx.is_match(&text) {
                seen_required[i] = true;
            }
        }
    }
    for (r, ok) in require.iter().zip(seen_required) {
        if !ok {
            findings.push(json!({"class": "required_missing", "pattern": r}));
        }
    }
    let result = if findings.is_empty() { "pass" } else { "fail" };
    Ok(
        json!({"schema": "cpp-rust-linked/1", "maps": maps, "legacy_sources_checked": src_rx.len(),
              "findings": findings, "result": result}),
    )
}

pub fn main(argv: &[String]) -> R<i32> {
    let spec = [
        ("--map", Kind::Multi),
        ("--forbid-sources", Kind::Multi),
        ("--forbid", Kind::Multi),
        ("--require", Kind::Multi),
        ("--root", Kind::Value),
        ("--out", Kind::Value),
    ];
    let a = Args::parse(argv, &spec)?;
    let maps = a.all("--map");
    if maps.is_empty() {
        return usage("at least one --map");
    }
    let (fs, fb, rq) = (a.all("--forbid-sources"), a.all("--forbid"), a.all("--require"));
    if (fs.is_empty() && fb.is_empty()) || rq.is_empty() {
        return usage("give --forbid-sources (or --forbid) and --require: a check that looks for nothing proves nothing");
    }
    let res = check(&maps, &fs, &fb, &rq, a.get("--root").unwrap_or("."))?;
    for f in res["findings"].as_array().unwrap() {
        println!("FINDING {}", canon(f));
    }
    println!(
        "linked-check: {} ({} maps, {} legacy sources)",
        py_str(&res["result"]).to_uppercase(),
        maps.len(),
        res["legacy_sources_checked"]
    );
    if let Some(out) = a.get("--out") {
        write_json(out, &res)?;
    }
    Ok(if res["findings"].as_array().unwrap().is_empty() { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    const GNU_CPP: &str = "Linker script and memory map
 .text          0x0000000000401000       0x1f0 CMakeFiles/tool.dir/src/main.cc.o
 .text          0x00000000004011f0       0x220 libparse.a(lexer.cc.o)
 .text          0x0000000000401410        0x90 libparse.a(parser.cc.o)
";
    const GNU_RUST: &str = "Linker script and memory map
 .text          0x0000000000401000       0x1f0 CMakeFiles/tool.dir/src/main.cc.o
 .text          0x00000000004011f0       0x220 libmigrated.a(core_parse-3f2a.core_parse.cgu0.rcgu.o)
 .text          0x0000000000401410        0x90 CMakeFiles/parse_facade.dir/gen/parse_facade.cc.o
";
    const MSVC_RUST: &str = " tool
 Timestamp is 6730a1b2
 Start         Length     Name                   Class
 0001:00000000 00012340H .text                   CODE
  Address         Publics by Value              Rva+Base               Lib:Object
 0001:00000000       main                       0000000140001000 f   main.obj
 0001:00000400       core_parse_parse_header    0000000140001400 f   migrated:core_parse.rcgu.o
 0001:00000900       ?parse@Facade@@QEAAHXZ     0000000140001900 f   parse_facade.obj
";

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn gnu_and_msvc_maps() {
        let d = TempDir::new("linked");
        let msvc_mixed = format!("{MSVC_RUST} 0001:00000a00       ?lex@@YAHXZ                0000000140001a00 f   parse:lexer.obj\n");
        for (n, t) in [
            ("gnu_cpp", GNU_CPP),
            ("gnu_rust", GNU_RUST),
            ("msvc_rust", MSVC_RUST),
            ("msvc_mixed", msvc_mixed.as_str()),
        ] {
            d.write(&format!("{n}.map"), t);
        }
        let p = |n: &str| vec![d.path().join(format!("{n}.map")).to_string_lossy().into_owned()];
        let src = s(&["src/parse/lexer.cc", "src/parse/parser.cc"]);
        let root = d.path().to_string_lossy().into_owned();
        assert_eq!(
            check(&p("gnu_rust"), &src, &s(&["cpp2rust_tap"]), &s(&[r"libmigrated\.a"]), &root).unwrap()["result"],
            "pass"
        );
        let r = check(&p("gnu_cpp"), &src, &[], &s(&[r"libmigrated\.a"]), &root).unwrap();
        let classes: BTreeSet<String> = r["findings"].as_array().unwrap().iter().map(|f| py_str(&f["class"])).collect();
        assert_eq!(classes, s(&["legacy_object_linked", "required_missing"]).into_iter().collect());
        assert_eq!(r["findings"].as_array().unwrap().len(), 3);
        assert_eq!(
            check(&p("msvc_rust"), &src, &[], &s(&["migrated:core_parse"]), &root).unwrap()["result"],
            "pass"
        );
        let r = check(&p("msvc_mixed"), &src, &[], &s(&["migrated:core_parse"]), &root).unwrap();
        assert_eq!(r["result"], "fail");
        assert_eq!(py_str(&r["findings"][0]["object"]).to_lowercase(), "lexer.obj");
        assert_eq!(
            check(&p("none"), &src, &[], &[], &root).unwrap()["findings"][0]["class"],
            "map_missing"
        );
        // parse_facade.cc.o must not match a legacy source named facade.cc
        assert_eq!(
            check(&p("gnu_rust"), &s(&["src/facade.cc"]), &[], &[], &root).unwrap()["result"],
            "pass"
        );
    }

    #[test]
    fn sources_are_globbed_under_root() {
        let d = TempDir::new("linked-glob");
        d.write("src/parse/lexer.cc", "");
        d.write("src/parse/sub/deep.cc", "");
        d.write("map.map", GNU_CPP);
        let root = d.path().to_string_lossy().into_owned();
        let map = vec![d.path().join("map.map").to_string_lossy().into_owned()];
        let r = check(&map, &s(&["src/parse/*.cc"]), &[], &s(&["nothing"]), &root).unwrap();
        assert_eq!(r["legacy_sources_checked"], 1); // `*` stays in one directory
        let r = check(&map, &s(&["src/**/*.cc"]), &[], &s(&["nothing"]), &root).unwrap();
        assert_eq!(r["legacy_sources_checked"], 2);
        if cfg!(windows) {
            // file names ignore letter case on Windows, like the shell and Python's glob there
            let r = check(&map, &s(&[r"src\Parse\**\*.CC"]), &[], &s(&["nothing"]), &root).unwrap();
            assert_eq!(r["legacy_sources_checked"], 2, "{r}");
        }
    }
}
