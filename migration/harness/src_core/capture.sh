#!/usr/bin/env bash
# Records the C++ seam trace of src_core: the tap build of the product runs every surface case of the component
# (src_core cases.jsonl and stats.cases.jsonl) and the product writes one cpp2rust-trace/1 event
# stream (src_core.cnf_*, src_core.stats_report, src_core.ugrep, src_core.terminal, ...), the same events as
# migration/corpus/src_core/seam/all.trace.jsonl:
#   capture.sh <out.jsonl>
# Cases are run one process each (run-cases) in a fresh directory, so the trace of a case does not depend on the others.
set -e
export MSYS_NO_PATHCONV=1   # keep /p:... switches intact under Git bash
cd "$(dirname "$0")/../../.."
out="${1:?usage: capture.sh out.jsonl}"
tools="cargo run -q --release --locked --manifest-path migration/tools/cpp2rust-tools/Cargo.toml --"
work=migration/out/src_core/capture
mkdir -p "$work" "$(dirname "$out")"
powershell -NoProfile -File migration/harness/build-cpp.ps1 -Extra "/p:CppRustTap=true /p:CppRustOutSuffix=-tap" >"$work/build.log" 2>&1 || { tail -30 "$work/build.log"; exit 127; }
abs="$(cd "$(dirname "$out")" && pwd -W)/$(basename "$out")"
rm -f "$abs"
python migration/harness/src_core/tap_cases.py "$abs" migration/corpus/src_core/cases.jsonl migration/corpus/src_core/stats.cases.jsonl >"$work/tapped.cases.jsonl"
$tools run-cases --bin msvc/bin/Base-Release-x64-tap/ugrep.exe --impl cpp --cases "$work/tapped.cases.jsonl" --env-matrix migration/env-matrix.json --out "$work/surface.tap.jsonl" >/dev/null
# the product writes events as it runs; a thread of a case may finish after another: order the file by case, thread, seq
python migration/harness/src_core/sort_trace.py "$abs"
