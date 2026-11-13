#!/usr/bin/env bash
# Coverage of src/glob.cpp by the recorded seam cases: coverage.sh <out.json> [cases.jsonl]
# Replays the seam cases through a clang-cl coverage build of the seam driver and writes
# `llvm-cov export -summary-only` JSON for src/glob.cpp. The llvm tools must match clang-cl (raw profile\n# version): they are taken from the clang-cl directory unless LLVM_BIN is set.
set -e
cd "$(dirname "$0")/../../.."
out="${1:?usage: coverage.sh out.json [cases.jsonl]}"
cases="${2:-migration/corpus/src_leaves/seam/cases.jsonl}"
llvm="${LLVM_BIN:-$(rustc --print sysroot)/lib/rustlib/x86_64-pc-windows-msvc/bin}"
work=migration/out/src_leaves/coverage
mkdir -p "$work" "$(dirname "$out")"
rm -f "$work"/cov.*
powershell -NoProfile -File migration/harness/src_leaves/build-coverage.ps1 >"$work/build.log" 2>&1 || { cat "$work/build.log"; exit 127; }
llvm="${LLVM_BIN:-$(cat "$work/clang-dir.txt")}"
LLVM_PROFILE_FILE="$work/cov.profraw" CPP2RUST_TAP_OUT="$work/cov.trace.jsonl" "$work/cov_driver.exe" "$cases" >/dev/null
"$llvm/llvm-profdata.exe" merge -sparse "$work/cov.profraw" -o "$work/cov.profdata"
"$llvm/llvm-cov.exe" export "$work/cov_driver.exe" -instr-profile="$work/cov.profdata" -summary-only src/glob.cpp >"$out"
