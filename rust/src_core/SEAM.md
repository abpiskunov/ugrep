# Seam notes: `src_core`

Component: `src/ugrep.cpp`, `src/cnf.cpp`, `src/mmap.cpp`, `src/output.cpp`, `src/stats.cpp` and their headers
(`ugrep.hpp`, `cnf.hpp`, `stats.hpp`, `output.hpp`, `mmap.hpp`). Prepared by the "Prepare seam: src_core" task.
Behavior is unchanged: `CPP_RUST_IMPL_SRC_CORE` defaults to `cpp` and the product build is byte-for-byte the legacy path
(the facades call the `*_cpp` bodies). The Rust bodies are `todo!()` stubs.

## What crosses the boundary

| # | Crossing | Direction | Switchable now |
|---|----------|-----------|----------------|
| 1 | `CNF::compile/new_pattern/prune/split/adjoin/first/report/quote/anchor` | C++ callers (`ugrep.cpp`) -> component | yes (facade) |
| 2 | `Stats::report(FILE*)` | `ugrep.cpp` -> component | yes (facade) |
| 3 | `ugrep()`, `ugrep_extract()` | TUI / `main` -> component | no (tapped only, see Retained) |
| 3b | `terminal()`, `set_terminal_hyperlink()`, `set_color()`, `trim()`, `init()`, `ugrep_find_text_preview()` | `main` / TUI -> component | yes (facades, sub-switches; see S2b) |
| 4 | `tui_query()`, `tui_screen_getsize()` | component -> retained `src_tui` | via `src/tui_entry.hpp` (was `query.hpp`) |
| 5 | `flag_*` globals, `Static::*`, `Stats` counters, `color_*` | read by TUI C++, option parser, RE-flex callbacks | stays C++ |
| 6 | `warning()`, `error()`, `abort()` | everyone -> component | stays C++ (`exit`) |

Cannot be switched in the headers: `Output` (large inline hot class with mutex/condvar, embedded by value in `Grep`),
`Flag` and `flag.hpp` (inline conversion operators used by the whole tree), the `Stats` inline counters (atomics on
the search hot path), the Windows POSIX shims and `utf8len` helpers in `ugrep.hpp`, `Grep`/`GrepWorker`
(RE-flex matcher ownership), `MMap` (no-op on Windows, `WITH_NO_MMAP`), `Static::matchers_clone`.

## Moves made (Prepare procedure, one kind per commit)

1. Cut the "cycle": `ugrep.cpp` included `query.hpp` (which includes `ugrep.hpp`) only for `Query::query()`,
   `Screen::getsize()` and `DEFAULT_QUERY_DELAY`. New `src/tui_entry.hpp` declares `tui_query()`,
   `tui_screen_getsize()` and `DEFAULT_QUERY_DELAY`; `query.cpp`/`screen.cpp` define the two functions.
   `ugrep.cpp` no longer includes `query.hpp`; the retained TUI keeps using core's headers (one direction).
2. Moved the bodies of `CNF::compile`, `quote`, `anchor` out of `cnf.hpp` into `cnf.cpp`.
3. Renamed the legacy bodies to `*_cpp` (protected) and added the public facade functions. The legacy bodies never
   call a facade function: `OpTree` and `new_pattern_cpp` call `quote_cpp/anchor_cpp/compile_cpp` (no re-entry).
4. Bridge crate `rust/src_core` (`src/lib.rs` cxx bridge, `src/signatures.rs` types-and-signatures), C++ glue
   `src/core_rust.hpp`, tap `src/core_tap.hpp`, workspace + `rust_umbrella` (feature `tap`).

## Encoding

- Strings are bytes (`Vec<u8>` / `&[u8]`), no NUL, no encoding assumed or validated (patterns can be arbitrary
  bytes, file names are OS bytes). Rust must not convert to `String`.
- `CNF::Terms` crosses by value as `Vec<CnfTerm>` -> `Vec<CnfPattern{defined,text}>`; `defined == false` is the NULL
  smart pointer (distinct from the empty string). Order and list lengths are preserved.
- Shared struct field names: `boolean` for `flag_bool` (a Rust-bridge limitation on the name `bool`).
- `FILE*` does not cross: `report` functions return the text as bytes and the C++ facade `fwrite`s it.
- Enums `Mode`, `Action` cross as `u8` with the C++ values (`Mode` UNSP=0 ERE=1 BRE=2 FIXED=3 PERL=4; `Action` UNSP=0 SKIP=1 READ=2 RECURSE=3).
- Compile-time inputs (`HAVE_PCRE2`, `WITH_HIDDEN`, `NEWLINESTR`, `PATHSEPCHR`) are passed as snapshot fields.
- Floats: elapsed time is `f64` milliseconds (`reflex::timer_elapsed`); `Stats::report` prints `%.3g`/`%.4g`.

## Re-entrancy

The component facade functions are leaves: Rust never calls back into C++ (the bridge has no `extern "C++"` part).
The legacy C++ bodies never call a facade function. `ugrep()` and the TUI call each other (TUI runs `ugrep()` per
query, `ugrep()` calls `tui_query()`); that cycle is between retained C++ functions only and does not cross Rust.

