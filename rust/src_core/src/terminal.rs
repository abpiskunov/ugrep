//! `terminal()` and `set_terminal_hyperlink()` of src/ugrep.cpp: TTY detection, `--color`, `--pretty`, `--pager`
//! and `--query` decisions, GREP_COLORS and `--colors` parsing, and the `--hyperlink` prefix, host and path.
//!
//! The work is done on the Rust-owned [`Settings`]. The operating system and the C++ owned state (the
//! `Static::output_stat*` globals, `Output::Tree`, the `std::string` hyperlink options, `usage`) are reached
//! through [`Host`]; `ffi/terminal.rs` implements it for the product.

use crate::color::{c_str, copy_color, find, set_color};
use crate::settings::{Settings, COLORLEN};
use std::ffi::CString;
use std::os::raw::c_char;

const STDOUT: i32 = 1;
const STDERR: i32 = 2;

/// Values of the retained C++ and the build.
pub struct Consts {
    /// `Static::NEVER`, `Static::ALWAYS` and `Static::AUTO`: the tri-state is compared by address.
    pub never: *const c_char,
    pub always: *const c_char,
    pub auto: *const c_char,
    /// DEFAULT_GREP_COLORS.
    pub default_grep_colors: Vec<u8>,
    /// `OS_WIN_OR_MINGW`.
    pub os_win: bool,
    /// The path separator character, `PATHSEPCHR`.
    pub pathsep: u8,
}

/// What `terminal()` needs from the operating system and the retained C++.
pub trait Host {
    /// The bytes of a NUL-terminated string the settings point to.
    fn c_string(&mut self, p: *const c_char) -> Vec<u8>;
    fn isatty(&mut self, fd: i32) -> bool;
    fn getenv(&mut self, name: &str) -> Option<Vec<u8>>;
    /// Not Windows: `fstat(STDOUT)` into `Static::output_stat*`; true when the output is `/dev/null`.
    fn probe_output(&mut self) -> bool;
    /// Set `Output::Tree::bar`, `ptr` and `end` to the UTF-8 box drawing strings.
    fn tree_utf8(&mut self);
    /// Windows: UTF-8 console output and virtual terminal processing; `None` without a console handle.
    fn enable_vt(&mut self) -> Option<bool>;
    /// The current working directory (`getcwd0`).
    fn cwd(&mut self) -> Option<Vec<u8>>;
    /// The host name, `localhost` when the system call fails.
    fn hostname(&mut self) -> Vec<u8>;
    /// `flag_hyperlink_host.assign`, `flag_hyperlink_path.append`, `flag_hyperlink_prefix.assign`.
    fn hyperlink_set(&mut self, host: &[u8], path: &[u8], prefix: &[u8]);
    /// `usage(message)`: returns only when `flag_usage_warnings` is set.
    fn usage(&mut self, message: &str);
}

/// A process-lifetime C string: the settings keep pointers into it, as they do into the C++ environment copies.
fn leak(bytes: &[u8]) -> *const c_char {
    let cstr = CString::new(c_str(bytes)).unwrap_or_default();
    cstr.into_raw()
}

fn as_u8(buf: &[c_char; COLORLEN]) -> [u8; COLORLEN] {
    buf.map(|c| c as u8)
}

fn from_u8(buf: &mut [c_char; COLORLEN], bytes: &[u8; COLORLEN]) {
    for (to, &from) in buf.iter_mut().zip(bytes) {
        *to = from as c_char;
    }
}

fn color(buf: &mut [c_char; COLORLEN], colors: &[u8], parameter: &[u8]) {
    let mut tmp = as_u8(buf);
    set_color(colors, parameter, &mut tmp);
    from_u8(buf, &tmp);
}

fn copy(to: &mut [c_char; COLORLEN], from: &[u8]) {
    let mut tmp = as_u8(to);
    copy_color(&mut tmp, from);
    from_u8(to, &tmp);
}

/// `set_color` of every capability a colors string names.
fn set_all(s: &mut Settings, colors: &[u8]) {
    color(&mut s.color_sl, colors, b"sl="); // selected line
    color(&mut s.color_cx, colors, b"cx="); // context line
    color(&mut s.color_mt, colors, b"mt="); // matched text in any line
    color(&mut s.color_ms, colors, b"ms="); // matched text in selected line
    color(&mut s.color_mc, colors, b"mc="); // matched text in a context line
    color(&mut s.color_fn, colors, b"fn="); // file name
    color(&mut s.color_ln, colors, b"ln="); // line number
    color(&mut s.color_cn, colors, b"cn="); // column number
    color(&mut s.color_bn, colors, b"bn="); // byte offset
    color(&mut s.color_se, colors, b"se="); // separator
    color(&mut s.color_qp, colors, b"qp="); // TUI prompt
    color(&mut s.color_qe, colors, b"qe="); // TUI errors
    color(&mut s.color_qr, colors, b"qr="); // TUI regex highlight
    color(&mut s.color_qm, colors, b"qm="); // TUI regex meta characters highlight
    color(&mut s.color_ql, colors, b"ql="); // TUI regex lists and literals highlight
    color(&mut s.color_qb, colors, b"qb="); // TUI regex braces highlight
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    find(hay, needle).is_some()
}

