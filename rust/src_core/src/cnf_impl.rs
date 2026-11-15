//! Port of src/cnf.cpp (`CNF`): bodies behind the `signatures::cnf` facade functions. Patterns are bytes; the
//! C++ walks `const char *` pointers that end at NUL, so every read past the end here yields 0.

use super::{pattern_mask, Anchored, Bytes, CnfFlags, Compiled, Mode, Term, Terms};

const NL: &[u8] = if cfg!(windows) { b"\r\n" } else { b"\n" };

const FUZZY_INS: usize = 0x1000;
const FUZZY_DEL: usize = 0x2000;
const FUZZY_SUB: usize = 0x4000;

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | 9..=13)
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > hay.len() || needle.len() > hay.len() - from {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

pub fn quote(pattern: &[u8]) -> Bytes {
    let mut p = pattern.to_vec();
    quote_in_place(&mut p);
    p
}

fn quote_in_place(pattern: &mut Bytes) {
    if pattern.is_empty() {
        return;
    }
    let mut from = 0;
    while let Some(to) = find(pattern, b"\\E", from) {
        pattern.splice(to + 2..to + 2, b"\\\\E\\Q".iter().copied());
        from = to + 7;
    }
    pattern.splice(0..0, b"\\Q".iter().copied());
    pattern.extend_from_slice(b"\\E");
}

pub fn anchor(pattern: &[u8], flags: &CnfFlags) -> Anchored {
    let mut p = pattern.to_vec();
    let enable_empty = anchor_in_place(&mut p, flags);
    Anchored { pattern: p, enable_empty }
}

/// Returns true when the C++ would set `flag_empty`.
fn anchor_in_place(pattern: &mut Bytes, flags: &CnfFlags) -> bool {
    let mode = Mode(flags.mode);
    if pattern.is_empty() {
        if flags.line_regexp {
            *pattern = b"^$".to_vec();
        }
    } else if flags.line_regexp {
        let (l, r): (&[u8], &[u8]) = if mode == Mode::BRE { (b"^\\(", b"\\)$") } else { (b"^(?:", b")$") };
        pattern.splice(0..0, l.iter().copied());
        pattern.extend_from_slice(r);
    } else if flags.word_regexp {
        if mode == Mode::PERL {
            let (wl, wr): (&[u8], &[u8]) = if flags.have_pcre2 {
                (
                    if pattern[0] != b'^' { b"(?<!\\w)(?:" } else { b"(?:" },
                    if pattern[pattern.len() - 1] != b'$' { b")(?!\\w)" } else { b")" },
                )
            } else {
                (
                    if pattern[0] != b'^' { b"(?<![[:word:]])(?:" } else { b"(?:" },
                    if pattern[pattern.len() - 1] != b'$' { b")(?![[:word:]])" } else { b")" },
                )
            };
            pattern.splice(0..0, wl.iter().copied());
            pattern.extend_from_slice(wr);
        }
    } else if pattern[0] == b'^' || pattern[pattern.len() - 1] == b'$' {
        return true;
    }
    false
}

// ---- operator tree ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Op {
    None,
    And,
    Or,
    Not,
}

#[derive(Clone, Debug)]
struct OpTree {
    op: Op,
    regex: Bytes,
    list: Vec<OpTree>,
}

/// The pattern pointer of the C++ parser. `parse1` may repoint it at the literal "^$".
struct Cur {
    buf: Bytes,
    pos: usize,
    /// set when an `anchor` call of the parse would set `flag_empty`
    enable_empty: bool,
}

impl Cur {
    fn at(&self, i: usize) -> u8 {
        self.buf.get(i).copied().unwrap_or(0)
    }
    fn cur(&self) -> u8 {
        self.at(self.pos)
    }
    fn skip_space(&mut self) {
        while self.cur() != b'\n' && is_space(self.cur()) {
            self.pos += 1;
        }
    }
}

fn is_oper(buf: &Cur, op: Op, idx: &mut usize) -> bool {
    let (word, n): (&[u8], usize) = match op {
        Op::And => (b"AND", 3),
        Op::Or => (b"OR", 2),
        Op::Not => (b"NOT", 3),
        Op::None => return false,
    };
    let matches = (0..n).all(|k| buf.at(*idx + k) == word[k]) && is_space(buf.at(*idx + n));
    if matches {
        *idx += n;
    }
    matches
}

