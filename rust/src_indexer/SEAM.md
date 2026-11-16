# Seam notes: `src_indexer`

Component: `src/ugrep-indexer.cpp` (the `ugrep-indexer` program, 2090 lines) and its new seam files. Prepared by the
"Prepare seam: src_indexer" task. Behavior is unchanged: `CPP_RUST_IMPL_SRC_INDEXER` defaults to `cpp` and the product
build runs the legacy bodies (the facade only forwards). The Rust bodies are stubs that abort (`std::process::abort`).

Interop: **cxx** (same row as `src_leaves` and `src_core`). Only `extern "Rust"` functions: the C++ facade calls Rust,
Rust never calls the component's legacy C++. cxx-build writes `lib.rs.h` under `<cargo target>/cxxbridge/src_indexer/src/`,
which `msvc/rust.targets` already adds to the include path of the Rust variant (`src/indexer_rust.hpp` includes it).
All bridge type names carry the `Indexer` prefix so the generated `cxxbridge1$rust_vec$ugrep_rust$...` symbols do not
collide with `src_core` in the one `rust_umbrella` staticlib.

## What crosses the boundary

| # | Crossing | Direction | Switchable now |
|---|----------|-----------|----------------|
| 1 | `index()`: index one file or one archive part (`bool index(Stream&, pathname, hashes, hashes_size, noise, compressed, archive, binary, size)`) | `indexer()` -> unit indexer_core | yes (facade) |
| 2 | `indexer(path)`: recursively index a tree | `main` -> unit indexer_walk | yes (facade) |
| 3 | `deleter(path)`: recursively delete index files | `main` -> unit indexer_walk | yes (facade) |
| 4 | `options(argc, argv)`: parse the command line / one config line | `main`, `load_config` -> unit indexer_cli | yes (facade) |
| 5 | `flag_*` globals (13 scalars + `flag_ignore_files`), `warnings`, `arg_path` | read/written by every function | stays C++ (snapshot in, result out) |
| 6 | `Zthread` (src/zthread.hpp), `zstreambuf` (src/zstream.hpp), `glob_match` (src/glob.hpp), RE-flex `reflex::Input` / `BufferedInput` | component -> other components / retained C++ | not crossing now (see Not switchable) |
| 7 | CRT: `fopen`, `stat`, `opendir`/`FindFirstFile`, `printf`, `exit` | component -> CRT | not seam; Port decision, see Console output |

### Not switchable in the headers

- `Stream` (src/ugrep-indexer.cpp) embeds a `reflex::Input` (FILE*, `std::istream*`, `reflex::BufferedInput`), a
  `Zthread` (thread, pipes, FILE*) or a `zstreambuf` depending on `HAVE_LIBZ` / `WITH_DECOMPRESSION_THREAD`: none of these
  can be handed to Rust by value or by cxx. Hence the stream state lives on the side that runs `index` (see Owner side).
- `Zthread` (src/zthread.hpp) is a retained header of another component (level 2, not ported by this seam): a Rust
  `indexer_core` that decompresses with `-z` needs it through a later seam (or the Rust `zstream` of `src_leaves`).
- `main`, `load_config` (reflex `BufferedInput` over a `FILE*`, `trim`, `getline`) and the Windows/POSIX shims
  (`fopenw_s`, `off_t`, `fseeko`, `ftruncate`, `PATHSEPSTR`, `OS_WIN*`) are retained C++, see Retained.

## Moves made (Prepare procedure, one kind per step)

1. Renamed the legacy bodies `index`, `indexer`, `deleter`, `options` to `index_cpp`, `indexer_cpp`, `deleter_cpp`,
   `options_cpp` (same code, same file). Their callers (`indexer_cpp` calls `index`, `load_config` and `main` call
   `options`, `indexer`, `deleter`) now reach the facade by the old names. The legacy bodies never call a facade function
   other than through those pre-existing call sites (`indexer_cpp` -> facade `index` -> `index_cpp`): no recursion.
2. New `src/ugrep-indexer_seam.h`: declares the C++-owned state (`extern` of `flag_*`, `warnings`, `arg_path`, which were
   already external-linkage globals, nothing was moved), the legacy bodies and the facade names; `Stream` stays opaque.
   One accessor `stream_rust_handle(Stream&)` and one member `Stream::rust_handle` (always 0 in the cpp build) were added.
