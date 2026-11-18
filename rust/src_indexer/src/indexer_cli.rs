//! Unit `indexer_cli`: the front end of ugrep-indexer (version, help, usage, strtonum, strtopos, `options`,
//! `load_config` and `main`). Rust owns the option state: `run` holds the flags, the warning counter and the PATH, and
//! `options` is a function of a flag snapshot, so it holds no state between calls.
//!
//! Text goes through the CRT `stdout` and `stderr` (`fwrite`) on Windows, like `std::cout`/`std::cerr`, so line ends
//! and ordering are the ones of the C++.
//!
//! Build configuration of the C++ (`HAVE_LIBZ` and the codec macros of the help text) is the compile-time
//! environment variable `UGREP_INDEXER_CODECS` (a comma separated list written by build.rs); without it, as in the
//! reference build and the replay harness, there is no decompression.

use super::indexer_walk::{self, ffi::sys, getline, open_mode, read_text, trim};
use super::{Bytes, Exit, IndexerFlags, OptionsResult};

const DEFAULT_IGNORE_FILE: &[u8] = b".gitignore";
const CONFIG_FILENAME: &[u8] = b".ugrep-indexer";
const VERSION: &str = env!("CARGO_PKG_VERSION");

const HELP_HEAD: &str = include_str!("help_head.txt");
const HELP_TAIL: &str = include_str!("help_tail.txt");

/// `true` when the C++ would have been built with the codec macro `name` (z, bz2, lzma, lz4, zstd, 7zip, brotli, bzip3)
fn have(name: &str) -> bool {
    option_env!("UGREP_INDEXER_CODECS").is_some_and(|list| list.split(',').any(|x| x == name))
}

fn platform() -> String {
    if cfg!(all(windows, target_env = "msvc")) {
        if cfg!(target_pointer_width = "64") { "WIN64" } else { "WIN32" }.to_string()
    } else {
        option_env!("UGREP_PLATFORM").unwrap_or("").to_string()
    }
}

fn out(bytes: &[u8]) {
    sys::write_stdout(bytes);
    sys::flush_stdout();
}

/// text of `help()`
fn help_text() -> String {
    let mut s = String::from(HELP_HEAD);
    if !have("z") {
        s.push_str("            This option is not available in this build of ugrep-indexer.\n");
    } else {
        s.push_str("            Supported compression formats: gzip (.gz), compress (.Z), zip");
        if have("7zip") {
            s.push_str(", 7z");
        }
        if have("bz2") {
            s.push_str(",\n            bzip2 (requires suffix .bz, .bz2, .bzip2, .tbz, .tbz2, .tb2, .tz2)");
        }
        if have("lzma") {
            s.push_str(",\n            lzma and xz (requires suffix .lzma, .tlz, .xz, .txz)");
        }
        if have("lz4") {
            s.push_str(",\n            lz4 (requires suffix .lz4)");
        }
        if have("zstd") {
            s.push_str(",\n            zstd (requires suffix .zst, .zstd, .tzst)");
        }
        if have("brotli") {
            s.push_str(",\n            brotli (requires suffix .br)");
        }
        if have("bzip3") {
            s.push_str(",\n            bzip3 (requires suffix .bz3)");
        }
        s.push_str(".\n");
    }
    s.push_str(HELP_TAIL);
    s
}

/// text of `version()`
fn version_text() -> String {
    format!(
        "ugrep-indexer {VERSION} {}\nLicense: BSD-3-Clause; ugrep user manual: <https://ugrep.com>\n\
         Written by Robert van Engelen and others: <https://github.com/Genivia/ugrep>\n\
         Ugrep utilizes the RE/flex regex library: <https://github.com/Genivia/RE-flex>\n",
        platform()
    )
}

/// the option parser state of one `options` call
struct Parser {
    flags: IndexerFlags,
    warnings: usize,
    /// `Some(code)` once the C++ would have called `exit(code)`
    exited: Option<i32>,
}

impl Parser {
    /// display usage information and exit, or count a warning for a configuration file line
    fn usage(&mut self, message: &str, arg: Option<&[u8]>) {
        let mut line = b"ugrep-indexer: ".to_vec();
        if self.flags.usage_warnings {
            self.warnings += 1;
            line.extend_from_slice(CONFIG_FILENAME);
            line.extend_from_slice(b" configuration file: ");
        }
        line.extend_from_slice(message.as_bytes());
        line.extend_from_slice(arg.unwrap_or(b""));
        line.push(b'\n');
        sys::write_stderr(&line);
        if !self.flags.usage_warnings {
            self.help();
        }
    }