## Thread affinity

Facade functions run on the calling thread. `CNF::*` and `Stats::report` are called from the main thread
(option parsing and end of run). The `Stats` counters are atomics updated from worker threads and stay in C++;
only `report`, on the main thread after the workers joined, reads them. Rust bodies must be thread-neutral (no
statics) so the Port can keep them that way.

## Owner side of what crosses

| Object | Owner | Crossing |
|--------|-------|----------|
| `CNF` object and its `terms` | C++ | copy in, copy out (by value) |
| pattern / filename strings | C++ | borrowed slice in, owned bytes out |
| `flag_*` globals | **C++** | read-only snapshot per crossing (see decision) |
| `Stats` counters | C++ | read by the facade, passed as numbers |
| `Output`, `MMap`, `Grep`, matchers, `Static::output/errout/source` `FILE*` | C++ | not crossing |

## Decision: ownership of the `flag_*` globals

**C++ owns every `flag_*` global.** Rust never holds, reads through a pointer, or writes one.

- Where defined/used: defined in `ugrep.cpp`, plain unsynchronised `extern` globals, written by the option parser,
  written also by the TUI (`query.cpp`, e.g. `flags_[8].flag = flag_with_filename`), read inline on the hot path by
  `Grep`, `Output` and the RE-flex `GrepHandler` callbacks, and by the TUI.
- Why not a Rust-owned store: the readers are retained C++ (TUI, option parser, RE-flex callbacks, `Output`); a
  Rust owner would need an accessor on every hot-path read and the TUI writes must be mirrored. There is no single
  owner side that works before the option parser and the TUI are ported.
- How Rust sees them: a typed read-only snapshot (`CnfFlags`, `StatsInput`) built by the C++ facade at each crossing.
  The flags are stable during a call (set during option parsing; the TUI changes them between searches, never
  during a facade call).
- A flag a function writes comes back in the result: `CNF::anchor` writes `flag_empty = true`; Rust returns
  `Anchored{enable_empty}` and the facade stores it back. That is the only flag written by a seam function.
- Revisit when the option parser (`ugrep.cpp` main) is ported: then the flags can become a Rust-owned struct with a
  C++ read accessor.

## Error kinds per crossing type

| Crossing | Error kind |
|----------|------------|
| `cnf_*`, `stats_report` | infallible in C++ (no throw, no return code); Rust bodies must not fail and must not panic (`panic = "abort"`) |
| `ugrep_find_text_preview` (facade, `..._TERMINAL`) | `std::runtime_error`/`bad_alloc` from the `Grep` body propagate through the C++ default; in the Rust path `core_find_text_preview` returns `Result<TextPreview>`: the Port maps them to `CoreError::Exception`, which cxx turns into a `rust::Error` exception that the caller's `catch (...)` handles |
| `ugrep_extract` (entry, not switched) | Future work: no facade yet |
| `abort(msg)`, `error(msg, arg)` | process exit (`exit(EXIT_ERROR)`), `CoreError::Abort/Errno`, never returned |
| `report` writes | a short write to `FILE*` is ignored exactly as in the legacy `fprintf` |

## Retained (stays C++, not switched by this seam)

`ugrep()` and `ugrep_extract` (tapped as unit entry functions), `Output`, `MMap`,
`Static::matchers_clone`, `Grep`/`GrepWorker`, `Flag`/`flag.hpp`, the `Stats` counters, option parsing. `terminal`, `set_terminal_hyperlink`, `set_color`, `trim`, `init` and `ugrep_find_text_preview` now have facades (see S2b).

## Unknown ownership (to resolve before porting those parts)

`Static::output/errout/source` (`FILE*`), `grep_handle`, `Static::matchers` (RE-flex), the `FILE*` argument of
`ugrep_extract`. Nobody frees them across a boundary today; ownership is only decided when `ugrep()` is ported.

## Tap

Historical (deleted at Cleanup): `src/core_tap.hpp` (`-DCPP2RUST_TAP`, via `/p:CppRustTap=true`): `TAP_CALL("src_core.<fn>", args...)` at every
facade function and unit entry function; neutral encoders for `CNF::Terms` (nested array, `null` for the NULL
pattern), the flag snapshot (`name=value;`) and `FILE*` output (captured text; tap build only). Without the macro the
tap macros expand to nothing. The Rust tap, capture script and replay harness crate (`rust/src_core-harness`) belong
to the Behavior task.

## Build

`msvc/rust.props` has the switch `CPP_RUST_IMPL_SRC_CORE` (default `cpp`), `msvc/rust.targets` defines
`CPP_RUST_IMPL_SRC_CORE=1` and links `rust_umbrella` when it is `rust`; no change was needed there. Autotools: the new
headers are listed in `src/Makefile.am`/`.in`; no autotools Rust glue exists for src_leaves either, so none was added.
`/p:CPP_RUST_IMPL_SRC_CORE=rust` builds and links with the `todo!()` stubs (calls panic -> abort at run time).

## S2b seam: init, terminal, set_terminal_hyperlink, set_color, trim, find_text_preview (Prepare seam, supersedes the retained notes for these functions)