3. New `src/ugrep-indexer_facade.cpp` (the facade, also holds the tap macros) and `src/indexer_rust.hpp` (C++ <-> cxx
   conversion, only with `CPP_RUST_IMPL_SRC_INDEXER`) and `src/indexer_tap.hpp` (tap macros and neutral encoders).
4. `rust/src_indexer` crate, workspace member, dependency and re-export of `rust_umbrella` (feature `tap`),
   `msvc/ugrep-indexer.vcxproj` compiles the facade and lists the new headers. `msvc/rust.props` / `rust.targets` needed no
   change: they already carry `CPP_RUST_IMPL_SRC_INDEXER` (default `cpp`), define it and link the umbrella when it is `rust`.
5. No `src/Makefile.am` change (autotools has no Rust glue for `src_leaves` or `src_core` either, and does not list
   `zopen_facade.cpp` / `zstream.cpp`); an autotools build of `ugrep-indexer` needs `ugrep-indexer_facade.cpp` and
   `ugrep-indexer_seam.h` added to `ugrep_indexer_SOURCES` first (**header-level change for approval**, see the result).

## Facade

| Seam id | C++ facade (`src/ugrep-indexer_facade.cpp`) | Legacy body | Rust bridge (`ugrep_rust::`) | Signature module |
|---|---|---|---|---|
| `src_indexer.index` | `index(Stream&, ...)` | `index_cpp` | `indexer_index` | `signatures::indexer_core::index` |
| `src_indexer.indexer` | `indexer(const char*)` | `indexer_cpp` | `indexer_run` | `signatures::indexer_walk::indexer` |
| `src_indexer.deleter` | `deleter(const char*)` | `deleter_cpp` | `indexer_delete` | `signatures::indexer_walk::deleter` |
| `src_indexer.options` | `options(int, const char**)` | `options_cpp` | `indexer_options` | `signatures::indexer_cli::options` |

`index()` stays ONE coarse call across the seam per file (or archive part): reading, windowed hashing and table compression
are not split. `indexer()` / `deleter()` are the coarse calls for the whole walk, so with the switch at `rust` the Rust
walk calls the Rust `index` directly and the facade `index` is only reached from the legacy walk (unit-level switching
and the unit replay use it).

Switch: `CPP_RUST_IMPL_SRC_INDEXER` (default `cpp`, `msvc/rust.props`); `rust` defines `CPP_RUST_IMPL_SRC_INDEXER=1`,
builds and links `rust_umbrella`. `/p:CPP_RUST_IMPL_SRC_INDEXER=rust` builds and links with the stubs (calls abort at run time).

## Units

| unit | C++ lines of `src/ugrep-indexer.cpp` | Rust module | content |
|---|---|---|---|
| `indexer_core` | 1-1050 (~960 code lines excluding the platform preamble) | `src/indexer_core.rs` | constants, `noise_percentage`, `Stream`, `is_binary`, `indexhash`, `index` |
| `indexer_walk` | 1050-1843 | `src/indexer_walk.rs` | `trim`, `getline`, `import_globs`, `include_dir`, `include_file`, `cat`, `deleter`, `indexer` |
| `indexer_cli` | 588-780 and 1844-2090 | `src/indexer_cli.rs` | `version`, `help`, `usage`, `warning`, `error`, `cannot_decompress`, `strtonum`, `strtopos`, `options` (and retained `load_config`, `main`) |

Line numbers are of the file before this task's edits (the rename edits shift later lines by about 20). Each unit is at
most 1500 lines. Cross-unit helpers: `warning`/`error`/`cannot_decompress` (cli unit) are called from core and walk; in
Rust they are a shared module, not a crossing. The Rust modules are reached through `#[path]` from `signatures.rs`, so a
replay harness that includes `signatures.rs` alone covers them.

## Crossing types

