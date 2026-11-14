# Translation rules of this migration

Project conventions that every Port task follows. Start from the plugin rulebook
(`cpp-rust-idioms`); add a rule here only when a Wave review found the same question twice.

| # | C++ construct | Rust form | Since |
|---|---|---|---|
| 1 | (example) `Status` return + out-parameter | `Result<T, Error>`; the facade maps back | wave L0 |

### Rules learned from the zopen port (src_leaves)

| # | C++ construct | Rust form | Evidence |
|---|---|---|---|
| 2 | A second Rust module of a component whose replay harness `#[path]`-includes only `signatures.rs` | Put the module in its own file next to `signatures.rs` and declare it there with `#[path = "x.rs"] pub mod x;`; no harness change (P-zopen-1). Files such a module needs (e.g. libc wrappers) are pulled in by `#[path]` from the module, never by `crate::` paths, because `crate` is the harness in the replay build | harness multi-module finding; `zopen.rs` builds in both crates |
| 3 | `FILE*`, `fread`/`fwrite`, `errno` | Opaque `*mut c_void` stream in the state; the only unsafe is a small wrapper file under `src/ffi/` (`cfile.rs`) with SAFETY comments; errno through `_errno` / `__errno_location` / `__error` per platform | zopen unit gate: errno is recorded per call in the seam trace |
| 4 | C `long`, `unsigned`, wrapping counters | `core::ffi::c_long` (32 bit on Windows) with `wrapping_*`; casts written as `as i64 as i32` where clippy flags a same-type cast on one platform only | D4 integer wrapping; clippy `unnecessary_cast` on Windows |
| 5 | C union overlaying read and write tables, `goto` into a loop | Separate fields and a `resume` flag; a handle used the other way round is refused with -1 instead of undefined behaviour | catalog B-src_leaves-041, plan D-zopen-1 (Rust-side tests) |
| 6 | Bug-compat decisions | `bug-preserve` and `dont-care` behaviors are ported as they are (`zs_mode` never set, `z_close` never flushes, a truncated stream is a clean EOF); only approved `bug-fix` differences change behavior | Q-zopen-1..6 |
| 7 | Unit gate `rust-tests` | Needs `cargo-nextest` on the worker (the pilot's glob gate failed on it); preflight `cargo nextest --version` before the gate | pilot Wave review, P-zopen-2 |

A construct with no rule: mirror the C++ statement by statement, mark it
`// TODO(port): rule-gap <what>`, and list it in the unit record's `notes`.
