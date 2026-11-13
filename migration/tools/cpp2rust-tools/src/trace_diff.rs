//! trace-diff: compare two cpp2rust-trace/1 event streams; learn noise.

use crate::util::*;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{BufRead, BufReader, Read};

pub const HELP: &str = "\
cpp2rust-tools trace-diff - compare two cpp2rust-trace/1 event streams (JSON Lines, optionally .gz).

  trace-diff diff  --a cpp.jsonl --b rust.jsonl [--masks m.json ...] [--fns f1,f2]
                   [--out summary.json] [--context 5] [--allow-unapproved]
  trace-diff noise --a cpp_run1.jsonl --b cpp_run2.jsonl [--out proposed-masks.json]

diff : exit 0 when every case matches after approved masks, 1 on any divergence, 2 on bad input.
noise: both inputs come from the SAME implementation (C++ run twice with different seed,
       temp dir, time). Fields that differ become PROPOSED masks (\"approved\": null). A mask is
       used by `diff` only after a person approved it (the \"approved\" field names the decision).
--fns: keep only events whose enclosing inbound call ('in', else 'fn') is listed.
--allow-unapproved: debugging only; never in a gate.
Rewrite patterns use Rust regex syntax (no look-around, no back-references); a replacement may
use \\1 or \\g<name> (Python style) or ${1} / ${name}.
Schema: references/trace-schema.md of the cpp-rust-comparison-toolkit skill.";

const META: [&str; 10] = ["run", "impl", "case", "thread", "seq", "kind", "fn", "in", "order", "t"];

/// case -> thread -> events
pub type Trace = BTreeMap<String, BTreeMap<String, Vec<Value>>>;

fn key_str(v: Option<&Value>, default: &str) -> String {
    match v {
        None => default.to_string(),
        Some(v) => py_str(v),
    }
}

fn seq_of(e: &Value) -> f64 {
    e.get("seq").and_then(Value::as_f64).unwrap_or(0.0)
}

fn fn_of(e: &Value) -> Value {
    e.get("fn").cloned().unwrap_or(Value::String(String::new()))
}

fn fn_str(e: &Value) -> String {
    e.get("fn").map(py_str).unwrap_or_default()
}

pub fn load(path: &str, fns: Option<&BTreeSet<String>>) -> R<Trace> {
    let f = std::fs::File::open(path).map_err(|e| bad_input(format!("trace-diff: {path}: {e}")))?;
    let reader: Box<dyn Read> = if path.ends_with(".gz") {
        Box::new(flate2::read::MultiGzDecoder::new(f))
    } else {
        Box::new(f)
    };
    let mut events = Vec::new();
    for (n, line) in BufReader::new(reader).split(b'\n').enumerate() {
        let line = line.map_err(|e| bad_input(format!("trace-diff: {path}: {e}")))?;
        let line = String::from_utf8_lossy(&line);
        let line = strip_bom(line.trim());
        if line.is_empty() {
            continue;
        }
        let e: Value = serde_json::from_str(line).map_err(|x| bad_input(format!("trace-diff: {path}:{} is not JSON: {x}", n + 1)))?;
        if e.get("kind").is_none() || e.get("case").is_none() {
            return Err(bad_input(format!("trace-diff: {path}:{} lacks 'kind' or 'case'", n + 1)));
        }
        events.push(e);
    }
    Ok(index(events, fns))
}

pub fn index(events: Vec<Value>, fns: Option<&BTreeSet<String>>) -> Trace {
    let mut cases: Trace = BTreeMap::new();
    for e in events {
        if let Some(fns) = fns {
            let enclosing = if truthy(e.get("in")) { e.get("in") } else { e.get("fn") };
            if !enclosing.is_some_and(|v| fns.contains(&py_str(v))) {
                continue;
            }
        }
        let case = key_str(e.get("case"), "");
        let thread = key_str(e.get("thread"), "0");
        cases.entry(case).or_default().entry(thread).or_default().push(e);
    }
    for threads in cases.values_mut() {
        for evs in threads.values_mut() {
            evs.sort_by(|a, b| seq_of(a).total_cmp(&seq_of(b)));
            *evs = sort_sets(std::mem::take(evs));
        }
    }
    cases
}

fn payload(e: &Value) -> Value {
    let mut m = Map::new();
    if let Value::Object(o) = e {
        for (k, v) in o {
            if !META.contains(&k.as_str()) {
                m.insert(k.clone(), v.clone());
            }
        }
    }
    Value::Object(m)
}

/// Consecutive events marked order=set compare as a multiset: sort each run canonically.
fn sort_sets(evs: Vec<Value>) -> Vec<Value> {
    let mut out = Vec::with_capacity(evs.len());
    let mut run: Vec<Value> = Vec::new();
    let flush = |run: &mut Vec<Value>, out: &mut Vec<Value>| {
        run.sort_by_key(|x| (canon(&x["kind"]), fn_str(x), canon(&payload(x))));
        out.append(run);
    };
    for e in evs {
        if e.get("order").and_then(Value::as_str) == Some("set") {
            run.push(e);
            continue;
        }
        flush(&mut run, &mut out);
        out.push(e);
    }
    flush(&mut run, &mut out);
    out
}

pub struct Masks {
    pub ignore: Vec<Value>,
    pub rewrites: Vec<(Regex, String)>,
    pub hits: BTreeMap<String, u64>,
}

/// Python `re.sub` replacement syntax (`\1`, `\g<name>`, `\\`) to the regex crate's (`${1}`).
fn py_replacement(rep: &str) -> String {
    let mut out = String::new();
    let mut it = rep.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            // `${1}` / `${name}` is already regex-crate syntax; any other `$` is literal
            '$' if it.peek() == Some(&'{') => out.push('$'),
            '$' => out.push_str("$$"),
            '\\' => match it.peek().copied() {
                Some(d) if d.is_ascii_digit() => {
                    let mut num = String::new();
                    while let Some(d) = it.peek().copied().filter(char::is_ascii_digit) {
                        num.push(d);
                        it.next();
                    }
                    out.push_str(&format!("${{{num}}}"));
                }
                Some('g') => {
                    it.next();
                    if it.peek() == Some(&'<') {
                        it.next();
                        let name: String = it.by_ref().take_while(|c| *c != '>').collect();
                        out.push_str(&format!("${{{name}}}"));
                    } else {
                        out.push_str("\\g");
                    }
                }
                Some('n') => {
                    it.next();
                    out.push('\n');
                }
                Some('t') => {
                    it.next();
                    out.push('\t');
                }
                Some('\\') => {
                    it.next();
                    out.push('\\');
                }
                _ => out.push('\\'),
            },
            c => out.push(c),
        }
    }
    out
}

