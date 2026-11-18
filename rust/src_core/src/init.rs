//! `init(argc, argv)` of src/ugrep.cpp (formerly `init_cpp`): the `--config` pre-scan, the program name
//! defaults (`ug`, `grep`, `egrep`, ...), the call of `options()`, and the normalization and validation of the
//! parsed options (`-t`, `--encoding`, `--binary-files`, `--hexdump`, FILE arguments, `--format`, `--*-fs`,
//! `--ignore-file`, `-O`, `-M`, `--filter-magic-label`, ...).
//!
//! The scalar option state is the Rust-owned [`Settings`]. What stays in C++ (the `std::string` and container
//! options, `Static::arg_pattern`/`arg_files`, the CNF, `load_config`, `save_config`, `options`, `terminal`,
//! `usage`/`warning`/`error`/`abort`, `exit`, the type and encoding tables, the reflex magic patterns, file
//! access) is reached through the [`Env`] trait; `ffi/init.rs` implements it with a table of C callbacks.
//!
//! The settings are never held across a call of `Env`: a callback may re-enter Rust (`load_config` and
//! `options` run the option parser on the same settings) or read them from C++, so every access takes a fresh
//! borrow from [`Env::settings`]. Every callback that ends the process in C++ (`exit`, `usage`, `error`,
//! `abort`) is followed by a `return` here, so the logic after it never runs in tests either.
//! `usage` returns only while a configuration file is read (`flag_usage_warnings`), which is over before
//! `init` reaches any of its calls, so a `return` after a `usage` is only used where the C++ would read past
//! the end of a table.

use crate::options::{strtoull10, List, PATTERN_ALT, PATTERN_AND, PATTERN_NEG, PATTERN_NOT};
use crate::settings::Settings;
use std::os::raw::c_char;

/// A NUL-terminated byte string (the terminator is the last byte).
pub type Z<'a> = &'a [u8];

const EMPTY: Z<'static> = b"\0";

/// `EXIT_OK` and `EXIT_ERROR`.
pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 2;

/// `Mode` and `Action` values of src/flag.hpp.
const MODE_BRE: i32 = 2;
const MODE_FIXED: i32 = 3;
const MODE_PERL: i32 = 4;
const ACTION_UNSP: i32 = 0;
const ACTION_SKIP: i32 = 1;
const ACTION_READ: i32 = 2;
const ACTION_RECURSE: i32 = 3;

/// Build-dependent values and C++ constants.
#[derive(Clone, Copy)]
pub struct Consts {
    /// `Static::AUTO` (the tri-state is compared by address elsewhere, so the same pointer is stored).
    pub auto: *const c_char,
    /// `PATHSEPCHR`
    pub pathsep: u8,
    /// `OS_WIN`
    pub os_win: bool,
    /// `OS_WIN_OR_MINGW`
    pub os_win_or_mingw: bool,
    /// `HAVE_LIBZ`
    pub have_libz: bool,
    /// `HAVE_PCRE2` or `HAVE_BOOST_REGEX`
    pub have_regex_perl: bool,
    /// `HAVE_STATVFS` or `HAVE_STATFS`
    pub have_stat_fs: bool,
    /// `__CYGWIN__`
    pub cygwin: bool,
    /// `MAX_HEX_COLUMNS`
    pub max_hex_columns: usize,
    /// `reflex::Input::file_encoding::null_data`
    pub encoding_null_data: u16,
}

/// One row of `type_table[]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeEntry {
    pub name: Vec<u8>,
    pub extensions: Vec<u8>,
    pub filenames: Option<Vec<u8>>,
    pub magic: Option<Vec<u8>>,
}

/// What the check of one FILE argument found (the C++ `stat`, `lstat` or `GetFileAttributesW` part).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArgFile {
    /// Does not exist (`errno` is set for the warning).
    Missing,
    /// `-p` and the argument is a symbolic link.
    Symlink,
    /// Not readable (`WITH_WARN_UNREADABLE_FILE_ARG`, `errno` is `EACCES`).
    Unreadable,
    Directory,
    Other,
}

