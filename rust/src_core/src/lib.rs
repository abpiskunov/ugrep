//! Rust side of the `src_core` component (src/cnf.cpp, src/stats.cpp; later src/ugrep.cpp and src/output.cpp).
//! One bridge module (`ffi`); all boundary code is here. The bodies are `panic!` stubs until the Port tasks.
//!
//! Only `extern "Rust"` functions: the C++ facade calls Rust, Rust never calls the component's legacy C++.
//! State stays in C++ (see SEAM.md): everything crosses by value, as a snapshot in and a result out.

// public so the signatures that are not called yet (stubs until the Port tasks) are part of the crate interface
pub mod signatures;

// the Rust-owned option state read by the remaining C++ (src/flag.hpp)
pub mod settings;
#[path = "ffi/settings.rs"]
pub mod settings_ffi;
// option parsing (`options()` of src/ugrep.cpp) and its C-ABI entry with the callbacks into the retained C++
pub mod options;
#[path = "ffi/options.rs"]
pub mod options_ffi;
// `load_config` and `save_config` of src/ugrep.cpp and their C-ABI entries
pub mod config;
#[path = "ffi/config.rs"]
pub mod config_ffi;
// `init()`: logic in Rust on the settings, C-ABI entry with the callbacks into the retained C++
pub mod init;
#[path = "ffi/init.rs"]
pub mod init_ffi;
// `usage`, `help`, `version`, `warning`, `error`, `abort`, `is_directory`, `cannot_decompress` and `main`
pub mod help_main;
#[path = "ffi/help_main.rs"]
pub mod help_main_ffi;
// `set_color` and `trim`
pub mod color;
// `terminal()` and `set_terminal_hyperlink()` and their C-ABI entries (callbacks into the retained C++)
pub mod terminal;
#[path = "ffi/terminal.rs"]
pub mod terminal_ffi;

// `Output` (src/output.hpp, src/output_rust.hpp): types-and-signatures of the output seam; stubs until the Port tasks
pub mod output;
pub mod output_format;
#[path = "ffi/output.rs"]
pub mod output_ffi;
pub mod output_sync;
#[path = "ffi/output_buf_core.rs"]
pub mod output_buf;
#[path = "ffi/output_buf.rs"]
pub mod output_buf_ffi;
#[path = "ffi/output_sync.rs"]
pub mod output_sync_ffi;
// the Zthread seam (decompression thread of src/ugrep.cpp): types in `zthread.rs` (declared from `signatures.rs`), the
// thread itself in `zthread_core.rs` on the zstream core of src_leaves, the C runtime calls (the `unsafe` of the unit)
// in `ffi/zthread_sys.rs` and the C-ABI entries of src/zthread_seam.h in `ffi/zthread.rs`
#[path = "ffi/zthread_sys.rs"]
pub mod zthread_sys;
pub mod zthread_core;
#[path = "ffi/zthread.rs"]
pub mod zthread_ffi;
#[path = "ffi/search_host.rs"]
pub mod search_host_ffi;

use signatures as sig;

#[cxx::bridge(namespace = "ugrep_rust")]
mod ffi {
    /// One group capture of the matcher: `group_id()`/`group_next_id()` entry and `(*matcher)[index]`.
    /// `offset` is relative to the start of `window` (`cap.first` minus the window start), so a lookbehind capture
    /// that begins before `begin()` stays representable.
    /// `listed` is true for a group of the `group_id()`/`group_next_id()` walk; false for an index that C++ reads
    /// directly with `(*matcher)[index]` because the format names it (`%[N]b`, `%1`), which works without the walk.
    struct OutputCapture {
        index: usize,
        has_name: bool,
        name: Vec<u8>,
        found: bool,
        listed: bool,
        offset: usize,
        size: usize,
    }

    /// Plain-data snapshot of a `reflex::AbstractMatcher` match, built by C++ per call (Rust cannot own the matcher).
    /// Offsets are relative to `window`, passed next to it. The window starts at `bol()` when `has_bol` (else at
    /// `begin()`), or earlier when a group capture begins before that, and ends at `max(eol(), end())` when `has_eol`
    /// (else at `end()`). `bol_off` and `eol_off` are the line bounds before that widening: the line of the match is
    /// `window[bol_off..eol_off]`. C++ sets them only when the
    /// format or flags need the line, as the inline `mat`/`quote` overloads do, because `eol()` may refill the buffer.
    /// `columno`, `lines`, `lineno_end`, `edits`/`distance` and `groups` are filled only when the format characters
    /// or flags that read them are present (`%K %k`, `%L %l`, `%Z`, and `groups` only for `%b %d %e %o %# %q %c %v %j %x %y %G %g %0-%9`, not for `%[..]` alone); otherwise they are zero/empty.
    /// `text()` is never called (it writes a NUL into the buffer); `text_off` equals `begin_off`.
    struct OutputMatch {
        present: bool,
        has_bol: bool,
        has_eol: bool,
        first: usize,
        last: usize,
        size: usize,
        wsize: usize,
        lineno: usize,
        lineno_end: usize,
        columno: usize,
        lines: usize,
        accept: usize,
        begin_off: usize,
        text_off: usize,
        end_off: usize,
        bol_off: usize,
        eol_off: usize,
        has_fuzzy: bool,
        edits: usize,
        distance: usize,
        groups: Vec<OutputCapture>,
    }