Context. S1 made the `flag_*`, `color_*`, `match_*` scalars Rust-owned (`ugrep_settings`, `settings.rs`), so the functions below work on that struct directly: no snapshot crossing and no flag write-back. Since Cleanup, `CPP_RUST_IMPL_SRC_CORE` is inert (CNF/Stats have no C++ body), so it cannot select these functions; two sub-switches do (same pattern as D-zstream-switch). Default `cpp`; behavior is unchanged.

| Function | Facade (src/ugrep.cpp, end of file) | C++ body kept as | Rust entry | Switch |
|---|---|---|---|---|
| `terminal()` | `terminal()` | `terminal_cpp()` | `ugrep_rust::core_terminal()` | `CPP_RUST_IMPL_SRC_CORE_TERMINAL` |
| `set_terminal_hyperlink()` | same name | `set_terminal_hyperlink_cpp()` | `core_set_terminal_hyperlink()` | `..._TERMINAL` |
| `set_color(colors, parameter, color[COLORLEN])` | same name | `set_color_cpp()` | `core_set_color(&[u8], &[u8], &mut [u8])` (the buffer is `COLORLEN` bytes) | `..._TERMINAL` |
| `trim(std::string&)` | same name | `trim_cpp()` | `core_trim(&[u8]) -> TrimRange{begin,end}`; the facade erases outside the range | `..._TERMINAL` |
| `init(argc, argv)` | same name | (deleted) | C ABI `ugrep_core_init(const InitEnv*, int, const char**)` (`ffi/init.rs`; a callback table like `TerminalEnv`, because argv pointers are stored in the settings and cannot be copied across cxx) | none (sub-switch removed) |

**Init Port (done).** `init_cpp()`, its prototype and the `CPP_RUST_IMPL_SRC_CORE_INIT` sub-switch (`msvc/rust.props`, `msvc/rust.targets`, the facade) are deleted; `init()` in src/ugrep.cpp is a thin shim that fills `InitEnv` and calls `ugrep_core_init`. The logic is `init.rs` on the S1 settings through an `Env` trait; `ffi/init.rs` holds the callback table and the only `unsafe`. The signature changed on purpose (the entry takes the env table first) and the `signatures::ui` stub module was removed. Settings are not borrowed across a callback because `load_config` and `options` re-enter Rust on the same settings. The `argv[1]` quirk is reproduced. Autotools never referenced the sub-switch. See `migration/units/init.yaml`.

Types and signatures: `rust/src_core/src/signatures.rs` module `ui` held the stubs (now removed; `init` and `terminal` use C-ABI callback tables). Build: the TERMINAL and INIT sub-switches are gone (`msvc/rust.targets`); autotools never had them. The tap was deleted at Cleanup, so no tap macros were added; the replay is the surface corpus.

Not switched by anything else: none of the six remain C++-only. `find_text_preview`: the facade `ugrep_find_text_preview` (src/ugrep.cpp, end of file) keeps the old body as `ugrep_find_text_preview_cpp` (which alone builds the `Grep`); its signature uses only boundary types, so the Rust entry is `core_find_text_preview(filename: &[u8], partname: &[u8], from_lineno, max) -> Result<TextPreview{lineno, num, text: Vec<Bytes>}>` and the facade copies the result into the out-parameters (assign into `text[i]` when it exists, else `emplace_back`; the tail is left untouched). Switch `..._TERMINAL`. The facade keeps the `Static::matcher == NULL` early return before calling Rust (outputs untouched); a Port must reproduce the exceptions listed under Error kinds via `Err`; the Rust body will need the `Grep` search as a callback until `Grep` is ported.

**Terminal Port (done).** The `CPP_RUST_IMPL_SRC_CORE_TERMINAL` sub-switch and the `*_cpp` bodies of `terminal`, `set_terminal_hyperlink`, `set_color` and `trim` are deleted; the facades call Rust unconditionally (no sub-switch remains; the `INIT` one was deleted by the Init Port). `terminal()` and `set_terminal_hyperlink()` use a C-ABI callback table (`TerminalEnv`, `ffi/terminal.rs`, entries `ugrep_core_terminal` / `ugrep_core_set_terminal_hyperlink`) instead of the cxx bridge: a deliberate signature change, so their `signatures::ui` stubs and bridge fns were removed. `set_color`/`trim` stay cxx (`core_set_color`, `core_trim`) and were removed from `signatures::ui`. `find_text_preview` is NOT ported: it is only a reflex matcher clone plus `Grep` (unported C++), so it stays C++ as `ugrep_find_text_preview`; its Rust stub, bridge fn and `TextPreview` struct were removed.

### Callbacks the Rust bodies need (extern C++, not declared yet)

Declared in the Port tasks as `extern "C++"` in the bridge (or C ABI), implemented in a C++ shim; nothing is declared now so the stubs link without them.

