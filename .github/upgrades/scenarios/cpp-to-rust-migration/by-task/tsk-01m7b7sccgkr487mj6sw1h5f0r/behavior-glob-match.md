# Behavior: src_leaves.glob_match discovery

- Seam driver: `migration/harness/src_leaves/` (`glob_driver.cpp`, `build-driver.ps1`, `capture.sh`); builds `src/glob.cpp` with the tap and replays `migration/corpus/src_leaves/seam/cases.jsonl` (184 cases, `\u00XX` = raw byte) into `seam/all.trace.jsonl`.
- Surface: 24 cases appended (52 total), recorded 3x.
- Catalog: `migration/behavior/src_leaves.yaml` (25 entries; 5 suspicious, proposed `bug-preserve` until Triage: B-013 `**a`, B-019 invalid UTF-8 with `?`, B-020 unterminated `[`, B-021 `[]]`, B-023 include-dir + exclude-dir / `-g s/,*.txt` select nothing).
- Masks: none needed (`migration/masks/src_leaves.proposed.json` is empty); C++ vs recording: 0 divergent (surface 52, seam 184).
- Platform: PATHSEP is `\` on Windows, so `/`-written pathnames never match `/` globs; `-win` cases carry the native form.
- Gaps for later: no C++ coverage tool on this worker (no clang, MSVC has no llvm-cov format), so `coverage.sh` and saturation (needs 2 quiet rounds; 2 rounds so far) are open for a follow-up; ASan runtime not installed (B-025 stays hypothesis); no tap run of the full ugrep surface.
