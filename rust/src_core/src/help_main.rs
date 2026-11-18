//! Messages of src/ugrep.cpp: `usage`, `help`, `version`, `warning`, `error`, `abort`, `is_directory` and
//! `cannot_decompress`. Every function returns the bytes the C++ wrote; the C-ABI entries (ffi/help_main.rs) write
//! them to the C++ streams and leave `exit()` to the C++ caller.
//!
//! The help text is the output recorded from the C++ `ugrep --help` (never retyped); the build-dependent parts of
//! the C++ (`#if` of PCRE2/Boost, the compression libraries, Windows mode, grep mode) are substituted from the flags
//! the C++ passes in.

/// `ugrep --help` recorded from the C++ Base build: the help text, then the trailer of `help(WHAT)`.
const HELP_RECORDED: &str = include_str!("../../../migration/corpus/src_core/help/help.txt");
const HELP_TRAILER: &str = "\n    Specify `ugrep --help WHAT' for help on WHAT.\n\n";

/// Text printed by `help(WHAT)` after the option search for a few WHATs, recorded from the C++ binary.
const TOPIC_FORMAT: &str = include_str!("../assets/help/format.txt");
const TOPIC_REGEX: &str = include_str!("../assets/help/regex.txt");
const TOPIC_GLOB: &str = include_str!("../assets/help/glob.txt");
const TOPIC_FUZZY: &str = include_str!("../assets/help/fuzzy.txt");

// markers of the recorded text around the build-dependent parts
const PERL_START: &str = "Interpret PATTERN as a Perl regular expression";
const PERL_END: &str = "            Note that Perl pattern matching differs from the default grep";
const Z_START: &str = "            filename extensions that match known compression and archive types.\n";
const Z_END: &str = "    --zmax=NUM\n";
const ZMAX_START: &str = "Increasing NUM values gradually degrades performance.\n";
const WINDOWS_LINE: &str = "\nWindows mode: short options may start with `-' or with `/'.\n";
const GREP_LINE: &str = "\nGrep compatibility mode: -Z and -z reassigned to --null and --null-data.\n";
const NOT_AVAILABLE: &str = "            This option is not available in this build configuration of ugrep.\n";

/// Regex engine of the build for option -P.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Perl {
    None,
    Pcre2,
    Boost,
}

/// The `#if` conditions of the C++ help text and version.
#[derive(Clone, Copy, Debug)]
pub struct BuildConfig {
    pub perl: Perl,
    pub zlib: bool,
    pub no_7zip: bool,
    pub bzip2: bool,
    pub lzma: bool,
    pub zstd: bool,
    pub lz4: bool,
    pub brotli: bool,
    pub bzip3: bool,
    pub decompression_thread: bool,
    pub os_win: bool,
    /// `flag_grep` at the time of the call
    pub grep: bool,
}

/// What stays C++ for `version()`: the build constants and the CPU feature strings.
pub struct VersionInfo<'a> {
    pub version: &'a [u8],
    pub platform: &'a [u8],
    pub simd: &'a [u8],
    pub regex: &'a [u8],
    /// `flag_mode`: 2 is BRE, 3 is FIXED
    pub mode: i32,
}

/// The ANSI color strings of the settings.
pub struct Colors<'a> {
    pub off: &'a [u8],
    pub high: &'a [u8],
    pub error: &'a [u8],
    pub warning: &'a [u8],
    pub message: &'a [u8],
    pub fn_: &'a [u8],
}

fn find(hay: &str, needle: &str, from: usize) -> Option<usize> {
    hay.get(from..)?.find(needle).map(|i| i + from)
}

fn replace_between(text: &mut String, start: &str, end: &str, with: &str, include_start: bool) {
    let Some(s) = text.find(start) else { return };
    let from = if include_start { s } else { s + start.len() };
    let Some(e) = find(text, end, from) else { return };
    text.replace_range(from..e, with);
}

