//! Types-and-signatures module for the `output` seam (src/output.cpp, src/output.hpp). See rust/src_core/SEAM.md,
//! section Seam: output.
//!
//! The shared structs are declared in the cxx bridge (`lib.rs`, module `ffi`) and are the types of these signatures.
//! The C++ facade (src/output.cpp) calls them; Rust is the only implementation.
//!
//! Shape of the seam: every function is a pure function of its inputs (except `--tree` headers, which stay in C++
//! because they take the Sync lock and update the process-wide `Output::Tree` state). C++ keeps the `Output` object (buffer,
//! `Sync`, hex-dump state, `FILE*`), builds a plain-data snapshot per call, and appends the returned bytes to its
//! buffer with `Output::str`. The Rust side reads the option state it already owns (`settings`, `color`).

use super::ffi::{
    OutputArgs, OutputBinaryResult, OutputCtx, OutputDumpLine, OutputHeaderResult, OutputMatch,
};

/// `Output::quote(data, size)`: the text appended to the buffer.
pub fn quote(data: &[u8]) -> Vec<u8> {
    let mut out = Out::default();
    out.quote(data);
    out.0
}

/// `Output::cpp(data, size)`.
pub fn cpp(data: &[u8]) -> Vec<u8> {
    let mut out = Out::default();
    out.cpp(data);
    out.0
}

/// `Output::csv(data, size)`.
pub fn csv(data: &[u8]) -> Vec<u8> {
    let mut out = Out::default();
    out.csv(data);
    out.0
}

/// `Output::hex(data, size)`.
pub fn hex(data: &[u8]) -> Vec<u8> {
    let mut out = Out::default();
    out.hex_bytes(data);
    out.0
}

/// `Output::json(data, size)`.
pub fn json(data: &[u8]) -> Vec<u8> {
    let mut out = Out::default();
    out.json(data);
    out.0
}

/// `Output::xml(data, size)`.
pub fn xml(data: &[u8]) -> Vec<u8> {
    let mut out = Out::default();
    out.xml(data);
    out.0
}

/// The output state of the process: the options of `Settings` that the ported functions read, as plain values.
/// `ffi::output::opts()` builds it from the exported settings object; tests build it directly. Colour strings are the
/// bytes before the NUL; `hl` is `None` for a NULL `color_hl` (no `--hyperlink`).
#[derive(Clone, Copy, Default)]
pub struct Opts<'a> {
    pub color_sl: &'a [u8],
    pub color_cx: &'a [u8],
    pub color_fn: &'a [u8],
    pub color_ln: &'a [u8],
    pub color_cn: &'a [u8],
    pub color_bn: &'a [u8],
    pub color_se: &'a [u8],
    pub color_mt: &'a [u8],
    pub color_ms: &'a [u8],
    pub color_mc: &'a [u8],
    pub match_ms: &'a [u8],
    pub match_mc: &'a [u8],
    pub match_off: &'a [u8],
    pub color_off: &'a [u8],
    pub color_st: &'a [u8],
    pub color_del: &'a [u8],
    pub color_hl: Option<&'a [u8]>,
    pub color: bool,
    pub no_filename: bool,
    pub line_number: bool,
    pub column_number: bool,
    pub byte_offset: bool,
    pub query: bool,
    pub text: bool,
    pub null: bool,
    pub heading: bool,
    pub initial_tab: bool,
    pub hyperlink_line: bool,
    pub hex_star: bool,
    pub hex_cbr: bool,
    pub hex_chr: bool,
    pub hex_hbr: bool,
    pub hex_columns: usize,
    pub width: usize,
    pub separator: &'a [u8],
    pub with_filename: bool,
    pub brk: bool,
    pub files_with_matches: bool,
    pub count: bool,
    pub invert_match: bool,
    pub only_matching: bool,
    pub hex_all: bool,
    pub with_hex: bool,
    pub match_all: bool,
    pub ungroup: bool,
    pub fuzzy: usize,
    /// `--hyperlink` strings that `std::string` statics keep in C++ (`flag_hyperlink_prefix`, `_host`, `_path`).
    pub hyperlink_prefix: &'a [u8],
    pub hyperlink_host: &'a [u8],
    pub hyperlink_path: &'a [u8],
}

