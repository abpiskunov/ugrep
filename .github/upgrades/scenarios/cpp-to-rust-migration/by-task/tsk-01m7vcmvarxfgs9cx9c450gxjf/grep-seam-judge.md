# Grep seam judge (surface cases for each Grep::search mode branch)

- No seam tap exists: the Grep seam PR added none (`SEAM.md`: "No tap is added to `src/`"; `src_core` is cleaned and
  `policy-scan --no-tap` forbids one). The cases are therefore recorded as surface traces of the C++ product build
  (`msvc/bin/Base-Release-x64/ugrep.exe`), the same way as the rest of `src_core`.
- `migration/harness/src_core/gen_grep_seam.py` generates 206 inputs-only cases (`gseam-*`): -q/-l/-L, -c, --format,
  -o, -o with -A/-B/-C, plain, --hex/--with-hex, -v, -y, -A/-B/-C, -A/-B/-C with -v, multi-file and recursive.
  The archive path (-z) needs a codec build, so directories stand in. Multi-file cases use `-J1`; the first run
  without it was flaky (12 cases, output order), so the flag was added to the inputs, not a mask.
- Appended to `corpus/src_core/cases.jsonl` (206 lines) and `surface/cases.trace.jsonl` (1030 lines); 0 deleted lines.
- Checks: `run-cases --repeat 3` 206 runs, 0 flaky; second run `trace-diff diff` cases 206, divergent 0 (no noise, no masks);
  `policy-scan --base HEAD` PASS.