/// What `init` needs from the retained C++ side.
pub trait Env {
    /// A fresh borrow of the settings; never kept across another call of the environment.
    fn settings(&mut self) -> &mut Settings;
    /// The bytes of a NUL-terminated string a settings field or argv entry points to (empty for NULL).
    fn c_string(&mut self, p: *const c_char) -> Vec<u8>;
    /// `Static::home_dir = getenv("HOME")` (`USERPROFILE` on Windows).
    fn set_home_dir(&mut self);
    /// Bytes to `std::cerr`.
    fn cerr(&mut self, text: &[u8]);
    /// `exit(code)`.
    fn exit(&mut self, code: i32);
    /// `usage(message)`: returns only when `flag_usage_warnings` is set.
    fn usage(&mut self, message: &[u8]);
    /// `warning(message, arg)`, reads `errno`.
    fn warning(&mut self, message: Option<&[u8]>, arg: &[u8]);
    /// `error(message, arg)` then exit, reads `errno`.
    fn error(&mut self, message: &[u8], arg: &[u8]);
    /// `abort(message)` then exit.
    fn abort(&mut self, message: &[u8]);
    /// `abort(message, what)` then exit.
    fn abort_what(&mut self, message: &[u8], what: &[u8]);
    /// `load_config(pattern_args)`
    fn load_config(&mut self);
    /// `save_config()`
    fn save_config(&mut self);
    /// `Static::warnings = 0` and `Static::warnings`.
    fn warnings_reset(&mut self);
    fn warnings(&mut self) -> usize;
    /// `options(pattern_args, argc, argv)`
    fn options(&mut self);
    /// `terminal()`
    fn terminal(&mut self);
    /// The `pattern_args` list: mask and a pointer into argv.
    fn patterns(&mut self) -> Vec<(i32, *const c_char)>;
    /// `Static::bcnf.new_pattern(mask, arg)`, `new_pattern(ALT, flag_regexp.c_str())` and `new_term()`.
    fn bcnf_new_pattern(&mut self, mask: i32, arg: *const c_char);
    fn bcnf_new_pattern_regexp(&mut self);
    fn bcnf_new_term(&mut self);
    /// `flag_regexp.append(text)`
    fn regexp_append(&mut self, text: &[u8]);
    /// A copy of the strings of a list option (for `List::IgnoreFiles`, in set order).
    fn list_all(&mut self, list: List) -> Vec<Vec<u8>>;
    /// `list.emplace_back(value)`
    fn list_push(&mut self, list: List, value: &[u8]);
    /// `list[index].resize(len)`
    fn list_truncate(&mut self, list: List, index: usize, len: usize);
    /// `flag_ignore_files.erase(name)`
    fn ignore_file_erase(&mut self, name: &[u8]);
    /// `Static::arg_pattern` (NULL when unset) and its assignment.
    fn arg_pattern(&mut self) -> *const c_char;
    fn set_arg_pattern(&mut self, p: *const c_char);
    /// `Static::arg_files`
    fn arg_files_len(&mut self) -> usize;
    fn arg_file(&mut self, i: usize) -> *const c_char;
    fn arg_files_erase(&mut self, i: usize);
    fn arg_files_insert_front(&mut self, p: *const c_char);
    /// Windows: expand wildcards in the arguments of `Static::arg_files`; nothing elsewhere.
    fn expand_arg_files(&mut self);
    /// Check one FILE argument, with `lstat` when `no_dereference`.
    fn check_arg_file(&mut self, p: *const c_char, no_dereference: bool) -> ArgFile;
    /// `trim_pathname_arg(p)`: modifies the argv string in place.
    fn trim_pathname_arg(&mut self, p: *const c_char);
    /// `fopen_smart(&file, name, "r")` and a buffered reader on it; false when it fails (`errno` is set).
    fn file_open(&mut self, name: &[u8]) -> bool;
    /// `getline` on the open file: true at end of file and the length of the line read.
    fn file_getline(&mut self) -> (bool, usize);
    /// Swap the line read into `arg_strings` and push it to `Static::arg_files`.
    fn arg_files_push_line(&mut self);
    /// `if (file != stdin) fclose(file)`
    fn file_close(&mut self);
    /// `import_globs(file, flag_exclude, flag_exclude_dir, gitignore)` or the include pair.
    fn import_globs(&mut self, include: bool, gitignore: bool);
    /// `stat_fs(path)`: the file system id, `None` when the call fails (`errno` is set).
    fn fs_probe(&mut self, path: &[u8]) -> Option<u64>;
    /// `include_fs_ids.insert(id)` or `exclude_fs_ids.insert(id)`.
    fn fs_insert(&mut self, include: bool, id: u64);
    /// `type_table[i]`, `None` at the terminating row.
    fn type_entry(&mut self, i: usize) -> Option<TypeEntry>;
    /// `encoding_table[i]`: format and encoding, `None` at the terminating row.
    fn encoding_entry(&mut self, i: usize) -> Option<(Vec<u8>, u16)>;
    /// `Static::magic_pattern.assign(regex, "r")` when given, then `Static::magic_matcher.pattern(...)`;
    /// the `regex_error` text on failure.
    fn magic_pattern_assign(&mut self, regex: Option<&[u8]>) -> Option<Vec<u8>>;
    /// `Static::filter_magic_pattern.assign(regex, "r")`; the `regex_error` text on failure.
    fn filter_magic_assign(&mut self, regex: &[u8]) -> Option<Vec<u8>>;
    /// `flag_filter.empty()`
    fn filter_empty(&mut self) -> bool;
    /// `isatty(STDIN_FILENO)`
    fn isatty_stdin(&mut self) -> bool;
    /// Windows: `_setmode(fileno(stdout), _O_BINARY)`; nothing elsewhere.
    fn set_binary_stdout(&mut self);
}

fn at(s: &[u8], k: usize) -> u8 {
    s.get(k).copied().unwrap_or(0)
}

fn body(z: Z<'_>) -> &[u8] {
    z.strip_suffix(b"\0").unwrap_or(z)
}

fn eq(a: Z<'_>, lit: &str) -> bool {
    body(a) == lit.as_bytes()
}

/// `strncmp(program, lit, len) == 0`
fn strncmp_eq(program: &[u8], lit: &str, len: usize) -> bool {
    let lit = lit.as_bytes();
    for k in 0..len {
        let (a, b) = (at(program, k), at(lit, k));
        if a != b {
            return false;
        }
        if a == 0 {
            return true;
        }
    }
    true
}

/// `std::setw(width) << text`: right aligned, padded with spaces.
fn setw(width: usize, text: &[u8]) -> Vec<u8> {
    let mut out = vec![b' '; width.saturating_sub(text.len())];
    out.extend_from_slice(text);
    out
}

fn find_from(s: &[u8], c: u8, from: usize) -> Option<usize> {
    s.get(from..)?.iter().position(|&b| b == c).map(|p| p + from)
}

/// The next comma separated item of `s` from `from`: `(from, size, next)`, `next` is `None` at the end
/// (`size_t to = s.find(',', from); size = (to == npos ? s.size() : to) - from`).
fn comma_item(s: &[u8], from: usize) -> (usize, Option<usize>) {
    match find_from(s, b',', from) {
        Some(to) => (to - from, Some(to + 1)),
        None => (s.len().saturating_sub(from), None),
    }
}

/// Insert `!` before every item of a comma separated list (negated `-t` types).
fn negate_items(list: &[u8]) -> Vec<u8> {
    let mut out = vec![b'!'];
    for &c in list {
        out.push(c);
        if c == b',' {
            out.push(b'!');
        }
    }
    out
}

fn sub(s: &[u8], from: usize, size: usize) -> &[u8] {
    let from = from.min(s.len());
    &s[from..from.saturating_add(size).min(s.len())]
}