/// `nl()` appends the platform newline: `\r\n` on Windows, `\n` elsewhere.
pub(crate) const NL: &[u8] = if cfg!(windows) { b"\r\n" } else { b"\n" };

/// The letter of the escape for `\b \f \n \r \t`, else the byte itself.
fn escape_letter(c: u8) -> u8 {
    match c {
        0x08 => b'b',
        0x0c => b'f',
        b'\n' => b'n',
        b'\r' => b'r',
        b'\t' => b't',
        _ => c,
    }
}

/// The appenders of `Output` (`chr`, `str`, `num`, `hex`, `uri`, `nl`) on a byte vector.
#[derive(Default)]
pub(crate) struct Out(pub(crate) Vec<u8>);

impl Out {
    pub(crate) fn chr(&mut self, c: u8) {
        self.0.push(c);
    }

    pub(crate) fn str(&mut self, s: &[u8]) {
        self.0.extend_from_slice(s);
    }

    pub(crate) fn nl(&mut self) {
        self.0.extend_from_slice(NL);
    }

    /// `num(i, w)`: decimal, padded with spaces to width `w`.
    pub(crate) fn num(&mut self, i: usize, w: usize) {
        let digits = i.to_string();
        self.0.resize(self.0.len() + w.saturating_sub(digits.len()), b' ');
        self.str(digits.as_bytes());
    }

    /// `hex(i, w)`: lower case hexadecimal, padded with `0` to width `w`.
    pub(crate) fn hex(&mut self, i: usize, w: usize) {
        let digits = format!("{i:x}");
        self.0.resize(self.0.len() + w.saturating_sub(digits.len()), b'0');
        self.str(digits.as_bytes());
    }

    /// `uri(s)`: URI-encoded string `s` (a C string: ends at the first NUL).
    fn uri(&mut self, s: &[u8]) {
        for &b in crate::color::c_str(s) {
            if (0x20..=0x7e).contains(&b) && b != b'%' && b != b';' {
                self.chr(b);
            } else {
                self.chr(b'%');
                self.hex(usize::from(b), 2);
            }
        }
    }

    /// `open_hyperlink(pathname, linkpos, lineno, columno)`: `linkpos` is the `(lineno, columno)` position to link to;
    /// only called when `color_hl` is set.
    fn open_hyperlink(&mut self, o: &Opts, pathname: &[u8], linkpos: Option<(usize, usize)>) {
        self.str(o.color_hl.unwrap_or_default());
        self.str(crate::color::c_str(o.hyperlink_prefix));
        self.str(b"://");
        self.uri(o.hyperlink_host);
        if pathname.first() != Some(&b'/') {
            self.chr(b'/');
            self.uri(o.hyperlink_path);
            self.chr(b'/');
        }
        self.uri(pathname);
        if let Some((lineno, columno)) = linkpos {
            self.chr(b':');
            self.num(lineno, 0);
            if o.column_number {
                self.chr(b':');
                self.num(columno, 0);
            }
        }
        self.str(o.color_st);
    }

    /// `oct(i)`: one byte in three octal digits.
    fn oct(&mut self, c: u8) {
        self.chr(b'0' + (c >> 6));
        self.chr(b'0' + ((c >> 3) & 7));
        self.chr(b'0' + (c & 7));
    }

    /// `Output::quote(data, size)`: quoted with a backslash before `\` and `"`.
    pub(crate) fn quote(&mut self, data: &[u8]) {
        self.chr(b'"');
        for &c in data {
            if c == b'\\' || c == b'"' {
                self.chr(b'\\');
            }
            self.chr(c);
        }
        self.chr(b'"');
    }

