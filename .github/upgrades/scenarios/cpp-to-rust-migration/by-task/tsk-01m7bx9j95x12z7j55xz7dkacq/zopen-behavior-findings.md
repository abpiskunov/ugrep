# zopen behavior findings (Behavior task, input to Triage)

Recorded from the C++ Base-Release build through the seam tap (`zopen_driver`); 105 cases `zo-*` in
`migration/corpus/src_leaves/seam/zopen.cases.jsonl`, events appended to `seam/all.trace.jsonl`.
Two captures of the whole trace are byte-identical, so there is no noise and no mask is proposed.

Facts that shape the work:
- Only the `.Z` (LZW) format is implemented in zopen.c. No codec library is linked in this build and
  ugrep.exe does not reach zopen (`-z` unavailable), so the seam trace is the only witness.
- tests/ holds no `.Z` archive; compressed inputs come from C++ writes plus hand-crafted streams.

Suspicious behaviors (catalog status `hypothesis`, `suspicious: true`, awaiting Triage):
| Entry | Behavior |
|---|---|
| B-src_leaves-030 | `z_open` never sets `zs_mode`, so `z_close` never flushes: the tail of a written stream is lost (300 bytes written read back as 279) |
| B-src_leaves-031 | `z_close` with no write emits nothing (not even the header); an empty stream is a 0-byte file |
| B-src_leaves-033 | `z_write` works on a handle opened with mode `r` |
| B-src_leaves-034 | writer accepts bits 1, 9, 11; the reader rejects the result |
| B-src_leaves-039 | a truncated stream reads as a clean EOF (no error) |
| B-src_leaves-041 | three misuse paths crash the C++ (segfault); not recordable, cases dropped |

The three dropped crash cases: `z_read` after `z_write` on one handle; `z_write` after `z_read` on a
read handle; a second `z_read` after EOF of an empty or header-only stream.

Uncovered regions (reasons in the catalog): fwrite failures in `output()`, calloc failure, the dead
`zmode == 'w'` branch of `z_close`.
