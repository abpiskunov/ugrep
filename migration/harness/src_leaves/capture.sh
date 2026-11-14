#!/usr/bin/env bash
# Records the C++ seam trace of src_leaves (glob_match, then z_open/z_read/z_write/z_close, then the zstream facade
# zs_*), the same events as migration/corpus/src_leaves/seam/all.trace.jsonl:
#   capture.sh <out.jsonl> [glob-cases.jsonl [zopen-cases.jsonl [zstream-cases.jsonl]]]
# zstream needs the codec libraries in <repo>/../codec-build/lib: migration/harness/zstream/fetch-codecs.ps1 fetches
# (pinned, SHA-256 verified) and builds them outside the repository when one is missing.
set -e
cd "$(dirname "$0")/../../.."
out="${1:?usage: capture.sh out.jsonl [glob-cases.jsonl [zopen-cases.jsonl [zstream-cases.jsonl]]]}"
cases="${2:-migration/corpus/src_leaves/seam/cases.jsonl}"
zcases="${3:-migration/corpus/src_leaves/seam/zopen.cases.jsonl}"
zscases="${4:-migration/corpus/src_leaves/zstream/seam/cases.jsonl}"
powershell -NoProfile -File migration/harness/src_leaves/build-driver.ps1 >/dev/null
mkdir -p "$(dirname "$out")"
rm -f "$out"
zout="$(cd "$(dirname "$out")" && pwd)/$(basename "$out")"
CPP2RUST_TAP_OUT="$out" migration/out/src_leaves/glob_driver.exe "$cases"
if [ -f "$zcases" ]; then
  powershell -NoProfile -File migration/harness/src_leaves/build-zopen-driver.ps1 >/dev/null
  zcases_abs="$(cd "$(dirname "$zcases")" && pwd)/$(basename "$zcases")"
  # the driver writes its scratch file into the current directory
  (cd migration/out/src_leaves && CPP2RUST_TAP_OUT="$zout" ./zopen_driver.exe "$zcases_abs")
fi
if [ -f "$zscases" ]; then
  codecs="$(cd .. && pwd)/codec-build/lib"
  for l in zlib bz2 lzma lz4 zstd viiz; do
    [ -f "$codecs/$l.lib" ] || { powershell -NoProfile -File migration/harness/zstream/fetch-codecs.ps1 >/dev/null; break; }
  done
  powershell -NoProfile -File migration/harness/zstream/build-zstream-driver.ps1 >/dev/null
  zscases_abs="$(cd "$(dirname "$zscases")" && pwd)/$(basename "$zscases")"
  # the driver writes its scratch file into the current directory
  (cd migration/out/zstream && CPP2RUST_TAP_OUT="$zout" ./zstream_driver.exe "$zscases_abs")
fi