    fn help(&mut self) {
        out(help_text().as_bytes());
        self.exited = Some(0);
    }

    fn version(&mut self) {
        out(version_text().as_bytes());
        self.exited = Some(0);
    }

    /// convert unsigned decimal to non-negative size_t, produce error when conversion fails
    fn strtonum(&mut self, string: &[u8], message: &str) -> usize {
        let (size, rest) = strtoull(string);
        if !rest.is_empty() {
            self.usage(message, Some(string));
        }
        size as usize
    }

    /// convert unsigned decimal to positive size_t, produce error when conversion fails or when the value is zero
    fn strtopos(&mut self, string: &[u8], message: &str) -> usize {
        let size = self.strtonum(string, message);
        if self.exited.is_none() && size == 0 {
            self.usage(message, Some(string));
        }
        size
    }
}

/// C `strtoull(string, &rest, 10)`: the value and the rest of the string; no digits give 0 and the whole string
fn strtoull(string: &[u8]) -> (u64, &[u8]) {
    let mut i = 0;
    while i < string.len() && matches!(string[i], b' ' | b'\t'..=b'\r') {
        i += 1;
    }
    let mut negative = false;
    if i < string.len() && (string[i] == b'+' || string[i] == b'-') {
        negative = string[i] == b'-';
        i += 1;
    }
    let first = i;
    let mut value: u64 = 0;
    let mut overflow = false;
    while i < string.len() && string[i].is_ascii_digit() {
        match value.checked_mul(10).and_then(|v| v.checked_add(u64::from(string[i] - b'0'))) {
            Some(v) => value = v,
            None => overflow = true,
        }
        i += 1;
    }
    if i == first {
        return (0, string);
    }
    if overflow {
        return (u64::MAX, &string[i..]);
    }
    (if negative { value.wrapping_neg() } else { value }, &string[i..])
}

/// Seam function `src_indexer.options` (`options()`): parse `args` (= argv[0..argc], `args[0]` is ignored) on top
/// of `flags`. `arg_path` is the PATH argument already taken (None if none). `--help`, `--version` and usage errors
/// end the process (`Exit::exited`) unless `flags.usage_warnings`, which counts a warning instead.
pub fn options(flags: &IndexerFlags, args: &[Bytes], arg_path: Option<&[u8]>, warnings: usize) -> OptionsResult {
    let mut p = Parser { flags: flags.clone(), warnings, exited: None };
    let mut path: Option<Vec<u8>> = arg_path.map(|a| a.to_vec());
    let mut path_index: i32 = -1;
    let mut options = true;

    'args: for (i, arg) in args.iter().enumerate().skip(1) {
        let prefixed = arg.first() == Some(&b'-') || (cfg!(all(windows, target_env = "msvc")) && arg.first() == Some(&b'/'));
        if prefixed && arg.len() > 1 && options {
            let mut pos = 1;
            while pos < arg.len() {
                let c = arg[pos];
                if c == b'-' {
                    let name = &arg[pos + 1..];
                    if name.is_empty() {
                        options = false;
                        break;
                    }
                    long_option(&mut p, name);
                    if p.exited.is_some() {
                        break 'args;
                    }
                    break;
                }
                short_option(&mut p, c, &arg[pos..]);
                if p.exited.is_some() {
                    break 'args;
                }
                pos += 1;
            }
        } else if path.is_none() {
            path = Some(arg.clone());
            path_index = i as i32;
        } else {
            p.usage("argument PATH already specified as ", path.as_deref());
            if p.exited.is_some() {
                break 'args;
            }
        }
    }

    if p.exited.is_none() {
        // -q overrides -v
        if p.flags.quiet {
            p.flags.verbose = false;
        }
        // -c silently overrides -d and -f
        if p.flags.check {
            p.flags.delete = false;
            p.flags.force = false;
        }
        // -d silently overrides -f
        if p.flags.delete {
            p.flags.force = false;
        }
        if !have("z") && p.flags.decompress {
            p.usage("Option -z (--decompress) is not available", None);
        }
        if p.exited.is_none() && p.flags.zmax > 99 {
            p.usage("option --zmax argument exceeds upper limit", None);
        }
    }

    OptionsResult {
        flags: p.flags,
        arg_path_index: path_index,
        exit: Exit { exited: p.exited.is_some(), exit_code: p.exited.unwrap_or(0), warnings: p.warnings },
    }
}