    /// Per-call state of the C++ `Output` object that the Rust function reads; `stdin_label` is the pointer test
    /// `pathname == Static::LABEL_STANDARD_INPUT`, which cannot cross.
    struct OutputCtx {
        lineno: usize,
        binary: bool,
        dump_incomplete: bool,
        stdin_label: bool,
    }

    /// Scalar arguments shared by the header and format functions (unused ones are zero/false).
    struct OutputArgs {
        matches: usize,
        has_matching: bool,
        matching: usize,
        heading: bool,
        body: bool,
        next: bool,
        lineno: usize,
        offset: usize,
        byte_offset: usize,
        has_separator: bool,
        newline: bool,
    }

    /// Result of `Output::header`: `complete_dump` asks the facade to call `dump.done()` before appending `bytes`.
    struct OutputHeaderResult {
        bytes: Vec<u8>,
        heading: bool,
        complete_dump: bool,
    }

    /// Result of `Output::format` and `format_invert`: `lineno_set` stores `lineno` in `Output::lineno_`.
    struct OutputFormatResult {
        bytes: Vec<u8>,
        ret: bool,
        heading: bool,
        matching: usize,
        lineno_set: bool,
        lineno: usize,
    }

    /// Result of `Output::binary_file_matches`: `set_binary` sets the BINARY mode bit.
    struct OutputBinaryResult {
        bytes: Vec<u8>,
        set_binary: bool,
    }

    /// Result of Output Dump line: the new pstar, and repeated for the star case (prevb kept, bytes reset);
    /// otherwise the facade also sets prevb = bytes and resets bytes.
    struct OutputDumpLine {
        bytes: Vec<u8>,
        pstar: bool,
        repeated: bool,
    }


    /// Bytes of a string, no NUL, no encoding assumed.
    struct Bytes {
        data: Vec<u8>,
    }

    /// `CNF::Pattern`: `defined` false is the NULL smart pointer.
    struct CnfPattern {
        defined: bool,
        text: Vec<u8>,
    }

    /// `CNF::Term`
    struct CnfTerm {
        patterns: Vec<CnfPattern>,
    }

    /// Snapshot of the `flag_*` globals the CNF code reads (`signatures::CnfFlags`).
    struct CnfFlags {
        mode: u8,
        line_regexp: bool,
        word_regexp: bool,
        boolean: bool,
        files: bool,
        invert_match: bool,
        ignore_case: bool,
        best_match: bool,
        fuzzy: usize,
        file: Vec<Bytes>,
        have_pcre2: bool,
    }

    /// Result of `CNF::anchor` (`signatures::Anchored`).
    struct CnfAnchored {
        pattern: Vec<u8>,
        enable_empty: bool,
    }

    /// Result of `CNF::compile` / `CNF::new_pattern` (`signatures::Compiled`).
    struct CnfCompiled {
        terms: Vec<CnfTerm>,
        enable_empty: bool,
    }

    /// Snapshot for `Stats::report` (`signatures::StatsInput`).
    struct StatsInput {
        searched_files: usize,
        searched_dirs: usize,
        searched_indexed: usize,
        searched_skipped: usize,
        searched_changed: usize,
        searched_added: usize,
        searched_lines: usize,
        found_files: usize,
        found_parts: usize,
        found_matches: usize,
        warnings: usize,
        threads: usize,
        elapsed_ms: f64,
        query: bool,
        pager: bool,
        quiet: bool,
        files_with_matches: bool,
        files_without_match: bool,
        ungroup: bool,
        count: bool,
        only_matching: bool,
        format: bool,
        index: bool,
        config_given: bool,
        config: Vec<u8>,
        config_files: Vec<Bytes>,
        fuzzy: usize,
        boolean: bool,
        files: bool,
        mode: u8,
        decompress: bool,
        zmax: usize,
        min_depth: usize,
        max_depth: usize,
        dereference: bool,
        no_dereference: bool,
        devices_action: u8,
        directories_action: u8,
        glob_ignore_case: bool,
        hidden: bool,
        hidden_default: bool,
        ignore_files: Vec<Bytes>,
        min_count: usize,
        max_count: usize,
        max_files: usize,
        min_line: usize,
        max_line: usize,
        sort_given: bool,
        sort: Vec<u8>,
        min_size: usize,
        max_size: usize,
        ignore: Vec<Bytes>,
        file_magic: Vec<Bytes>,
        include_fs: Vec<Bytes>,
        exclude_fs: Vec<Bytes>,
        all_include: Vec<Bytes>,
        all_exclude: Vec<Bytes>,
        all_include_dir: Vec<Bytes>,
        all_exclude_dir: Vec<Bytes>,
        include_iglob_size: usize,
        exclude_iglob_size: usize,
        include_iglob_dir_size: usize,
        exclude_iglob_dir_size: usize,
        crlf: bool,
        pathsep: u8,
    }

