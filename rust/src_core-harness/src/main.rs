//! Replay harness for `src_core`: replays recorded C++ seam traces against the Rust functions.
//! Usage: src_core-harness <recorded.jsonl> <out.jsonl> [function ids...]
//! Defaults to every switchable seam function. Not linked into any product binary.
//!
//! The recorded flag snapshots are the strings of `core_tap::cnf_flags` / `core_tap::stats_flags`
//! (src/core_tap.hpp); this file decodes them into the types of `signatures.rs`. The entry functions
//! (`ugrep`, `terminal`, ...) are tapped in the C++ but not switchable, so they are not replayed.
mod replay;
mod tap;

#[allow(dead_code)]
#[path = "../../src_core/src/signatures.rs"]
mod signatures;

use serde_json::Value;
use signatures::{cnf, stats, Bytes, CnfFlags, StatsInput, Terms};
use tap::{quote, ToNeutral};

/// Trace strings carry one char per byte (U+0000..U+00FF).
fn bytes(v: &Value) -> Bytes {
    v.as_str().unwrap().chars().map(|c| c as u32 as u8).collect()
}

fn terms(v: &Value) -> Terms {
    v.as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_array().unwrap().iter().map(|p| if p.is_null() { None } else { Some(bytes(p)) }).collect())
        .collect()
}

struct TermsN<'a>(&'a Terms);
impl ToNeutral for TermsN<'_> {
    fn to_neutral(&self) -> String {
        let t: Vec<String> = self
            .0
            .iter()
            .map(|term| {
                let p: Vec<String> = term.iter().map(|p| p.as_ref().map_or("null".to_string(), |b| quote(b))).collect();
                format!("[{}]", p.join(","))
            })
            .collect();
        format!("[{}]", t.join(","))
    }
}

struct Null;
impl ToNeutral for Null {
    fn to_neutral(&self) -> String {
        "null".into()
    }
}

fn split_items(s: &[u8], sep: u8) -> Vec<Bytes> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split(|&c| c == sep).map(|x| x.to_vec()).collect()
}

fn num(v: &[u8]) -> usize {
    std::str::from_utf8(v).unwrap().parse().unwrap()
}

fn cnf_flags(s: &[u8]) -> CnfFlags {
    let mut f = CnfFlags::default();
    // `file=` is last and holds items each followed by '\n', which may contain ';'
    let at = s.windows(6).position(|w| w == b";file=").map(|i| i + 1).unwrap_or(s.len());
    let (head, file) = s.split_at(at);
    for kv in head.split(|&c| c == b';').filter(|x| !x.is_empty()) {
        let eq = kv.iter().position(|&c| c == b'=').unwrap();
        let (k, v) = (&kv[..eq], &kv[eq + 1..]);
        let b = v == b"1";
        match k {
            b"mode" => f.mode = num(v) as u8,
            b"line_regexp" => f.line_regexp = b,
            b"word_regexp" => f.word_regexp = b,
            b"bool" => f.boolean = b,
            b"files" => f.files = b,
            b"invert_match" => f.invert_match = b,
            b"ignore_case" => f.ignore_case = b,
            b"best_match" => f.best_match = b,
            b"fuzzy" => f.fuzzy = num(v),
            b"have_pcre2" => f.have_pcre2 = b,
            _ => panic!("unknown cnf flag {}", String::from_utf8_lossy(k)),
        }
    }
    let list = file.strip_prefix(b"file=").unwrap_or(b"");
    f.file = list.split(|&c| c == b'\n').filter(|x| !x.is_empty()).map(|x| x.to_vec()).collect();
    f
}

