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
Header: `rust/src_leaves/cpp2rust_tap.hpp`. No outgoing calls, so no `TAP_OUT`, no state.
Rust-side tap and replay (`rust/src_leaves-harness`) belong to the Behavior task.

## Facade: unit zopen (src/zopen.c, plain C, `extern "C"`)

| Seam id | C++ facade (`src/zopen_facade.cpp`) | Legacy body (`src/zopen.c`) | Rust (C ABI, `rust/src_leaves/src/ffi/zopen.rs`) | Signature module |
|---|---|---|---|---|
| `src_leaves.z_open` | `z_open(FILE*, const char *mode, int bits, int plain)` -> `void*` | `z_open_cpp` | `ugrep_rust_z_open` | `signatures::zopen::z_open` |
| `src_leaves.z_close` | `z_close(void*)` -> `int` | `z_close_cpp` | `ugrep_rust_z_close` | `signatures::zopen::z_close` |
| `src_leaves.z_read` | `z_read(void*, unsigned char*, int)` -> `int` | `z_read_cpp` | `ugrep_rust_z_read` | `signatures::zopen::z_read` |
| `src_leaves.z_write` | `z_write(void*, const unsigned char*, int)` -> `int` | `z_write_cpp` | `ugrep_rust_z_write` | `signatures::zopen::z_write` |

`src/zopen.h` is unchanged. `src/zopen_seam.h` declares the legacy bodies and the Rust entry points. The only edits to
`src/zopen.c` are the four renames (`z_*` -> `z_*_cpp`) and the include of `zopen_seam.h`. Both vcxproj files compile the facade.

Switch: `/p:CPP_RUST_IMPL_SRC_LEAVES_ZOPEN=rust` defines `CPP_RUST_IMPL_ZOPEN` (and links the umbrella); default `cpp`.
It is a separate sub-switch because the stub Rust zopen must not become active through `CPP_RUST_IMPL_SRC_LEAVES`, which
already defaults to `rust` for the ported glob. The Switch over task folds it into `CPP_RUST_IMPL_SRC_LEAVES`.

Crossing: `FILE*` is passed as an opaque `*mut c_void` (caller owns it; `z_close` frees the state only, the caller does
`fclose`); the handle is a leaked `Box<ZState>` from `z_open` to `z_close`; `mode` is the C string bytes; buffers are slices.
`z_open` returns NULL on failure, `z_read` returns 0 at end of file and negative on error, `z_close` 0 or -1. No panics cross
(`panic = "abort"`), unsafe only in `src/ffi/zopen.rs`. Retained codecs (zlib, bzip2, ...) are not touched by zopen.c: it only
implements the `.Z` (LZW) format; `zstream.hpp` calls the facade.

Tap: `TAP_CALL`/`TAP_STATE`/`TAP_RET` in the facade: handles and `FILE*` as `#n` object ids, `z_read` bytes read as `state`,
`z_write` bytes as an argument. No outgoing calls. The driver and capture/coverage glue are judge files
(`migration/harness/src_leaves/`, protected by policy-scan, so not in this implementation change): the Behavior task adds
them (`zopen_driver.cpp`, `build-zopen-driver.ps1`, `build-zopen-coverage.ps1`, `capture-zopen.sh`, `coverage-zopen.sh`);
a smoke-tested draft is in the run's artifacts. Rust-side replay dispatch for the zopen ids is Behavior work too.

Port status: `signatures::zopen` (`src/zopen.rs`) is ported; libc `FILE*`/errno calls are in `src/ffi/cfile.rs`. The Rust returns -1
for a handle used out of sequence (D-zopen-1); `ugrep_rust_z_read/write` accept a NULL buffer with len 0 like the C.

Note: in the Base-Release build zopen is not reachable from `ugrep.exe` (the recorded surface shows `-z` is not available;
the linker map has no zopen object), so the seam trace is the only witness for this unit.

## Not in this seam yet

`zstream.hpp`, `flag.hpp`: later Prepare seam tasks of the component.