    /// Part of the line `trim` keeps: `line[begin..end]` (`signatures::TrimRange`).
    struct TrimRange {
        begin: usize,
        end: usize,
    }

    /// Positions of the current match of a matcher handle, offsets from the start of the matcher buffer
    /// (`signatures::search::TextBounds`).
    struct MatcherBounds {
        begin: usize,
        end: usize,
        first: usize,
        last: usize,
        size: usize,
        bol: usize,
        eol: usize,
        lineno: usize,
        lines: usize,
    }

    /// The buffer-shift event of `reflex::AbstractMatcher::Handler` (`signatures::search::ShiftEvent`); the shifted
    /// out text `buf[..len]` is passed next to it.
    struct MatcherShiftEvent {
        len: usize,
        num: usize,
    }

    extern "Rust" {
        /// The handle of a matcher that C++ owns, of any kind (`signatures::search::MatcherHandle`): `MatcherKind::REFLEX`,
        /// `PCRE2`, `LINE`, `FUZZY` or `BOOST`.
        type MatcherHandle;
        /// The Rust-owned locals shared by the `Grep::search` branches (`signatures::search::SearchContext`).
        type SearchContext;

        /// Seam function `src_core.matcher_handle_new`: `kind` is a `MatcherKind` value, `token` identifies the C++ matcher.
        fn matcher_handle_new(kind: u8, token: usize) -> Box<MatcherHandle>;
        /// Seam function `src_core.matcher_find`: `matcher->find()`. Every handle function takes `&MatcherHandle`:
        /// the C++ matcher mutates, and its shift handler re-enters while `find` or `eol` runs.
        fn matcher_find(handle: &MatcherHandle) -> bool;
        /// Seam function `src_core.matcher_next_match`: the loop step, optionally skipping the rest of the line.
        fn matcher_next_match(handle: &MatcherHandle, skip_line: bool) -> bool;
        /// Seam function `src_core.matcher_bol`.
        fn matcher_bol(handle: &MatcherHandle) -> usize;
        /// Seam function `src_core.matcher_eol`: `eol(inclusive)`.
        fn matcher_eol(handle: &MatcherHandle, inclusive: bool) -> usize;
        /// Seam function `src_core.matcher_begin`.
        fn matcher_begin(handle: &MatcherHandle) -> usize;
        /// Seam function `src_core.matcher_end`.
        fn matcher_end(handle: &MatcherHandle) -> usize;
        /// Seam function `src_core.matcher_text_bounds`.
        fn matcher_text_bounds(handle: &MatcherHandle) -> MatcherBounds;
        /// Seam function `src_core.matcher_lineno_skip`.
        fn matcher_lineno_skip(handle: &MatcherHandle, on: bool);
        /// Seam function `src_core.matcher_set_shift_listener`: C++ keeps the handler object.
        fn matcher_set_shift_listener(handle: &MatcherHandle, enabled: bool);
        /// Seam function `src_core.matcher_shift_event`: the C++ handler forwards the buffer shift (C++ calls Rust).
        fn matcher_shift_event(handle: &MatcherHandle, event: &MatcherShiftEvent, buf: &[u8]);
        /// Seam function `src_core.matcher_reset`.
        fn matcher_reset(handle: &MatcherHandle);
        /// Seam functions `src_core.matcher_*`: the remaining `reflex::AbstractMatcher` accessors the branches use.
        fn matcher_first(handle: &MatcherHandle) -> usize;
        fn matcher_last(handle: &MatcherHandle) -> usize;
        fn matcher_size(handle: &MatcherHandle) -> usize;
        fn matcher_lineno(handle: &MatcherHandle) -> usize;
        fn matcher_lines(handle: &MatcherHandle) -> usize;
        fn matcher_columno(handle: &MatcherHandle) -> usize;
        fn matcher_at_bol(handle: &MatcherHandle) -> bool;
        fn matcher_hit_end(handle: &MatcherHandle) -> bool;
        fn matcher_skip(handle: &MatcherHandle, c: u8) -> bool;
        fn matcher_border(handle: &MatcherHandle) -> usize;
        fn matcher_before(handle: &MatcherHandle) -> usize;
        fn matcher_after(handle: &MatcherHandle) -> usize;
        fn matcher_bef(handle: &MatcherHandle) -> usize;
        fn matcher_aft(handle: &MatcherHandle) -> usize;
        fn matcher_set_reserve(handle: &MatcherHandle, size: usize);
        /// Seam functions `src_core.matcher_fuzzy_*`: the `-Zbest` calls of a `FuzzyMatcher`.
        fn matcher_fuzzy_distance(handle: &MatcherHandle, max: u16);
        fn matcher_fuzzy_edits(handle: &MatcherHandle) -> u16;

        /// Seam function `src_core.search_context_new`: `Grep::SearchContext(pathname, cost)`.
        fn search_context_new(pathname: &[u8], cost: u16) -> Box<SearchContext>;
        /// Seam function `src_core.search_context_pathname`.
        fn search_context_pathname(ctx: &SearchContext) -> &[u8];
        /// Seam function `src_core.search_context_matched`.
        fn search_context_matched(ctx: &SearchContext) -> bool;

        /// Seam functions `src_core.search_*`, one per branch of `Grep::search`; the `u8` result is a `SearchStep`
        /// (CONTINUE 0, DONE 1, EXIT 2). The context is shared: the shift handler reads it while a branch runs.
        fn search_setup(ctx: &SearchContext);
        fn search_quiet_list(ctx: &SearchContext, matcher: &MatcherHandle) -> u8;
        fn search_count(ctx: &SearchContext, matcher: &MatcherHandle) -> u8;
        fn search_format(ctx: &SearchContext, matcher: &MatcherHandle) -> u8;
        fn search_only_matching(ctx: &SearchContext, matcher: &MatcherHandle) -> u8;
        fn search_only_matching_context(ctx: &SearchContext, matcher: &MatcherHandle) -> u8;
        fn search_plain(ctx: &SearchContext, matcher: &MatcherHandle) -> u8;
        fn search_invert(ctx: &SearchContext, matcher: &MatcherHandle) -> u8;
        fn search_any_line(ctx: &SearchContext, matcher: &MatcherHandle) -> u8;
        fn search_context(ctx: &SearchContext, matcher: &MatcherHandle) -> u8;
        fn search_context_invert(ctx: &SearchContext, matcher: &MatcherHandle) -> u8;
        fn search_epilogue(ctx: &SearchContext, matcher: &MatcherHandle);
    }

