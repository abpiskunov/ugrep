//! Command-line option parsing of `ugrep` (the port of `options()`, `option_*`, `set_depth*`, `getoptarg`,
//! `getloptarg`, `strarg`, `strtonum`, `strtopos`, `strtopos2`, `strtosize`, `strtofuzzy` and
//! `trim_pathname_arg` of src/ugrep.cpp).
//!
//! The option state is the Rust-owned `Settings` (read and written through `settings_ffi::cfg`). What stays C++
//! (`std::string` and container options, the pattern list, `Static::arg_*`, `usage`, `help`, `version`, the
//! screen width) is reached through the `Host` trait, which `ffi/options.rs` implements with C callbacks.
//! Argument strings are bytes that are followed by a NUL in memory (`Arg`), so a suffix of one can be stored
//! as a `const char*` in the settings.

use crate::settings_ffi::cfg;
use std::cell::Cell;
use std::os::raw::c_char;

/// A NUL-terminated string that lives for the whole process, without its NUL.
pub type Arg = &'static [u8];

/// A string literal as an `Arg`.
macro_rules! lit {
    ($s:literal) => {{
        const S: &str = concat!($s, "\0");
        const B: &[u8] = S.as_bytes();
        const R: &[u8] = B.split_at(B.len() - 1).0;
        R
    }};
}

macro_rules! set {
    ($f:ident = $v:expr) => {{
        let v = $v;
        cfg(|s| s.$f = v)
    }};
}

macro_rules! get {
    ($f:ident) => {
        cfg(|s| s.$f)
    };
}

/// The `std::vector<std::string>` options that stay C++ (ids shared with src/settings.cpp).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Vector {
    File = 0,
    FileType,
    FileExtension,
    FileMagic,
    FilterMagicLabel,
    From,
    Glob,
    Iglob,
    Include,
    IncludeDir,
    IncludeFrom,
    IncludeFs,
    Exclude,
    ExcludeDir,
    ExcludeFrom,
    ExcludeFs,
}

/// `CNF::PATTERN` mask values.
pub const PATTERN_ALT: i32 = 0;
pub const PATTERN_NEG: i32 = 1;
pub const PATTERN_NOT: i32 = 2;
pub const PATTERN_AND: i32 = 4;

const FUZZY_INS: usize = 0x1000;
const FUZZY_DEL: usize = 0x2000;
const FUZZY_SUB: usize = 0x4000;

/// Build-dependent constants and the C++ strings that options compare against or store.
#[derive(Clone, Copy)]
pub struct Consts {
    pub never: Arg,
    pub always: Arg,
    pub auto: Arg,
    pub default_ignore_file: Arg,
    pub default_tag: Arg,
    pub default_tabs: usize,
    pub max_mmap_size: usize,
    /// `OS_WIN`: `/` starts an option as well as `-`
    pub os_win: bool,
}

/// What option parsing needs from the C++ side. Every function may return: `usage` does not exit while a
/// configuration file is read.
pub trait Host {
    fn usage(&self, message: Arg, arg: Option<Arg>, valid: Option<Arg>);
    fn help(&self, what: Option<Arg>);
    fn version(&self);
    fn screen_width(&self) -> usize;
    fn pattern_push(&self, kind: i32, arg: Arg);
    fn pattern_empty(&self) -> bool;
    fn vec_push(&self, vector: Vector, arg: Arg);
    fn vec_clear(&self, vector: Vector);
    fn vec_empty(&self, vector: Vector) -> bool;
    fn filter_append(&self, arg: Arg);
    fn filter_clear(&self);
    fn ignore_files_insert(&self, arg: Arg);
    fn ignore_files_clear(&self);
    fn arg_pattern_set(&self) -> bool;
    fn set_arg_pattern(&self, arg: Arg);
    fn push_arg_file(&self, arg: Arg);
}

fn ch(a: &[u8], k: usize) -> u8 {
    a.get(k).copied().unwrap_or(0)
}

fn eq(a: &[u8], s: &str) -> bool {
    a == s.as_bytes()
}

fn pre(a: &[u8], s: &str) -> bool {
    a.starts_with(s.as_bytes())
}

fn cs(a: Arg) -> *const c_char {
    a.as_ptr().cast()
}

/// C `strtoull(s, &rest, 10)` in the C locale: the value and the number of bytes consumed (0 when there is no
/// conversion, then `rest` is the start of the string).
pub fn strtoull(s: &[u8]) -> (u64, usize) {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | 9..=13) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let digits = i;
    let mut v: u64 = 0;
    let mut over = false;
    while i < s.len() && s[i].is_ascii_digit() {
        match v
            .checked_mul(10)
            .and_then(|x| x.checked_add(u64::from(s[i] - b'0')))
        {
            Some(x) => v = x,
            None => over = true,
        }
        i += 1;
    }
    if i == digits {
        return (0, 0);
    }
    if over {
        (u64::MAX, i)
    } else if neg {
        (v.wrapping_neg(), i)
    } else {
        (v, i)
    }
}

/// Positions of the two numbers set by `-K` / `--range` (line) and `-m` (count).
#[derive(Clone, Copy)]
enum Pair {
    Line,
    Count,
}

/// `trim_pathname_arg`: the index at which the trailing path separators of `arg` start, after the drive prefix
/// and path; `None` when nothing is trimmed. `arg` is the string without its NUL.
pub fn trim_pathname_start(arg: &[u8], sep: u8) -> Option<usize> {
    let base = arg.iter().position(|&c| c == b':').map_or(0, |p| p + 1);
    let path = &arg[base..];
    let mut len = path.len();
    let mut cut = None;
    while len > 1 {
        len -= 1;
        if path[len] == sep {
            cut = Some(base + len);
        } else {
            break;
        }
    }
    cut
}

/// Parse `argv` (`argv[0]` is skipped) into the settings and the C++ option containers.
pub fn options(host: &dyn Host, consts: Consts, argv: &[Arg]) {
    let parser = Parser {
        host,
        consts,
        argv,
        i: Cell::new(0),
    };
    parser.run();
}

struct Parser<'a> {
    host: &'a dyn Host,
    consts: Consts,
    argv: &'a [Arg],
    i: Cell<usize>,
}

