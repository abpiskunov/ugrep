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

/// Trace strings are JSON strings of the bytes; pathnames and arguments are plain ASCII/UTF-8 in the corpus.
fn bytes(v: &Value) -> Bytes {
    v.as_str().unwrap().as_bytes().to_vec()
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
    let list = files.strip_prefix(b"ignore_files=").unwrap_or(b"");
    f.ignore_files = list.split(|&c| c == b'\n').filter(|x| !x.is_empty()).map(|x| x.to_vec()).collect();
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

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: src_indexer-harness <recorded.jsonl> <out.jsonl> [functions...]");
        std::process::exit(2);
    }
    let only: Vec<&str> = if a.len() > 3 { a[3..].iter().flat_map(|s| s.split(',')).collect() } else { ALL.to_vec() };
    let n = replay::replay_trace(a[1].as_ref(), a[2].as_ref(), &only, |phase, func, args, _replay, tap| {
        if phase == replay::Phase::Ret {
            return;
        }
        let args = args.as_array().unwrap();
        // functions that end the process (`Exit::exited`) record a call and nothing else
        let finish = |tap: &mut tap::Tap, exit: Exit| {
            if !exit.exited {
                tap.state(func, &exit.warnings);
                tap.ret(func, &Null);
            }
        };
        match func {
            "src_indexer.options" => {
                let (fl, argc) = (bytes(&args[0]), args[1].as_i64().unwrap());
                let argv: Vec<Bytes> = args[2].as_array().unwrap().iter().map(bytes).collect();
                let arg_path = opt_bytes(&args[3]);
                let warnings = args[4].as_u64().unwrap() as usize;
                let argv_n: Vec<&[u8]> = argv.iter().map(|x| x.as_slice()).collect();
                tap.call(func, &[&Text(fl.clone()), &(argc as i32), &argv_n, &arg_path.as_deref(), &warnings]);
                let r = indexer_cli::options(&decode_flags(&fl), &argv, arg_path.as_deref(), warnings);
                if !r.exit.exited {
                    tap.state(func, &Text(encode_flags(&r.flags)));
                    tap.ret(func, &Null);
                }
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
                finish(tap, r);
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
                tap.state(func, &Text(state.into_bytes()));
                tap.ret(func, &r.ok);
                if r.handle != 0 {
                    indexer_core::stream_drop(r.handle);
                }
            }
            other => panic!("unknown seam function {other}"),
        }
    })
    .unwrap();
    eprintln!("replayed {n} calls");
}
