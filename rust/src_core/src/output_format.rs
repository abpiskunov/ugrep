//! `Output::format`, `Output::format_invert` and the `--format` interpreter of src/output.cpp.
//!
//! Pure functions of the format string, the option state (`Opts`) and a plain-data snapshot of the matcher (see
//! `OutputMatch` in lib.rs and rust/src_core/SEAM.md, section Seam: output): they return the bytes that the C++
//! facade appends to its buffer. Offsets of the snapshot refer to the `window` bytes passed next to it.

use crate::ffi::{OutputArgs, OutputCapture, OutputCtx, OutputFormatResult, OutputMatch};
use crate::output::{Opts, Out, PATHSEP};

/// `strtol(s, &end, base)` for the platform `long` (32 bits on Windows): the value and the bytes consumed (0 for no number).
fn strtol(s: &[u8], base: u32) -> (i64, usize) {
    let mut i = 0;
    while matches!(s.get(i), Some(b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')) {
        i += 1;
    }
    let mut neg = false;
    match s.get(i) {
        Some(b'-') => {
            neg = true;
            i += 1;
        }
        Some(b'+') => i += 1,
        _ => {}
    }
    if base == 16
        && s.get(i) == Some(&b'0')
        && matches!(s.get(i + 1), Some(b'x' | b'X'))
        && s.get(i + 2).is_some_and(u8::is_ascii_hexdigit)
    {
        i += 2;
    }
    let start = i;
    let max: i128 = if cfg!(windows) { i128::from(i32::MAX) } else { i128::from(i64::MAX) };
    let mut v: i128 = 0;
    while let Some(d) = s.get(i).and_then(|&c| char::from(c).to_digit(base)) {
        v = (v * i128::from(base) + i128::from(d)).min(max + 1);
        i += 1;
    }
    if i == start {
        return (0, 0);
    }
    let v = if neg { -v } else { v.min(max) };
    // the clamp of the C library: LONG_MIN for a large negative number
    let v = v.max(-max - 1);
    (i64::try_from(v).unwrap_or(0), i)
}

/// `strtoul(s, NULL, 10)` for a string that starts with a digit: saturates at the platform `unsigned long`.
fn strtoul(s: &[u8]) -> usize {
    let max: u128 = if cfg!(windows) { u128::from(u32::MAX) } else { u128::from(u64::MAX) };
    let mut v: u128 = 0;
    for &c in s.iter().take_while(|c| c.is_ascii_digit()) {
        v = (v * 10 + u128::from(c - b'0')).min(max);
    }
    usize::try_from(v).unwrap_or(usize::MAX)
}

/// `reflex::utf8(c, buf)`: UTF-8 of a code point; out of range gives the 5 byte non-character `REFLEX_NONCHAR_UTF8`.
fn utf8_of(c: i32) -> Vec<u8> {
    // truncation of `static_cast<char>(c)` is intended: C++ stores the low byte for c < 0x80, negative numbers included
    if c < 0x80 {
        return vec![c as u8];
    }
    if c > 0x10_ffff {
        return vec![0xf8, 0x88, 0x80, 0x80, 0x80];
    }
    let b = |v: i32| (v & 0xff) as u8;
    if c < 0x800 {
        vec![b(0xc0 | ((c >> 6) & 0x1f)), b(0x80 | (c & 0x3f))]
    } else if c < 0x1_0000 {
        vec![b(0xe0 | ((c >> 12) & 0x0f)), b(0x80 | ((c >> 6) & 0x3f)), b(0x80 | (c & 0x3f))]
    } else {
        vec![
            b(0xf0 | ((c >> 18) & 0x07)),
            b(0x80 | ((c >> 12) & 0x3f)),
            b(0x80 | ((c >> 6) & 0x3f)),
            b(0x80 | (c & 0x3f)),
        ]
    }
}

/// `reflex::isutf8(s, e)`: valid UTF-8 without NUL; passes surrogates and 3 and 4 byte overlongs.
fn isutf8(s: &[u8]) -> bool {
    let mut i = 0;
    let n = s.len();
    while i < n {
        let mut c = 0i8;
        while i < n {
            c = s[i] as i8;
            if c <= 0 {
                break;
            }
            i += 1;
        }
        if i >= n {
            break;
        }
        i += 1;
        let cont = |i: &mut usize| -> bool {
            let ok = *i < n && s[*i] & 0xc0 == 0x80;
            *i += 1;
            ok
        };
        if !(-62..=-12).contains(&c) || !cont(&mut i) {
            return false;
        }
        if c >= -32 && !cont(&mut i) {
            return false;
        }
        if c >= -16 && !cont(&mut i) {
            return false;
        }
    }
    true
}

/// `Output::utf8pos(s, n, k)`: the index of position `k` (not beyond the end) in a UTF-8 string.
fn utf8pos(s: &[u8], k: usize) -> usize {
    let mut n = s.len();
    let mut k = k;
    let mut i = 0;
    while n > 0 && k > 0 {
        n -= 1;
        k -= 1;
        i += 1;
        while n > 0 && s.get(i).is_some_and(|&c| c & 0xc0 == 0x80) {
            n -= 1;
            i += 1;
        }
    }
    i
}

/// `Output::utf8back(b, s, k)`: the index `k` characters before index `at` (not before 0).
fn utf8back(s: &[u8], at: usize, k: usize) -> usize {
    let mut i = at.min(s.len());
    let mut k = k;
    while i > 0 && k > 0 {
        k -= 1;
        while i > 0 {
            i -= 1;
            if s[i] & 0xc0 != 0x80 {
                break;
            }
        }
    }
    i
}

/// The unsigned count of `-width` as C++ computes it for a negative `int` width.
fn neg(width: i32) -> usize {
    usize::try_from(-i64::from(width)).unwrap_or(0)
}

/// `w[a..b]`, empty when the range is not inside `w` (the snapshot always fits; this keeps a bad snapshot from panicking).
fn sl(w: &[u8], a: usize, b: usize) -> &[u8] {
    w.get(a..b).unwrap_or_default()
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Quote,
    Cpp,
    Csv,
    Json,
    Xml,
    Hex,
}

/// One parsed `%` specifier.
struct Spec {
    width: i32,
    plus: bool,
    /// Index of the first byte after `[`, if the specifier has `[ARG]`.
    arg: Option<usize>,
    c: u8,
    /// Index of `c` in the format.
    at: usize,
}

/// A group capture: offset in the window and size.
struct Cap {
    offset: usize,
    size: usize,
}

struct Fmt<'a> {
    o: &'a Opts<'a>,
    fmt: &'a [u8],
    pathname: &'a [u8],
    partname: &'a [u8],
    out: Out,
    heading: bool,
    next: bool,
    /// `$` separator: start and length in the format.
    sep: Option<(usize, usize)>,
}

