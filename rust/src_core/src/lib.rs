//! Rust side of the `src_core` component (src/cnf.cpp, src/stats.cpp; later src/ugrep.cpp and src/output.cpp).
//! One bridge module (`ffi`); all boundary code is here. The bodies are `todo!()` stubs until the Port tasks.
//!
//! Only `extern "Rust"` functions: the C++ facade calls Rust, Rust never calls the component's legacy C++.
//! State stays in C++ (see SEAM.md): everything crosses by value, as a snapshot in and a result out.

// public so the signatures that are not called yet (stubs until the Port tasks) are part of the crate interface
pub mod signatures;

use signatures as sig;

#[cxx::bridge(namespace = "ugrep_rust")]
mod ffi {
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

    extern "Rust" {
        /// Seam function `src_core.cnf_compile`.
        fn cnf_compile(terms: Vec<CnfTerm>, pattern: &[u8], flags: &CnfFlags) -> Vec<CnfTerm>;
        /// Seam function `src_core.cnf_new_pattern`.
        fn cnf_new_pattern(terms: Vec<CnfTerm>, mask: i32, pattern: &[u8], flags: &CnfFlags) -> Vec<CnfTerm>;
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

fn cnf_compile(terms: Vec<ffi::CnfTerm>, pattern: &[u8], flags: &ffi::CnfFlags) -> Vec<ffi::CnfTerm> {
    terms_out(sig::cnf::compile(terms_in(&terms), pattern, &flags_in(flags)))
}

fn cnf_new_pattern(terms: Vec<ffi::CnfTerm>, mask: i32, pattern: &[u8], flags: &ffi::CnfFlags) -> Vec<ffi::CnfTerm> {
    terms_out(sig::cnf::new_pattern(terms_in(&terms), mask, pattern, &flags_in(flags)))
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

fn stats_report(input: &ffi::StatsInput) -> Vec<u8> {
    sig::stats::report(&stats_in(input))
}
