# Behavior: zthread — findings

## What was recorded
- 177 surface cases: `migration/corpus/zthread/cases.jsonl` (generator `migration/harness/zthread/gen_cases.py`).
- C++ trace, 5 runs per case, 0 flaky: `migration/corpus/zthread/surface/cases.trace.jsonl`.
- C++ against itself: 177 cases, 0 divergent, 0 masked hits. No noise, so `migration/masks/zthread.proposed.json` is empty.
- Catalog `migration/behavior/zthread.yaml`: 34 entries (27 observed, 7 suspicious as hypothesis).
- Covered: -z, --zmax 1/2/3, zip/tar/cpio/tgz/tbz/txz/tzst, multi-member archives, -m/-l/-q cancellation (single stream, archives, zmax chains, many files), corrupt and unreadable inputs.

## Recording binary
The Base-Release product has no HAVE_LIBZ (`-z` unavailable). Cases were recorded with the codec-enabled pure-C++ reference `msvc/bin/CodecRef-x64/ugrep.exe`:
`migration/harness/zstream/fetch-codecs.ps1` then `build-codec-ref.ps1` (add VS Installer to PATH for vswhere).

Re-record:
`cpp2rust-tools run-cases --bin msvc/bin/CodecRef-x64/ugrep.exe --impl cpp --repeat 5 --cases migration/corpus/zthread/cases.jsonl --env-matrix migration/env-matrix.json --timeout 60 --out <trace>`

## Suspicious behaviors (for Triage; bug-compatibility is ask-per-behavior)
- B-zthread-101: truncated gzip prints decoded part, exit 0, no warning.
- B-zthread-102: truncated bz2/xz/tgz silently exits 1.
- B-zthread-103: damaged zip member output as wrong data (no CRC check; WITH_ZIP_CRC32 undefined).
- B-zthread-104: zip with LZMA (method 14) rejected.
- B-zthread-105: truncated tar, bad tar checksum, truncated cpio output members read so far, no warning.
- B-zthread-106: `--heading` with an archive prints `{MEMBER}:` without archive name or heading line.
- B-zthread-107: silent truncation one level down a --zmax=2 chain.

## Gaps
1. `behavior-ready --target zthread` fails ("component 'zthread' is not in the plan"); the gate is component-scoped. Working target: `src_core` (cleaned variants chosen automatically). That gate lints `behavior/src_core.yaml`, not `zthread.yaml`; zthread.yaml was linted separately (PASS, saturated, 0 open questions).
2. The src_core surface/unit-cleaned gates use the Base-Release product, which cannot run `-z`; the zthread corpus is not in `corpus/src_core`. Port needs a codec-enabled build of HEAD and a plan amendment.
3. No seam trace: src_core is cleaned and has no tap.
4. run-cases stdin is text only; gzip on stdin was probed by hand (works), not recorded.
5. Open ownership questions from SEAM.md: who closes `file_in` when `start()` returns NULL; who closes the pipe read end when `open_next` fails.