    /// `Output::cpp(data, size)`: C/C++ string, non-ASCII bytes verbatim.
    pub(crate) fn cpp(&mut self, data: &[u8]) {
        self.chr(b'"');
        for &c in data {
            if c & 0x80 == 0 && (c < 0x20 || c == b'"' || c == b'\\') {
                let c = escape_letter(c);
                self.chr(b'\\');
                if c > 0x20 {
                    self.chr(c);
                } else {
                    self.oct(c);
                }
            } else {
                self.chr(c);
            }
        }
        self.chr(b'"');
    }

    /// `Output::csv(data, size)`.
    pub(crate) fn csv(&mut self, data: &[u8]) {
        self.chr(b'"');
        for &c in data {
            if c & 0x80 != 0 {
                self.chr(c);
            } else if c == b'"' {
                self.str(b"\"\"");
            } else if (c < 0x20 && c != b'\t') || c == b'\\' {
                let c = escape_letter(c);
                if c > 0x20 {
                    self.chr(b'\\');
                    self.chr(c);
                } else {
                    self.str(b"\\x");
                    self.hex(usize::from(c), 2);
                }
            } else {
                self.chr(c);
            }
        }
        self.chr(b'"');
    }

    /// `Output::hex(data, size)`: bytes as two hex digits, separated by a space.
    pub(crate) fn hex_bytes(&mut self, data: &[u8]) {
        for (i, &c) in data.iter().enumerate() {
            if i > 0 {
                self.chr(b' ');
            }
            self.hex(usize::from(c), 2);
        }
    }

    /// `Output::json(data, size)`.
    pub(crate) fn json(&mut self, data: &[u8]) {
        self.chr(b'"');
        for &c in data {
            if c & 0x80 == 0 && (c < 0x20 || c == b'"' || c == b'\\') {
                let c = escape_letter(c);
                if c > 0x20 {
                    self.chr(b'\\');
                    self.chr(c);
                } else {
                    self.str(b"\\u");
                    self.hex(usize::from(c), 4);
                }
            } else {
                self.chr(c);
            }
        }
        self.chr(b'"');
    }

    /// `Output::xml(data, size)`.
    pub(crate) fn xml(&mut self, data: &[u8]) {
        for &c in data {
            if c & 0x80 != 0 {
                self.chr(c);
                continue;
            }
            match c {
                b'&' => self.str(b"&amp;"),
                b'<' => self.str(b"&lt;"),
                b'>' => self.str(b"&gt;"),
                b'"' => self.str(b"&quot;"),
                0x7f => self.str(b"&#x7f;"),
                0..=0x1f => {
                    self.str(b"&#");
                    self.num(usize::from(c), 0);
                    self.chr(b';');
                }
                _ => self.chr(c),
            }
        }
    }

    fn close_hyperlink(&mut self, o: &Opts) {
        self.str(o.color_hl.unwrap_or_default());
        self.str(o.color_st);
    }
}

/// `Output::color(arg)`: `arg` is the two-letter colour code, `None` (`has_arg` false) is `color_off`.
pub fn color(o: &Opts, has_arg: bool, arg: &[u8]) -> Vec<u8> {
    if !has_arg {
        return o.color_off.to_vec();
    }
    let colors = [
        o.color_sl, o.color_cx, o.color_mt, o.color_ms, o.color_mc, o.color_fn, o.color_ln, o.color_cn, o.color_bn,
        o.color_se,
    ];
    const CODES: &[u8] = b"sl cx mt ms mc fn ln cn bn se ";
    // the C code is the C string { arg[0], arg[1], ' ' }: it ends at the first NUL
    let at = |i: usize| arg.get(i).copied().unwrap_or(0);
    let mut code = vec![at(0)];
    if at(0) != 0 {
        code.push(at(1));
        if at(1) != 0 {
            code.push(b' ');
        }
    }
    let code = crate::color::c_str(&code);
    match crate::color::find(CODES, code) {
        Some(pos) => colors[pos / 3].to_vec(),
        None => Vec::new(),
    }
}