/// `getenv(name) != NULL && *val != '\0'`.
fn env_set(h: &mut dyn Host, name: &str) -> bool {
    h.getenv(name).is_some_and(|v| v.first().is_some_and(|&b| b != 0))
}

/// The UTF-8 terminal guess of `--tree`: LANG, LC_CTYPE or LC_ALL mention UTF-8.
fn utf8_locale(h: &mut dyn Host) -> bool {
    ["LANG", "LC_CTYPE", "LC_ALL"]
        .iter()
        .any(|name| h.getenv(name).is_some_and(|v| contains(&v, b"UTF-8")))
}

/// Normalize a `WHEN` argument to `Static::NEVER`, `ALWAYS` or `AUTO`; `None` for other values.
fn normalize(word: &[u8], c: &Consts) -> Option<*const c_char> {
    match c_str(word) {
        b"never" | b"no" | b"none" => Some(c.never),
        b"always" | b"yes" | b"force" => Some(c.always),
        b"auto" | b"tty" | b"if-tty" => Some(c.auto),
        _ => None,
    }
}

/// `terminal()`.
pub fn terminal(s: &mut Settings, h: &mut dyn Host, c: &Consts) {
    // check if standard output is a TTY
    s.flag_tty_term = h.isatty(STDOUT);

    if s.flag_query {
        // -Q: disable --quiet
        s.flag_quiet = false;
    } else if !s.flag_quiet && !c.os_win && !s.flag_tty_term && h.probe_output() {
        // output is sent to /dev/null: enable -q
        s.flag_quiet = true;
    }

    // if not to a TTY or --quiet then disable --pager
    if !s.flag_tty_term || s.flag_quiet {
        s.flag_pager = std::ptr::null();
    }

    // --color=WHEN: normalize WHEN argument
    if !s.flag_color.is_null() {
        let word = h.c_string(s.flag_color);
        match normalize(&word, c) {
            Some(when) => s.flag_color = when,
            None => h.usage("invalid argument --color=WHEN, valid arguments are 'never', 'always' and 'auto'"),
        }
    }

    // --pretty=WHEN: normalize WHEN argument
    if !s.flag_pretty.is_null() {
        let word = h.c_string(s.flag_pretty);
        match c_str(&word) {
            b"never" | b"no" | b"none" => s.flag_pretty = std::ptr::null(),
            b"always" | b"yes" | b"force" => s.flag_pretty = c.always,
            b"auto" | b"tty" | b"if-tty" => s.flag_pretty = c.auto,
            _ => h.usage("invalid argument --pretty=WHEN, valid arguments are 'never', 'always' and 'auto'"),
        }
    }

    // whether to apply colors based on --tag, --query and --pretty
    if !s.flag_tag.is_null() {
        s.flag_color = std::ptr::null();
    }

    if s.flag_quiet {
        return;
    }

    if s.flag_tty_term || s.flag_query || s.flag_pretty == c.always {
        if !s.flag_pretty.is_null() {
            // --pretty: if output is sent to a TTY then enable --color, --heading, -T, -n, --sort and --tree
            if s.flag_color.is_null() && s.flag_tag.is_null() {
                s.flag_color = c.always;
            }
            if s.flag_heading == -1 {
                s.flag_heading = 1;
            }
            if s.flag_initial_tab == -1 {
                s.flag_initial_tab = 1;
            }
            if s.flag_line_number == -1 {
                s.flag_line_number = 1;
            }
            if s.flag_sort.is_null() {
                s.flag_sort = c"name".as_ptr();
            }
            if s.flag_tree == -1 {
                s.flag_tree = 1;
            }
        }

        if s.flag_query {
            // --query: run the interactive query UI
            if s.flag_heading == -1 {
                s.flag_heading = 1;
            }
            s.flag_line_buffered = true;
        }
    }

    // --tree: check UTF-8 terminal support, this is a guess on LANG or LC_CTYPE or LC_ALL
    if s.flag_tree == 1 && (s.flag_query || s.flag_files_with_matches || s.flag_files_without_match || s.flag_count) && utf8_locale(h) {
        h.tree_utf8();
    }

    // --color: (re)set flag_color depending on color term and TTY output
    if s.flag_color.is_null() {
        return;
    }

    if s.flag_color == c.never {
        s.flag_color = std::ptr::null();
        return;
    }

    if c.os_win {
        if s.flag_tty_term {
            // assume we have a color terminal on Windows if isatty() is true
            if let Some(vt) = h.enable_vt() {
                s.flag_color_term = vt;
            }

            // no color when NO_COLOR is set or when ConEmuANSI is OFF
            if env_set(h, "NO_COLOR") || h.getenv("ConEmuANSI").is_some_and(|v| c_str(&v) == b"OFF") {
                s.flag_color_term = false;
            } else if !s.flag_color_term
                && (h.getenv("ANSICON").is_some() || h.getenv("COLORTERM").is_some() || h.getenv("ConEmuANSI").is_some_and(|v| c_str(&v) == b"ON"))
            {
                // try ANSICON or COLORTERM or ConEmuANSI is ON
                s.flag_color_term = true;
            }
        }
    } else if s.flag_tty_term && !env_set(h, "NO_COLOR") {
        // check COLORTERM or if TERM supports color
        if h.getenv("COLORTERM").is_some() {
            s.flag_color_term = true;
        } else if let Some(term) = h.getenv("TERM") {
            if [b"ansi".as_slice(), b"xterm", b"screen", b"color"].iter().any(|k| contains(&term, k)) {
                s.flag_color_term = true;
            }
        }
    }

    if s.flag_query {
        // --query: assume that a color terminal is used, save color to use with TUI output
        if s.flag_color_term || s.flag_color == c.always {
            s.flag_color_query = s.flag_color;
        }
        s.flag_color = c.always;
    } else if s.flag_color == c.auto {
        if s.flag_pretty == c.always {
            s.flag_color = c.always;
        } else if !s.flag_color_term && s.flag_save_config.is_null() {
            s.flag_color = std::ptr::null();
        }
    }

    if s.flag_color.is_null() {
        return;
    }

    parse_colors(s, h, c);
}

