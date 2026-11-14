//! replay.rs - replay driver for the Rust side. TEMPLATE: copy into the harness crate next to
//! `tap.rs`. Needs `serde_json` (harness crate only).
//!
//! Reads a recorded C++ seam trace and, for every inbound `call` event, hands the function id,
//! the recorded arguments and the recorded answers of its outbound calls to `dispatch`.
//! `dispatch` is the only generated part: one arm per seam function that decodes the arguments,
//! calls the Rust function with a dependency stub backed by `Replay`, and records state and
//! result through `Tap`. The Rust trace is then compared with the C++ trace by `cpp2rust-tools trace-diff`.
//! Functions outside `only` (not ported yet) are skipped; pass the same list to `--fns`.
//!
//! Every dispatched call is handed to `dispatch` twice: `Phase::Call` at its recorded `call`, `Phase::Ret` at its
//! recorded `ret`. Inbound calls recorded between the two (the legacy body called other seam functions, e.g.
//! `zs_open` calls `zs_close` and the `zs_is_*` predicates) are dispatched in between, so an arm that emits its `call`
//! in `Phase::Call` and runs the Rust and emits its `ret` in `Phase::Ret` reproduces the recorded nesting. Arms
//! without nesting may do all their work in `Phase::Call` and ignore `Phase::Ret`.
use crate::tap::{Recorded, Replay, Tap};
use serde_json::Value;
use std::collections::VecDeque;
use std::io::BufRead;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Call,
    Ret,
}

/// A dispatched call waiting for its recorded `ret`.
struct Open {
    func: String,
    thread: Option<Value>,
    args: Value,
    replay: Replay,
}

pub fn replay_trace(
    recorded: &std::path::Path,
    out: &std::path::Path,
    only: &[&str],
    mut dispatch: impl FnMut(Phase, &str, &Value, &mut Replay, &mut Tap),
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
    let mut open: Vec<Open> = Vec::new();
    let mut i = 0;
    while i < events.len() {
        let e = &events[i];
        i += 1;
        // calls of the previous case that have no recorded ret end with their case
        if tap.as_ref().is_some_and(|(c, _)| *c != s(e, "case")) {
            let t = &mut tap.as_mut().unwrap().1;
            while let Some(mut o) = open.pop() {
                dispatch(Phase::Ret, &o.func, &o.args, &mut o.replay, t);
            }
        }
        if s(e, "kind") == "ret" {
            if open.last().is_some_and(|o| o.func == s(e, "fn") && o.thread.as_ref() == e.get("thread")) {
                let mut o = open.pop().unwrap();
                dispatch(Phase::Ret, &o.func, &o.args, &mut o.replay, &mut tap.as_mut().unwrap().1);
            }
            continue;
        }
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
        let mut o = Open { func, thread, args: e["args"].clone(), replay: Replay { answers } };
        dispatch(Phase::Call, &o.func, &o.args, &mut o.replay, &mut tap.as_mut().unwrap().1);
        open.push(o);
        calls += 1;
    }
    if let Some((_, t)) = tap.as_mut() {
        while let Some(mut o) = open.pop() {
            dispatch(Phase::Ret, &o.func, &o.args, &mut o.replay, t);
        }
    }
    Ok(calls)
}