| Parameter | C++ | Rust | encoding / owner |
|---|---|---|---|
| flags snapshot | 13 scalars `flag_*` + `StrVec flag_ignore_files` | `IndexerFlags` (shared struct, by value) | `bool`, `int` (`accuracy` 0-9), `size_t` -> `usize` (`zmax`), file names as `Vec<IndexerString>`; `flag_delete` is the field `delete_indexes` (C++ keyword). Read-only: Rust never writes a flag |
| pathname, path, arg_path | `const char*`, NUL-terminated, borrowed for the call | `&[u8]` (no NUL) + `has_*: bool` where NULL is a legal value (`path`, `arg_path`) | bytes, no encoding assumed or validated (OS file names, UTF-8 not required); caller owns |
| `args` of `options` | `argv[0..argc-1]` | `Vec<IndexerString>` | copied bytes; `argv[0]` is ignored like the C++ |
| `hashes` out | caller's `uint8_t[65536]` | `Vec<u8>` of exactly `hashes_size` bytes | Rust allocates, the facade copies into the caller's array |
| `noise` | `float` | `f32` | `float` arithmetic of the C++ must be kept (`%`-printing is derived from it) |
| `size` | `uint64_t` | `u64` | - |
| `archive` | `bool&` in/out | `bool` in, `bool` out | same value flow as C++ |
| stream | `Stream&` | `handle: u64` in, `handle: u64` out | see Owner side |
| `warnings` | global `size_t` | `usize` in, `usize` out (`Exit::warnings`) | facade stores it back |
| process end | `exit(code)` inside the call | `Exit { exited, exit_code, warnings }` | the facade runs `exit(exit_code)`: stdio is flushed by the CRT exactly as before |
| `arg_path` out | pointer into `argv` | `arg_path_index: i32` (index into `args`, -1 = unchanged) | the pointer identity into `argv` is kept by the facade |

## Re-entrancy

None across the seam. The Rust functions never call back into C++ (no `extern "C++"` block). `options` is called repeatedly
from `load_config` (one config line per call, `flag_usage_warnings` set) and once from `main`: it holds no state between
calls, everything is in the snapshot and the result. The legacy bodies call only the facade `index`, `options`, no other
facade function (`indexer_cpp` -> `index`; `load_config` -> `options`).

## Thread affinity

Single-threaded from the C++ side: `main` calls `options`, `indexer`/`deleter` and, for `index`, always from the main thread.
With `WITH_DECOMPRESSION_THREAD` the `Zthread` decompression thread runs inside `Stream` (C++) and feeds a pipe the main
thread reads; no seam function is called from it. Rust bodies must be thread-neutral (no statics) so the Port keeps `multi`
safe; there is no `static mut` anywhere in the crate.

## Owner side of what crosses

| Object | Owner | Crossing |
|---|---|---|
| `flag_*` globals, `warnings`, `arg_path` | **C++** | by-value snapshot per crossing; changes return in the result (`options` returns all flags, `indexer`/`deleter`/`options` return `warnings`) |
| `Stream` (FILE*, input, Zthread, partname) | C++ legacy; **Rust** with the switch at `rust` | with the switch at `rust` the stream state of `index` (open file, decompression chain, archive cursor) is owned by Rust behind `handle` (0 = none), created by the first `index` call of a file, kept while `archive` is true, released by Rust at exactly the places where the C++ calls `stream.close()` (it returns handle 0). C++ keeps the number in `Stream::rust_handle` and never dereferences it |
| directory stack, `Entry`, `Ignore` stack, `file_entries`, hashes table | the function that creates them (inside one coarse call) | not crossing |
| index files (`._UG#_Store`) | the file system; opened/closed inside one call | not crossing |
| `FILE*` of the config file | C++ (`load_config`) | not crossing |

## Error kinds per crossing type

| Crossing | Error kind |
|---|---|
| `index` | `ok == false` is the C++ `false` return (cannot open the file, archive extraction ended); no exception |
| `indexer`, `deleter` | infallible at the boundary: file errors are `error()` diagnostics on stdout plus `warnings` (the C++ keeps going); `--check` ends with `exited` and exit code 0 or 1 (`EXIT_SUCCESS`/`EXIT_FAILURE`) |
| `options` | usage errors, `--help`, `--version` end the process (`exited`); with `flags.usage_warnings` a usage error is a warning count and parsing continues; invalid numbers (`--zmax=`) go through `usage` |
| any | no panics cross: `panic = "abort"` (D4). The C++ code throws nothing that is caught here (`std::bad_alloc` terminates as before) |