impl Masks {
    pub fn empty() -> Masks {
        Masks {
            ignore: Vec::new(),
            rewrites: Vec::new(),
            hits: BTreeMap::new(),
        }
    }

    pub fn load(files: &[String], allow_unapproved: bool) -> R<Masks> {
        let mut m = Masks::empty();
        for f in files {
            let doc = load_json_file(f)?;
            for x in arr(doc.get("masks")) {
                if !truthy(x.get("approved")) && !allow_unapproved {
                    return Err(bad_input(format!(
                        "trace-diff: mask without 'approved' in {f}: {} (approve it in the plan first)",
                        canon(&x)
                    )));
                }
                if x.get("path").and_then(Value::as_str).is_none() {
                    return Err(bad_input(format!("trace-diff: mask without 'path' in {f}: {}", canon(&x))));
                }
                m.ignore.push(x);
            }
            for r in arr(doc.get("rewrites")) {
                if !truthy(r.get("approved")) && !allow_unapproved {
                    return Err(bad_input(format!("trace-diff: rewrite without 'approved' in {f}")));
                }
                let pat = r.get("pattern").and_then(Value::as_str).unwrap_or("");
                let rx = Regex::new(pat).map_err(|e| bad_input(format!("trace-diff: rewrite pattern {pat:?} in {f}: {e}")))?;
                m.rewrites
                    .push((rx, py_replacement(r.get("replace").and_then(Value::as_str).unwrap_or(""))));
            }
        }
        Ok(m)
    }