/// The colors part of `terminal()`: GREP_COLOR, GREP_COLORS and `--colors`, then `--hyperlink` and the escapes.
fn parse_colors(s: &mut Settings, h: &mut dyn Host, c: &Consts) {
    // get GREP_COLOR and GREP_COLORS, when defined
    let env_grep_color = h.getenv("GREP_COLOR");
    let env_grep_colors = h.getenv("GREP_COLORS");
    let mut deprecated_grep_colors: Vec<u8> = Vec::new();

    // the colors string and the process-lifetime copy `--hyperlink` may point into
    let (mut grep_colors, mut grep_colors_ptr): (Vec<u8>, *const c_char) = match (&env_grep_colors, &env_grep_color) {
        (Some(colors), _) => (c_str(colors).to_vec(), leak(colors)),
        // if GREP_COLOR is defined but not GREP_COLORS, use it
        (None, Some(color)) => (c_str(color).to_vec(), leak(color)),
        (None, None) => (c.default_grep_colors.clone(), std::ptr::null()),
    };

    // parse deprecated GREP_COLOR format with ANSI SGR codes for matched text only
    if !grep_colors.contains(&b'=') {
        // use default colors then override matched text color with the specified color
        deprecated_grep_colors = std::mem::replace(&mut grep_colors, c.default_grep_colors.clone());
        grep_colors_ptr = std::ptr::null();
    }

    // parse GREP_COLORS
    set_all(s, &grep_colors);

    // deprecated GREP_COLOR format with ANSI SGR codes for matched text only
    if !deprecated_grep_colors.is_empty() {
        color(&mut s.color_ms, &deprecated_grep_colors, b"");
    }

    // parse --colors to override GREP_COLORS
    let flag_colors = if s.flag_colors.is_null() { None } else { Some(h.c_string(s.flag_colors)) };

    if let Some(colors) = &flag_colors {
        if colors.contains(&b'=') {
            set_all(s, colors);
        } else {
            // deprecated format with ANSI SGR codes only
            color(&mut s.color_mt, colors, b"");
        }
    }

    let in_grep = |needle: &[u8]| contains(&grep_colors, needle);
    let in_flag = |needle: &[u8]| flag_colors.as_deref().is_some_and(|f| contains(f, needle));

    // -v: if rv in GREP_COLORS then swap the sl and cx colors (note that rv does not match color letters)
    if s.flag_invert_match && (in_grep(b"rv") || in_flag(b"rv")) {
        let mut tmp = [0 as c_char; COLORLEN];
        copy(&mut tmp, &as_u8(&s.color_sl));
        let cx = as_u8(&s.color_cx);
        copy(&mut s.color_sl, &cx);
        copy(&mut s.color_cx, &as_u8(&tmp));
    }

    // if ms= is not specified, use the mt= value
    if s.color_ms[0] == 0 {
        let mt = as_u8(&s.color_mt);
        copy(&mut s.color_ms, &mt);
    }

    // if mc= is not specified, use the mt= value
    if s.color_mc[0] == 0 {
        let mt = as_u8(&s.color_mt);
        copy(&mut s.color_mc, &mt);
    }

    // if not --hyperlink and hl or hl= is specified, then set --hyperlink
    if s.flag_hyperlink.is_null() {
        // the C++ takes the text 3 bytes into the colors string, wherever `hl=` was found
        if in_grep(b"hl=") {
            s.flag_hyperlink = grep_colors_ptr.wrapping_add(3);
        } else if in_flag(b"hl=") {
            s.flag_hyperlink = s.flag_colors.wrapping_add(3);
        } else if in_grep(b"hl") || in_flag(b"hl") {
            s.flag_hyperlink = c"".as_ptr();
        }
    }

    // --hyperlink
    set_terminal_hyperlink(s, h, c);

    // if CSI erase line is OK (note that ne does not match color letters so strstr can be used)
    if !in_grep(b"ne") && !in_flag(b"ne") {
        s.color_del = c"\x1b[K".as_ptr();
    }

    s.color_off = c"\x1b[m".as_ptr();
    copy(&mut s.match_off, b"\x1b[m");

    if h.isatty(STDERR) {
        s.color_high = c"\x1b[1m".as_ptr();
        s.color_error = c"\x1b[1;31m".as_ptr();
        s.color_warning = c"\x1b[1;35m".as_ptr();
        s.color_message = c"\x1b[1;36m".as_ptr();
    }
}