/// one `--name` option, `name` is the text after the two dashes
fn long_option(p: &mut Parser, name: &[u8]) {
    if name.starts_with(b"accuracy=") && name.get(9).is_some_and(u8::is_ascii_digit) {
        p.flags.accuracy = i32::from(name[9] - b'0');
    } else if name == b"check" {
        p.flags.check = true;
    } else if name == b"decompress" {
        p.flags.decompress = true;
    } else if name == b"delete" {
        p.flags.delete = true;
    } else if name == b"dereference-files" {
        p.flags.dereference_files = true;
    } else if name == b"force" {
        p.flags.force = true;
    } else if name == b"help" {
        p.help();
    } else if name == b"hidden" {
        p.flags.hidden = true;
    } else if name == b"ignore-binary" {
        p.flags.ignore_binary = true;
    } else if name == b"ignore-files" {
        p.flags.ignore_files.push(DEFAULT_IGNORE_FILE.to_vec());
    } else if name.starts_with(b"ignore-files=") {
        p.flags.ignore_files.push(name[13..].to_vec());
    } else if name == b"no-messages" {
        p.flags.no_messages = true;
    } else if name == b"quiet" || name == b"silent" {
        p.flags.quiet = true;
        p.flags.no_messages = true;
    } else if name == b"verbose" {
        p.flags.verbose = true;
    } else if name == b"version" {
        p.version();
    } else if name.starts_with(b"zmax=") {
        p.flags.zmax = p.strtopos(&name[5..], "invalid argument --zmax=");
    } else {
        p.usage("invalid option --", Some(name));
    }
}

/// one grouped short option `c`; `rest` is the text from `c` to the end of the argument
fn short_option(p: &mut Parser, c: u8, rest: &[u8]) {
    match c {
        b'c' => p.flags.check = true,
        b'd' => p.flags.delete = true,
        b'f' => p.flags.force = true,
        b'I' => p.flags.ignore_binary = true,
        b'q' => {
            p.flags.quiet = true;
            p.flags.no_messages = true;
        }
        b'S' => p.flags.dereference_files = true,
        b's' => p.flags.no_messages = true,
        b'V' => p.version(),
        b'v' => p.flags.verbose = true,
        b'z' => p.flags.decompress = true,
        b'.' => p.flags.hidden = true,
        b'X' => p.flags.ignore_files.push(DEFAULT_IGNORE_FILE.to_vec()),
        b'?' => p.help(),
        _ if c.is_ascii_digit() => p.flags.accuracy = i32::from(c - b'0'),
        _ => p.usage("invalid option -", Some(rest)),
    }
}

/// the value of an environment variable as bytes
fn env_bytes(name: &str) -> Option<Vec<u8>> {
    let value = std::env::var_os(name)?;
    #[cfg(windows)]
    {
        Some(value.to_string_lossy().into_owned().into_bytes())
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(value.as_bytes().to_vec())
    }
}

/// The state `main` owns: the flags, the warning counter and the PATH argument
struct State {
    flags: IndexerFlags,
    warnings: usize,
    arg_path: Option<Bytes>,
}

impl State {
    /// runs `options` on `args` and stores the result; `Some(code)` when the process must exit
    fn parse(&mut self, args: &[Bytes]) -> Option<i32> {
        let r = options(&self.flags, args, self.arg_path.as_deref(), self.warnings);
        self.flags = r.flags;
        self.warnings = r.exit.warnings;
        if r.arg_path_index >= 0 {
            self.arg_path = Some(args[r.arg_path_index as usize].clone());
        }
        r.exit.exited.then_some(r.exit.exit_code)
    }

