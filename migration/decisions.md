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
Scenario settings: abi-stability free; bug-compatibility ask-per-behavior; end-state hybrid-with-retained-cpp;
switch-default cpp-until-switch-task; ui-strategy keep-shell; migration-scope: ugrep's own `src/`
(`lib/` retained per D2; bundled third-party libraries and generated code retained).

Foundation notes (not user decisions):

- Toolchain pinned to Rust 1.99.1 (`rust-toolchain.toml`).
- Components of the plan were fixed from the depgraph, see README "Port order". The investigation's provisional
  pilot "glob + stats" was wrong: stats is inside the ugrep.hpp include cycle, glob is a true leaf.

Kinds: scope, third-party, plan acceptance, contract, policy, mask approval, behavior triage
(`intended`, `bug-preserve`, `bug-fix`, `dont-care`), header change, rebaseline, go/no-go.