impl Parser<'_> {
    fn usage(&self, message: Arg, arg: Option<Arg>) {
        self.host.usage(message, arg, None);
    }

    /// `getoptarg`: short option argument
    fn getoptarg(&self, arg: Arg) -> Arg {
        let mut a = &arg[1..];
        if ch(a, 0) == b'=' {
            a = &a[1..];
        }
        if !a.is_empty() {
            return a;
        }
        self.i.set(self.i.get() + 1);
        if self.i.get() < self.argv.len() {
            return self.argv[self.i.get()];
        }
        lit!("")
    }

    /// `getloptarg`: long option argument after `=`
    fn getloptarg(&self, arg: Arg) -> Arg {
        if !arg.is_empty() {
            return arg;
        }
        self.i.set(self.i.get() + 1);
        if self.i.get() < self.argv.len() {
            return self.argv[self.i.get()];
        }
        lit!("")
    }

    /// `strarg`: the string lives as long as the process
    fn strarg(&self, string: Arg) -> Arg {
        let mut copy = Vec::with_capacity(string.len() + 1);
        copy.extend_from_slice(string);
        copy.push(0);
        let copy: &'static [u8] = Box::leak(copy.into_boxed_slice());
        &copy[..copy.len() - 1]
    }

    fn strtonum(&self, string: Arg, message: Arg) -> usize {
        let (size, n) = strtoull(string);
        if ch(string, n) != 0 {
            self.host.usage(message, Some(string), None);
        }
        size as usize
    }

    fn strtopos(&self, string: Arg, message: Arg) -> usize {
        let size = self.strtonum(string, message);
        if size == 0 {
            self.host.usage(message, Some(string), None);
        }
        size
    }

    /// The range of `strtopos2`, `None` when the string is empty
    fn strtopos2(&self, string: Arg, message: Arg) -> Option<(usize, usize)> {
        if string.is_empty() {
            self.host.usage(message, Some(string), None);
            return None;
        }
        let mut rest = 0;
        let mut min = 0;
        if string[0] != b',' {
            let (v, n) = strtoull(string);
            min = v as usize;
            rest = n;
        }
        let max;
        if ch(string, rest) == b',' {
            let (v, n) = strtoull(&string[rest + 1..]);
            max = v as usize;
            rest += 1 + n;
        } else {
            max = min;
            min = 0;
        }
        if ch(string, rest) != 0 || (max > 0 && min > max) {
            self.host.usage(message, Some(string), None);
        }
        Some((min, max))
    }

    fn strtopos2_flags(&self, string: Arg, pair: Pair, message: Arg) {
        if let Some((min, max)) = self.strtopos2(string, message) {
            match pair {
                Pair::Line => {
                    set!(flag_min_line = min);
                    set!(flag_max_line = max);
                }
                Pair::Count => {
                    set!(flag_min_count = min);
                    set!(flag_max_count = max);
                }
            }
        }
    }

    fn strtosize(&self, string: Arg, message: Arg) -> usize {
        let (v, n) = strtoull(string);
        let mut size = v;
        if n == 0 || size == 0 {
            self.host.usage(message, Some(string), None);
        }
        let mut rest = n;
        match ch(string, rest) {
            b'k' | b'K' => {
                size = size.wrapping_mul(1024);
                rest += 1;
            }
            b'm' | b'M' => {
                size = size.wrapping_mul(1024 * 1024);
                rest += 1;
            }
            b'g' | b'G' => {
                size = size.wrapping_mul(1024 * 1024 * 1024);
                rest += 1;
            }
            b't' | b'T' if usize::BITS >= 64 => {
                size = size.wrapping_mul(1024 * 1024 * 1024 * 1024);
                rest += 1;
            }
            _ => {}
        }
        let c = ch(string, rest);
        if c != b'b' && c != b'B' && c != 0 {
            self.host.usage(message, Some(string), None);
        }
        size as usize
    }

    fn strtofuzzy(&self, string: Arg, message: Arg) -> usize {
        let mut s = string;
        let mut flags = 0;
        let mut max: usize = 1;
        while !s.is_empty() {
            match s[0] {
                b'b' => {
                    if !pre(s, "best") {
                        self.host.usage(message, Some(s), None);
                    }
                    set!(flag_best_match = true);
                    s = &s[4.min(s.len())..];
                }
                b'+' => {
                    flags |= FUZZY_INS;
                    s = &s[1..];
                }
                b'-' => {
                    flags |= FUZZY_DEL;
                    s = &s[1..];
                }
                b'~' => {
                    flags |= FUZZY_SUB;
                    s = &s[1..];
                }
                _ => {
                    let (v, n) = strtoull(s);
                    max = v as usize;
                    if max == 0 || max > 0xff || ch(s, n) != 0 {
                        self.host.usage(message, Some(s), None);
                    }
                    s = &s[n..];
                }
            }
        }
        max | flags
    }

    /// `set_depth`: the index in `arg` of the last character consumed plus one (`range - arg`)
    fn set_depth(&self, arg: Arg) -> usize {
        let mut idx;
        if get!(flag_max_depth) > 0 {
            if get!(flag_min_depth) == 0 {
                set!(flag_min_depth = get!(flag_max_depth));
            }
            let (v, n) = strtoull(arg);
            set!(flag_max_depth = v as usize);
            idx = n;
        } else {
            let (v, n) = strtoull(arg);
            set!(flag_max_depth = v as usize);
            idx = n;
            if matches!(ch(arg, idx), b'-' | b',') {
                set!(flag_min_depth = v as usize);
                let (v, n) = strtoull(&arg[idx + 1..]);
                set!(flag_max_depth = v as usize);
                idx += 1 + n;
            }
        }
        if get!(flag_min_depth) > get!(flag_max_depth) {
            self.host.usage(lit!("invalid argument -"), Some(arg), None);
        }
        idx
    }

    fn set_depth_long(&self, arg: Arg) {
        let idx = self.set_depth(arg);
        if ch(arg, idx) != 0 {
            self.host
                .usage(lit!("invalid argument --depth="), Some(arg), None);
        }
    }

    fn help_arg(&self, arg: Arg) {
        let what = if !arg.is_empty() {
            Some(arg)
        } else {
            self.i.set(self.i.get() + 1);
            if self.i.get() < self.argv.len() {
                Some(self.argv[self.i.get()])
            } else {
                None
            }
        };
        self.host.help(what);
    }

    fn pattern_kind(&self) -> i32 {
        if get!(flag_not) {
            PATTERN_NOT
        } else {
            PATTERN_ALT
        }
    }

    /// `-e PATTERN` and `-N PATTERN`
    fn option_regexp(&self, arg: Arg, is_neg: bool) {
        let kind = self.pattern_kind() | if is_neg { PATTERN_NEG } else { PATTERN_ALT };
        self.host.pattern_push(kind, arg);
    }

    fn next_is_pattern(&self) -> bool {
        self.i.get() + 1 < self.argv.len() && ch(self.argv[self.i.get() + 1], 0) != b'-'
    }

    fn missing_not(&self) {
        if get!(flag_not) {
            self.usage(lit!("missing PATTERN for --not"), None);
        }
    }

    /// `--and [PATTERN]`
    fn option_and_next(&self) {
        self.missing_not();
        self.host.pattern_push(PATTERN_AND, lit!(""));
        if self.next_is_pattern() {
            self.i.set(self.i.get() + 1);
            let kind = self.pattern_kind();
            self.host.pattern_push(kind, self.argv[self.i.get()]);
        }
    }

    /// `--and=PATTERN`
    fn option_and(&self, arg: Arg) {
        self.missing_not();
        self.host.pattern_push(PATTERN_AND, lit!(""));
        let kind = self.pattern_kind();
        self.host.pattern_push(kind, arg);
    }

    /// `--andnot [PATTERN]`
    fn option_andnot_next(&self) {
        self.missing_not();
        self.host.pattern_push(PATTERN_AND, lit!(""));
        set!(flag_not = true);
        if self.next_is_pattern() {
            self.i.set(self.i.get() + 1);
            self.host.pattern_push(PATTERN_NOT, self.argv[self.i.get()]);
            set!(flag_not = false);
        }
    }

    /// `--andnot=PATTERN`
    fn option_andnot(&self, arg: Arg) {
        self.missing_not();
        self.host.pattern_push(PATTERN_AND, lit!(""));
        self.host.pattern_push(PATTERN_NOT, arg);
    }

    /// `--not [PATTERN]`
    fn option_not_next(&self) {
        set!(flag_not = !get!(flag_not));
        if self.next_is_pattern() {
            self.i.set(self.i.get() + 1);
            let kind = self.pattern_kind();
            self.host.pattern_push(kind, self.argv[self.i.get()]);
            set!(flag_not = false);
        }
    }

    /// `--not=PATTERN`
    fn option_not(&self, arg: Arg) {
        set!(flag_not = !get!(flag_not));
        let kind = self.pattern_kind();
        self.host.pattern_push(kind, arg);
        set!(flag_not = false);
    }

    /// `--all`: cancel all previously specified file and directory search restrictions
    fn option_all_files(&self) {
        for v in [
            Vector::Glob,
            Vector::Iglob,
            Vector::Exclude,
            Vector::ExcludeDir,
            Vector::ExcludeFrom,
            Vector::Include,
            Vector::IncludeDir,
            Vector::IncludeFrom,
            Vector::FileType,
            Vector::FileExtension,
            Vector::FileMagic,
        ] {
            self.host.vec_clear(v);
        }
        self.host.ignore_files_clear();
        if crate::settings_ffi::binary_files_is(b"without-match") {
            set!(flag_binary_files = cs(lit!("binary")));
        }
    }

    fn run(&self) {
        let argc = self.argv.len();
        let mut options = true;
        self.i.set(1);
        while self.i.get() < argc {
            let mut arg: Arg = self.argv[self.i.get()];
            if (ch(arg, 0) == b'-' || (self.consts.os_win && ch(arg, 0) == b'/'))
                && ch(arg, 1) != 0
                && options
            {
                let mut is_grouped = true;
                while is_grouped {
                    arg = &arg[1..];
                    if arg.is_empty() {
                        break;
                    }
                    match arg[0] {
                        b'-' => {
                            is_grouped = false;
                            arg = &arg[1..];
                            if arg.is_empty() {
                                options = false;
                                continue;
                            }
                            self.long_option(arg);
                        }
                        b'A' => {
                            let v = self.getoptarg(arg);
                            let v = self.strtonum(v, lit!("invalid argument -A="));
                            set!(flag_after_context = v);
                            is_grouped = false;
                        }
                        b'a' => set!(flag_binary_files = cs(lit!("text"))),
                        b'B' => {
                            let v = self.getoptarg(arg);
                            let v = self.strtonum(v, lit!("invalid argument -B="));
                            set!(flag_before_context = v);
                            is_grouped = false;
                        }
                        b'b' => set!(flag_byte_offset = 1),
                        b'C' => {
                            let v = self.getoptarg(arg);
                            let v = self.strtonum(v, lit!("invalid argument -C="));
                            set!(flag_after_context = v);
                            set!(flag_before_context = v);
                            is_grouped = false;
                        }
                        b'c' => set!(flag_count = true),
                        b'D' => {
                            let v = self.getoptarg(arg);
                            set!(flag_devices = cs(v));
                            is_grouped = false;
                        }
                        b'd' => {
                            let v = self.getoptarg(arg);
                            set!(flag_directories = cs(v));
                            is_grouped = false;
                        }
                        b'E' => set!(flag_mode = 1),
                        b'e' => {
                            let v = self.getoptarg(arg);
                            self.option_regexp(v, false);
                            is_grouped = false;
                        }
                        b'F' => set!(flag_mode = 3),
                        b'f' => {
                            let v = self.getoptarg(arg);
                            self.host.vec_push(Vector::File, v);
                            is_grouped = false;
                        }
                        b'G' => set!(flag_mode = 2),
                        b'g' => {
                            let v = self.getoptarg(arg);
                            self.host.vec_push(Vector::Glob, v);
                            is_grouped = false;
                        }
                        b'H' => set!(flag_with_filename = true),
                        b'h' => set!(flag_no_filename = true),
                        b'I' => set!(flag_binary_files = cs(lit!("without-match"))),
                        b'i' => set!(flag_ignore_case = 1),
                        b'J' => {
                            let v = self.getoptarg(arg);
                            let v = self.strtonum(v, lit!("invalid argument -J="));
                            set!(flag_jobs = v);
                            is_grouped = false;
                        }
                        b'j' => set!(flag_smart_case = 1),
                        b'K' => {
                            let v = self.getoptarg(arg);
                            self.strtopos2_flags(v, Pair::Line, lit!("invalid argument -K="));
                            is_grouped = false;
                        }
                        b'k' => set!(flag_column_number = 1),
                        b'L' => set!(flag_files_without_match = true),
                        b'l' => set!(flag_files_with_matches = true),
                        b'M' => {
                            let v = self.getoptarg(arg);
                            self.host.vec_push(Vector::FileMagic, v);
                            is_grouped = false;
                        }
                        b'm' => {
                            let v = self.getoptarg(arg);
                            self.strtopos2_flags(v, Pair::Count, lit!("invalid argument -m="));
                            is_grouped = false;
                        }
                        b'N' => {
                            let v = self.getoptarg(arg);
                            self.option_regexp(v, true);
                            is_grouped = false;
                        }
                        b'n' => set!(flag_line_number = 1),
                        b'O' => {
                            let v = self.getoptarg(arg);
                            self.host.vec_push(Vector::FileExtension, v);
                            is_grouped = false;
                        }
                        b'o' => set!(flag_only_matching = true),
                        b'P' => set!(flag_mode = 4),
                        b'p' => set!(flag_no_dereference = true),
                        b'Q' => {
                            if ch(arg, 1) == b'=' {
                                let v = self.strtonum(&arg[2..], lit!("invalid argument -Q="));
                                set!(flag_delay = v);
                                set!(flag_query = true);
                                is_grouped = false;
                            } else {
                                set!(flag_query = true);
                            }
                        }
                        b'q' => {
                            set!(flag_quiet = true);
                            set!(flag_no_messages = true);
                        }
                        b'R' => set!(flag_directories = cs(lit!("dereference-recurse"))),
                        b'r' => set!(flag_directories = cs(lit!("recurse"))),
                        b'S' => set!(flag_dereference_files = true),
                        b's' => set!(flag_no_messages = true),
                        b'T' => set!(flag_initial_tab = 1),
                        b't' => {
                            let v = self.getoptarg(arg);
                            self.host.vec_push(Vector::FileType, v);
                            is_grouped = false;
                        }
                        b'U' => set!(flag_binary = 1),
                        b'u' => set!(flag_ungroup = 1),
                        b'V' => self.host.version(),
                        b'v' => set!(flag_invert_match = true),
                        b'W' => set!(flag_binary_files = cs(lit!("with-hex"))),
                        b'w' => set!(flag_word_regexp = true),
                        b'X' => set!(flag_binary_files = cs(lit!("hex"))),
                        b'x' => set!(flag_line_regexp = true),
                        b'Y' => set!(flag_empty = 1),
                        b'y' => set!(flag_any_line = true),
                        b'Z' => {
                            if get!(flag_grep) {
                                set!(flag_null = true);
                            } else {
                                let next = &arg[1..];
                                let c = ch(next, 0);
                                // note: the C++ strchr("+-~", c) also matches the terminating NUL
                                if c == b'='
                                    || pre(next, "best")
                                    || c.is_ascii_digit()
                                    || c == 0
                                    || matches!(c, b'+' | b'-' | b'~')
                                {
                                    let s = if c == b'=' { &next[1..] } else { next };
                                    let v = self.strtofuzzy(s, lit!("invalid argument -Z="));
                                    set!(flag_fuzzy = v);
                                    is_grouped = false;
                                } else {
                                    set!(flag_fuzzy = 1);
                                }
                            }
                        }
                        b'z' => {
                            if get!(flag_grep) {
                                set!(flag_null_data = true);
                            } else {
                                set!(flag_decompress = true);
                            }
                        }
                        b'0' => {
                            set!(flag_null_data = get!(flag_null));
                            set!(flag_null = !get!(flag_null));
                        }
                        b'1'..=b'9' => {
                            let idx = self.set_depth(arg);
                            arg = &arg[idx.saturating_sub(1)..];
                        }
                        b'?' => self.help_arg(&arg[1..]),
                        b'%' => {
                            if get!(flag_bool) {
                                set!(flag_files = true);
                            }
                            set!(flag_bool = true);
                        }
                        b'^' => set!(flag_tree = 1),
                        b'+' => set!(flag_heading = 1),
                        b'.' => set!(flag_hidden = true),
                        b'@' => self.option_all_files(),
                        _ => self.usage(lit!("invalid option -"), Some(arg)),
                    }
                }
            } else if eq(arg, "-") {
                set!(flag_stdin = true);
            } else if !self.host.arg_pattern_set()
                && !get!(flag_match)
                && !get!(flag_not)
                && self.host.pattern_empty()
                && self.host.vec_empty(Vector::File)
            {
                self.host.set_arg_pattern(arg);
            } else {
                self.host.push_arg_file(arg);
            }
            self.i.set(self.i.get() + 1);
        }
        if get!(flag_not) {
            self.usage(lit!("missing PATTERN for --not"), None);
        }
    }

    /// the option after `--`, `arg` is its name and value
    fn long_option(&self, arg: Arg) {
        match arg[0] {
            b'-' => {}
            b'a' => {
                if eq(arg, "after-context") {
                    let v = self.strtonum(
                        self.getloptarg(lit!("")),
                        lit!("invalid argument --after-context="),
                    );
                    set!(flag_after_context = v);
                } else if pre(arg, "after-context=") {
                    let v = self.strtonum(
                        self.getloptarg(&arg[14..]),
                        lit!("invalid argument --after-context="),
                    );
                    set!(flag_after_context = v);
                } else if eq(arg, "all") {
                    self.option_all_files();
                } else if eq(arg, "and") {
                    self.option_and_next();
                } else if pre(arg, "and=") {
                    let v = self.getloptarg(&arg[4..]);
                    self.option_and(v);
                } else if eq(arg, "andnot") {
                    self.option_andnot_next();
                } else if pre(arg, "andnot=") {
                    let v = self.getloptarg(&arg[7..]);
                    self.option_andnot(v);
                } else if eq(arg, "any-line") {
                    set!(flag_any_line = true);
                } else if eq(arg, "ascii") {
                    set!(flag_binary = 1);
                } else {
                    self.host.usage(
                        lit!("invalid option --"),
                        Some(arg),
                        Some(lit!(
                            "--after-context=, all, --and, --andnot, --any-line or --ascii"
                        )),
                    );
                }
            }
            b'b' => {
                if eq(arg, "basic-regexp") {
                    set!(flag_mode = 2);
                } else if eq(arg, "before-context") {
                    let v = self.strtonum(
                        self.getloptarg(lit!("")),
                        lit!("invalid argument --before-context="),
                    );
                    set!(flag_before_context = v);
                } else if pre(arg, "before-context=") {
                    let v = self.strtonum(
                        self.getloptarg(&arg[15..]),
                        lit!("invalid argument --before-context="),
                    );
                    set!(flag_before_context = v);
                } else if eq(arg, "best-match") {
                    set!(flag_best_match = true);
                } else if eq(arg, "binary") {
                    set!(flag_binary = 1);
                } else if eq(arg, "binary-files") {
                    set!(flag_binary_files = cs(self.strarg(self.getloptarg(lit!("")))));
                } else if pre(arg, "binary-files=") {
                    set!(flag_binary_files = cs(self.strarg(self.getloptarg(&arg[13..]))));
                } else if eq(arg, "bool") {
                    set!(flag_bool = true);
                } else if eq(arg, "break") {
                    set!(flag_break = 1);
                } else if eq(arg, "byte-offset") {
                    set!(flag_byte_offset = 1);
                } else {
                    self.host.usage(lit!("invalid option --"), Some(arg), Some(lit!("--basic-regexp, --before-context=, --binary, --binary-files=, --bool, --break or --byte-offset")));
                }
            }
            b'c' => {
                if eq(arg, "color") || eq(arg, "colour") {
                    set!(flag_color = cs(self.consts.auto));
                } else if pre(arg, "color=") {
                    set!(flag_color = cs(self.strarg(self.getloptarg(&arg[6..]))));
                } else if pre(arg, "colour=") {
                    set!(flag_color = cs(self.strarg(self.getloptarg(&arg[7..]))));
                } else if pre(arg, "colors=") {
                    set!(flag_colors = cs(self.strarg(&arg[7..])));
                } else if pre(arg, "colours=") {
                    set!(flag_colors = cs(self.strarg(&arg[8..])));
                } else if eq(arg, "column-number") {
                    set!(flag_column_number = 1);
                } else if eq(arg, "config") || pre(arg, "config=") {
                    {}
                } else if eq(arg, "confirm") {
                    set!(flag_confirm = true);
                } else if eq(arg, "context") {
                    let v = self.strtonum(
                        self.getloptarg(lit!("")),
                        lit!("invalid argument --context="),
                    );
                    set!(flag_after_context = v);
                    set!(flag_before_context = v);
                } else if pre(arg, "context=") {
                    let v = self.strtonum(
                        self.getloptarg(&arg[8..]),
                        lit!("invalid argument --context="),
                    );
                    set!(flag_after_context = v);
                    set!(flag_before_context = v);
                } else if pre(arg, "context-separator=") {
                    set!(flag_separator_dash = cs(self.strarg(&arg[18..])));
                } else if eq(arg, "count") {
                    set!(flag_count = true);
                } else if eq(arg, "cpp") {
                    set!(flag_cpp = true);
                } else if eq(arg, "csv") {
                    set!(flag_csv = true);
                } else if eq(arg, "colors") || eq(arg, "colours") || eq(arg, "context-separator") {
                    self.host
                        .usage(lit!("missing argument for --"), Some(arg), None);
                } else {
                    self.host.usage(lit!("invalid option --"), Some(arg), Some(lit!("--color, --colors=, --column-number, --config, --confirm, --context=, context-separator=, --count, --cpp or --csv")));
                }
            }
            b'd' => {
                if eq(arg, "decompress") {
                    set!(flag_decompress = true);
                } else if pre(arg, "delay=") {
                    let v = self.strtonum(
                        self.getloptarg(&arg[6..]),
                        lit!("invalid argument --delay="),
                    );
                    set!(flag_delay = v);
                } else if eq(arg, "depth") {
                    self.set_depth_long(self.getloptarg(lit!("")));
                } else if pre(arg, "depth=") {
                    let v = self.getloptarg(&arg[6..]);
                    self.set_depth_long(v);
                } else if eq(arg, "dereference") {
                    set!(flag_dereference = true);
                } else if eq(arg, "dereference-files") {
                    set!(flag_dereference_files = true);
                } else if eq(arg, "dereference-recursive") {
                    set!(flag_directories = cs(lit!("dereference-recurse")));
                } else if eq(arg, "devices") {
                    set!(flag_devices = cs(self.strarg(self.getloptarg(lit!("")))));
                } else if pre(arg, "devices=") {
                    set!(flag_devices = cs(self.strarg(self.getloptarg(&arg[8..]))));
                } else if eq(arg, "directories") {
                    set!(flag_directories = cs(self.strarg(self.getloptarg(lit!("")))));
                } else if pre(arg, "directories=") {
                    set!(flag_directories = cs(self.strarg(self.getloptarg(&arg[12..]))));
                } else if eq(arg, "dotall") {
                    set!(flag_dotall = 1);
                } else if eq(arg, "delay") {
                    self.host
                        .usage(lit!("missing argument for --"), Some(arg), None);
                } else {
                    self.host.usage(lit!("invalid option --"), Some(arg), Some(lit!("--decompress, --delay=, --depth=, --dereference, --dereference-files, --dereference-recursive, --devices=, --directories= or --dotall")));
                }
            }
            b'e' => {
                if eq(arg, "empty") {
                    set!(flag_empty = 1);
                } else if pre(arg, "encoding=") {
                    set!(flag_encoding = cs(self.strarg(self.getloptarg(&arg[9..]))));
                } else if eq(arg, "exclude") {
                    self.host
                        .vec_push(Vector::Exclude, self.getloptarg(lit!("")));
                } else if pre(arg, "exclude=") {
                    self.host
                        .vec_push(Vector::Exclude, self.getloptarg(&arg[8..]));
                } else if eq(arg, "exclude-dir") {
                    self.host
                        .vec_push(Vector::ExcludeDir, self.getloptarg(lit!("")));
                } else if pre(arg, "exclude-dir=") {
                    self.host
                        .vec_push(Vector::ExcludeDir, self.getloptarg(&arg[12..]));
                } else if eq(arg, "exclude-from") {
                    self.host
                        .vec_push(Vector::ExcludeFrom, self.getloptarg(lit!("")));
                } else if pre(arg, "exclude-from=") {
                    self.host
                        .vec_push(Vector::ExcludeFrom, self.getloptarg(&arg[13..]));
                } else if eq(arg, "exclude-fs") {
                    self.host.vec_push(Vector::ExcludeFs, lit!(""));
                } else if pre(arg, "exclude-fs=") {
                    self.host
                        .vec_push(Vector::ExcludeFs, self.getloptarg(&arg[11..]));
                } else if eq(arg, "extended-regexp") {
                    set!(flag_mode = 1);
                } else if eq(arg, "encoding") {
                    self.host
                        .usage(lit!("missing argument for --"), Some(arg), None);
                } else {
                    self.host.usage(lit!("invalid option --"), Some(arg), Some(lit!("--empty, --encoding=, --exclude=, --exclude-dir=, --exclude-from=, --exclude-fs= or --extended-regexp")));
                }
            }
            b'f' => {
                if eq(arg, "file") {
                    self.host.vec_push(Vector::File, self.getloptarg(lit!("")));
                } else if pre(arg, "file=") {
                    self.host.vec_push(Vector::File, self.getloptarg(&arg[5..]));
                } else if pre(arg, "file-extension=") {
                    self.host
                        .vec_push(Vector::FileExtension, self.getloptarg(&arg[15..]));
                } else if pre(arg, "file-magic=") {
                    self.host
                        .vec_push(Vector::FileMagic, self.getloptarg(&arg[11..]));
                } else if pre(arg, "file-type=") {
                    self.host
                        .vec_push(Vector::FileType, self.getloptarg(&arg[10..]));
                } else if eq(arg, "files") {
                    set!(flag_files = true);
                } else if eq(arg, "files-with-matches") {
                    set!(flag_files_with_matches = true);
                } else if eq(arg, "files-without-match") {
                    set!(flag_files_without_match = true);
                } else if eq(arg, "fixed-strings") {
                    set!(flag_mode = 3);
                } else if pre(arg, "filter=") {
                    let v = self.getloptarg(&arg[7..]);
                    self.host.filter_append(v);
                } else if pre(arg, "filter-magic-label=") {
                    self.host
                        .vec_push(Vector::FilterMagicLabel, self.getloptarg(&arg[19..]));
                } else if pre(arg, "format=") {
                    set!(flag_format = cs(self.strarg(self.getloptarg(&arg[7..]))));
                } else if pre(arg, "format-begin=") {
                    set!(flag_format_begin = cs(self.strarg(&arg[13..])));
                } else if pre(arg, "format-close=") {
                    set!(flag_format_close = cs(self.strarg(&arg[13..])));
                } else if pre(arg, "format-end=") {
                    set!(flag_format_end = cs(self.strarg(&arg[11..])));
                } else if pre(arg, "format-open=") {
                    set!(flag_format_open = cs(self.strarg(&arg[12..])));
                } else if eq(arg, "from") {
                    self.host.vec_push(Vector::From, self.getloptarg(lit!("")));
                } else if pre(arg, "from=") {
                    self.host.vec_push(Vector::From, self.getloptarg(&arg[5..]));
                } else if eq(arg, "fuzzy") {
                    set!(flag_fuzzy = 1);
                } else if pre(arg, "fuzzy=") {
                    let v = self.strtofuzzy(
                        self.getloptarg(&arg[6..]),
                        lit!("invalid argument --fuzzy="),
                    );
                    set!(flag_fuzzy = v);
                } else if eq(arg, "free-space") {
                    set!(flag_free_space = 1);
                } else if eq(arg, "file-extension")
                    || eq(arg, "file-magic")
                    || eq(arg, "file-type")
                    || eq(arg, "filter")
                    || eq(arg, "filter-magic-label")
                    || eq(arg, "format")
                    || eq(arg, "format-begin")
                    || eq(arg, "format-close")
                    || eq(arg, "format-end")
                    || eq(arg, "format-open")
                {
                    self.host
                        .usage(lit!("missing argument for --"), Some(arg), None);
                } else {
                    self.host.usage(lit!("invalid option --"), Some(arg), Some(lit!("--file=, --file-extension=, --file-magic=, --file-type=, --files, --files-with-matches, --files-without-match, --fixed-strings, --filter=, --filter-magic-label=, --format=, --format-begin=, --format-close, --format-end=, --format-open=, --fuzzy or --free-space")));
                }
            }
            b'g' => {
                if pre(arg, "glob=") {
                    self.host.vec_push(Vector::Glob, self.getloptarg(&arg[5..]));
                } else if eq(arg, "glob-ignore-case") {
                    set!(flag_glob_ignore_case = true);
                } else if eq(arg, "grep") {
                    set!(flag_grep = true);
                    set!(flag_empty = 1);
                } else if eq(arg, "group-separator") {
                    set!(flag_group_separator = cs(lit!("--")));
                } else if pre(arg, "group-separator=") {
                    set!(flag_group_separator = cs(self.strarg(&arg[16..])));
                } else if eq(arg, "glob") {
                    self.host
                        .usage(lit!("missing argument for --"), Some(arg), None);
                } else {
                    self.host.usage(
                        lit!("invalid option --"),
                        Some(arg),
                        Some(lit!(
                            "--glob=, --glob-ignore-case, --grep or --group-separator"
                        )),
                    );
                }
            }
            b'h' => {
                if eq(arg, "heading") {
                    set!(flag_heading = 1);
                } else if pre(arg, "help") {
                    self.help_arg(&arg[4..]);
                } else if eq(arg, "hex") {
                    set!(flag_binary_files = cs(lit!("hex")));
                } else if eq(arg, "hexdump") {
                    set!(flag_hexdump = cs(lit!("2")));
                } else if pre(arg, "hexdump=") {
                    set!(flag_hexdump = cs(self.strarg(&arg[8..])));
                } else if eq(arg, "hidden") {
                    set!(flag_hidden = true);
                } else if pre(arg, "hyperlink=") {
                    set!(flag_hyperlink = cs(self.strarg(&arg[10..])));
                } else if eq(arg, "hyperlink") {
                    set!(flag_hyperlink = cs(lit!("")));
                } else {
                    self.host.usage(
                        lit!("invalid option --"),
                        Some(arg),
                        Some(lit!(
                            "--heading, --help, --hex, --hexdump, --hidden or --hyperlink"
                        )),
                    );
                }
            }
            b'i' => {
                if pre(arg, "iglob=") {
                    self.host
                        .vec_push(Vector::Iglob, self.getloptarg(&arg[6..]));
                } else if eq(arg, "ignore-binary") {
                    set!(flag_binary_files = cs(lit!("without-match")));
                } else if eq(arg, "ignore-case") {
                    set!(flag_ignore_case = 1);
                } else if eq(arg, "ignore-files") {
                    self.host
                        .ignore_files_insert(self.consts.default_ignore_file);
                } else if pre(arg, "ignore-files=") {
                    let v = self.getloptarg(&arg[13..]);
                    self.host.ignore_files_insert(v);
                } else if eq(arg, "include") {
                    self.host
                        .vec_push(Vector::Include, self.getloptarg(lit!("")));
                } else if pre(arg, "include=") {
                    self.host
                        .vec_push(Vector::Include, self.getloptarg(&arg[8..]));
                } else if eq(arg, "include-dir") {
                    self.host
                        .vec_push(Vector::IncludeDir, self.getloptarg(lit!("")));
                } else if pre(arg, "include-dir=") {
                    self.host
                        .vec_push(Vector::IncludeDir, self.getloptarg(&arg[12..]));
                } else if eq(arg, "include-from") {
                    self.host
                        .vec_push(Vector::IncludeFrom, self.getloptarg(lit!("")));
                } else if pre(arg, "include-from=") {
                    self.host
                        .vec_push(Vector::IncludeFrom, self.getloptarg(&arg[13..]));
                } else if eq(arg, "include-fs") {
                    self.host.vec_push(Vector::IncludeFs, lit!(""));
                } else if pre(arg, "include-fs=") {
                    self.host
                        .vec_push(Vector::IncludeFs, self.getloptarg(&arg[11..]));
                } else if eq(arg, "index") {
                    set!(flag_index = cs(lit!("safe")));
                } else if pre(arg, "index=") {
                    set!(flag_index = cs(self.strarg(self.getloptarg(&arg[6..]))));
                } else if eq(arg, "initial-tab") {
                    set!(flag_initial_tab = 1);
                } else if eq(arg, "invert-match") {
                    set!(flag_invert_match = true);
                } else {
                    self.host.usage(lit!("invalid option --"), Some(arg), Some(lit!("--iglob=, --ignore-case, --ignore-files, --include=, --include-dir=, --include-from=, --include-fs=, --initial-tab or --invert-match")));
                }
            }
            b'j' => {
                if pre(arg, "jobs=") {
                    let v =
                        self.strtonum(self.getloptarg(&arg[5..]), lit!("invalid argument --jobs="));
                    set!(flag_jobs = v);
                } else if eq(arg, "json") {
                    set!(flag_json = true);
                } else if eq(arg, "jobs") {
                    self.host
                        .usage(lit!("missing argument for --"), Some(arg), None);
                } else {
                    self.host.usage(
                        lit!("invalid option --"),
                        Some(arg),
                        Some(lit!("--jobs= or --json")),
                    );
                }
            }
            b'l' => {
                if eq(arg, "label") {
                    set!(flag_label = cs(self.strarg(self.getloptarg(lit!("")))));
                } else if pre(arg, "label=") {
                    set!(flag_label = cs(self.strarg(&arg[6..])));
                } else if eq(arg, "line-buffered") {
                    set!(flag_line_buffered = true);
                } else if eq(arg, "line-number") {
                    set!(flag_line_number = 1);
                } else if eq(arg, "line-regexp") {
                    set!(flag_line_regexp = true);
                } else if eq(arg, "lines") {
                    set!(flag_files = false);
                } else {
                    self.host.usage(
                        lit!("invalid option --"),
                        Some(arg),
                        Some(lit!(
                            "--label=, --line-buffered, --line-number, --line-regexp or --lines"
                        )),
                    );
                }
            }
            b'm' => {
                if eq(arg, "match") {
                    set!(flag_match = true);
                } else if pre(arg, "max-count=") {
                    let v = self.strtopos(
                        self.getloptarg(&arg[10..]),
                        lit!("invalid argument --max-count="),
                    );
                    set!(flag_max_count = v);
                } else if pre(arg, "max-depth=") {
                    let v = self.strtopos(
                        self.getloptarg(&arg[10..]),
                        lit!("invalid argument --max-depth="),
                    );
                    set!(flag_max_depth = v);
                } else if pre(arg, "max-files=") {
                    let v = self.strtopos(
                        self.getloptarg(&arg[10..]),
                        lit!("invalid argument --max-files="),
                    );
                    set!(flag_max_files = v);
                } else if pre(arg, "max-line=") {
                    let v = self.strtopos(
                        self.getloptarg(&arg[9..]),
                        lit!("invalid argument --max-line="),
                    );
                    set!(flag_max_line = v);
                } else if pre(arg, "max-size=") {
                    let v = self.strtosize(
                        self.getloptarg(&arg[9..]),
                        lit!("invalid argument --max-size="),
                    );
                    set!(flag_max_size = v);
                } else if pre(arg, "max-queue=") {
                    let v = self.strtopos(
                        self.getloptarg(&arg[10..]),
                        lit!("invalid argument --max-queue="),
                    );
                    set!(flag_max_queue = v);
                } else if pre(arg, "min-count=") {
                    let v = self.strtopos(
                        self.getloptarg(&arg[10..]),
                        lit!("invalid argument --min-count="),
                    );
                    set!(flag_min_count = v);
                } else if pre(arg, "min-depth=") {
                    let v = self.strtopos(
                        self.getloptarg(&arg[10..]),
                        lit!("invalid argument --min-depth="),
                    );
                    set!(flag_min_depth = v);
                } else if pre(arg, "min-line=") {
                    let v = self.strtopos(
                        self.getloptarg(&arg[9..]),
                        lit!("invalid argument --min-line="),
                    );
                    set!(flag_min_line = v);
                } else if pre(arg, "min-size=") {
                    let v = self.strtosize(
                        self.getloptarg(&arg[9..]),
                        lit!("invalid argument --min-size="),
                    );
                    set!(flag_min_size = v);
                } else if pre(arg, "min-steal=") {
                    let v = self.strtopos(
                        self.getloptarg(&arg[10..]),
                        lit!("invalid argument --min-steal="),
                    );
                    set!(flag_min_steal = v);
                } else if eq(arg, "mmap") {
                    set!(flag_max_mmap = self.consts.max_mmap_size);
                } else if pre(arg, "mmap=") {
                    let v =
                        self.strtopos(self.getloptarg(&arg[5..]), lit!("invalid argument --mmap="));
                    set!(flag_max_mmap = v);
                } else if eq(arg, "messages") {
                    set!(flag_no_messages = false);
                } else if eq(arg, "max-count")
                    || eq(arg, "max-depth")
                    || eq(arg, "max-files")
                    || eq(arg, "max-line")
                    || eq(arg, "max-size")
                    || eq(arg, "min-count")
                    || eq(arg, "min-depth")
                    || eq(arg, "min-line")
                    || eq(arg, "min-size")
                {
                    self.host
                        .usage(lit!("missing argument for --"), Some(arg), None);
                } else {
                    self.host.usage(lit!("invalid option --"), Some(arg), Some(lit!("--match, --max-count=, --max-depth=, --max-files=, --max-line=, --max-size=--min-count=, --min-depth=, --min-line=, --min-size=, --mmap or --messages")));
                }
            }
            b'n' => {
                if pre(arg, "neg-regexp=") {
                    let v = self.getloptarg(&arg[11..]);
                    self.option_regexp(v, true);
                } else if eq(arg, "not") {
                    self.option_not_next();
                } else if pre(arg, "not=") {
                    let v = self.getloptarg(&arg[4..]);
                    self.option_not(v);
                } else if eq(arg, "no-any-line") {
                    set!(flag_any_line = false);
                } else if eq(arg, "no-ascii") {
                    set!(flag_binary = 0);
                } else if eq(arg, "no-binary") {
                    set!(flag_binary = 0);
                } else if eq(arg, "no-bool") {
                    set!(flag_bool = false);
                } else if eq(arg, "no-break") {
                    set!(flag_break = 0);
                } else if eq(arg, "no-byte-offset") {
                    set!(flag_byte_offset = 0);
                } else if eq(arg, "no-color") || eq(arg, "no-colour") {
                    set!(flag_color = cs(self.consts.never));
                } else if eq(arg, "no-column-number") {
                    set!(flag_column_number = 0);
                } else if eq(arg, "no-config") {
                    {}
                } else if eq(arg, "no-confirm") {
                    set!(flag_confirm = false);
                } else if eq(arg, "no-count") {
                    set!(flag_count = false);
                } else if eq(arg, "no-decompress") {
                    set!(flag_decompress = false);
                } else if eq(arg, "no-dereference") {
                    set!(flag_no_dereference = true);
                } else if eq(arg, "no-dereference-files") {
                    set!(flag_dereference_files = false);
                } else if eq(arg, "no-dotall") {
                    set!(flag_dotall = 0);
                } else if eq(arg, "no-empty") {
                    set!(flag_empty = 0);
                } else if eq(arg, "no-encoding") {
                    set!(flag_encoding = std::ptr::null());
                } else if eq(arg, "no-filename") {
                    set!(flag_no_filename = true);
                } else if eq(arg, "no-files-with-matches") {
                    set!(flag_files_with_matches = false);
                } else if eq(arg, "no-filter") {
                    self.host.filter_clear();
                } else if eq(arg, "no-glob-ignore-case") {
                    set!(flag_glob_ignore_case = false);
                } else if eq(arg, "no-group-separator") {
                    set!(flag_group_separator = std::ptr::null());
                } else if eq(arg, "no-heading") {
                    set!(flag_heading = 0);
                } else if eq(arg, "no-hidden") {
                    set!(flag_hidden = false);
                } else if eq(arg, "no-hyperlink") {
                    set!(flag_hyperlink = std::ptr::null());
                } else if eq(arg, "no-ignore-binary") {
                    set!(flag_binary_files = cs(lit!("binary")));
                } else if eq(arg, "no-ignore-case") {
                    set!(flag_ignore_case = 0);
                } else if eq(arg, "no-ignore-files") {
                    self.host.ignore_files_clear();
                } else if eq(arg, "no-index") {
                    set!(flag_index = std::ptr::null());
                } else if eq(arg, "no-initial-tab") {
                    set!(flag_initial_tab = 0);
                } else if eq(arg, "no-invert-match") {
                    set!(flag_invert_match = false);
                } else if eq(arg, "no-line-number") {
                    set!(flag_line_number = 0);
                } else if eq(arg, "no-only-line-number") {
                    set!(flag_only_line_number = false);
                } else if eq(arg, "no-only-matching") {
                    set!(flag_only_matching = false);
                } else if eq(arg, "no-messages") {
                    set!(flag_no_messages = true);
                } else if eq(arg, "no-mmap") {
                    set!(flag_max_mmap = 0);
                } else if eq(arg, "no-pager") {
                    set!(flag_pager = std::ptr::null());
                } else if eq(arg, "no-passthru") {
                    set!(flag_any_line = false);
                } else if eq(arg, "no-pretty") {
                    set!(flag_pretty = std::ptr::null());
                } else if eq(arg, "no-smart-case") {
                    set!(flag_smart_case = 0);
                } else if eq(arg, "no-sort") {
                    set!(flag_sort = std::ptr::null());
                } else if eq(arg, "no-split") {
                    set!(flag_split = false);
                } else if eq(arg, "no-tree") {
                    set!(flag_tree = 0);
                } else if eq(arg, "no-stats") {
                    set!(flag_stats = std::ptr::null());
                } else if eq(arg, "no-ungroup") {
                    set!(flag_ungroup = 0);
                } else if eq(arg, "no-view") {
                    set!(flag_view = std::ptr::null());
                } else if eq(arg, "null") {
                    set!(flag_null = true);
                } else if eq(arg, "null-data") {
                    set!(flag_null_data = true);
                } else if eq(arg, "neg-regexp") {
                    self.host
                        .usage(lit!("missing argument for --"), Some(arg), None);
                } else {
                    self.host.usage(lit!("invalid option --"), Some(arg), Some(lit!("--neg-regexp=, --not, --no-any-line, --no-ascii, --no-binary, --no-bool, --no-break, --no-byte-offset, --no-color, --no-config, --no-confirm, --no-count, --no-decompress, --no-dereference, --no-dereference-files, --no-dotall, --no-encoding, --no-empty, --no-filename, --no-files-with-matches, --no-filter, --no-glob-ignore-case, --no-group-separator, --no-heading, --no-hidden, --no-hyperlink, --no-ignore-binary, --no-ignore-case, --no-ignore-files, --no-index, --no-initial-tab, --no-invert-match, --no-line-number, --no-only-line-number, --no-only-matching, --no-messages, --no-mmap, --no-pager, --no-pretty, --no-smart-case, --no-sort, --no-split, --no-stats, --no-tree, --no-ungroup, --no-view, --null or --null-data")));
                }
            }
            b'o' => {
                if eq(arg, "only-line-number") {
                    set!(flag_only_line_number = true);
                } else if eq(arg, "only-matching") {
                    set!(flag_only_matching = true);
                } else {
                    self.host.usage(
                        lit!("invalid option --"),
                        Some(arg),
                        Some(lit!("--only-line-number or --only-matching")),
                    );
                }
            }
            b'p' => {
                if eq(arg, "pager") {
                    set!(flag_pager = cs(lit!("")));
                } else if pre(arg, "pager=") {
                    set!(flag_pager = cs(self.strarg(self.getloptarg(&arg[6..]))));
                } else if eq(arg, "passthru") {
                    set!(flag_any_line = true);
                } else if eq(arg, "perl-regexp") {
                    set!(flag_mode = 4);
                } else if eq(arg, "pretty") {
                    set!(flag_pretty = cs(self.consts.auto));
                } else if pre(arg, "pretty=") {
                    set!(flag_pretty = cs(self.strarg(self.getloptarg(&arg[7..]))));
                } else {
                    self.host.usage(
                        lit!("invalid option --"),
                        Some(arg),
                        Some(lit!("--pager, --passthru, --perl-regexp= or --pretty")),
                    );
                }
            }
            b'q' => {
                if eq(arg, "query") {
                    set!(flag_query = true);
                } else if pre(arg, "query=") {
                    let v = self.getloptarg(&arg[6..]);
                    let v = self.strtonum(v, lit!("invalid argument --query="));
                    set!(flag_delay = v);
                    set!(flag_query = true);
                } else if eq(arg, "quiet") {
                    set!(flag_quiet = true);
                    set!(flag_no_messages = true);
                } else {
                    self.host.usage(
                        lit!("invalid option --"),
                        Some(arg),
                        Some(lit!("--query or --quiet")),
                    );
                }
            }
            b'r' => {
                if pre(arg, "range=") {
                    self.strtopos2_flags(
                        self.getloptarg(&arg[6..]),
                        Pair::Line,
                        lit!("invalid argument --range="),
                    );
                } else if eq(arg, "recursive") {
                    set!(flag_directories = cs(lit!("recurse")));
                } else if eq(arg, "regexp") {
                    let v = self.getloptarg(lit!(""));
                    self.option_regexp(v, false);
                } else if pre(arg, "regexp=") {
                    let v = self.getloptarg(&arg[7..]);
                    self.option_regexp(v, false);
                } else if pre(arg, "replace=") {
                    set!(flag_replace = cs(self.strarg(self.getloptarg(&arg[8..]))));
                } else if eq(arg, "range") || eq(arg, "replace") {
                    self.host
                        .usage(lit!("missing argument for --"), Some(arg), None);
                } else {
                    self.host.usage(
                        lit!("invalid option --"),
                        Some(arg),
                        Some(lit!("--range=, --recursive, --regexp= or --replace=")),
                    );
                }
            }
            b's' => {
                if eq(arg, "save-config") {
                    set!(flag_save_config = cs(lit!(".ugrep")));
                } else if pre(arg, "save-config=") {
                    set!(flag_save_config = cs(self.strarg(self.getloptarg(&arg[12..]))));
                } else if eq(arg, "separator") {
                    set!(flag_separator = std::ptr::null());
                } else if pre(arg, "separator=") {
                    set!(flag_separator = cs(self.strarg(&arg[10..])));
                } else if eq(arg, "silent") {
                    set!(flag_quiet = true);
                    set!(flag_no_messages = true);
                } else if eq(arg, "smart-case") {
                    set!(flag_smart_case = 1);
                } else if eq(arg, "sort") {
                    set!(flag_sort = cs(lit!("name")));
                } else if pre(arg, "sort=") {
                    set!(flag_sort = cs(self.strarg(self.getloptarg(&arg[5..]))));
                } else if eq(arg, "split") {
                    set!(flag_split = true);
                } else if eq(arg, "stats") {
                    set!(flag_stats = cs(lit!("")));
                } else if pre(arg, "stats=") {
                    set!(flag_stats = cs(self.strarg(&arg[6..])));
                } else {
                    self.host.usage(lit!("invalid option --"), Some(arg), Some(lit!("--save-config, --separator, --silent, --smart-case, --sort, --split or --stats")));
                }
            }
            b't' => {
                if eq(arg, "tabs") {
                    set!(flag_tabs = self.consts.default_tabs);
                } else if pre(arg, "tabs=") {
                    let v =
                        self.strtopos(self.getloptarg(&arg[5..]), lit!("invalid argument --tabs="));
                    set!(flag_tabs = v);
                } else if eq(arg, "tag") {
                    set!(flag_tag = cs(self.consts.default_tag));
                } else if pre(arg, "tag=") {
                    set!(flag_tag = cs(self.strarg(self.getloptarg(&arg[4..]))));
                } else if eq(arg, "text") {
                    set!(flag_binary_files = cs(lit!("text")));
                } else if eq(arg, "tree") {
                    set!(flag_tree = 1);
                } else {
                    self.host.usage(
                        lit!("invalid option --"),
                        Some(arg),
                        Some(lit!("--tabs, --tag, --text or --tree")),
                    );
                }
            }
            b'u' => {
                if eq(arg, "ungroup") {
                    set!(flag_ungroup = 1);
                } else {
                    self.host.usage(
                        lit!("invalid option --"),
                        Some(arg),
                        Some(lit!("--ungroup")),
                    );
                }
            }
            b'v' => {
                if eq(arg, "version") {
                    self.host.version();
                } else if pre(arg, "view=") {
                    set!(flag_view = cs(self.strarg(self.getloptarg(&arg[5..]))));
                } else if eq(arg, "view") {
                    set!(flag_view = cs(lit!("")));
                } else {
                    self.host.usage(
                        lit!("invalid option --"),
                        Some(arg),
                        Some(lit!("--view or --version")),
                    );
                }
            }
            b'w' => {
                if eq(arg, "width") {
                    let v = self.host.screen_width();
                    set!(flag_width = v);
                } else if pre(arg, "width=") {
                    let v = self.strtopos(
                        self.getloptarg(&arg[6..]),
                        lit!("invalid argument --width="),
                    );
                    set!(flag_width = v);
                } else if eq(arg, "with-filename") {
                    set!(flag_with_filename = true);
                } else if eq(arg, "with-hex") {
                    set!(flag_binary_files = cs(lit!("with-hex")));
                } else if eq(arg, "word-regexp") {
                    set!(flag_word_regexp = true);
                } else {
                    self.host.usage(
                        lit!("invalid option --"),
                        Some(arg),
                        Some(lit!(
                            "--width, --with-filename, --with-hex or --word-regexp"
                        )),
                    );
                }
            }
            b'x' => {
                if eq(arg, "xml") {
                    set!(flag_xml = true);
                } else {
                    self.host
                        .usage(lit!("invalid option --"), Some(arg), Some(lit!("--xml")));
                }
            }
            b'z' => {
                if pre(arg, "zmax=") {
                    let v =
                        self.strtopos(self.getloptarg(&arg[5..]), lit!("invalid argument --zmax="));
                    set!(flag_zmax = v);
                } else if eq(arg, "zmax") {
                    self.host
                        .usage(lit!("missing argument for --"), Some(arg), None);
                } else {
                    self.host
                        .usage(lit!("invalid option --"), Some(arg), Some(lit!("--zmax=")));
                }
            }
            _ => {
                if arg[0].is_ascii_digit() {
                    self.set_depth_long(arg);
                } else {
                    self.usage(lit!("invalid option --"), Some(arg));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strtoull_follows_c() {
        assert_eq!(strtoull(b"123x"), (123, 3));
        assert_eq!(strtoull(b""), (0, 0));
        assert_eq!(strtoull(b"x"), (0, 0));
        assert_eq!(strtoull(b"  +7"), (7, 4));
        assert_eq!(strtoull(b"-1"), (u64::MAX, 2));
        assert_eq!(strtoull(b"99999999999999999999999"), (u64::MAX, 23));
    }

    #[test]
    fn trim_pathname_start_cuts_trailing_separators() {
        assert_eq!(trim_pathname_start(b"dir//", b'/'), Some(3));
        assert_eq!(trim_pathname_start(b"dir", b'/'), None);
        assert_eq!(trim_pathname_start(b"/", b'/'), None);
    }
}