    extern "Rust" {
        /// Seam function `src_core.set_color`: `color` is the `char[COLORLEN]` the C++ passes.
        fn core_set_color(colors: &[u8], parameter: &[u8], color: &mut [u8]);
        /// Seam function `src_core.trim`: the facade erases what is outside the returned range.
        fn core_trim(line: &[u8]) -> TrimRange;
        /// Seam function `src_core.cnf_compile`.
        fn cnf_compile(terms: Vec<CnfTerm>, pattern: &[u8], flags: &CnfFlags) -> CnfCompiled;
        /// Seam function `src_core.cnf_new_pattern`.
        fn cnf_new_pattern(terms: Vec<CnfTerm>, mask: i32, pattern: &[u8], flags: &CnfFlags) -> CnfCompiled;
        /// Seam function `src_core.cnf_prune`.
        fn cnf_prune(terms: Vec<CnfTerm>, flags: &CnfFlags) -> Vec<CnfTerm>;
        /// Seam function `src_core.cnf_split`.
        fn cnf_split(terms: Vec<CnfTerm>, flags: &CnfFlags) -> Vec<CnfTerm>;
        /// Seam function `src_core.cnf_adjoin`.
        fn cnf_adjoin(terms: Vec<CnfTerm>, flags: &CnfFlags) -> Vec<u8>;
        /// Seam function `src_core.cnf_first`.
        fn cnf_first(terms: Vec<CnfTerm>) -> Vec<u8>;
        /// Seam function `src_core.cnf_report`: the text for the `FILE*` the C++ facade writes.
        fn cnf_report(terms: Vec<CnfTerm>, flags: &CnfFlags) -> Vec<u8>;
        /// Seam function `src_core.cnf_quote`.
        fn cnf_quote(pattern: &[u8]) -> Vec<u8>;
        /// Seam function `src_core.cnf_anchor`.
        fn cnf_anchor(pattern: &[u8], flags: &CnfFlags) -> CnfAnchored;
        /// Seam function `src_core.stats_report`: the text for the `FILE*` the C++ facade writes.
        fn stats_report(input: &StatsInput) -> Vec<u8>;

        /// Seam functions of the output component (see SEAM.md): bytes out, appended by the C++ facade.
        fn output_quote(data: &[u8]) -> Vec<u8>;
        fn output_cpp(data: &[u8]) -> Vec<u8>;
        fn output_csv(data: &[u8]) -> Vec<u8>;
        fn output_hex(data: &[u8]) -> Vec<u8>;
        fn output_json(data: &[u8]) -> Vec<u8>;
        fn output_xml(data: &[u8]) -> Vec<u8>;
        fn output_color(has_arg: bool, arg: &[u8]) -> Vec<u8>;
        fn output_header_line(
            pathname: &[u8],
            partname: &[u8],
            separator: &[u8],
            link: &[u8],
            args: &OutputArgs,
            matcher: &OutputMatch,
            ctx: &OutputCtx,
        ) -> OutputHeaderResult;
        fn output_header_file(pathname: &[u8], partname: &[u8], link: &[u8], ctx: &OutputCtx) -> OutputHeaderResult;
        fn output_header_tree(pathname: &[u8], partname: &[u8], link: &[u8], ctx: &OutputCtx) -> Vec<u8>;
        fn output_binary_file_matches(
            pathname: &[u8],
            partname: &[u8],
            link: &[u8],
            ctx: &OutputCtx,
        ) -> OutputBinaryResult;
        fn output_format_count(format: &[u8], matches: usize, ctx: &OutputCtx) -> Vec<u8>;
        fn output_format_match(
            format: &[u8],
            pathname: &[u8],
            partname: &[u8],
            window: &[u8],
            args: &OutputArgs,
            matcher: &OutputMatch,
            ctx: &OutputCtx,
        ) -> OutputFormatResult;
        fn output_format_invert(
            format: &[u8],
            pathname: &[u8],
            partname: &[u8],
            line: &[u8],
            args: &OutputArgs,
            ctx: &OutputCtx,
        ) -> OutputFormatResult;
    }
}