fn stats_input(a: &[Value]) -> StatsInput {
    let n = |i: usize| a[i].as_u64().unwrap() as usize;
    let mut s = StatsInput {
        searched_files: n(1),
        searched_dirs: n(2),
        searched_indexed: n(3),
        searched_skipped: n(4),
        searched_changed: n(5),
        searched_added: n(6),
        searched_lines: n(7),
        found_files: n(8),
        found_parts: n(9),
        found_matches: n(10),
        warnings: n(11),
        threads: n(12),
        elapsed_ms: a[13].as_str().unwrap().parse().unwrap(),
        ignore: split_items(&bytes(&a[14]), b'\n'),
        ..Default::default()
    };
    let flags = bytes(&a[0]);
    for kv in flags.split(|&c| c == b'\n').filter(|x| !x.is_empty()) {
        let eq = kv.iter().position(|&c| c == b'=').unwrap();
        let (k, v) = (&kv[..eq], &kv[eq + 1..]);
        let b = v == b"1";
        let list = || split_items(v, 0x1f);
        let opt = || if v == b"-" { None } else { Some(v[1..].to_vec()) };
        match k {
            b"query" => s.query = b,
            b"pager" => s.pager = b,
            b"quiet" => s.quiet = b,
            b"files_with_matches" => s.files_with_matches = b,
            b"files_without_match" => s.files_without_match = b,
            b"ungroup" => s.ungroup = b,
            b"count" => s.count = b,
            b"only_matching" => s.only_matching = b,
            b"format" => s.format = b,
            b"index" => s.index = b,
            b"config" => s.config = opt(),
            b"config_files" => s.config_files = list(),
            b"fuzzy" => s.fuzzy = num(v),
            b"boolean" => s.boolean = b,
            b"files" => s.files = b,
            b"mode" => s.mode = num(v) as u8,
            b"decompress" => s.decompress = b,
            b"zmax" => s.zmax = num(v),
            b"min_depth" => s.min_depth = num(v),
            b"max_depth" => s.max_depth = num(v),
            b"dereference" => s.dereference = b,
            b"no_dereference" => s.no_dereference = b,
            b"devices_action" => s.devices_action = num(v) as u8,
            b"directories_action" => s.directories_action = num(v) as u8,
            b"glob_ignore_case" => s.glob_ignore_case = b,
            b"hidden" => s.hidden = b,
            b"hidden_default" => s.hidden_default = b,
            b"ignore_files" => s.ignore_files = list(),
            b"min_count" => s.min_count = num(v),
            b"max_count" => s.max_count = num(v),
            b"max_files" => s.max_files = num(v),
            b"min_line" => s.min_line = num(v),
            b"max_line" => s.max_line = num(v),
            b"sort" => s.sort = opt(),
            b"min_size" => s.min_size = num(v),
            b"max_size" => s.max_size = num(v),
            b"file_magic" => s.file_magic = list(),
            b"include_fs" => s.include_fs = list(),
            b"exclude_fs" => s.exclude_fs = list(),
            b"all_include" => s.all_include = list(),
            b"all_exclude" => s.all_exclude = list(),
            b"all_include_dir" => s.all_include_dir = list(),
            b"all_exclude_dir" => s.all_exclude_dir = list(),
            b"include_iglob_size" => s.include_iglob_size = num(v),
            b"exclude_iglob_size" => s.exclude_iglob_size = num(v),
            b"include_iglob_dir_size" => s.include_iglob_dir_size = num(v),
            b"exclude_iglob_dir_size" => s.exclude_iglob_dir_size = num(v),
            b"crlf" => s.crlf = b,
            b"pathsep" => s.pathsep = num(v) as u8,
            _ => panic!("unknown stats flag {}", String::from_utf8_lossy(k)),
        }
    }
    s
}

