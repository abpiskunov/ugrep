//! Types and signatures of the seam of `src_core` (src/ugrep.cpp, cnf, output, stats, mmap), fixed by the
//! Prepare seam task. Every body is `todo!()` until a Port task; the Port never edits a signature (the
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
    use super::{Anchored, Bytes, CnfFlags, Terms};

    /// `CNF::compile`: parse a `--bool` query into an operator tree, normalize to CNF, append to `terms`.
    pub fn compile(_terms: Terms, _pattern: &[u8], _flags: &CnfFlags) -> Terms {
        todo!()
    }

    /// `CNF::new_pattern`: add an ALT, ALT-NOT or `-N` pattern (`mask` is `pattern_mask`).
    pub fn new_pattern(_terms: Terms, _mask: i32, _pattern: &[u8], _flags: &CnfFlags) -> Terms {
        todo!()
    }

    /// `CNF::prune`
    pub fn prune(_terms: Terms, _flags: &CnfFlags) -> Terms {
        todo!()
    }

    /// `CNF::split`
    pub fn split(_terms: Terms, _flags: &CnfFlags) -> Terms {
        todo!()
    }

    /// `CNF::adjoin`
    pub fn adjoin(_terms: &Terms, _flags: &CnfFlags) -> Bytes {
        todo!()
    }

    /// `CNF::first`
    pub fn first(_terms: &Terms) -> Bytes {
        todo!()
    }

    /// `CNF::report`: the text the C++ writes to the `FILE*` (the facade does the `fwrite`).
    pub fn report(_terms: &Terms, _flags: &CnfFlags) -> Bytes {
        todo!()
    }

    /// `CNF::quote` (static)
    pub fn quote(_pattern: &[u8]) -> Bytes {
        todo!()
    }

    /// `CNF::anchor` (static): reads `flag_line_regexp`, `flag_word_regexp`, `flag_mode`; writes `flag_empty`.
    pub fn anchor(_pattern: &[u8], _flags: &CnfFlags) -> Anchored {
        todo!()
    }
}

/// Facade function of `Stats` (src/stats.cpp).
pub mod stats {
    use super::{Bytes, StatsInput};

    /// `Stats::report`: the text the C++ writes to the `FILE*` (the facade does the `fwrite`).
    pub fn report(_input: &StatsInput) -> Bytes {
        todo!()
    }
}

/// Entry functions of the component that are tapped in the C++ but are not switchable yet: their bodies are the
/// `Grep`/`Output`/matcher machinery that stays C++ (RE-flex, `Output` and `MMap` are retained). They are listed
/// so a later Prepare seam follow-up can turn them into facades without changing the types here.
pub mod entry {
    use super::{Bytes, CoreError};

    /// Result of `ugrep_find_text_preview`: `lineno` and `num` are out-parameters, `text` the found lines.
    #[derive(Clone, Debug, Default)]
    pub struct Preview {
        pub lineno: usize,
        pub num: usize,
        pub text: Vec<Bytes>,
    }

    /// `ugrep()`: search the specified files, directories and standard input. Reads all of `flag_*` and `Static`.
    pub fn ugrep() -> Result<(), CoreError> {
        todo!()
    }

    /// `ugrep_find_text_preview(filename, partname, from_lineno, max, lineno, num, text)`
    pub fn ugrep_find_text_preview(
        _filename: &[u8],
        _partname: &[u8],
        _from_lineno: usize,
        _max: usize,
        _lineno: usize,
        _num: usize,
    ) -> Result<Preview, CoreError> {
        todo!()
    }

    /// `ugrep_extract(filename, partname, FILE *output)`; the `FILE*` is an opaque handle owned by the caller.
    pub fn ugrep_extract(_filename: &[u8], _partname: &[u8], _output: *mut core::ffi::c_void) -> Result<(), CoreError> {
        todo!()
    }

    /// `terminal()`: check TTY availability and set the colors (writes `flag_color_term`, `flag_tty_term`, `color_*`).
    pub fn terminal() {
        todo!()
    }

    /// `set_terminal_hyperlink()`
    pub fn set_terminal_hyperlink() {
        todo!()
    }
}