Terminal group:
- `Static::output_stat`, `Static::output_stat_regular`, `Static::output_stat_result` (read/write; `fstat` of stdout, `stat` of `/dev/null`) and `Static::NEVER/ALWAYS/AUTO` constants (values of the color/pager tri-state).
- `flag_hyperlink_host` and `flag_hyperlink_path` (`std::string`, opaque C++): accessors `hyperlink_host_set(bytes)`, `hyperlink_path_set(bytes)`, and `hyperlink_*_get()`. The OS calls (`isatty`, `getenv`, `getcwd`, `gethostname`, `WSAStartup`) are made from Rust.
- `warning(msg, arg)`, `error(msg, arg)`, `abort(msg)`: process exit paths in C++ (`exit(EXIT_ERROR)`), plus `color_off/color_error/color_high` (already in settings).

`init`:
- `CNF`/`Static::bcnf`, `Static::arg_pattern`, `Static::arg_files`, `Static::filter_magic_pattern`, `Static::magic_matcher` (RE-flex objects, never cross).
- `load_config`/`save_config`, `import_globs`, `fopen_smart`, `trim_pathname_arg` (modifies argv in place), `stat_fs` and `fsid_to_uint64`.
- `usage(...)` (16 call sites), `error`, `abort`, `warning`, `exit`.
- The opaque containers: `flag_exclude*`, `flag_include*`, `flag_file`, `flag_glob`, `flag_ignore_files`, `flag_regexp`, `flag_filter`, `flag_config_files` and the other `std::vector<std::string>` options: C++ shims `vec_push(name, bytes)`, `vec_clear(name)`, `str_set(name, bytes)`.
- If the S2 siblings (options, config, help/errors) have merged before the init Port, call their Rust entries instead.

Known C++ quirk for Triage (preserve until classified): init tests `argv[1]`, not `argv[i]`, for `-e/-g/-N/--regexp` near the start of the option loop.

## Units judged by the surface corpus, not the seam replay

- `src_core.ugrep` is tapped for call order only and is not replayed. The driver, options, init and terminal parts of ugrep.cpp therefore have no seam cases: their Port units are judged by the surface corpus (migration/corpus/src_core, 1097 cases) and the switch gate, so expect the unit gate's seam-replay check to have nothing to compare for them.
- `cnf_anchor` writes `flag_empty`, which the tap does not record; the seam compares the anchored pattern only.

## output.cpp retained (D-src_core-output-retained)

`src/output.cpp` was retained here at S2b; the output-buffer Port supersedes this (see "Seam: output-buffer" below): the file is deleted.


## ugrep.cpp globals/pager range retained (D-src_core-globals-retained)

The ugrep.cpp ~228-757 and ~4456-4670 range (flag globals, Static members, tables, pager, signal handlers, CPU affinity, fopen_smart) stays C++: the flag/Static ownership decision keeps the globals in C++, and no seam signature, tap or recorded case exists for the pager, signal, affinity and fopen_smart functions. Porting it later needs its own Prepare seam + Behavior follow-up.

## ugrep.cpp Zthread seam (Prepare seam: Zthread)

D-src_core-zthread-retained was superseded by D-S1-supersede. Prepared here; the body is still C++ (`ZthreadCpp`), the Rust
side is a stub. Behavior is unchanged: `CPP_RUST_IMPL_ZTHREAD` defaults to `cpp`; the facade then calls `ZthreadCpp`
(the old `struct Zthread`, body unchanged apart from the rename of the type and its `new`/`&Zthread::decompress`
uses). The text of `migration/decisions.md` / `migration/units/zthread.yaml` for this seam is at the end of this section
(the Prepare task does not write under `migration/`; a task allowed to do so appends it).

### What crosses the boundary

| # | Crossing | Direction | Switchable now |
|---|----------|-----------|----------------|
| 1 | `Zthread::start(ztstage, pathname, file_in, find)` -> `FILE*` | `Grep::open_file` -> component | yes (facade) |
| 2 | `Zthread::open_next(pathname)` -> `FILE*` | `Grep::close_file` -> component | yes (facade) |
| 3 | `Zthread::cancel()` | `Grep::cancel`, `Grep::close_file`, `Grep::search` -> component | yes (facade) |
| 4 | `Zthread::close()` (join, the destructor of `Grep::zthread`) | `~Grep` -> component | yes (facade) |
| 5 | `Grep::partname` (`std::string&`) written by the decompression thread | component -> C++ | callback `set_partname` |
| 6 | `warning(message, arg)` | component -> C++ (`ugrep_core_warning` through the existing diagnostics bridge) | callback `warning` |
| 7 | `select_matching` flags part: hidden, `--exclude`/`--include`, `-M` magic (`Static::magic_pattern`, RE-flex) | component -> retained C++ | callback `select_by_flags` |
| 8 | `zstreambuf` (open, `zipinfo`, `decompress`, `get_buffer`) | component -> `src_leaves` | Rust-to-Rust in the Port (see decision) |

Callers are `Grep` only (`open_file` ~3333, `close_file` ~3719/3722, `cancel` ~3104, `search` ~8308, member ~3950, all
in `src/ugrep.cpp`); no header is involved: `struct Zthread` was never visible outside `ugrep.cpp`.

Cannot be switched: `ZthreadCpp` internals (`ztchain`, mutex and condition variables, `filter_tar`, `filter_cpio`
are one thread state machine; there is no seam inside it), `Grep::partname` (read by the Grep thread, no accessor),
`select_matching` while the options `flag_all_include/exclude`, `flag_file_magic` stay C++ containers (S2).