fn bytes_in(v: &[ffi::Bytes]) -> Vec<sig::Bytes> {
    v.iter().map(|b| b.data.clone()).collect()
}

fn terms_in(terms: &[ffi::CnfTerm]) -> sig::Terms {
    terms
        .iter()
        .map(|t| t.patterns.iter().map(|p| p.defined.then(|| p.text.clone())).collect())
        .collect()
}

fn terms_out(terms: sig::Terms) -> Vec<ffi::CnfTerm> {
    terms
        .into_iter()
        .map(|t| ffi::CnfTerm {
            patterns: t
                .into_iter()
                .map(|p| ffi::CnfPattern { defined: p.is_some(), text: p.unwrap_or_default() })
                .collect(),
        })
        .collect()
}

fn flags_in(f: &ffi::CnfFlags) -> sig::CnfFlags {
    sig::CnfFlags {
        mode: f.mode,
        line_regexp: f.line_regexp,
        word_regexp: f.word_regexp,
        boolean: f.boolean,
        files: f.files,
        invert_match: f.invert_match,
        ignore_case: f.ignore_case,
        best_match: f.best_match,
        fuzzy: f.fuzzy,
        file: bytes_in(&f.file),
        have_pcre2: f.have_pcre2,
    }
}

fn stats_in(s: &ffi::StatsInput) -> sig::StatsInput {
    sig::StatsInput {
        searched_files: s.searched_files,
        searched_dirs: s.searched_dirs,
        searched_indexed: s.searched_indexed,
        searched_skipped: s.searched_skipped,
        searched_changed: s.searched_changed,
        searched_added: s.searched_added,
        searched_lines: s.searched_lines,
        found_files: s.found_files,
        found_parts: s.found_parts,
        found_matches: s.found_matches,
        warnings: s.warnings,
        threads: s.threads,
        elapsed_ms: s.elapsed_ms,
        query: s.query,
        pager: s.pager,
        quiet: s.quiet,
        files_with_matches: s.files_with_matches,
        files_without_match: s.files_without_match,
        ungroup: s.ungroup,
        count: s.count,
        only_matching: s.only_matching,
        format: s.format,
        index: s.index,
        config: s.config_given.then(|| s.config.clone()),
        config_files: bytes_in(&s.config_files),
        fuzzy: s.fuzzy,
        boolean: s.boolean,
        files: s.files,
        mode: s.mode,
        decompress: s.decompress,
        zmax: s.zmax,
        min_depth: s.min_depth,
        max_depth: s.max_depth,
        dereference: s.dereference,
        no_dereference: s.no_dereference,
        devices_action: s.devices_action,
        directories_action: s.directories_action,
        glob_ignore_case: s.glob_ignore_case,
        hidden: s.hidden,
        hidden_default: s.hidden_default,
        ignore_files: bytes_in(&s.ignore_files),
        min_count: s.min_count,
        max_count: s.max_count,
        max_files: s.max_files,
        min_line: s.min_line,
        max_line: s.max_line,
        sort: s.sort_given.then(|| s.sort.clone()),
        min_size: s.min_size,
        max_size: s.max_size,
        ignore: bytes_in(&s.ignore),
        file_magic: bytes_in(&s.file_magic),
        include_fs: bytes_in(&s.include_fs),
        exclude_fs: bytes_in(&s.exclude_fs),
        all_include: bytes_in(&s.all_include),
        all_exclude: bytes_in(&s.all_exclude),
        all_include_dir: bytes_in(&s.all_include_dir),
        all_exclude_dir: bytes_in(&s.all_exclude_dir),
        include_iglob_size: s.include_iglob_size,
        exclude_iglob_size: s.exclude_iglob_size,
        include_iglob_dir_size: s.include_iglob_dir_size,
        exclude_iglob_dir_size: s.exclude_iglob_dir_size,
        crlf: s.crlf,
        pathsep: s.pathsep,
    }
}

