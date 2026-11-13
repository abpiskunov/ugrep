#!/usr/bin/env bash
# Records the C++ seam trace of src_leaves.glob_match: capture.sh <out.jsonl> [cases.jsonl]
set -e
cd "$(dirname "$0")/../../.."
out="${1:?usage: capture.sh out.jsonl [cases.jsonl]}"
cases="${2:-migration/corpus/src_leaves/seam/cases.jsonl}"
powershell -NoProfile -File migration/harness/src_leaves/build-driver.ps1 >/dev/null
mkdir -p "$(dirname "$out")"
rm -f "$out"
CPP2RUST_TAP_OUT="$out" migration/out/src_leaves/glob_driver.exe "$cases"