### Moves made (one kind per step)

1. Rename `struct Zthread` to `struct ZthreadCpp` (type, constructor, destructor, `new ZthreadCpp`, thread entry,
   `ztchain` member); no other change.
2. Add the facade `struct Zthread` after it (same name, constructor signature and public functions, so `Grep` and the
   five call sites are untouched). `close()` is new: `~Zthread()` calls it; the legacy path runs `join()` there and
   then the member destructor runs `join()` again, a no-op (`thread` not joinable, `zstream` NULL).
3. C ABI `src/zthread_seam.h` and Rust `src/ffi/zthread.rs`; types-and-signatures `src/zthread.rs`
   (declared from `signatures.rs`, so the replay harness sees it); `msvc/rust.props` and `rust.targets`
   (`CPP_RUST_IMPL_ZTHREAD`, `/p:CppRustTap=true`).
4. Tap: `src/zthread_tap.hpp` (`TAP_CALL`/`TAP_RET` on the four facade functions, `src_core.zthread_*`, the `FILE*`
   encoder).

### Where the Rust Zthread lives

**`rust/src_core`**, which now depends on `rust/src_leaves` (`default-features = false`; the umbrella unifies the
`codecs` feature). `src_leaves` depends on no workspace crate, so there is no cycle (`cargo tree -p src_core` shows
`src_leaves`). Reasons: Zthread's callers, `Grep` and the settings it writes (`flag_no_header`), are `src_core`; the zstream
core it drives (`ffi/zstream.rs`, `zstream_core.rs`) is in `src_leaves` and is reached by a plain crate call, with
no second bridge. Putting it in `src_leaves` would invert the port order (level 2 into level 0/1) and give `src_leaves`
the pipe and `warning` callbacks. The Port may call `src_leaves` through `signatures::zstream_core`, not through the C ABI.

### Encoding

Strings are bytes (`&[u8]`, no NUL, no encoding assumed: file names are OS bytes, `partname` is built from names
inside archives). On the C ABI they are NUL-terminated (`const char*`), `find` and `arg` may be NULL. Sizes are `size_t`.
The pipe read end is a C `int` file descriptor (binary mode, `_O_BINARY` on Windows); the facade turns it into `FILE*`
with `fdopen(fd, "rb")`. `FILE*` of the source file crosses as an opaque pointer, never dereferenced by the stub.

### Re-entrancy

Rust calls back into C++ only through the callbacks of `ugrep_core_zthread_env`. `set_partname` and `warning` take no
lock of this side; `select_by_flags` calls retained C++ that takes none either (it builds a local `reflex::Matcher`, the
legacy code did the same on the decompression thread). No callback calls a facade function. The legacy
`ZthreadCpp::start`/`open_next` recurse through the chain (`ztchain->start`, `ztchain->open_next` from the next-stage thread):
that stays inside one side, the facade is not re-entered.

### Thread affinity

`start`, `open_next`, `cancel` and `close` are called by the thread that owns the `Grep` (a `GrepWorker` thread, or the main
thread for stdin). The decompression thread (one per stage) is created by `start` and joined by `close`; it calls
`set_partname`, `warning` (`errno` is the thread's own: Rust must set `errno` before the callback as the C++ pipe code
left it) and `select_by_flags`. `cancel` may be called while the thread runs (`stop` is an atomic). The handle is never
used by two caller threads. The stub starts no thread.

### Owner side

| Object | Owner | Crossing |
|--------|-------|----------|
| the handle (thread state, chain, `zstreambuf` or Rust zstream) | **Rust** from `new` to `close` | opaque `void*`; the facade closes it in `~Zthread` |
| pipe read end | C++ after the call returns | descriptor, then `FILE*`, `fclose`d by `Grep::close_file`; if `fdopen` fails the facade `close`s it and Rust sees a broken pipe |
| pipe write end | Rust | never crosses |
| source `FILE*` (`file_in`) | C++ (opened by `open_file`) | borrowed until the next `start` or `close`; the legacy `zstreambuf` closes it at end of input |
| `Grep::partname` | C++ | written through the callback; read by the Grep thread after data arrives on the pipe (ordering is the pipe) |
| flags `flag_no_filename`, `flag_no_header` | Rust (`settings`) | Rust writes `flag_no_header = false` for archives, as the legacy thread did |
| `findpart` | Rust (copy of `find`) | the C++ keeps `find` alive only for the call |

### Error kinds per crossing

| Crossing | Error kind |
|----------|------------|
| `start`, `open_next` | status `ZTHREAD_OK` (fd set), `ZTHREAD_NONE` (the legacy NULL; the warning "cannot create pipe/thread to decompress" was already given through `warning`, or there is no next file), `ZTHREAD_NOT_PORTED` (stub; the facade warns "decompression thread is not ported" and returns NULL) |
| `cancel`, `close` | infallible, `close` accepts NULL and repeats |
| thread creation failure | warning `cannot create thread to decompress` and `ZTHREAD_NONE`, as in C++ |
| panic | never crosses: the entries catch it and return `ZTHREAD_NONE` (release builds are `panic = "abort"` anyway) |
| a short `write` to a broken pipe | the legacy drain logic, no error value |