impl<'a> Fmt<'a> {
    fn new(o: &'a Opts<'a>, fmt: &'a [u8], pathname: &'a [u8], partname: &'a [u8]) -> Self {
        Fmt { o, fmt, pathname, partname, out: Out::default(), heading: false, next: false, sep: None }
    }

    /// The byte of the format at `i`; the format is a C string, so a NUL follows the end.
    fn at(&self, i: usize) -> u8 {
        self.fmt.get(i).copied().unwrap_or(0)
    }

    /// Copies the literal text up to the next `%` and parses the specifier; `None` at the end of the format.
    fn parse(&mut self, s: &mut usize) -> Option<Spec> {
        let t = *s;
        while self.at(*s) != 0 && self.at(*s) != b'%' {
            *s += 1;
        }
        let literal = sl(self.fmt, t, *s);
        self.out.str(literal);
        if self.at(*s) == 0 || self.at(*s + 1) == 0 {
            return None;
        }
        *s += 1;
        let mut width = 0;
        let mut plus = false;
        let mut arg = None;
        if self.at(*s) == b'{' {
            plus = self.at(*s + 1) == b'+';
            let (v, used) = strtol(self.fmt.get(*s + 1..).unwrap_or_default(), 10);
            // `int width = strtol(...)` truncates `long`
            width = v as i32;
            let r = *s + 1 + used;
            if self.at(r) == b'}' {
                *s = r + 1;
            }
        }
        if self.at(*s) == b'[' {
            *s += 1;
            arg = Some(*s);
            while self.at(*s) != 0 && self.at(*s) != b']' {
                *s += 1;
            }
            if self.at(*s) == 0 || self.at(*s + 1) == 0 {
                return None;
            }
            *s += 1;
        }
        Some(Spec { width, plus, arg, c: self.at(*s), at: *s })
    }

