//! Types and signatures of the seam of `src_core` (src/ugrep.cpp, cnf, output, stats, mmap), fixed by the
//! Prepare seam task. Every body is `panic!` until a Port task; the Port never edits a signature (the
//! signature change protocol of the plan applies). The replay harness includes this file alone, so it names
//! no `cxx` type: `lib.rs` converts between the bridge structs and the types here.
//!
//! Ownership decisions are in `SEAM.md`. The short form:
//! - `flag_*`, `Static::*`, `Stats::*` counters, the `CNF` terms, `Output`, `MMap`, `Grep` stay owned by C++.
//! - Rust sees the flags a seam function reads as a typed snapshot taken by the C++ facade at the crossing
//!   (`CnfFlags`, `StatsInput`) and never writes a flag; a flag a function writes comes back in its result.
//! - The CNF terms cross by value (copy in, copy out): C++ keeps the `CNF` object.

/// Bytes of a C string without its NUL, or of a `std::string`. No encoding is assumed (UTF-8 is not validated).
pub type Bytes = Vec<u8>;

#[path = "cfmt.rs"]
mod cfmt;
#[path = "cnf_impl.rs"]
mod cnf_impl;
#[path = "stats_impl.rs"]
mod stats_impl;

/// A pattern in the CNF: `None` is the undefined (NULL smart pointer) pattern, `Some` a possibly empty string.
pub type Pattern = Option<Bytes>;
/// A term in the CNF: the first is the OR pattern (with alternations) or `None`, the rest are ALT-NOT patterns.
pub type Term = Vec<Pattern>;
/// An AND-list of ALT-term lists (`CNF::Terms`).
pub type Terms = Vec<Term>;

/// `Mode` of `flag.hpp`: `-E` (default), `-G`, `-F`, `-P`. A transparent newtype so the C++ values map 1:1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode(pub u8);

impl Mode {
    pub const UNSP: Mode = Mode(0);
    pub const ERE: Mode = Mode(1);
    pub const BRE: Mode = Mode(2);
    pub const FIXED: Mode = Mode(3);
    pub const PERL: Mode = Mode(4);
}

/// `Action` of `flag.hpp` (`-D` and `-d`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Action(pub u8);

impl Action {
    pub const UNSP: Action = Action(0);
    pub const SKIP: Action = Action(1);
    pub const READ: Action = Action(2);
    pub const RECURSE: Action = Action(3);
}

/// `CNF::PATTERN` mask values (`-e`, `-N`, `--not`, and the new AND-term marker).
pub mod pattern_mask {
    pub const ALT: i32 = 0;
    pub const NEG: i32 = 1;
    pub const NOT: i32 = 2;
    pub const AND: i32 = 4;
}

/// The `flag_*` globals that the CNF code reads, as they are at the crossing. Read only.
#[derive(Clone, Debug, Default)]
pub struct CnfFlags {
    pub mode: u8,
    pub line_regexp: bool,
    pub word_regexp: bool,
    pub boolean: bool,
    pub files: bool,
    pub invert_match: bool,
    pub ignore_case: bool,
    pub best_match: bool,
    pub fuzzy: usize,
    /// `flag_file`: the -f FILE names, in order.
    pub file: Vec<Bytes>,
    /// `HAVE_PCRE2` at the time the C++ was compiled (changes how `-w` is anchored in perl mode).
    pub have_pcre2: bool,
}

/// Result of `CNF::anchor`: the new pattern and the value of `flag_empty` afterwards. The C++ function writes
/// that global (`-Y` is enabled when the pattern starts with `^` or ends with `$`); the facade stores it back.
#[derive(Clone, Debug, Default)]
pub struct Anchored {
    pub pattern: Bytes,
    pub enable_empty: bool,
}

/// Result of `CNF::compile` / `CNF::new_pattern`: the new terms and whether the C++ would have set `flag_empty`
/// (an `anchor` call inside saw `^` first or `$` last without -x/-w).
#[derive(Clone, Debug, Default)]
pub struct Compiled {
    pub terms: Terms,
    pub enable_empty: bool,
}