    /// load a .ugrep-indexer config file when present in the working or home directory
    fn load_config(&mut self, config_filename: &[u8]) -> Option<i32> {
        let mut file = open_mode(config_filename, "r").ok();
        if file.is_none() {
            let home = env_bytes(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
            if let Some(mut path) = home {
                path.push(if cfg!(windows) { b'\\' } else { b'/' });
                path.extend_from_slice(config_filename);
                file = open_mode(&path, "r").ok();
            }
        }
        let data = read_text(file?);
        // warn about invalid options but do not exit
        self.flags.usage_warnings = true;
        let mut pos = 0;
        while let Some(line) = getline(&data, &mut pos) {
            let line = trim(&line);
            // parse option or skip empty lines and comments
            if !line.is_empty() && line[0] != b'#' {
                let mut arg = b"--".to_vec();
                arg.extend_from_slice(line);
                if let Some(code) = self.parse(&[Vec::new(), arg]) {
                    return Some(code);
                }
            }
        }
        // bail out when config file has errors
        if self.warnings > 0 {
            return Some(1);
        }
        self.flags.usage_warnings = false;
        None
    }
}

/// `main`: returns the exit code of the process. `args` is argv.
pub fn run(args: &[Bytes]) -> i32 {
    if have("z") {
        sys::ignore_sigpipe();
    }
    let mut state = State {
        flags: IndexerFlags {
            accuracy: 4,
            check: false,
            decompress: false,
            delete: false,
            dereference_files: false,
            force: false,
            hidden: false,
            ignore_binary: false,
            no_messages: false,
            quiet: false,
            usage_warnings: false,
            verbose: false,
            zmax: 1,
            ignore_files: Vec::new(),
        },
        warnings: 0,
        arg_path: None,
    };
    if let Some(code) = state.load_config(CONFIG_FILENAME) {
        return code;
    }
    if let Some(code) = state.parse(args) {
        return code;
    }
    let path = state.arg_path.as_deref();
    let exit = if state.flags.delete {
        indexer_walk::deleter(&state.flags, path, state.warnings)
    } else {
        indexer_walk::indexer(&state.flags, path, state.warnings)
    };
    if exit.exited {
        exit.exit_code
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> IndexerFlags {
        IndexerFlags {
            accuracy: 4,
            check: false,
            decompress: false,
            delete: false,
            dereference_files: false,
            force: false,
            hidden: false,
            ignore_binary: false,
            no_messages: false,
            quiet: false,
            usage_warnings: false,
            verbose: false,
            zmax: 1,
            ignore_files: Vec::new(),
        }
    }

    fn argv(a: &[&str]) -> Vec<Bytes> {
        std::iter::once("ugrep-indexer").chain(a.iter().copied()).map(|s| s.as_bytes().to_vec()).collect()
    }

    #[test]
    fn strtoull_like_c() {
        assert_eq!(strtoull(b"12"), (12, &b""[..]));
        assert_eq!(strtoull(b"  +7x"), (7, &b"x"[..]));
        assert_eq!(strtoull(b"-1"), (u64::MAX, &b""[..]));
        assert_eq!(strtoull(b"x"), (0, &b"x"[..]));
        assert_eq!(strtoull(b""), (0, &b""[..]));
        assert_eq!(strtoull(b"99999999999999999999999"), (u64::MAX, &b""[..]));
    }

    #[test]
    fn accuracy_and_overrides() {
        let r = options(&defaults(), &argv(&["-c", "-d", "-f", "-q", "-v", "--accuracy=10", "dir"]), None, 0);
        assert!(!r.exit.exited);
        // `--accuracy=10` reads one digit (Q-indexer-1), -c overrides -d and -f, -q overrides -v
        assert_eq!(r.flags.accuracy, 1);
        assert!(r.flags.check && !r.flags.delete && !r.flags.force && !r.flags.verbose && r.flags.quiet);
        assert_eq!(r.arg_path_index, 7);
    }

    #[test]
    fn usage_error_exits_zero() {
        let r = options(&defaults(), &argv(&["--zmax=0"]), None, 0);
        assert_eq!(r.exit, Exit { exited: true, exit_code: 0, warnings: 0 });
        let r = options(&defaults(), &argv(&["a", "b"]), None, 0);
        assert!(r.exit.exited && r.exit.exit_code == 0);
    }

    #[test]
    fn config_line_warns() {
        let mut f = defaults();
        f.usage_warnings = true;
        let r = options(&f, &argv(&["--bogus"]), None, 0);
        assert_eq!(r.exit, Exit { exited: false, exit_code: 0, warnings: 1 });
    }

    #[test]
    fn help_text_without_codecs() {
        let h = help_text();
        assert!(h.contains("This option is not available in this build of ugrep-indexer."));
        assert!(h.ends_with("outdated index files.\n\n"));
    }
}