/// `help(std::ostream&)`: the recorded help text for this build configuration, without the trailer.
pub fn help_text(cfg: &BuildConfig) -> String {
    let mut text = HELP_RECORDED.strip_suffix(HELP_TRAILER).unwrap_or(HELP_RECORDED).to_string();

    let perl = match cfg.perl {
        Perl::Pcre2 => " using PCRE2.\n".to_string(),
        Perl::Boost => " using Boost.Regex.\n".to_string(),
        Perl::None => format!(".\n{NOT_AVAILABLE}"),
    };
    replace_between(&mut text, PERL_START, PERL_END, &perl, false);

    let mut z = String::new();
    if !cfg.zlib {
        z.push_str(NOT_AVAILABLE);
    } else {
        z.push_str("            Supported compression formats: gzip (.gz), compress (.Z), zip");
        if !cfg.no_7zip {
            z.push_str(", 7z");
        }
        if cfg.bzip2 {
            z.push_str(",\n            bzip2 (.bz, .bz2, .bzip2, .tbz, .tbz2, .tb2, .tz2)");
        }
        if cfg.lzma {
            z.push_str(",\n            xz (.xz, .txz) and lzma (requires suffix .lzma, .tlz)");
        }
        if cfg.zstd {
            z.push_str(",\n            zstd (.zst, .zstd, .tzst)");
        }
        if cfg.lz4 {
            z.push_str(",\n            lz4 (requires suffix .lz4)");
        }
        if cfg.brotli {
            z.push_str(",\n            brotli (requires suffix .br)");
        }
        if cfg.bzip3 {
            z.push_str(",\n            bzip3 (requires suffix .bz3)");
        }
        z.push_str(".\n");
    }
    if let Some(s) = text.find(Z_START) {
        let from = s + Z_START.len();
        if let Some(e) = find(&text, Z_END, from) {
            text.replace_range(from..e, &z);
        }
    }

    // the part after the --zmax text: not-available note, grep and Windows mode lines, then the fixed footer
    if let Some(w) = text.rfind(WINDOWS_LINE) {
        let footer = text[w + WINDOWS_LINE.len()..].to_string();
        text.truncate(w);
        if let Some(z) = text.rfind(ZMAX_START) {
            text.truncate(z + ZMAX_START.len());
        }
        if !cfg.decompression_thread {
            text.push_str(NOT_AVAILABLE);
        }
        if cfg.grep {
            text.push_str(GREP_LINE);
        }
        if cfg.os_win {
            text.push_str(WINDOWS_LINE);
        }
        text.push_str(&footer);
    }
    text
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn find_bytes(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > hay.len() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|i| i + from)
}

/// `help(const char *what)`: the bytes written to standard output; the process exits with status 2 afterwards.
pub fn help_what(what: Option<&[u8]>, cfg: &BuildConfig) -> Vec<u8> {
    let mut what = what;
    // strip = from =WHAT
    if let Some(w) = what {
        if w.first() == Some(&b'=') {
            what = Some(&w[1..]);
        }
    }
    // strip --no from --no-WHAT
    if let Some(w) = what {
        if w.starts_with(b"--no") {
            what = Some(&w[4..]);
        }
    }
    // strip one dash from --WHAT
    if let Some(w) = what {
        if w.starts_with(b"--") {
            what = Some(&w[1..]);
        }
    }

    let text = help_text(cfg);
    let what = match what {
        Some(w) if !w.is_empty() => w,
        _ => {
            let mut out = text.into_bytes();
            out.extend_from_slice(HELP_TRAILER.as_bytes());
            return out;
        }
    };

    let s = text.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut found = 0;

    for pass in 0..2 {
        let mut pos = 0_usize;
        while let Some(end) = find_bytes(s, b"\n    -", pos + 1).or_else(|| find_bytes(s, b"\n\n", pos + 1)) {
            let nl = find_bytes(s, b"\n", pos + 1).unwrap_or(usize::MAX);

            // roughly find a case-independent match of WHAT
            let limit = if pass == 0 { nl } else { end };
            let mut i = pos + 5;
            while i < limit {
                let mut j = 0;
                while j < what.len() {
                    match s.get(i + j) {
                        Some(&c) if (what[j] ^ c) & !0x20 == 0 => j += 1,
                        _ => break,
                    }
                }
                let at_end = j == what.len();
                if at_end || what[j] == b'=' || is_space(what[j]) {
                    if if pass == 0 { i < nl } else { i > nl } {
                        if found == 0 && pass == 0 {
                            out.extend_from_slice(b"\nOptions and arguments:\n");
                        } else if found == 1 && pass == 1 {
                            out.extend_from_slice(b"\n\nRelated options:\n");
                        } else if found == 0 {
                            out.extend_from_slice(b"\nNo matching option, other relevant options:\n");
                        }
                        out.extend_from_slice(&s[pos..end]);
                        found = pass + 1;
                    }
                    break;
                }
                i += 1;
            }
            pos = end;
        }
    }

    if found == 0 {
        out.extend_from_slice(b"ugrep --help: nothing appropriate for ");
        out.extend_from_slice(what);
    } else {
        out.extend_from_slice(b"\n\n    Long options may start with `--no-' to disable, when applicable.");
    }
    out.extend_from_slice(b"\n\n");

    let topic = match what {
        b"format" => Some(TOPIC_FORMAT),
        b"regex" => Some(TOPIC_REGEX),
        b"glob" | b"globs" | b"globbing" => Some(TOPIC_GLOB),
        b"fuzzy" => Some(TOPIC_FUZZY),
        _ => None,
    };
    if let Some(t) = topic {
        out.extend_from_slice(t.as_bytes());
    }
    out
}