/// `set_terminal_hyperlink()`: the prefix, host and working directory path of `--hyperlink`.
pub fn set_terminal_hyperlink(s: &mut Settings, h: &mut dyn Host, c: &Consts) {
    if s.flag_hyperlink.is_null() {
        return;
    }

    // get current working directory path in hyperlink
    let Some(cwd) = h.cwd() else {
        return;
    };

    let path = match cwd.first() {
        Some(&b) if b == c.pathsep => &cwd[1..],
        _ => &cwd[..],
    };

    // get host in hyperlink
    let host = h.hostname();

    // get custom prefix in hyperlink or default file://
    let link = h.c_string(s.flag_hyperlink);
    let alnum = link.iter().take_while(|b| b.is_ascii_alphanumeric()).count();
    let prefix: &[u8] = if alnum == 0 { b"file" } else { &link[..alnum] };

    h.hyperlink_set(&host, path, prefix);

    s.flag_hyperlink_line = link.get(alnum) == Some(&b'+');
    s.color_hl = c"\x1b]8;;".as_ptr();
    s.color_st = c"\x1b\\".as_ptr();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Mock {
        tty_out: bool,
        tty_err: bool,
        env: HashMap<&'static str, &'static str>,
        dev_null: bool,
        usage: Vec<String>,
        tree: bool,
        link: Option<(Vec<u8>, Vec<u8>, Vec<u8>)>,
    }

    impl Host for Mock {
        fn c_string(&mut self, p: *const c_char) -> Vec<u8> {
            crate::terminal_ffi::test_c_bytes(p)
        }
        fn isatty(&mut self, fd: i32) -> bool {
            if fd == STDOUT { self.tty_out } else { self.tty_err }
        }
        fn getenv(&mut self, name: &str) -> Option<Vec<u8>> {
            self.env.get(name).map(|v| v.as_bytes().to_vec())
        }
        fn probe_output(&mut self) -> bool {
            self.dev_null
        }
        fn tree_utf8(&mut self) {
            self.tree = true;
        }
        fn enable_vt(&mut self) -> Option<bool> {
            Some(true)
        }
        fn cwd(&mut self) -> Option<Vec<u8>> {
            Some(b"/work/dir".to_vec())
        }
        fn hostname(&mut self) -> Vec<u8> {
            b"box".to_vec()
        }
        fn hyperlink_set(&mut self, host: &[u8], path: &[u8], prefix: &[u8]) {
            self.link = Some((host.to_vec(), path.to_vec(), prefix.to_vec()));
        }
        fn usage(&mut self, message: &str) {
            self.usage.push(message.to_string());
        }
    }

    fn consts() -> Consts {
        Consts {
            never: c"never".as_ptr(),
            always: c"always".as_ptr(),
            auto: c"auto".as_ptr(),
            default_grep_colors: b"mt=1;31:fn=1;35".to_vec(),
            os_win: false,
            pathsep: b'/',
        }
    }

    fn settings() -> Settings {
        Settings::DEFAULT
    }

    #[test]
    fn output_to_a_pipe_is_not_a_tty() {
        let (mut s, mut h, c) = (settings(), Mock::default(), consts());
        s.flag_pager = c"less".as_ptr();
        terminal(&mut s, &mut h, &c);
        assert!(!s.flag_tty_term && !s.flag_quiet && s.flag_pager.is_null());
    }

    #[test]
    fn dev_null_enables_quiet() {
        let (mut s, mut h, c) = (settings(), Mock { dev_null: true, ..Mock::default() }, consts());
        terminal(&mut s, &mut h, &c);
        assert!(s.flag_quiet);
    }

    #[test]
    fn color_always_sets_the_escapes() {
        let (mut s, mut h, c) = (settings(), Mock::default(), consts());
        s.flag_color = c"always".as_ptr();
        terminal(&mut s, &mut h, &c);
        assert_eq!(s.flag_color, c.always);
        assert_eq!(h.c_string(s.color_off), b"\x1b[m");
        assert_eq!(c_str(&as_u8(&s.color_mt)), b"\x1b[1;31m");
        assert_eq!(c_str(&as_u8(&s.color_ms)), b"\x1b[1;31m");
        assert_eq!(c_str(&as_u8(&s.match_off)), b"\x1b[m");
    }

    #[test]
    fn bad_color_argument_reports_usage_and_continues() {
        let (mut s, mut h, c) = (settings(), Mock::default(), consts());
        s.flag_color = c"sometimes".as_ptr();
        terminal(&mut s, &mut h, &c);
        assert_eq!(h.usage.len(), 1);
        assert!(!s.flag_color.is_null());
    }

    #[test]
    fn pretty_on_a_tty_turns_on_the_layout() {
        let (mut s, mut h, c) = (settings(), Mock { tty_out: true, ..Mock::default() }, consts());
        h.env.insert("TERM", "xterm-256color");
        s.flag_pretty = c"always".as_ptr();
        terminal(&mut s, &mut h, &c);
        assert_eq!((s.flag_heading, s.flag_initial_tab, s.flag_line_number, s.flag_tree), (1, 1, 1, 1));
        assert_eq!(h.c_string(s.flag_sort), b"name");
        assert_eq!(s.flag_color, c.always);
    }

    #[test]
    fn auto_color_needs_a_color_terminal() {
        let (mut s, mut h, c) = (settings(), Mock { tty_out: true, ..Mock::default() }, consts());
        s.flag_color = c"auto".as_ptr();
        terminal(&mut s, &mut h, &c);
        assert!(s.flag_color.is_null());
        let (mut s, mut h) = (settings(), Mock { tty_out: true, ..Mock::default() });
        h.env.insert("COLORTERM", "truecolor");
        s.flag_color = c"auto".as_ptr();
        terminal(&mut s, &mut h, &c);
        assert!(s.flag_color_term && !s.flag_color.is_null());
    }

    #[test]
    fn deprecated_grep_color_overrides_ms() {
        let (mut s, mut h, c) = (settings(), Mock::default(), consts());
        h.env.insert("GREP_COLOR", "1;33");
        s.flag_color = c"always".as_ptr();
        terminal(&mut s, &mut h, &c);
        assert_eq!(c_str(&as_u8(&s.color_ms)), b"\x1b[1;33m");
        assert_eq!(c_str(&as_u8(&s.color_mt)), b"\x1b[1;31m");
    }

    #[test]
    fn hl_in_colors_sets_the_hyperlink() {
        let (mut s, mut h, c) = (settings(), Mock::default(), consts());
        s.flag_color = c"always".as_ptr();
        s.flag_colors = c"hl".as_ptr();
        terminal(&mut s, &mut h, &c);
        assert_eq!(h.link, Some((b"box".to_vec(), b"work/dir".to_vec(), b"file".to_vec())));
        assert!(!s.color_hl.is_null() && !s.flag_hyperlink_line);
    }

    #[test]
    fn hyperlink_prefix_and_line() {
        let (mut s, mut h, c) = (settings(), Mock::default(), consts());
        s.flag_hyperlink = c"vscode+".as_ptr();
        set_terminal_hyperlink(&mut s, &mut h, &c);
        assert_eq!(h.link.map(|l| l.2), Some(b"vscode".to_vec()));
        assert!(s.flag_hyperlink_line);
    }
}
