#!/usr/bin/env bash
# Records the C++ seam trace of src_leaves (glob_match, then z_open/z_read/z_write/z_close):
#   capture.sh <out.jsonl> [glob-cases.jsonl [zopen-cases.jsonl]]
set -e
cd "$(dirname "$0")/../../.."
out="${1:?usage: capture.sh out.jsonl [glob-cases.jsonl [zopen-cases.jsonl]]}"
cases="${2:-migration/corpus/src_leaves/seam/cases.jsonl}"
zcases="${3:-migration/corpus/src_leaves/seam/zopen.cases.jsonl}"
powershell -NoProfile -File migration/harness/src_leaves/build-driver.ps1 >/dev/null
mkdir -p "$(dirname "$out")"
rm -f "$out"
CPP2RUST_TAP_OUT="$out" migration/out/src_leaves/glob_driver.exe "$cases"
if [ -f "$zcases" ]; then
  powershell -NoProfile -File migration/harness/src_leaves/build-zopen-driver.ps1 >/dev/null
  zout="$(cd "$(dirname "$out")" && pwd)/$(basename "$out")"
  zcases_abs="$(cd "$(dirname "$zcases")" && pwd)/$(basename "$zcases")"
  # the driver writes its scratch file into the current directory
  (cd migration/out/src_leaves && CPP2RUST_TAP_OUT="$zout" ./zopen_driver.exe "$zcases_abs")
fi
