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

**Port status: ported.** `ZthreadCpp`, the `CPP_RUST_IMPL_ZTHREAD` switch and `src/zthread.hpp` are deleted; the default build
runs the Rust thread (`src/zthread_core.rs`, unsafe only in `src/ffi/zthread_sys.rs`). The notes below describe the seam as
prepared; where they mention `ZthreadCpp`, the stub or the switch they are historical. Changes of the Port are in
D-zthread-signature and D-zthread-races of `migration/decisions.md`.

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
   (`CPP_RUST_IMPL_ZTHREAD`).
4. No tap: `src_core` is `cleaned: true` and its cleanup gate runs `policy-scan --no-tap rust/src_core/src src`, so no
   `CPP2RUST_TAP` / `TAP_*` code lives in the product tree.

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
The pipe read end crosses as an open `FILE*` (binary mode, `_O_BINARY` on Windows): Rust does the `fdopen(fd, "rb")` before it
hands the job to the thread, so an `fdopen` failure is reported as no pipe and leaves no thread on a closed `FILE`.
`FILE*` of the source file crosses as an opaque pointer, never dereferenced by the stub.

### Re-entrancy

Rust calls back into C++ only through the callbacks of `ugrep_core_zthread_env`. `set_partname` and `warning` take no
lock of this side; `select_by_flags` calls retained C++ that takes none either (it builds a local `reflex::Matcher`, the
legacy code did the same on the decompression thread). No callback calls a facade function. The legacy
`ZthreadCpp::start`/`open_next` recurse through the chain (`ztchain->start`, `ztchain->open_next` from the next-stage thread):
that stays inside one side, the facade is not re-entered.

### Thread affinity

`start`, `open_next` and `close` are called by the thread that owns the `Grep` (a `GrepWorker` thread, or the main
thread for stdin). The decompression thread (one per stage) is created by `start` and joined by `close`; it calls
`set_partname`, `warning` (`errno` is the thread's own: Rust must set `errno` before the callback as the C++ pipe code
left it) and `select_by_flags`. The stub starts no thread.

**`cancel` may come from any thread, at the same time as `start`/`open_next`/`close_file` work of the search thread.**
The query UI thread calls it (`query.cpp:900,1487,1806,1831,1855,1863` -> `Static::cancel_ugrep()` -> `Grep::cancel()` ->
`zthread.cancel()`) while the search thread is inside `search`/`open_file`/`close_file`; `grep_handle_mutex` does not
serialize this. Consequences for Rust: the FFI forms only `&Zthread` (never `&mut`) in `start`, `open_next` and `cancel`;
`Zthread` methods other than `close` take `&self`; the cancel flag is an `AtomicBool` and all other mutable state of a
stage goes behind a `Mutex` (with `Condvar`s). `close` takes the handle by value and runs on the owner thread after the
search ended, as with the legacy destructor; `Zthread` is `Send + Sync` because of this, documented in the Port.