    fn masked(&mut self, e: &Value, path: &str) -> bool {
        let efn = fn_str(e);
        for m in &self.ignore {
            let mfn = m.get("fn").map(py_str).unwrap_or_else(|| "*".into());
            let kind_ok = m.get("kind").is_none_or(|k| *k == e["kind"]);
            let mpath = m.get("path").map(py_str).unwrap_or_default();
            if fnmatchcase(&efn, &mfn) && kind_ok && fnmatchcase(path, &mpath) {
                let key = format!("{}:{}:{}", mfn, m.get("kind").map(py_str).unwrap_or_else(|| "*".into()), mpath);
                *self.hits.entry(key).or_default() += 1;
                return true;
            }
        }
        false
    }

    fn rewrite(&self, v: &Value) -> Value {
        match v {
            Value::String(s) => {
                let mut s = s.clone();
                for (rx, rep) in &self.rewrites {
                    s = rx.replace_all(&s, rep.as_str()).into_owned();
                }
                Value::String(s)
            }
            Value::Array(a) => Value::Array(a.iter().map(|x| self.rewrite(x)).collect()),
            Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), self.rewrite(x))).collect()),
            other => other.clone(),
        }
    }
}

/// Floats compare numerically with ints (1 == 1.0), and NaN equals NaN.
pub fn float_eq(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

fn is_float(v: &Value) -> bool {
    matches!(v, Value::Number(n) if n.is_f64())
}

/// Every leaf that differs as (path, a, b). List indices appear as numbers.
pub fn field_diffs(a: &Value, b: &Value, path: &str, out: &mut Vec<(String, Value, Value)>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let keys: BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            for k in keys {
                let p = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                match (x.get(k), y.get(k)) {
                    (Some(u), Some(v)) => field_diffs(u, v, &p, out),
                    (u, v) => {
                        let absent = || Value::String("<absent>".into());
                        out.push((p, u.cloned().unwrap_or_else(absent), v.cloned().unwrap_or_else(absent)))
                    }
                }
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (u, v)) in x.iter().zip(y).enumerate() {
                field_diffs(u, v, &format!("{path}.{i}"), out);
            }
        }
        _ if is_float(a) || is_float(b) => {
            let same = match (a.as_f64(), b.as_f64()) {
                (Some(x), Some(y)) if !a.is_boolean() && !b.is_boolean() => float_eq(x, y),
                _ => false,
            };
            if !same {
                out.push((path.to_string(), a.clone(), b.clone()));
            }
        }
        _ => {
            if a != b {
                out.push((path.to_string(), a.clone(), b.clone()));
            }
        }
    }
}

fn brief(e: &Value) -> String {
    let mut m = Map::new();
    if let Value::Object(o) = e {
        for (k, v) in o {
            if !["run", "impl", "t"].contains(&k.as_str()) {
                m.insert(k.clone(), v.clone());
            }
        }
    }
    let s = canon(&Value::Object(m));
    if s.chars().count() <= 300 {
        s
    } else {
        truncate_chars(&s, 300) + "..."
    }
}

// --- difflib.SequenceMatcher(None, a, b, autojunk=False).get_opcodes() ----------------------

fn find_longest_match(
    a: &[String],
    b: &[String],
    b2j: &HashMap<&str, Vec<usize>>,
    alo: usize,
    ahi: usize,
    blo: usize,
    bhi: usize,
) -> (usize, usize, usize) {
    let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0usize);
    let mut j2len: HashMap<usize, usize> = HashMap::new();
    for (i, ai) in a.iter().enumerate().take(ahi).skip(alo) {
        let mut newj2len = HashMap::new();
        if let Some(js) = b2j.get(ai.as_str()) {
            for &j in js {
                if j < blo {
                    continue;
                }
                if j >= bhi {
                    break;
                }
                let k = j.checked_sub(1).and_then(|p| j2len.get(&p)).copied().unwrap_or(0) + 1;
                newj2len.insert(j, k);
                if k > bestsize {
                    besti = i + 1 - k;
                    bestj = j + 1 - k;
                    bestsize = k;
                }
            }
        }
        j2len = newj2len;
    }
    while besti > alo && bestj > blo && a[besti - 1] == b[bestj - 1] {
        besti -= 1;
        bestj -= 1;
        bestsize += 1;
    }
    while besti + bestsize < ahi && bestj + bestsize < bhi && a[besti + bestsize] == b[bestj + bestsize] {
        bestsize += 1;
    }
    (besti, bestj, bestsize)
}

