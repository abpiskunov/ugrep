# Decisions

One line per user decision. The id is the `ask_user` question id. Tasks append; nobody rewrites.
All of the Specifying answers were given as one batch, art_01M7B1Z66B0CXMNJ8KS9WXR9SR (Q1-Q6); the
DQ ids below name each question of that batch.

| Id | Date | Kind | Subject | Answer | Recorded in |
|---|---|---|---|---|---|
| DQ-0001 (Q2) | 2026-11-13 | scope | Targets in scope; what stays C++ | hybrid-with-retained-cpp. Port leaves (utf8, unicode, glob, cnf, stats, input), RE-flex (convert/pattern/matcher) and output formatting. TUI (-Q: query/screen/vkey), ugrep.cpp shell, ugrep-indexer and wrapper scripts stay C++. Default stays `cpp` until each component's Switch over | plan `components` |
| DQ-0002 (Q3) | 2026-11-13 | third-party | Keep or replace | keep-all: zlib, bzip2, xz/lzma, lz4, zstd, pcre2 stay C behind the bridge; generated unicode tables retained | plan `third_party` |
| DQ-0003 | 2026-11-13 | plan acceptance | The whole validation plan | accepted through the Q1-Q6 batch (no separate plan question was asked) | plan `accepted` |
| DQ-0004 (Q4) | 2026-11-13 | contract | Contractual outputs | exit codes 0/1/2; stdout and stderr text for text, json, xml, csv, hex and color. Order only with `--sort`. Masked: `--stats` timings and the version string | catalog `observation`, masks |
| DQ-0005 (Q5) | 2026-11-13 | policy | Panic, overflow, OOM; inputs | panic=abort; integer overflow reproduces C++ wrapping; OOM abort accepted; data races fixed and reported; bug-compatibility asked per behavior in each Triage; corpus inputs are tests/ samples only, no capture build | this file; `rust/Cargo.toml` |
| DQ-0006 (Q6) | 2026-11-13 | thresholds | Plan thresholds, environment matrix | template defaults, no changes; Windows x64 MSVC Base-Release primary; default techniques only, no opt-in sanitizers or coverage | plan `gates` |
| DQ-0007 (Q1) | 2026-11-13 | rebaseline | Baseline repair | apply: `/p:PlatformToolset=v143` on the command line (the vcxproj files hard-code v142, not installed) | plan `product.build_command` |

Kinds: scope, third-party, plan acceptance, contract, policy, mask approval, behavior triage
(`intended`, `bug-preserve`, `bug-fix`, `dont-care`), header change, rebaseline, go/no-go.
