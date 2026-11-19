# Behavior: zthread - findings

Zthread is a unit of `src_core`. Its decompression paths are judged on component `src_core_codec` (D-src_core-codec, PR #98), on `Pcre-Release`, the configuration with every codec. The `src_core` corpus and catalog are not changed.

## What was recorded
- 224 surface cases (`zt-*`) in `migration/corpus/src_core_codec/cases.jsonl` (5.4 MB). Generator: `migration/harness/src_core_codec/gen_cases.py`. The big fixtures are built from repeated text, so they stay small. The 7z and lz4 bytes are in `fixtures.json` (built once with py7zr and python-lz4).
- C++ traces, 5 runs per case, 0 flaky: `migration/corpus/src_core_codec/surface/cases.trace.jsonl`, recorded on `msvc/bin/Pcre-Release-x64/ugrep.exe`. Review round 2 changed 3 cases, replaced 4 and added 7; the other 210 trace blocks are byte-identical to the previous recording.
- C++ against itself: 224 cases, 0 divergent, 0 proposed masks, so no `src_core_codec.proposed.json`. The approved masks of master (`migration/masks/src_core_codec.json`) are unchanged.
- Catalog `migration/behavior/src_core_codec.yaml` (B-src_core_codec-001..039, B-028 deleted as a duplicate): 38 entries. Decision D-src_core-codec is operator decision PR #98.
- Covered: -z, --zmax 1/2/3, -N (--depth), zip/tar/cpio/7z/lz4/tgz/tbz/txz/tzst, multi-member archives, -m/-l/-q cancellation (-l stops the current member only), corrupt and unreadable inputs.

## Build and re-record
`migration/harness/zstream/fetch-codecs.ps1` (add the VS Installer folder to PATH for vswhere), then `migration/harness/build-cpp.ps1 -Configuration Pcre-Release`.

Only the zt-* subset in src_core_codec is recorded (never src_core):
`python migration/harness/src_core_codec/gen_cases.py`, then
`cpp2rust-tools run-cases --bin msvc/bin/Pcre-Release-x64/ugrep.exe --impl cpp --repeat 5 --cases migration/corpus/src_core_codec/cases.jsonl --env-matrix migration/env-matrix.json --timeout 60 --out migration/corpus/src_core_codec/surface/cases.trace.jsonl`.

## Suspicious behaviors
Bug-preserve with the src_leaves decisions (not new Triage questions): B-030, B-031 (Q-zstream-049/050), B-032, B-033 (Q-zstream-054), B-037 (Q-zstream-053), B-038 (Q-zstream-050).

For Triage (hypothesis, Zthread-specific):
- B-034: truncated tar, bad tar checksum, truncated cpio output members read so far, no warning.
- B-035: a single archive without file names prints `{MEMBER}:` without the archive name; with `--with-filename --heading` the C++ prints an `a.zip` heading per member.
- B-036: a corrupt inner gzip of a zip/tar member under --zmax=2 is never reported.

## Gaps
1. No seam trace: src_core is cleaned and has no tap.
2. run-cases stdin is text only; gzip on stdin was probed by hand (works), not recorded.
3. Plain zstd, brotli and bzip3 streams are not covered (only .tzst).
4. Open ownership questions from SEAM.md: who closes `file_in` when `start()` returns NULL; who closes the pipe read end when `open_next` fails.