fn is_alternation(cur: &mut Cur) -> bool {
    let mut la = cur.pos;
    while cur.at(la) != b'\n' && is_space(cur.at(la)) {
        la += 1;
    }
    if cur.at(la) == b'\n' {
        la += 1;
    } else if cur.at(la) == b'|' {
        loop {
            la += 1;
            if cur.at(la) != b'|' {
                break;
            }
        }
    } else if !is_oper(cur, Op::Or, &mut la) {
        return false;
    }
    while cur.at(la) != b'\n' && is_space(cur.at(la)) {
        la += 1;
    }
    cur.pos = la;
    true
}

/// Skip `\x`-style escapes inside a `[...]` or `"..."` scan: `*la++ == '\\' && *la != 0` then skip one more.
fn skip_to(cur: &Cur, la: &mut usize, stop: u8) {
    while cur.at(*la) != 0 && cur.at(*la) != stop {
        let c = cur.at(*la);
        *la += 1;
        if c == b'\\' && cur.at(*la) != 0 {
            *la += 1;
        }
    }
}

impl OpTree {
    fn new(op: Op) -> OpTree {
        OpTree { op, regex: Vec::new(), list: Vec::new() }
    }

    fn parse(&mut self, cur: &mut Cur, f: &CnfFlags) {
        loop {
            self.parse1(cur, f);
            if cur.cur() == 0 {
                break;
            }
        }
    }

    fn parse1(&mut self, cur: &mut Cur, f: &CnfFlags) {
        cur.skip_space();
        if cur.cur() == 0 && f.line_regexp {
            cur.buf = b"^$".to_vec();
            cur.pos = 0;
        }
        while cur.cur() != 0 {
            self.list.push(OpTree::new(Op::Or));
            self.list.last_mut().unwrap().parse2(cur, f);
            cur.skip_space();
            if cur.cur() == b')' {
                break;
            }
            let mut p = cur.pos;
            if is_oper(cur, Op::And, &mut p) {
                cur.pos = p;
                cur.skip_space();
            }
        }
    }

    fn parse2(&mut self, cur: &mut Cur, f: &CnfFlags) {
        loop {
            self.list.push(OpTree::new(Op::None));
            self.list.last_mut().unwrap().parse3(cur, f);
            if !is_alternation(cur) {
                break;
            }
        }
    }