    /// `str(arg, s - arg - 1)`: the text of `[ARG]`.
    fn arg_text(&self, sp: &Spec) -> &'a [u8] {
        match sp.arg {
            Some(a) => sl(self.fmt, a, sp.at.saturating_sub(1)),
            None => &[],
        }
    }

    fn put_arg(&mut self, sp: &Spec) {
        if sp.arg.is_some() {
            let t = self.arg_text(sp);
            self.out.str(t);
        }
    }

    /// The format from `[ARG]` to its end, as C++ passes `arg` on as a C string.
    fn arg_rest(&self, sp: &Spec) -> &'a [u8] {
        sp.arg.and_then(|a| self.fmt.get(a..)).unwrap_or_default()
    }

    fn put_sep(&mut self) {
        match self.sep {
            Some((a, l)) => self.out.str(sl(self.fmt, a, a + l)),
            None => self.out.str(self.o.separator),
        }
    }

    fn num(&mut self, i: usize, width: i32) {
        self.out.num(i, usize::try_from(width).unwrap_or(0));
    }

    fn nl_chr(&mut self) {
        if cfg!(windows) {
            self.out.chr(b'\r');
        }
        self.out.chr(b'\n');
    }

    /// `{partname}` appended to a pathname, as in the `%H`, `%h`, `%I`, `%i` names.
    fn braced(&self, with_path: bool) -> Vec<u8> {
        let mut name = if with_path { self.pathname.to_vec() } else { Vec::new() };
        name.push(b'{');
        name.extend_from_slice(self.partname);
        name.push(b'}');
        name
    }

    fn emit(&mut self, kind: Kind, data: &[u8]) {
        match kind {
            Kind::Quote => self.out.quote(data),
            Kind::Cpp => self.out.cpp(data),
            Kind::Csv => self.out.csv(data),
            Kind::Json => self.out.json(data),
            Kind::Xml => self.out.xml(data),
            Kind::Hex => self.out.hex_bytes(data),
        }
    }

    /// The specifiers that match and invert formats share.
    /// Returns false for a character that is not one of them.
    fn common(&mut self, sp: &Spec) -> bool {
        let o = self.o;
        let has_part = !self.partname.is_empty();
        match sp.c {
            b'+' => {
                if o.heading && o.with_filename {
                    if self.heading {
                        self.put_arg(sp);
                        self.out.str(self.pathname);
                        if o.null {
                            self.out.chr(0);
                        }
                        self.out.nl();
                        self.heading = false;
                    } else if o.brk {
                        self.out.nl();
                    }
                }
            }
            b'F' => {
                if o.with_filename && (self.heading || has_part) {
                    self.put_arg(sp);
                    if self.heading {
                        self.out.str(self.pathname);
                    }
                    if has_part {
                        self.out.chr(b'{');
                        self.out.str(self.partname);
                        self.out.chr(b'}');
                    }
                    if o.null {
                        self.out.chr(0);
                    } else {
                        self.put_sep();
                    }
                }
            }
            b'f' => {
                self.out.str(self.pathname);
                if has_part {
                    self.out.chr(b'{');
                    self.out.str(self.partname);
                    self.out.chr(b'}');
                }
            }
            b'a' => {
                let base = self.pathname.iter().rposition(|&b| b == PATHSEP).map_or(0, |p| p + 1);
                self.out.str(&self.pathname[base..]);
            }
            b'p' => {
                if let Some(p) = self.pathname.iter().rposition(|&b| b == PATHSEP) {
                    self.out.str(&self.pathname[..p]);
                }
            }
            b'z' => self.out.str(self.partname),
            b'H' | b'I' => {
                let kind = if sp.c == b'H' { Kind::Quote } else { Kind::Xml };
                if o.with_filename && (self.heading || has_part) {
                    self.put_arg(sp);
                    if has_part {
                        let name = self.braced(self.heading);
                        self.emit(kind, &name);
                    } else {
                        self.emit(kind, self.pathname);
                    }
                    self.put_sep();
                }
            }
            b'h' | b'i' => {
                let kind = if sp.c == b'h' { Kind::Quote } else { Kind::Xml };
                if has_part {
                    let name = self.braced(true);
                    self.emit(kind, &name);
                } else {
                    self.emit(kind, self.pathname);
                }
            }
            b'T' => {
                if o.initial_tab {
                    self.put_arg(sp);
                    self.out.chr(b'\t');
                }
            }
            b't' => self.out.chr(b'\t'),
            b'S' => {
                if self.next {
                    self.put_arg(sp);
                    self.put_sep();
                }
            }
            b's' => self.put_sep(),
            b'$' => {
                self.sep = sp.arg.map(|a| (a, sp.at.saturating_sub(a + 1)));
            }
            b'R' | b'~' => {
                if sp.c == b'~' || o.brk {
                    self.nl_chr();
                }
            }
            b'U' => {
                if sp.arg.is_some() {
                    let (v, _) = strtol(self.arg_rest(sp), 16);
                    // `wchr(int)`: strtol is truncated to int
                    self.out.str(&utf8_of(v as i32));
                }
            }
            b'=' => {
                let bytes = if sp.arg.is_some() {
                    crate::output::color(o, true, self.arg_rest(sp))
                } else {
                    crate::output::color(o, false, &[])
                };
                self.out.str(&bytes);
            }
            b'<' => {
                if !self.next {
                    self.put_arg(sp);
                }
            }
            b'>' => {
                if self.next {
                    self.put_arg(sp);
                }
            }
            b',' | b':' | b';' | b'|' => {
                if self.next {
                    self.out.chr(sp.c);
                }
            }
            _ => return false,
        }
        true
    }
}

/// `Output::format(format, matches)` (--format-begin, --format-end).
pub fn format_count(o: &Opts, format: &[u8], matches: usize, _ctx: &OutputCtx) -> Vec<u8> {
    let mut f = Fmt::new(o, format, &[], &[]);
    let mut s = 0;
    while f.at(s) != 0 {
        let Some(sp) = f.parse(&mut s) else { break };
        match sp.c {
            b'T' => {
                if o.initial_tab {
                    f.put_arg(&sp);
                    f.out.chr(b'\t');
                }
            }
            b'S' => {
                if matches > 1 {
                    f.put_arg(&sp);
                    f.put_sep();
                }
            }
            b'$' => f.sep = sp.arg.map(|a| (a, sp.at.saturating_sub(a + 1))),
            b't' => f.out.chr(b'\t'),
            b's' => f.put_sep(),
            b'R' | b'~' => {
                if sp.c == b'~' || o.brk {
                    f.nl_chr();
                }
            }
            b'm' => f.num(matches, sp.width),
            b'U' => {
                if sp.arg.is_some() {
                    let (v, _) = strtol(f.arg_rest(&sp), 16);
                    f.out.str(&utf8_of(v as i32));
                }
            }
            b'=' => {
                let bytes = crate::output::color(o, sp.arg.is_some(), f.arg_rest(&sp));
                f.out.str(&bytes);
            }
            b'<' => {
                if matches <= 1 {
                    f.put_arg(&sp);
                }
            }
            b'>' => {
                if matches > 1 {
                    f.put_arg(&sp);
                }
            }
            b',' | b':' | b';' | b'|' => {
                if matches > 1 {
                    f.out.chr(sp.c);
                }
            }
            c => f.out.chr(c),
        }
        s += 1;
    }
    f.out.0
}