### Unknown ownership

- Does anything besides `fclose(input.file())` close the read end when `open_next` returns NULL after the pipe was
  opened? (In the legacy `open_next` failure path both ends are closed inside; the Port must keep that.)
- `file_in` when `start` returns NULL: `open_file` `fclose`s it; when the stream already took it the legacy
  `zstreambuf` also may. Only the Port can tell which path double-closes; the stub does not touch it.

### Retained

`ZthreadCpp` stays the product code until the Port; `select_matching` is called through a never-started `ZthreadCpp`
member of the rust facade (`selector`, `findpart` NULL) until the Port splits the findpart logic from the flags part.
`src/zthread.hpp` (an older, ugrep-indexer copy of the same struct) is **kept**: no source includes it any more
(`src/ugrep-indexer.cpp` is the Rust entry stub), so it is not compiled, but `msvc/ugrep-indexer.vcxproj:101` lists it as
a `ClInclude` and `src/Makefile.am:67,95` distribute it. Removing it changes the project and the distribution lists,
which is not a seam-preparation move; it goes with the Cleanup that deletes `ZthreadCpp`.

### Tap

`src/zthread_tap.hpp`, built with `/p:CppRustTap=true` (`CPP2RUST_TAP`, C++17): `TAP_CALL("src_core.zthread_start", ztstage,
pathname, file_in, find)` + `TAP_RET(pipe)`, the same on `zthread_open_next` (pathname), `zthread_cancel`,
`zthread_close`; a `FILE*` is recorded as a per-case object id (`"#n"`) or `null`, never an address. The Rust tap, the
capture script and the replay harness belong to the Behavior task. Without the macro all of this compiles to nothing.

### Build

`/p:CPP_RUST_IMPL_ZTHREAD=cpp|rust` (default `cpp`) in `msvc/rust.props`; `rust` defines `CPP_RUST_IMPL_ZTHREAD_RUST` for
every `ClCompile`, so the facade calls `ugrep_core_zthread_*` of the always-linked umbrella. The facade only exists when
`HAVE_LIBZ` is defined (as before); the Base configurations may not define it.

### Text for the migration records (to be appended by a task that may write `migration/`)

`decisions.md`: `D-zthread-seam` | scope | the Zthread port is back in scope (D-S1-supersede); Prepare seam done; Rust Zthread lives
in `rust/src_core` with a dependency on `rust/src_leaves` (acyclic); `src/zthread.hpp` kept, listed only (reason above) |
`rust/src_core/SEAM.md`. `units/zthread.yaml`: `unit: zthread`, `component: src_core`, `functions: [src_core.zthread_start,
src_core.zthread_open_next, src_core.zthread_cancel, src_core.zthread_close]`, `cpp_files: [src/ugrep.cpp, src/zthread.hpp]`,
`rust_files: rust/src_core/src/zthread.rs rust/src_core/src/ffi/zthread.rs`, `depends_on: [zstream_core]`, `status: planned`.

## ugrep.cpp Grep part 1 retained (D-src_core-grep1-retained)

The ugrep.cpp ~1913-3000 range (struct Grep members: file/dir search) stays C++. Grep owns the RE-flex matcher and embeds Output by value; no seam signature, tap or recorded case exists. Porting it later needs one coarse seam together with Output and the driver (its own Prepare seam + Behavior follow-up).

## ugrep.cpp Grep part 2 retained (D-src_core-grep2-retained)

The ugrep.cpp ~3000-4130 range (Grep::cnf_matching, open_file, filter, close_file, init_read, output_restline etc.) stays C++. Grep owns the RE-flex matcher and embeds Output by value; no seam signature, facade, tap, recorded trace or unit record exists. Porting it later needs one coarse seam together with Output and the driver (its own Prepare seam + Behavior follow-up).

## ugrep.cpp GrepMaster/GrepWorker retained (D-src_core-grepmaster-retained)

The ugrep.cpp ~4133-4448 range (GrepMaster, GrepWorker job queue: mutex/condvar, thread handoff) stays C++. GrepWorker owns a Grep (RE-flex matcher + Output by value); no seam signature, facade, tap, recorded trace or unit record exists. Porting it later needs one coarse seam together with Grep, Output and the driver (its own Prepare seam + Behavior follow-up).

## ugrep.cpp config load/save retained (D-src_core-config-retained)

The ugrep.cpp ~4806-5282 range (load_config, save_config: config file parsing and writing, which set the flag globals) stays C++. No seam signature, facade, tap, recorded trace or unit record exists, and the flag globals are retained C++ (D-src_core-globals-retained). Porting it later needs its own Prepare seam + Behavior follow-up.

## ugrep.cpp help/version/error range retained (D-src_core-help-retained)

The ugrep.cpp 14047-15365 range (help, version, is_directory, cannot_decompress, warning, error, abort) stays C++. No facade, bridge entry, tap, recorded trace or unit record exists; warning/error/abort exit and read C++-owned flag/color globals. Porting it later needs its own Prepare seam + Behavior follow-up.

## ugrep.cpp option helpers retained (D-src_core-helpers-retained)