/// `Output::Dump::line()`: one hex dump line from `bytes`/`prevb` (`-1` is an unused column).
pub fn dump_line(o: &Opts, bytes: &[i16], prevb: &[i16], pstar: bool, offset: usize) -> OutputDumpLine {
    let cols = o.hex_columns;
    let at = |v: &[i16], i: usize| v.get(i).copied().unwrap_or(-1);
    let color_hex = [o.match_ms, o.color_sl, o.match_mc, o.color_cx];
    let mut out = Out::default();

    if o.hex_star {
        // if the previous hex line was the same as this hex line, output a * (but only once)
        let same = (0..cols).all(|i| at(prevb, i) >= 0 && at(bytes, i) == at(prevb, i));
        if same {
            if !pstar {
                out.str(o.color_se);
                out.chr(b'*');
                out.nl();
            }
            return OutputDumpLine { bytes: out.0, pstar: true, repeated: true };
        }
    }

    out.str(o.color_bn);
    let line_offset = offset.wrapping_sub(1);
    out.hex(line_offset - line_offset % cols.max(1), 8);
    out.str(o.color_off);
    out.chr(b' ');

    // HEX_MAX (4) is "no colour yet"
    let mut last: i16 = 4;
    let hex_color = |c: i16| color_hex.get(usize::try_from(c).unwrap_or(usize::MAX)).copied().unwrap_or(&[]);
    let color_change = |out: &mut Out, last: &mut i16, to: i16| {
        out.str(if *last == 0 || *last == 2 { o.match_off } else { o.color_off });
        *last = to;
        out.str(hex_color(to));
    };

    for i in 0..cols {
        let byte = at(bytes, i);
        if byte < 0 {
            if last != -1 {
                last = -1;
                out.str(o.color_off);
                out.str(o.color_cx);
            }
            if o.hex_hbr || (i == 0 && o.hex_cbr) {
                out.chr(b' ');
            }
            out.str(b"--");
            if o.hex_cbr && (i & 7) == 7 {
                out.chr(b' ');
            }
        } else {
            if (byte >> 8) != last {
                color_change(&mut out, &mut last, byte >> 8);
            }
            if o.hex_hbr || (i == 0 && o.hex_cbr) {
                out.chr(b' ');
            }
            out.hex(usize::from((byte & 0xff) as u8), 2);
            if o.hex_cbr && (i & 7) == 7 {
                out.chr(b' ');
            }
        }
    }

    out.str(o.color_off);

    if o.hex_chr {
        out.str(o.color_se);
        if o.hex_hbr {
            out.chr(b' ');
        }
        out.chr(b'|');

        last = 4;
        let mut inverted = false;

        for i in 0..cols {
            let byte = at(bytes, i);
            if byte < 0 {
                if last != -1 {
                    last = -1;
                    out.str(o.color_off);
                    out.str(o.color_cx);
                }
                out.chr(b'-');
            } else {
                if (byte >> 8) != last {
                    color_change(&mut out, &mut last, byte >> 8);
                }
                let byte = (byte & 0xff) as u8;
                if o.color {
                    if byte < 0x20 {
                        out.str(b"\x1b[7m");
                        out.chr(b'@' + byte);
                        inverted = true;
                    } else if byte == 0x7f {
                        out.str(b"\x1b[7m~");
                        inverted = true;
                    } else if byte > 0x7f {
                        out.str(b"\x1b[7m.");
                        inverted = true;
                    } else if inverted {
                        out.str(o.color_off);
                        out.str(hex_color(last));
                        out.chr(byte);
                        inverted = false;
                    } else {
                        out.chr(byte);
                    }
                } else if !(0x20..0x7f).contains(&byte) {
                    out.chr(b'.');
                } else {
                    out.chr(byte);
                }
            }
        }

        out.str(o.color_off);
        out.str(o.color_se);
        out.chr(b'|');
        out.str(o.color_off);
    }

    out.nl();
    OutputDumpLine { bytes: out.0, pstar: false, repeated: false }
}