fn cnf_compile(terms: Vec<ffi::CnfTerm>, pattern: &[u8], flags: &ffi::CnfFlags) -> ffi::CnfCompiled {
    compiled_out(sig::cnf::compile_flagged(terms_in(&terms), pattern, &flags_in(flags)))
}

fn cnf_new_pattern(terms: Vec<ffi::CnfTerm>, mask: i32, pattern: &[u8], flags: &ffi::CnfFlags) -> ffi::CnfCompiled {
    compiled_out(sig::cnf::new_pattern_flagged(terms_in(&terms), mask, pattern, &flags_in(flags)))
}

fn compiled_out(c: sig::Compiled) -> ffi::CnfCompiled {
    ffi::CnfCompiled { terms: terms_out(c.terms), enable_empty: c.enable_empty }
}

fn cnf_prune(terms: Vec<ffi::CnfTerm>, flags: &ffi::CnfFlags) -> Vec<ffi::CnfTerm> {
    terms_out(sig::cnf::prune(terms_in(&terms), &flags_in(flags)))
}

fn cnf_split(terms: Vec<ffi::CnfTerm>, flags: &ffi::CnfFlags) -> Vec<ffi::CnfTerm> {
    terms_out(sig::cnf::split(terms_in(&terms), &flags_in(flags)))
}

fn cnf_adjoin(terms: Vec<ffi::CnfTerm>, flags: &ffi::CnfFlags) -> Vec<u8> {
    sig::cnf::adjoin(&terms_in(&terms), &flags_in(flags))
}

fn cnf_first(terms: Vec<ffi::CnfTerm>) -> Vec<u8> {
    sig::cnf::first(&terms_in(&terms))
}

fn cnf_report(terms: Vec<ffi::CnfTerm>, flags: &ffi::CnfFlags) -> Vec<u8> {
    sig::cnf::report(&terms_in(&terms), &flags_in(flags))
}

fn cnf_quote(pattern: &[u8]) -> Vec<u8> {
    sig::cnf::quote(pattern)
}

fn cnf_anchor(pattern: &[u8], flags: &ffi::CnfFlags) -> ffi::CnfAnchored {
    let a = sig::cnf::anchor(pattern, &flags_in(flags));
    ffi::CnfAnchored { pattern: a.pattern, enable_empty: a.enable_empty }
}

fn core_set_color(colors: &[u8], parameter: &[u8], color: &mut [u8]) {
    color::set_color(colors, parameter, color)
}

fn core_trim(line: &[u8]) -> ffi::TrimRange {
    let r = color::trim(line);
    ffi::TrimRange { begin: r.begin, end: r.end }
}

fn stats_report(input: &ffi::StatsInput) -> Vec<u8> {
    sig::stats::report(&stats_in(input))
}

fn output_quote(data: &[u8]) -> Vec<u8> {
    output::quote(data)
}

fn output_cpp(data: &[u8]) -> Vec<u8> {
    output::cpp(data)
}

fn output_csv(data: &[u8]) -> Vec<u8> {
    output::csv(data)
}

fn output_hex(data: &[u8]) -> Vec<u8> {
    output::hex(data)
}

fn output_json(data: &[u8]) -> Vec<u8> {
    output::json(data)
}