/// The snapshot of the matcher with the window that its offsets refer to.
struct Mt<'a> {
    m: &'a OutputMatch,
    w: &'a [u8],
}

impl Mt<'_> {
    /// `matcher->begin()..matcher->begin() + size()`.
    fn matched(&self) -> &[u8] {
        sl(self.w, self.m.begin_off, self.m.begin_off.wrapping_add(self.m.size))
    }

    /// The group capture `index` (0 is the match), or None for `(NULL, 0)`.
    fn group(&self, index: usize) -> Option<Cap> {
        if index == 0 {
            return Some(Cap { offset: self.m.begin_off, size: self.m.size });
        }
        self.m.groups.iter().find(|g| g.index == index && g.found).map(|g| Cap { offset: g.offset, size: g.size })
    }

    fn cap_bytes(&self, c: &Cap) -> &[u8] {
        sl(self.w, c.offset, c.offset.wrapping_add(c.size))
    }

    /// `cap.first - matcher->text()`: the offset of a capture from the start of the match.
    fn cap_from_text(&self, c: &Cap) -> usize {
        c.offset.wrapping_sub(self.m.text_off)
    }

    /// `Output::capture(matcher, arg)`: `arg` is the format from `[ARG]` on.
    fn capture(&self, arg: &[u8]) -> Option<Cap> {
        let mut arg = arg;
        loop {
            let end = arg.iter().position(|&c| c == b']')?;
            let bar = match arg.iter().position(|&c| c == b'|') {
                Some(b) if b <= end => b,
                _ => end,
            };
            if arg.first().is_some_and(u8::is_ascii_digit) {
                let index = strtoul(arg);
                if index == 0 || arg[bar] == b']' {
                    return self.group(index);
                }
                if self.m.groups.iter().any(|g| g.index == index && g.listed) {
                    return self.group(index);
                }
            } else {
                let name = &arg[..bar];
                if let Some(g) = self.m.groups.iter().find(|g: &&OutputCapture| g.has_name && g.name == name) {
                    return self.group(g.index);
                }
            }
            if arg[bar] == b']' {
                return None;
            }
            arg = &arg[bar + 1..];
        }
    }

    /// `Output::match_context(matcher, plus, width, n)`.
    fn context(&self, plus: bool, width: i32) -> &[u8] {
        let m = self.m;
        if width == 0 {
            self.matched()
        } else if width > 0 {
            let k = usize::try_from(width).unwrap_or(0);
            if plus {
                let rest = sl(self.w, m.end_off, m.eol_off);
                sl(rest, 0, utf8pos(rest, k))
            } else {
                let s = self.matched();
                sl(s, 0, utf8pos(s, k))
            }
        } else {
            let line = self.line();
            let at = m.begin_off.saturating_sub(m.bol_off);
            let b = utf8back(line, at, neg(width));
            sl(line, b, at)
        }
    }

    /// The line of the match, `bol()` to `eol()`: the window may be wider because of captures outside the line.
    fn line(&self) -> &[u8] {
        sl(self.w, self.m.bol_off, self.m.eol_off)
    }

    /// The line of the match (`bol()` to `eol()`) cut as the inline `quote`, `cpp`, ... overloads do for width `w`.
    fn line_cut(&self, w: i32) -> &[u8] {
        let line = self.line();
        if w > 0 {
            sl(line, 0, utf8pos(line, usize::try_from(w).unwrap_or(0)))
        } else if w < 0 {
            sl(line, utf8back(line, line.len(), neg(w)), line.len())
        } else {
            line
        }
    }
}

fn bool_text(o: &Opts) -> &'static [u8] {
    if o.invert_match {
        b"false"
    } else {
        b"true"
    }
}

/// `Output::format(format, pathname, partname, matches, matching, matcher, heading, body, next)`.
pub fn format_match(
    o: &Opts,
    format: &[u8],
    names: (&[u8], &[u8]),
    window: &[u8],
    args: &OutputArgs,
    matcher: &OutputMatch,
    ctx: &OutputCtx,
) -> OutputFormatResult {
    let (pathname, partname) = names;
    let mut res = OutputFormatResult {
        bytes: Vec::new(),
        ret: true,
        heading: args.heading,
        matching: args.matching,
        lineno_set: false,
        lineno: 0,
    };
    let mt = Mt { m: matcher, w: window };
    if !args.body {
        res.lineno_set = true;
        res.lineno = 0;
    } else if ctx.lineno > 0 && ctx.lineno == matcher.lineno && matcher.lines == 1 {
        res.ret = false;
        return res;
    }
    if args.has_matching {
        res.matching = res.matching.wrapping_add(1);
    }
    let matches = args.matches;
    let matching = if args.has_matching { res.matching } else { matches };

    let mut f = Fmt::new(o, format, pathname, partname);
    f.heading = args.heading;
    f.next = args.next;
    let mut s = 0;
    while f.at(s) != 0 {
        let Some(sp) = f.parse(&mut s) else { break };
        if !f.common(&sp) {
            match_case(&mut f, &mt, &sp, matches, matching, &mut res);
        }
        s += 1;
    }
    res.heading = f.heading;
    res.bytes = f.out.0;
    res
}

