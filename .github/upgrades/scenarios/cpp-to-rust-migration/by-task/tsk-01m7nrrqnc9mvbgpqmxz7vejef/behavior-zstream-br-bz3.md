# Behavior: zstream .br/.bz3 reference and corpus (S6, D-S6-1)

Addendum to `by-task/tsk-01m7caxzwnkpx7fvz6rf9r8y6g/behavior-zstream.md`. The intent named `migration/behavior-zstream.md`;
that file does not exist, the catalog is `migration/behavior/src_leaves.yaml` (entries B-src_leaves-058..068).

## Reference
- `fetch-codecs.ps1` now also fetches brotli 1.1.0 and bzip3 1.5.1 (SHA-256 pinned, approval recorded in D-S6-1) and builds
  `brotlicommon.lib`, `brotlidec.lib`, `brotlienc.lib` (encoder, corpus generator only) and `bzip3.lib` into `../codec-build/lib`.
  bzip3 publishes no digest: the hash was computed on first download and the size (413067 bytes) matched the release asset.
- `build-codec-ref.ps1` builds CodecRef-x64 from a detached git worktree of the parent of 9f87c32 (`../codec-ref-src`) with
  HAVE_LIBBROTLI, HAVE_LIBBZIP3 and `/DNOMINMAX` (the old `BZ3` struct has a member `max`, which the Windows macro breaks).
- Determinism: the 197 earlier cases re-run on the rebuilt reference: `cases 197, divergent 0`. The 90 new cases: run-cases `--repeat 3`
  flaky 0, and two independent recordings `trace-diff`: `cases 90, divergent 0`. Appended corpus (287 cases) vs the trace: divergent 0.

## Corpus (append only)
`gen_br_bz3_cases.py` (encoder `zenc_br_bz3.c`, `build-zenc-br-bz3.ps1`) appended 90 cases (`zs-br-*`, `zs-bz3-*`, `zs-tar-br-*`,
`zs-tar-bz3-*`) to `corpus/src_leaves/zstream/cases.jsonl` and the recording to `surface/cases.trace.jsonl`. No existing line changed
(`git diff`: 0 removed lines in `migration/corpus`).
Valid, truncated, corrupt, empty, multi-stream/multi-block, large, tar-in-codec, options, extension and multi-file variants for both codecs.

## Not admitted
Seven bz3 header cases: bad magic / version (4 crash, Windows fast-fail 0xC0000409) and files shorter than 9 bytes (stale buffer
bytes decide the result). B-src_leaves-068; listed in the generator.

## Suspicious (for Triage)
061 only the first brotli stream is decoded. 062 truncated brotli silent. 063 brotli corrupt data: message only sometimes.
064 bz3 truncation after a full block silent exit 0. 065 bz3 header errors report 2-3 times, exit 2. 066 bz3 original-size field trusted.
067 bz3 trailing 8+ bytes / concatenated files: message with exit 0. 068 bz3 header crash / stale buffer.

## Notes for the Port task
Plan gates replay `corpus/src_leaves/cases.jsonl` on Base-Release only; the zstream corpus needs CodecRef-style binaries
(a Plan amendment, as already noted for the 197 cases). Rebuild: fetch-codecs.ps1, build-codec-ref.ps1, build-zenc-br-bz3.ps1.
