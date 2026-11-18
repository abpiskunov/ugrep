//! Command-line option parsing: `options()` of src/ugrep.cpp with `option_regexp`, `option_and`, `option_andnot`,
//! `option_not`, `option_all_files`, `set_depth`, `set_depth_long` and the argument helpers `getoptarg`,
//! `getloptarg`, `strarg`, `strtonum`, `strtopos`, `strtopos2`, `strtosize` and `strtofuzzy`.
//!
//! The scalar option state is the Rust-owned [`Settings`]. What stays in C++ (the `std::string` and container
//! options, the CNF pattern list, `Static::arg_pattern`/`arg_files`, `usage`, `help`, `version`, the screen
//! size) is reached through the [`Env`] trait; `ffi/options.rs` implements it with a table of C callbacks.
//!
//! Arguments are NUL-terminated byte slices (`Z`): the C strings of `argv`, never converted to `String`.
//! `usage` may return (when a config file is read), so every call site continues exactly like the C++ does.

use crate::settings::Settings;
use std::os::raw::c_char;

/// A NUL-terminated byte string (the terminator is the last byte).
pub type Z<'a> = &'a [u8];

const EMPTY: Z<'static> = b"\0";

/// `CNF::PATTERN` mask values.
pub const PATTERN_ALT: i32 = 0;
pub const PATTERN_NEG: i32 = 1;
pub const PATTERN_NOT: i32 = 2;
pub const PATTERN_AND: i32 = 4;

/// `Mode` values of src/flag.hpp.
const MODE_ERE: i32 = 1;
const MODE_BRE: i32 = 2;
const MODE_FIXED: i32 = 3;
const MODE_PERL: i32 = 4;

/// `reflex::FuzzyMatcher::INS`, `DEL` and `SUB`.
const FUZZY_INS: usize = 0x1000;
const FUZZY_DEL: usize = 0x2000;
const FUZZY_SUB: usize = 0x4000;

/// The C++ string and container options that options() fills.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum List {
    File,
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
    /// `std::set<std::string>`: pushing inserts.
    IgnoreFiles,
}

/// Build-dependent values and C++ globals that the parser needs.
#[derive(Clone, Copy)]
pub struct Consts {
    /// `Static::AUTO` and `Static::NEVER` (compared by address elsewhere, so the same pointers are stored).
    pub auto: *const c_char,
    pub never: *const c_char,
    pub max_mmap_size: usize,
    pub default_tabs: usize,
    pub default_tag: *const c_char,
    /// DEFAULT_IGNORE_FILE, NUL-terminated.
    pub default_ignore_file: Z<'static>,
    /// `OS_WIN`: `/` also starts an option.
    pub os_win: bool,
}