fn match_case(f: &mut Fmt, mt: &Mt, sp: &Spec, matches: usize, matching: usize, res: &mut OutputFormatResult) {
    let o = f.o;
    let m = mt.m;
    let width = sp.width;
    let has_arg = sp.arg.is_some();
    match sp.c {
        b'N' => {
            if o.line_number {
                f.put_arg(sp);
                f.num(m.lineno, if !has_arg && o.initial_tab { 6 } else { width });
                f.put_sep();
            }
        }
        b'n' => f.num(m.lineno, width),
        b'L' => f.num(m.lines, width),
        b'l' => f.num(m.lineno_end, width),
        b'K' => {
            if o.column_number {
                f.put_arg(sp);
                f.num(m.columno.wrapping_add(1), if !has_arg && o.initial_tab { 3 } else { width });
                f.put_sep();
            }
        }
        b'k' => f.num(m.columno.wrapping_add(1), width),
        b'A' => {
            f.out.hex(m.first, 8);
            f.out.chr(b'-');
            f.out.hex(m.last.wrapping_sub(1), 8);
        }
        b'B' => {
            if o.byte_offset {
                f.put_arg(sp);
                f.num(m.first, width);
                f.put_sep();
            }
        }
        b'b' => {
            if has_arg {
                if let Some(cap) = mt.capture(f.arg_rest(sp)) {
                    f.num(mt.cap_from_text(&cap).wrapping_add(m.first), width);
                }
            } else {
                f.num(m.first, width);
            }
        }
        b'w' => f.num(m.wsize, width),
        b'd' => {
            if has_arg {
                if let Some(cap) = mt.capture(f.arg_rest(sp)) {
                    f.num(cap.size, width);
                }
            } else {
                f.num(m.size, width);
            }
        }
        b'e' => {
            if has_arg {
                if let Some(cap) = mt.capture(f.arg_rest(sp)) {
                    f.num(mt.cap_from_text(&cap).wrapping_add(cap.size).wrapping_add(m.first), width);
                }
            } else {
                f.num(m.last, width);
            }
        }
        b'G' => groups_all(f, mt, sp),
        b'g' => group_one(f, mt, sp),
        b'M' => f.num(matches, width),
        b'm' => f.num(matching, width),
        b'O' => mat(f, mt, width),
        b'o' | b'#' => match_text(f, mt, sp),
        b'Q' => {
            if o.only_matching {
                let s = if width > 0 { mt.context(false, width) } else { mt.matched() };
                f.out.quote(s);
            } else {
                f.out.quote(mt.line_cut(width));
            }
        }
        b'q' => lower(f, mt, sp, Kind::Quote, matches),
        b'C' => upper(f, mt, sp, Kind::Cpp, matches),
        b'c' => lower(f, mt, sp, Kind::Cpp, matches),
        b'V' => upper(f, mt, sp, Kind::Csv, matches),
        b'v' => lower(f, mt, sp, Kind::Csv, matches),
        b'J' => upper(f, mt, sp, Kind::Json, matches),
        b'j' => lower(f, mt, sp, Kind::Json, matches),
        b'X' => upper(f, mt, sp, Kind::Xml, matches),
        b'x' => lower(f, mt, sp, Kind::Xml, matches),
        b'Y' => upper(f, mt, sp, Kind::Hex, matches),
        b'y' => lower(f, mt, sp, Kind::Hex, matches),
        b'Z' => {
            if o.fuzzy > 0 {
                if !o.match_all {
                    // C++ dereferences the dynamic_cast to the fuzzy matcher; without one there is nothing to print
                    if m.has_fuzzy {
                        if !o.files_with_matches && !o.count {
                            f.num(m.edits, width);
                        } else {
                            f.num(m.distance & 0xff, width);
                        }
                    }
                } else {
                    f.out.chr(b'0');
                }
            }
        }
        b'u' => {
            if !o.ungroup {
                res.lineno_set = true;
                res.lineno = m.lineno;
            }
        }
        b'0'..=b'9' => {
            if let Some(cap) = mt.group(usize::from(sp.c - b'0')) {
                f.out.str(mt.cap_bytes(&cap));
            }
        }
        c => f.out.chr(c),
    }
}

/// `%G`: the names or numbers of all groups that matched, separated by the separator.
fn groups_all(f: &mut Fmt, mt: &Mt, sp: &Spec) {
    let mut colon = false;
    // C++ advances `arg` while looking up one group and keeps it for the next group
    let mut arg = sp.arg.map(|_| f.arg_rest(sp));
    for g in mt.m.groups.iter().filter(|g| g.listed) {
        if colon {
            f.put_sep();
        }
        colon = true;
        group_label(f, g, &mut arg);
    }
}