    fn parse3(&mut self, cur: &mut Cur, f: &CnfFlags) {
        let mode = Mode(f.mode);
        let mut p = cur.pos;
        if cur.cur() == b'-' || is_oper(cur, Op::Not, &mut p) {
            if cur.cur() != b'-' {
                cur.pos = p;
            }
            self.op = Op::Not;
            cur.pos += 1;
            cur.skip_space();
        }

        let mut parens = false;

        if cur.cur() == b'(' && (mode == Mode::FIXED || cur.at(cur.pos + 1) != b'?') {
            let mut level = 0i32;
            let mut la = cur.pos;
            while cur.at(la) != 0 {
                la += 1;
                let c = cur.at(la);
                if c == b'(' {
                    level += 1;
                } else if c == b')' {
                    if level == 0 {
                        la += 1;
                        let n = cur.at(la);
                        parens = n == 0 || n == b'|' || n == b')' || is_space(n);
                        break;
                    }
                    level -= 1;
                } else if c == b'[' && mode != Mode::FIXED {
                    la += 1;
                    la += (cur.at(la) == b'^') as usize;
                    la += (cur.at(la) != 0) as usize;
                    skip_to(cur, &mut la, b']');
                } else if c == b'"' {
                    la += 1;
                    skip_to(cur, &mut la, b'"');
                } else if c == b'\\' {
                    la += 1;
                    if cur.at(la) == b'Q' {
                        while cur.at(la) != 0 && (cur.at(la) != b'\\' || cur.at(la + 1) != b'E') {
                            la += 1;
                        }
                    }
                }
            }

            if parens {
                cur.pos += 1;
                cur.skip_space();
                self.list.push(OpTree::new(Op::And));
                if cur.cur() != b')' {
                    self.list.last_mut().unwrap().parse1(cur, f);
                }
                if cur.cur() == b')' {
                    cur.pos += 1;
                }
            }
        }

        if !parens {
            let mut level = 0i32;
            let mut la = cur.pos;
            self.regex.clear();

            while cur.at(la) != 0 {
                let c = cur.at(la);
                if c == b'"' {
                    self.regex.extend_from_slice(&cur.buf[cur.pos..la]);
                    la += 1;
                    cur.pos = la;
                    skip_to(cur, &mut la, b'"');
                    let mut quoted = cur.buf[cur.pos..la].to_vec();
                    let mut from = 0;
                    while let Some(to) = find(&quoted, b"\\\"", from) {
                        quoted.remove(to);
                        from = to + 1;
                    }
                    if mode != Mode::FIXED {
                        quote_in_place(&mut quoted);
                    }
                    self.regex.extend_from_slice(&quoted);
                    if cur.at(la) == b'"' {
                        la += 1;
                    }
                    cur.pos = la;
                } else if c == b'[' && mode != Mode::FIXED {
                    la += 1;
                    la += (cur.at(la) == b'^') as usize;
                    la += (cur.at(la) != 0) as usize;
                    skip_to(cur, &mut la, b']');
                    if cur.at(la) == b']' {
                        la += 1;
                    }
                } else if c == b'\\' {
                    la += 1;
                    if cur.at(la) == b'Q' {
                        while cur.at(la) != 0 && (cur.at(la) != b'\\' || cur.at(la + 1) != b'E') {
                            la += 1;
                        }
                    }
                    if cur.at(la) != 0 {
                        la += 1;
                    }
                } else if c == b'(' {
                    level += 1;
                    la += 1;
                } else if c == b')' && level > 0 {
                    level -= 1;
                    la += 1;
                } else {
                    la += 1;
                }

                let n = cur.at(la);
                if level == 0 && (n == b')' || n == b'|' || is_space(n)) {
                    break;
                }
            }

            self.regex.extend_from_slice(&cur.buf[cur.pos..la]);
            cur.pos = la;

            if f.line_regexp && self.regex.is_empty() {
                self.regex = b"^$".to_vec();
            } else if mode == Mode::FIXED {
                quote_in_place(&mut self.regex);
            }

            cur.enable_empty |= anchor_in_place(&mut self.regex, f);
        }
    }

    fn normalize(&mut self, invert: bool) {
        let invert = invert ^ (self.op == Op::Not);

        if self.list.is_empty() {
            self.op = if invert { Op::Not } else { Op::None };
            return;
        }

        for i in self.list.iter_mut() {
            i.normalize(invert);
        }

        if self.list.len() == 1 {
            let child = self.list.pop().unwrap();
            *self = child;
        } else if invert {
            if self.op == Op::And {
                self.op = Op::Or;
            } else if self.op == Op::Or {
                self.op = Op::And;
            }
        }

        // P&(Q&R) => P&Q&R, P|(Q|R) => P|Q|R; appended operands are visited too
        let mut i = 0;
        while i < self.list.len() {
            if self.list[i].op == self.op {
                let removed = self.list.remove(i);
                self.list.extend(removed.list);
            } else {
                i += 1;
            }
        }

        if self.op == Op::Or {
            if let Some(at) = self.list.iter().position(|x| x.op == Op::And) {
                let isolated = self.list.remove(at);
                let mut newlist: Vec<OpTree> = isolated
                    .list
                    .into_iter()
                    .map(|j| {
                        let mut opt = OpTree::new(Op::Or);
                        opt.list.push(j);
                        opt
                    })
                    .collect();

                for i in std::mem::take(&mut self.list) {
                    if i.op == Op::And {
                        let mut product = Vec::new();
                        for j in &i.list {
                            let mut dup = newlist.clone();
                            for k in dup.iter_mut() {
                                k.list.push(j.clone());
                            }
                            product.extend(dup);
                        }
                        newlist = product;
                    } else {
                        for j in newlist.iter_mut() {
                            j.list.push(i.clone());
                        }
                    }
                }

                self.op = Op::And;
                self.list = newlist;
            }
        }
    }