/// `Output::header(pathname, partname, heading, lineno, matcher, byte_offset, separator, newline)`.
/// `window` is the matcher's line when the format needs it (see `OutputMatch`); `matcher.present` false is a NULL matcher; the separator is
/// `args.has_separator` and `separator`.
pub fn header_line(
    o: &Opts,
    pathname: &[u8],
    partname: &[u8],
    separator: &[u8],
    args: &OutputArgs,
    matcher: &OutputMatch,
    ctx: &OutputCtx,
) -> OutputHeaderResult {
    let mut heading = args.heading;
    let lineno = args.lineno;
    let separator = if args.has_separator { separator } else { &[] };
    let has_part = !o.no_filename && !partname.is_empty();

    // if hex dump line is incomplete and a header is output, then complete the hex dump first
    let complete_dump = ctx.dump_incomplete
        && (heading || has_part || o.line_number || o.column_number || o.byte_offset);

    // get column number when we need it
    let columno = if o.column_number && matcher.present { matcher.columno.wrapping_add(1) } else { 1 };

    let mut out = Out::default();

    // -Q: mark pathname with three \0 markers in headings, unless -a
    let mut nul = o.query && heading && !o.text;
    if nul {
        out.chr(0);
    }

    // --hyperlink: open link, unless standard input
    let hyp = !ctx.stdin_label && o.color_hl.is_some();
    if hyp {
        let linkpos = (!(heading && o.heading) && o.hyperlink_line).then_some((lineno, columno));
        out.open_hyperlink(o, pathname, linkpos);
    }

    // when a separator is needed
    let mut sep = false;

    // header should include the pathname
    if heading {
        out.str(o.color_fn);
        if nul {
            out.chr(0);
        }
        out.str(pathname);
        if nul {
            out.chr(0);
        }
        out.str(o.color_off);
        if o.null {
            out.chr(0);
        }
        if o.heading {
            // --hyperlink: close link
            if hyp {
                out.close_hyperlink(o);
            }
            out.str(o.color_fn);
            out.str(o.color_del);
            out.str(o.color_off);
            out.nl();
            // --hyperlink: open link
            if hyp {
                out.open_hyperlink(o, pathname, o.hyperlink_line.then_some((lineno, columno)));
            }
            // the next headers should not include this pathname
            heading = false;
        } else {
            sep = !o.null;
        }
    }

    if has_part {
        nul = o.query && !o.text && (o.heading || !nul);
        if nul {
            out.chr(0);
        }
        out.str(o.color_fn);
        if nul {
            out.chr(0);
        }
        out.chr(b'{');
        out.str(partname);
        out.chr(b'}');
        if nul {
            out.chr(0);
        }
        out.str(o.color_off);
        sep = true;
    }

    let separate = |out: &mut Out, sep: bool| {
        if sep {
            out.str(o.color_se);
            out.str(separator);
            out.str(o.color_off);
        }
    };

    if o.line_number {
        separate(&mut out, sep);
        out.str(o.color_ln);
        out.num(lineno, if o.initial_tab { 6 } else { 1 });
        out.str(o.color_off);
        sep = true;
    }

    if o.column_number {
        separate(&mut out, sep);
        out.str(o.color_cn);
        out.num(columno, if o.initial_tab { 3 } else { 1 });
        out.str(o.color_off);
        sep = true;
    }

    // --hyperlink: close link
    if hyp {
        out.close_hyperlink(o);
    }

    if o.byte_offset {
        separate(&mut out, sep);
        out.str(o.color_bn);
        out.num(args.byte_offset, if o.initial_tab { 7 } else { 1 });
        out.str(o.color_off);
        sep = true;
    }

    if sep {
        separate(&mut out, sep);
        if o.initial_tab {
            out.chr(b'\t');
        }
        if args.newline {
            out.nl();
        }
    }

    OutputHeaderResult { bytes: out.0, heading, complete_dump }
}