/// The label of one group for `%G` and `%g`: the n-th alternative of `[ARG]`, else the name, else the number.
fn group_label(f: &mut Fmt, g: &OutputCapture, arg_state: &mut Option<&[u8]>) {
    if let Some(mut arg) = *arg_state {
        let mut n = g.index;
        let bar;
        loop {
            let b = arg.iter().position(|&c| c == b'|');
            let e = arg.iter().position(|&c| c == b']');
            n = n.wrapping_sub(1);
            match b {
                Some(bp) if e.is_none_or(|ep| bp <= ep) => {
                    if n == 0 {
                        bar = Some(bp);
                        break;
                    }
                    arg = &arg[bp + 1..];
                }
                _ => {
                    bar = e;
                    break;
                }
            }
        }
        match bar {
            Some(bp) if n == 0 => f.out.str(&arg[..bp]),
            _ => name_or_number(f, g),
        }
        *arg_state = Some(arg);
    } else {
        name_or_number(f, g);
    }
}

fn name_or_number(f: &mut Fmt, g: &OutputCapture) {
    if g.has_name {
        f.out.str(&g.name);
    } else {
        f.out.num(g.index, 0);
    }
}

/// `%g`: the first group that matched with `[ARG]`, else the accepted group number.
fn group_one(f: &mut Fmt, mt: &Mt, sp: &Spec) {
    if sp.arg.is_some() {
        if let Some(g) = mt.m.groups.iter().find(|g| g.listed) {
            let mut arg = Some(f.arg_rest(sp));
            group_label(f, g, &mut arg);
        }
    } else {
        f.out.num(mt.m.accept, 0);
    }
}

/// `mat(matcher, w)`: a matching line or, with -o, the match; `%O`.
fn mat(f: &mut Fmt, mt: &Mt, w: i32) {
    let o = f.o;
    if o.only_matching {
        let s = mt.matched();
        if o.hex_all || (o.with_hex && !isutf8(s)) {
            let n = match usize::try_from(w) {
                Ok(k) if k > 0 && k < s.len() => k,
                _ => s.len(),
            };
            f.out.hex_bytes(&s[..n]);
        } else if w > 0 {
            f.out.str(mt.context(false, w));
        } else {
            f.out.str(s);
        }
    } else {
        let line = mt.line();
        let (mut b, mut e) = (0usize, line.len());
        if o.hex_all || (o.with_hex && !isutf8(line)) {
            if w > 0 && usize::try_from(w).is_ok_and(|k| b + k < e) {
                e = b + usize::try_from(w).unwrap_or(0);
            } else if w < 0 && e > b + neg(w) {
                b = e - neg(w);
            }
            f.out.hex_bytes(&line[b..e]);
        } else if w > 0 {
            let n = utf8pos(line, usize::try_from(w).unwrap_or(0));
            f.out.str(&line[..n]);
        } else {
            if w < 0 {
                b = utf8back(line, e, neg(w));
            }
            f.out.str(&line[b..e]);
        }
    }
}

/// `%o` and `%#`: the match, or the capture `[ARG]`, shortened to `{width}` characters or in context.
fn match_text(f: &mut Fmt, mt: &Mt, sp: &Spec) {
    let o = f.o;
    let width = sp.width;
    if sp.arg.is_some() {
        if let Some(cap) = mt.capture(f.arg_rest(sp)) {
            let p = mt.cap_bytes(&cap);
            if o.hex_all || (o.with_hex && !isutf8(p)) {
                let n = match usize::try_from(width) {
                    Ok(k) if k > 0 && k < p.len() => k,
                    _ => p.len(),
                };
                f.out.hex_bytes(&p[..n]);
            } else if width > 0 {
                f.out.str(&p[..utf8pos(p, usize::try_from(width).unwrap_or(0))]);
            } else {
                f.out.str(p);
            }
        }
    } else {
        let p = mt.matched();
        if o.hex_all || (o.with_hex && !isutf8(p)) {
            let n = match usize::try_from(width) {
                Ok(k) if k > 0 && k < p.len() => k,
                _ => p.len(),
            };
            f.out.hex_bytes(&p[..n]);
        } else if width != 0 {
            f.out.str(mt.context(sp.plus, width));
        } else {
            f.out.str(p);
        }
    }
}

/// The text that `-F`/`-c` options put in place of the match for the `%C %V %J %X %Y` family.
fn files_or_count(f: &mut Fmt, kind: Kind, matches: usize, upper: bool) -> bool {
    let o = f.o;
    if o.files_with_matches {
        match (kind, upper) {
            (Kind::Cpp, _) => {
                f.out.chr(b'"');
                f.out.str(bool_text(o));
                f.out.chr(b'"');
            }
            (Kind::Hex, true) => f.out.hex(usize::from(!o.invert_match), 0),
            _ => f.out.str(bool_text(o)),
        }
        true
    } else if o.count {
        match (kind, upper) {
            (Kind::Cpp, _) => {
                f.out.chr(b'"');
                f.out.num(matches, 0);
                f.out.chr(b'"');
            }
            (Kind::Hex, true) => f.out.hex(matches, 0),
            _ => f.out.num(matches, 0),
        }
        true
    } else {
        false
    }
}

/// `%C %V %J %X %Y`: the match or line in the given encoding.
fn upper(f: &mut Fmt, mt: &Mt, sp: &Spec, kind: Kind, matches: usize) {
    if files_or_count(f, kind, matches, true) {
        return;
    }
    let data = if f.o.only_matching { mt.matched() } else { mt.line_cut(sp.width) };
    f.emit(kind, data);
}

