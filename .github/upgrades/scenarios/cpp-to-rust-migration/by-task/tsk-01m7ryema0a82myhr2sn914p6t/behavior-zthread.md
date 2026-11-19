# Behavior: zthread - findings

Zthread is a unit of `src_core`. Its decompression paths are judged on component `src_core_codec` (D-src_core-codec, PR #98), on `Pcre-Release`, the configuration with every codec. The `src_core` corpus and catalog are not changed.

## What was recorded
- 217 surface cases (`zt-*`) in `migration/corpus/src_core_codec/cases.jsonl` (5.4 MB). Generator: `migration/harness/src_core_codec/gen_cases.py`. The big fixtures are built from repeated text, so they stay small. The 7z and lz4 bytes are in `fixtures.json` (built once with py7zr and python-lz4).
- C++ traces, 5 runs per case, 0 flaky: `migration/corpus/src_core_codec/surface/cases.trace.jsonl`, recorded on `msvc/bin/Pcre-Release-x64/ugrep.exe`.
- C++ against itself: 217 cases, 0 divergent, 0 proposed masks, so no `src_core_codec.proposed.json`. The approved masks of master (`migration/masks/src_core_codec.json`) are unchanged.
- Catalog `migration/behavior/src_core_codec.yaml` (B-src_core_codec-001..038): 38 entries, 9 suspicious (hypothesis).
- Covered: -z, --zmax 1/2/3, zip/tar/cpio/7z/lz4/tgz/tbz/txz/tzst, multi-member archives, -m/-l/-q cancellation (single stream, archives, zmax chains, many files), corrupt and unreadable inputs.

## Build and re-record
`migration/harness/zstream/fetch-codecs.ps1` (add the VS Installer folder to PATH for vswhere), then `migration/harness/build-cpp.ps1 -Configuration Pcre-Release`.

Only the zt-* subset in src_core_codec is recorded (never src_core):
`python migration/harness/src_core_codec/gen_cases.py`, then
`cpp2rust-tools run-cases --bin msvc/bin/Pcre-Release-x64/ugrep.exe --impl cpp --repeat 5 --cases migration/corpus/src_core_codec/cases.jsonl --env-matrix migration/env-matrix.json --timeout 60 --out migration/corpus/src_core_codec/surface/cases.trace.jsonl`.

## Suspicious behaviors (for Triage; bug-compatibility is ask-per-behavior)
- B-030: truncated gzip prints the decoded part, exit 0, no warning.
- B-031: truncated bz2/xz/tgz silently exits 1.
- B-032: damaged zip member output as wrong data (no CRC check; WITH_ZIP_CRC32 undefined).
- B-033: zip with LZMA (method 14) rejected.
- B-034: truncated tar, bad tar checksum, truncated cpio output members read so far, no warning.
- B-035: `--heading` with an archive prints `{MEMBER}:` without archive name or heading line.
- B-036: silent truncation one level down a --zmax=2 chain.
- B-037: corrupt 7z reports "encrypted data or 7z input is not seekable"; a damaged 7z data byte is silent.
- B-038: truncated lz4 warns "cannot read NAME  " (exit 2), unlike the other codecs.

## Gaps
1. No seam trace: src_core is cleaned and has no tap.
2. run-cases stdin is text only; gzip on stdin was probed by hand (works), not recorded.
3. Plain zstd, brotli and bzip3 streams are not covered (only .tzst).
4. Open ownership questions from SEAM.md: who closes `file_in` when `start()` returns NULL; who closes the pipe read end when `open_next` fails.