fn output_xml(data: &[u8]) -> Vec<u8> {
    output::xml(data)
}

/// Three strings that C++ passes as one buffer, NUL-separated: the `--hyperlink` prefix, host and path, or the tree glyphs.
fn link_parts(link: &[u8]) -> (&[u8], &[u8], &[u8]) {
    let mut it = link.split(|&b| b == 0);
    let mut next = || it.next().unwrap_or_default();
    (next(), next(), next())
}

fn output_color(has_arg: bool, arg: &[u8]) -> Vec<u8> {
    output::color(&output_ffi::opts(&[], &[], &[]), has_arg, arg)
}

fn output_header_line(
    pathname: &[u8],
    partname: &[u8],
    separator: &[u8],
    link: &[u8],
    args: &ffi::OutputArgs,
    matcher: &ffi::OutputMatch,
    ctx: &ffi::OutputCtx,
) -> ffi::OutputHeaderResult {
    let (p, h, d) = link_parts(link);
    output::header_line(&output_ffi::opts(p, h, d), pathname, partname, separator, args, matcher, ctx)
}

fn output_header_file(pathname: &[u8], partname: &[u8], link: &[u8], ctx: &ffi::OutputCtx) -> ffi::OutputHeaderResult {
    let (p, h, d) = link_parts(link);
    output::header_file(&output_ffi::opts(p, h, d), pathname, partname, ctx)
}

fn output_header_tree(pathname: &[u8], partname: &[u8], link: &[u8], ctx: &ffi::OutputCtx) -> Vec<u8> {
    let (p, h, d) = link_parts(link);
    let mut tree = output_buf::tree();
    // the glyphs are copied: the guard borrows the tree state mutably for `header_tree`
    let glyphs = tree.glyphs();
    let (bar, ptr, end) = (glyphs.bar.to_vec(), glyphs.ptr.to_vec(), glyphs.end.to_vec());
    output::header_tree(
        &output_ffi::opts(p, h, d),
        pathname,
        partname,
        ctx,
        &output::TreeGlyphs { bar: &bar, ptr: &ptr, end: &end },
        &mut tree.state,
    )
}

fn output_binary_file_matches(
    pathname: &[u8],
    partname: &[u8],
    link: &[u8],
    ctx: &ffi::OutputCtx,
) -> ffi::OutputBinaryResult {
    let (p, h, d) = link_parts(link);
    output::binary_file_matches(&output_ffi::opts(p, h, d), pathname, partname, ctx)
}

fn output_format_count(format: &[u8], matches: usize, ctx: &ffi::OutputCtx) -> Vec<u8> {
    output_format::format_count(&output_ffi::format_opts(), format, matches, ctx)
}

fn output_format_match(
    format: &[u8],
    pathname: &[u8],
    partname: &[u8],
    window: &[u8],
    args: &ffi::OutputArgs,
    matcher: &ffi::OutputMatch,
    ctx: &ffi::OutputCtx,
) -> ffi::OutputFormatResult {
    output_format::format_match(&output_ffi::format_opts(), format, (pathname, partname), window, args, matcher, ctx)
}

fn output_format_invert(
    format: &[u8],
    pathname: &[u8],
    partname: &[u8],
    line: &[u8],
    args: &ffi::OutputArgs,
    ctx: &ffi::OutputCtx,
) -> ffi::OutputFormatResult {
    output_format::format_invert(&output_ffi::format_opts(), format, (pathname, partname), line, args, ctx)
}

// the Grep search seam (see SEAM.md, "Grep seam"): the opaque handles and the stub bodies of signatures::search
use signatures::search::{self as grep_search, MatcherHandle, SearchContext};

fn matcher_handle_new(kind: u8, token: usize) -> Box<MatcherHandle> {
    Box::new(MatcherHandle::new(grep_search::MatcherKind(kind), token))
}

fn matcher_find(handle: &MatcherHandle) -> bool {
    handle.find()
}

fn matcher_next_match(handle: &MatcherHandle, skip_line: bool) -> bool {
    handle.next_match(skip_line)
}

fn matcher_bol(handle: &MatcherHandle) -> usize {
    handle.bol()
}

fn matcher_eol(handle: &MatcherHandle, inclusive: bool) -> usize {
    handle.eol(inclusive)
}

fn matcher_begin(handle: &MatcherHandle) -> usize {
    handle.begin()
}

fn matcher_end(handle: &MatcherHandle) -> usize {
    handle.end()
}

fn matcher_text_bounds(handle: &MatcherHandle) -> ffi::MatcherBounds {
    let b = handle.text_bounds();
    ffi::MatcherBounds {
        begin: b.begin,
        end: b.end,
        first: b.first,
        last: b.last,
        size: b.size,
        bol: b.bol,
        eol: b.eol,
        lineno: b.lineno,
        lines: b.lines,
    }
}

