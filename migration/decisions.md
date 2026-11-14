# Decisions

One line per user decision. The id is the `ask_user` question id. Tasks append; nobody rewrites.

| Id | Date | Kind | Subject | Answer | Recorded in |
|---|---|---|---|---|---|
| Question v1 (D1) | 2026-11-13 | scope | Baseline repair | Repair only, no prebuilt reference: v142->v143, per-file /arch for simd_avx2/avx512bw, remove duplicate reflex::Matcher::advance_string* objects. `bin/win64/ugrep.exe` is NOT the C++ reference; the reference is the repaired source build (Base-Release\|x64). | `msvc/*.vcxproj`, environment.json |
| Question v2 (D2) | 2026-11-13 | third-party | RE-flex (`lib/`, `include/reflex`) | Retain as C++; port ugrep `src/` only. Called through the bridge. | plan `third_party` |
| Question v3 (D3) | 2026-11-13 | contract | Contractual output | Exit codes (0/1/2), stdout, stderr text and output formats (text, json, csv, xml, --format) are byte-for-byte. Masked/excluded: ordering under -J threads, --stats timings, TUI (-Q) ANSI timing. TUI not otherwise contractual (keep-shell). | catalog `observation`, masks |
| Question v4 (D4) | 2026-11-13 | policy | Policies, inputs, matrix | panic=abort; reproduce C++ integer wrapping; OOM abort accepted; data races fixed and reported. Thresholds: template defaults. Matrix: Windows x64 MSVC only. Inputs: tests/ archives plus generated corpora. | this file, `env-matrix.json` |
| Question v5 (D5) | 2026-11-13 | plan acceptance | The validation plan | accepted | plan `accepted` |
| Q-glob-1 (Triage, 2026-11-13) | behavior triage | B-src_leaves-013: `**` not followed by `/` never matches | bug-preserve | catalog B-src_leaves-013 |
| Q-glob-2 (Triage, 2026-11-13) | behavior triage | B-src_leaves-019: `?` on invalid/truncated UTF-8 consumes bytes inconsistently | bug-preserve | catalog B-src_leaves-019 |
| Q-glob-3 (Triage, 2026-11-13) | behavior triage | B-src_leaves-020: unterminated class `[a` accepted; `[^` matches any char | bug-preserve | catalog B-src_leaves-020 |
| Q-glob-4 (Triage, 2026-11-13) | behavior triage | B-src_leaves-021: `]` right after `[` closes the class (`[]]` matches nothing) | bug-preserve | catalog B-src_leaves-021 |
| Q-glob-5 (Triage, 2026-11-13) | behavior triage | B-src_leaves-023: `--include-dir=s*` + `--exclude-dir=s2` and `-g s/,*.txt` select nothing | bug-preserve | catalog B-src_leaves-023 |
| Q-glob-masks (Triage, 2026-11-13) | mask approval | src_leaves: proposed masks (none; the C++ run against itself is identical) | no masks | `masks/src_leaves.json` |
| P-zopen-1 (Prepare seam, 2026-11-13) | harness | zopen is a second Rust module: it lives in `rust/src_leaves/src/zopen.rs`, declared `#[path = "zopen.rs"] pub mod zopen;` in `signatures.rs`. The harness's `#[path]` include of `signatures.rs` resolves it, built and checked with `cargo build -p src_leaves-harness` and `cargo clippy -p src_leaves`; no judge-only harness change is needed for the module. The harness still needs the zopen dispatch (`src_leaves.z_*` in `main.rs`) and a way to give `z_open` a stream: that is a judge change for the Behavior task, not Port. | `rust/src_leaves/SEAM.md` |
| P-zopen-2 (Prepare seam, 2026-11-13) | switch | zopen has its own build sub-switch `CPP_RUST_IMPL_SRC_LEAVES_ZOPEN` (default `cpp`, defines `CPP_RUST_IMPL_ZOPEN`), because `CPP_RUST_IMPL_SRC_LEAVES` already defaults to `rust` for the ported glob and the stub zopen must not become active. Switch over merges it into the component switch. cargo-nextest 0.9.152 is present on this worker. | `msvc/rust.props`, `msvc/rust.targets` |
| Q-zopen-1 (Triage, 2026-11-14) | behavior triage | B-src_leaves-030: z_open never sets zs_mode, z_close never flushes, written stream loses its tail | dont-care | catalog B-src_leaves-030 |
| Q-zopen-2 (Triage, 2026-11-14) | behavior triage | B-src_leaves-031: z_close with no writes emits nothing; empty stream is a 0-byte file z_read rejects | dont-care | catalog B-src_leaves-031 |
| Q-zopen-3 (Triage, 2026-11-14) | behavior triage | B-src_leaves-033: z_write succeeds on a handle opened with mode r | dont-care | catalog B-src_leaves-033 |
| Q-zopen-4 (Triage, 2026-11-14) | behavior triage | B-src_leaves-034: writer accepts bits 1, 9, 11 and produces a stream z_read rejects | dont-care | catalog B-src_leaves-034 |
| Q-zopen-5 (Triage, 2026-11-14) | behavior triage | B-src_leaves-039: truncated .Z stream reads as clean EOF with shorter data | bug-preserve | catalog B-src_leaves-039 |
| Q-zopen-6 (Triage, 2026-11-14) | behavior triage | B-src_leaves-041: three handle-misuse paths segfault in C++ | bug-fix (return -1 / 0, no crash; Rust-side tests only) | catalog B-src_leaves-041, plan D-zopen-1 |
| Q-zopen-masks (Triage, 2026-11-14) | mask approval | src_leaves zopen: proposed masks (none; 289 seam cases byte-identical C++ vs C++); no header change to src/zopen.h | no masks, no header change | `masks/src_leaves.json` |
| D-zopen-2 (Port review, 2026-11-14) | bug-fix | A crafted .Z stream can make the decode prefix chain a cycle (first code unchecked, e.g. codes `[300] + [65]*42 + [257, 65, 300]`); the C has undefined behaviour (de_stack overrun), the Rust would grow its stack without bound. Rust caps the decode stack at `1 << BITS`, sets errno EINVAL and returns -1 like the existing bad-stream error. Same class as Q-zopen-6 (fail safely) | bug-fix, Rust-side test `crafted_prefix_cycle_is_bounded` | `rust/src_leaves/src/zopen.rs` |
| D-zopen-3 (Port review, 2026-11-14) | intentional difference | After a mid-stream bad-code error a further `z_read` returns -1 in Rust; the C resumes and emits garbage. ugrep closes on num <= 0, so it is unaffected | intended | `rust/src_leaves/src/zopen.rs` |
| D-zopen-4 (Port review, 2026-11-14) | dont-care | Writer divide by zero when `bytes_out` wraps on a 32-bit `c_long` is writer-only (covered by Q-zopen-1); Rust uses `checked_div` so it cannot panic across FFI | dont-care | `rust/src_leaves/src/zopen.rs` |

Scenario settings: abi-stability free; bug-compatibility ask-per-behavior; end-state hybrid-with-retained-cpp;
switch-default cpp-until-switch-task; ui-strategy keep-shell; migration-scope: ugrep's own `src/`
(`lib/` retained per D2; bundled third-party libraries and generated code retained).

Foundation notes (not user decisions):

- Toolchain pinned to Rust 1.99.1 (`rust-toolchain.toml`).
- Components of the plan were fixed from the depgraph, see README "Port order". The investigation's provisional
  pilot "glob + stats" was wrong: stats is inside the ugrep.hpp include cycle, glob is a true leaf.

Kinds: scope, third-party, plan acceptance, contract, policy, mask approval, behavior triage
(`intended`, `bug-preserve`, `bug-fix`, `dont-care`), header change, rebaseline, go/no-go.