The ugrep.cpp ~13500-14046 range (find_text_preview/extract, trim helpers, set_color, option-argument parsers, usage, import_globs) stays C++: it serves the option parsing and driver that stay C++, so a port would add an FFI crossing per call and need its own seam for little gain.
## ugrep.cpp Grep::ugrep/select/recurse and Grep::search retained (D-src_core-select-retained, D-src_core-search-retained)

The ugrep.cpp ~9166-10482 range (Grep::ugrep, select, recurse, Static helpers) and the ~10483-13499 range (Grep::compute_cost, Grep::search) stay C++. They drive the RE-flex matcher and Output on every match; porting them needs one coarse seam together with Grep and Output.

## ugrep.cpp options and main/wmain retained (D-src_core-options-main-retained)

The ugrep.cpp ~4670-4765 (main/wmain) and ~5282-6354 (options, option_regexp/and/andnot/not/all_files) ranges stay C++. No seam signature, facade, tap, recorded trace or unit record exists; options only writes C++-owned flag_* globals (D-src_core-globals-retained), and main installs signal handlers and exit paths. Porting it later needs its own Prepare seam (flag snapshot in, resolved options out) + Behavior follow-up.


## Seam: output (src/output.cpp, src/output.hpp) - Prepare seam

Switch: removed by the output-core and output-format Ports. Every ported member (colour, `Dump::line`, both `header`, `binary_file_matches`, `flush_truncated_lines`, `Sync`, `format` x2, `format_invert`, `quote`, `cpp`, `csv`, `hex`, `json`, `xml`) calls Rust unconditionally; all `*_cpp` bodies, the inline matcher overloads (`mat`, `quote(matcher)`, ...), `capture` and the C++ `Sync` are deleted; `CPP_RUST_IMPL_OUTPUT` no longer exists. `Sync::Lock` and `lock->handle` are gone: each `OutputBuf` holds its own reference to the reference-counted Rust sync object, so an `~Output` flush after the `Sync` is destroyed never reads a destroyed object. Only `src/ugrep.cpp` includes `output.hpp`; **no call site changed**, the public
`Output` API is the facade.

Facade (members of `Output`, inline in `src/output_rust.hpp`; `src/output.cpp` is deleted): `color`, `header` x2, `binary_file_matches`, `format` x2,
`format_invert`, `quote`, `cpp`, `csv`, `hex`, `json`, `xml` (`Dump::line` is Rust too: `OutputBuf::dump_line`). Rust bridge: `output_*` in `rust/src_core/src/lib.rs`, bodies in
`rust/src_core/src/output.rs` and `output_format.rs`. C++ conversions: `src/output_rust.hpp`.

### Decision: matcher handoff = plain-data snapshot (not a callback table)

Rust cannot own `reflex::AbstractMatcher`. C++ builds `OutputMatch` per call. Always: first, last, size, wsize, lineno, accept and
`begin`/`end` as offsets into `window`. `text()` is never called (it writes a NUL at `txt_[len_]`, inside the window); `begin()`
is used for `text_off` and the capture offsets. The rest is computed only under the flags or format characters the C++ checks
(`output_rust::scan_format` mirrors the format parse of `output_format.rs`): `eol()` (may refill and grow the buffer, so callers' pointers such as
`to`/`begin` in ugrep.cpp could dangle) only for `%O %Q` and `%C %V %J %X %Y` without `-o`, or `%{+N}` forms; `bol()` for those and `%{-N}`;
`columno` for `%K %k` (and `-k` headers: `flag_column_number`); `lines`/`lineno_end` for `%L %l` and the same-line check; fuzzy
`edits`/`distance` for `%Z`; `groups` (a `Vec` per call) only for the fields that read captures (`%b %d %e %o %# %q %c %v %j %x %y %G %g %0-%9`); `%[..]` alone, as in the `--json/--csv/--xml` presets, does not fill them. `Opts` is rebuilt on every format call because flags change between calls (`-Q` `set_flags`, `--format-begin`). The window is `bol()..max(eol(), end())` when
`has_bol`/`has_eol`, else `begin()..end()`; it is widened to hold a group capture that starts before `begin()` (PCRE2 lookbehind), and capture offsets are relative to the window start. Reason: the accessors are cheap and stable during a call; a callback table
would cost one crossing per accessor and make Rust call back into C++ (re-entrancy). `eol()` is called before `bol()` as the
C++ requires, and the pointers are read after it. The matcher and its buffer are not changed by the snapshot.

### Decision: bytes out, C++ keeps the buffer (this stage)

