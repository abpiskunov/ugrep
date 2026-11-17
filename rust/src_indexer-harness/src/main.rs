//! Replay harness for `src_indexer`: replays recorded C++ seam traces against the Rust functions.
//! Usage: src_indexer-harness <recorded.jsonl> <out.jsonl> [function ids...]
//! Defaults to the four seam functions. Not linked into any product binary.
//!
//! The recorded flag snapshot is the string of `indexer_tap::flags` (src/indexer_tap.hpp); this file decodes it into
//! `IndexerFlags` of `signatures.rs` and encodes the result back the way the C++ tap does. `index` is replayed with
//! stream handle 0 (a new file): the recorded call carries no handle.
mod replay;
mod tap;

#[path = "../../src_indexer/src/signatures.rs"]
mod signatures;

use serde_json::Value;
use signatures::{indexer_cli, indexer_core, indexer_walk, Bytes, Exit, IndexerFlags};
use tap::{quote, ToNeutral};

/// Trace strings carry one byte per character: the tap writes every byte as the code point of the same value
/// (Latin-1), so a non-ASCII pathname such as UTF-8 `caf\xc3\xa9.txt` arrives as two characters, not one.
fn bytes(v: &Value) -> Bytes {
    v.as_str().unwrap().chars().map(|c| c as u32 as u8).collect()
}

fn opt_bytes(v: &Value) -> Option<Bytes> {
    if v.is_null() {
        None
    } else {
        Some(bytes(v))
    }
}

fn num(v: &[u8]) -> usize {
    std::str::from_utf8(v).unwrap().parse().unwrap()
}

fn decode_flags(s: &[u8]) -> IndexerFlags {
    let mut f = IndexerFlags {
        accuracy: 4,
        check: false,
        decompress: false,
        delete: false,
        dereference_files: false,
        force: false,
        hidden: false,
        ignore_binary: false,
        no_messages: false,
        quiet: false,
        usage_warnings: false,
        verbose: false,
        zmax: 1,
        ignore_files: Vec::new(),
    };
    // `ignore_files=` is last: each name is followed by '\n' and may contain ';'
    let at = s.windows(14).position(|w| w == b";ignore_files=").map(|i| i + 1).unwrap_or(s.len());
    let (head, files) = s.split_at(at);
    for kv in head.split(|&c| c == b';').filter(|x| !x.is_empty()) {
        let eq = kv.iter().position(|&c| c == b'=').unwrap();
        let (k, v) = (&kv[..eq], &kv[eq + 1..]);
        let b = v == b"1";
        match k {
            b"accuracy" => f.accuracy = num(v) as i32,
            b"check" => f.check = b,
            b"decompress" => f.decompress = b,
            b"delete" => f.delete = b,
            b"dereference_files" => f.dereference_files = b,
            b"force" => f.force = b,
            b"hidden" => f.hidden = b,
            b"ignore_binary" => f.ignore_binary = b,
            b"no_messages" => f.no_messages = b,
            b"quiet" => f.quiet = b,
            b"usage_warnings" => f.usage_warnings = b,
            b"verbose" => f.verbose = b,
            b"zmax" => f.zmax = num(v),
            _ => panic!("unknown flag {}", String::from_utf8_lossy(k)),
        }
    }
    // every name ends with '\n'; only that final one is a terminator, so an empty name stays
    let list = files.strip_prefix(b"ignore_files=").unwrap_or(b"");
    let list = list.strip_suffix(b"\n").unwrap_or(list);
    if !files.is_empty() && files.len() > b"ignore_files=".len() {
        f.ignore_files = list.split(|&c| c == b'\n').map(|x| x.to_vec()).collect();
    }
    f
}

fn encode_flags(f: &IndexerFlags) -> Bytes {
    let b = |v: bool| if v { 1 } else { 0 };
    let mut s = format!(
        "accuracy={};check={};decompress={};delete={};dereference_files={};force={};hidden={};ignore_binary={};no_messages={};quiet={};usage_warnings={};verbose={};zmax={};ignore_files=",
        f.accuracy,
        b(f.check),
        b(f.decompress),
        b(f.delete),
        b(f.dereference_files),
        b(f.force),
        b(f.hidden),
        b(f.ignore_binary),
        b(f.no_messages),
        b(f.quiet),
        b(f.usage_warnings),
        b(f.verbose),
        f.zmax
    )
    .into_bytes();
    for file in &f.ignore_files {
        s.extend_from_slice(file);
        s.push(b'\n');
    }
    s
}

