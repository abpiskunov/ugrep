//! Replay harness for `src_leaves`: replays recorded C++ seam traces against the Rust functions.
//! Usage: src_leaves-harness <recorded.jsonl> <out.jsonl> [function ids...]
//! Defaults to every seam function. Not linked into any product binary.
mod replay;
mod tap;

#[path = "../../src_leaves/src/signatures.rs"]
mod signatures;

use serde_json::Value;

/// Trace strings carry one char per byte (U+0000..U+00FF).
fn bytes(v: &Value) -> Vec<u8> {
    v.as_str().unwrap().chars().map(|c| c as u32 as u8).collect()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: src_leaves-harness <recorded.jsonl> <out.jsonl> [functions...]");
        std::process::exit(2);
    }
    let all = ["src_leaves.glob_match"];
    let only: Vec<&str> = if a.len() > 3 { a[3..].iter().map(String::as_str).collect() } else { all.to_vec() };
    let n = replay::replay_trace(a[1].as_ref(), a[2].as_ref(), &only, |func, args, _replay, tap| match func {
        "src_leaves.glob_match" => {
            let (pathname, basename, glob) = (bytes(&args[0]), bytes(&args[1]), bytes(&args[2]));
            let (icase, lead, path) = (args[3].as_bool().unwrap(), args[4].as_bool().unwrap(), args[5].as_bool().unwrap());
            tap.call(func, &[&pathname.as_slice(), &basename.as_slice(), &glob.as_slice(), &icase, &lead, &path]);
            let r = signatures::glob_match(&pathname, &basename, &glob, icase, lead, path);
            tap.ret(func, &r);
        }
        other => panic!("unknown seam function {other}"),
    })
    .unwrap();
    eprintln!("replayed {n} calls");
}