const ALL: [&str; 9] = [
    "src_core.cnf_compile",
    "src_core.cnf_new_pattern",
    "src_core.cnf_prune",
    "src_core.cnf_split",
    "src_core.cnf_adjoin",
    "src_core.cnf_first",
    "src_core.cnf_report",
    "src_core.cnf_quote",
    "src_core.cnf_anchor",
];

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: src_core-harness <recorded.jsonl> <out.jsonl> [functions...]");
        std::process::exit(2);
    }
    let mut all: Vec<&str> = ALL.to_vec();
    all.push("src_core.stats_report");
    let only: Vec<&str> = if a.len() > 3 { a[3..].iter().flat_map(|s| s.split(',')).collect() } else { all };
    let n = replay::replay_trace(a[1].as_ref(), a[2].as_ref(), &only, |phase, func, args, _replay, tap| {
        if phase == replay::Phase::Ret {
            return;
        }
        let args = args.as_array().unwrap();
        match func {
            "src_core.cnf_compile" => {
                let (t, p, fl) = (terms(&args[0]), bytes(&args[1]), bytes(&args[2]));
                tap.call(func, &[&TermsN(&t), &p.as_slice(), &fl.as_slice()]);
                let r = cnf::compile(t, &p, &cnf_flags(&fl));
                tap.state(func, &TermsN(&r));
                tap.ret(func, &Null);
            }
            "src_core.cnf_new_pattern" => {
                let (t, mask, p, fl) = (terms(&args[0]), args[1].as_i64().unwrap() as i32, bytes(&args[2]), bytes(&args[3]));
                tap.call(func, &[&TermsN(&t), &mask, &p.as_slice(), &fl.as_slice()]);
                let r = cnf::new_pattern(t, mask, &p, &cnf_flags(&fl));
                tap.state(func, &TermsN(&r));
                tap.ret(func, &Null);
            }
            "src_core.cnf_prune" | "src_core.cnf_split" => {
                let (t, fl) = (terms(&args[0]), bytes(&args[1]));
                tap.call(func, &[&TermsN(&t), &fl.as_slice()]);
                let flags = cnf_flags(&fl);
                let r = if func == "src_core.cnf_prune" { cnf::prune(t, &flags) } else { cnf::split(t, &flags) };
                tap.state(func, &TermsN(&r));
                tap.ret(func, &Null);
            }
            "src_core.cnf_adjoin" | "src_core.cnf_report" => {
                let (t, fl) = (terms(&args[0]), bytes(&args[1]));
                tap.call(func, &[&TermsN(&t), &fl.as_slice()]);
                let flags = cnf_flags(&fl);
                let r = if func == "src_core.cnf_adjoin" { cnf::adjoin(&t, &flags) } else { cnf::report(&t, &flags) };
                tap.ret(func, &r.as_slice());
            }
            "src_core.cnf_first" => {
                let t = terms(&args[0]);
                tap.call(func, &[&TermsN(&t)]);
                let r = cnf::first(&t);
                tap.ret(func, &r.as_slice());
            }
            "src_core.cnf_quote" => {
                let p = bytes(&args[0]);
                tap.call(func, &[&p.as_slice()]);
                let r = cnf::quote(&p);
                tap.state(func, &r.as_slice());
                tap.ret(func, &Null);
            }
            "src_core.cnf_anchor" => {
                let (p, fl) = (bytes(&args[0]), bytes(&args[1]));
                tap.call(func, &[&p.as_slice(), &fl.as_slice()]);
                let r = cnf::anchor(&p, &cnf_flags(&fl));
                // the recorded state is the pattern only; `enable_empty` is not observed in the C++ tap
                tap.state(func, &r.pattern.as_slice());
                tap.ret(func, &Null);
            }
            "src_core.stats_report" => {
                let input = stats_input(args);
                let rec: Vec<Value> = args.clone();
                let (flags, ignore) = (bytes(&rec[0]), bytes(&rec[14]));
                let (flags, ignore) = (flags.as_slice(), ignore.as_slice());
                let counters: Vec<usize> = (1..=12).map(|i| rec[i].as_u64().unwrap() as usize).collect();
                let mut call: Vec<&dyn ToNeutral> = vec![&flags];
                call.extend(counters.iter().map(|c| c as &dyn ToNeutral));
                call.push(&input.elapsed_ms);
                call.push(&ignore);
                tap.call(func, &call);
                let r = stats::report(&input);
                tap.ret(func, &r.as_slice());
            }
            other => panic!("unknown seam function {other}"),
        }
    })
    .unwrap();
    eprintln!("replayed {n} calls");
}
