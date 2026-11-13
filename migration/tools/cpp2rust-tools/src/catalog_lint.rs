//! catalog-lint: check a behavior catalog against executable evidence.

use crate::util::*;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashSet};
use std::path::Path;

pub const HELP: &str = "\
cpp2rust-tools catalog-lint - check a behavior catalog against executable evidence.

  catalog-lint --catalog migration/behavior/core_parse.yaml [--migration migration]
               [--plan migration/validation-plan.yaml] [--out catalog.json]

Rules (errors fail the lint, exit 1):
  E1 every entry whose status is not `hypothesis` cites at least one evidence id that resolves:
       case:<id>   a case id in migration/corpus/<component>/**/*.jsonl (\"id\" or \"case\" field)
       test:<id>   a line of migration/corpus/test-ids.txt
       prop:<name> the name occurs in a file under migration/harness/<component>/
  E2 `bug-fix` and `dont-care` need `decision`; a `bug-fix` id must be listed in the plan's
     compare.differences[].behavior (so the comparison knows the expected difference)
  E3 a `suspicious: true` entry may not stay `observed`: it needs a user classification
  E4 every observation channel of a function is either compared or excluded with a reason
  E5 every `uncovered` region has a reason: unreachable | platform-only | excluded | deferred
  E6 ids are unique; statuses are from the list
Reported, not errors (the plan's gate decides): counts per status, hypotheses, open questions,
saturation (the last `stop.quiet_rounds` rounds added nothing).
Exit: 0 pass, 1 lint errors, 2 usage or input error.";

pub const STATUSES: [&str; 6] = ["hypothesis", "observed", "intended", "bug-preserve", "bug-fix", "dont-care"];
pub const REASONS: [&str; 4] = ["unreachable", "platform-only", "excluded", "deferred"];

struct Evidence {
    cases: HashSet<String>,
    tests: HashSet<String>,
    harness: String,
}

fn evidence_index(mig: &Path, comp: &str) -> Evidence {
    let mut cases = HashSet::new();
    let corpus = mig.join("corpus").join(comp);
    for rel in walk_files(&corpus, &corpus) {
        if !rel.ends_with(".jsonl") || rel.split('/').any(|s| s.starts_with('.')) {
            continue;
        }
        for line in read_text(corpus.join(&rel)).unwrap_or_default().lines() {
            let line = line.trim();
            let Ok(Value::Object(e)) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            for k in ["id", "case"] {
                if let Some(v) = e.get(k) {
                    let s = py_str(v);
                    cases.insert(s.split('@').next().unwrap_or("").to_string());
                    cases.insert(s);
                }
            }
        }
    }
    let tests = read_text(mig.join("corpus").join("test-ids.txt"))
        .map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default();
    let hdir = mig.join("harness").join(comp);
    let harness = walk_files(&hdir, &hdir)
        .iter()
        .map(|r| read_text(hdir.join(r)).unwrap_or_default())
        .collect();
    Evidence { cases, tests, harness }
}

pub fn lint(cat: &Value, mig: &Path, plan: Option<&Value>) -> Value {
    let comp = cat.get("component").map(py_str).unwrap_or_default().replace('/', "_");
    let ev = evidence_index(mig, &comp);
    let mut errors: Vec<String> = Vec::new();
    let mut counts = [0usize; 6];
    let mut seen = HashSet::new();
    let diffs: HashSet<String> = plan
        .map(|p| arr(get(p, "compare").and_then(|c| get(c, "differences"))))
        .unwrap_or_default()
        .iter()
        .filter_map(|d| d.get("behavior").map(py_str))
        .collect();
    let resolves = |e: &Value| {
        let s = py_str(e);
        let (kind, name) = s.split_once(':').unwrap_or((s.as_str(), ""));
        (kind == "case" && ev.cases.contains(name))
            || (kind == "test" && ev.tests.contains(name))
            || (kind == "prop" && !name.is_empty() && ev.harness.contains(name))
    };
    for b in arr(get(cat, "behaviors")) {
        let bid = b.get("id").map(py_str).unwrap_or_else(|| "?".into());
        if !seen.insert(bid.clone()) {
            errors.push(format!("E6 {bid}: duplicate id"));
        }
        let st = b.get("status").and_then(Value::as_str).unwrap_or("");
        let Some(si) = STATUSES.iter().position(|s| *s == st) else {
            errors.push(format!(
                "E6 {bid}: unknown status {}",
                py_repr(b.get("status").unwrap_or(&Value::Null))
            ));
            continue;
        };
        counts[si] += 1;
        if st != "hypothesis" {
            let evid = arr(b.get("evidence"));
            if !evid.iter().any(resolves) {
                let cited = if evid.is_empty() {
                    "none cited".to_string()
                } else {
                    py_repr(&Value::Array(evid))
                };
                errors.push(format!("E1 {bid}: status {st} but no evidence resolves ({cited})"));
            }
        }
        if (st == "bug-fix" || st == "dont-care") && !truthy(b.get("decision")) {
            errors.push(format!("E2 {bid}: {st} needs the user's decision id"));
        }
        if st == "bug-fix" && plan.is_some() && !diffs.contains(&bid) {
            errors.push(format!("E2 {bid}: bug-fix is not listed in the plan's compare.differences"));
        }
        if truthy(b.get("suspicious")) && st == "observed" {
            errors.push(format!("E3 {bid}: suspicious behavior is not classified by the user yet"));
        }
    }
    if let Some(Value::Object(obs)) = get(cat, "observation") {
        for (f, o) in obs {
            let compared: BTreeSet<String> = arr(o.get("compared")).iter().map(py_str).collect();
            let excluded: Vec<(String, Option<Value>)> = arr(o.get("excluded"))
                .iter()
                .map(|x| (x.get("channel").map(py_str).unwrap_or_default(), x.get("reason").cloned()))
                .collect();
            for ch in arr(o.get("channels")).iter().map(py_str) {
                // like a dict: the last exclusion of a channel counts
                let reason = excluded.iter().rev().find(|(c, _)| *c == ch).and_then(|(_, r)| r.clone());
                if !compared.contains(&ch) && !truthy(reason.as_ref()) {
                    errors.push(format!("E4 {f}: channel {ch} is neither compared nor excluded with a reason"));
                }
            }
        }
    }
    let reasons_repr = py_repr(&json!(REASONS));
    let bad_reason = |u: &Value| !u.get("reason").and_then(Value::as_str).is_some_and(|r| REASONS.contains(&r));
    let uncovered = arr(get(cat, "uncovered"));
    for u in &uncovered {
        if bad_reason(u) {
            errors.push(format!(
                "E5 uncovered {}: reason must be one of {reasons_repr}",
                py_str(u.get("region").unwrap_or(&Value::Null))
            ));
        }
    }
    let rounds = arr(get(cat, "discovery").and_then(|d| get(d, "rounds")));
    let k = get(cat, "stop")
        .and_then(|s| get(s, "quiet_rounds"))
        .and_then(Value::as_f64)
        .filter(|k| *k != 0.0)
        .unwrap_or(2.0) as usize;
    let quiet = rounds.len() >= k
        && rounds[rounds.len() - k..]
            .iter()
            .all(|r| !(truthy(r.get("new_cases")) || truthy(r.get("new_coverage")) || truthy(r.get("new_behaviors"))));
    let count_map: serde_json::Map<String, Value> = STATUSES.iter().zip(counts).map(|(s, n)| (s.to_string(), json!(n))).collect();
    json!({
        "schema": "cpp-rust-catalog-lint/1",
        "component": cat.get("component").cloned().unwrap_or(Value::Null),
        "entries": counts.iter().sum::<usize>(),
        "counts": count_map,
        "hypotheses": counts[0],
        "open_questions": arr(get(cat, "openQuestions")).len(),
        "unreasoned_uncovered": uncovered.iter().filter(|u| bad_reason(u)).count(),
        "rounds": rounds.len(),
        "saturated": quiet,
        "error_count": errors.len(),
        "result": if errors.is_empty() { "pass" } else { "fail" },
        "errors": errors,
    })
}

pub fn main(argv: &[String]) -> R<i32> {
    let spec = [
        ("--catalog", Kind::Value),
        ("--migration", Kind::Value),
        ("--plan", Kind::Value),
        ("--out", Kind::Value),
    ];
    let a = Args::parse(argv, &spec)?;
    let Some(catalog) = a.get("--catalog") else {
        return usage("--catalog is required");
    };
    let cat = load_yaml_file(catalog)?;
    let plan = match a.get("--plan") {
        Some(p) => Some(load_yaml_file(p)?),
        None => None,
    };
    let res = lint(&cat, Path::new(a.get("--migration").unwrap_or("migration")), plan.as_ref());
    for e in res["errors"].as_array().unwrap() {
        println!("ERROR {}", py_str(e));
    }
    let counts = STATUSES
        .iter()
        .map(|s| format!("'{s}': {}", res["counts"][*s]))
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "catalog {}: {}; {{{counts}}}; hypotheses {}, open questions {}, saturated {}",
        py_str(&res["component"]),
        py_str(&res["result"]).to_uppercase(),
        res["hypotheses"],
        res["open_questions"],
        py_repr(&res["saturated"])
    );
    if let Some(out) = a.get("--out") {
        write_json(out, &res)?;
    }
    Ok(if res["error_count"].as_u64().unwrap_or(0) > 0 { 1 } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn evidence() -> TempDir {
        let mig = TempDir::new("catalog");
        mig.write("corpus/core/cases.jsonl", "{\"id\": \"hdr-len-4g\"}\n{\"id\": \"empty\"}\n");
        mig.write("corpus/core/seam/t.trace.jsonl", "{\"case\": \"s1@0\", \"kind\": \"call\"}\n");
        mig.write("corpus/test-ids.txt", "Parse.Empty\n");
        mig.write("harness/core/props.rs", "fn hdr_len_roundtrip() {}\n");
        mig
    }

    fn good() -> Value {
        json!({"component": "core", "stop": {"quiet_rounds": 2},
            "observation": {"parse_header": {"channels": ["return", "errno", "log:debug"], "compared": ["return", "errno"],
                                             "excluded": [{"channel": "log:debug", "reason": "plan exclusion EX-3"}]}},
            "behaviors": [
                {"id": "B-1", "statement": "empty input returns E_EMPTY", "status": "intended", "evidence": ["case:empty", "test:Parse.Empty"]},
                {"id": "B-2", "statement": "length over 4 GiB is truncated", "status": "bug-fix", "suspicious": true,
                 "evidence": ["case:hdr-len-4g", "prop:hdr_len_roundtrip"], "decision": "DQ-7"},
                {"id": "B-3", "statement": "maybe retries on EINTR", "status": "hypothesis"},
                {"id": "B-4", "statement": "seam session", "status": "observed", "evidence": ["case:s1"]}],
            "uncovered": [{"region": "parse.cc:301-318", "reason": "unreachable"}],
            "discovery": {"rounds": [{"new_cases": 9}, {"new_cases": 0}, {"new_cases": 0, "new_behaviors": 0}]}})
    }

    #[test]
    fn a_good_catalog_passes() {
        let mig = evidence();
        let plan = json!({"compare": {"differences": [{"id": "D-1", "behavior": "B-2"}]}});
        let r = lint(&good(), mig.path(), Some(&plan));
        assert_eq!(r["result"], "pass", "{r}");
        assert_eq!(r["saturated"], true);
        assert_eq!(r["hypotheses"], 1);
    }

    #[test]
    fn every_rule_fires() {
        let mig = evidence();
        let mut bad = good();
        let bs = bad["behaviors"].as_array_mut().unwrap();
        bs.push(json!({"id": "B-5", "statement": "typed from reading code", "status": "observed", "evidence": ["case:nope"]}));
        bs.push(json!({"id": "B-6", "statement": "odd rounding", "status": "observed", "suspicious": true, "evidence": ["case:empty"]}));
        bs.push(json!({"id": "B-1", "statement": "dup", "status": "dont-care", "evidence": ["case:empty"]}));
        bad["observation"]["parse_header"]["channels"]
            .as_array_mut()
            .unwrap()
            .push(json!("global:g_last_error"));
        bad["uncovered"]
            .as_array_mut()
            .unwrap()
            .push(json!({"region": "x.cc:1-2", "reason": "boring"}));
        let r = lint(&bad, mig.path(), Some(&json!({"compare": {}})));
        let kinds: BTreeSet<String> = r["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| py_str(e).split(' ').next().unwrap().to_string())
            .collect();
        assert_eq!(
            kinds.into_iter().collect::<Vec<_>>(),
            vec!["E1", "E2", "E3", "E4", "E5", "E6"],
            "{}",
            r["errors"]
        );
        assert!(
            r["errors"]
                .as_array()
                .unwrap()
                .contains(&json!("E1 B-5: status observed but no evidence resolves (['case:nope'])"))
        );
        assert_eq!(r["unreasoned_uncovered"], 1);
    }
}