/// Opcodes as (tag, i1, i2, j1, j2); tags: equal, replace, delete, insert.
pub fn opcodes(a: &[String], b: &[String]) -> Vec<(&'static str, usize, usize, usize, usize)> {
    let mut b2j: HashMap<&str, Vec<usize>> = HashMap::new();
    for (j, x) in b.iter().enumerate() {
        b2j.entry(x.as_str()).or_default().push(j);
    }
    let mut queue = vec![(0, a.len(), 0, b.len())];
    let mut blocks = Vec::new();
    while let Some((alo, ahi, blo, bhi)) = queue.pop() {
        let (i, j, k) = find_longest_match(a, b, &b2j, alo, ahi, blo, bhi);
        if k > 0 {
            blocks.push((i, j, k));
            if alo < i && blo < j {
                queue.push((alo, i, blo, j));
            }
            if i + k < ahi && j + k < bhi {
                queue.push((i + k, ahi, j + k, bhi));
            }
        }
    }
    blocks.sort();
    let (mut i1, mut j1, mut k1) = (0, 0, 0);
    let mut merged = Vec::new();
    for (i2, j2, k2) in blocks {
        if i1 + k1 == i2 && j1 + k1 == j2 {
            k1 += k2;
        } else {
            if k1 > 0 {
                merged.push((i1, j1, k1));
            }
            (i1, j1, k1) = (i2, j2, k2);
        }
    }
    if k1 > 0 {
        merged.push((i1, j1, k1));
    }
    merged.push((a.len(), b.len(), 0));
    let (mut i, mut j) = (0, 0);
    let mut ops = Vec::new();
    for (ai, bj, size) in merged {
        let tag = if i < ai && j < bj {
            "replace"
        } else if i < ai {
            "delete"
        } else if j < bj {
            "insert"
        } else {
            ""
        };
        if !tag.is_empty() {
            ops.push((tag, i, ai, j, bj));
        }
        (i, j) = (ai + size, bj + size);
        if size > 0 {
            ops.push(("equal", ai, i, bj, j));
        }
    }
    ops
}

// --- diff ------------------------------------------------------------------------------------

/// A divergence and its class parts (kept apart so a function id containing ':' is not split).
struct Div {
    at: usize,
    body: Map<String, Value>,
    class: (String, String, String, String),
}

