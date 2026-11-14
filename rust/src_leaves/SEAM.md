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

Switch: `CPP_RUST_IMPL_SRC_LEAVES` (default `rust`) defines `CPP_RUST_IMPL_SRC_LEAVES` and `CPP_RUST_IMPL_ZOPEN` and links
the umbrella; `/p:CPP_RUST_IMPL_SRC_LEAVES=cpp` builds the legacy C. The former sub-switch `CPP_RUST_IMPL_SRC_LEAVES_ZOPEN`
was folded into it by the Switch over task.

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

## Facade: unit group zstream (src/zstream.hpp, new src/zstream.cpp, `src/zstream_seam.h`)

`src/zstream.hpp` was 2914 lines (not the 2520 of the plan) and is now 853: class declaration, `viiz_stream`, the decoder
structs, the `std::streambuf` overrides (`underflow`, `uflow`, `showmanyc`, `xsgetn`), `u16/u32/u64`, and the declarations
of the legacy bodies `<name>_cpp`. The bodies moved to `src/zstream.cpp` (2520 lines, the whole file is inside `#ifdef HAVE_LIBZ`
like the include of the header). MSVC projects compile it separately and define `ZSTREAM_FACADE`; autotools builds
still compile only the header, which includes `zstream.cpp` at its end when `ZSTREAM_FACADE` is not set.
`zstreambuf` stays the thin C++ adapter: it owns the buffer, `cur_`, `len_` and the diagnostics, and forwards to a Rust
handle (`void *rust_`, NULL in the cpp build) when `CPP_RUST_IMPL_ZSTREAM` is defined.

| seam function | cxx / C ABI | tap id |
|---|---|---|
| `is_{bz,xz,lz4,zstd,br,bz3,Z,zip,7z,rar}` (10 predicates, `zstreambuf::is_*`) | cxx `ugrep_rust::zs_is_*` (`zs_is_z`) on `rust::Slice<const uint8_t>` path bytes | `src_leaves.zs_is_<fmt>` |
| `open`, `close`, `decompressing`, `next`, `zipinfo`, `zippeek` | C ABI `ugrep_rust_zs_{open,close,decompressing,next,at_end,zipinfo,zippeek}` in `ffi/zstream.rs` (a `FILE*` is not expressible in cxx, as for zopen) | `src_leaves.zs_open`, `zs_close`, `zs_decompressing`, `zs_next`, `zs_zipinfo`, `zs_zippeek` |
| diagnostics `cannot_decompress`, `warning` | Rust queues them, C++ drains `ugrep_rust_zs_diag` (Rust never calls C++) | outgoing `ugrep.cannot_decompress`, `ugrep.warning` |

Plain moves without facade: `ZipInfo::{header,decompress,descriptor,peek,read_num,read,crc32}`, `file_read`, `has_ext`.
Objects are tapped as `#n` (first-seen order per case), `zs_next` taps the produced bytes as state.

Macros: `HAVE_LIBBZ2/LIBLZMA/LIBLZ4/LIBZSTD/LIBBROTLI/LIBBZIP3`, `WITH_NO_7ZIP`, `WITH_ZIP_CRC32` and `WITH_MAX_7ZIP_SIZE` are
mirrored at runtime, not as Cargo features: the Rust staticlib is built once while the macros vary per program (ugrep,
ugrep-indexer) and configuration. The adapter passes `ugrep_rust_zs_config` (`ZS_CODEC_*` bit mask, `zip_crc32`,
`max_7zip_size`) to `ugrep_rust_zs_open`; zlib is always on. The codec libraries stay C and are reached from Rust through
`extern "C"` in the Port tasks. `ugrep.cpp` defines `WITH_MAX_7ZIP_SIZE` (1 GiB) and ugrep-indexer does not, so
`ugrep.vcxproj` passes `ZSTREAM_MAX_7ZIP_SIZE=1073741824` to `zstream.cpp` only. The Base-Release build defines no
`HAVE_LIB*`, so `zstream.cpp` is empty there; compile-checked outside it with HAVE_LIBZ/BZ2/LZ4/LZMA/ZSTD, in all four
combinations of cpp/rust and tap, and with `WITH_NO_7ZIP`, `WITH_ZIP_CRC32`.

Switch (merged, D-zstream-switch): the sub-switch `CPP_RUST_IMPL_SRC_LEAVES_ZSTREAM` is gone; `CPP_RUST_IMPL_SRC_LEAVES` (default `rust`) also sets `CPP_RUST_IMPL_ZSTREAM`
(msvc/rust.targets) and links the umbrella. Rust modules `zstream_codecs.rs`, `zstream_core.rs` (stubs) are reached through
`#[path]` from `signatures.rs`, so the harness, which includes `signatures.rs`, covers them with no harness change (checked
by building the harness). `signatures` is `pub mod` so the stubs are not dead code before the Port tasks use them.

Units (each at most 1500 lines of legacy C++, to be ported; the adapter code is not ported):

| unit | C++ lines | total |
|---|---|---|
| `zstream_codecs` | `src/zstream.hpp` 130-745 (ZipInfo, viiz_stream, decoder structs Z/BZ/XZ/LZ4/ZSTD/BR/BZ3), `src/zstream.cpp` 373-1170 (ZipInfo members, `file_read`, `has_ext`) | 616 + 798 = 1414 |
| `zstream_core` | `src/zstream.cpp` 1171-2520 (predicates, `open_cpp`, `close_cpp`, `decompressing_cpp`, `zipinfo_cpp`, `zippeek_cpp`, `next_cpp` decoding loops) | 1350 |

The decoding loops of `next` are in core, not in codecs as the plan proposed, because that split keeps both units under 1500.
Rust modules: `zstream_codecs.rs`, `zstream_core.rs`, `ffi/zstream.rs`.

## Not in this seam yet

`flag.hpp`: a later Prepare seam task of the component.

Codec libraries: the Rust zstream calls them only with the cargo feature `codecs` (default for tests; msvc/rust.targets enables it on the umbrella for non-Base configurations, which link the libraries through project references). Without it no codec symbol is referenced. The umbrella never bundles codec libraries; tests and the replay harness link `..\codec-build\lib` (build error if missing).