/// `Output::header(pathname, partname)` (--files-with-matches, --count) without `--tree`.
pub fn header_file(
    o: &Opts,
    pathname: &[u8],
    partname: &[u8],
    ctx: &OutputCtx,
) -> OutputHeaderResult {
    let hyp = !ctx.stdin_label && o.color_hl.is_some();
    // -Q: mark pathname with three \0 markers for quick navigation
    let nul = o.query;
    let mut out = Out::default();

    if nul {
        out.chr(0);
    }
    out.str(o.color_fn);
    if hyp {
        out.open_hyperlink(o, pathname, None);
    }
    if nul {
        out.chr(0);
    }
    out.str(pathname);
    if nul {
        out.chr(0);
    }
    if hyp {
        out.close_hyperlink(o);
    }
    if !partname.is_empty() {
        out.chr(b'{');
        out.str(partname);
        out.chr(b'}');
    }
    out.str(o.color_off);
    OutputHeaderResult { bytes: out.0, heading: false, complete_dump: false }
}

/// The directory tree state of `--tree`, kept in C++ (`Output::Tree::path` and `depth`) and passed in and out under the
/// Sync lock (`acquire()`).
pub struct TreeState {
    pub path: Vec<u8>,
    pub depth: i32,
}

/// The drawing strings of `Tree::bar`, `Tree::ptr` and `Tree::end`, set by the terminal setup (ASCII or box drawing).
#[derive(Clone, Copy)]
pub struct TreeGlyphs<'a> {
    pub bar: &'a [u8],
    pub ptr: &'a [u8],
    pub end: &'a [u8],
}

/// The path separator of the platform (`PATHSEPCHR`).
pub(crate) const PATHSEP: u8 = if cfg!(windows) { b'\\' } else { b'/' };

/// `Output::header(pathname, partname)` with `--tree`: the tree lines and the new `Tree::path` and `Tree::depth`.
pub fn header_tree(
    o: &Opts,
    pathname: &[u8],
    partname: &[u8],
    ctx: &OutputCtx,
    glyphs: &TreeGlyphs,
    tree: &mut TreeState,
) -> Vec<u8> {
    let hyp = !ctx.stdin_label && o.color_hl.is_some();
    // -Q: mark pathname with three \0 markers for quick navigation
    let nul = o.query;
    let mut out = Out::default();
    let path = &mut tree.path;
    let depth = &mut tree.depth;
    let bars = |out: &mut Out, depth: i32| {
        for _ in 1..depth {
            out.str(glyphs.bar);
        }
        if depth > 0 {
            out.str(glyphs.ptr);
        }
    };

    let mut up = 0;
    while !path.is_empty() && !pathname.starts_with(path) {
        path.pop();
        match path.iter().rposition(|&b| b == PATHSEP) {
            Some(len) => path.truncate(len + 1),
            None => path.clear(),
        }
        up += 1;
        *depth = depth.wrapping_sub(1);
    }

    if up > 0 {
        for _ in 0..*depth {
            out.str(glyphs.bar);
        }
        for _ in 1..up {
            out.str(glyphs.end);
        }
        out.nl();
        // make sure to add a break between trees with terminated leafs
        if up > 1 && !glyphs.end.is_empty() && *depth == 0 {
            out.nl();
        }
    } else if path.is_empty() && pathname.contains(&PATHSEP) {
        // add a break between the list of filenames without path and filenames with paths
        out.nl();
    }

    while let Some(sep) = pathname[path.len()..].iter().position(|&b| b == PATHSEP) {
        if nul {
            out.chr(0);
        }
        bars(&mut out, *depth);
        if nul {
            out.chr(0);
        }
        out.str(&pathname[path.len()..path.len() + sep + 1]);
        if nul {
            out.chr(0);
        }
        out.nl();
        let len = path.len() + sep + 1;
        path.clear();
        path.extend_from_slice(&pathname[..len]);
        *depth = depth.wrapping_add(1);
    }

    if nul {
        out.chr(0);
    }
    bars(&mut out, *depth);
    out.str(o.color_fn);
    if hyp {
        out.open_hyperlink(o, pathname, None);
    }
    if nul {
        out.chr(0);
    }
    out.str(&pathname[path.len()..]);
    if nul {
        out.chr(0);
    }
    if !partname.is_empty() {
        out.chr(b'{');
        out.str(partname);
        out.chr(b'}');
    }
    out.str(o.color_off);
    out.0
}

