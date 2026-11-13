# src_leaves seam notes

Interop: **cxx** (default row; C++ boundary with a plain function). Only `extern "Rust"` functions, so the
C++ facade calls Rust; Rust never calls the component's legacy C++. cxx-build compiles the generated glue
into the crate and writes `lib.rs.h` under `<cargo target>/cxxbridge/src_leaves/src/`, which
`msvc/rust.targets` adds to the include path of the Rust variant.

## Facade

| Seam id | C++ facade | Rust | Switch |
|---|---|---|---|
| `src_leaves.glob_match` | `glob_match` in `src/glob.cpp` (declaration unchanged in `src/glob.hpp`) | `ugrep_rust::glob_match` -> `signatures::glob_match` | `CPP_RUST_IMPL_SRC_LEAVES` |

The legacy body is `static glob_match_cpp` in `src/glob.cpp`; the facade calls it, or `rust_glob_match`
(`src/glob_rust.hpp`) when `CPP_RUST_IMPL_SRC_LEAVES` is defined. Default default arguments stay in `glob.hpp`.

## Crossing types

| Parameter | C++ | Rust | encoding / owner |
|---|---|---|---|
| pathname, basename, glob | `const char*`, NUL-terminated, borrowed for the call | `&[u8]` (no NUL) | bytes (UTF-8 multi-byte sequences are decoded by the matcher; not validated); caller owns |
| icase, lead, path | `bool` | `bool` | - |
| return | `bool` | `bool` | - |

Error kinds: none (pure function; the C++ does not throw). The C++ treats `basename` as always non-NULL when
the glob has no `/`; a NULL `basename` would crash in C++ and in `strlen` of the facade alike (not a behavior to keep).

## Decisions

- Panic strategy: `abort` (decision D4, `panic = "abort"` in the workspace release profile).
- Re-entrancy: none, no outbound C++ calls. Thread affinity: stateless free function, `multi`.
- Opaque types, globals, templates, shims: none.
- Owner of global state: none.

## Tap

C++: `TAP_CALL` / `TAP_RET` in the facade, compiled with `/p:CppRustTap=true` (`CPP2RUST_TAP`, C++17).
Header: `migration/harness/src_leaves/cpp2rust_tap.hpp`. No outgoing calls, so no `TAP_OUT`, no state.
Rust-side tap and replay (`rust/src_leaves-harness`) belong to the Behavior task.

## Not in this seam yet

`zopen`, `zstream.hpp`, `flag.hpp`: later Prepare seam tasks of the component.
