//! replay.rs - replay driver for the Rust side. TEMPLATE: copy into the harness crate next to
//! `tap.rs`. Needs `serde_json` (harness crate only).
//!
//! Reads a recorded C++ seam trace and, for every inbound `call` event, hands the function id,
//! the recorded arguments and the recorded answers of its outbound calls to `dispatch`.
//! `dispatch` is the only generated part: one arm per seam function that decodes the arguments,
//! calls the Rust function with a dependency stub backed by `Replay`, and records state and
//! result through `Tap`. The Rust trace is then compared with the C++ trace by `cpp2rust-tools trace-diff`.
//! Functions outside `only` (not ported yet) are skipped; pass the same list to `--fns`.
use crate::tap::{Recorded, Replay, Tap};
use serde_json::Value;
use std::collections::VecDeque;
use std::io::BufRead;

pub fn replay_trace(
    recorded: &std::path::Path,
    out: &std::path::Path,
    only: &[&str],
    mut dispatch: impl FnMut(&str, &Value, &mut Replay, &mut Tap),
) -> std::io::Result<usize> {
    let _ = std::fs::remove_file(out);
    let file = std::io::BufReader::new(std::fs::File::open(recorded)?);
    let mut events: Vec<Value> = Vec::new();
    for line in file.lines() {
        let line = line?;
        if !line.trim().is_empty() {
            events.push(serde_json::from_str(&line).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?);
        }
    }
    let s = |e: &Value, k: &str| e.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let mut calls = 0;
    let mut tap: Option<(String, Tap)> = None;
    let mut i = 0;
    while i < events.len() {
        let e = &events[i];
        i += 1;
        if s(e, "kind") != "call" || !only.contains(&s(e, "fn").as_str()) {
            continue;
        }
        let (case, func, thread) = (s(e, "case"), s(e, "fn"), e.get("thread").cloned());
        // answers of this call's outbound calls: recorded out_ret events up to the matching ret
        let mut answers = VecDeque::new();
        for r in &events[i..] {
            if s(r, "case") != case || r.get("thread") != thread.as_ref() {
                continue;
            }
            if s(r, "kind") == "ret" && s(r, "fn") == func {
                break;
            }
            if s(r, "kind") == "out_ret" {
                let key = if r.get("err").is_some() { "err" } else { "ret" };
                answers.push_back(Recorded { callee: s(r, "fn"), key: key.into(), raw: r[key].to_string() });
            }
        }
        if tap.as_ref().map(|(c, _)| c != &case).unwrap_or(true) {
            tap = Some((case.clone(), Tap::to_file(out, &case)?));
        }
        let mut replay = Replay { answers };
        dispatch(&func, &e["args"], &mut replay, &mut tap.as_mut().unwrap().1);
        calls += 1;
    }
    Ok(calls)
}