/// `Output::binary_file_matches(pathname, partname)`.
pub fn binary_file_matches(
    o: &Opts,
    pathname: &[u8],
    partname: &[u8],
    ctx: &OutputCtx,
) -> OutputBinaryResult {
    if ctx.binary {
        return OutputBinaryResult { bytes: Vec::new(), set_binary: false };
    }
    let hyp = !ctx.stdin_label && o.color_hl.is_some();
    let mut out = Out::default();
    out.str(o.color_off);
    out.str(b"Binary file ");
    out.str(o.color_fn);
    if hyp {
        out.open_hyperlink(o, pathname, None);
    }
    out.str(pathname);
    if hyp {
        out.close_hyperlink(o);
    }
    if !partname.is_empty() {
        out.chr(b'{');
        out.str(partname);
        out.chr(b'}');
    }
    out.str(o.color_off);
    out.str(b" matches");
    out.nl();
    OutputBinaryResult { bytes: out.0, set_binary: true }
}

/// The state of `flush_truncated_lines` kept in `Output` (`skip_`, `cols_`, `ansi_`): `ansi` is 0 none, 1 ESC, 2 CSI,
/// 3 OSC, 4 OSC ESC.
pub struct TruncState {
    pub skip: bool,
    pub cols: usize,
    pub ansi: u8,
}

const ANSI_NA: u8 = 0;
const ANSI_ESC: u8 = 1;
const ANSI_CSI: u8 = 2;
const ANSI_OSC: u8 = 3;
const ANSI_OSC_ESC: u8 = 4;