**Port requirements on the switch:** the gate judges the default build, so the Port (or Switch over) must delete
`ZthreadCpp`, its facade branch and the `CPP_RUST_IMPL_ZTHREAD` switch (`rust.props`, `rust.targets`,
`CPP_RUST_IMPL_ZTHREAD_RUST` in `ugrep.cpp`); flipping the default is not enough (the lesson of the Output ports, #90).

### Owner side

| Object | Owner | Crossing |
|--------|-------|----------|
| the handle (thread state, chain, `zstreambuf` or Rust zstream) | **Rust** from `new` to `close` | opaque `void*`; the facade closes it in `~Zthread` |
| pipe read end | C++ after the call returns | open `FILE*` made by Rust with `fdopen` before the thread is told, `fclose`d by `Grep::close_file`; if `fdopen` fails Rust closes both pipe ends, warns and signals the thread as for a failed `pipe()` |
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
At Prepare seam, `src/zthread.hpp` (an older, ugrep-indexer copy of the same struct) was **kept** (the Port deleted it, Q-zthread-header): no source includes it any more
(`src/ugrep-indexer.cpp` is the Rust entry stub), so it is not compiled, but `msvc/ugrep-indexer.vcxproj:101` lists it as
a `ClInclude` and `src/Makefile.am:67,95` distribute it. Removing it changes the project and the distribution lists,
which is not a seam-preparation move; it goes with the Cleanup that deletes `ZthreadCpp`.

### Tap

None in the product tree (`src_core` is cleaned; `policy-scan --no-tap` would fail on it). New behavior is recorded from a
reference build outside the product, like `migration/harness/zstream/build-codec-ref.ps1`, and the Port is judged with
surface cases (`unit-cleaned`).

### Build

`/p:CPP_RUST_IMPL_ZTHREAD=cpp|rust` (default `cpp`) in `msvc/rust.props`; `rust` defines `CPP_RUST_IMPL_ZTHREAD_RUST` for
every `ClCompile`, so the facade calls `ugrep_core_zthread_*` of the always-linked umbrella. The facade only exists when
`HAVE_LIBZ` is defined (as before); the Base configurations may not define it.

### Text for the migration records (to be appended by a task that may write `migration/`)

`decisions.md`: `D-zthread-seam` | scope | the Zthread port is back in scope (D-S1-supersede); Prepare seam done; Rust Zthread lives
in `rust/src_core` with a dependency on `rust/src_leaves` (acyclic); `src/zthread.hpp` kept, listed only (reason above; deleted by the Port, Q-zthread-header) |
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
## ugrep.cpp Grep::ugrep/select/recurse retained (D-src_core-select-retained)

The ugrep.cpp ~9166-10482 range (Grep::ugrep, select, recurse, Static helpers) and `Grep::compute_cost` stay C++. `Grep::search` is no longer retained: the earlier D-src_core-search-retained decision is superseded by D-S1-supersede, see "Grep seam" below.

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

## Grep seam (Prepare seam: `Grep::search` split into branch functions; D-S1-supersede supersedes the earlier "Grep::search retained" notes)

Behavior unchanged, default build stays C++. `Grep::search` is now a thin shell plus one member function per mode branch. Nothing is switched yet:
the Rust bodies in `signatures::search` are stubs that `panic!` (no `todo!()`). There are 12 branch stubs and 5 remaining children; the table in
"Which child ports which stub" says who ports which.

### What crosses and what is deleted later
- Crosses (declared in the bridge, `lib.rs`; types in `rust/src_core/src/search.rs`, included from `signatures.rs` with `#[path]`; the C ABI of the host
  table is `src/search_seam.h` and `ffi/search_host.rs`): the matcher handle (`MatcherHandle`, opaque), the search context (`SearchContext`, Rust-owned,
  opaque), one function per branch and the host table.
- **Every matcher kind crosses, at the `reflex::AbstractMatcher` level** (`MatcherKind`): `reflex::Matcher` (`REFLEX` 1), `PCRE2Matcher` (`PCRE2` 2),
  `LineMatcher` (`LINE` 3, `--match`), `FuzzyMatcher` (`FUZZY` 4, `-Z`) and `BoostPerlMatcher` (`BOOST` 5). `PCRE2_UTF` is dropped. No search stays in C++
  (D-S1-supersede). The `-Zbest` calls on a `FuzzyMatcher` are explicit handle methods: `fuzzy_distance(max)` (`distance()`) and `fuzzy_edits()` (`edits()`).
- Nothing in `Grep` stays for good. S4c-6 deletes the C++ `Grep`, including the handler classes. Until each branch is ported, the `goto done_search/exit_search`
  flow stays in the C++ shell and each branch returns `SearchStep::DONE/EXIT`; only the RE-flex/PCRE2/Boost matcher objects stay C++-owned.
- Moves made: (1) `SearchContext` + `SearchStep` + 12 member declarations in `Grep`; (2) branch bodies moved verbatim into member functions
  (original indentation kept so the diff reads as a move); (3) shell dispatches with the original conditions. No tap is added to `src/`.

### `Grep::search` shell
Prologue (best-match cost, `open_file`, label fix) -> `SearchContext ctx(pathname, cost)` -> `search_setup` -> dispatch chain -> `search_epilogue`
unless the branch returned `EXIT` -> catch blocks, `exit_search:` (`out.release()`, `close_file(ctx.pathname)`, `Stats::found_file()` if `ctx.matched`).

### Search context (shared locals): `Grep::SearchContext` / Rust `SearchContext`
Fields: `pathname` (`const char *`, bytes in Rust), `cost` (`uint16_t`), `matched` (`bool`, `Cell<bool>` in Rust), and in Rust the attached `GrepHost`
and `flags` (`GrepFlags`, read from `settings.rs` `UGREP_SETTINGS` by `flags_from_settings` when the host is attached; no callbacks; `color_*`/`match_*` strings are read from `ugrep_settings` directly).
Other state read by branches (not in the context; stays in `Grep` members, reached through `GrepHost`, or branch-locals): `lineno`, `heading`, `matches`, `stop`, `binfile`,
`hex`, `binary`, `restline*`, `partname`, `out`, `matcher`/`matchers`, `matching`/`notmatching`.

### Branches (dispatch function, condition, context fields, handle mapping)
| Branch | `Grep::` function (Rust `branch::`) | Condition | Context fields | Handle methods used |
|---|---|---|---|---|
| setup | `search_setup` (`setup`) | always | none (members `out`, `matchers`, `matching`, `notmatching`) | none |
| -q/-l/-L | `search_quiet_list` | `flag_quiet \|\| flag_files_with_matches` | `pathname` | find, bol, eol, lineno, lineno_skip; host: out acquire/header/format/holding, cnf_matching |
| -c | `search_count` | `flag_count` | `pathname` | find, bol, eol, at_bol, skip, lineno, lineno_skip; host: cnf_matching, Stats, out |
| --format | `search_format` | `flag_format != NULL` | `pathname` | find, bol, eol, at_bol, skip, before, after, lineno, lines, set_shift_handler(FORMAT_INVERT_MATCH), invoke_handler; host: `out_format` with the `matching` counter |
| -o | `search_only_matching` | `flag_only_matching` without -ABC | `pathname` | find, bol, eol, begin, first, size, lineno, lineno_skip, lines, bytes; host: out dump, header, format |
| -o -ABC | `search_only_matching_context` | `-o` with -ABC | `pathname` | as -o plus end, last, border, set_shift_handler(ONLY_MATCHING_CONTEXT); host: `utf8strn`, restline |
| plain/hex | `search_plain` | no -ABC, -v, -y | `pathname` | find, bol, eol, begin, end, first, last, size, bef(len), aft(len), border, at_bol, skip, hit_end, set_reserve, lineno, lineno_skip, lines, bytes, set_shift_handler(GREP); host: restline_*, out |
| -v | `search_invert` | `-v` only | `pathname` | find, bol, eol, before, after, lineno, lines, set_shift_handler(INVERT_MATCH), invoke_handler |
| -y | `search_any_line` | `-y` | `pathname` | as plain plus before, after, set_shift_handler(ANY_LINE), invoke_handler |
| -ABC | `search_context` | -ABC, no -v | `pathname` | as -y with set_shift_handler(CONTEXT); host: `handler_output_before_context`, `handler_set_after_lineno` |
| -ABC -v | `search_context_invert` | -ABC with -v | `pathname` | as -y plus columno, set_shift_handler(INVERT_CONTEXT); host: `handler_add_before_context_line`, `handler_add_before_context_match` |
| epilogue | `search_epilogue` | after any non-EXIT branch | `matched` (set true), `pathname` unused | lineno; host: `cnf_satisfied`, `matches`, `out.nl`, `Stats::score_matches` |

Matcher-handle API: `find`, `next_match(skip_line)`, `bol`, `eol(inclusive)`, `begin`, `end`, `text_bounds` (begin/end/first/last/size/bol/eol/lineno/lines
snapshot), `with_bytes(offset, len, |b| ..)` (unsafe fn) / `bytes_to_vec(offset, len)` (the buffer bytes; there is no method returning a borrowed slice), `before()`/`after()` returning `BufContext {offset, len, num}`, `bef(len)`/`aft(len)` returning an offset,
`lineno_skip`, `reset`. `lineno()` and `lines()` are handle calls (hot loop), also snapshotted in `text_bounds`.
Buffer-shift event: `set_shift_handler(HandlerKind)` registers one of the C++ handler classes (GREP, ONLY_MATCHING_CONTEXT, INVERT_MATCH,
FORMAT_INVERT_MATCH, ANY_LINE, CONTEXT, INVERT_CONTEXT; `NONE` unregisters) as the matcher's event handler, or `set_rust_handler(Box<dyn ShiftHandler>)`
registers a Rust one (`RUST`); `invoke_handler(BufContext)` is the explicit `handler(*matcher, buf, len, num)` call; `shift_event` delivers the event.
Also on the handle: `first`, `last`, `size`, `columno`, `at_bol`, `hit_end`, `skip(c)`, `border`, `set_reserve`; for a `FuzzyMatcher` the `-Zbest` calls
`fuzzy_distance(max)` and `fuzzy_edits()`.
Every call in the branch bodies maps to one of these methods or to `GrepHost`; the mapping was checked branch by branch against the call lists of the
`search_*` bodies. The discriminants and the `GrepEnv` layout are pinned by Rust tests against `src/search_seam.h`.

### Host callback interface (`GrepHost`, `ffi/search_host.rs`)
What stays in C++ and a branch needs reaches Rust through `GrepHost` (`src/search.rs`), in the style of the Zthread `Host`: a `#[repr(C)]` table
`GrepEnv` of `extern "C"` callbacks with an opaque `ctx`, attached to the context by `ugrep_core_search_set_host(ctx, env)`; branches reach it through
`SearchContext::with_host`. The table is a fixed signature: the C++ facade (a `ugrep_core_search_env` header) is filled by S4c-2 and later children
without editing the trait or the table. It carries:
- `Grep` members: `matches`, `lineno`, `heading`, `stop`, `binfile`, `hex`, `binary`, `partname`, `restline_size`, `restline_last` (get/set), and
  `output_restline`, `output_or_save_restline`, `save_restline`.
- `restline_data` get/set (`RestlineData`: none, an offset into the matcher buffer, or the saved `restline` string), `restline_advance`, `restline_bytes`.
- Context-handler methods (`handler_op`): `handler_output_before_context`, `handler_set_after_lineno`, `handler_add_before_context_line`,
  `handler_add_before_context_match`.
- CNF and matchers: `cnf_matching(bol, eol, acquire)`, `cnf_satisfied(acquire)`, `matchers_len` (`None` = NULL), `matchers_matcher(i)` (a handle of any
  kind), `matchers_reset` (the `--files` bitmask reset of `search_setup`).
- `out` (`Output`): `eof`, `hold`, `holding`, `acquire`, `release`, `check_flush`, `chr`, `str`, `utf8strn`, `num`, `nl`, `header` (both forms),
  `binary_file_matches`, `format` (both forms; the in/out `matching` counter is `Option<&mut usize>`, `None` for --format-open/-close), `dump.hex` (modes `HEX_MATCH` 0, `HEX_LINE` 1, `HEX_CONTEXT_MATCH` 2, `HEX_CONTEXT_LINE` 3, exported in `search.rs` and `search_seam.h`), `dump.next`, `dump.done`.
- `Stats`: `found_file`, `found_part`, `undo_found_part`, `found_parts`, `score_matches`.
- Flags are not callbacks: `GrepFlags` is read from `settings.rs` (`UGREP_SETTINGS`) directly.
- The C ABI is mirrored by `src/search_seam.h` (`struct ugrep_core_search_env`); Rust tests pin enum discriminants and field offsets.
Callbacks run on the searching thread only; byte results stay valid until the next callback.

### Which child ports which stub
Bodies of `signatures::search::branch` (and the bridge `search_*` functions, unchanged):
| Child | Ports |
|---|---|
| S4c-2 | no branch stub: Grep file handling (`cnf_matching`, `open_file`, `filter`, `close_file`, `init_read`, restline) through the host interface |
| S4c-3 | `setup`, `quiet_list`, `count`, `format` |
| S4c-4 | `only_matching`, `only_matching_context` |
| S4c-5 | `plain`, `invert`, `any_line` |
| S4c-6 | `context`, `context_invert`, `epilogue`; then `find_text_preview`, `extract`, delete C++ `Grep` |
That is all 12 stubs: 4 + 2 + 3 + 3.
### Every `Grep` member
- Nested types: `EXIT_SEARCH`, `Type`, `Entry`, `Job`, `JobQueue` (two variants under `WITH_LOCK_FREE_JOB_QUEUE`), `BinaryHandler`, `StdInHandler`
  (non-Windows), `GrepHandler`, `OnlyMatchingContextGrepHandler`, `InvertMatchGrepHandler`, `FormatInvertMatchGrepHandler`, `AnyLineGrepHandler`,
  `ContextGrepHandler`, `InvertContextGrepHandler`, new `SearchContext`, `SearchStep`. Subclasses: `GrepMaster`, `GrepWorker` (retained).
- Methods: constructor, `~Grep`, `cancel`, `ugrep`, `select`, `recurse`, `compute_cost`, `search` (shell), the 12 `search_*` above,
  `find_text_preview`, `extract`, `cnf_matching`, `cnf_satisfied`, `open_file`, `filter`, `close_file`, `init_read`, `init_is_binary`,
  `output_or_save_restline`, `output_restline`, `save_restline`.
- Fields: `filename`, `partname`, `restline`, `restline_data`, `restline_size`, `restline_last`, `out` (`Output`), `matcher`, `matchers`, `matching`,
  `notmatching`, `mmap`, `input`, `file_in`, `interactive`, `binfile`, `lineno`, `heading`, `hex`, `binary`, `matches`, `stop`, `bin_handler`,
  `stdin_handler` (non-Windows), `zthread` (or `zstream`/`stream` under `HAVE_LIBZ`; see the Zthread seam).
All stay in C++ in this stage; the branch functions read them through `this`.

### Encoding, re-entrancy, thread affinity, owner, errors
- **Encoding:** paths and text are bytes (`&[u8]`), offsets `usize`, no UTF-8 assumption; `cost` is `u16`.
- **Re-entrancy:** the C++ matcher fires the shift handler during `find`/`eol(true)` while a branch runs, and the handler reads `ctx.pathname`.
  Therefore every handle method takes `&self`, branches take `&SearchContext`, `matched` is a `Cell`; no `&mut` is held across a call into C++.
- **Thread affinity:** a `SearchContext` and `MatcherHandle` are created and used on the one worker thread running `Grep::search`; they are not
  `Send`/`Sync`-shared. `cancel()` from the TUI thread touches `Grep` members only, never the handle.
- **Owner side:** `SearchContext` owned by Rust (boxed, created and freed by the shell); matcher object owned by C++ (the handle borrows it through its `MatcherTable`);
  pathname memory owned by the caller of `search`.
- **Error kinds:** the stubs `panic!` (`panic = "abort"` in the profile); the ported branches report I/O failure as C++ does today (exceptions
  `EXIT_SEARCH`/`std::exception` caught in the shell), result is a `SearchStep` (`u8`: CONTINUE 0, DONE 1, EXIT 2), not an error.

### Matcher callback table (how Rust calls the C++ matcher)
`MatcherHandle` carries a `MatcherTable` (`search.rs`, mirrored by `struct ugrep_core_matcher_table` in `src/search_seam.h`): a `ctx` (the facade's
handle of the matcher), the `kind`, and one `extern "C"` function pointer per handle method: `find`, `next_match`, `bol`, `eol`, `begin`, `end`,
`text_bounds`, `first`, `last`, `size`, `lineno`, `lines`, `columno`, `at_bol`, `hit_end`, `skip`, `border`, `before`, `after`, `bef`, `aft`, `bytes`,
`set_reserve`, `fuzzy_distance`, `fuzzy_edits`, `lineno_skip`, `set_shift_handler` (handler construction by `HandlerKind`), `set_rust_handler`
(trampoline registration) and `invoke_handler`, plus `reset`: 32 words (38 after "Grep file members" below). The facade fills one table per matcher; `GrepEnv.matchers_matcher(index)` fills
it for `matchers->at(index)`, and the C entry `ugrep_core_matcher_handle_new(table)` copies it for `Grep::matcher` (one handle per matcher). Rust tests pin the layout and run the
handle against a fake matcher through the table; the header pins enum last values, `offsetof` and `sizeof` with `static_assert`.
- **Buffer bytes:** `MatcherHandle` has no method returning `&[u8]`. `bytes_to_vec` returns an owned copy (a cost for the performance task on the hot
  path), and `unsafe fn with_bytes(offset, len, |b| ..)` lends the slice zero-copy: while the closure runs, every other call on the handle panics, and the caller guarantees the handle is the only one on the matcher, so no call can
  shift the buffer under the slice. The host (`out_str` and the like) does not touch the matcher and may be called from the closure.
- **Handler lifetime:** the C++ handler object (`GrepHandler` and its derived classes, or the trampoline of a Rust handler) is owned by the facade. It
  lives until the next `set_shift_handler`/`set_rust_handler`, `reset` or the destruction of the facade, whichever comes first, and the facade never
  leaves the matcher pointing at a destroyed handler. `reset()` destroys the registered handler (RE-flex forgets it as well), so a branch registers
  its handler again after `reset` and after each new input source (once per file). A Rust handler lives in the handle: `reset` and the next
  registration drop it after C++ replaced the trampoline, and dropping the handle first unregisters the trampoline. Handler classes are registered only
  by `set_shift_handler`; `HandlerKind::RUST` is registered by `set_rust_handler` alone.
- **unsafe:** every raw-pointer read, the `bytes`/`deref`/`read` functions (all `unsafe fn` with a `# Safety` contract), the slice creation of `with_bytes`,
  and the shift trampoline `rust_shift_trampoline` are in `ffi/matcher_raw.rs`, declared from `search.rs`, so policy-scan sees them in `ffi/` and the
  signatures-only harness still builds. The table reaches Rust through the C entry `ugrep_core_matcher_handle_new(const ugrep_core_matcher_table *)`
  (`ffi/search_host.rs`, declared in `src/search_seam.h`, a typed pointer, no `usize`) and goes away with `ugrep_core_matcher_handle_free`.
- **One handle per C++ matcher:** the `lent` guard of `with_bytes` and the Rust handler slot live in the handle, so they only protect the matcher if no
  second handle exists for it. `Grep::matcher` gets one handle from `ugrep_core_matcher_handle_new`, but `GrepHost::matchers_matcher(i)` builds a
  new handle on every call, so a branch must not hold two handles on one matcher while it uses `with_bytes`. `with_bytes` is therefore `unsafe fn`:
  its contract is "the only handle, no matcher call while the closure runs". `bytes_to_vec` copies immediately and is safe: `bytes` returns NULL
  outside the buffer and Rust aborts on NULL. `raw::new_handle` (the only constructor of a handle) and the shift trampoline are `unsafe fn` in `ffi/`, so no safe code builds a handle from
  an arbitrary table or calls the trampoline with an arbitrary pointer.
- **Re-entrancy:** a shift event arrives inside `find`, `next_match` or `eol(true)` through the trampoline into `dispatch_shift`, which only borrows the
  handler slot; a handler that replaces itself while running is skipped, not aliased.

### Unknown ownership
- None for the matcher calls: they go through the table above. Open for the Port that fills the facade: the concrete C++ functions behind each table
  entry (S4c-2 fills them with the first switched branch).

## Grep file members (Prepare seam S4c-2a; C++ bodies stay active, default build C++, Rust is panic stubs)

Members: `cnf_matching`, `cnf_satisfied`, `open_file`, `filter`, `close_file`, `init_read`, `init_is_binary`, `output_or_save_restline`,
`output_restline`, `save_restline`. Rust entry points: `grep_search::file::*` (panic stubs) behind cxx bridge functions `search_cnf_matching`,
`search_cnf_satisfied`, `search_open_file(ctx, pathname, find, has_find, is_stdin)`, `search_filter`, `search_close_file`,
`search_init_read(state: &GrepState, ctx, matcher)`, `search_init_is_binary`, `search_output_or_save_restline`, `search_output_restline`,
`search_save_restline` (lib.rs). `GrepState` (`grep_state_new()`) is the Grep-lifetime Rust state (the mmap mapping); the facade owner creates it with
the Grep and drops it with the Grep (Drop unmaps in the mmap port).

- **Facade:** `Grep::SeamFacade` in `src/ugrep.cpp` fills `ugrep_core_search_env` (66 words) and the matcher table (39 words) with non-capturing
  lambdas. `matcher_kind()` picks the kind by `dynamic_cast` (FuzzyMatcher, LineMatcher, PCRE2 under `HAVE_PCRE2` or Boost under `HAVE_BOOST_REGEX`,
  Matcher); one template (`fill_matcher_table`) serves all kinds. Nothing instantiates it in the default build. `bind(pathname)` sets a slot the
  handlers refer to by reference, so handlers made before `bind()` work; one facade per Grep on the search thread, destroyed before the matchers it
  handed out. `clear_handler` clears only the handler pointers that are this matcher's own.
- **Env slots added** (appended, old offsets unchanged; 66 total): filename, interactive (get/set), file_in (get/set), input (file/set/clear/get),
  source, stdin_file, filter_magic_scan, warning, zthread_start/open_next/cancel, cnf_alts, cnf_alt_matcher, cnf_bit_get/set, then the review gaps:
  error_pending, restline_set, input_encoding, encoding_null_data, flag_filter, flag_filter_magic_label, file_open, file_close, file_read_error,
  file_set_binary, file_nonblock_regular, file_probe_interactive, file_seek_set, stdin_drain_check, filter_spawn, is_binary. **No mmap slot**: mmap is
  ported to Rust (operator answer); `Static::filter_magic_pattern` is only used through `filter_magic_scan`.
- **zthread slots are never NULL:** with `HAVE_LIBZ` and `WITH_DECOMPRESSION_THREAD` they forward to `Grep::zthread`; otherwise (Base-Release has no
  `HAVE_LIBZ`) they are stubs (`start`/`open_next` return NULL, `cancel` no-op) and the Rust mirror reads them as `Option<extern "C" fn>`. Rust calls
  them only when `flag_decompress` is set, which needs `HAVE_LIBZ`.
- **Matcher-table slots added** (39 total): input, buffer, buffer_all, avail, begin_ptr(ctx, len) (pointer to the match start, NULL unless `len` bytes lie
  inside the data; `begin` stays an offset), set_input_handler (bin/stdin handler through `in.set_handler`), buffer_line(ctx, src_ctx, bol, eol)
  (cnf_matching reads a line of the buffer of `Grep::matcher` with another matcher). Pins in `search_seam.h` and the Rust layout tests agree
  (`input`=32, `begin_ptr`=36, `set_input_handler`=37, `buffer_line`=38; env `filename`=31, `cnf_bit_set`=49, `error_pending`=50, `is_binary`=65).
- **Exception channel (cpp-rust-error-handling):** an exception must not unwind through a Rust frame. Every facade callback (env and matcher table)
  runs under `guard()`: `catch (...)` stores a `std::exception_ptr`, the callback returns a neutral value (0/NULL/false) and later callbacks do
  nothing. Rust reads `error_pending` after callbacks, stops with `EXIT` (`cnf_matching`/`cnf_satisfied` return 2) and returns; the C++ caller of the
  bridge calls `rethrow()`. `EXIT_SEARCH` of `cnf_matching`/`cnf_satisfied` is the protocol exception: caught inside the callback and returned as
  `UGREP_CORE_CNF_EXIT` (2 == `STEP_EXIT`); the C++ members still throw it for the active C++ path. 0 = no, 1 = yes. Panics abort.
- **No path to UB:** the handle types `BufferPtr`, `FileHandle`, `InputHandle` have private fields (built only by `unsafe` `ffi/` constructors);
  `copy_begin` is `unsafe`; `begin_ptr` takes `len`. The facade checks every offset and index Rust passes (cnf bol/eol <= buffer end, restline
  offsets and sizes against the data they index, `invoke_handler`, `add_before_context_line`, `cnf_bit_*`, `buffer_line`); a bad value is a
  `std::out_of_range` on the exception channel, never an access out of bounds.
- **Restlines:** `restline_data_get` returns BUFFER (offset in the buffer of `Grep::matcher`), SAVED (offset in `Grep::restline`) or NONE. `restline_set`
  is the primitive of `save_restline` (`restline = bytes; restline_data = restline.c_str()`, `restline_size` unchanged), so Rust does not recurse into
  `save_restline`.
- **filter:** the OS part (pipe, fork or CreateProcess) is split from `Grep::filter` into `Grep::filter_spawn` (same code, behavior unchanged) and
  reached by the `filter_spawn` slot; on failure `file_in` is cleared and 0 is returned, which Rust handles as `None`.
- **open_file/close_file/init_read gaps found by walking the bodies:** `is_stdin` (pointer identity with `LABEL_STANDARD_INPUT`), `fopenw_s` with its
  errno-sensitive warning (`file_open`), `_setmode` (Windows), the Linux `O_NONBLOCK` regular-file path, the interactive probe, `ferror` + warning,
  `fclose(input.file())`, the stdin drain, `input_encoding` and `encoding_null_data` for the mmap check, `buffer_all` (Boost), `is_binary`.
- **Build facts:** `WITH_DECOMPRESSION_THREAD` and `WITH_NO_MMAP` are defined unconditionally in `ugrep.hpp`, so the non-thread zstream branch of
  `open_file`/`close_file` is dead in this build and is not bridged. Built: Base-Release. Not built: Pcre-Release (the pcre2, zlib, lz4, brotli, zstd,
  bzip sources are absent in the sandbox) and `HAVE_BOOST_REGEX`; the `HAVE_PCRE2` and Boost branches are unverified by compilation.
- **cnf offsets** are relative to the buffer base of `Grep::matcher`; Rust derives pointers from `begin_ptr`.
  `matchers_matcher(index)` is the first matcher of AND term `index`; `cnf_alts`/`cnf_alt_matcher`/`cnf_bit_*` give the full walk; the CHost caches
  `cnf_alt_matcher` handles.
- **Open for S4c-2b:** ownership of `Grep::file_in`/`input` once Rust `open_file` is switched; the switch and deletion of the C++ bodies.