/// Counters and flags read by `Stats::report`, taken by the facade when the report is requested.
#[derive(Clone, Debug, Default)]
pub struct StatsInput {
    pub searched_files: usize,
    pub searched_dirs: usize,
    pub searched_indexed: usize,
    pub searched_skipped: usize,
    pub searched_changed: usize,
    pub searched_added: usize,
    pub searched_lines: usize,
    pub found_files: usize,
    pub found_parts: usize,
    pub found_matches: usize,
    pub warnings: usize,
    pub threads: usize,
    /// `reflex::timer_elapsed(timer)` in milliseconds.
    pub elapsed_ms: f64,
    pub query: bool,
    /// `flag_pager != NULL`
    pub pager: bool,
    pub quiet: bool,
    pub files_with_matches: bool,
    pub files_without_match: bool,
    pub ungroup: bool,
    pub count: bool,
    pub only_matching: bool,
    /// `flag_format != NULL`
    pub format: bool,
    /// `flag_index != NULL`
    pub index: bool,
    /// `flag_config != NULL` and its value
    pub config: Option<Bytes>,
    pub config_files: Vec<Bytes>,
    pub fuzzy: usize,
    pub boolean: bool,
    pub files: bool,
    pub mode: u8,
    pub decompress: bool,
    pub zmax: usize,
    pub min_depth: usize,
    pub max_depth: usize,
    pub dereference: bool,
    pub no_dereference: bool,
    pub devices_action: u8,
    pub directories_action: u8,
    pub glob_ignore_case: bool,
    pub hidden: bool,
    /// `WITH_HIDDEN` at the time the C++ was compiled (the "(default)" marker moves).
    pub hidden_default: bool,
    pub ignore_files: Vec<Bytes>,
    pub min_count: usize,
    pub max_count: usize,
    pub max_files: usize,
    pub min_line: usize,
    pub max_line: usize,
    /// `flag_sort != NULL` and its value
    pub sort: Option<Bytes>,
    pub min_size: usize,
    pub max_size: usize,
    /// `Stats::ignore`: the .gitignore files encountered
    pub ignore: Vec<Bytes>,
    pub file_magic: Vec<Bytes>,
    pub include_fs: Vec<Bytes>,
    pub exclude_fs: Vec<Bytes>,
    pub all_include: Vec<Bytes>,
    pub all_exclude: Vec<Bytes>,
    pub all_include_dir: Vec<Bytes>,
    pub all_exclude_dir: Vec<Bytes>,
    pub include_iglob_size: usize,
    pub exclude_iglob_size: usize,
    pub include_iglob_dir_size: usize,
    pub exclude_iglob_dir_size: usize,
    /// `NEWLINESTR` is `"\r\n"` (OS_WIN) or `"\n"`.
    pub crlf: bool,
    /// `PATHSEPCHR`
    pub pathsep: u8,
}

/// Error kinds of the seam, one per crossing type (see SEAM.md). The facade functions below are infallible in the
/// C++ (they do not throw); the kinds exist for the entry functions that are not switchable yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreError {
    /// `abort(message)`: print and `exit(EXIT_ERROR)` in C++. Never returned across the seam, the process ends.
    Abort(Bytes),
    /// `error(message, arg)`: like `Abort` but with `strerror(errno)`.
    Errno(Bytes),
    /// `ugrep_find_text_preview`/`ugrep_extract` may throw `std::runtime_error` or `std::bad_alloc`; the facade
    /// catches nothing, so the exception propagates through the Rust frame only if the Rust never unwinds:
    /// with `panic = "abort"` the Rust side returns this value and the facade rethrows.
    Exception(Bytes),
}

/// Facade functions of `CNF` (src/cnf.cpp): pure functions over the terms, the pattern and the flag snapshot.
pub mod cnf {
    use super::{Anchored, Bytes, CnfFlags, Compiled, Terms};

