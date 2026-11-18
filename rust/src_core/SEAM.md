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
| `init(argc, argv)` | same name | `init_cpp()` | C ABI `ugrep_core_init(int, const char**)` (`ffi/init.rs`; argv pointers are stored in the settings, so they cannot be copied across cxx) | `CPP_RUST_IMPL_SRC_CORE_INIT` |

Types and signatures: `rust/src_core/src/signatures.rs` module `ui` (stubs: `todo!()` until the Ports; a Port does not change a signature). Build: msbuild `/p:CPP_RUST_IMPL_SRC_CORE_TERMINAL=rust` and/or `/p:CPP_RUST_IMPL_SRC_CORE_INIT=rust` define `CPP_RUST_IMPL_SRC_CORE_{TERMINAL,INIT}_RUST` (`msvc/rust.targets`); autotools builds the cpp path (`CPPFLAGS=-DCPP_RUST_IMPL_SRC_CORE_TERMINAL_RUST` selects Rust by hand). The tap was deleted at Cleanup, so no tap macros were added; the replay is the surface corpus.

Not switched by anything else: none of the six remain C++-only. `find_text_preview`: the facade `ugrep_find_text_preview` (src/ugrep.cpp, end of file) keeps the old body as `ugrep_find_text_preview_cpp` (which alone builds the `Grep`); its signature uses only boundary types, so the Rust entry is `core_find_text_preview(filename: &[u8], partname: &[u8], from_lineno, max) -> Result<TextPreview{lineno, num, text: Vec<Bytes>}>` and the facade copies the result into the out-parameters (assign into `text[i]` when it exists, else `emplace_back`; the tail is left untouched). Switch `..._TERMINAL`. The facade keeps the `Static::matcher == NULL` early return before calling Rust (outputs untouched); a Port must reproduce the exceptions listed under Error kinds via `Err`; the Rust body will need the `Grep` search as a callback until `Grep` is ported.

**Terminal Port (done).** The `CPP_RUST_IMPL_SRC_CORE_TERMINAL` sub-switch and the `*_cpp` bodies of `terminal`, `set_terminal_hyperlink`, `set_color` and `trim` are deleted; the facades call Rust unconditionally (only `INIT` remains a sub-switch). `terminal()` and `set_terminal_hyperlink()` use a C-ABI callback table (`TerminalEnv`, `ffi/terminal.rs`, entries `ugrep_core_terminal` / `ugrep_core_set_terminal_hyperlink`) instead of the cxx bridge: a deliberate signature change, so their `signatures::ui` stubs and bridge fns were removed. `set_color`/`trim` stay cxx (`core_set_color`, `core_trim`) and were removed from `signatures::ui`. `find_text_preview` is NOT ported: it is only a reflex matcher clone plus `Grep` (unported C++), so it stays C++ as `ugrep_find_text_preview`; its Rust stub, bridge fn and `TextPreview` struct were removed.

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

`src/output.cpp` (Output and Output::Dump members) stays C++. Output is a fine-grained hot-path buffered writer used by the whole search driver; a per-member facade would add an FFI crossing per write, and no seam, signatures or traces exist for it. Porting it later needs one coarse seam together with the search driver (its own Prepare seam + Behavior follow-up).


## ugrep.cpp globals/pager range retained (D-src_core-globals-retained)

The ugrep.cpp ~228-757 and ~4456-4670 range (flag globals, Static members, tables, pager, signal handlers, CPU affinity, fopen_smart) stays C++: the flag/Static ownership decision keeps the globals in C++, and no seam signature, tap or recorded case exists for the pager, signal, affinity and fopen_smart functions. Porting it later needs its own Prepare seam + Behavior follow-up.

## ugrep.cpp Zthread retained (D-src_core-zthread-retained)

The ugrep.cpp ~757-1909 range (struct Zthread: decompression thread, pipes, FILE*) stays C++. zstream and zopen are already Rust from src_leaves; only the threading and pipe plumbing stays C++. No seam signature, tap or recorded case exists. Porting it later needs its own Prepare seam + Behavior follow-up.

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