fn compare_thread(ea: &[Value], eb: &[Value], masks: &mut Masks, ctx: usize) -> Vec<Div> {
    let sig = |e: &Value| format!("{}\u{0}{}", canon(&e["kind"]), canon(&fn_of(e)));
    let ka: Vec<String> = ea.iter().map(sig).collect();
    let kb: Vec<String> = eb.iter().map(sig).collect();
    let mut div = Vec::new();
    let pairs: Vec<(usize, usize)> = if ka == kb {
        (0..ea.len()).map(|i| (i, i)).collect()
    } else {
        let mut pairs = Vec::new();
        for (tag, i1, i2, j1, j2) in opcodes(&ka, &kb) {
            if tag == "equal" {
                pairs.extend((i1..i2).zip(j1..j2));
                continue;
            }
            let first = if i2 > i1 { &ea[i1] } else { &eb[j1] };
            let mut body = Map::new();
            body.insert("type".into(), json!("sequence"));
            body.insert("at".into(), json!(i1));
            body.insert("a_only".into(), json!(ea[i1..i2].iter().take(ctx).map(brief).collect::<Vec<_>>()));
            body.insert("b_only".into(), json!(eb[j1..j2].iter().take(ctx).map(brief).collect::<Vec<_>>()));
            body.insert("fn".into(), fn_of(first));
            body.insert("kind".into(), first["kind"].clone());
            body.insert(
                "context".into(),
                json!(ea[i1.saturating_sub(ctx)..i1].iter().map(brief).collect::<Vec<_>>()),
            );
            let class = ("sequence".into(), py_str(&first["kind"]), py_str(&fn_of(first)), String::new());
            div.push(Div { at: i1, body, class });
        }
        pairs
    };
    for (i, j) in pairs {
        let (a, b) = (masks.rewrite(&payload(&ea[i])), masks.rewrite(&payload(&eb[j])));
        let mut diffs = Vec::new();
        field_diffs(&a, &b, "", &mut diffs);
        for (path, x, y) in diffs {
            if masks.masked(&ea[i], &path) {
                continue;
            }
            let mut body = Map::new();
            body.insert("type".into(), json!("value"));
            body.insert("at".into(), json!(i));
            body.insert("seq".into(), ea[i].get("seq").cloned().unwrap_or(Value::Null));
            body.insert("kind".into(), ea[i]["kind"].clone());
            body.insert("fn".into(), fn_of(&ea[i]));
            body.insert("path".into(), json!(path));
            body.insert("a".into(), x);
            body.insert("b".into(), y);
            body.insert(
                "context".into(),
                json!(ea[i.saturating_sub(ctx)..=i].iter().map(brief).collect::<Vec<_>>()),
            );
            let class = ("value".into(), py_str(&ea[i]["kind"]), py_str(&fn_of(&ea[i])), path);
            div.push(Div { at: i, body, class });
        }
    }
    div.sort_by_key(|d| d.at);
    div
}

/// Result of a diff plus the structured class keys (for noise learning).
pub struct DiffResult {
    pub summary: Value,
    classes: Vec<((String, String, String, String), u64)>,
}

pub fn diff(a: &Trace, b: &Trace, masks: &mut Masks, ctx: usize) -> DiffResult {
    let mut classes: Vec<((String, String, String, String), u64)> = Vec::new();
    let bump = |classes: &mut Vec<_>, k: (String, String, String, String)| match classes.iter_mut().find(|(c, _)| *c == k) {
        Some((_, n)) => *n += 1,
        None => classes.push((k, 1)),
    };
    let mut firsts = Vec::new();
    let mut bad = 0u64;
    let all: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    for case in &all {
        let (Some(ta), Some(tb)) = (a.get(*case), b.get(*case)) else {
            bad += 1;
            let side = if a.contains_key(*case) { "b" } else { "a" };
            bump(&mut classes, ("case_missing".into(), side.into(), String::new(), String::new()));
            if firsts.len() < 20 {
                firsts.push(json!({"case": case, "type": "case_missing", "missing_in": side}));
            }
            continue;
        };
        let mut case_div = Vec::new();
        let threads: BTreeSet<&String> = ta.keys().chain(tb.keys()).collect();
        let none = Vec::new();
        for th in threads {
            for mut d in compare_thread(ta.get(th).unwrap_or(&none), tb.get(th).unwrap_or(&none), masks, ctx) {
                d.body.insert("thread".into(), json!(th));
                case_div.push(d);
            }
        }
        if !case_div.is_empty() {
            bad += 1;
            for d in &case_div {
                bump(&mut classes, d.class.clone());
            }
            if firsts.len() < 20 {
                let mut first = case_div[0].body.clone();
                first.insert("case".into(), json!(case));
                first.insert("more_in_case".into(), json!(case_div.len() - 1));
                firsts.push(Value::Object(first));
            }
        }
    }
    classes.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let class_key = |(t, k, f, p): &(String, String, String, String)| {
        if t == "case_missing" {
            format!("{t}:{k}")
        } else {
            format!("{t}:{k}:{f}:{p}")
        }
    };
    let events: usize = a.values().flat_map(|t| t.values()).map(Vec::len).sum();
    let summary = json!({
        "schema": "cpp2rust-tracediff/1",
        "cases": all.len(),
        "events_a": events,
        "divergent_cases": bad,
        "divergence_classes": classes.iter().map(|(k, n)| (class_key(k), json!(n))).collect::<Map<_, _>>(),
        "masked_hits": masks.hits,
        "first_divergences": firsts,
    });
    DiffResult { summary, classes }
}

