# Behavior: output (recorded under src_core)

Operator direction: Output is part of cleaned component src_core; no `output` component, no plan amendment.
Gate: `gate-check --gate behavior-ready --target src_core` -> `GATE behavior-ready-cleaned src_core: PASS`.

- 2053 surface cases `out-*` appended to migration/corpus/src_core/cases.jsonl and surface/cases.trace.jsonl (recorded 3x-5x from the product build, flaky 0; C++ against itself: 0 divergent on all 2053).
- Catalog B-src_core-161..200 (40 entries), discovery rounds 19-23 (rounds 21-23 quiet).
- Generators: migration/harness/src_core/gen_output.py, quiet_output.py.
- Proposed masks: none (no noise found; unordered multi-file output uses -J1, `--max-files` with -J>1 is racy and not recorded).
- Suspicious (status hypothesis, for Triage): --replace %n counter without -n (also --max-line changes it); %l under -v prints `l`; --color with --width leaves an empty separator pair `ESC[36mESC[m` after a cut file name (B-src_core-194); adjacent matches leave an empty `ESC[1;37mESC[m` pair, with or without --width (B-src_core-200); coloured hex dumps and multi-line matches leave an empty match pair `ESC[1;31mESC[m` (B-src_core-199); --replace -o has no line ending; last line without LF gets CRLF in plain output; CSV file-name quote escaping differs from match text.