fn contains(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Settings of the `grep`, `egrep`, `fgrep` and `z*` program names.
fn grep_defaults(s: &mut Settings, mode: Option<i32>, decompress: bool) {
    if decompress {
        s.flag_decompress = true;
    }
    if let Some(mode) = mode {
        s.flag_mode = mode;
    }
    s.flag_grep = true;
    s.flag_empty = 1;
    s.flag_hidden = true;
    s.flag_sort = c"name".as_ptr();
}

/// `init(argc, argv)`: `argv` are the NUL-terminated process arguments (`argv[0]` is the program).
pub fn init<E: Env>(env: &mut E, k: &Consts, argv: &[Z<'_>]) {
    let argc = argv.len();

    // get home directory path to expand ~ in options with path arguments, using fopen_smart()
    env.set_home_dir();

    // --config=FILE or ---FILE or --no-config: load configuration file first before parsing any other options
    let mut no_config = false;
    let mut i = 1;
    while i < argc {
        let a = argv[i];
        if eq(a, "--") {
            break;
        }

        // the C++ tests argv[1], not argv[i]: kept as is (decision Q-core-114)
        let a1 = argv[1];
        if eq(a1, "-e") || eq(a1, "-g") || eq(a1, "-N") || eq(a1, "--regexp") {
            i += 1;
        } else if a.starts_with(b"--config") {
            if !env.settings().flag_config.is_null() {
                warn_multiple(env, a);
            } else if at(a, 8) == 0 {
                env.settings().flag_config = c"".as_ptr();
            } else if at(a, 8) == b'=' {
                env.settings().flag_config = a[9..].as_ptr().cast();
            }
        } else if a.starts_with(b"---") {
            if !env.settings().flag_config.is_null() {
                warn_multiple(env, a);
            } else {
                env.settings().flag_config = a[3..].as_ptr().cast();
            }
        } else if eq(a, "--no-config") {
            no_config = true;
        }
        i += 1;
    }

    if !env.settings().flag_config.is_null() {
        env.load_config();
    }

    // reset warnings
    env.warnings_reset();

    // apply the appropriate options when the program is named grep, egrep, fgrep, zgrep, zegrep, zfgrep
    let argv0 = body(argv.first().copied().unwrap_or(EMPTY));
    let program = match argv0.iter().rposition(|&c| c == k.pathsep) {
        Some(p) => &argv0[p + 1..],
        None => argv0,
    };
    let mut len = program.len();
    if k.os_win_or_mingw {
        // Windows: compare executable name up to a dot when present, e.g. "ug.exe" compares as "ug"
        if let Some(dot) = program.iter().position(|&c| c == b'.') {
            len = dot;
        }
    }

    if strncmp_eq(program, "ug", len) {
        // the 'ug' command is equivalent to 'ugrep --config --pretty --sort'
        env.settings().flag_pretty = k.auto;
        env.settings().flag_sort = c"name".as_ptr();
        if !no_config && env.settings().flag_config.is_null() {
            env.load_config();
        }
    } else if strncmp_eq(program, "grep", len) {
        grep_defaults(env.settings(), Some(MODE_BRE), false);
    } else if strncmp_eq(program, "egrep", len) {
        grep_defaults(env.settings(), None, false);
    } else if strncmp_eq(program, "fgrep", len) {
        grep_defaults(env.settings(), Some(MODE_FIXED), false);
    } else if strncmp_eq(program, "zgrep", len) {
        grep_defaults(env.settings(), Some(MODE_BRE), true);
    } else if strncmp_eq(program, "zegrep", len) {
        grep_defaults(env.settings(), None, true);
    } else if strncmp_eq(program, "zfgrep", len) {
        grep_defaults(env.settings(), Some(MODE_FIXED), true);
    }

    // parse ugrep command-line options and arguments
    env.options();

    if env.warnings() > 0 {
        env.cerr(b"Usage: ugrep [OPTIONS] [PATTERN] [-f FILE] [-e PATTERN] [FILE ...]\n");
        env.cerr(b"Try `ugrep --help' or `ugrep --help WHAT' for more information\n");
        env.exit(EXIT_ERROR);
        return;
    }

    // -t list: list table of types and exit
    let file_types = env.list_all(List::FileType);
    if file_types.len() == 1 && file_types[0] == b"list" {
        let mut text = setw(12, b"FILE TYPE");
        text.extend_from_slice(b"   -O EXTENSIONS, -g FILENAMES AND FILE SIGNATURE -M 'MAGIC BYTES'\n");
        env.cerr(&text);

        let mut j = 0;
        while let Some(t) = env.type_entry(j) {
            let mut text = setw(12, &t.name);
            text.extend_from_slice(b" = -O ");
            text.extend_from_slice(&t.extensions);
            text.push(b'\n');
            env.cerr(&text);
            if let Some(filenames) = &t.filenames {
                let mut text = setw(18, b"-g ");
                text.extend_from_slice(filenames);
                text.push(b'\n');
                env.cerr(&text);
            }
            if let Some(magic) = &t.magic {
                let mut text = setw(19, b"-M '");
                text.extend_from_slice(magic);
                text.extend_from_slice(b"'\n");
                env.cerr(&text);
            }
            j += 1;
        }

        env.exit(EXIT_ERROR);
        return;
    }

    // -z: but we don't have libz
    if !k.have_libz && env.settings().flag_decompress {
        env.usage(b"option -z is not available in this build configuration of ugrep");
    }

    // --zmax: NUM argument exceeds limit?
    if env.settings().flag_zmax > 99 {
        env.usage(b"option --zmax argument exceeds upper limit");
    }

    let pattern_args = env.patterns();

    // -P is incompatible with option -Z and with -N patterns
    if env.settings().flag_mode == MODE_PERL {
        if k.have_regex_perl {
            if env.settings().flag_fuzzy > 0 {
                env.usage(b"options -P and -Z are not compatible");
            }
            for &(mask, _) in &pattern_args {
                if mask & PATTERN_NEG != 0 {
                    env.usage(b"options -P and -N are not compatible");
                }
            }
        } else {
            env.usage(b"option -P is not available in this build configuration of ugrep");
        }
    }

    // -o (or -u) disables -Y to emulate grep behavior when ugrep is aliased to grep/egrep/fgrep
    if env.settings().flag_only_matching || env.settings().flag_ungroup == 1 {
        env.settings().flag_empty = 0;
    }

    // -e, -N, --and, --andnot, --not
    if !pattern_args.is_empty() {
        if env.settings().flag_bool || env.settings().flag_query {
            // -Q: if --and, --andnot or --not specified then switch to bool mode, -F can't work with -N
            for &(mask, _) in &pattern_args {
                if mask == PATTERN_AND || mask == PATTERN_NOT {
                    env.settings().flag_bool = true;
                } else if mask == PATTERN_NEG && env.settings().flag_mode == MODE_FIXED {
                    env.usage(b"option -F with -% or -Q does not support -N PATTERN");
                }
            }

            // -%: collect -e, --and, --andnot, --not patterns
            let (lparen, rparen): (&[u8], &[u8]) = if env.settings().flag_bool { (b"(", b")") } else { (b"", b"") };
            let mut sep = false;
            let mut regexp: Vec<u8> = Vec::new();

            for &(mask, arg) in &pattern_args {
                let arg = env.c_string(arg);
                if sep {
                    regexp.extend_from_slice(if mask == PATTERN_AND { b" " } else { b"\n" });
                }
                sep = true;
                if mask == PATTERN_ALT {
                    regexp.extend_from_slice(lparen);
                    regexp.extend_from_slice(&arg);
                    regexp.extend_from_slice(rparen);
                } else if mask == PATTERN_NEG && !arg.is_empty() {
                    regexp.extend_from_slice(b"(?^");
                    regexp.extend_from_slice(&arg);
                    regexp.push(b')');
                } else if mask == PATTERN_NOT && !arg.is_empty() {
                    regexp.extend_from_slice(b"-(");
                    regexp.extend_from_slice(&arg);
                    regexp.push(b')');
                } else {
                    sep = false;
                }
            }
            env.regexp_append(&regexp);

            if !env.settings().flag_query {
                env.bcnf_new_pattern_regexp();
            }
        } else {
            // populate the CNF with the collected pattern args, each arg points to a persistent command line argv[]
            for &(mask, arg) in &pattern_args {
                if mask == PATTERN_AND {
                    env.bcnf_new_term();
                } else {
                    env.bcnf_new_pattern(mask, arg);
                }
            }
        }
    }

    // --query: override --pager
    if env.settings().flag_query {
        env.settings().flag_pager = std::ptr::null();
    }

    // --tree: require sort to produce directory tree
    if env.settings().flag_tree == 1 && env.settings().flag_sort.is_null() {
        env.settings().flag_sort = c"name".as_ptr();
    }

    // check TTY info and set colors (warnings and errors may occur from here on)
    env.terminal();

    // --save-config and --save-config=FILE
    if !env.settings().flag_save_config.is_null() {
        env.save_config();
        env.exit(EXIT_OK);
        return;
    }

    // --separator: override : and | otherwise use default separators : and |
    let separator = env.settings().flag_separator;
    if separator.is_null() || at(&env.c_string(separator), 0) == 0 {
        // (the C++ tests `*flag_separator == '\0'`)
        env.settings().flag_separator = c":".as_ptr();
    } else {
        env.settings().flag_separator_bar = separator;
    }

    // save_config() and help() assume text mode, so switch to binary after we're no longer going to call them
    env.set_binary_stdout();

    if !encoding(env, k) {
        return;
    }

    // --binary-files: normalize by assigning flags
    let p = env.settings().flag_binary_files;
    let binary_files = env.c_string(p);
    match binary_files.as_slice() {
        b"without-match" => env.settings().flag_binary_without_match = 1,
        b"text" => env.settings().flag_text = 1,
        b"hex" => env.settings().flag_hex = true,
        b"with-hex" => env.settings().flag_with_hex = true,
        b"binary" => {}
        _ => env.usage(
            b"invalid argument --binary-files=TYPE, valid arguments are 'binary', 'without-match', 'text', 'hex' and 'with-hex'",
        ),
    }

    hexdump(env, k);

    // --hex takes priority over --with-hex takes priority over -I takes priority over -a
    {
        let s = env.settings();
        if s.flag_hex {
            s.flag_binary_without_match = 0;
            s.flag_text = 0;
            s.flag_with_hex = false;
        } else if s.flag_with_hex {
            s.flag_text = 0;
            s.flag_binary_without_match = 0;
        } else if s.flag_binary_without_match == 1 {
            s.flag_text = 0;
        }
    }

    // --tabs: value should be 1, 2, 4, or 8
    let tabs = env.settings().flag_tabs;
    if tabs != 0 && tabs != 1 && tabs != 2 && tabs != 4 && tabs != 8 {
        env.usage(b"invalid argument --tabs=NUM, valid arguments are 1, 2, 4, or 8");
    }

    // --match: same as specifying an empty "" pattern argument
    if env.settings().flag_match {
        env.set_arg_pattern(c"".as_ptr());
    }

    // if no regex pattern is specified and no -e PATTERN and no -f FILE and not -Q, then exit with usage message
    if env.arg_pattern().is_null()
        && pattern_args.is_empty()
        && env.list_all(List::File).is_empty()
        && !env.settings().flag_query
    {
        let mut msg: Vec<u8> = b"no search PATTERN specified\n\n\
            Examples:\n\
            \x20 ugrep \"PATTERN\"       # match PATTERN\n\
            \x20 ugrep -e \"-PATTERN\"   # match -PATTERN (including the leading dash)\n"
            .to_vec();
        if k.os_win {
            msg.extend_from_slice(b"  ugrep -e \"/PATTERN\"   # match /PATTERN (including the leading slash)\n");
        }
        msg.extend_from_slice(
            b"  ugrep \"\"              # match all lines\n  ugrep --match         # match all lines\n",
        );
        env.usage(&msg);
    }

    // regex PATTERN should be a FILE argument when -Q or -e PATTERN is specified
    if !env.settings().flag_match
        && !env.arg_pattern().is_null()
        && (env.settings().flag_query || !pattern_args.is_empty())
    {
        let p = env.arg_pattern();
        env.arg_files_insert_front(p);
        env.set_arg_pattern(std::ptr::null());
    }

    // Windows shell does not expand wildcards in arguments, do that now (basename part only)
    env.expand_arg_files();

    if !env.settings().flag_devices.is_null() {
        // -D: check ACTION value
        let p = env.settings().flag_devices;
        let devices = env.c_string(p);
        match devices.as_slice() {
            b"skip" => env.settings().flag_devices_action = ACTION_SKIP,
            b"read" => env.settings().flag_devices_action = ACTION_READ,
            _ => env.usage(b"invalid argument -D ACTION, valid arguments are 'skip' and 'read'"),
        }
    }

    if !env.settings().flag_directories.is_null() {
        // normalize -R (--dereference-recurse) option
        let p = env.settings().flag_directories;
        let mut directories = env.c_string(p);
        if directories == b"dereference-recurse" {
            env.settings().flag_directories = c"recurse".as_ptr();
            env.settings().flag_dereference = true;
            directories = b"recurse".to_vec();
        }

        // -d: check ACTION value and set flags
        match directories.as_slice() {
            b"skip" => env.settings().flag_directories_action = ACTION_SKIP,
            b"read" => env.settings().flag_directories_action = ACTION_READ,
            b"recurse" => env.settings().flag_directories_action = ACTION_RECURSE,
            _ => env.usage(
                b"invalid argument -d ACTION, valid arguments are 'skip', 'read', 'recurse' and 'dereference-recurse'",
            ),
        }
    }

    if !env.settings().flag_index.is_null() {
        let p = env.settings().flag_index;
        let index = env.c_string(p);
        if index != b"safe" && index != b"fast" && index != b"log" {
            env.usage(b"invalid argument --index=MODE, valid arguments are 'safe', 'fast' and 'log'");
        }
    }

    // --from=FILE: add file pathnames to search from FILE as if specified on the command line
    for from in env.list_all(List::From) {
        if !from.is_empty() {
            if !env.file_open(&from) {
                env.error(b"option --from: cannot read", &from);
                return;
            }

            // read globs from the specified file or files
            loop {
                let (eof, len) = env.file_getline();
                if eof {
                    break;
                }
                if len != 0 {
                    // push file pathnames imported from the file to the back of the arg files vector
                    env.arg_files_push_line();
                }
            }

            env.file_close();
        }
    }

    // check all FILE arguments, warn about non-existing and non-readable files and directories
    let mut n = 0;
    while n < env.arg_files_len() {
        let p = env.arg_file(n);
        let no_dereference = env.settings().flag_no_dereference;
        match env.check_arg_file(p, no_dereference) {
            ArgFile::Missing => {
                // the specified argument FILE does not exist
                let arg = env.c_string(p);
                env.warning(None, &arg);
                env.arg_files_erase(n);
                if env.arg_files_len() == 0 {
                    env.exit(EXIT_ERROR);
                    return;
                }
            }
            ArgFile::Symlink => {
                // -p: skip symlinks
                env.arg_files_erase(n);
                if env.arg_files_len() == 0 {
                    env.exit(EXIT_ERROR);
                    return;
                }
            }
            ArgFile::Unreadable => {
                // the specified argument FILE or directory is not readable, even when executing as su
                let arg = env.c_string(p);
                env.warning(Some(b"cannot read"), &arg);
                env.arg_files_erase(n);
                if env.arg_files_len() == 0 {
                    env.exit(EXIT_ERROR);
                    return;
                }
            }
            ArgFile::Directory => {
                // use threads to descent into a directory
                if env.settings().flag_directories_action == ACTION_UNSP {
                    env.settings().flag_all_threads = true;
                }

                // remove trailing path separators, if any (the argument points to argv[])
                env.trim_pathname_arg(p);
                n += 1;
            }
            ArgFile::Other => n += 1,
        }
    }

    if !env.settings().flag_stdin && env.arg_files_len() == 0 {
        let froms = env.list_all(List::From);
        if !froms.is_empty() {
            // all --from=FILE are empty, warn (don't search recursively)
            for from in &froms {
                env.warning(Some(b"--from: empty"), from);
            }
        } else if env.isatty_stdin()
            && (env.settings().flag_directories_action == ACTION_UNSP
                || env.settings().flag_directories_action == ACTION_RECURSE)
        {
            // if no FILE specified when reading standard input from a TTY then enable -r if not already -r or -R
            env.settings().flag_directories_action = ACTION_RECURSE;

            // recursive search with worker threads
            env.settings().flag_all_threads = true;
        } else {
            // if no FILE specified then read standard input
            env.settings().flag_stdin = true;
        }
    }

    format(env.settings());

    // --replace clashes with --format
    if !env.settings().flag_replace.is_null() && !env.settings().flag_format.is_null() {
        env.abort(b"--format is not permitted with --replace");
        return;
    }

    // -v with --files is not permitted
    if env.settings().flag_invert_match && env.settings().flag_files {
        env.abort(b"--invert-match is not permitted with --files, invert the Boolean query instead");
        return;
    }

    // --min-count is not permitted with --files
    if env.settings().flag_min_count > 0 && env.settings().flag_files {
        env.abort(b"--min-count is not permitted with --files");
        return;
    }

    // --min-count is not permitted with -v when not combined with -q, -l, -L or -c
    {
        let s = env.settings();
        if s.flag_min_count > 0
            && s.flag_invert_match
            && !s.flag_quiet
            && !s.flag_files_with_matches
            && !s.flag_files_without_match
            && !s.flag_count
        {
            env.abort(b"--min-count is not permitted with --invert-match");
            return;
        }
    }

    if k.have_stat_fs {
        file_systems(env);
    }

    // --ignore-file=FILE when FILE are pathnames: add globs to the exclude and exclude-dir lists with gitignore rules
    for name in env.list_all(List::IgnoreFiles) {
        if name == b"-" || name.contains(&k.pathsep) {
            if !env.file_open(&name) {
                env.error(b"option --ignore-files: cannot read", &name);
                return;
            }

            // push globs imported from the file to the back of the vectors with gitignore flag = true
            env.import_globs(false, true);
            env.file_close();
            env.ignore_file_erase(&name);
        }
    }

    // --exclude-from=FILE: add globs to the exclude and exclude-dir lists
    for from in env.list_all(List::ExcludeFrom) {
        if !from.is_empty() {
            if !env.file_open(&from) {
                env.error(b"option --exclude-from: cannot read", &from);
                return;
            }
            env.import_globs(false, false);
            env.file_close();
        }
    }

    // --include-from=FILE: add globs to the include and include-dir lists
    for from in env.list_all(List::IncludeFrom) {
        if !from.is_empty() {
            if !env.file_open(&from) {
                env.error(b"option --include-from: cannot read", &from);
                return;
            }
            env.import_globs(true, false);
            env.file_close();
        }
    }

    if !file_type(env) {
        return;
    }

    file_extension(env);

    if !magic(env) {
        return;
    }

    // --filter: Cygwin forked process may hang when searching with multiple threads, force one worker thread
    if k.cygwin && !env.filter_empty() {
        env.settings().flag_jobs = 1;
    }
}

fn warn_multiple<E: Env>(env: &mut E, arg: Z<'_>) {
    let mut text = b"ugrep: warning: multiple configurations specified, ignoring extra ".to_vec();
    text.extend_from_slice(body(arg));
    text.push(b'\n');
    env.cerr(&text);
}

/// `--encoding`: false when the process has ended.
fn encoding<E: Env>(env: &mut E, k: &Consts) -> bool {
    let flag_encoding = env.settings().flag_encoding;
    if !flag_encoding.is_null() {
        let wanted = env.c_string(flag_encoding);

        if wanted == b"list" {
            // list the encoding_table[]
            let mut i = 0;
            while let Some((format, _)) = env.encoding_entry(i) {
                let mut text = format;
                text.push(b'\n');
                env.cerr(&text);
                i += 1;
            }
            env.exit(EXIT_ERROR);
            return false;
        }

        // scan the encoding_table[] for a matching encoding, case insensitive
        let mut i = 0;
        let mut found = None;
        while let Some((format, encoding)) = env.encoding_entry(i) {
            if wanted.eq_ignore_ascii_case(&format) {
                found = Some(encoding);
                break;
            }
            i += 1;
        }

        let Some(encoding) = found else {
            let mut msg = b"invalid argument --encoding=ENCODING, valid arguments are".to_vec();
            let mut j = 0;
            while let Some((format, _)) = env.encoding_entry(j) {
                msg.extend_from_slice(b" '");
                msg.extend_from_slice(&format);
                msg.extend_from_slice(b"',");
                j += 1;
            }
            msg.pop();
            env.usage(&msg);
            // only reached when usage() returns: the C++ would read the terminating row
            return false;
        };

        // encoding is the file encoding used by all input files, if no BOM is present
        env.settings().flag_encoding_type = encoding;
    } else if env.settings().flag_null_data {
        env.settings().flag_encoding_type = k.encoding_null_data;
    }
    true
}

/// `--hexdump`: normalize by assigning flags.
fn hexdump<E: Env>(env: &mut E, k: &Consts) {
    let flag_hexdump = env.settings().flag_hexdump;
    if flag_hexdump.is_null() {
        return;
    }
    let hexdump = env.c_string(flag_hexdump);
    let mut context = 0;

    {
        let s = env.settings();
        s.flag_hex_after = s.flag_after_context.wrapping_add(1);
        s.flag_hex_before = s.flag_before_context.wrapping_add(1);
    }

    let mut p = 0;
    while p < hexdump.len() {
        match hexdump[p] {
            b'a' => {
                env.settings().flag_hex_star = true;
                context = 0;
            }
            b'b' => {
                let s = env.settings();
                s.flag_hex_hbr = false;
                s.flag_hex_cbr = false;
                context = 0;
            }
            b'c' => {
                env.settings().flag_hex_chr = false;
                context = 0;
            }
            b'h' => {
                env.settings().flag_hex_hbr = false;
                context = 0;
            }
            b'A' => {
                env.settings().flag_hex_after = 2;
                context = 1;
            }
            b'B' => {
                env.settings().flag_hex_before = 2;
                context = 2;
            }
            b'C' => {
                let s = env.settings();
                s.flag_hex_after = 2;
                s.flag_hex_before = 2;
                context = 3;
            }
            _ => {
                // strtoull never yields a NULL end pointer
                let (value, end) = strtoull10(&hexdump[p..]);
                let num = value as usize;
                // the C++ continues at the end of the number; without digits that is this very character
                p += end;
                match context {
                    0 => {
                        let columns = num.wrapping_mul(8);
                        env.settings().flag_hex_columns = columns;
                        if columns == 0 || columns > k.max_hex_columns {
                            env.usage(b"invalid argument --hexdump=[1-8][a][bch][A[NUM]][B[NUM]][C[NUM]]");
                        }
                    }
                    1 => env.settings().flag_hex_after = num.wrapping_add(1),
                    2 => env.settings().flag_hex_before = num.wrapping_add(1),
                    _ => {
                        let s = env.settings();
                        s.flag_hex_after = num.wrapping_add(1);
                        s.flag_hex_before = num.wrapping_add(1);
                    }
                }
                if end == 0 {
                    // the C++ loops forever here (strtoull consumed nothing and the scan restarts at the same
                    // character); only reachable when usage() returned, so stop instead of hanging
                    return;
                }
                continue;
            }
        }
        p += 1;
    }

    // enable option -X if -W is not enabled
    let s = env.settings();
    if !s.flag_with_hex {
        s.flag_hex = true;
    }

    // disable -ABC line context to use hex binary context
    s.flag_after_context = 0;
    s.flag_before_context = 0;
}

/// Normalize `--cpp`, `--csv`, `--json`, `--xml` to their corresponding `--format`.
fn format(s: &mut Settings) {
    if s.flag_cpp {
        s.flag_format_begin = c"const struct grep {\n  const char *file;\n  size_t line;\n  size_t column;\n  size_t offset;\n  const char *match;\n} matches[] = {\n".as_ptr();
        s.flag_format_open = c"  // %f\n".as_ptr();
        s.flag_format = c"  { %h, %n, %k, %b, %C },\n%u".as_ptr();
        s.flag_format_close = c"  \n".as_ptr();
        s.flag_format_end = c"  { NULL, 0, 0, 0, NULL }\n};\n".as_ptr();
    } else if s.flag_csv {
        s.flag_format_open = c"%+".as_ptr();
        s.flag_format = c"%[,]$%H%N%K%B%V\n%u".as_ptr();
    } else if s.flag_json {
        s.flag_format_begin = c"[".as_ptr();
        s.flag_format_open = c"%,\n  {\n    %[,\n    ]$%[\"file\": ]H\"matches\": [".as_ptr();
        s.flag_format = c"%,\n      { %[, ]$%[\"line\": ]N%[\"column\": ]K%[\"offset\": ]B\"match\": %J }%u".as_ptr();
        s.flag_format_close = c"\n    ]\n  }".as_ptr();
        s.flag_format_end = c"\n]\n".as_ptr();
    } else if s.flag_xml {
        s.flag_format_begin = c"<grep>\n".as_ptr();
        s.flag_format_open = c"  <file%[\"]$%[ name=\"]I>\n".as_ptr();
        s.flag_format = c"    <match%[\"]$%[ line=\"]N%[ column=\"]K%[ offset=\"]B>%X</match>\n%u".as_ptr();
        s.flag_format_close = c"  </file>\n".as_ptr();
        s.flag_format_end = c"</grep>\n".as_ptr();
    } else if s.flag_only_line_number {
        s.flag_format_open = c"%[fn]=%+%=".as_ptr();
        s.flag_format = c"%[fn]=%F%=%[ln]=%n%=%[se]=%s%=%[cn]=%K%=%[bn]=%B%=\n%u".as_ptr();
        s.flag_format_close = c"%R".as_ptr();
    }
}

/// `--exclude-fs` and `--include-fs`: add file system ids to exclude or include.
fn file_systems<E: Env>(env: &mut E) {
    for mounts in env.list_all(List::ExcludeFs) {
        if mounts.is_empty() {
            if env.arg_files_len() == 0 {
                // --exclude-fs without MOUNT points and no targets for recursive search: only include the working dir FS
                if let Some(id) = env.fs_probe(b".") {
                    env.fs_insert(true, id);
                }
            } else {
                // --exclude-fs without MOUNT points: only include FS associated with the specified file and directory targets
                let mut n = 0;
                while n < env.arg_files_len() {
                    let p = env.arg_file(n);
                    let target = env.c_string(p);
                    if let Some(id) = env.fs_probe(&target) {
                        env.fs_insert(true, id);
                    }
                    n += 1;
                }
            }
        } else {
            mount_list(env, &mounts, false, b"--exclude-fs");
        }
    }

    for mounts in env.list_all(List::IncludeFs) {
        if mounts.is_empty() {
            // --include-fs without MOUNT points includes the working directory
            if let Some(id) = env.fs_probe(b".") {
                env.fs_insert(true, id);
            }
        } else {
            mount_list(env, &mounts, true, b"--include-fs");
        }
    }
}

fn mount_list<E: Env>(env: &mut E, mounts: &[u8], include: bool, option: &[u8]) {
    let mut from = 0;
    loop {
        let (size, next) = comma_item(mounts, from);
        if size > 0 {
            let mount = sub(mounts, from, size);
            match env.fs_probe(mount) {
                Some(id) => env.fs_insert(include, id),
                None => env.warning(Some(option), mount),
            }
        }
        match next {
            Some(n) => from = n,
            None => break,
        }
    }
}

/// `-t`: parse TYPES and access the type table to add `-O` (`--file-extension`), `-g` (`--glob`) and `-M`
/// (`--file-magic`) values. False when the process has ended.
fn file_type<E: Env>(env: &mut E) -> bool {
    for types in env.list_all(List::FileType) {
        let mut from = 0;
        loop {
            let (mut size, next) = comma_item(&types, from);

            if size > 0 {
                let negate = size > 1 && (at(&types, from) == b'!' || at(&types, from) == b'^');
                if negate {
                    from += 1;
                    size -= 1;
                }

                let name = sub(&types, from, size).to_vec();

                let mut i = 0;
                let mut found = false;
                let mut valid = true;

                // scan the type_table[] for the specified type, allowing type to be a prefix when unambiguous
                let mut j = 0;
                while let Some(t) = env.type_entry(j) {
                    let typelen = t.name.len();
                    if size <= typelen && name == t.name[..size] {
                        // an ambiguous prefix is not a valid type
                        if found {
                            valid = false;
                        }
                        found = true;
                        i = j;

                        // if full type match then we found a valid type match
                        if size == typelen {
                            valid = true;
                            break;
                        }
                    }
                    j += 1;
                }

                // if not found a valid type, then find an unambiguous type with the specified filename suffix
                if !found && valid {
                    let mut j = 0;
                    while let Some(t) = env.type_entry(j) {
                        if at(&t.name, 0).is_ascii_lowercase() {
                            if let Some(s) = contains(&t.extensions, &name) {
                                if s == 0 || t.extensions[s - 1] == b',' {
                                    if found {
                                        valid = false;
                                        break;
                                    }
                                    found = true;
                                    i = j;
                                }
                            }
                        }
                        j += 1;
                    }
                }

                if !found || !valid {
                    let mut msg = b"invalid argument -t TYPES, valid arguments are".to_vec();
                    let mut j = 0;
                    while let Some(t) = env.type_entry(j) {
                        msg.extend_from_slice(b" '");
                        msg.extend_from_slice(&t.name);
                        msg.extend_from_slice(b"',");
                        j += 1;
                    }
                    msg.extend_from_slice(b" and 'list' to show a detailed list of file types");
                    env.usage(&msg);
                    // only reached when usage() returns: the C++ would read the terminating row
                    return false;
                }

                let Some(entry) = env.type_entry(i) else {
                    return false;
                };

                let temp = if negate { negate_items(&entry.extensions) } else { entry.extensions.clone() };
                env.list_push(List::FileExtension, &temp);

                if let Some(filenames) = &entry.filenames {
                    let temp = if negate { negate_items(filenames) } else { filenames.clone() };
                    env.list_push(List::Glob, &temp);
                }

                if let Some(magic) = &entry.magic {
                    let mut temp = Vec::new();
                    if negate {
                        temp.push(b'!');
                    }
                    temp.extend_from_slice(magic);
                    env.list_push(List::FileMagic, &temp);
                }
            }

            match next {
                Some(n) => from = n,
                None => break,
            }
        }
    }
    true
}

/// The end of the glob list item that starts at `from`: commas inside `[...]` and after a backslash do not end it.
pub fn glob_item_end(s: &[u8], from: usize) -> usize {
    let mut to = from;
    while to < s.len() && s[to] != b',' {
        if s[to] == b'[' {
            loop {
                to += 1;
                if to >= s.len() || s[to] == b']' {
                    break;
                }
                if s[to] == b'\\' {
                    to += 1;
                }
            }
        } else if s[to] == b'\\' {
            to += 1;
        }
        to += 1;
    }
    to
}

/// `-O`: add filename extensions as globs.
fn file_extension<E: Env>(env: &mut E) {
    for extensions in env.list_all(List::FileExtension) {
        let mut from = 0;
        loop {
            let to = glob_item_end(&extensions, from);
            let mut size = to - from;

            if size > 0 {
                let negate = size > 1 && (at(&extensions, from) == b'!' || at(&extensions, from) == b'^');
                if negate {
                    from += 1;
                    size -= 1;
                }

                let mut glob: Vec<u8> = if negate { b"^*.".to_vec() } else { b"*.".to_vec() };
                glob.extend_from_slice(sub(&extensions, from, size));
                env.list_push(List::Glob, &glob);
            }

            if to >= extensions.len() {
                break;
            }

            from = to + 1;
        }
    }
}

fn negated_magic(magic: &[u8]) -> bool {
    magic.len() > 1 && (magic[0] == b'!' || magic[0] == b'^')
}

/// `-M` and `--filter-magic-label`: the magic patterns. False when the process has ended.
fn magic<E: Env>(env: &mut E) -> bool {
    // -M: file "magic bytes" regex string
    let mut magic_regex: Vec<u8> = Vec::new();
    let file_magic = env.list_all(List::FileMagic);

    // -M !MAGIC: combine to create a regex string
    for magic in &file_magic {
        if negated_magic(magic) {
            if !magic_regex.is_empty() {
                magic_regex.push(b'|');
            }
            magic_regex.extend_from_slice(&magic[1..]);

            // tally negative MAGIC patterns
            env.settings().flag_min_magic += 1;
        }
    }

    // -M MAGIC: append to regex string
    for magic in &file_magic {
        if !negated_magic(magic) {
            if !magic_regex.is_empty() {
                magic_regex.push(b'|');
            }
            magic_regex.extend_from_slice(magic);

            // we have positive MAGIC patterns, so scan() is a match when flag_min_magic or greater
            let s = env.settings();
            s.flag_not_magic = s.flag_min_magic;
        }
    }

    // -M: create a magic matcher for the MAGIC regex to match file with magic.scan()
    let regex = if magic_regex.is_empty() { None } else { Some(magic_regex.as_slice()) };
    if let Some(what) = env.magic_pattern_assign(regex) {
        env.abort_what(b"option -M: ", &what);
        return false;
    }

    // --filter-magic-label: construct Static::filter_magic_pattern and map "magic bytes" to labels
    let mut magic_regex: Vec<u8> = b"(".to_vec();

    // --filter-magic-label: append pattern to magic_labels, parenthesized to ensure capture indexing
    for (index, label) in env.list_all(List::FilterMagicLabel).into_iter().enumerate() {
        if !label.is_empty() {
            match label.iter().position(|&c| c == b':') {
                Some(sep) if sep > 0 && sep + 1 < label.len() => {
                    if magic_regex.len() > 1 {
                        magic_regex.extend_from_slice(b")|(");
                    }
                    magic_regex.extend_from_slice(&label[sep + 1..]);

                    // truncate so we end up with a list of labels without patterns
                    env.list_truncate(List::FilterMagicLabel, index, sep);
                }
                _ => {
                    env.abort_what(b"option --filter-magic-label: invalid LABEL:MAGIC argument ", &label);
                    return false;
                }
            }
        }
    }

    magic_regex.push(b')');

    // --filter-magic-label: create a Static::filter_magic_pattern
    if magic_regex.len() > 2 {
        if let Some(what) = env.filter_magic_assign(&magic_regex) {
            env.abort_what(b"option --filter-magic-label: ", &what);
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests;
