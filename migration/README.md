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

## ugrep 7.8.5 (Foundation, 2026-11-13)

Product: CLI search tool (hybrid-with-retained-cpp; decisions in `decisions.md`). Primary build
Windows x64, MSBuild, toolset v143. Rust pinned to 1.99.1 (`rust-toolchain.toml`), workspace in
`rust/Cargo.toml` (no crates yet; every component is `cpp`).

### Baseline build

```text
msbuild msvc/ugrep.sln /m /p:Configuration=Base-Release /p:Platform=x64 /p:PlatformToolset=v143
```

MSBuild is `...\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\amd64\MSBuild.exe` and is not on PATH.
Output is `msvc\bin\Base-Release-x64\ugrep.exe` (`ugrep --version`: 7.8.5 WIN64 +avx2). The tracked
`bin\win64\ugrep.exe` is a prebuilt release binary, not a build product; the build does not touch it.

Findings that differ from the investigation, with the glue that fixes them (no C++ source edited):

1. `msvc/ugrep.vcxproj` is stale against the 7.8.5 `lib/` layout: it compiles obsolete wrappers in
   `msvc/include_ugrep/matcher_avx*.cpp` and lacks `lib/simd.cpp`, `letter_case.cpp`, `composer.cpp`,
   `matcher_avx2.cpp`, `matcher_avx512bw.cpp`, and gives `lib/simd_avx*.cpp` no `/arch` flag. The v143
   command failed (C1189, then LNK2001/LNK2005) until `msvc/Directory.Build.targets` repaired the item list
   for the `ugrep` project. The project file is unchanged.
2. Only the `Base-*` configurations build: the bzip2, lz4, pcre2, xz, zlib and zstd sources are not in this
   checkout, so `Pcre-*` fails. The Base binary has no `-P` and no `-z` decompression; archive cases cannot be
   recorded. Corpus cases use plain text and inline files.
3. `tests/verify.sh` (bash, cygwin) fails on Windows with the Base build (`ugrep --sort -Zio '' lorem.utf8.txt`),
   so it is not a baseline. The `cpp-suite` check of the gates is the MSBuild build; `baseline` also compares the
   recorded surface (`migration/corpus/utf8/surface`, 8 cases x 3 env rows, repeat 3, no flaky case).
   `corpus/test-ids.txt` is not created for that reason.
4. The default branch is `master`; policy commands use `origin/master`.

Run gates with MSBuild on PATH (CI uses `microsoft/setup-msbuild`): `gate-check --gate baseline --target utf8` gave
`GATE baseline utf8: PASS`; `policy-scan --base origin/master`: PASS (0 findings).

### Build glue

`msvc/Directory.Build.props` imports `msvc/rust.props` (target triple, profile, umbrella path
`msvc/bin/cargo/<triple>/<profile>/rust_umbrella.lib`, `CppRustAnyRust=false`, tap variant). `msvc/Directory.Build.targets` imports
`msvc/rust.targets` (`CargoBuild` and link wiring, active only when `CppRustAnyRust=true`; the tap define `CPP2RUST_TAP` with
`/p:Cpp2RustTap=true`). Each component's Prepare seam task adds `CppRustImpl_<COMPONENT>` (default `cpp`) and sets `CppRustAnyRust`.
Switch option names are `CppRustImpl_<COMPONENT>` (MSBuild form of `CPP_RUST_IMPL_<COMPONENT>`). `cxxbridge`, the umbrella crate
`rust_umbrella` and the `RustDebug` configuration come with the first Prepare seam task (verify: untested until a crate exists).

### Dependency graph (`depgraph --src-root . --map migration/depmap.json`, result `migration/depgraph.json`)

No compile database exists; the source-root fallback groups files per translation unit (`depmap.json`). Angle-bracket
includes of `include/reflex/*` are not resolved, so edges inside `lib/` are absent (all level 0); the order inside `lib/` is
from reading the code: utf8, unicode, posix/error/debug, input, then convert, pattern, matcher, composer.

| Level | Components |
|---|---|
| 0 | lib/utf8, lib/unicode, lib/posix, lib/error, lib/debug, lib/input, lib/convert, lib/pattern, lib/matcher, lib/composer, lib/simd, generated tables (lib/*_scripts, letter_case, block_scripts), src/glob, src/flag, src/zopen, lzma/C |
| 1 | src/zstream (<- zopen), lzma/7zls |
| 2 | src/zthread; the `src/` cycle below |
| 3 | src/ugrep-indexer (glob, zstream, zthread) |

**Cycle:** `src/ugrep`, `cnf`, `stats`, `output`, `query`, `screen`, `vkey`, `mmap` form one strongly connected group through
`ugrep.hpp` globals. Correction to the provisional waves: `cnf`, `stats` and `output` are not independent leaves
or L3 leaves; porting them needs a Prepare seam that cuts the group (move shared state behind an accessor) or porting
`ugrep.cpp` state first. `glob` (level 0) and `utf8`, `unicode` (lib leaves) are confirmed level 0.
The plan defines the components `utf8`, `unicode` and `glob`; further components are added by a Plan amendment.
