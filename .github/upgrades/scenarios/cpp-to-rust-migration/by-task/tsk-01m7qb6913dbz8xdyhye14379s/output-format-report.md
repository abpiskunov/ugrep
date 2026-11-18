# output-format port report

Commit e31aaa4. Unit `output-format`, component `src_core`.

## Result

- `GATE unit-cleaned output-format: PASS` (migration/out/gates/unit-cleaned/output-format/report.json). `gate-check --gate unit` runs `unit-cleaned` because `src_core` is cleaned.
- Surface: 5034 cases, 0 divergent under the existing masks. Rust tests: 68 passed (nextest junit).
- `policy-scan --base HEAD --plan migration/validation-plan.yaml`: PASS (9 files changed, 0 findings).
- `cargo clippy -p src_core --all-targets`: clean. MSVC `build-cpp.ps1` (Base-Release-x64): OK.

## Scope

`Output::format` (count and match), `format_invert`, `capture` and the quote/cpp/csv/hex/json/xml emitters are Rust (`output_format.rs`, `output.rs`).
Rust is the only path: the `*_cpp` bodies, `Output::capture` and the inline matcher overloads are deleted; no build switch.

## Fix found by the gate

Case `cov-rand-1385` (`-vc -A2 -ou --format=%[1]b:%b:%O%~ -e ""`) printed empty where C++ printed `0`.
C++ returns `(*matcher)[N]` directly for a bare index, even when `group_id()` does not list the group.
The snapshot now carries `OutputCapture.listed` and `Need.indices`. Capture offsets are window-relative and the window widens for lookbehind captures.

## Operator notes (PR #90)

- `Sync::acquire/try_acquire/release/finish` use `lock->handle`.
- Stale wording fixed in `src/output_rust.hpp` and `rust/src_core/SEAM.md`.
- No scratch files committed.

## Hot-path benchmark

`migration/validation-plan.yaml` defines no benchmark or threshold. Measured here instead.
Setup: 22.6 MB file, 300000 lines. C++ baseline is commit 1672755 (C++ format path) built in a worktree; Rust is this commit.
MSVC Release x64, same machine, interleaved runs, min of 9, wall time, output to NUL.

| case | C++ ms | Rust ms | ratio |
|---|---|---|---|
| plain -c | 407 | 345 | 0.85 |
| --format %n:%o%~ | 433 | 508 | 1.17 |
| --format %q | 438 | 501 | 1.14 |
| --format %j | 428 | 463 | 1.08 |
| --json | 498 | 728 | 1.46 |
| --csv | 539 | 818 | 1.52 |
| --xml | 508 | 716 | 1.41 |
| -v --format | 470 | 533 | 1.13 |
| --replace | 645 | 615 | 0.95 |

Noise is high (a single-pass run gave 0.50 for plain `-c`). The emitter paths are about 40-50% slower: each field crosses the bridge and allocates a `Vec`, and `Opts` is rebuilt per call.
Not resolved. With no threshold in the plan this needs a user decision: accept, or follow up with batching or a Rust-owned buffer.

## Known differences (listed, not silently decided)

- `%Z` without a fuzzy matcher prints nothing where C++ dereferences NULL.
- With `--line-buffered`, flush points inside one call move to the append (`Output::lines` flushes after each newline).
- `strtol` clamps to a 32-bit long on Windows, as the MSVC C++ does.
- Autotools (autoreconf, configure, make) is proven by PR CI only.