/// A path with list indices generalized: `args.1` -> `args.*` (the first segment is kept).
pub fn generalize(path: &str) -> String {
    path.split('.')
        .enumerate()
        .map(|(i, s)| {
            if i > 0 && !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
                "*"
            } else {
                s
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

pub fn noise(a: &Trace, b: &Trace) -> Value {
    let res = diff(a, b, &mut Masks::empty(), 0);
    let mut proposed: BTreeMap<(String, String, String), u64> = BTreeMap::new();
    let mut unstable = Vec::new();
    for ((typ, kind, f, path), n) in &res.classes {
        if typ == "value" {
            *proposed.entry((f.clone(), kind.clone(), generalize(path))).or_default() += n;
        } else {
            let key = if typ == "case_missing" {
                format!("{typ}:{kind}")
            } else {
                format!("{typ}:{kind}:{f}:{path}")
            };
            unstable.push(json!({"class": key, "count": n}));
        }
    }
    let masks: Vec<Value> = proposed
        .into_iter()
        .map(|((f, kind, path), n)| {
            json!({"fn": f, "kind": kind, "path": path, "action": "ignore", "seen": n,
                   "reason": "varies between two runs of the same implementation", "approved": null})
        })
        .collect();
    json!({"schema": "cpp2rust-masks/1", "masks": masks, "rewrites": [], "unstable_sequences": unstable,
           "note": "Proposals only. Unstable sequences cannot be masked: fix the nondeterminism or mark the events order=set at the seam."})
}

pub fn main(argv: &[String]) -> R<i32> {
    let spec = [
        ("--a", Kind::Value),
        ("--b", Kind::Value),
        ("--masks", Kind::Multi),
        ("--fns", Kind::Value),
        ("--out", Kind::Value),
        ("--context", Kind::Value),
        ("--allow-unapproved", Kind::Flag),
    ];
    let a = Args::parse(argv, &spec)?;
    let mode = match a.positionals.as_slice() {
        [m] if m == "diff" || m == "noise" => m.clone(),
        _ => return usage("trace-diff: give the mode diff or noise"),
    };
    let (Some(pa), Some(pb)) = (a.get("--a"), a.get("--b")) else {
        return usage("--a and --b are required");
    };
    let fns: Option<BTreeSet<String>> = a.get("--fns").map(|s| s.split(',').map(String::from).collect());
    let (ta, tb) = (load(pa, fns.as_ref())?, load(pb, fns.as_ref())?);
    let (mut res, rc) = if mode == "noise" {
        let res = noise(&ta, &tb);
        println!(
            "proposed masks: {}, unstable sequences: {}",
            res["masks"].as_array().unwrap().len(),
            res["unstable_sequences"].as_array().unwrap().len()
        );
        (res, 0)
    } else {
        if ta.is_empty() {
            return Err(bad_input(format!("trace-diff: no events in {pa} (an empty trace never passes)")));
        }
        let mut masks = Masks::load(&a.all("--masks"), a.flag("--allow-unapproved"))?;
        let res = diff(&ta, &tb, &mut masks, a.num("--context", 5usize)?).summary;
        let rc = if res["divergent_cases"].as_u64().unwrap_or(0) > 0 { 1 } else { 0 };
        let hits: u64 = res["masked_hits"].as_object().unwrap().values().filter_map(Value::as_u64).sum();
        println!("cases {}, divergent {}, masked hits {}", res["cases"], res["divergent_cases"], hits);
        for d in res["first_divergences"].as_array().unwrap().iter().take(3) {
            let rest: Map<String, Value> = d
                .as_object()
                .unwrap()
                .iter()
                .filter(|(k, _)| *k != "case" && *k != "context")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            println!(
                "FIRST DIVERGENCE case={} {}",
                py_str(&d["case"]),
                truncate_chars(&canon(&Value::Object(rest)), 600)
            );
            for c in arr(d.get("context")) {
                println!("   | {}", py_str(&c));
            }
        }
        (res, rc)
    };
    if mode == "diff" {
        res["unapproved_masks_allowed"] = json!(a.flag("--allow-unapproved"));
    }
    if let Some(out) = a.get("--out") {
        write_json(out, &res)?;
    }
    Ok(rc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(case: &str, seq: f64, kind: &str, f: &str, extra: Value) -> Value {
        let mut e = json!({"case": case, "thread": 0, "seq": seq, "kind": kind, "fn": f});
        for (k, v) in extra.as_object().unwrap() {
            e[k] = v.clone();
        }
        e
    }

    fn base() -> Vec<Value> {
        vec![
            ev("c1", 0.0, "call", "parse", json!({"args": ["a.txt"]})),
            ev("c1", 1.0, "out_call", "fs.read", json!({"args": ["/tmp/x1/a.txt"]})),
            ev("c1", 2.0, "out_ret", "fs.read", json!({"ret": {"len": 3, "ms": 5}})),
            ev("c1", 3.0, "ret", "parse", json!({"ret": {"ok": true}, "state": {"count": 1}})),
            ev("c2", 0.0, "call", "put", json!({"args": [1], "order": "set"})),
            ev("c2", 1.0, "call", "put", json!({"args": [2], "order": "set"})),
        ]
    }

    fn idx(evs: &[Value]) -> Trace {
        index(evs.to_vec(), None)
    }

    fn divergent(a: &[Value], b: &[Value], m: &mut Masks) -> u64 {
        diff(&idx(a), &idx(b), m, 5).summary["divergent_cases"].as_u64().unwrap()
    }

    #[test]
    fn identical_and_set_ordered() {
        assert_eq!(divergent(&base(), &base(), &mut Masks::empty()), 0);
        let mut swapped = base();
        swapped[4]["args"] = json!([2]);
        swapped[5]["args"] = json!([1]);
        assert_eq!(divergent(&base(), &swapped, &mut Masks::empty()), 0);
    }

    #[test]
    fn value_divergence_is_localized() {
        let mut bad = base();
        bad[3]["state"] = json!({"count": 2});
        let r = diff(&idx(&base()), &idx(&bad), &mut Masks::empty(), 5).summary;
        assert_eq!(r["divergent_cases"], 1);
        assert_eq!(r["first_divergences"][0]["path"], "state.count");
        assert!(r["divergence_classes"].get("value:ret:parse:state.count").is_some(), "{r}");
    }

    #[test]
    fn extra_outbound_call_is_a_sequence_divergence() {
        let mut extra = base();
        extra.insert(3, ev("c1", 2.5, "out_call", "fs.fsync", json!({"args": []})));
        let r = diff(&idx(&base()), &idx(&extra), &mut Masks::empty(), 5).summary;
        assert_eq!(r["first_divergences"][0]["type"], "sequence");
        assert_eq!(r["first_divergences"][0]["fn"], "fs.fsync");
    }

    #[test]
    fn noise_proposes_masks_that_count_only_once_approved() {
        let mut n2 = base();
        n2[2]["ret"]["ms"] = json!(9);
        n2[1]["args"] = json!(["/tmp/x2/a.txt"]);
        let prop = noise(&idx(&base()), &idx(&n2));
        let paths: BTreeSet<(String, String)> = prop["masks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| (py_str(&m["fn"]), py_str(&m["path"])))
            .collect();
        let want: BTreeSet<(String, String)> = [("fs.read", "ret.ms"), ("fs.read", "args.*")]
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        assert_eq!(paths, want);
        assert!(prop["masks"].as_array().unwrap().iter().all(|m| m["approved"].is_null()));
        let mut m = Masks::empty();
        m.ignore = vec![json!({"fn": "fs.read", "kind": "out_ret", "path": "ret.ms", "approved": "DQ-1"})];
        m.rewrites = vec![(Regex::new(r"/tmp/[^/]+").unwrap(), py_replacement("$TMP"))];
        assert_eq!(divergent(&base(), &n2, &mut m), 0);
        assert_eq!(m.hits.get("fs.read:out_ret:ret.ms"), Some(&1));
        let mut bad = base();
        bad[3]["state"] = json!({"count": 2});
        assert_eq!(divergent(&base(), &bad, &mut m), 1); // a mask does not hide a real difference
    }

    #[test]
    fn missing_case_nan_and_numbers() {
        let only_c1: Vec<Value> = base().into_iter().filter(|e| e["case"] == "c1").collect();
        assert_eq!(divergent(&base(), &only_c1, &mut Masks::empty()), 1);
        assert!(float_eq(f64::NAN, f64::NAN));
        let mut d = Vec::new();
        field_diffs(&json!({"x": 1, "y": true}), &json!({"x": 1.0, "y": 1}), "", &mut d);
        assert_eq!(d.len(), 1, "1 == 1.0, but true != 1: {d:?}");
        assert_eq!(d[0].0, "y");
    }

    #[test]
    fn function_ids_with_colons_and_replacements() {
        let a = vec![ev("c", 0.0, "ret", "ns::f", json!({"ret": 1}))];
        let b = vec![ev("c", 0.0, "ret", "ns::f", json!({"ret": 2}))];
        let prop = noise(&idx(&a), &idx(&b));
        assert_eq!(prop["masks"][0]["fn"], "ns::f");
        assert_eq!(prop["masks"][0]["path"], "ret");
        assert_eq!(py_replacement(r"\1-\g<x>$"), "${1}-${x}$$");
        assert_eq!(generalize("args.1.name.20"), "args.*.name.*");
        assert_eq!(generalize("3.x"), "3.x");
    }

    #[test]
    fn masks_match_case_sensitively_on_every_platform() {
        let a = vec![ev("c", 0.0, "ret", "Parse", json!({"ret": {"Ms": 1}}))];
        let b = vec![ev("c", 0.0, "ret", "Parse", json!({"ret": {"Ms": 2}}))];
        let mut m = Masks::empty();
        m.ignore = vec![json!({"fn": "parse", "path": "ret.Ms", "approved": "DQ-1"})];
        assert_eq!(divergent(&a, &b, &mut m), 1); // `parse` is another function than `Parse`
        m.ignore = vec![json!({"fn": "Parse", "path": "ret.ms", "approved": "DQ-1"})];
        assert_eq!(divergent(&a, &b, &mut m), 1);
        m.ignore = vec![json!({"fn": "Pa*", "path": "ret.M?", "approved": "DQ-1"})];
        assert_eq!(divergent(&a, &b, &mut m), 0);
    }

    #[test]
    fn trace_file_with_bom_and_crlf() {
        let d = crate::testutil::TempDir::new("tracebom");
        d.write(
            "t.jsonl",
            "\u{feff}{\"case\":\"c\",\"seq\":0,\"kind\":\"ret\",\"fn\":\"f\",\"ret\":1}\r\n\r\n",
        );
        let t = load(&d.path().join("t.jsonl").to_string_lossy(), None).unwrap();
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn opcodes_match_difflib() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        // difflib.SequenceMatcher(None, "abxcd", "abcyd", autojunk=False).get_opcodes()
        assert_eq!(
            opcodes(&s(&["a", "b", "x", "c", "d"]), &s(&["a", "b", "c", "y", "d"])),
            vec![
                ("equal", 0, 2, 0, 2),
                ("delete", 2, 3, 2, 2),
                ("equal", 3, 4, 2, 3),
                ("insert", 4, 4, 3, 4),
                ("equal", 4, 5, 4, 5)
            ]
        );
        assert_eq!(opcodes(&s(&["a"]), &s(&["b"])), vec![("replace", 0, 1, 0, 1)]);
    }
}