/// What the parser needs from the retained C++ side.
pub trait Env {
    /// `usage(message, arg, valid)`; the call returns when `flag_usage_warnings` is set.
    fn usage(&mut self, message: &str, arg: Option<&[u8]>, valid: Option<&str>);
    /// `help(what)`
    fn help(&mut self, what: Option<Z<'_>>);
    /// `version()`
    fn version(&mut self);
    /// `strarg(string)`: a copy kept for the whole run.
    fn strarg(&mut self, string: Z<'_>) -> *const c_char;
    /// `pattern_args.emplace_back(mask, arg)`; `arg` is a pointer into argv that stays valid.
    fn pattern_push(&mut self, mask: i32, arg: Z<'_>);
    fn patterns_empty(&mut self) -> bool;
    fn list_push(&mut self, list: List, value: Z<'_>);
    fn list_clear(&mut self, list: List);
    fn list_empty(&mut self, list: List) -> bool;
    /// `flag_filter.append(flag_filter.empty() ? "" : ",").append(value)`
    fn filter_append(&mut self, value: Z<'_>);
    fn filter_clear(&mut self);
    fn arg_pattern_is_null(&mut self) -> bool;
    fn set_arg_pattern(&mut self, arg: Z<'_>);
    fn push_arg_file(&mut self, arg: Z<'_>);
    /// `tui_screen_getsize()`
    fn screen_width(&mut self) -> usize;
    /// The bytes of a settings C string (empty for a null pointer).
    fn c_string(&mut self, p: *const c_char) -> Vec<u8>;
}

fn at(s: &[u8], k: usize) -> u8 {
    s.get(k).copied().unwrap_or(0)
}

fn body(z: Z<'_>) -> &[u8] {
    z.strip_suffix(b"\0").unwrap_or(z)
}

/// `strcmp(arg, lit) == 0`
fn eq(a: Z<'_>, lit: &str) -> bool {
    body(a) == lit.as_bytes()
}

/// `strncmp(arg, lit, strlen(lit)) == 0`
fn starts(a: Z<'_>, lit: &str) -> bool {
    a.starts_with(lit.as_bytes())
}

fn ptr(z: Z<'_>) -> *const c_char {
    z.as_ptr().cast()
}

fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// C `strtoull(s, &rest, 10)`: the value and the index of `rest`.
pub fn strtoull10(s: &[u8]) -> (u64, usize) {
    let mut p = 0;
    while is_space(at(s, p)) {
        p += 1;
    }
    let mut neg = false;
    if at(s, p) == b'-' || at(s, p) == b'+' {
        neg = at(s, p) == b'-';
        p += 1;
    }
    if !is_digit(at(s, p)) {
        return (0, 0);
    }
    let mut v: u64 = 0;
    let mut overflow = false;
    while is_digit(at(s, p)) {
        let d = u64::from(at(s, p) - b'0');
        match v.checked_mul(10).and_then(|x| x.checked_add(d)) {
            Some(x) => v = x,
            None => overflow = true,
        }
        p += 1;
    }
    if overflow {
        return (u64::MAX, p);
    }
    (if neg { v.wrapping_neg() } else { v }, p)
}

struct Parser<'a, 'e, E: Env> {
    s: &'a mut Settings,
    env: &'e mut E,
    argv: &'a [Z<'a>],
    k: Consts,
}

/// Parses `argv[1..]` (`argv[0]` is not looked at) into `settings` and the C++ side.
pub fn options<E: Env>(settings: &mut Settings, env: &mut E, k: Consts, argv: &[Z<'_>]) {
    let mut p = Parser { s: settings, env, argv, k };
    p.run();
}

impl<'a, E: Env> Parser<'a, '_, E> {
    fn usage(&mut self, message: &str, arg: Option<Z<'_>>, valid: Option<&str>) {
        self.env.usage(message, arg.map(body), valid);
    }

    /// `getloptarg(argc, argv, arg, i)`: `rest` is the text after `=`.
    fn lopt(&self, rest: Z<'a>, i: &mut usize) -> Z<'a> {
        if at(rest, 0) != 0 {
            return rest;
        }
        *i += 1;
        if *i < self.argv.len() {
            return self.argv[*i];
        }
        EMPTY
    }

    /// `getoptarg(argc, argv, arg, i)` for the short option at `s[k]`.
    fn sopt(&self, s: Z<'a>, k: usize, i: &mut usize) -> Z<'a> {
        let mut p = k + 1;
        if at(s, p) == b'=' {
            p += 1;
        }
        if at(s, p) != 0 {
            return &s[p..];
        }
        *i += 1;
        if *i < self.argv.len() {
            return self.argv[*i];
        }
        EMPTY
    }

    /// `--name=VALUE` with VALUE from the text or the next word.
    fn lv(&self, a: Z<'a>, prefix: &str, i: &mut usize) -> Option<Z<'a>> {
        a.strip_prefix(prefix.as_bytes()).map(|rest| self.lopt(rest, i))
    }

    fn strarg(&mut self, z: Z<'_>) -> *const c_char {
        self.env.strarg(z)
    }

    fn strtonum(&mut self, z: Z<'_>, message: &str) -> usize {
        let (v, rest) = strtoull10(z);
        if at(z, rest) != 0 {
            self.usage(message, Some(z), None);
        }
        v as usize
    }

    fn strtopos(&mut self, z: Z<'_>, message: &str) -> usize {
        let size = self.strtonum(z, message);
        if size == 0 {
            self.usage(message, Some(z), None);
        }
        size
    }

    /// `strtopos2`: `None` when nothing is assigned (empty argument).
    fn strtopos2(&mut self, z: Z<'_>, message: &str) -> Option<(usize, usize)> {
        if at(z, 0) == 0 {
            self.usage(message, Some(z), None);
            return None;
        }
        let mut rest = 0;
        let mut min;
        let max;
        if at(z, 0) != b',' {
            let (v, r) = strtoull10(z);
            min = v as usize;
            rest = r;
        } else {
            min = 0;
        }
        if at(z, rest) == b',' {
            let (v, r) = strtoull10(&z[rest + 1..]);
            max = v as usize;
            rest = rest + 1 + r;
        } else {
            max = min;
            min = 0;
        }
        if at(z, rest) != 0 || (max > 0 && min > max) {
            self.usage(message, Some(z), None);
        }
        Some((min, max))
    }

    fn strtosize(&mut self, z: Z<'_>, message: &str) -> usize {
        let (v, mut rest) = strtoull10(z);
        let mut size = v as usize;
        if size == 0 {
            self.usage(message, Some(z), None);
        }
        let shift = match at(z, rest) {
            b'k' | b'K' => Some(10),
            b'm' | b'M' => Some(20),
            b'g' | b'G' => Some(30),
            b't' | b'T' => Some(40),
            _ => None,
        };
        if let Some(sh) = shift {
            size = size.wrapping_mul(1usize << sh);
            rest += 1;
        }
        if !matches!(at(z, rest), b'b' | b'B' | 0) {
            self.usage(message, Some(z), None);
        }
        size
    }

    fn strtofuzzy(&mut self, z: Z<'_>, message: &str) -> usize {
        let mut string = z;
        let mut flags = 0usize;
        let mut max = 1usize;
        while at(string, 0) != 0 {
            match string[0] {
                b'b' => {
                    if !starts(string, "best") {
                        self.usage(message, Some(string), None);
                    }
                    self.s.flag_best_match = true;
                    string = &string[4.min(string.len())..];
                }
                b'+' => {
                    flags |= FUZZY_INS;
                    string = &string[1..];
                }
                b'-' => {
                    flags |= FUZZY_DEL;
                    string = &string[1..];
                }
                b'~' => {
                    flags |= FUZZY_SUB;
                    string = &string[1..];
                }
                _ => {
                    let (v, rest) = strtoull10(string);
                    max = v as usize;
                    if max == 0 || max > 0xff || at(string, rest) != 0 {
                        self.usage(message, Some(string), None);
                    }
                    string = &string[rest..];
                }
            }
        }
        max | flags
    }

    /// `set_depth(arg)`: `s[k..]` is the text; returns the index of the first byte not consumed.
    fn set_depth(&mut self, s: Z<'_>, k: usize) -> usize {
        let mut range = k;
        if self.s.flag_max_depth > 0 {
            if self.s.flag_min_depth == 0 {
                self.s.flag_min_depth = self.s.flag_max_depth;
            }
            let (v, r) = strtoull10(&s[range..]);
            self.s.flag_max_depth = v as usize;
            range += r;
        } else {
            let (v, r) = strtoull10(&s[range..]);
            self.s.flag_max_depth = v as usize;
            range += r;
            if at(s, range) == b'-' || at(s, range) == b',' {
                self.s.flag_min_depth = self.s.flag_max_depth;
                let (v, r) = strtoull10(&s[range + 1..]);
                self.s.flag_max_depth = v as usize;
                range = range + 1 + r;
            }
        }
        if self.s.flag_min_depth > self.s.flag_max_depth {
            self.usage("invalid argument -", Some(&s[k..]), None);
        }
        range
    }

    /// `set_depth_long(arg)`
    fn set_depth_long(&mut self, arg: Z<'_>) {
        let end = self.set_depth(arg, 0);
        // C++: `range` is one before `end`, and `range[1]` is the byte at `end`
        if at(arg, end) != 0 {
            self.usage("invalid argument --depth=", Some(arg), None);
        }
    }

    fn option_regexp(&mut self, arg: Z<'_>, is_neg: bool) {
        let mask = (if self.s.flag_not { PATTERN_NOT } else { PATTERN_ALT }) | (if is_neg { PATTERN_NEG } else { PATTERN_ALT });
        self.env.pattern_push(mask, arg);
    }

    fn next_is_pattern(&self, i: usize) -> bool {
        i + 1 < self.argv.len() && at(self.argv[i + 1], 0) != b'-'
    }

    fn not_mask(&self) -> i32 {
        if self.s.flag_not {
            PATTERN_NOT
        } else {
            PATTERN_ALT
        }
    }

    fn missing_not(&mut self) {
        self.usage("missing PATTERN for --not", None, None);
    }

    /// `option_and(pattern_args, i, argc, argv)`
    fn option_and_next(&mut self, i: &mut usize) {
        if self.s.flag_not {
            self.missing_not();
        }
        self.env.pattern_push(PATTERN_AND, EMPTY);
        if self.next_is_pattern(*i) {
            *i += 1;
            let mask = self.not_mask();
            self.env.pattern_push(mask, self.argv[*i]);
        }
    }

    fn option_and(&mut self, arg: Z<'_>) {
        if self.s.flag_not {
            self.missing_not();
        }
        self.env.pattern_push(PATTERN_AND, EMPTY);
        let mask = self.not_mask();
        self.env.pattern_push(mask, arg);
    }

    fn option_andnot_next(&mut self, i: &mut usize) {
        if self.s.flag_not {
            self.missing_not();
        }
        self.env.pattern_push(PATTERN_AND, EMPTY);
        self.s.flag_not = true;
        if self.next_is_pattern(*i) {
            *i += 1;
            self.env.pattern_push(PATTERN_NOT, self.argv[*i]);
            self.s.flag_not = false;
        }
    }

    fn option_andnot(&mut self, arg: Z<'_>) {
        if self.s.flag_not {
            self.missing_not();
        }
        self.env.pattern_push(PATTERN_AND, EMPTY);
        self.env.pattern_push(PATTERN_NOT, arg);
    }

    fn option_not_next(&mut self, i: &mut usize) {
        self.s.flag_not = !self.s.flag_not;
        if self.next_is_pattern(*i) {
            *i += 1;
            let mask = self.not_mask();
            self.env.pattern_push(mask, self.argv[*i]);
            self.s.flag_not = false;
        }
    }

    fn option_not(&mut self, arg: Z<'_>) {
        self.s.flag_not = !self.s.flag_not;
        let mask = self.not_mask();
        self.env.pattern_push(mask, arg);
        self.s.flag_not = false;
    }

    fn option_all_files(&mut self) {
        for list in [
            List::Glob,
            List::Iglob,
            List::Exclude,
            List::ExcludeDir,
            List::ExcludeFrom,
            List::Include,
            List::IncludeDir,
            List::IncludeFrom,
            List::FileType,
            List::FileExtension,
            List::FileMagic,
            List::IgnoreFiles,
        ] {
            self.env.list_clear(list);
        }
        if self.env.c_string(self.s.flag_binary_files) == b"without-match" {
            self.s.flag_binary_files = c"binary".as_ptr();
        }
    }

    fn run(&mut self) {
        let argc = self.argv.len();
        let mut options = true;
        let mut i = 1;
        while i < argc {
            let s = self.argv[i];
            let first = at(s, 0);
            if (first == b'-' || (self.k.os_win && first == b'/')) && at(s, 1) != 0 && options {
                let mut is_grouped = true;
                let mut k = 0usize;
                while is_grouped {
                    k += 1;
                    if at(s, k) == 0 {
                        break;
                    }
                    if at(s, k) == b'-' {
                        is_grouped = false;
                        k += 1;
                        if at(s, k) == 0 {
                            options = false;
                            continue;
                        }
                        self.long_option(&s[k..], &mut i);
                    } else {
                        self.short_option(s, &mut k, &mut i, &mut is_grouped);
                    }
                }
            } else if eq(s, "-") {
                self.s.flag_stdin = true;
            } else if self.env.arg_pattern_is_null()
                && !self.s.flag_match
                && !self.s.flag_not
                && self.env.patterns_empty()
                && self.env.list_empty(List::File)
            {
                self.env.set_arg_pattern(s);
            } else {
                self.env.push_arg_file(s);
            }
            i += 1;
        }

        if self.s.flag_not {
            self.usage("missing PATTERN for --not", None, None);
        }
    }

    fn long_option(&mut self, a: Z<'a>, i: &mut usize) {
        match a[0] {
            b'-' => {}
            b'a' => self.long_a(a, i),
            b'b' => self.long_b(a, i),
            b'c' => self.long_c(a, i),
            b'd' => self.long_d(a, i),
            b'e' => self.long_e(a, i),
            b'f' => self.long_f(a, i),
            b'g' => self.long_g(a, i),
            b'h' => self.long_h(a, i),
            b'i' => self.long_i(a, i),
            b'j' => {
                if let Some(v) = self.lv(a, "jobs=", i) {
                    self.s.flag_jobs = self.strtonum(v, "invalid argument --jobs=");
                } else if eq(a, "json") {
                    self.s.flag_json = true;
                } else if eq(a, "jobs") {
                    self.usage("missing argument for --", Some(a), None);
                } else {
                    self.usage("invalid option --", Some(a), Some("--jobs= or --json"));
                }
            }
            b'l' => self.long_l(a, i),
            b'm' => self.long_m(a, i),
            b'n' => self.long_n(a, i),
            b'o' => {
                if eq(a, "only-line-number") {
                    self.s.flag_only_line_number = true;
                } else if eq(a, "only-matching") {
                    self.s.flag_only_matching = true;
                } else {
                    self.usage("invalid option --", Some(a), Some("--only-line-number or --only-matching"));
                }
            }
            b'p' => self.long_p(a, i),
            b'q' => {
                if eq(a, "query") {
                    self.s.flag_query = true;
                } else if let Some(v) = self.lv(a, "query=", i) {
                    self.s.flag_delay = self.strtonum(v, "invalid argument --query=");
                    self.s.flag_query = true;
                } else if eq(a, "quiet") {
                    self.s.flag_quiet = true;
                    self.s.flag_no_messages = true;
                } else {
                    self.usage("invalid option --", Some(a), Some("--query or --quiet"));
                }
            }
            b'r' => self.long_r(a, i),
            b's' => self.long_s(a, i),
            b't' => self.long_t(a, i),
            b'u' => {
                if eq(a, "ungroup") {
                    self.s.flag_ungroup = 1;
                } else {
                    self.usage("invalid option --", Some(a), Some("--ungroup"));
                }
            }
            b'v' => {
                if eq(a, "version") {
                    self.env.version();
                } else if let Some(v) = self.lv(a, "view=", i) {
                    self.s.flag_view = self.strarg(v);
                } else if eq(a, "view") {
                    self.s.flag_view = c"".as_ptr();
                } else {
                    self.usage("invalid option --", Some(a), Some("--view or --version"));
                }
            }
            b'w' => {
                if eq(a, "width") {
                    self.s.flag_width = self.env.screen_width();
                } else if let Some(v) = self.lv(a, "width=", i) {
                    self.s.flag_width = self.strtopos(v, "invalid argument --width=");
                } else if eq(a, "with-filename") {
                    self.s.flag_with_filename = true;
                } else if eq(a, "with-hex") {
                    self.s.flag_binary_files = c"with-hex".as_ptr();
                } else if eq(a, "word-regexp") {
                    self.s.flag_word_regexp = true;
                } else {
                    self.usage("invalid option --", Some(a), Some("--width, --with-filename, --with-hex or --word-regexp"));
                }
            }
            b'x' => {
                if eq(a, "xml") {
                    self.s.flag_xml = true;
                } else {
                    self.usage("invalid option --", Some(a), Some("--xml"));
                }
            }
            b'z' => {
                if let Some(v) = self.lv(a, "zmax=", i) {
                    self.s.flag_zmax = self.strtopos(v, "invalid argument --zmax=");
                } else if eq(a, "zmax") {
                    self.usage("missing argument for --", Some(a), None);
                } else {
                    self.usage("invalid option --", Some(a), Some("--zmax="));
                }
            }
            c => {
                if is_digit(c) {
                    self.set_depth_long(a);
                } else {
                    self.usage("invalid option --", Some(a), None);
                }
            }
        }
    }

    fn long_a(&mut self, a: Z<'a>, i: &mut usize) {
        if eq(a, "after-context") {
            let v = self.lopt(EMPTY, i);
            self.s.flag_after_context = self.strtonum(v, "invalid argument --after-context=");
        } else if let Some(v) = self.lv(a, "after-context=", i) {
            self.s.flag_after_context = self.strtonum(v, "invalid argument --after-context=");
        } else if eq(a, "all") {
            self.option_all_files();
        } else if eq(a, "and") {
            self.option_and_next(i);
        } else if let Some(v) = self.lv(a, "and=", i) {
            self.option_and(v);
        } else if eq(a, "andnot") {
            self.option_andnot_next(i);
        } else if let Some(v) = self.lv(a, "andnot=", i) {
            self.option_andnot(v);
        } else if eq(a, "any-line") {
            self.s.flag_any_line = true;
        } else if eq(a, "ascii") {
            self.s.flag_binary = 1;
        } else {
            self.usage("invalid option --", Some(a), Some("--after-context=, all, --and, --andnot, --any-line or --ascii"));
        }
    }

    fn long_b(&mut self, a: Z<'a>, i: &mut usize) {
        if eq(a, "basic-regexp") {
            self.s.flag_mode = MODE_BRE;
        } else if eq(a, "before-context") {
            let v = self.lopt(EMPTY, i);
            self.s.flag_before_context = self.strtonum(v, "invalid argument --before-context=");
        } else if let Some(v) = self.lv(a, "before-context=", i) {
            self.s.flag_before_context = self.strtonum(v, "invalid argument --before-context=");
        } else if eq(a, "best-match") {
            self.s.flag_best_match = true;
        } else if eq(a, "binary") {
            self.s.flag_binary = 1;
        } else if eq(a, "binary-files") {
            let v = self.lopt(EMPTY, i);
            self.s.flag_binary_files = self.strarg(v);
        } else if let Some(v) = self.lv(a, "binary-files=", i) {
            self.s.flag_binary_files = self.strarg(v);
        } else if eq(a, "bool") {
            self.s.flag_bool = true;
        } else if eq(a, "break") {
            self.s.flag_break = 1;
        } else if eq(a, "byte-offset") {
            self.s.flag_byte_offset = 1;
        } else {
            self.usage(
                "invalid option --",
                Some(a),
                Some("--basic-regexp, --before-context=, --binary, --binary-files=, --bool, --break or --byte-offset"),
            );
        }
    }

    fn long_c(&mut self, a: Z<'a>, i: &mut usize) {
        if eq(a, "color") || eq(a, "colour") {
            self.s.flag_color = self.k.auto;
        } else if let Some(v) = self.lv(a, "color=", i) {
            self.s.flag_color = self.strarg(v);
        } else if let Some(v) = self.lv(a, "colour=", i) {
            self.s.flag_color = self.strarg(v);
        } else if starts(a, "colors=") {
            self.s.flag_colors = self.strarg(&a[7..]);
        } else if starts(a, "colours=") {
            self.s.flag_colors = self.strarg(&a[8..]);
        } else if eq(a, "column-number") {
            self.s.flag_column_number = 1;
        } else if eq(a, "config") || starts(a, "config=") {
            // --config is pre-parsed before other options are parsed
        } else if eq(a, "confirm") {
            self.s.flag_confirm = true;
        } else if eq(a, "context") {
            let v = self.lopt(EMPTY, i);
            let n = self.strtonum(v, "invalid argument --context=");
            self.s.flag_before_context = n;
            self.s.flag_after_context = n;
        } else if let Some(v) = self.lv(a, "context=", i) {
            let n = self.strtonum(v, "invalid argument --context=");
            self.s.flag_before_context = n;
            self.s.flag_after_context = n;
        } else if starts(a, "context-separator=") {
            self.s.flag_separator_dash = self.strarg(&a[18..]);
        } else if eq(a, "count") {
            self.s.flag_count = true;
        } else if eq(a, "cpp") {
            self.s.flag_cpp = true;
        } else if eq(a, "csv") {
            self.s.flag_csv = true;
        } else if eq(a, "colors") || eq(a, "colours") || eq(a, "context-separator") {
            self.usage("missing argument for --", Some(a), None);
        } else {
            self.usage(
                "invalid option --",
                Some(a),
                Some("--color, --colors=, --column-number, --config, --confirm, --context=, context-separator=, --count, --cpp or --csv"),
            );
        }
    }

    fn long_d(&mut self, a: Z<'a>, i: &mut usize) {
        if eq(a, "decompress") {
            self.s.flag_decompress = true;
        } else if let Some(v) = self.lv(a, "delay=", i) {
            self.s.flag_delay = self.strtonum(v, "invalid argument --delay=");
        } else if eq(a, "depth") {
            let v = self.lopt(EMPTY, i);
            self.set_depth_long(v);
        } else if let Some(v) = self.lv(a, "depth=", i) {
            self.set_depth_long(v);
        } else if eq(a, "dereference") {
            self.s.flag_dereference = true;
        } else if eq(a, "dereference-files") {
            self.s.flag_dereference_files = true;
        } else if eq(a, "dereference-recursive") {
            self.s.flag_directories = c"dereference-recurse".as_ptr();
        } else if eq(a, "devices") {
            let v = self.lopt(EMPTY, i);
            self.s.flag_devices = self.strarg(v);
        } else if let Some(v) = self.lv(a, "devices=", i) {
            self.s.flag_devices = self.strarg(v);
        } else if eq(a, "directories") {
            let v = self.lopt(EMPTY, i);
            self.s.flag_directories = self.strarg(v);
        } else if let Some(v) = self.lv(a, "directories=", i) {
            self.s.flag_directories = self.strarg(v);
        } else if eq(a, "dotall") {
            self.s.flag_dotall = 1;
        } else if eq(a, "delay") {
            self.usage("missing argument for --", Some(a), None);
        } else {
            self.usage(
                "invalid option --",
                Some(a),
                Some("--decompress, --delay=, --depth=, --dereference, --dereference-files, --dereference-recursive, --devices=, --directories= or --dotall"),
            );
        }
    }

    /// `--NAME [VALUE]` and `--NAME=VALUE` pushed to a C++ list.
    fn list_option(&mut self, a: Z<'a>, name: &str, list: List, i: &mut usize) -> bool {
        if eq(a, name) {
            let v = self.lopt(EMPTY, i);
            self.env.list_push(list, v);
            true
        } else if let Some(v) = a.strip_prefix(name.as_bytes()).and_then(|r| r.strip_prefix(b"=")) {
            let v = self.lopt(v, i);
            self.env.list_push(list, v);
            true
        } else {
            false
        }
    }

    /// `--NAME=VALUE` only (no legacy form) pushed to a C++ list.
    fn list_option_eq(&mut self, a: Z<'a>, prefix: &str, list: List, i: &mut usize) -> bool {
        if let Some(v) = self.lv(a, prefix, i) {
            self.env.list_push(list, v);
            true
        } else {
            false
        }
    }

    fn long_e(&mut self, a: Z<'a>, i: &mut usize) {
        if eq(a, "empty") {
            self.s.flag_empty = 1;
        } else if let Some(v) = self.lv(a, "encoding=", i) {
            self.s.flag_encoding = self.strarg(v);
        } else if self.list_option(a, "exclude", List::Exclude, i)
            || self.list_option(a, "exclude-dir", List::ExcludeDir, i)
            || self.list_option(a, "exclude-from", List::ExcludeFrom, i)
        {
        } else if eq(a, "exclude-fs") {
            self.env.list_push(List::ExcludeFs, EMPTY);
        } else if self.list_option_eq(a, "exclude-fs=", List::ExcludeFs, i) {
        } else if eq(a, "extended-regexp") {
            self.s.flag_mode = MODE_ERE;
        } else if eq(a, "encoding") {
            self.usage("missing argument for --", Some(a), None);
        } else {
            self.usage(
                "invalid option --",
                Some(a),
                Some("--empty, --encoding=, --exclude=, --exclude-dir=, --exclude-from=, --exclude-fs= or --extended-regexp"),
            );
        }
    }

    fn long_f(&mut self, a: Z<'a>, i: &mut usize) {
        if self.list_option(a, "file", List::File, i)
            || self.list_option_eq(a, "file-extension=", List::FileExtension, i)
            || self.list_option_eq(a, "file-magic=", List::FileMagic, i)
            || self.list_option_eq(a, "file-type=", List::FileType, i)
        {
        } else if eq(a, "files") {
            self.s.flag_files = true;
        } else if eq(a, "files-with-matches") {
            self.s.flag_files_with_matches = true;
        } else if eq(a, "files-without-match") {
            self.s.flag_files_without_match = true;
        } else if eq(a, "fixed-strings") {
            self.s.flag_mode = MODE_FIXED;
        } else if let Some(v) = self.lv(a, "filter=", i) {
            self.env.filter_append(v);
        } else if self.list_option_eq(a, "filter-magic-label=", List::FilterMagicLabel, i) {
        } else if let Some(v) = self.lv(a, "format=", i) {
            self.s.flag_format = self.strarg(v);
        } else if starts(a, "format-begin=") {
            self.s.flag_format_begin = self.strarg(&a[13..]);
        } else if starts(a, "format-close=") {
            self.s.flag_format_close = self.strarg(&a[13..]);
        } else if starts(a, "format-end=") {
            self.s.flag_format_end = self.strarg(&a[11..]);
        } else if starts(a, "format-open=") {
            self.s.flag_format_open = self.strarg(&a[12..]);
        } else if self.list_option(a, "from", List::From, i) {
        } else if eq(a, "fuzzy") {
            self.s.flag_fuzzy = 1;
        } else if let Some(v) = self.lv(a, "fuzzy=", i) {
            self.s.flag_fuzzy = self.strtofuzzy(v, "invalid argument --fuzzy=");
        } else if eq(a, "free-space") {
            self.s.flag_free_space = 1;
        } else if [
            "file-extension",
            "file-magic",
            "file-type",
            "filter",
            "filter-magic-label",
            "format",
            "format-begin",
            "format-close",
            "format-end",
            "format-open",
        ]
        .iter()
        .any(|n| eq(a, n))
        {
            self.usage("missing argument for --", Some(a), None);
        } else {
            self.usage(
                "invalid option --",
                Some(a),
                Some("--file=, --file-extension=, --file-magic=, --file-type=, --files, --files-with-matches, --files-without-match, --fixed-strings, --filter=, --filter-magic-label=, --format=, --format-begin=, --format-close, --format-end=, --format-open=, --fuzzy or --free-space"),
            );
        }
    }

    fn long_g(&mut self, a: Z<'a>, i: &mut usize) {
        if self.list_option_eq(a, "glob=", List::Glob, i) {
        } else if eq(a, "glob-ignore-case") {
            self.s.flag_glob_ignore_case = true;
        } else if eq(a, "grep") {
            self.s.flag_grep = true;
            self.s.flag_empty = 1;
        } else if eq(a, "group-separator") {
            self.s.flag_group_separator = c"--".as_ptr();
        } else if starts(a, "group-separator=") {
            self.s.flag_group_separator = self.strarg(&a[16..]);
        } else if eq(a, "glob") {
            self.usage("missing argument for --", Some(a), None);
        } else {
            self.usage("invalid option --", Some(a), Some("--glob=, --glob-ignore-case, --grep or --group-separator"));
        }
    }

    fn long_h(&mut self, a: Z<'a>, i: &mut usize) {
        if eq(a, "heading") {
            self.s.flag_heading = 1;
        } else if starts(a, "help") {
            if at(a, 4) != 0 {
                self.env.help(Some(&a[4..]));
            } else {
                *i += 1;
                if *i < self.argv.len() {
                    self.env.help(Some(self.argv[*i]));
                } else {
                    self.env.help(None);
                }
            }
        } else if eq(a, "hex") {
            self.s.flag_binary_files = c"hex".as_ptr();
        } else if eq(a, "hexdump") {
            self.s.flag_hexdump = c"2".as_ptr();
        } else if starts(a, "hexdump=") {
            self.s.flag_hexdump = self.strarg(&a[8..]);
        } else if eq(a, "hidden") {
            self.s.flag_hidden = true;
        } else if starts(a, "hyperlink=") {
            self.s.flag_hyperlink = self.strarg(&a[10..]);
        } else if eq(a, "hyperlink") {
            self.s.flag_hyperlink = c"".as_ptr();
        } else {
            self.usage("invalid option --", Some(a), Some("--heading, --help, --hex, --hexdump, --hidden or --hyperlink"));
        }
    }

    fn long_i(&mut self, a: Z<'a>, i: &mut usize) {
        if self.list_option_eq(a, "iglob=", List::Iglob, i) {
        } else if eq(a, "ignore-binary") {
            self.s.flag_binary_files = c"without-match".as_ptr();
        } else if eq(a, "ignore-case") {
            self.s.flag_ignore_case = 1;
        } else if eq(a, "ignore-files") {
            let f = self.k.default_ignore_file;
            self.env.list_push(List::IgnoreFiles, f);
        } else if self.list_option_eq(a, "ignore-files=", List::IgnoreFiles, i)
            || self.list_option(a, "include", List::Include, i)
            || self.list_option(a, "include-dir", List::IncludeDir, i)
            || self.list_option(a, "include-from", List::IncludeFrom, i)
        {
        } else if eq(a, "include-fs") {
            self.env.list_push(List::IncludeFs, EMPTY);
        } else if self.list_option_eq(a, "include-fs=", List::IncludeFs, i) {
        } else if eq(a, "index") {
            self.s.flag_index = c"safe".as_ptr();
        } else if let Some(v) = self.lv(a, "index=", i) {
            self.s.flag_index = self.strarg(v);
        } else if eq(a, "initial-tab") {
            self.s.flag_initial_tab = 1;
        } else if eq(a, "invert-match") {
            self.s.flag_invert_match = true;
        } else {
            self.usage(
                "invalid option --",
                Some(a),
                Some("--iglob=, --ignore-case, --ignore-files, --include=, --include-dir=, --include-from=, --include-fs=, --initial-tab or --invert-match"),
            );
        }
    }

    fn long_l(&mut self, a: Z<'a>, i: &mut usize) {
        if eq(a, "label") {
            let v = self.lopt(EMPTY, i);
            self.s.flag_label = self.strarg(v);
        } else if starts(a, "label=") {
            self.s.flag_label = self.strarg(&a[6..]);
        } else if eq(a, "line-buffered") {
            self.s.flag_line_buffered = true;
        } else if eq(a, "line-number") {
            self.s.flag_line_number = 1;
        } else if eq(a, "line-regexp") {
            self.s.flag_line_regexp = true;
        } else if eq(a, "lines") {
            self.s.flag_files = false;
        } else {
            self.usage(
                "invalid option --",
                Some(a),
                Some("--label=, --line-buffered, --line-number, --line-regexp or --lines"),
            );
        }
    }

    fn long_m(&mut self, a: Z<'a>, i: &mut usize) {
        if eq(a, "match") {
            self.s.flag_match = true;
        } else if let Some(v) = self.lv(a, "max-count=", i) {
            self.s.flag_max_count = self.strtopos(v, "invalid argument --max-count=");
        } else if let Some(v) = self.lv(a, "max-depth=", i) {
            self.s.flag_max_depth = self.strtopos(v, "invalid argument --max-depth=");
        } else if let Some(v) = self.lv(a, "max-files=", i) {
            self.s.flag_max_files = self.strtopos(v, "invalid argument --max-files=");
        } else if let Some(v) = self.lv(a, "max-line=", i) {
            self.s.flag_max_line = self.strtopos(v, "invalid argument --max-line=");
        } else if let Some(v) = self.lv(a, "max-size=", i) {
            self.s.flag_max_size = self.strtosize(v, "invalid argument --max-size=");
        } else if let Some(v) = self.lv(a, "max-queue=", i) {
            self.s.flag_max_queue = self.strtopos(v, "invalid argument --max-queue=");
        } else if let Some(v) = self.lv(a, "min-count=", i) {
            self.s.flag_min_count = self.strtopos(v, "invalid argument --min-count=");
        } else if let Some(v) = self.lv(a, "min-depth=", i) {
            self.s.flag_min_depth = self.strtopos(v, "invalid argument --min-depth=");
        } else if let Some(v) = self.lv(a, "min-line=", i) {
            self.s.flag_min_line = self.strtopos(v, "invalid argument --min-line=");
        } else if let Some(v) = self.lv(a, "min-size=", i) {
            self.s.flag_min_size = self.strtosize(v, "invalid argument --min-size=");
        } else if let Some(v) = self.lv(a, "min-steal=", i) {
            self.s.flag_min_steal = self.strtopos(v, "invalid argument --min-steal=");
        } else if eq(a, "mmap") {
            self.s.flag_max_mmap = self.k.max_mmap_size;
        } else if let Some(v) = self.lv(a, "mmap=", i) {
            self.s.flag_max_mmap = self.strtopos(v, "invalid argument --mmap=");
        } else if eq(a, "messages") {
            self.s.flag_no_messages = false;
        } else if [
            "max-count",
            "max-depth",
            "max-files",
            "max-line",
            "max-size",
            "min-count",
            "min-depth",
            "min-line",
            "min-size",
        ]
        .iter()
        .any(|n| eq(a, n))
        {
            self.usage("missing argument for --", Some(a), None);
        } else {
            self.usage(
                "invalid option --",
                Some(a),
                Some("--match, --max-count=, --max-depth=, --max-files=, --max-line=, --max-size=--min-count=, --min-depth=, --min-line=, --min-size=, --mmap or --messages"),
            );
        }
    }

    fn long_n(&mut self, a: Z<'a>, i: &mut usize) {
        if let Some(v) = self.lv(a, "neg-regexp=", i) {
            self.option_regexp(v, true);
        } else if eq(a, "not") {
            self.option_not_next(i);
        } else if let Some(v) = self.lv(a, "not=", i) {
            self.option_not(v);
        } else if eq(a, "neg-regexp") {
            self.usage("missing argument for --", Some(a), None);
        } else if !self.long_no(a) {
            self.usage(
                "invalid option --",
                Some(a),
                Some("--neg-regexp=, --not, --no-any-line, --no-ascii, --no-binary, --no-bool, --no-break, --no-byte-offset, --no-color, --no-config, --no-confirm, --no-count, --no-decompress, --no-dereference, --no-dereference-files, --no-dotall, --no-encoding, --no-empty, --no-filename, --no-files-with-matches, --no-filter, --no-glob-ignore-case, --no-group-separator, --no-heading, --no-hidden, --no-hyperlink, --no-ignore-binary, --no-ignore-case, --no-ignore-files, --no-index, --no-initial-tab, --no-invert-match, --no-line-number, --no-only-line-number, --no-only-matching, --no-messages, --no-mmap, --no-pager, --no-pretty, --no-smart-case, --no-sort, --no-split, --no-stats, --no-tree, --no-ungroup, --no-view, --null or --null-data"),
            );
        }
    }

    /// The `--no-...`, `--null` and `--null-data` options; false when `a` is none of them.
    fn long_no(&mut self, a: Z<'a>) -> bool {
        let null = std::ptr::null();
        match body(a) {
            b"no-any-line" | b"no-passthru" => self.s.flag_any_line = false,
            b"no-ascii" | b"no-binary" => self.s.flag_binary = 0,
            b"no-bool" => self.s.flag_bool = false,
            b"no-break" => self.s.flag_break = 0,
            b"no-byte-offset" => self.s.flag_byte_offset = 0,
            b"no-color" | b"no-colour" => self.s.flag_color = self.k.never,
            b"no-column-number" => self.s.flag_column_number = 0,
            b"no-config" => {}
            b"no-confirm" => self.s.flag_confirm = false,
            b"no-count" => self.s.flag_count = false,
            b"no-decompress" => self.s.flag_decompress = false,
            b"no-dereference" => self.s.flag_no_dereference = true,
            b"no-dereference-files" => self.s.flag_dereference_files = false,
            b"no-dotall" => self.s.flag_dotall = 0,
            b"no-empty" => self.s.flag_empty = 0,
            b"no-encoding" => self.s.flag_encoding = null,
            b"no-filename" => self.s.flag_no_filename = true,
            b"no-files-with-matches" => self.s.flag_files_with_matches = false,
            b"no-filter" => self.env.filter_clear(),
            b"no-glob-ignore-case" => self.s.flag_glob_ignore_case = false,
            b"no-group-separator" => self.s.flag_group_separator = null,
            b"no-heading" => self.s.flag_heading = 0,
            b"no-hidden" => self.s.flag_hidden = false,
            b"no-hyperlink" => self.s.flag_hyperlink = null,
            b"no-ignore-binary" => self.s.flag_binary_files = c"binary".as_ptr(),
            b"no-ignore-case" => self.s.flag_ignore_case = 0,
            b"no-ignore-files" => self.env.list_clear(List::IgnoreFiles),
            b"no-index" => self.s.flag_index = null,
            b"no-initial-tab" => self.s.flag_initial_tab = 0,
            b"no-invert-match" => self.s.flag_invert_match = false,
            b"no-line-number" => self.s.flag_line_number = 0,
            b"no-only-line-number" => self.s.flag_only_line_number = false,
            b"no-only-matching" => self.s.flag_only_matching = false,
            b"no-messages" => self.s.flag_no_messages = true,
            b"no-mmap" => self.s.flag_max_mmap = 0,
            b"no-pager" => self.s.flag_pager = null,
            b"no-pretty" => self.s.flag_pretty = null,
            b"no-smart-case" => self.s.flag_smart_case = 0,
            b"no-sort" => self.s.flag_sort = null,
            b"no-split" => self.s.flag_split = false,
            b"no-tree" => self.s.flag_tree = 0,
            b"no-stats" => self.s.flag_stats = null,
            b"no-ungroup" => self.s.flag_ungroup = 0,
            b"no-view" => self.s.flag_view = null,
            b"null" => self.s.flag_null = true,
            b"null-data" => self.s.flag_null_data = true,
            _ => return false,
        }
        true
    }

    fn long_p(&mut self, a: Z<'a>, i: &mut usize) {
        if eq(a, "pager") {
            self.s.flag_pager = c"".as_ptr();
        } else if let Some(v) = self.lv(a, "pager=", i) {
            self.s.flag_pager = self.strarg(v);
        } else if eq(a, "passthru") {
            self.s.flag_any_line = true;
        } else if eq(a, "perl-regexp") {
            self.s.flag_mode = MODE_PERL;
        } else if eq(a, "pretty") {
            self.s.flag_pretty = self.k.auto;
        } else if let Some(v) = self.lv(a, "pretty=", i) {
            self.s.flag_pretty = self.strarg(v);
        } else {
            self.usage("invalid option --", Some(a), Some("--pager, --passthru, --perl-regexp= or --pretty"));
        }
    }

    fn long_r(&mut self, a: Z<'a>, i: &mut usize) {
        if let Some(v) = self.lv(a, "range=", i) {
            if let Some((min, max)) = self.strtopos2(v, "invalid argument --range=") {
                self.s.flag_min_line = min;
                self.s.flag_max_line = max;
            }
        } else if eq(a, "recursive") {
            self.s.flag_directories = c"recurse".as_ptr();
        } else if eq(a, "regexp") {
            let v = self.lopt(EMPTY, i);
            self.option_regexp(v, false);
        } else if let Some(v) = self.lv(a, "regexp=", i) {
            self.option_regexp(v, false);
        } else if let Some(v) = self.lv(a, "replace=", i) {
            self.s.flag_replace = self.strarg(v);
        } else if eq(a, "range") || eq(a, "replace") {
            self.usage("missing argument for --", Some(a), None);
        } else {
            self.usage("invalid option --", Some(a), Some("--range=, --recursive, --regexp= or --replace="));
        }
    }

    fn long_s(&mut self, a: Z<'a>, i: &mut usize) {
        if eq(a, "save-config") {
            self.s.flag_save_config = c".ugrep".as_ptr();
        } else if let Some(v) = self.lv(a, "save-config=", i) {
            self.s.flag_save_config = self.strarg(v);
        } else if eq(a, "separator") {
            self.s.flag_separator = std::ptr::null();
        } else if starts(a, "separator=") {
            self.s.flag_separator = self.strarg(&a[10..]);
        } else if eq(a, "silent") {
            self.s.flag_quiet = true;
            self.s.flag_no_messages = true;
        } else if eq(a, "smart-case") {
            self.s.flag_smart_case = 1;
        } else if eq(a, "sort") {
            self.s.flag_sort = c"name".as_ptr();
        } else if let Some(v) = self.lv(a, "sort=", i) {
            self.s.flag_sort = self.strarg(v);
        } else if eq(a, "split") {
            self.s.flag_split = true;
        } else if eq(a, "stats") {
            self.s.flag_stats = c"".as_ptr();
        } else if starts(a, "stats=") {
            self.s.flag_stats = self.strarg(&a[6..]);
        } else {
            self.usage(
                "invalid option --",
                Some(a),
                Some("--save-config, --separator, --silent, --smart-case, --sort, --split or --stats"),
            );
        }
    }

    fn long_t(&mut self, a: Z<'a>, i: &mut usize) {
        if eq(a, "tabs") {
            self.s.flag_tabs = self.k.default_tabs;
        } else if let Some(v) = self.lv(a, "tabs=", i) {
            self.s.flag_tabs = self.strtopos(v, "invalid argument --tabs=");
        } else if eq(a, "tag") {
            self.s.flag_tag = self.k.default_tag;
        } else if let Some(v) = self.lv(a, "tag=", i) {
            self.s.flag_tag = self.strarg(v);
        } else if eq(a, "text") {
            self.s.flag_binary_files = c"text".as_ptr();
        } else if eq(a, "tree") {
            self.s.flag_tree = 1;
        } else {
            self.usage("invalid option --", Some(a), Some("--tabs, --tag, --text or --tree"));
        }
    }

    /// One short option letter `s[*k]` of a cluster.
    fn short_option(&mut self, s: Z<'a>, k: &mut usize, i: &mut usize, grouped: &mut bool) {
        let c = at(s, *k);
        match c {
            b'A' => {
                let v = self.sopt(s, *k, i);
                self.s.flag_after_context = self.strtonum(v, "invalid argument -A=");
                *grouped = false;
            }
            b'a' => self.s.flag_binary_files = c"text".as_ptr(),
            b'B' => {
                let v = self.sopt(s, *k, i);
                self.s.flag_before_context = self.strtonum(v, "invalid argument -B=");
                *grouped = false;
            }
            b'b' => self.s.flag_byte_offset = 1,
            b'C' => {
                let v = self.sopt(s, *k, i);
                let n = self.strtonum(v, "invalid argument -C=");
                self.s.flag_before_context = n;
                self.s.flag_after_context = n;
                *grouped = false;
            }
            b'c' => self.s.flag_count = true,
            b'D' => {
                self.s.flag_devices = ptr(self.sopt(s, *k, i));
                *grouped = false;
            }
            b'd' => {
                self.s.flag_directories = ptr(self.sopt(s, *k, i));
                *grouped = false;
            }
            b'E' => self.s.flag_mode = MODE_ERE,
            b'e' => {
                let v = self.sopt(s, *k, i);
                self.option_regexp(v, false);
                *grouped = false;
            }
            b'F' => self.s.flag_mode = MODE_FIXED,
            b'f' => {
                let v = self.sopt(s, *k, i);
                self.env.list_push(List::File, v);
                *grouped = false;
            }
            b'G' => self.s.flag_mode = MODE_BRE,
            b'g' => {
                let v = self.sopt(s, *k, i);
                self.env.list_push(List::Glob, v);
                *grouped = false;
            }
            b'H' => self.s.flag_with_filename = true,
            b'h' => self.s.flag_no_filename = true,
            b'I' => self.s.flag_binary_files = c"without-match".as_ptr(),
            b'i' => self.s.flag_ignore_case = 1,
            b'J' => {
                let v = self.sopt(s, *k, i);
                self.s.flag_jobs = self.strtonum(v, "invalid argument -J=");
                *grouped = false;
            }
            b'j' => self.s.flag_smart_case = 1,
            b'K' => {
                let v = self.sopt(s, *k, i);
                if let Some((min, max)) = self.strtopos2(v, "invalid argument -K=") {
                    self.s.flag_min_line = min;
                    self.s.flag_max_line = max;
                }
                *grouped = false;
            }
            b'k' => self.s.flag_column_number = 1,
            b'L' => self.s.flag_files_without_match = true,
            b'l' => self.s.flag_files_with_matches = true,
            b'M' => {
                let v = self.sopt(s, *k, i);
                self.env.list_push(List::FileMagic, v);
                *grouped = false;
            }
            b'm' => {
                let v = self.sopt(s, *k, i);
                if let Some((min, max)) = self.strtopos2(v, "invalid argument -m=") {
                    self.s.flag_min_count = min;
                    self.s.flag_max_count = max;
                }
                *grouped = false;
            }
            b'N' => {
                let v = self.sopt(s, *k, i);
                self.option_regexp(v, true);
                *grouped = false;
            }
            b'n' => self.s.flag_line_number = 1,
            b'O' => {
                let v = self.sopt(s, *k, i);
                self.env.list_push(List::FileExtension, v);
                *grouped = false;
            }
            b'o' => self.s.flag_only_matching = true,
            b'P' => self.s.flag_mode = MODE_PERL,
            b'p' => self.s.flag_no_dereference = true,
            b'Q' => {
                if at(s, *k + 1) == b'=' {
                    *k += 1;
                    self.s.flag_delay = self.strtonum(&s[*k + 1..], "invalid argument -Q=");
                    self.s.flag_query = true;
                    *grouped = false;
                } else {
                    self.s.flag_query = true;
                }
            }
            b'q' => {
                self.s.flag_quiet = true;
                self.s.flag_no_messages = true;
            }
            b'R' => self.s.flag_directories = c"dereference-recurse".as_ptr(),
            b'r' => self.s.flag_directories = c"recurse".as_ptr(),
            b'S' => self.s.flag_dereference_files = true,
            b's' => self.s.flag_no_messages = true,
            b'T' => self.s.flag_initial_tab = 1,
            b't' => {
                let v = self.sopt(s, *k, i);
                self.env.list_push(List::FileType, v);
                *grouped = false;
            }
            b'U' => self.s.flag_binary = 1,
            b'u' => self.s.flag_ungroup = 1,
            b'V' => self.env.version(),
            b'v' => self.s.flag_invert_match = true,
            b'W' => self.s.flag_binary_files = c"with-hex".as_ptr(),
            b'w' => self.s.flag_word_regexp = true,
            b'X' => self.s.flag_binary_files = c"hex".as_ptr(),
            b'x' => self.s.flag_line_regexp = true,
            b'Y' => self.s.flag_empty = 1,
            b'y' => self.s.flag_any_line = true,
            b'Z' => {
                if self.s.flag_grep {
                    self.s.flag_null = true;
                } else {
                    *k += 1;
                    let c = at(s, *k);
                    // strchr("+-~", c) also finds the terminating NUL
                    if c == b'=' || starts(&s[*k..], "best") || is_digit(c) || c == 0 || b"+-~".contains(&c) {
                        let from = *k + usize::from(c == b'=');
                        self.s.flag_fuzzy = self.strtofuzzy(&s[from..], "invalid argument -Z=");
                        *grouped = false;
                    } else {
                        self.s.flag_fuzzy = 1;
                        *k -= 1;
                    }
                }
            }
            b'z' => {
                if self.s.flag_grep {
                    self.s.flag_null_data = true;
                } else {
                    self.s.flag_decompress = true;
                }
            }
            b'0' => {
                self.s.flag_null_data = self.s.flag_null;
                self.s.flag_null = !self.s.flag_null;
            }
            b'1'..=b'9' => {
                let end = self.set_depth(s, *k);
                // C++: arg = range - 1, then the loop pre-increments
                *k = end - 1;
            }
            b'?' => {
                if at(s, *k + 1) != 0 {
                    self.env.help(Some(&s[*k + 1..]));
                } else {
                    *i += 1;
                    if *i < self.argv.len() {
                        self.env.help(Some(self.argv[*i]));
                    } else {
                        self.env.help(None);
                    }
                }
            }
            b'%' => {
                if self.s.flag_bool {
                    self.s.flag_files = true;
                }
                self.s.flag_bool = true;
            }
            b'^' => self.s.flag_tree = 1,
            b'+' => self.s.flag_heading = 1,
            b'.' => self.s.flag_hidden = true,
            b'@' => self.option_all_files(),
            _ => self.usage("invalid option -", Some(&s[*k..]), None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Mock {
        usage: Vec<String>,
        patterns: Vec<(i32, Vec<u8>)>,
        lists: Vec<(List, Vec<u8>)>,
        cleared: Vec<List>,
        arg_pattern: Option<Vec<u8>>,
        files: Vec<Vec<u8>>,
        strs: Vec<Vec<u8>>,
        helps: Vec<Option<Vec<u8>>>,
    }

    impl Env for Mock {
        fn usage(&mut self, message: &str, arg: Option<&[u8]>, valid: Option<&str>) {
            self.usage.push(format!("{message}|{}|{}", String::from_utf8_lossy(arg.unwrap_or(b"")), valid.unwrap_or("")));
        }
        fn help(&mut self, what: Option<Z<'_>>) {
            self.helps.push(what.map(|w| body(w).to_vec()));
        }
        fn version(&mut self) {}
        fn strarg(&mut self, string: Z<'_>) -> *const c_char {
            self.strs.push(body(string).to_vec());
            ptr(string)
        }
        fn pattern_push(&mut self, mask: i32, arg: Z<'_>) {
            self.patterns.push((mask, body(arg).to_vec()));
        }
        fn patterns_empty(&mut self) -> bool {
            self.patterns.is_empty()
        }
        fn list_push(&mut self, list: List, value: Z<'_>) {
            self.lists.push((list, body(value).to_vec()));
        }
        fn list_clear(&mut self, list: List) {
            self.cleared.push(list);
            self.lists.retain(|(l, _)| *l != list);
        }
        fn list_empty(&mut self, list: List) -> bool {
            !self.lists.iter().any(|(l, _)| *l == list)
        }
        fn filter_append(&mut self, _value: Z<'_>) {}
        fn filter_clear(&mut self) {}
        fn arg_pattern_is_null(&mut self) -> bool {
            self.arg_pattern.is_none()
        }
        fn set_arg_pattern(&mut self, arg: Z<'_>) {
            self.arg_pattern = Some(body(arg).to_vec());
        }
        fn push_arg_file(&mut self, arg: Z<'_>) {
            self.files.push(body(arg).to_vec());
        }
        fn screen_width(&mut self) -> usize {
            80
        }
        fn c_string(&mut self, _p: *const c_char) -> Vec<u8> {
            b"binary".to_vec()
        }
    }

    fn consts() -> Consts {
        Consts {
            auto: c"auto".as_ptr(),
            never: c"never".as_ptr(),
            max_mmap_size: 1 << 30,
            default_tabs: 8,
            default_tag: c"___".as_ptr(),
            default_ignore_file: b".gitignore\0",
            os_win: false,
        }
    }

    fn run(args: &[&str]) -> (Box<Settings>, Mock) {
        let owned: Vec<Vec<u8>> = std::iter::once("ugrep".to_string())
            .chain(args.iter().map(|s| s.to_string()))
            .map(|s| {
                let mut v = s.into_bytes();
                v.push(0);
                v
            })
            .collect();
        let argv: Vec<Z<'_>> = owned.iter().map(|v| v.as_slice()).collect();
        let mut s = Box::new(Settings::DEFAULT);
        let mut env = Mock::default();
        options(&mut s, &mut env, consts(), &argv);
        (s, env)
    }

    #[test]
    fn strtoull_follows_c() {
        assert_eq!(strtoull10(b"12x\0"), (12, 2));
        assert_eq!(strtoull10(b"x\0"), (0, 0));
        assert_eq!(strtoull10(b"-1\0"), (u64::MAX, 2));
        assert_eq!(strtoull10(b"99999999999999999999999\0").0, u64::MAX);
        assert_eq!(strtoull10(b"  7\0"), (7, 3));
    }

    #[test]
    fn short_cluster_with_attached_argument() {
        let (s, env) = run(&["-nHm1", "pat", "file"]);
        assert_eq!(s.flag_line_number, 1);
        assert!(s.flag_with_filename);
        assert_eq!((s.flag_min_count, s.flag_max_count), (0, 1));
        assert_eq!(env.arg_pattern.as_deref(), Some(&b"pat"[..]));
        assert_eq!(env.files, vec![b"file".to_vec()]);
        assert!(env.usage.is_empty());
    }

    #[test]
    fn depth_and_ranges() {
        let (s, _) = run(&["-2,4"]);
        assert_eq!((s.flag_min_depth, s.flag_max_depth), (2, 4));
        let (s, env) = run(&["--depth=3", "x"]);
        assert_eq!(s.flag_max_depth, 3);
        assert!(env.usage.is_empty());
        let (_, env) = run(&["--depth=3x"]);
        assert_eq!(env.usage, vec!["invalid argument --depth=|3x|".to_string()]);
    }

    #[test]
    fn and_not_patterns() {
        let (s, env) = run(&["-e", "a", "--and", "b", "--not", "c"]);
        let kinds: Vec<i32> = env.patterns.iter().map(|p| p.0).collect();
        assert_eq!(kinds, vec![PATTERN_ALT, PATTERN_AND, PATTERN_ALT, PATTERN_NOT]);
        assert!(!s.flag_not);
        let (_, env) = run(&["--not"]);
        assert_eq!(env.usage, vec!["missing PATTERN for --not||".to_string()]);
    }

    #[test]
    fn invalid_option_reports_candidates() {
        let (_, env) = run(&["--ignore-c"]);
        assert_eq!(env.usage.len(), 1);
        assert!(env.usage[0].starts_with("invalid option --|ignore-c|--iglob=,"));
        let (_, env) = run(&["-@@", "--all"]);
        assert_eq!(env.cleared.len(), 36);
    }

    #[test]
    fn fuzzy_and_sizes() {
        let (s, _) = run(&["-Z+2"]);
        assert_eq!(s.flag_fuzzy, 2 | FUZZY_INS);
        let (s, _) = run(&["--max-size=2k"]);
        assert_eq!(s.flag_max_size, 2048);
        let (s, env) = run(&["-Z", "pat"]);
        assert_eq!(s.flag_fuzzy, 1);
        assert_eq!(env.arg_pattern.as_deref(), Some(&b"pat"[..]));
    }

    #[test]
    fn double_dash_ends_options() {
        let (s, env) = run(&["--", "-n", "-"]);
        assert_eq!(s.flag_line_number, -1);
        assert_eq!(env.arg_pattern.as_deref(), Some(&b"-n"[..]));
        assert!(s.flag_stdin);
    }
}
