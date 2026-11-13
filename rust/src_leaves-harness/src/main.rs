//! Replay harness of `src_leaves`: reads a recorded C++ seam trace and replays every call of the
//! listed functions against the Rust port.
//!   cargo run --profile harness -p src_leaves-harness -- <recorded.jsonl> <out.jsonl> <fn,fn,...>
mod replay;
mod tap;

// The unit under test, compiled from the component's own source (no cxx glue, no C++).
#[path = "../../src_leaves/src/signatures.rs"]
mod signatures;

use serde_json::Value;

/// Recorded text arguments are JSON strings whose chars are bytes (non-ASCII as \u00xx).
fn bytes(v: &Value) -> Vec<u8> {
    v.as_str().unwrap().chars().map(|c| c as u32 as u8).collect()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: src_leaves-harness <recorded.jsonl> <out.jsonl> [fn,fn,...]");
        std::process::exit(2);
    }
    let only: Vec<&str> = match a.get(3) {
        Some(s) => s.split(|c| c == ',' || c == ' ').filter(|s| !s.is_empty()).collect(),
        None => Vec::new(),
    };
    let n = replay::replay_trace(a[1].as_ref(), a[2].as_ref(), &only, |func, args, _replay, tap| match func {
        "src_leaves.glob_match" => {
            let (pathname, basename, glob) = (bytes(&args[0]), bytes(&args[1]), bytes(&args[2]));
            let (icase, lead, path) = (args[3].as_bool().unwrap(), args[4].as_bool().unwrap(), args[5].as_bool().unwrap());
            tap.call(func, &[&pathname.as_slice(), &basename.as_slice(), &glob.as_slice(), &icase, &lead, &path]);
            let r = signatures::glob_match(&pathname, &basename, &glob, icase, lead, path);
            tap.ret(func, &r);
        }
        other => panic!("no dispatch arm for {other}"),
    })
    .unwrap();
    eprintln!("replayed {n} calls");
}