/// FNV-1a of the table bytes, as in `indexer_tap::indexed`.
fn fnv(data: &[u8]) -> u64 {
    data.iter().fold(14695981039346656037u64, |h, &c| (h ^ c as u64).wrapping_mul(1099511628211))
}

struct Null;
impl ToNeutral for Null {
    fn to_neutral(&self) -> String {
        "null".into()
    }
}

/// An already encoded text state: quoted as a string like the C++ tap does.
struct Text(Bytes);
impl ToNeutral for Text {
    fn to_neutral(&self) -> String {
        quote(&self.0)
    }
}

const ALL: [&str; 4] = ["src_indexer.options", "src_indexer.indexer", "src_indexer.deleter", "src_indexer.index"];

fn from_hex(h: &str) -> Vec<u8> {
    (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect()
}

/// The file system of the recorded cases: `run-cases` writes the `files` of a case into a fresh directory and runs the
/// binary there. Each case is replayed in a fresh temp directory with the same files, as the current directory, so
/// `indexer(NULL)` walks only that tree. `cases.jsonl` is `CPP2RUST_INDEXER_CASES` or else
/// `<directory of the recorded trace>/../cases.jsonl`.
struct CaseFs {
    cases_path: std::path::PathBuf,
    cases: Option<std::collections::HashMap<String, Vec<(String, Vec<u8>)>>>,
    case: String,
    base: std::path::PathBuf,
}

impl CaseFs {
    fn new(recorded: &std::path::Path) -> CaseFs {
        let cases_path = std::env::var_os("CPP2RUST_INDEXER_CASES").map(std::path::PathBuf::from).unwrap_or_else(|| {
            recorded.parent().unwrap_or(std::path::Path::new(".")).join("..").join("cases.jsonl")
        });
        let base = std::env::temp_dir().join(format!("src_indexer_replay_{}", std::process::id()));
        CaseFs { cases_path, cases: None, case: String::new(), base }
    }

    fn load(path: &std::path::Path) -> std::collections::HashMap<String, Vec<(String, Vec<u8>)>> {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("src_indexer cases {}: {e} (set CPP2RUST_INDEXER_CASES)", path.display()));
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let c: Value = serde_json::from_str(l).unwrap();
                let mut files: Vec<(String, Vec<u8>)> = c
                    .get("files")
                    .and_then(Value::as_object)
                    .map(|o| {
                        o.iter()
                            .map(|(k, v)| {
                                let data = match v.get("hex").and_then(Value::as_str) {
                                    Some(h) => from_hex(h),
                                    None => v.as_str().unwrap_or("").as_bytes().to_vec(),
                                };
                                (k.clone(), data)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                // sorted key order: the seeded `._UG#_Store` files sort after the files they cover
                files.sort();
                (c["id"].as_str().unwrap().to_string(), files)
            })
            .collect()
    }

    /// Makes `case` the current case: a fresh directory holding its files becomes the current directory.
    fn enter(&mut self, case: &str) {
        if self.case == case {
            return;
        }
        self.leave();
        self.case = case.to_string();
        let path = &self.cases_path;
        let cases = self.cases.get_or_insert_with(|| CaseFs::load(path));
        let files = cases.get(case).unwrap_or_else(|| panic!("no src_indexer case {case} in {}", self.cases_path.display()));
        let cwd = self.base.join("w");
        for (rel, data) in files {
            let p = cwd.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, data).unwrap();
        }
        std::fs::create_dir_all(&cwd).unwrap();
        std::env::set_current_dir(&cwd).unwrap();
    }

    fn leave(&mut self) {
        if self.base.exists() {
            let _ = std::env::set_current_dir(std::env::temp_dir());
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }
}

/// What a dispatched call produced at its `Phase::Call`, emitted at its `Phase::Ret`.
enum Pending {
    /// the function ended the process: the recording has no state or ret
    Exited,
    Text(Bytes),
    Null(usize),
    Index { state: String, ok: bool, handle: usize },
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: src_indexer-harness <recorded.jsonl> <out.jsonl> [functions...]");
        std::process::exit(2);
    }
    let only: Vec<&str> = if a.len() > 3 { a[3..].iter().flat_map(|s| s.split(',')).collect() } else { ALL.to_vec() };
    // every case runs in its own current directory (`CaseFs::enter`), so relative paths would resolve elsewhere
    let recorded = std::path::absolute(&a[1]).expect("recorded trace path");
    let out = std::path::absolute(&a[2]).expect("output path");
    let mut fs = CaseFs::new(&recorded);
    // results of the calls between their Phase::Call and Phase::Ret; the recorded nesting is last in, first out
    let mut pending: Vec<Pending> = Vec::new();
    let n = replay::replay_trace(&recorded, &out, &only, |phase, func, args, _replay, tap| {
        if phase == replay::Phase::Ret {
            match pending.pop().expect("Ret without Call") {
                Pending::Exited => {}
                Pending::Text(flags) => {
                    tap.state(func, &Text(flags));
                    tap.ret(func, &Null);
                }
                Pending::Null(warnings) => {
                    tap.state(func, &warnings);
                    tap.ret(func, &Null);
                }
                Pending::Index { state, ok, handle } => {
                    tap.state(func, &Text(state.into_bytes()));
                    tap.ret(func, &ok);
                    if handle != 0 {
                        indexer_core::stream_drop(handle as u64);
                    }
                }
            }
            return;
        }
        fs.enter(tap.case());
        let args = args.as_array().unwrap();
        // functions that end the process (`Exit::exited`) record a call and nothing else
        let finish = |exit: Exit| if exit.exited { Pending::Exited } else { Pending::Null(exit.warnings) };
        match func {
            "src_indexer.options" => {
                let (fl, argc) = (bytes(&args[0]), args[1].as_i64().unwrap());
                let argv: Vec<Bytes> = args[2].as_array().unwrap().iter().map(bytes).collect();
                let arg_path = opt_bytes(&args[3]);
                let warnings = args[4].as_u64().unwrap() as usize;
                let argv_n: Vec<&[u8]> = argv.iter().map(|x| x.as_slice()).collect();
                tap.call(func, &[&Text(fl.clone()), &(argc as i32), &argv_n, &arg_path.as_deref(), &warnings]);
                let r = indexer_cli::options(&decode_flags(&fl), &argv, arg_path.as_deref(), warnings);
                pending.push(if r.exit.exited { Pending::Exited } else { Pending::Text(encode_flags(&r.flags)) });
            }
            "src_indexer.indexer" | "src_indexer.deleter" => {
                let (fl, path) = (bytes(&args[0]), opt_bytes(&args[1]));
                let warnings = args[2].as_u64().unwrap() as usize;
                tap.call(func, &[&Text(fl.clone()), &path.as_deref(), &warnings]);
                let flags = decode_flags(&fl);
                let r = if func == "src_indexer.indexer" {
                    indexer_walk::indexer(&flags, path.as_deref(), warnings)
                } else {
                    indexer_walk::deleter(&flags, path.as_deref(), warnings)
                };
                pending.push(finish(r));
            }
            "src_indexer.index" => {
                let (fl, path, archive) = (bytes(&args[0]), bytes(&args[1]), args[2].as_bool().unwrap());
                tap.call(func, &[&Text(fl.clone()), &path.as_slice(), &archive]);
                let r = indexer_core::index(&decode_flags(&fl), 0, &path, archive);
                let state = format!(
                    "hashes_size={};hashes_fnv={};noise={};compressed={};archive={};binary={};size={}",
                    r.hashes.len(),
                    fnv(&r.hashes),
                    tap::format_g17(r.noise as f64),
                    r.compressed as u8,
                    r.archive as u8,
                    r.binary as u8,
                    r.size
                );
                pending.push(Pending::Index { state, ok: r.ok, handle: r.handle as usize });
            }
            other => panic!("unknown seam function {other}"),
        }
    })
    .unwrap();
    fs.leave();
    eprintln!("replayed {n} calls");
}