## Console output (decision for the Port, unchanged now)

The C++ writes progress and diagnostics with `printf` + `fflush(stdout)`, help/usage/`version` with `std::cout` / `std::cerr`.
A Rust body that prints must write through the same CRT streams (C `stdout`, `stderr`) or flush its own buffers before it
returns, so the order of lines relative to the retained C++ (`load_config` output, `exit`) is the one the recorded surface
traces show. `Exit.exited` means the facade runs `exit()` after Rust returned: Rust must not call `exit` itself.

## Unknown ownership (to resolve before porting those parts)

- `Zthread` / `zstreambuf` ownership inside the Rust `Stream`: the decompression thread and its pipes exist only as C++
  (src/zthread.hpp, component of level 2); whether Rust `index` calls the Rust `zstream` of `src_leaves` or a bridge to
  the retained `Zthread` is open until `indexer_core` is ported.
- `reflex::Input` over `FILE*` (utf-16/32 BOM conversion by `reflex::Input::get`): Rust must reproduce or reuse it;
  nobody frees across the boundary today.
- `ignore_stack` (global `std::stack<Ignore>`) is C++-owned global state used only by `cat` (walk unit): moves with the walk.

## Retained (stays C++, not switched by this seam)

`main` (calls `signal(SIGPIPE)`, `load_config`, then the facade), `load_config`, `fopenw_s` and the Windows/POSIX preamble,
`Stream` with `Zthread`/`zstreambuf` (until `indexer_core` is ported), the `flag_*` globals, `warnings`, `arg_path`,
`reflex::Input`/`BufferedInput`, `zthread.hpp`, `zstream.hpp`, `glob.hpp`.
When `ugrep-indexer.cpp` is dropped from the Rust variant (`linked_args: --forbid-sources "src/ugrep-indexer.cpp"` of the
plan), `main`, `load_config`, `fopenw_s` and the globals must first move to a retained file: that move belongs to the
Port of `indexer_cli`, with its own seam.

## Tap

`src/indexer_tap.hpp` (`-DCPP2RUST_TAP`, via `/p:CppRustTap=true`): `TAP_CALL("src_indexer.<fn>", args...)` at the four
facade functions. The flag snapshot is one `name=value;` string, argv a JSON array, `index` out-parameters one
`hashes_size=..;hashes_fnv=..;noise=..;...` state string (the 64-bit FNV-1a of the table stands for its 65536..128 bytes),
`void` results are `null` with the `warnings` counter as `state`. `indexer`, `options` end the process inside the call for
`--check`, `--help`, `--version`, usage errors: the C++ trace then has a call without a return event and the Rust-mode
facade exits at the same point (before `TAP_STATE`/`TAP_RET`). Smoke-tested: a tap build records
options -> indexer -> index -> ... in order. The Rust tap, the capture script and the replay harness crate
(`rust/src_indexer-harness`) belong to the Behavior task (judge files).

Absolute paths appear in `pathname`/`path` arguments: the Behavior task must give the cases a stable working directory
or propose a mask.

## Build

- `cpp` (default): `msvc/bin/Base-Release-x64/ugrep-indexer.exe`; the facade is compiled into it, no Rust.
- `rust`: `/p:CPP_RUST_IMPL_SRC_INDEXER=rust /p:CppRustOutSuffix=-rust` compiles and links the umbrella with the stubs
  (`msvc/bin/Base-Release-x64-rust/ugrep-indexer.exe`, map contains the `src_indexer` bridge objects).
- Tap: `/p:CppRustTap=true` (C++17) for either.
- `msvc/rust.props` currently defaults `CPP_RUST_IMPL_SRC_LEAVES` and `CPP_RUST_IMPL_SRC_CORE` to `rust` (set by earlier
  Switch-over tasks), so every build links the umbrella; the indexer code path only changes with `CPP_RUST_IMPL_SRC_INDEXER`.
- Building the workspace outside the product build needs the codec libraries of `src_leaves`: set
  `CPP2RUST_CODECS_FROM_LINKER=1` for `cargo check --workspace`, or fetch them (see `rust/src_leaves/SEAM.md`).