/// `usage()`: the text for standard error. When `message` is not `NULL`; `arg` and `valid` may be absent.
pub fn usage_text(message: &[u8], arg: Option<&[u8]>, valid: Option<&[u8]>) -> Vec<u8> {
    let mut out = b"ugrep: ".to_vec();
    out.extend_from_slice(message);
    out.extend_from_slice(arg.unwrap_or(b""));
    if let Some(valid) = valid {
        out.extend_from_slice(b", did you mean ");
        out.extend_from_slice(valid);
        out.extend_from_slice(b"?\nFor more help, run `ugrep --help' or `ugrep --help WHAT'\n");
        return out;
    }
    out.extend_from_slice(b"\nFor more help, run `ugrep --help' or `ugrep --help ");
    let s = message.iter().position(|&c| c == b'-').unwrap_or(message.len());
    if s == message.len() {
        out.extend_from_slice(b"WHAT'\n");
        return out;
    }
    let mut e = s;
    while e < message.len() && message[e] == b'-' {
        e += 1;
    }
    if e == message.len() {
        match arg {
            Some(arg) => {
                out.extend_from_slice(&message[s..e]);
                out.extend_from_slice(arg);
                out.extend_from_slice(b"'\n");
            }
            None => out.extend_from_slice(b"WHAT'\n"),
        }
    } else {
        while e < message.len() && (message[e] == b'-' || message[e].is_ascii_alphabetic()) {
            e += 1;
        }
        out.extend_from_slice(&message[s..e]);
        out.extend_from_slice(b"'\n");
    }
    out
}

