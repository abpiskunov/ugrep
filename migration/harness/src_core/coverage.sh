#!/usr/bin/env bash
# Coverage of src/ugrep.cpp, output.cpp, stats.cpp and cnf.cpp by the recorded surface cases: coverage.sh <out.json>
# build-coverage.ps1 builds the product with clang-cl source-based coverage of those four files; every case of
# migration/corpus/src_core/{cases,stats.cases}.jsonl runs against it (one process each, one raw profile each) and
# `llvm-cov export -summary-only` writes the totals as JSON. The llvm tools are the ones of the clang-cl that compiled.
set -e
export MSYS_NO_PATHCONV=1
cd "$(dirname "$0")/../../.."
out="${1:?usage: coverage.sh out.json}"
tools="cargo run -q --release --locked --manifest-path migration/tools/cpp2rust-tools/Cargo.toml --"
work=migration/out/src_core/coverage
mkdir -p "$work" "$(dirname "$out")"
powershell -NoProfile -File migration/harness/src_core/build-coverage.ps1 >"$work/build.log" 2>&1 || { tail -30 "$work/build.log"; exit 127; }
llvm="${LLVM_BIN:-$(cat "$work/llvm-dir.txt")}"
rm -rf "$work/profiles"
mkdir -p "$work/profiles"
python migration/harness/src_core/cov_cases.py "$work/profiles" migration/corpus/src_core/cases.jsonl migration/corpus/src_core/stats.cases.jsonl >"$work/cov.cases.jsonl"
$tools run-cases --bin "$work/ugrep-cov.exe" --impl cpp --cases "$work/cov.cases.jsonl" --env-matrix migration/env-matrix.json --out "$work/surface.cov.jsonl" >/dev/null
ls "$work"/profiles/*.profraw >"$work/profiles.txt"
"$llvm/llvm-profdata.exe" merge -sparse -f "$work/profiles.txt" -o "$work/cov.profdata"
"$llvm/llvm-cov.exe" export "$work/ugrep-cov.exe" -instr-profile="$work/cov.profdata" -summary-only \
  src/ugrep.cpp src/output.cpp src/stats.cpp src/cnf.cpp >"$out"