    /// `CNF::compile`: parse a `--bool` query into an operator tree, normalize to CNF, append to `terms`.
    pub fn compile(terms: Terms, pattern: &[u8], flags: &CnfFlags) -> Terms {
        super::cnf_impl::compile(terms, pattern, flags)
    }

    /// `CNF::compile` with the `flag_empty` side effect the facade stores back.
    pub fn compile_flagged(terms: Terms, pattern: &[u8], flags: &CnfFlags) -> Compiled {
        super::cnf_impl::compile_flagged(terms, pattern, flags)
    }

    /// `CNF::new_pattern`: add an ALT, ALT-NOT or `-N` pattern (`mask` is `pattern_mask`).
    pub fn new_pattern(terms: Terms, mask: i32, pattern: &[u8], flags: &CnfFlags) -> Terms {
        super::cnf_impl::new_pattern(terms, mask, pattern, flags)
    }

    /// `CNF::new_pattern` with the `flag_empty` side effect the facade stores back.
    pub fn new_pattern_flagged(terms: Terms, mask: i32, pattern: &[u8], flags: &CnfFlags) -> Compiled {
        super::cnf_impl::new_pattern_flagged(terms, mask, pattern, flags)
    }

    /// `CNF::prune`
    pub fn prune(terms: Terms, flags: &CnfFlags) -> Terms {
        super::cnf_impl::prune(terms, flags)
    }

    /// `CNF::split`
    pub fn split(terms: Terms, flags: &CnfFlags) -> Terms {
        super::cnf_impl::split(terms, flags)
    }

    /// `CNF::adjoin`
    pub fn adjoin(terms: &Terms, flags: &CnfFlags) -> Bytes {
        super::cnf_impl::adjoin(terms, flags)
    }

    /// `CNF::first`
    pub fn first(terms: &Terms) -> Bytes {
        super::cnf_impl::first(terms)
    }

    /// `CNF::report`: the text the C++ writes to the `FILE*` (the facade does the `fwrite`).
    pub fn report(terms: &Terms, flags: &CnfFlags) -> Bytes {
        super::cnf_impl::report(terms, flags)
    }

    /// `CNF::quote` (static)
    pub fn quote(pattern: &[u8]) -> Bytes {
        super::cnf_impl::quote(pattern)
    }

    /// `CNF::anchor` (static): reads `flag_line_regexp`, `flag_word_regexp`, `flag_mode`; writes `flag_empty`.
    pub fn anchor(pattern: &[u8], flags: &CnfFlags) -> Anchored {
        super::cnf_impl::anchor(pattern, flags)
    }
}

/// Part of a line that `trim` keeps: `line[begin..end]`, white space (`isspace`, C locale) removed from both ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrimRange {
    pub begin: usize,
    pub end: usize,
}

/// Result of `find_text_preview`: the `lineno`, `num` and `text` out-parameters of `ugrep_find_text_preview`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPreview {
    pub lineno: usize,
    pub num: usize,
    pub text: Vec<Vec<u8>>,
}

/// Seam functions of the terminal group and `init` (src/ugrep.cpp, Prepare seam "init/terminal/hyperlink/set_color/trim").
/// They work on `ugrep_settings` (S1) directly; the C++ they need is listed in `SEAM.md` (callbacks). Stubs until the Ports.
pub mod ui {
    use std::os::raw::c_char;

    /// `init(argc, argv)`: parses options into `ugrep_settings` and the opaque C++ containers.
    ///
    /// `argv` is the process argument vector (`argc` entries, valid NUL-terminated strings that live for the whole
    /// process: the settings keep pointers into them, and `trim_pathname_arg` modifies them in place).
    pub fn init(argv: &mut [*const c_char]) {
        let _ = argv;
        panic!("not ported yet: init")
    }
}

/// Facade function of `Stats` (src/stats.cpp).
pub mod stats {
    use super::{Bytes, StatsInput};

    /// `Stats::report`: the text the C++ writes to the `FILE*` (the facade does the `fwrite`).
    pub fn report(input: &StatsInput) -> Bytes {
        super::stats_impl::report(input)
    }
}