    fn convert(&self, terms: &mut Terms, f: &CnfFlags) {
        let mode = Mode(f.mode);
        if self.op != Op::And && terms.is_empty() {
            terms.push(Vec::new());
        }
        if self.op == Op::And {
            for i in &self.list {
                if terms.is_empty() {
                    terms.push(Vec::new());
                }
                if !terms.last().unwrap().is_empty() {
                    terms.push(Vec::new());
                }
                if i.op == Op::Or {
                    match i.list.iter().find(|k| k.op == Op::None && k.regex.is_empty()) {
                        Some(k) => k.add_to(terms, f),
                        None => {
                            for j in &i.list {
                                j.add_to(terms, f);
                            }
                        }
                    }
                } else {
                    i.add_to(terms, f);
                }
                if terms.last().is_none_or(|t| t.is_empty()) {
                    terms.pop();
                    if mode == Mode::BRE {
                        terms.pop();
                    }
                }
            }
        } else if self.op == Op::Or {
            for i in &self.list {
                i.add_to(terms, f);
            }
        } else {
            self.add_to(terms, f);
        }
    }

    fn add_to(&self, terms: &mut Terms, f: &CnfFlags) {
        let term = terms.last_mut().unwrap();
        if self.op == Op::Not {
            if !self.regex.is_empty() {
                if term.is_empty() {
                    term.push(None);
                } else if matches!(&term[0], Some(p) if p.is_empty()) {
                    return;
                }
                term.push(Some(self.regex.clone()));
            }
        } else {
            add_alt(term, &self.regex, f);
        }
    }
}

/// The shared "add an ALT pattern" step of `add_to` and `new_pattern`.
fn add_alt(term: &mut Term, regex: &[u8], f: &CnfFlags) {
    if term.is_empty() {
        term.push(Some(regex.to_vec()));
    } else if term[0].is_none() {
        term[0] = Some(regex.to_vec());
    } else if term[0].as_ref().unwrap().is_empty() {
        // empty pattern means anything matches
    } else if regex.is_empty() {
        term[0].as_mut().unwrap().clear();
    } else {
        let first = term[0].as_mut().unwrap();
        first.extend_from_slice(if Mode(f.mode) == Mode::BRE { b"\\|" } else { b"|" });
        first.extend_from_slice(regex);
    }
    if term[0].as_ref().unwrap().is_empty() {
        term.truncate(1);
    }
}

pub fn compile(terms: Terms, pattern: &[u8], flags: &CnfFlags) -> Terms {
    compile_flagged(terms, pattern, flags).terms
}

pub fn compile_flagged(mut terms: Terms, pattern: &[u8], flags: &CnfFlags) -> Compiled {
    let mut cur = Cur { buf: pattern.to_vec(), pos: 0, enable_empty: false };
    let mut tree = OpTree::new(Op::And);
    tree.parse(&mut cur, flags);
    tree.normalize(false);
    tree.convert(&mut terms, flags);
    Compiled { terms, enable_empty: cur.enable_empty }
}

pub fn new_pattern(terms: Terms, mask: i32, pattern: &[u8], flags: &CnfFlags) -> Terms {
    new_pattern_flagged(terms, mask, pattern, flags).terms
}

pub fn new_pattern_flagged(mut terms: Terms, mask: i32, pattern: &[u8], flags: &CnfFlags) -> Compiled {
    if terms.is_empty() {
        terms.push(Vec::new());
    }
    if flags.boolean {
        return compile_flagged(terms, pattern, flags);
    }

    let mut spattern = pattern.to_vec();
    if Mode(flags.mode) == Mode::FIXED {
        quote_in_place(&mut spattern);
    }
    let enable_empty = anchor_in_place(&mut spattern, flags);

    if (mask & pattern_mask::NEG) != 0 && !spattern.is_empty() {
        spattern.splice(0..0, b"(?^".iter().copied());
        spattern.push(b')');
    }

    let term = terms.last_mut().unwrap();
    if (mask & pattern_mask::NOT) != 0 {
        if !spattern.is_empty() {
            if term.is_empty() {
                term.push(None);
            } else if matches!(&term[0], Some(p) if p.is_empty()) {
                return Compiled { terms, enable_empty };
            }
            term.push(Some(spattern));
        }
    } else {
        add_alt(term, &spattern, flags);
    }
    Compiled { terms, enable_empty }
}