Every seam function is pure except the `--tree` header: inputs are slices plus small structs, output is `Vec<u8>` that the facade appends with `Output::str`.
`header(pathname, partname)` with `flag_tree` calls Rust too (`output_header_tree`): the facade takes the Sync lock with `acquire()` first (ordered mode waits for
the worker's turn), passes the process-wide `Output::Tree::path`/`depth` in and writes the returned values back under that lock.
`Output` (Buffer, Sync, Dump state, `FILE*`, `mode_`, `lineno_`) stays in C++: it is embedded by value in `Grep`, its inline
primitives (`chr`, `str`, `num`, `nl`, `hex`, `uri`, `try_flush`) are hot and stay inline, so **no per-token bridge crossing**:
one crossing per header, per format, per emitted data item (`quote`...`xml`) and per hex-dump line. The Rust side reads the options
it already owns (`settings`, `color`), so no flag snapshot crosses. State a function changes comes back in the result:
`heading`, `matching`, `lineno_` (`lineno_set`), BINARY bit (`set_binary`), `Dump::done()` request (`complete_dump`),
`pstar`/`prevb` (`repeated`). `pathname == Static::LABEL_STANDARD_INPUT` is a pointer test: passed as `OutputCtx.stdin_label`.
`Dump::hex`, `Dump::done` and `flush_truncated_lines` are state machines on the C++ buffer and are not seam functions yet.

Known differences the Port must settle (listed, not decided here):
1. Inside one facade call the C++ body writes through `str()` and may flush mid-call when the 32768-byte buffer fills or on
   `nl()` with --line-buffered; the Rust result is appended after the call, so the flush points of one call move to the end of the
   call (total bytes and order unchanged; interleaving between threads in unordered mode can differ).
2. `Output::str` from `append` runs `check_flush` only through `next()`, not per `nl()` inside the bytes: with --line-buffered
   the facade must call `check_flush()` per line, or Port Unit A moves `Buffer` and `Sync` to Rust (Rust-owned buffer pointer,
   opaque `OutputState`, C++ inline appenders writing into it) and the facade shrinks to that. Decide there, with the benchmark.
3. A NULL `separator`/`arg` is `has_separator`/`has_arg` false; a NULL `pathname` is the empty slice.

### Decision: threading (cpp-rust-concurrency)

This stage adds no Rust thread state: the seam functions are stateless and thread-neutral, so they are safe from any `GrepWorker`
(the `--tree` header is a seam function: `Tree::path`/`depth` and the Sync lock are handled by the facade).
`Sync` (UNORDERED/ORDERED, `acquire/try_acquire/release/finish/cancel`, STOP) is Rust (`output_sync.rs`, `Mutex + Condvar`); the buffers stay C++. The shape
planned for Unit A: `Sync` as `Mutex + Condvar` with a slot counter, `Output` `Send` per worker (one per thread, never shared),
`std::unique_lock` ownership mapped to a guard stored in the worker's `Output`; `cancel()` sets an atomic `eof` and wakes waiters.

### Tap and gates

No tap macro was added: `src/core_tap.hpp` was deleted at Cleanup and the plan has no output component or seam gate (the Output
bodies are part of `src_core`, whose gates are cleaned). The generic `rust/src_core/cpp2rust_tap.hpp` template exists for the
Behavior task. Evidence at Prepare: both modes build with MSVC (default and `/p:CPP_RUST_IMPL_OUTPUT=rust /p:CppRustOutSuffix=-rust`),
`gate-check --gate baseline --target src_core` PASS (2981 surface cases, 0 divergent), `policy-scan` PASS.

## Seam: output-buffer (supersedes the "C++ keeps the buffer" decision and known differences 1 and 2 above)

`src/output.cpp` is deleted and no `output.obj` of ugrep is linked. The facade members (`color`, `header` x2, `binary_file_matches`,
`format` x2, `format_invert`, `quote`...`xml`) are `inline` at the end of `src/output_rust.hpp`, included by `ugrep.cpp` after `output.hpp`.

- **Buffer in Rust.** `rust/src_core/src/ffi/output_buf_core.rs` (`OutputBuf`) owns the 32768-byte blocks, the ordered/hold blocks, the
  Sync slot, `--width` truncation state, the hex-dump state (`Dump::hex` and friends) and the global `--tree` state (path, depth, glyphs).
  The module lives under `ffi/` because the plan only allows `unsafe` there. Flush is CRT `fwrite`/`fflush` on the same `FILE*` (static CRT on both
  sides), so Windows text-mode CRLF is unchanged.
- **Batching API (for the S4 performance task).** The C++ `Output` embeds `Regs {char *cur; char *end; size_t mode;}` (`repr(C)`); its address
  and the `FILE*` and `eof` go to `ugrep_output_new`. `chr`, `str`, `nl`... write straight through `regs_.cur` to `regs_.end` inline; Rust is called
  only when the block is full (`ugrep_output_next`), after an `nl` in `--line-buffered` mode (`ugrep_output_try_flush`), for `flush`/`begin`/`acquire`/
  `release`/`end`/`cancel`, per hex-dump row batch (`ugrep_output_dump_*`) and per tree header. `Output` must not move after construction (Rust holds
  `&regs_` and `&eof`); `GrepWorker` already constructs it in place.
- **Tree.** `Output::Tree::reset()/depth()/end()/glyphs()` are Rust calls; `output_header_tree` takes no path/depth, it uses the Rust global.
- **Flush points** of one facade call still happen where `Output::lines` appends, so with `--line-buffered` each `nl` flushes (verified byte-identical
  to the baseline, also with output redirected to a file).
- **Threading.** `OutputBuf` is `Send` (one per worker, never shared); the ordered-mode flush runs under the Rust `OutputSync` lock; the raw `regs`
  and `eof` pointers are used by the owner thread only, `eof` through `AtomicBool::from_ptr`.
