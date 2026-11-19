# Behavior: zthread — findings

## What was recorded
- 177 surface cases: `migration/corpus/src_core/cases.jsonl (zt-* cases appended)` (generator `migration/harness/zthread/gen_cases.py`).
- C++ trace, 5 runs per case, 0 flaky: `migration/corpus/src_core/surface/cases.trace.jsonl (zt-* cases appended)`.
- C++ against itself: 177 cases, 0 divergent, 0 masked hits. No noise, so no zthread mask is added to `migration/masks/src_core.proposed.json`.
- Catalog `migration/behavior/src_core.yaml (B-src_core-201..234)`: 34 entries (27 observed, 7 suspicious as hypothesis).
- Covered: -z, --zmax 1/2/3, zip/tar/cpio/tgz/tbz/txz/tzst, multi-member archives, -m/-l/-q cancellation (single stream, archives, zmax chains, many files), corrupt and unreadable inputs.

## Recording binary
The Base-Release product has no HAVE_LIBZ (`-z` unavailable). Cases were recorded with the codec-enabled pure-C++ reference `msvc/bin/CodecRef-x64/ugrep.exe`:
`migration/harness/zstream/fetch-codecs.ps1` then `build-codec-ref.ps1` (add VS Installer to PATH for vswhere).

Re-record:
`cpp2rust-tools run-cases --bin msvc/bin/CodecRef-x64/ugrep.exe --impl cpp --repeat 5 --cases migration/corpus/src_core/cases.jsonl (zt-* cases appended) --env-matrix migration/env-matrix.json --timeout 60 --out <trace>`

## Suspicious behaviors (for Triage; bug-compatibility is ask-per-behavior)
- B-src_core-228: truncated gzip prints decoded part, exit 0, no warning.
- B-src_core-229: truncated bz2/xz/tgz silently exits 1.
- B-src_core-230: damaged zip member output as wrong data (no CRC check; WITH_ZIP_CRC32 undefined).
- B-src_core-231: zip with LZMA (method 14) rejected.
- B-src_core-232: truncated tar, bad tar checksum, truncated cpio output members read so far, no warning.
- B-src_core-233: `--heading` with an archive prints `{MEMBER}:` without archive name or heading line.
- B-src_core-234: silent truncation one level down a --zmax=2 chain.

## Gaps
1. Gate target is `src_core` (zthread is a unit of it; `gate-check --changed origin/master` runs behavior-proposed-cleaned). The zthread cases and catalog entries now live in `corpus/src_core`, `surface/cases.trace.jsonl` and `behavior/src_core.yaml` (ids `zt-*`, `B-src_core-201..234`); generator `migration/harness/zthread/gen_cases.py` replaces the `zt-*` lines in `corpus/src_core/cases.jsonl`.
2. The `zt-*` cases need a codec-enabled build (the Base-Release product has no `-z`); the Port needs a codec build of the default product to replay them.
3. No seam trace: src_core is cleaned and has no tap.
4. run-cases stdin is text only; gzip on stdin was probed by hand (works), not recorded.
5. Open ownership questions from SEAM.md: who closes `file_in` when `start()` returns NULL; who closes the pipe read end when `open_next` fails.
