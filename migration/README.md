# migration/ - the state of the C++ to Rust migration

Everything a later session, a reviewer or CI needs is in this folder. Nothing here is a cache of
a tool: files are written by tasks, checked by the migration tools, and read by people.

| Path | What | Written by | May a Port task change it? |
|---|---|---|---|
| `validation-plan.yaml` | Accepted plan: components, third-party decisions, comparison settings, thresholds, gates, policy globs | Specifying; later only a `Plan amendment:` task | No |
| `decisions.md` | User decisions with question ids | Any task that asked | Append only |
| `environment.json` | Preflight result: Rust, C++ and build toolchains, network, context | Investigating, Foundation | No (report re-checks in the task result) |
| `depgraph.json` | Dependency graph and port order (`cpp2rust-tools depgraph`) | Investigating, Planning | No (regenerate) |
| `behavior/<component>.yaml` | Behavior catalog: what the C++ does, each entry citing a passing check | Behavior tasks | No |
| `corpus/<component>/cases.jsonl` | Surface cases: inputs only | Behavior tasks | No (append-only for Behavior tasks) |
| `corpus/<component>/seam/*.trace.jsonl` | Recorded C++ seam traces: inputs and expected results | Behavior tasks, by running the C++ | No |
| `corpus/<component>/surface/*.trace.jsonl` | Recorded C++ surface observations | Behavior tasks, by running the C++ | No |
| `corpus/test-ids.txt` | Test ids of the C++ suite at the start | Foundation | No |
| `masks/<component>.json` | Approved noise masks and rewrites | Behavior tasks propose; the user approves | No |
| `harness/<component>/` | Tap header, facade taps, capture and replay glue, properties | Seam and Behavior tasks | No |
| `units/<unit>.yaml` | One record per port unit | Planning; the unit's own task | Its own record only |
| `translation-rules.md` | Project translation conventions | Foundation; Plan amendment | No |
| `tools/cpp2rust-tools/` | The migration tools crate, copied from the scenario skill, so CI and people run the same code; `target/` is not committed | Foundation; later only a judge-only pull request | No |
| `out/` | Logs and results of local runs. Not committed (`.gitignore`) | Any run | n/a |

Rules:

1. Expected values are **recorded from the C++**, never typed. Recorded files are append-only.
2. One change is either a judge change (plan, corpus, masks, harness, tools) or an
   implementation change, never both. `cpp2rust-tools policy-scan` enforces it.
3. A unit is done when `cpp2rust-tools gate-check --gate unit --target <unit>` passes on the commit
   **in CI**. A local pass is a claim.
4. Thresholds live only in `validation-plan.yaml`.

The toolchain is pinned in `rust-toolchain.toml` at the repository root. `cpp2rust-tools` means:

```text
cargo run -q --release --locked --manifest-path migration/tools/cpp2rust-tools/Cargo.toml -- <subcommand> ...
```

Run a gate: `cpp2rust-tools gate-check --gate <gate> --target <unit or component>`.
Results: `migration/out/gates/<gate>/<target>/report.json` and one log per check.

## ugrep specifics (Foundation)

**Baseline.** `powershell -NoProfile -File migration/harness/build-cpp.ps1` builds `msvc/ugrep.sln`
(Base-Release|x64) into `msvc/bin/Base-Release-x64/` (`ugrep.exe`, `ugrep-indexer.exe`). Repairs: toolset v143,
direct per-file `/arch` for `lib/simd_avx2.cpp`, `simd_avx512bw.cpp`, `matcher_avx2.cpp`, `matcher_avx512bw.cpp`
(replacing the `include_ugrep` wrappers that duplicated `matcher.obj`), new `msvc/ugrep-indexer.vcxproj`.
The committed `bin/win64/ugrep.exe` is not a reference (D1).

**Switch.** `msvc/rust.props` / `msvc/rust.targets` (imported by both projects): `CPP_RUST_IMPL_SRC_LEAVES`,
`_SRC_CORE`, `_SRC_TUI`, `_SRC_INDEXER` = `cpp` (default) or `rust`, passed as `/p:`. Any `rust` builds
`rust/rust_umbrella` (staticlib, `+crt-static`) with cargo into `build/cargo`, links it, writes a linker map and
outputs to `msvc/bin/Base-Release-x64-rust/`. `/p:CppRustTap=true` defines `CPP2RUST_TAP` and enables cargo feature `tap`.
With all switches at `cpp` nothing changes. The umbrella is empty until the first Port.

**Port order** (`depgraph.json`, source-root, file map `depmap.json`; no compile database):

| Level | Components |
|---|---|
| 0 | `src_leaves` (glob, zopen, flag.hpp), reflex (retained C++), lzma/C (retained) |
| 1 | `src_leaves` (zstream.hpp, depends on zopen); lzma (retained) |
| 2 | zthread; the cycle {ugrep, cnf, mmap, output, query, screen, stats, vkey} (all include ugrep.hpp) = `src_core` + `src_tui` |
| 3 | `src_indexer` (ugrep-indexer; depends on glob, zstream, zthread) |

**Finding for Planning.** The provisional pilot "glob + stats" does not hold: `stats` is inside the ugrep.hpp
cycle. `glob` is the true leaf, so the pilot is `src_leaves` (glob first). `src_core`/`src_tui` need a Prepare seam
that cuts the ugrep.hpp cycle before any Port.

**Corpus.** `corpus/<component>/cases.jsonl` (inputs) and `surface/cases.trace.jsonl` (recorded from the C++, 3 repeats,
env matrix `env-matrix.json`): src_core 41, src_leaves 28, src_tui 24, src_indexer 23. Cases that were not
deterministic (`--stats` timings, unordered multi-file output) were made deterministic (`--sort=name`, `-J1`) or dropped.
Masks are empty; D3 masks are added only when a Behavior task proves noise.

**Limits.** `tests/verify.sh` is not usable as a judge on Windows (see `environment.json`); the baseline gate is the
build plus the recorded surface. The Base configs do not link the third-party decompression libraries.