pub fn prune(mut terms: Terms, flags: &CnfFlags) -> Terms {
    if flags.line_regexp {
        return terms;
    }
    let mut idx = 0;
    let mut first = true;
    while idx < terms.len() {
        let t = &terms[idx];
        let droppable = t.is_empty() || (t.len() == 1 && t[0].as_ref().is_none_or(|p| p.is_empty()));
        if droppable && (!first || flags.file.is_empty()) {
            terms.remove(idx);
        } else {
            idx += 1;
        }
        first = false;
    }
    terms
}

pub fn split(mut terms: Terms, flags: &CnfFlags) -> Terms {
    if flags.boolean {
        return terms;
    }
    let sep: &[u8] = match Mode(flags.mode) {
        Mode::FIXED => b"\\E|\\Q",
        Mode::BRE => b"\\|",
        _ => b"|",
    };
    let nl = |c: &u8| *c == b'\n' || *c == b'\r';

    for term in terms.iter_mut() {
        for pattern in term.iter_mut().flatten() {
            let mut from = 0;
            while let Some(to) = pattern.get(from..).and_then(|s| s.iter().position(nl)).map(|p| p + from) {
                if from < to {
                    let next = pattern.get(to + 1..).and_then(|s| s.iter().position(|c| !nl(c))).map(|p| p + to + 1);
                    let Some(next) = next else {
                        pattern.truncate(to);
                        break;
                    };
                    if &pattern[next..] == b"\\E" {
                        pattern.truncate(to);
                        pattern.extend_from_slice(b"\\E");
                        break;
                    }
                    pattern.splice(to..next, sep.iter().copied());
                }
                from = to + 1;
            }
        }
    }
    terms
}

fn is_empty(terms: &Terms) -> bool {
    terms.is_empty() || (terms.len() == 1 && terms[0].is_empty())
}

pub fn report(terms: &Terms, f: &CnfFlags) -> Bytes {
    let mut o: Bytes = Vec::new();
    if is_empty(terms) && f.file.is_empty() {
        return o;
    }
    let mode = Mode(f.mode);
    let put = |o: &mut Bytes, s: &[u8]| o.extend_from_slice(s);

    put(&mut o, if f.files { b"Files " } else { b"Lines " });

    if f.fuzzy > 0 {
        put(&mut o, b"fuzzy-matched ");
        if f.best_match {
            put(&mut o, b"as best matching ");
        }
        put(&mut o, b"with max edit distance ");
        if f.fuzzy & FUZZY_INS != 0 {
            put(&mut o, b"+");
        }
        if f.fuzzy & FUZZY_DEL != 0 {
            put(&mut o, b"-");
        }
        if f.fuzzy & FUZZY_SUB != 0 {
            put(&mut o, b"~");
        }
        put(&mut o, (f.fuzzy & 0xff).to_string().as_bytes());
    } else {
        put(&mut o, b"matched");
    }
    if f.ignore_case {
        put(&mut o, b" ignoring case");
    }
    put(&mut o, b" if:");
    put(&mut o, NL);
    put(&mut o, b"  ");

    if !f.file.is_empty() {
        put(&mut o, if mode == Mode::FIXED { b"a string in " } else { b"a pattern in " });
        for (n, name) in f.file.iter().enumerate() {
            if n > 0 {
                put(&mut o, b" or ");
            }
            put(&mut o, name);
        }
        put(&mut o, if f.invert_match { b" does not match" } else { b" matches" });
        if f.line_regexp {
            put(&mut o, b" a line");
        } else if f.word_regexp {
            put(&mut o, b" a word");
        }
        if f.files {
            put(&mut o, b" on a line");
        }
        if !terms.is_empty() {
            if terms[0].is_empty() {
                put(&mut o, b", and");
                put(&mut o, NL);
                put(&mut o, b"  ");
            } else {
                put(&mut o, b" or ");
            }
        }
    }

    let mut and_sep = false;
    for term in terms {
        if and_sep {
            put(&mut o, b", and");
            put(&mut o, NL);
            put(&mut o, b"  ");
        }
        let mut or_sep = false;
        for (j, p) in term.iter().enumerate() {
            let Some(p) = p else { continue };
            if or_sep {
                put(&mut o, b" or ");
            }
            if p.is_empty() {
                put(&mut o, b"anything");
            } else {
                put(&mut o, b"\"");
                put(&mut o, p);
                put(&mut o, b"\"");
            }
            if j != 0 {
                put(&mut o, b" does not match");
                if f.files {
                    put(&mut o, b" any line");
                }
            } else {
                put(&mut o, if f.invert_match { b" does not match" } else { b" matches" });
                if f.line_regexp {
                    put(&mut o, b" a line");
                } else if f.word_regexp {
                    put(&mut o, b" a word");
                }
                if f.files {
                    put(&mut o, b" on a line");
                }
            }
            or_sep = true;
            and_sep = true;
        }
    }
    put(&mut o, NL);
    o
}