/// `%q %c %v %j %x %y`: the match, the capture `[ARG]` or the context in the given encoding.
fn lower(f: &mut Fmt, mt: &Mt, sp: &Spec, kind: Kind, matches: usize) {
    if kind != Kind::Quote && files_or_count(f, kind, matches, false) {
        return;
    }
    if sp.arg.is_some() {
        if let Some(cap) = mt.capture(f.arg_rest(sp)) {
            let p = mt.cap_bytes(&cap);
            let n = if sp.width > 0 { utf8pos(p, usize::try_from(sp.width).unwrap_or(0)) } else { p.len() };
            f.emit(kind, sl(p, 0, n));
        }
    } else {
        f.emit(kind, mt.context(sp.plus, sp.width));
    }
}

/// `Output::format_invert(format, pathname, partname, matches, lineno, offset, ptr, size, heading, next)`.
pub fn format_invert(
    o: &Opts,
    format: &[u8],
    names: (&[u8], &[u8]),
    line: &[u8],
    args: &OutputArgs,
    _ctx: &OutputCtx,
) -> OutputFormatResult {
    let (pathname, partname) = names;
    let mut f = Fmt::new(o, format, pathname, partname);
    f.heading = args.heading;
    f.next = args.next;
    let (lineno, offset, matches, size) = (args.lineno, args.offset, args.matches, line.len());
    let mut s = 0;
    while f.at(s) != 0 {
        let Some(sp) = f.parse(&mut s) else { break };
        if !f.common(&sp) {
            invert_case(&mut f, &sp, line, lineno, offset, matches, size);
        }
        s += 1;
    }
    OutputFormatResult { bytes: f.out.0, ret: true, heading: f.heading, matching: args.matching, lineno_set: false, lineno: 0 }
}

fn invert_case(f: &mut Fmt, sp: &Spec, line: &[u8], lineno: usize, offset: usize, matches: usize, size: usize) {
    let o = f.o;
    let width = sp.width;
    let has_arg = sp.arg.is_some();
    match sp.c {
        b'N' => {
            if o.line_number {
                f.put_arg(sp);
                f.num(lineno, if !has_arg && o.initial_tab { 6 } else { width });
                f.put_sep();
            }
        }
        b'n' => f.num(lineno, width),
        b'K' => {
            if o.column_number {
                f.put_arg(sp);
                f.out.chr(b'1');
                f.put_sep();
            }
        }
        b'k' => f.out.chr(b'1'),
        b'A' => {
            f.out.hex(offset, 8);
            f.out.chr(b'-');
            f.out.hex(offset.wrapping_add(size).wrapping_sub(1), 8);
        }
        b'B' => {
            if o.byte_offset {
                f.put_arg(sp);
                f.num(offset, width);
                f.put_sep();
            }
        }
        b'b' => f.num(offset, width),
        b'w' => f.num(line.iter().filter(|&&c| c & 0xc0 != 0x80).count(), width),
        b'd' => f.num(size, width),
        b'e' => f.num(offset.wrapping_add(size), width),
        b'G' | b'g' | b'Z' | b'u' | b'#' | b'0'..=b'9' => {}
        b'm' => f.num(matches, width),
        b'O' | b'o' => f.out.str(line),
        b'Q' | b'q' => f.out.quote(line),
        b'C' | b'c' => invert_kind(f, Kind::Cpp, line, matches),
        b'V' | b'v' => invert_kind(f, Kind::Csv, line, matches),
        b'J' | b'j' => invert_kind(f, Kind::Json, line, matches),
        b'X' | b'x' => invert_kind(f, Kind::Xml, line, matches),
        b'Y' | b'y' => invert_kind(f, Kind::Hex, line, matches),
        c => f.out.chr(c),
    }
}