/// `Output::flush_truncated_lines(data, size)`: the bytes to write for a block of data cut to `--width` columns,
/// taking into account tabs, UTF-8 and ANSI sequences. The caller writes them with one `fwrite`.
pub fn truncate_lines(o: &Opts, data: &[u8], st: &mut TruncState) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let end = data.len();
    let mut d = 0;
    let newline_from = |from: usize| data[from..].iter().position(|&b| b == b'\n');

    if st.skip {
        match newline_from(d) {
            None => return out,
            Some(p) => {
                d += p + 1;
                st.skip = false;
            }
        }
    }

    while d < end {
        // search for \n or cols + width Unicode characters, whichever comes first
        let mut esc = end;
        let mut scan = d;

        while scan < end && st.cols <= o.width && data[scan] != b'\n' {
            let c = data[scan];
            if st.ansi != ANSI_NA {
                // skip ANSI escape sequence state machine
                st.ansi = match (st.ansi, c) {
                    (ANSI_ESC, b'[') => ANSI_CSI,
                    (ANSI_ESC, b']') => ANSI_OSC,
                    (ANSI_CSI, 0x40..=0x7e) => ANSI_NA,
                    (ANSI_CSI, _) => ANSI_CSI,
                    (ANSI_OSC, 0x07) => ANSI_NA,
                    (ANSI_OSC, 0x1b) => ANSI_OSC_ESC,
                    (ANSI_OSC, _) => ANSI_OSC,
                    (ANSI_OSC_ESC, b'\\') => ANSI_NA,
                    (ANSI_OSC_ESC, _) => ANSI_OSC,
                    _ => ANSI_NA,
                };
            } else if c == b'\t' {
                st.cols = st.cols.wrapping_add(1 + (!st.cols & 7));
            } else if c == 0x1b {
                esc = scan;
                st.ansi = ANSI_ESC;
            } else if c < 0x80 {
                st.cols = st.cols.wrapping_add(usize::from(c >= b' '));
            } else {
                st.cols = st.cols.wrapping_add(usize::from((c & 0xc0) != 0x80));
            }
            scan += 1;
        }

        if scan < end && data[scan] == b'\n' {
            // write data with newline
            out.extend_from_slice(&data[d..=scan]);
            d = scan + 1;
            st.cols = 0;
        } else if st.cols <= o.width {
            // write data
            out.extend_from_slice(&data[d..scan]);
            d = scan;
        } else {
            // counted one column over, back up to ensure UTF-8 multibyte sequence is complete
            let num = scan.saturating_sub(1) - d;

            if st.ansi != ANSI_NA {
                // write data up to but not including ANSI ESC
                out.extend_from_slice(&data[d..esc]);
            } else {
                out.extend_from_slice(&data[d..d + num]);
            }
            d += num;

            // disable CSI when line was truncated
            if o.color {
                out.extend_from_slice(b"\x1b[m");
            }
            out.extend_from_slice(NL);
            st.cols = 0;

            // look for newline to reach next line
            match newline_from(d) {
                None => {
                    st.skip = true;
                    break;
                }
                Some(p) => d += p + 1,
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> Opts<'static> {
        Opts {
            color_fn: b"<fn>",
            color_ln: b"<ln>",
            color_off: b"<off>",
            color_mt: b"<mt>",
            color_sl: b"<sl>",
            width: 5,
            hex_columns: 16,
            ..Opts::default()
        }
    }

    fn ctx() -> OutputCtx {
        OutputCtx { lineno: 0, binary: false, dump_incomplete: false, stdin_label: false }
    }

    #[test]
    fn color_selects_by_code_or_off() {
        let o = opts();
        assert_eq!(color(&o, true, b"fn"), b"<fn>");
        assert_eq!(color(&o, true, b"mt"), b"<mt>");
        assert_eq!(color(&o, false, b""), b"<off>");
    }

    #[test]
    fn header_file_marks_pathname_and_part() {
        let mut o = opts();
        let r = header_file(&o, b"a.txt", b"x", &ctx());
        assert_eq!(r.bytes, b"<fn>a.txt{x}<off>");
        o.query = true;
        let r = header_file(&o, b"a.txt", b"", &ctx());
        assert_eq!(r.bytes, b"\0<fn>\0a.txt\0<off>");
    }

    #[test]
    fn binary_matches_once() {
        let o = opts();
        let r = binary_file_matches(&o, b"f", b"", &ctx());
        assert!(r.set_binary);
        assert_eq!(r.bytes, [b"<off>Binary file <fn>f<off> matches".as_slice(), NL].concat());
        let again = OutputCtx { binary: true, ..ctx() };
        assert!(binary_file_matches(&o, b"f", b"", &again).bytes.is_empty());
    }

    #[test]
    fn tree_lines_and_state() {
        let o = opts();
        let g = TreeGlyphs { bar: b"| ", ptr: b"|_", end: b"~ " };
        let mut t = TreeState { path: Vec::new(), depth: 0 };
        let sep = char::from(PATHSEP);
        let name = format!("d{sep}f");
        let out = header_tree(&o, name.as_bytes(), b"", &ctx(), &g, &mut t);
        assert_eq!(t.path, format!("d{sep}").into_bytes());
        assert_eq!(t.depth, 1);
        assert_eq!(out, [NL, b"d".as_slice(), &[PATHSEP], NL, b"|_<fn>f<off>"].concat());
    }

    #[test]
    fn truncate_cuts_columns_and_skips_rest_of_line() {
        let o = opts();
        let mut st = TruncState { skip: false, cols: 0, ansi: 0 };
        let out = truncate_lines(&o, b"abcdefghij\nxy\n", &mut st);
        assert_eq!(out, [b"abcde".as_slice(), NL, b"xy\n"].concat());
        assert!(!st.skip);
        let mut st = TruncState { skip: false, cols: 0, ansi: 0 };
        assert_eq!(truncate_lines(&o, b"abcdefghij", &mut st), [b"abcde".as_slice(), NL].concat());
        assert!(st.skip);
        assert!(truncate_lines(&o, b"more", &mut st).is_empty());
        assert_eq!(truncate_lines(&o, b"rest\nok\n", &mut st), b"ok\n");
    }

    #[test]
    fn dump_line_pads_and_renders() {
        let o = opts();
        let mut bytes = [-1i16; 64];
        bytes[0] = 0x41;
        let r = dump_line(&o, &bytes, &[-1i16; 64], false, 17);
        assert!(r.bytes.starts_with(b"00000010"));
        assert!(!r.repeated);
    }
}