fn matcher_lineno_skip(handle: &MatcherHandle, on: bool) {
    handle.lineno_skip(on)
}

fn matcher_set_shift_listener(handle: &MatcherHandle, enabled: bool) {
    handle.set_shift_listener(enabled)
}

fn matcher_shift_event(handle: &MatcherHandle, event: &ffi::MatcherShiftEvent, buf: &[u8]) {
    handle.shift_event(grep_search::ShiftEvent { len: event.len, num: event.num }, buf)
}

fn matcher_reset(handle: &MatcherHandle) {
    handle.reset()
}

fn matcher_first(handle: &MatcherHandle) -> usize {
    handle.first()
}

fn matcher_last(handle: &MatcherHandle) -> usize {
    handle.last()
}

fn matcher_size(handle: &MatcherHandle) -> usize {
    handle.size()
}

fn matcher_lineno(handle: &MatcherHandle) -> usize {
    handle.lineno()
}

fn matcher_lines(handle: &MatcherHandle) -> usize {
    handle.lines()
}

fn matcher_columno(handle: &MatcherHandle) -> usize {
    handle.columno()
}

fn matcher_at_bol(handle: &MatcherHandle) -> bool {
    handle.at_bol()
}

fn matcher_hit_end(handle: &MatcherHandle) -> bool {
    handle.hit_end()
}

fn matcher_skip(handle: &MatcherHandle, c: u8) -> bool {
    handle.skip(c)
}

fn matcher_border(handle: &MatcherHandle) -> usize {
    handle.border()
}

fn matcher_before(handle: &MatcherHandle) -> usize {
    handle.before()
}

fn matcher_after(handle: &MatcherHandle) -> usize {
    handle.after()
}

fn matcher_bef(handle: &MatcherHandle) -> usize {
    handle.bef()
}

fn matcher_aft(handle: &MatcherHandle) -> usize {
    handle.aft()
}

fn matcher_set_reserve(handle: &MatcherHandle, size: usize) {
    handle.set_reserve(size)
}

fn matcher_fuzzy_distance(handle: &MatcherHandle, max: u16) {
    handle.fuzzy_distance(max)
}

fn matcher_fuzzy_edits(handle: &MatcherHandle) -> u16 {
    handle.fuzzy_edits()
}

fn search_context_new(pathname: &[u8], cost: u16) -> Box<SearchContext> {
    Box::new(SearchContext::new(pathname, cost))
}

fn search_context_pathname(ctx: &SearchContext) -> &[u8] {
    &ctx.pathname
}

fn search_context_matched(ctx: &SearchContext) -> bool {
    ctx.matched.get()
}

fn search_setup(ctx: &SearchContext) {
    grep_search::branch::setup(ctx)
}

fn search_quiet_list(ctx: &SearchContext, matcher: &MatcherHandle) -> u8 {
    grep_search::branch::quiet_list(ctx, matcher).0
}

fn search_count(ctx: &SearchContext, matcher: &MatcherHandle) -> u8 {
    grep_search::branch::count(ctx, matcher).0
}

fn search_format(ctx: &SearchContext, matcher: &MatcherHandle) -> u8 {
    grep_search::branch::format(ctx, matcher).0
}

fn search_only_matching(ctx: &SearchContext, matcher: &MatcherHandle) -> u8 {
    grep_search::branch::only_matching(ctx, matcher).0
}

fn search_only_matching_context(ctx: &SearchContext, matcher: &MatcherHandle) -> u8 {
    grep_search::branch::only_matching_context(ctx, matcher).0
}

fn search_plain(ctx: &SearchContext, matcher: &MatcherHandle) -> u8 {
    grep_search::branch::plain(ctx, matcher).0
}

fn search_invert(ctx: &SearchContext, matcher: &MatcherHandle) -> u8 {
    grep_search::branch::invert(ctx, matcher).0
}

fn search_any_line(ctx: &SearchContext, matcher: &MatcherHandle) -> u8 {
    grep_search::branch::any_line(ctx, matcher).0
}

fn search_context(ctx: &SearchContext, matcher: &MatcherHandle) -> u8 {
    grep_search::branch::context(ctx, matcher).0
}

fn search_context_invert(ctx: &SearchContext, matcher: &MatcherHandle) -> u8 {
    grep_search::branch::context_invert(ctx, matcher).0
}

fn search_epilogue(ctx: &SearchContext, matcher: &MatcherHandle) {
    grep_search::branch::epilogue(ctx, matcher)
}