/// The `%C %V %J %X %Y` family of `format_invert`: the same text for upper and lower case; Hex prints true/false.
fn invert_kind(f: &mut Fmt, kind: Kind, line: &[u8], matches: usize) {
    if !files_or_count(f, kind, matches, false) {
        f.emit(kind, line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> OutputCtx {
        OutputCtx { lineno: 0, binary: false, dump_incomplete: false, stdin_label: false }
    }

    fn opts() -> Opts<'static> {
        Opts { separator: b":", color_off: b"<off>", color_mt: b"<mt>", ..Opts::default() }
    }

    fn args() -> OutputArgs {
        OutputArgs {
            matches: 3,
            has_matching: false,
            matching: 0,
            heading: false,
            body: true,
            next: false,
            lineno: 7,
            offset: 100,
            byte_offset: 0,
            has_separator: false,
            newline: false,
        }
    }

    fn om() -> OutputMatch {
        OutputMatch {
            present: true,
            has_bol: false,
            has_eol: false,
            first: 0,
            last: 0,
            size: 0,
            wsize: 0,
            lineno: 0,
            lineno_end: 0,
            columno: 0,
            lines: 0,
            accept: 0,
            begin_off: 0,
            text_off: 0,
            end_off: 0,
            bol_off: 0,
            eol_off: 0,
            has_fuzzy: false,
            edits: 0,
            distance: 0,
            groups: Vec::new(),
        }
    }

    #[test]
    fn strtol_like_c() {
        assert_eq!(strtol(b"  -12}", 10), (-12, 5));
        assert_eq!(strtol(b"x", 10), (0, 0));
        assert_eq!(strtol(b"0x41]", 16), (0x41, 4));
        assert_eq!(strtol(b"41]", 16), (0x41, 2));
        assert_eq!(strtol(b"0xg", 16), (0, 1));
    }

    #[test]
    fn isutf8_matches_the_scalar_rules() {
        assert!(isutf8(b"abc \xc3\xa9 \xe2\x82\xac \xf0\x9f\x98\x80"));
        assert!(!isutf8(b"a\0b"));
        assert!(!isutf8(b"\xc3"));
        assert!(!isutf8(b"\xc0\x80"));
        assert!(!isutf8(b"\xff"));
        assert!(isutf8(b"\xed\xa0\x80"));
    }

    #[test]
    fn utf8_positions() {
        let s = "a\u{e9}b\u{20ac}c".as_bytes();
        assert_eq!(utf8pos(s, 2), 3);
        assert_eq!(utf8pos(s, 99), s.len());
        assert_eq!(utf8back(s, s.len(), 2), s.len() - 4);
    }

    #[test]
    fn count_format() {
        let o = opts();
        let out = format_count(&o, b"n=%m%S;%t%s%%", 2, &ctx());
        assert_eq!(out, b"n=2:;\t:%");
        assert_eq!(format_count(&o, b"%{5}m|%[X]>", 1, &ctx()), b"    1|");
    }

    #[test]
    fn invert_format() {
        let mut o = opts();
        o.line_number = true;
        let r = format_invert(&o, b"%N%o|%q|%j|%d|%w|%M", (b"f", b""), b"a\"b\xc3\xa9", &args(), &ctx());
        assert_eq!(r.bytes, b"7:a\"b\xc3\xa9|\"a\\\"b\xc3\xa9\"|\"a\\\"b\xc3\xa9\"|5|4|M");
    }

    #[test]
    fn match_format_fields() {
        let o = opts();
        let window = b"xx hello yy";
        let m = OutputMatch {
            first: 13,
            last: 18,
            size: 5,
            wsize: 5,
            lineno: 4,
            begin_off: 3,
            text_off: 3,
            end_off: 8,
            bol_off: 0,
            eol_off: 11,
            has_eol: true,
            has_bol: true,
            ..om()
        };
        let mut a = args();
        a.has_matching = true;
        let r = format_match(&o, b"%n:%b:%e:%d:%o:%m:%u", (b"f", b""), window, &a, &m, &ctx());
        assert_eq!(r.bytes, b"4:13:18:5:hello:1:");
        assert!(r.ret && r.matching == 1 && r.lineno_set && r.lineno == 4);
        let r = format_match(&o, b"%{2}o%{-2}o%{+2}o", (b"f", b""), window, &a, &m, &ctx());
        assert_eq!(r.bytes, b"hex  y");
    }

    #[test]
    fn match_skips_same_line() {
        let o = opts();
        let m = OutputMatch { lineno: 5, lines: 1, ..om() };
        let c = OutputCtx { lineno: 5, ..ctx() };
        let r = format_match(&o, b"x", (b"f", b""), b"", &args(), &m, &c);
        assert!(!r.ret && r.bytes.is_empty());
    }

    #[test]
    fn match_groups_and_json() {
        let mut o = opts();
        o.only_matching = true;
        let window = b"foo=bar";
        let groups = vec![OutputCapture { index: 1, has_name: true, name: b"k".to_vec(), found: true, listed: true, offset: 0, size: 3 }];
        let m = OutputMatch { size: 7, begin_off: 0, end_off: 7, eol_off: 7, groups, ..om() };
        let r = format_match(&o, b"%G|%[k]o|%[1]b|%1|%j", (b"f", b""), window, &args(), &m, &ctx());
        assert_eq!(r.bytes, b"k|foo|0|foo|\"foo=bar\"");
    }

    #[test]
    fn lookaround_capture_outside_the_line() {
        let o = opts();
        // `a(?=(b\nc))` on "xab\nc": the window is widened to hold the capture, the line is not
        let window = b"xab\nc";
        let groups = vec![OutputCapture { index: 1, has_name: false, name: Vec::new(), found: true, listed: true, offset: 2, size: 3 }];
        let m = OutputMatch {
            size: 1,
            begin_off: 1,
            text_off: 1,
            end_off: 2,
            bol_off: 0,
            eol_off: 3,
            has_bol: true,
            has_eol: true,
            groups,
            ..om()
        };
        let r = format_match(&o, b"%O|%1", (b"f", b""), window, &args(), &m, &ctx());
        assert_eq!(r.bytes, b"xab|b\nc");
        // `(?<=(x\n))y` on "x\ny": the previous line does not leak into %O or %{-N}o
        let m = OutputMatch {
            size: 1,
            begin_off: 2,
            text_off: 2,
            end_off: 3,
            bol_off: 2,
            eol_off: 3,
            has_bol: true,
            has_eol: true,
            ..om()
        };
        let r = format_match(&o, b"%O|%{-3}o", (b"f", b""), b"x\ny", &args(), &m, &ctx());
        assert_eq!(r.bytes, b"y|");
    }
}