pub fn adjoin(terms: &Terms, f: &CnfFlags) -> Bytes {
    let mut adjoined: Bytes = Vec::new();
    let sep: &[u8] = if Mode(f.mode) == Mode::BRE { b"\\|" } else { b"|" };

    if f.files {
        for p in terms.iter().flatten().flatten() {
            if !p.is_empty() {
                adjoined.extend_from_slice(p);
                adjoined.extend_from_slice(sep);
            }
        }
    } else {
        let allnot = !terms.iter().any(|t| t.len() <= 1);
        if !allnot {
            for t in terms {
                if let Some(Some(p)) = t.first() {
                    if !p.is_empty() {
                        adjoined.extend_from_slice(p);
                        adjoined.extend_from_slice(sep);
                    }
                }
            }
        }
    }

    if !adjoined.is_empty() {
        adjoined.pop();
        if Mode(f.mode) == Mode::BRE {
            adjoined.pop();
        }
    }
    adjoined
}

pub fn first(terms: &Terms) -> Bytes {
    if let Some(Some(Some(p))) = terms.first().map(|t| t.first()) {
        return p.clone();
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags() -> CnfFlags {
        CnfFlags { mode: Mode::ERE.0, ..Default::default() }
    }

    #[test]
    fn enable_empty_follows_anchors() {
        let f = flags();
        let ne = |mask, p: &[u8], f: &CnfFlags| new_pattern_flagged(Vec::new(), mask, p, f).enable_empty;
        assert!(ne(0, b"^$", &f));
        assert!(ne(0, b"^foo", &f));
        assert!(ne(0, b"foo$", &f));
        assert!(!ne(0, b"foo", &f));
        assert!(!ne(0, b"^$", &CnfFlags { line_regexp: true, ..flags() }));
        assert!(!ne(0, b"^foo", &CnfFlags { word_regexp: true, ..flags() }));

        let b = CnfFlags { boolean: true, ..flags() };
        assert!(ne(0, b"^ foo", &b));
        assert!(compile_flagged(Vec::new(), b"foo$ bar", &b).enable_empty);
        assert!(!compile_flagged(Vec::new(), b"foo bar", &b).enable_empty);
        assert!(!ne(0, b"^ foo", &CnfFlags { line_regexp: true, ..b.clone() }));
        assert!(!ne(0, b"^ foo", &CnfFlags { word_regexp: true, ..b }));
    }

    #[test]
    fn bre_pops_do_not_refill_terms() {
        let f = CnfFlags { mode: Mode::BRE.0, boolean: true, ..Default::default() };
        assert_eq!(compile(Vec::new(), b"a -\"\"", &f), Vec::<Term>::new());
    }

    #[test]
    fn quote_escapes_end_marker() {
        assert_eq!(quote(b""), b"".to_vec());
        assert_eq!(quote(b"a\\Eb"), b"\\Qa\\E\\\\E\\Qb\\E".to_vec());
    }

    #[test]
    fn bool_query_normalizes_to_cnf() {
        let t = compile(vec![Vec::new()], b"a b|c", &flags());
        assert_eq!(t, vec![vec![Some(b"a".to_vec())], vec![Some(b"b|c".to_vec())]]);
        assert_eq!(adjoin(&t, &flags()), b"a|b|c".to_vec());
        assert_eq!(first(&t), b"a".to_vec());
    }

    #[test]
    fn split_at_newlines() {
        let t = split(vec![vec![Some(b"a\nb".to_vec())]], &flags());
        assert_eq!(t, vec![vec![Some(b"a|b".to_vec())]]);
    }
}