/// `version()`: the text for standard output.
pub fn version_text(info: &VersionInfo, cfg: &BuildConfig) -> Vec<u8> {
    let mut out = b"ugrep ".to_vec();
    out.extend_from_slice(info.version);
    if cfg.grep {
        out.extend_from_slice(b" (");
        out.extend_from_slice(match info.mode {
            2 => b"",
            3 => b"f",
            _ => b"e",
        });
        out.extend_from_slice(b"grep compat)");
    }
    out.push(b' ');
    out.extend_from_slice(info.platform);
    out.extend_from_slice(info.simd);
    out.extend_from_slice(info.regex);
    if cfg.zlib {
        out.extend_from_slice(b"; -z:zlib");
        for (on, name) in [
            (cfg.bzip2, &b",bzip2"[..]),
            (cfg.lzma, b",lzma"),
            (cfg.lz4, b",lz4"),
            (cfg.zstd, b",zstd"),
            (cfg.brotli, b",brotli"),
            (cfg.bzip3, b",bzip3"),
            (!cfg.no_7zip, b",7z"),
        ] {
            if on {
                out.extend_from_slice(name);
            }
        }
        out.extend_from_slice(b",tar/pax/cpio/zip");
    }
    out.extend_from_slice(
        b"\nLicense: BSD-3-Clause; ugrep user manual: <https://ugrep.com>\n\
Written by Robert van Engelen and others: <https://github.com/Genivia/ugrep>\n\
Ugrep utilizes the RE/flex regex library: <https://github.com/Genivia/RE-flex>\n",
    );
    out
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

/// `is_directory()`
pub fn is_directory_text(c: &Colors, pathname: &[u8]) -> Vec<u8> {
    cat(&[c.off, b"ugrep: ", c.high, pathname, c.off, b" is a directory\n"])
}

/// `cannot_decompress()`
pub fn cannot_decompress_text(c: &Colors, pathname: &[u8], message: Option<&[u8]>) -> Vec<u8> {
    cat(&[c.off, b"ugrep: cannot decompress ", c.fn_, pathname, c.off, b": ", message.unwrap_or(b""), b"\n"])
}

/// `warning()`: `errmsg` is the text of `errno` when it is set.
pub fn warning_text(c: &Colors, message: Option<&[u8]>, arg: Option<&[u8]>, errmsg: Option<&[u8]>) -> Vec<u8> {
    cat(&[
        c.off,
        b"ugrep: ",
        c.warning,
        b"warning:",
        c.off,
        b" ",
        c.high,
        message.unwrap_or(b""),
        if message.is_some() { b" " } else { b"" },
        arg.unwrap_or(b""),
        if errmsg.is_some() { b":" } else { b" " },
        c.off,
        b" ",
        c.message,
        errmsg.unwrap_or(b""),
        c.off,
        b"\n",
    ])
}

/// `error()`: `errmsg` is the text of `errno`.
pub fn error_text(c: &Colors, message: Option<&[u8]>, arg: Option<&[u8]>, errmsg: &[u8]) -> Vec<u8> {
    cat(&[
        c.off,
        b"ugrep: ",
        c.error,
        b"error:",
        c.off,
        b" ",
        c.high,
        message.unwrap_or(b""),
        if message.is_some() { b" " } else { b"" },
        arg.unwrap_or(b""),
        b":",
        c.off,
        b" ",
        c.message,
        errmsg,
        c.off,
        b"\n\n",
    ])
}

/// `abort(message)`
pub fn abort_text(c: &Colors, message: &[u8]) -> Vec<u8> {
    cat(&[c.off, b"ugrep: ", c.error, message, c.off, b"\n\n"])
}

/// `abort(message, what)`
pub fn abort_what_text(c: &Colors, message: Option<&[u8]>, what: &[u8]) -> Vec<u8> {
    cat(&[c.off, b"ugrep: ", c.error, message.unwrap_or(b""), c.off, c.high, what, c.off, b"\n\n"])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// the configuration of the recorded text: the Base build on Windows without third-party libraries
    const BASE: BuildConfig = BuildConfig {
        perl: Perl::None,
        zlib: false,
        no_7zip: false,
        bzip2: false,
        lzma: false,
        zstd: false,
        lz4: false,
        brotli: false,
        bzip3: false,
        decompression_thread: true,
        os_win: true,
        grep: false,
    };

    #[test]
    fn recorded_text_is_reproduced_for_the_recorded_configuration() {
        let full = help_what(None, &BASE);
        assert_eq!(String::from_utf8(full).unwrap(), HELP_RECORDED);
    }

    #[test]
    fn build_dependent_parts_are_substituted() {
        let cfg = BuildConfig { perl: Perl::Pcre2, zlib: true, bzip2: true, brotli: true, os_win: false, grep: true, decompression_thread: false, ..BASE };
        let t = help_text(&cfg);
        assert!(t.contains("Perl regular expression using PCRE2.\n            Note that Perl"));
        assert!(t.contains("zip, 7z,\n            bzip2 (.bz, .bz2, .bzip2, .tbz, .tbz2, .tb2, .tz2),\n            brotli (requires suffix .br).\n    --zmax=NUM\n"));
        assert!(t.contains("performance.\n            This option is not available in this build configuration of ugrep.\n\nGrep compatibility mode"));
        assert!(!t.contains("Windows mode"));
    }

    #[test]
    fn usage_variants() {
        assert_eq!(usage_text(b"invalid option -x", None, None), b"ugrep: invalid option -x\nFor more help, run `ugrep --help' or `ugrep --help -x'\n");
        assert_eq!(usage_text(b"missing argument for --", Some(b"abc"), None), b"ugrep: missing argument for --abc\nFor more help, run `ugrep --help' or `ugrep --help --abc'\n");
        assert_eq!(usage_text(b"oops", None, Some(b"--foo")), b"ugrep: oops, did you mean --foo?\nFor more help, run `ugrep --help' or `ugrep --help WHAT'\n");
        assert_eq!(usage_text(b"oops", None, None), b"ugrep: oops\nFor more help, run `ugrep --help' or `ugrep --help WHAT'\n");
    }

    #[test]
    fn help_what_unmatched() {
        let out = help_what(Some(b"nosuchword"), &BASE);
        assert_eq!(out, b"ugrep --help: nothing appropriate for nosuchword\n\n");
    }
}
