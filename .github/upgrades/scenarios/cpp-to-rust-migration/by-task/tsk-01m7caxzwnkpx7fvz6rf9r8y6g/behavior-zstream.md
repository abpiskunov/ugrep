# Behavior: zstream (src_leaves)

Gates: `GATE behavior-ready src_leaves: PASS` (migration/out/gates/behavior-ready/src_leaves/report.json),
`GATE behavior-proposed src_leaves: PASS`, catalog-lint PASS, policy-scan PASS (0 findings).

## Codec-enabled C++ reference (D-zstream-codecref)
Pinned sources fetched outside the repo by `migration/harness/zstream/fetch-codecs.ps1` (SHA-256 in
`migration/environment.json` `codec_reference`), built by `build-codec-ref.ps1` into `msvc/bin/CodecRef-x64`.

Macros enabled: HAVE_LIBZ, HAVE_LIBBZ2, HAVE_LIBLZMA, HAVE_LIBLZ4, HAVE_LIBZSTD, 7z (WITH_NO_7ZIP undefined),
WITH_MAX_7ZIP_SIZE = 1 GiB. Not enabled: HAVE_LIBBROTLI, HAVE_LIBBZIP3, WITH_ZIP_CRC32.

| Codec | Status |
|---|---|
| gzip/zlib, zip, bzip2, xz/lzma, lz4, zstd, 7z, compress .Z, tar | built |
| brotli, bz3 | NOT built (no cases) |

## Recorded
197 surface cases (`migration/corpus/src_leaves/zstream/cases.jsonl`, ids `zs-*`, 3 repeats, flaky 0) and
33 seam cases (`zss-*`, `zstream_driver`, two runs byte-identical). No noise, no masks.
Catalog entries B-src_leaves-042..057.

## Suspicious (for Triage)
049 truncated gz/xz/zstd/.Z/tar: partial output, exit 0. 050 truncation reported inconsistently (bz2, lz4, zip, 7z).
051 corrupt or trailing garbage (lz4/zstd "Binary file matches" exit 0; xz trailing garbage exit 1).
052 xz memory limit gives the generic error. 053 7z size limit and misleading bad-signature message.
054 zip CRC32 not verified; a corrupt deflate entry is skipped silently.

## Caveats
- The plan's behavior-ready gate replays only the top-level glob/zopen cases against Base-Release; the zstream corpus
  is checked by catalog-lint only. Replay against CodecRef needs a Plan amendment (plan not edited here).
- .Z cases are small streams only (no multi-width LZW encoder); zip LZMA (method 14) case may not match ugrep's expected stream.
- CI must run fetch-codecs.ps1, build-codec-ref.ps1 and build-zstream-driver.ps1 to reproduce.
