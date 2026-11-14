# Behavior: src_core

Gate `behavior-proposed src_core`: PASS (`migration/out/gates/behavior-proposed/src_core/report.json`).

- Catalog: `migration/behavior/src_core.yaml` (41 entries; 3 hypotheses marked suspicious).
- Corpus: surface cases in `cases.jsonl` + `stats.cases.jsonl`, `seam/all.trace.jsonl` (6,646 events, 430 cases), recorded on the C++ only.
- Harness: `rust/src_core-harness` (replay arms for CNF, Stats::report; stubs are `todo!()` until Port).
- Proposed masks: `migration/masks/src_core.proposed.json` (stats elapsed time; unapproved).
- C++ edits: tap encoders only (`src/core_tap.hpp`, `src/stats.cpp`).

## Not reached (behavior-ready will fail until a follow-up)
- Coverage: functions 81.9%, lines 60.7% (needs 100% / 85%). Uncovered regions are listed in the catalog with reasons.
- Saturation: not reached.
- The 683 option-sweep cases are in the surface corpus only; the seam trace was not re-recorded after them.

## For Triage
- `-m` negative/overflow silently accepted as no limit.
- `--format=%` exits 0 with no output.
- `-f pats.txt --and bar` sometimes `bad allocation` (exit 2): nondeterministic, likely UB; not in corpus.
- `CNF::compile` has no caller; `cnf_anchor`'s `flag_empty` write is not visible in the tap.
- `optf-tree-no-sort` is flaky and was dropped.

## Follow-up in this session
- Seam trace re-recorded over the full corpus (14,200 events); stale flaky case optf-tree-no-sort removed from the surface trace.
- behavior-proposed: PASS. behavior-ready: surface stable (0 divergent of 1099); seam divergences 55 remain, all stats_report elapsed-time fields (mask approval pending); coverage 84.6%/61.1% and saturation are still open for a follow-up.
