#!/usr/bin/env python3
"""Candidate surface cases of the coverage rounds of src_core (inputs only, never expected values):
   gen_cov.py <group> > candidates.jsonl      (groups: opts, fmt, ctx, misc, tree, enc, config, big)
Every case is generated, deterministic input: the large files are made here (a counter in every line, a needle near the
256 KiB boundary of the reflex buffer) and written into the case, because the case format has only inline files.
Candidates are admitted by admit.py (adds coverage) and then recorded by run-cases --repeat 3."""
import json
import sys

import gen_cases
from gen_cases import A, B, BIN, D2, c, plus

start = len(gen_cases.cases)
BUF = 256 * 1024

# ---------------------------------------------------------------- fixtures
T = "foo bar\nbar foo\n\nfoo\nbar\nbaz qux\nFoo Bar\nfoobar\n\tfoo\ttab\nh\u00e9llo w\u00f6rld foo\nfoo foo foo\nlast foo"
CT = "".join(("match %d\n" if i in (3, 4, 9, 10, 11, 17, 20) else "line %d\n") % i for i in range(1, 21))
MIXED = {"a.bin": {"hex": "666f6f00626172ff0a666f6f2062617a0a00666f6f0a"}}
NULBIN = {"n.bin": {"hex": "6c696e65310a6d617463680a00006c696e65330a6d6174636820320a"}}
U8 = "caf\u00e9 foo\nna\u00efve bar\n\u65e5\u672c\u8a9e foo\n"
FILES = plus(A, **{"t.txt": T, "c.txt": CT, "u.txt": U8, "e.txt": ""})
FILES.update(BIN)
FILES.update(MIXED)
FILES.update(NULBIN)
FILES.update({"nonl.txt": "foo\nbar foo", "crlf.txt": "foo\r\nbar\r\nfoo bar\r\n", "tabs.txt": "a\tfoo\tb\n\tfoo\n"})


def line(i):
    return "line %06d lorem ipsum dolor sit amet\n" % i


def big(kib, needles=(), nul_at=None, tail=True):
    """text of kib KiB; the needle lines replace line i (needle words in them), NUL optional at a byte offset"""
    size = kib * 1024
    out = []
    n = 0
    i = 0
    while n < size:
        s = ("needle %06d match here\n" % i) if i in needles else line(i)
        out.append(s)
        n += len(s)
        i += 1
    text = "".join(out)
    return text


def edge(kib=300):
    """line numbers of needles around the BUF boundary of a text of ~37 bytes per line"""
    L0 = BUF // 37
    return (2, L0 - 4, L0 - 3, L0 - 1, L0, L0 + 1, L0 + 3, L0 + 40, (kib * 1024) // 37 - 3)


def bigf(kib=300, name="big.txt"):
    return {name: big(kib, edge(kib))}


# ---------------------------------------------------------------- group: ctx x output x invert
def g_ctx():
    ctxs = [[], ["-A1"], ["-B1"], ["-C1"], ["-A2", "-B1"], ["-C2"], ["-A1", "-y"], ["-y"]]
    outs = [[], ["-n"], ["-b"], ["-nb"], ["-k"], ["-T"], ["-nT"], ["-H"], ["-c"], ["-o"], ["-on"], ["-ob"], ["-ok"], ["-onT"],
            ["--color=always"], ["--color=always", "-n", "-b"], ["--tag"], ["--tag", "-n"], ["-+"], ["-+", "--break"], ["--break"],
            ["-0"], ["--json"], ["--csv"], ["--xml"], ["--hexdump"], ["--hex"], ["--with-hex"], ["-X"], ["-W"],
            ["--no-group-separator"], ["--group-separator=##"], ["--pretty=always"], ["-l"], ["-L"], ["-q"], ["-m1"], ["-m2"],
            ["-w"], ["-x"], ["-i"], ["-j"], ["--ungroup"], ["-F"], ["-E"], ["--no-filename", "-r"]]
    for ci, ctx in enumerate(ctxs):
        for inv in ([], ["-v"]):
            for oi, out in enumerate(outs):
                for fn, files in (("c", ("c.txt", FILES)), ("t", ("t.txt", FILES)), ("m", ("a.bin", FILES))):
                    pat = "match" if fn == "c" else "foo"
                    if (ci + oi) % 3 and fn != "c":
                        continue  # thin the cross product on the other files
                    id = "cov-ctx-%s%d-%s%d" % (fn, ci, "v" if inv else "p", oi)
                    c(id, ctx + inv + out + [pat, files[0]], files[1])


# ---------------------------------------------------------------- group: --format
LETTERS = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+~=<>,:;|$#@!?^.%&*-"


def g_fmt():
    pats = ["(f)(o+)", "foo", "b[ao]r|foo", "\\w+"]
    for ch in LETTERS:
        for k, f in enumerate(["%" + ch + "%~", "%{3}" + ch + "%~", "%[1]" + ch + "%~", "%[w]" + ch + "%~", "%[2|w]" + ch + "%~",
                               "%{2}[w]" + ch + "%~"]):
            id = "cov-fmt-%d-%d" % (ord(ch), k)
            c(id, ["--format=" + f, "-e", pats[k % 4], "-n", "t.txt", "c.txt"], FILES)
    for k, f in enumerate(["%f:%n:%k:%b:%l:%c:%d:%e:%w:%o%~", "%[1]f%[w]f%~", "%H %h %N %K %I %i %m %M %z %Z %u %U %a %p%~",
                           "%O%~", "%o%~", "%C%~", "%Q%~", "%A%B%~", "%s%S%~", "%,%;%:%|%~", "%1%2%3%~", "%J%X%Y%V%W%~",
                           "%T%t%~", "%{4}n%~", "%F%G%g%~", "%T%S%~", "%[1]#%~", "%[w]%~", "%%%~", "%R%~", "%v%V%~",
                           "%{2}[1|w]d %[1|w]e%~", "%[a|b|w]G%~", "%G%~", "%[1]G%~", "%[a]d%~"]):
        for mode in ([], ["-o"], ["-v"], ["-c"], ["-l"], ["-L"], ["-Hn"], ["-T"], ["--and", "bar"], ["-A1"], ["-C1", "-v"], ["-ob"]):
            id = "cov-fmt2-%d-%s" % (k, "".join(m.strip("-") for m in mode) or "none")
            c(id, ["--format=" + f] + mode + ["-e", "(f)(o+)|bar", "t.txt", "c.txt"], FILES)
    for k, extra in enumerate([["--format-begin=<<", "--format-end=>>"], ["--format-open=[", "--format-close=]"],
                               ["--format-open=[%m", "--format-close=]%~"], ["--format-begin=B%~", "--format-end=E%~"]]):
        for mode in ([], ["-o"], ["-v"], ["-c"], ["-l"], ["-A1"]):
            c("cov-fmtx-%d-%s" % (k, "".join(m.strip("-") for m in mode) or "none"),
              extra + ["--format=%f:%n:%O%~"] + mode + ["foo", "t.txt", "a.txt"], FILES)
    # json, csv, xml
    for k, mode in enumerate([[], ["-o"], ["-v"], ["-c"], ["-l"], ["-L"], ["-A1"], ["-C1", "-n"], ["-b", "-k"], ["-T"], ["--and", "bar"],
                              ["--stats"], ["-Hn", "-o"], ["-m1"], ["--color=always"], ["-nbk", "-o"]]):
        for fm in ("--json", "--csv", "--xml"):
            c("cov-struct-%s-%d" % (fm.strip("-"), k), [fm] + mode + ["foo", "t.txt", "a.txt", "a.bin", "u.txt"], FILES)
    for k, mode in enumerate([[], ["-o"], ["-A1"], ["-c"], ["-C1", "-v"]]):
        for fm in ("--json", "--csv", "--xml"):
            c("cov-structu-%s-%d" % (fm.strip("-"), k), [fm] + mode + ["-e", "foo", "-e", "bar", "--and", "baz", "u.txt", "nonl.txt", "crlf.txt"], FILES)


# ---------------------------------------------------------------- group: options and misc
def g_opts():
    base = ["foo", "a.txt"]
    for k, o in enumerate([["-a"], ["-I"], ["-U"], ["-B1"], ["-h"], ["-K1,2"], ["-K1"], ["-O"], ["-O", "txt"], ["-p"], ["-V"], ["-u"], ["-W"], ["-X"],
                           ["-Y"], ["-y"], ["-0"], ["-1"], ["-2"], ["-3"], ["-?"], ["-%"], ["-^"], ["-+"], ["-."], ["-@"], ["-z"],
                           ["-Zu"], ["-Z3"], ["-Z+3"], ["-Z-1"], ["-Z~2"], ["-Z+"], ["-Z"], ["-e", "foo", "-N", "bar"], ["-Dskip"], ["-dskip"],
                           ["-ds"], ["-dr"], ["-dread"], ["-d", "recurse"], ["-D", "read"], ["--depth=1"], ["--depth=0"], ["-1", "-2"], ["-10"],
                           ["-r", "--depth=1,2"], ["-r", "-1"], ["-r", "-3"], ["-r", "-2", "-3"], ["-r", "-1", "-5"], ["--depth=,2"], ["--depth=2,"],
                           ["--max-depth=2"], ["--min-depth=1"], ["-g", "*.txt"], ["-g", "^*.log"], ["-g", "*.txt,^*.log"], ["-t", "txt"],
                           ["-t", "list"], ["-t", "c,cpp"], ["-t", "^txt"], ["-t", "txt,c"], ["-M", "#!"], ["-M", "^#!"], ["-M", "#!,^%PDF"],
                           ["-O", "txt,c"], ["--file-magic=#!"], ["--file-type=txt"], ["--file-extension=txt,log"], ["--file-extension=^txt"],
                           ["--file-extension=[txt,log]"], ["-S"], ["-tc"], ["-tcpp"], ["-t?"], ["-j"], ["-jw"], ["--smart-case"],
                           ["--no-smart-case"], ["--dotall"], ["--no-dotall"], ["--no-hidden"], ["--hidden"], ["-.", "-r"], ["-u"],
                           ["--no-ignore-binary"], ["--ignore-binary"], ["--any-line"], ["--no-any-line"], ["--pretty=never"], 
                           ["--no-view"], ["--no-stats"], ["--stats=terse"], ["--stats=vm"], ["--stats=vmx"], ["--stats=terse,vm"], ["--tree"], ["-t"],
                           ["--no-tree"], ["--free-space"], ["-%"], ["--bool"], ["--files"], ["--max-queue=1"], ["--min-steal=1"], ["--mmap"],
                           ["--mmap=1"], ["--no-mmap"], ["--line-buffered"], ["--delay=1"], ["--confirm"], ["--no-confirm"], ["--ungroup"],
                           ["--split"], ["--no-split"], ["--grep"], ["--ugrep"], ["--perl-regexp"], ["-P"], ["--regexp=foo"], ["--regexp", "foo"],
                           ["--help"], ["--help", "format"], ["--help", "regex"], ["--help", "globs"], ["--help", "fuzzy"], ["--help", "bool"],
                           ["--help", "options"], ["--help", "colors"], ["--help", "nope"], ["-?", "all"], ["--help", "context"], ["--help", "where"],
                           ["--version"], ["-V"], ["--help", "count"], ["--help", "encoding"], ["-h", "--help"]]):
        c("cov-opt-%d" % k, o + base, FILES)
    for k, lvl in enumerate(["1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "ab", "h", "H", "c", "hc", "a", "bh", "Bc", "1A", "1ahA2", "2bA3", "3B", "A1", "B2", "C3",
                             "A1B2C3", "1aA", "1h", "1c", "1b", "4ch", "8abch", "5bh", "6cb"]):
        for kk, f in enumerate([[], ["-n"], ["-v"], ["-C1"], ["-A1", "-B1", "-v"], ["-o"], ["-c"]]):
            c("cov-hexdump-%s-%d" % (lvl, kk), ["--hexdump=" + lvl] + f + ["foo", "a.bin", "a.txt", "t.txt"], FILES)
    for k, f in enumerate(["--hex", "--with-hex", "-X", "-W", "--hexdump", "--hexdump=2", "-X -C1", "-W -v", "-X -v -A1", "-W -A1 -B1", "--hexdump=1 -C1", "-X -n -b",
                           "-W -o", "-X -o", "-X -c", "-X -l", "-W -l", "-X --color=always", "-W --color=always -n", "-X -H", "-X -y", "-W -y", "-X --tag", "-X -T"]):
        for kk, pat in enumerate(["foo", "bar", "\\x00", "."]):
            c("cov-hexf-%d-%d" % (k, kk), f.split() + [pat, "a.bin", "n.bin", "a.txt", "t.txt"], FILES)
    for k, f in enumerate(["-b", "-k", "-n", "-nk", "-T", "-H", "-c", "-o", "-on", "--color=always", "-A1", "-v", "-vc", "-vo"]):
        for kk, bf in enumerate(["binary", "text", "without-match", "hex", "with-hex", "nope"]):
            c("cov-bin-%d-%d" % (k, kk), ["--binary-files=" + bf] + f.split() + ["foo", "a.bin", "n.bin", "a.txt"], FILES)
    for k, f in enumerate([["-a"], ["-I"], ["-U"], ["-U", "-o"], ["-a", "-o"], ["-I", "-c"], ["-U", "-a"], ["-a", "-v"], ["-I", "-L"], ["-U", "-l"], ["-aU"], ["-IU"], ["-I", "-o"],
                           ["-a", "-A1"], ["-I", "-A1"], ["-U", "-C1"]]):
        for kk, pat in enumerate(["foo", "\\x00", "foo\\x00", "[^a]", "\\xff"]):
            c("cov-bina-%d-%d" % (k, kk), f + ["-e", pat, "a.bin", "n.bin", "a.txt"], FILES)
    # -m, --max-count, ranges
    for k, f in enumerate([["-m1"], ["-m2", "-A1"], ["-m1", "-v"], ["-m1", "-c"], ["-m1", "-o"], ["-m1", "-C1"], ["-m1", "-l"], ["-m1", "--and", "bar"], ["-m0"], ["-m1", "-y"],
                           ["--range=2,4"], ["--range=3"], ["--range=2,4", "-v"], ["--range=2,4", "-C1"], ["--range=2,4", "-o"], ["--range=2,4", "-c"], ["--range=,2"],
                           ["--max-line=5"], ["--min-line=3"], ["--max-line=5", "-v"], ["--max-line=5", "-C1"], ["--max-line=5", "-o"], ["--min-count=2"], ["--min-count=2", "-c"],
                           ["--min-count=2", "-o"], ["--min-count=2", "-v"], ["--min-count=2", "-n"], ["--min-count=2", "--ungroup"], ["--min-count=2", "-C1"], ["--min-count=3", "-c", "-v"],
                           ["--min-count=2", "-l"], ["--min-count=2", "-L"], ["--min-count=2", "--format=%f%~"], ["--max-count=1", "--min-count=1", "-c"],
                           ["--max-line=3", "--format=%n%~"], ["--max-line=3", "-v", "--format=%n%~"], ["--max-line=3", "-y"], ["--max-line=3", "-y", "-C1"], ["--max-line=3", "-c", "-v"],
                           ["--max-line=3", "-v", "-C1"], ["--max-line=3", "-v", "--hexdump"], ["--min-count=2", "--hexdump"], ["--min-count=2", "-X"], ["--min-count=2", "-v", "-X"],
                           ["--max-line=3", "-X"], ["--max-line=3", "-o"], ["--max-line=3", "-o", "-C1"], ["--min-count=2", "--json"], ["--min-count=2", "--csv"], ["--min-count=2", "--xml"]]):
        c("cov-limit-%d" % k, f + ["foo", "t.txt", "c.txt"], FILES)
        c("cov-limit2-%d" % k, f + ["-e", "line", "c.txt"], FILES)
    # patterns and anchors
    for k, f in enumerate([["-w", "foo"], ["-x", "foo"], ["-i", "FOO"], ["-F", "foo.*"], ["-G", "fo\\+"], ["-E", "fo+"], ["-w", "-e", "foo", "-e", "bar"], ["-x", "-e", "foo", "-e", "bar"],
                           ["-w", "-v", "foo"], ["-x", "-v", "foo"], ["-i", "-v", "foo"], ["-wo", "foo"], ["-xo", "foo"], ["-wc", "foo"], ["-xn", "foo"], ["-w", "-C1", "foo"], ["-x", "-C1", "foo"],
                           ["-w", "-e", "foo", "--and", "bar"], ["-w", "-F", "foo"], ["-x", "-F", "foo"], ["-w", "-i", "foo"], ["-w", "--bool", "foo|bar"], ["-j", "-e", "FOO"], ["-jw", "foo"],
                           ["-e", "", "-c"], ["-e", "", "-v", "-c"], ["-e", "^", "-c"], ["-e", "$", "-c"], ["-e", "^$"], ["-e", "^$", "-v"], ["-e", ".*"], ["-e", "foo|"], ["-e", "|foo", "-o"],
                           ["-e", "\\bfoo\\b"], ["-e", "\\<foo\\>"], ["-e", "(?i)FOO"], ["-e", "(?m)^foo$"], ["-e", "foo\\nbar", "-U"], ["-e", "foo.bar", "-U"], ["-e", "(foo|bar)+", "-o"],
                           ["-e", "[[:alpha:]]+", "-o"], ["-e", "\\p{L}+", "-o"], ["-e", "\\w+", "-o"], ["-e", "\\d+", "-o"], ["-e", "\\x66oo"], ["-e", "[^f]oo"], ["-e", "f.{2}"],
                           ["-F", "-e", "foo", "-e", "bar", "-w"], ["-F", "-i", "-e", "FOO", "-e", "BAR"], ["-F", "-x", "-e", "foo", "-e", "bar"], ["-F", "-w", "-o", "foo"],
                           ["--and", "foo", "bar"], ["-e", "foo", "--not", "bar"], ["--not", "foo"], ["-N", "foo"], ["--bool", "foo -bar"], ["--bool", "(foo|bar) -baz"],
                           ["-Z1", "foa"], ["-Z2", "fa"], ["-Z+1", "fooo"], ["-Z-1", "fo"], ["-Z~1", "ofo"], ["-Z1", "-o", "fa"], ["-Z1", "--best-match", "fa"], ["-Z1", "--sort=best", "fa"],
                           ["-Z1", "-c", "fa"], ["-Z1", "-v", "fa"], ["-Z1", "-w", "foa"], ["-Z1", "-x", "foa"], ["-Z1", "-i", "FOa"], ["-Z1", "-C1", "foa"], ["-Z1", "-n", "-b", "foa"],
                           ["-Z1", "--bool", "foa bra"], ["-Z1", "--and", "bra", "foa"], ["-Z1", "-F", "foa"], ["-Z1", "-U", "foa"], ["-Z1", "-l", "foa"], ["-Z1", "-L", "foa"]]):
        c("cov-pat-%d" % k, f + ["t.txt", "a.txt", "u.txt", "nonl.txt", "crlf.txt"], FILES)
    for k, f in enumerate([["-Z1", "--best-match", "fa"], ["-Z1", "--best-match", "-l", "fa"], ["-Z1", "--best-match", "-c", "fa"], ["-Z2", "--best-match", "fa"],
                           ["-Z1", "--sort=best", "fa"], ["-Z1", "--sort=rbest", "fa"], ["-Z1", "--best-match", "-v", "fa"], ["-Z1", "--best-match", "-o", "fa"],
                           ["-Z1", "--best-match", "-L", "fa"], ["-Z1", "--best-match", "-n", "-A1", "fa"], ["-Z1", "--best-match", "--format=%f:%n%~", "fa"],
                           ["-Z1", "--best-match", "--files", "fa"], ["-Z1", "--best-match", "-m1", "fa"], ["-Z1", "--best-match", "-q", "fa"],
                           ["-Z1", "--best-match", "--json", "fa"], ["-Z1", "--best-match", "-e", "fa", "--and", "ba"], ["-Z1", "--best-match", "--bool", "fa ba"],
                           ["-Z1", "--best-match", "-r", "fa"], ["-Z1", "--sort=best", "-r", "fa"], ["-Z1", "--sort=best", "-l", "-r", "fa"], ["-Z1", "--sort=rbest", "-l", "-r", "fa"],
                           ["-Z1", "--sort=best", "-c", "-r", "fa"], ["-Z1", "--sort=best", "-v", "-r", "fa"]]):
        c("cov-best-%d" % k, f + ["a.txt", "b.txt", "t.txt", "u.txt", "d"], plus(FILES, **D2, **{"b.txt": "fob\nfa\nfoa\nfaa\n"}))


MULTI = plus(FILES, **{"d/a.txt": "foo\nbar\n", "d/b.log": "foo\n", "d/s/c.txt": "foo\nfoo\n", "d/s/.h.txt": "foo\n", "d/.hid/x.txt": "foo\n", "d/s/t/deep.txt": "foo bar\n", "d/z.bin": {"hex": "666f6f00"},
                       "d/.gitignore": "*.log\n", "d/s/.ignore": "c.txt\n", "d/s/t/u.txt": "foo\nfoo\nfoo\n", "d/big1.txt": "foo\n" * 50, "d/big2.txt": "bar\n" * 1000 + "foo\n",
                       "ig.txt": "*.log\ns/\n", "pats.txt": "foo\nbaz\n", "inc.txt": "*.txt\n", "ex.txt": "*.log\n*.bin\n", "list.txt": "d/a.txt\nd/s/c.txt\n"})


def g_tree():
    for k, f in enumerate([["-r"], ["-R"], ["-r", "-l"], ["-r", "-L"], ["-r", "-c"], ["-r", "-c", "-v"], ["-r", "-o"], ["-r", "-n", "-H"], ["-r", "--tree", "-l"], ["-r", "--tree", "-c"],
                           ["-r", "--tree", "-n"], ["-r", "-t", "-l", "--break"], ["-r", "--heading"], ["-r", "-+", "--break"], ["-r", "--hidden"], ["-r", "--no-hidden"],
                           ["-r", "--include=*.txt"], ["-r", "--exclude=*.txt"], ["-r", "--exclude-dir=s"], ["-r", "--include-dir=s"], ["-r", "--exclude-dir=s*", "--include-dir=d"],
                           ["-r", "--exclude=s/*"], ["-r", "--include=d/s/*"], ["-r", "-g", "d/*.txt"], ["-r", "-g", "^d/s/**"], ["-r", "-g", "**/c.txt"], ["-r", "-g", "s/"],
                           ["-r", "-g", "/d"], ["-r", "-g", "*/a.txt"], ["-r", "-g", "d/s/", "--no-hidden"], ["-r", "--iglob=*.TXT"], ["-r", "--glob-ignore-case", "-g", "*.TXT"],
                           ["-r", "--exclude-from=ex.txt"], ["-r", "--include-from=inc.txt"], ["-r", "--ignore-files"], ["-r", "--ignore-files=.ignore"], ["-r", "--ignore-files=.gitignore", "--ignore-files=.ignore"],
                           ["-r", "--no-ignore-files"], ["-r", "--ignore-files=ig.txt"], ["-r", "--ignore-files=/nope"], ["-r", "--depth=1"], ["-r", "--depth=2"], ["-r", "-1"], ["-r", "-2"],
                           ["-r", "--depth=1,2"], ["-r", "--min-depth=2"], ["-r", "--max-files=2"], ["-r", "--max-files=1", "-l"], ["-r", "--max-size=10"], ["-r", "--min-size=10"],
                           ["-r", "--max-size=1K"], ["-r", "--min-size=1M"], ["-r", "--max-size=10,20"], ["-r", "--min-line=2"], ["-r", "--sort=name"], ["-r", "--sort=rname"], ["-r", "--sort=size"],
                           ["-r", "--sort=rsize"], ["-r", "--sort=used"], ["-r", "--sort=rused"], ["-r", "--sort=changed"], ["-r", "--sort=rchanged"], ["-r", "--sort=created"], ["-r", "--sort=rcreated"],
                           ["-r", "--sort=list"], ["-r", "--sort=rlist"], ["-r", "--sort=best"], ["-r", "--sort=bogus"], ["-r", "--sort"], ["-r", "-s"], ["-r", "--no-sort"],
                           ["-r", "-d", "skip"], ["-d", "skip", "d"], ["-d", "read", "d"], ["-d", "recurse", "d"], ["-d", "recurse,follow", "d"], ["-d", "bogus", "d"], ["-r", "-D", "skip"], ["-r", "-S"],
                           ["-r", "--dereference"], ["-r", "-R", "-S"], ["-r", "--no-dereference"], ["-r", "--dereference-files"], ["-r", "--no-dereference-files"], ["-r", "--exclude-fs=C:/"],
                           ["-r", "--include-fs=C:/"], ["-r", "--exclude-fs=/nope"], ["-r", "-I"], ["-r", "-a"], ["-r", "-U"], ["-r", "-z"], ["-r", "--zmax=2"], ["-r", "-z", "--zmax=3"],
                           ["-r", "--no-decompress"], ["-r", "--decompress"], ["-r", "--files", "-e", "foo", "--and", "bar"], ["-r", "-l", "--files", "-e", "foo", "-e", "bar"], ["-r", "-c", "--and", "bar"],
                           ["-r", "--stats", "-l"], ["-r", "--stats=terse", "-c"], ["-r", "--json"], ["-r", "--csv", "-n"], ["-r", "--xml", "-o"], ["-r", "--format=%f:%n%~"], ["-r", "-0", "-l"],
                           ["-r", "--tag"], ["-r", "-C1"], ["-r", "-A1", "-v"], ["-r", "-v", "-l"], ["-r", "-m1"], ["-r", "-m1", "-c"], ["-r", "-q"], ["-r", "-s"], ["-r", "--no-messages"],
                           ["-r", "-h"], ["-r", "--label=L"], ["-r", "-H"], ["-r", "--min-count=2", "-c"], ["-r", "--min-count=2", "-l"], ["-r", "--min-count=2", "-L"], ["-r", "-L", "-v"],
                           ["-r", "--break", "-n"], ["-r", "--break", "-o", "-n"], ["-r", "--break", "-c"], ["-r", "--break", "--heading", "-C1"], ["-r", "--break", "-v", "-C1"],
                           ["-r", "--file=pats.txt"], ["-r", "-f", "pats.txt", "-w"], ["-r", "-f", "pats.txt", "--and", "bar"]]):
        pat = [] if "--file=pats.txt" in f or "-f" in f else ["foo"]
        c("cov-tree-%d" % k, f + pat + ["d"], MULTI)
    for k, f in enumerate([["-r", "-J1"], ["-r", "-J2"], ["-r", "-J4"], ["-r", "-J8"], ["-r", "-J4", "--min-steal=1"], ["-r", "-J4", "--max-queue=1"], ["-r", "-J4", "--min-steal=1", "--max-queue=1"],
                           ["-r", "-J3", "--sort=name", "--min-steal=1"], ["-r", "-J2", "-l", "--sort=name"], ["-r", "-J4", "-c", "--sort=size"], ["-r", "-J4", "--sort=name", "-C1"],
                           ["-r", "-J4", "--sort=name", "-o"], ["-r", "-J4", "--sort=name", "--tree", "-l"], ["-r", "-J0"], ["-r", "-J-1"], ["-r", "-Jz"], ["-r", "--jobs=65"], ["-r", "--jobs=2", "--sort=name"],
                           ["-r", "-J4", "--files", "-e", "foo", "--and", "bar", "--sort=name"], ["-r", "-J4", "-m1", "--sort=name"], ["-r", "-J4", "-q"], ["-r", "-J4", "-Z1", "--best-match", "--sort=name"]]):
        c("cov-jobs-%d" % k, f + ["foo", "d"], MULTI)
    for k, f in enumerate([["-f", "pats.txt"], ["-f", "-"], ["-f", "pats.txt", "-f", "pats.txt"], ["-f", "nope.txt"], ["-f", "pats.txt", "-e", "x"], ["-f", "pats.txt", "-v"], ["-f", "pats.txt", "-w"],
                           ["-f", "pats.txt", "-F"], ["-f", "pats.txt", "-c"], ["-f", "pats.txt", "-o"], ["-f", "pats.txt", "-i", "-n"], ["-f", "pats.txt", "--and", "bar"],
                           ["--file=pats.txt"], ["--file", "pats.txt"], ["-e", "foo", "-e", "baz"], ["--regexp=foo", "--regexp=baz"], ["-e"], ["-f"], ["--and"], ["--not"], ["-N"]]):
        c("cov-pfile-%d" % k, f + ["t.txt", "a.txt"], MULTI) if k != 1 else gen_cases.cases.append({"id": "cov-pfile-1", "args": f + ["t.txt", "a.txt"], "stdin": "foo\nbaz\n", "files": MULTI})


# ---------------------------------------------------------------- encodings
def u16(s, bom="fffe", be=False):
    return bom + "".join(("%04x" % ord(ch) if be else "%02x%02x" % (ord(ch) & 255, ord(ch) >> 8)) for ch in s)


def g_enc():
    txt = "foo bar\nBar foo\nbaz\n"
    files = {"le.txt": {"hex": u16(txt)}, "be.txt": {"hex": u16(txt, "feff", True)}, "nobom.txt": {"hex": "".join("%02x00" % ord(ch) for ch in txt)},
             "u8.txt": {"hex": "efbbbf" + txt.encode().hex()}, "l1.txt": {"hex": "63616ae9206f6f6620e9e90a666f6f0a"}, "a.txt": "foo\n",
             "u32.txt": {"hex": "fffe0000" + "".join("%02x000000" % ord(ch) for ch in txt)}, "u32be.txt": {"hex": "0000feff" + "".join("000000%02x" % ord(ch) for ch in txt)},
             "bad.txt": {"hex": "666f6f20c3280a666f6f0a"}, "cp.txt": {"hex": "666f6f20808182830a"}, "e.txt": {"hex": "86969640" + "828199" + "15"}}
    for k, f in enumerate(["le.txt", "be.txt", "nobom.txt", "u8.txt", "l1.txt", "u32.txt", "u32be.txt", "bad.txt", "cp.txt", "e.txt"]):
        for kk, mode in enumerate([[], ["-o"], ["-c"], ["-n", "-b"], ["-v"], ["-C1"], ["-i"], ["-w"], ["-X"], ["--with-hex"], ["--json"], ["--csv"], ["--xml"], ["--format=%f:%n:%o:%O%~"],
                                   ["-l"], ["-U"], ["-a"], ["-I"], ["--color=always"], ["-k"], ["-ok"], ["-Z1"], ["--hexdump"], ["-T"]]):
            c("cov-enc-%d-%d" % (k, kk), mode + ["foo", f], files)
    encs = ["binary", "ascii", "utf-8", "utf-16", "UTF-16LE", "utf-16be", "utf-32", "UTF-32LE", "utf-32be", "latin1", "iso-8859-1", "iso-8859-2", "iso-8859-3", "iso-8859-4", "iso-8859-5",
            "iso-8859-6", "iso-8859-7", "iso-8859-8", "iso-8859-9", "iso-8859-10", "iso-8859-11", "iso-8859-13", "iso-8859-14", "iso-8859-15", "iso-8859-16", "macroman", "koi8-r", "koi8-u", "koi8-ru",
            "cp437", "cp850", "cp858", "cp1250", "cp1251", "cp1252", "cp1253", "cp1254", "cp1255", "cp1256", "cp1257", "cp1258", "ebcdic", "bogus", "list"]
    for k, e in enumerate(encs):
        c("cov-encn-%d" % k, ["--encoding=" + e, "foo", "l1.txt", "cp.txt", "e.txt", "le.txt"], files)
        c("cov-encn-o-%d" % k, ["--encoding=" + e, "-o", "-i", "\\w+", "l1.txt", "cp.txt", "e.txt", "le.txt", "be.txt"], files)
    for k, f in enumerate([["--encoding=utf-16", "-c", "-e", "foo", "--and", "bar"], ["--encoding=latin1", "-i", "-F", "FOO"], ["--encoding=utf-16", "--bool", "foo bar"], ["--encoding=utf-16", "-w", "foo"]]):
        c("cov-encm-%d" % k, f + ["le.txt", "be.txt", "l1.txt"], files)


# ---------------------------------------------------------------- config, colors, misc env
def g_config():
    cf = {"a.txt": "foo bar\nfoo\nbaz\n", "ug.conf": "# config\nignore-case\nline-number\ncolor=always\n", "bad.conf": "no-such-option\nline-number=\n",
          "inc.conf": "config=ug.conf\nnull\n", "cyc.conf": "config=cyc.conf\n", "empty.conf": "", "colors.conf": "colors=mt=1;31:fn=34\ncontext=1\nsort=name\n",
          "quoted.conf": "format=\"%f:%n:%O%~\"\nhidden\ntabs=4\n"}
    for k, f in enumerate([["--config=ug.conf"], ["--config=bad.conf"], ["--config=inc.conf"], ["--config=cyc.conf"], ["--config=empty.conf"], ["--config=colors.conf"], ["--config=quoted.conf"],
                           ["--config=nope.conf"], ["--config"], ["--config=ug.conf", "--config=colors.conf"], ["--config=ug.conf", "--no-config"], ["--no-config", "--config=ug.conf"],
                           ["---config=ug.conf"], ["--config=ug.conf", "-v"], ["--config=colors.conf", "-r"], ["--config=ug.conf", "--stats"]]):
        c("cov-config-%d" % k, f + ["foo", "a.txt"], cf)
        c("cov-config-env-%d" % k, ["foo", "a.txt"], cf, env={"UGREP_CONFIG": "ug.conf" if k % 2 else "colors.conf"})
    c("cov-config-env-nope", ["foo", "a.txt"], cf, env={"UGREP_CONFIG": "nope.conf"})
    c("cov-config-home", ["foo", "a.txt"], plus(cf, **{".ugrep": "ignore-case\nline-number\n"}))
    c("cov-config-home-bad", ["foo", "a.txt"], plus(cf, **{".ugrep": "bogus\n"}))
    saves = [["-i"], ["-n", "-b", "-k", "-T"], ["--color=always", "--colors=mt=1;32"], ["--pretty"], ["--pretty=always"], ["--heading", "--break"], ["--no-heading"], ["--no-break"], ["--line-number"],
             ["--column-number"], ["--byte-offset"], ["--hex"], ["--with-hex"], ["--hexdump=2"], ["--no-group-separator"], ["--group-separator=--"], ["-C2"], ["-A1", "-B2"], ["--ignore-case"],
             ["--smart-case"], ["--min-count=2"], ["--glob-ignore-case"], ["--hidden"], ["--no-hidden"], ["--ignore-binary"], ["--binary-files=text"], ["--binary-files=hex"], ["--binary-files=without-match"],
             ["-a"], ["-I"], ["-U"], ["--dereference"], ["--dereference-files"], ["-r"], ["-R"], ["-S"], ["--depth=3"], ["--max-depth=2"], ["--min-depth=1"], ["--max-files=3"], ["--max-size=1M"],
             ["--min-size=1"], ["--max-count=5"], ["--max-line=100"], ["--sort=size"], ["--sort=rname"], ["--sort=best"], ["--ignore-files"], ["--ignore-files=.ig"], ["--exclude=*.o"], ["--include=*.c"],
             ["--exclude-dir=x"], ["--include-dir=x"], ["--exclude-from=ex"], ["--include-from=inc"], ["--glob=*.c"], ["--iglob=*.C"], ["--file-type=c"], ["--file-extension=c"], ["--file-magic=#!"],
             ["--filter=pdf:pdftotext % -"], ["--encoding=latin1"], ["--tabs=4"], ["-T"], ["--tag"], ["--separator=|"], ["--stats"], ["--jobs=3"], ["--no-messages"], ["--null"], ["--tree"], ["--decompress"], ["-z"],
             ["--zmax=3"], ["--fuzzy=2"], ["-Z+1"], ["--best-match"], ["-w"], ["-x"], ["-F"], ["-G"], ["-E"], ["--bool"], ["--files"], ["--format=%f%~"], ["--json"], ["--csv"], ["--xml"], ["--hyperlink=file"],
             ["--any-line"], ["-y"], ["--invert-match"], ["-v"], ["-c"], ["-l"], ["-L"], ["-o"], ["-q"], ["--width=80"], ["--pager=less"], ["--no-pager"], ["--no-color"], ["--no-stats"], ["--no-mmap"],
             ["--mmap"], ["--dotall"], ["--free-space"], ["--no-ignore-case"], ["--no-byte-offset"], ["-O", "c"], ["--ungroup"], ["--sort=created"], ["--sort=changed"], ["--sort=used"], ["--label=X"],
             ["--no-confirm"], ["--confirm"], ["--no-tree"], ["--exclude-fs=/"], ["--include-fs=/"], ["--devices=skip"], ["--directories=skip"], ["--no-filename"], ["-H"], ["--no-line-number"]]
    for k, f in enumerate(saves):
        c("cov-save-%d" % k, ["--save-config=out.conf"] + f, {"a.txt": "foo\n"})
        c("cov-saveu-%d" % k, ["--save-config"] + f, {"a.txt": "foo\n"})
    c("cov-save-all", ["--save-config=out.conf", "-i", "-n", "-b", "--color=always", "--hyperlink=file", "--tabs=2", "--break", "--heading", "--no-group-separator", "-C1", "--hidden", "-r", "--depth=3",
                       "--sort=size", "--exclude=*.o", "--include=*.c", "--jobs=2", "--max-count=3", "--binary-files=hex", "--hexdump=2", "--encoding=latin1", "--stats", "--tree", "--zmax=2", "-z", "--filter=pdf:cat",
                       "--format=%f%~", "--separator=:"], {"a.txt": "foo\n"})
    c("cov-save-update", ["--save-config=ug.conf", "-v"], cf)
    c("cov-save-nodir", ["--save-config=nodir/out.conf", "-v"], cf)
    c("cov-save-then-use", ["--config=ug.conf", "--save-config=new.conf", "-w"], cf)
    c("cov-save-type", ["--save-config=out.conf", "-t", "c"], {"a.txt": "foo\n"})
    c("cov-save-qq", ["--save-config=out.conf", "-q", "foo"], {"a.txt": "foo\n"})
    envs = [{"GREP_COLORS": "mt=1;31"}, {"GREP_COLORS": "ms=1;32:mc=1;33:sl=1:cx=2:fn=35:ln=32:cn=33:bn=34:se=36:rv:ne"}, {"GREP_COLORS": "bogus"}, {"GREP_COLORS": "mt=:ne"}, {"GREP_COLOR": "1;32"},
            {"GREP_COLOR": "bogus"}, {"NO_COLOR": "1"}, {"NO_COLOR": ""}, {"TERM": "dumb"}, {"TERM": "xterm-256color"}, {"COLORTERM": "truecolor"}, {"GREP_COLORS": "hl=1:hy=file"}, {"GREP_COLORS": "hl"}, {"GREP_COLORS": "ms=1;31:mc=1;31:rv"}, {"UGREP_COLORS": "mt=1;34"}, {"HOSTNAME": "h"}, {"COMPUTERNAME": "PC"}]
    for k, e in enumerate(envs):
        for kk, f in enumerate([["--color=always"], ["--color=always", "-n", "-b", "-H", "-A1", "-v"], ["--color=always", "-o", "-C1"], ["--color=always", "-c"], ["--color=always", "-l"], ["--color=always", "--tag"],
                                ["--color=auto"], ["--color"], ["--color=always", "--hyperlink"], ["--color=always", "-+", "--break", "-T"], ["--color=always", "-v", "-C1", "-n"], ["--color=always", "--format=%f:%n%~"]]):
            c("cov-env-%d-%d" % (k, kk), f + ["foo", "a.txt", "t.txt"], plus(FILES, **{}), env=e)
    for k, col in enumerate(["mt=1;31", "ms=4:mc=5", "sl=1;40:cx=2;41", "rv", "ne", "fn=1:ln=2:cn=3:bn=4:se=5", "hl:hl", "bogus=1", "mt=", "ms=1;31:rv:ne", "ne:rv:mt=7", "sl=:cx=", "mt=1;38;5;200", "ma=1:mn=2:cs=3:fn=4"]):
        for kk, f in enumerate([["-n"], ["-v", "-C1", "-nb"], ["-o"], ["-c"], ["-l"], ["-vn", "-A1"], ["-y"], ["--tag"], ["-H", "-k", "-T"], ["--break", "-+"], ["--hexdump", "-C1"], ["-X"], ["-W", "-v"]]):
            c("cov-colors-%d-%d" % (k, kk), ["--color=always", "--colors=" + col] + f + ["foo", "t.txt", "a.bin"], FILES)
    for k, hl in enumerate(["", "+", "file", "file://host", "+line", "+column", "file+line", "vscode", "bogus", "host", "path", "+,host", "file://,+", "line,column", "net", "unc", "wsl", "mac:file"]):
        for kk, f in enumerate([[], ["-n"], ["-c"], ["-l"], ["-r", "-o", "-n"], ["-o", "--column-number"], ["--tag"]]):
            c("cov-hyper-%d-%d" % (k, kk), ["--color=always", "--hyperlink=" + hl] + f + ["foo", "t.txt", "a.txt"], FILES)
    for k, f in enumerate([["--tabs=1"], ["--tabs=2", "-n"], ["--tabs=3", "-T", "-nb"], ["--tabs=4", "-o", "-T", "-k"], ["--tabs=8", "-T", "-H", "-C1"], ["--tabs=0"], ["--tabs=9"],
                           ["-T", "--break", "-+"], ["--separator=|", "-nbkH"], ["--separator=", "-n"], ["--separator=//", "-nbH", "-C1"], ["--context-separator=##", "-C1"], ["--group-separator=", "-C1"],
                           ["--no-group-separator", "-C1"], ["--group-separator=XX", "-C1", "-v"], ["--width=10"], ["--width=10", "-o"], ["--width=10", "-C1", "-n"], ["--width=3", "--color=always"],
                           ["--width=0"], ["--width=5", "-v"], ["--width=40", "--hexdump"], ["--width=40", "-X"], ["--width=3", "--format=%O%~"], ["--width=20", "--json"], ["--label=LBL", "-H"],
                           ["--label=LBL", "-c", "-H"], ["--label=LBL", "-l"], ["--label=LBL", "-L", "-v"], ["--label=LBL", "-n"], ["--replace=X"], ["--replace=%1%~"], ["--replace=[%m]", "-o"],
                           ["--replace=%f:%n"], ["--replace=%1", "--color=always"], ["--replace=%1", "-v"], ["--replace=%1", "-c"], ["--replace=%1", "-l"], ["--replace=X", "-C1"], ["--replace=X", "-A1", "-n"],
                           ["--replace=%1", "--and", "bar"], ["--replace=%[1]", "-n"], ["--replace=%b", "-b"], ["--replace=%Z", "-o"], ["--replace=%u", "-o"], ["--replace=%2", "-e", "(f)(o+)"],
                           ["--tag=T"], ["--tag=[%]", "-n"], ["--tag", "-o"], ["--tag", "-v"], ["--tag", "-C1", "-n"], ["--tag", "--color=always", "-o"], ["--tag=", "-n"], ["--tag", "-c"], ["--tag", "-l"],
                           ["--tag", "--hexdump"], ["--tag", "-X"], ["--tag", "-b", "-k", "-nT"], ["--null", "-n"], ["-0", "-c"], ["-0", "-l"], ["-0", "-L"], ["-0", "--tag"], ["-0", "-o", "-H"],
                           ["-0", "--break", "-+"], ["--null-data", "-c"], ["-z", "-c"], ["--line-buffered", "-n"], ["--line-buffered", "-c"], ["-s", "nope.txt"], ["--no-messages", "nope.txt"],
                           ["-s", "-q", "nope.txt"], ["-q", "nope.txt", "a.txt"], ["-q", "-s", "nope.txt", "a.txt"], ["-ql"], ["-qc"], ["-qo"], ["-qv"], ["-lq"], ["-lc"], ["-Lc"], ["-lv"], ["-Lv"], ["-lo"], ["-Lo"],
                           ["-lcv"], ["-lnb"], ["-l", "--format=%f%~"], ["-L", "--format=%f%~"], ["-l", "--json"], ["-L", "--json"], ["-c", "--json"], ["-lc", "--csv"], ["-L", "--xml"], ["-l", "-0", "--tag"]]):
        c("cov-misc-%d" % k, f + ["foo", "t.txt", "a.txt"], FILES) if not f[0].endswith(("nope.txt",)) else c("cov-misc-%d" % k, f, FILES)
    c("cov-stdin-1", ["foo"], FILES, stdin=T)
    c("cov-stdin-2", ["-c", "foo", "-"], FILES, stdin=T)
    c("cov-stdin-3", ["-H", "foo", "-"], FILES, stdin=T)
    c("cov-stdin-4", ["--label=X", "-H", "-n", "foo"], FILES, stdin=T)
    c("cov-stdin-5", ["-o", "-C1", "foo"], FILES, stdin=T)
    c("cov-stdin-6", ["-A1", "-v", "foo"], FILES, stdin=T)
    c("cov-stdin-7", ["-r", "foo"], FILES, stdin=T)
    c("cov-stdin-8", ["-l", "foo", "-", "a.txt"], FILES, stdin=T)
    c("cov-stdin-9", ["-X", "foo"], FILES, stdin="foo\x00bar\n")
    c("cov-stdin-10", ["foo", "-", "-"], FILES, stdin=T)
    c("cov-stdin-11", ["-f", "-", "a.txt"], FILES, stdin="foo\nbaz\n")
    c("cov-stdin-12", ["-z", "foo"], FILES, stdin=T)
    c("cov-filter-1", ["--filter=txt:cmd /c type %", "foo", "a.txt"], FILES)
    c("cov-filter-2", ["--filter=txt:cmd /c type %,bin:cmd /c exit 0", "foo", "a.txt", "a.bin"], FILES)
    c("cov-filter-3", ["--filter=*:cmd /c type %", "foo", "a.txt"], FILES)
    c("cov-filter-4", ["--filter=txt:nope-cmd %", "foo", "a.txt"], FILES)
    c("cov-filter-5", ["--filter=txt:cmd /c type %", "--filter-magic-label=txt:foo", "foo", "a.txt"], FILES)
    c("cov-filter-6", ["--filter=txt:cmd /c type %", "-c", "foo", "a.txt", "t.txt"], FILES)
    c("cov-filter-7", ["--filter=txt:cmd /c type %", "-n", "-o", "foo", "a.txt"], FILES)
    c("cov-filter-8", ["--filter=txt:cmd /c type \"%\"", "foo", "a.txt"], FILES)
    c("cov-filter-9", ["--filter=txt:cmd /c type", "foo", "a.txt"], FILES)
    c("cov-filter-10", ["--filter=txt:cmd /c type %", "-r", "foo", "d"], MULTI)
    c("cov-filter-11", ["--filter=txt:cmd /c type %", "foo", "nosuffix"], plus(FILES, nosuffix="foo\n"))
    c("cov-filter-12", ["--filter=*:cmd /c type %", "--filter-magic-label=*:foo", "foo", "nosuffix"], plus(FILES, nosuffix="foo\n"))
    c("cov-filter-13", ["--filter=txt:cmd /c type %", "foo"], FILES, stdin="foo\n")
    c("cov-filter-14", ["--filter=txt:cmd /c type %,+txt:cmd /c type %", "foo", "a.txt"], FILES)
    c("cov-filter-15", ["--filter=txt:cmd /c exit 3", "foo", "a.txt"], FILES)
    c("cov-filter-16", ["--filter=txt:cmd /c type %", "--no-filter", "foo", "a.txt"], FILES)
    c("cov-filter-17", ["--filter=txt:", "foo", "a.txt"], FILES)
    c("cov-filter-18", ["--filter=:cmd", "foo", "a.txt"], FILES)


# ---------------------------------------------------------------- large inputs
def g_big():
    N = edge()
    f3 = bigf(300)
    cases3 = [["-A2"], ["-B2"], ["-C2"], ["-A3", "-n"], ["-B3", "-nb"], ["-C3", "-n", "-H"], ["-C2", "-c"], ["-c"], ["-c", "-v"], ["-v", "-c", "-C1"], ["-o"], ["-on"], ["-ob"], ["-o", "-A2"], ["-o", "-B2"], ["-o", "-C2", "-n"],
              ["-o", "-C1", "-b", "-k"], ["-o", "-y"], ["-m1"], ["-m2"], ["-m1", "-A3"], ["-m3", "-C2"], ["-m3", "-o"], ["-m3", "-c"], ["-l"], ["-L"], ["-q"], ["-n"], ["-nb"], ["-nbk"], ["-k"], ["-b"], ["-ok"], ["-H"],
              ["--color=always", "-C2", "-n"], ["--color=always", "-o", "-C2"], ["--color=always", "-v", "-C1"], ["--tag", "-C2"], ["--tag", "-o"], ["--json"], ["--json", "-C2"], ["--csv", "-C1"], ["--xml", "-A1"],
              ["--format=%n:%o:%O%~"], ["--format=%n:%O%~", "-v"], ["--format=%[1]n:%b%~", "-C2"], ["--format=%n%~", "-c"], ["--hexdump"], ["--hexdump", "-C1"], ["--hexdump=2", "-A2"], ["-X"], ["-X", "-C1"], ["-W", "-A2"],
              ["-X", "-o"], ["-W", "-v"], ["--with-hex", "-C2"], ["-w"], ["-x"], ["-i"], ["--and", "here"], ["--bool", "needle -line"], ["-e", "needle", "--not", "000"], ["-U"], ["-U", "-A2"], ["-a", "-C2"], ["-I", "-C2"],
              ["--min-count=2"], ["--min-count=3", "-c"], ["--max-line=8000", "-c"], ["--max-line=8000", "-A2"], ["--range=7000,7100", "-C1"], ["--range=1,8000"], ["--stats", "-c"], ["-Z1", "-c"], ["-j", "-o"], ["--mmap", "-C2"],
              ["--no-mmap", "-C2"], ["--ungroup", "-o"], ["--break", "-C1"], ["-+", "--break", "-n"], ["-0", "-c"], ["-y", "-m2"], ["-y", "-m1", "-C1"], ["-y", "-v", "-m2"]]
    for k, f in enumerate(cases3):
        pat = ["needle"] if not any(x in ("-v", "-e", "--and", "--bool", "--not", "-j", "-Z1") for x in f) else []
        if f and f[0] in ("-v",) or "-v" in f:
            pat = ["line"]
        if "-e" in f or "--bool" in f or "--and" in f or "-j" in f or "-Z1" in f:
            pat = pat if pat else []
        args = list(f)
        if "--and" in f:
            args = ["-e", "needle"] + f
        elif "--bool" in f:
            args = f
        elif "-j" in f:
            args = f + ["NEEDLE"]
        elif "-Z1" in f:
            args = f + ["needlf"]
        elif "-e" in f:
            args = f
        else:
            args = f + pat
        c("cov-big-300-%d" % k, args + ["big.txt"], f3)
    f6 = bigf(600)
    for k, f in enumerate([["-c", "needle"], ["-A2", "-m5", "needle"], ["-o", "-C1", "needle"], ["-v", "-c", "line"], ["-v", "-A1", "-B1", "line"], ["-B2", "-n", "needle"], ["-C1", "-b", "needle"], ["-o", "-b", "-A1", "needle"],
                           ["-l", "needle"], ["-m1", "needle"], ["--json", "-m4", "needle"], ["-X", "-m3", "needle"], ["--format=%n%~", "-v", "line"]]):
        c("cov-big-600-%d" % k, f + ["big.txt"], f6)
    f12 = bigf(1200)
    for k, f in enumerate([["-c", "needle"], ["-A1", "-B1", "-m6", "needle"], ["-o", "-A1", "needle"], ["-v", "-c", "line"], ["-v", "-C1", "line"], ["-n", "-C1", "needle"], ["-on", "needle"], ["-b", "needle", "-m3"]]):
        c("cov-big-1200-%d" % k, f + ["big.txt"], f12)
    # a single line longer than the buffer
    long = "x" * 100000 + "needle" + "y" * 100000 + "foo" + "z" * 100000 + "\n"
    fl = {"long.txt": "first foo\n" + long + "needle again\nlast foo\n"}
    for k, f in enumerate([["-c", "needle"], ["-o", "needle"], ["-on", "needle"], ["-ob", "needle"], ["-n", "-o", "-C1", "foo"], ["-C1", "-c", "foo"], ["-A1", "-n", "needle"], ["-B1", "-b", "needle"], ["-v", "-c", "x"],
                           ["-v", "-A1", "-c", "foo"], ["-m1", "-o", "needle"], ["-m1", "-c", "foo"], ["-l", "needle"], ["-o", "-w", "foo"], ["-o", "-x", "needle"], ["--format=%n:%o:%d%~", "needle"], ["--format=%n:%o%~", "-v", "x"],
                           ["--json", "-o", "needle"], ["--csv", "-o", "needle"], ["--xml", "-o", "needle"], ["-X", "-o", "needle"], ["-X", "needle"], ["--hexdump=1", "-o", "foo"], ["--hexdump", "-C1", "foo"],
                           ["--color=always", "-o", "needle"], ["--color=always", "-C1", "foo"], ["--tag", "-o", "foo"], ["--max-line=1", "foo"], ["-U", "-o", "foo.z"], ["-U", "-c", "needle"],
                           ["-o", "--tabs=2", "-T", "needle"], ["-w", "-c", "foo"], ["-i", "-c", "NEEDLE"], ["-F", "-c", "needle"], ["-y", "-c"], ["-y", "-m2", "-C1"], ["-b", "-o", "again"], ["-ob", "foo"]]):
        c("cov-long-%d" % k, f + ["long.txt"], fl)
    # NUL after the boundary
    base = big(300, edge(300))
    cut = BUF + 5000
    nul = base[:cut] + "\x00" + base[cut + 1:]
    fn = {"nul.txt": nul}
    for k, f in enumerate([["needle"], ["-c", "needle"], ["-o", "needle"], ["-n", "needle"], ["-C1", "needle"], ["-v", "-c", "line"], ["-v", "line"], ["-l", "needle"], ["-L", "needle"], ["-q", "needle"], ["-a", "-c", "needle"],
                           ["-a", "-C1", "needle"], ["-a", "-v", "-c", "line"], ["-a", "-o", "needle"], ["-I", "-c", "needle"], ["-I", "needle"], ["-I", "-v", "-c", "line"], ["-U", "-c", "needle"], ["-U", "-o", "needle"],
                           ["--binary-files=binary", "needle"], ["--binary-files=text", "-C1", "needle"], ["--binary-files=text", "-c", "needle"], ["--binary-files=text", "-o", "needle"], ["--binary-files=without-match", "needle"],
                           ["--binary-files=without-match", "-c", "needle"], ["--binary-files=hex", "-m2", "needle"], ["--binary-files=with-hex", "-m2", "needle"], ["-X", "-m2", "needle"], ["-W", "-m3", "needle"],
                           ["-X", "-C1", "-m2", "needle"], ["--hexdump", "-m2", "needle"], ["--with-hex", "-C1", "-m3", "needle"], ["-m1", "needle"], ["-m3", "-a", "needle"], ["--json", "-m3", "needle"], ["--format=%n:%O%~", "-m3", "needle"],
                           ["-o", "-m3", "-a", "-C1", "needle"], ["-v", "-m2", "-a", "-A1", "needle"], ["-v", "-A1", "-m3", "line"], ["-a", "-v", "-C1", "-m3", "line"], ["-A2", "needle"], ["-B2", "needle"], ["-y", "-m2"], ["-a", "-y", "-m2"],
                           ["--tag", "needle"], ["-a", "--tag", "-m2", "needle"]]):
        c("cov-nul-%d" % k, f + ["nul.txt"], fn)
    # NUL straddles via stdin and with 600K
    c("cov-nul-stdin-1", ["-c", "needle"], {}, stdin=nul)
    c("cov-nul-stdin-2", ["-a", "-c", "needle"], {}, stdin=nul)
    c("cov-big-stdin-1", ["-C2", "needle"], {}, stdin=big(300, edge(300)))
    c("cov-big-stdin-2", ["-o", "-C1", "-n", "needle"], {}, stdin=big(300, edge(300)))
    c("cov-big-stdin-3", ["-v", "-c", "line"], {}, stdin=big(300, edge(300)))
    c("cov-big-stdin-4", ["-v", "-A1", "-B1", "line"], {}, stdin=big(300, edge(300)))
    base6 = big(600, edge(600))
    cut6 = 2 * BUF + 100
    n6 = {"nul6.txt": base6[:cut6] + "\x00" + base6[cut6 + 1:]}
    for k, f in enumerate([["-c", "needle"], ["-a", "-c", "needle"], ["-I", "-c", "needle"], ["--binary-files=without-match", "-c", "needle"], ["-a", "-o", "-m3", "needle"], ["-v", "-c", "line"]]):
        c("cov-nul6-%d" % k, f + ["nul6.txt"], n6)
    # restline cases (matches near the buffer boundary with output of the rest of the line)
    pad = "p" * (BUF - 60)
    rl = {"rl.txt": pad + "\nneedle-start " + "q" * 70 + " tail\nline\nneedle again\n" + "r" * (60 * 1024) + "\n"}
    rl2 = {"rl2.txt": "".join("row %05d needle xyz padding padding padding\n" % i for i in range(7000))}
    for k, f in enumerate([["-o", "needle"], ["-o", "-C1", "needle"], ["-o", "-A1", "-n", "needle"], ["-X", "needle"], ["--hexdump", "-C1", "needle"], ["-v", "-C1", "needle"], ["-v", "-c", "needle"], ["-v", "-X", "needle"],
                           ["-C2", "needle"], ["-A1", "-v", "line"], ["-y", "-o", "needle"], ["--hex", "-v", "line"], ["--with-hex", "-A2", "needle"]]):
        c("cov-rest-%d" % k, f + ["rl.txt"], rl)
        c("cov-rest2-%d" % k, f + ["rl2.txt"], rl2)


def g_rand(n=6000, seed=20260514):
    """seeded pseudo-random combinations of option groups on a handful of files (deterministic: same seed, same cases)"""
    import random
    rnd = random.Random(seed)
    mix = {"m.bin": {"hex": "666f6f2062617200626172206261720a0a666f6f0a626100666f6f0a6c696e650a"},
           "m2.bin": {"hex": "6c696e65310a666f6f20626172206261780a6c696e65330a666f6f00626172ff0a6c696e65350a6c696e65360a666f6f0a"},
           "ml.txt": "foo\nbar\nbaz\n\nfoo bar\nbar\nfoo\nlast\n", "w.txt": "foo_bar foo-bar\nfoobar\n bar foo \nFOO\n", "u8.txt": U8 + "\u00e9\u00e9 foo\n" + "x" * 300 + " foo\n",
           "u16.txt": {"hex": u16("foo bar\nbar foo\nline\nfoo\n")}}
    files = plus(FILES, **mix)
    sets = {
        "mode": [[], [], [], ["-y"], ["-v"], ["-c"], ["-l"], ["-L"], ["-o"], ["-q"], ["-vy"], ["-vc"], ["-vo"], ["-yo"], ["-cy"], ["-ly"]],
        "ctx": [[], [], [], ["-A1"], ["-B1"], ["-C1"], ["-C2"], ["-A2"], ["-A1", "-B2"], ["-C3"]],
        "num": [[], [], ["-n"], ["-b"], ["-k"], ["-nb"], ["-nbk"], ["-H"], ["-T"], ["-nT"], ["-u"], ["-nu"], ["-ou"], ["--break"], ["-+"], ["-+", "--break", "-n"], ["-0"], ["-h"]],
        "out": [[], [], [], [], ["--color=always"], ["--tag"], ["-X"], ["-W"], ["--hexdump"], ["--hexdump=2"], ["--hexdump=1b"], ["--hexdump=4A2B3"], ["--with-hex"], ["--hex"], ["--json"], ["--csv"], ["--xml"],
                ["--format=%f:%n:%o%~"], ["--format=%[1]b:%b:%O%~"], ["--format=%n%$%o%t%s%~"], ["--replace=<%m>"], ["--replace=%1%~"], ["--color=always", "-A1"], ["--tag", "--color=always"], ["--colors=ne:rv", "--color=always"]],
        "pat": [["foo"], ["foo"], ["bar"], ["foo.*bar"], ["-e", "foo", "-e", "bar"], ["-e", "foo", "--and", "bar"], ["-e", "foo", "--not", "bar"], ["--bool", "foo bar|line"], ["--bool", "foo -bar"],
                ["-w", "foo"], ["-x", "foo"], ["-i", "foo"], ["-F", "foo"], ["-Z1", "fo"], ["-e", "(foo)(bar)?"], ["-U", "foo\\nbar"], ["-U", "-e", "foo\\n"], ["-e", "^$"], ["-e", ""], ["-e", "[a-z]+", "-w"], ["-e", ".", "-c"],
                ["-e", "\\x00"], ["-U", "-e", "bar\\x00bar"], ["-e", "o+"], ["-e", "l.ne|foo"]],
        "lim": [[], [], [], [], [], ["-m1"], ["-m2"], ["--min-count=2"], ["--max-line=3"], ["--range=2,6"], ["--max-line=5", "-v"], ["--min-line=2"]],
        "bin": [[], [], [], [], ["-a"], ["-I"], ["-U"], ["--binary-files=text"], ["--binary-files=without-match"], ["--binary-files=hex"], ["--binary-files=with-hex"], ["-aU"]],
        "file": [["t.txt"], ["c.txt"], ["m.bin"], ["m2.bin"], ["m.bin", "m2.bin", "ml.txt"], ["ml.txt"], ["w.txt"], ["u8.txt"], ["u16.txt"], ["nonl.txt"], ["crlf.txt"], ["a.bin", "t.txt"], ["--encoding=utf-16", "u16.txt"]],
    }
    seen = set()
    for i in range(n):
        args = []
        for k in ("mode", "ctx", "num", "out", "lim", "bin"):
            args += rnd.choice(sets[k])
        pat = rnd.choice(sets["pat"])
        fl = rnd.choice(sets["file"])
        a = args + pat + fl
        key = tuple(a)
        if key in seen:
            continue
        seen.add(key)
        c("cov-rand-%d" % i, a, files)


def g_rand2(n=4000, seed=7771):
    """patterns, tags, colour letters, wildcards, ignore files, file types: seeded combinations"""
    import random
    import re
    rnd = random.Random(seed)
    src = open("src/ugrep.cpp", encoding="utf-8", errors="replace").read()
    types = re.findall(r'^\s*\{ "([a-z0-9+#-]+)",\s+"', src, re.M)
    tree = plus(MULTI, **{"x.c": "foo\n", "y.cpp": "foo\n", "z.py": "#!/usr/bin/python\nfoo\n", "s.sh": "#!/bin/sh\nfoo\n", "p.pdf": "%PDF-1.4\nfoo\n", "h.html": "<html>foo</html>\n",
                          "m.md": "foo\n", "j.json": "{\"foo\":1}\n", "r.rs": "fn foo(){}\n", "g.go": "func foo(){}\n", "ab.txt": "foo\n", "q.txt": "foo\n", ".hidden": "foo\n", ".hid.txt": "foo\n",
                          "Foo.TXT": "foo\n", "sub/.gitignore": "/keep.txt\n!/n.txt\nneg\n*.tmp\n!a.tmp\nsub2/\n", "sub/keep.txt": "foo\n", "sub/n.txt": "foo\n", "sub/a.tmp": "foo\n", "sub/b.tmp": "foo\n",
                          "sub/sub2/in.txt": "foo\n", "sub/neg": "foo\n", ".ignore": "# c\n\n*.md\n/z.py\nd/s/\n!y.cpp\n", "pdf.dat": "%PDF-1.4\nfoo\n"})
    for t in types:
        c("cov-type-%s" % t.replace("+", "plus").replace("#", "sharp"), ["-r", "-l", "-t", t, "foo", "."], tree)
        c("cov-ntype-%s" % t.replace("+", "plus").replace("#", "sharp"), ["-r", "-l", "-t", "^" + t, "foo", "."], tree)
    for k, f in enumerate([["-O", "c,py"], ["-O", "^c"], ["-O", "txt", "-O", "md"], ["-M", "#!/bin/sh"], ["-M", "%PDF"], ["-M", "^%PDF"], ["-M", "#!,%PDF"], ["-M", "%PDF", "-O", "txt"],
                           ["-t", "sh", "-M", "#!/bin/sh"], ["-g", "*.c", "-O", "py"], ["-O", "c", "-g", "^x.c"], ["-M", "(#!)", "-l"], ["-M", "#!/usr/bin/python"], ["-M", "[%]PDF"], ["-tc,cpp,py"], ["-t", "c", "-t", "cpp"],
                           ["--file-type=c,^cpp"], ["-t", "python,shell"], ["-t", "pdf"], ["-t", "txt", "--exclude=*.md"], ["-O", "[c,py]"], ["-O", "c", "-v"]]):
        c("cov-ft-%d" % k, ["-r", "-l"] + f + ["foo", "."], tree)
    ig = [["--ignore-files"], ["--ignore-files=.ignore"], ["--ignore-files=.gitignore"], ["--ignore-files=.gitignore", "--ignore-files=.ignore"], ["--ignore-files", "--hidden"], ["--ignore-files=sub/.gitignore"],
          ["--ignore-files=.ignore", "-t", "c"], ["--ignore-files=.gitignore", "--no-ignore-files"], ["--ignore-files", "-I"], ["--ignore-files", "--include=*.txt"], ["--ignore-files", "--exclude-dir=s"],
          ["--ignore-files", "-d", "recurse"], ["--ignore-files=ig.txt", "--hidden"], ["--ignore-files", "-1"], ["--ignore-files", "-3"], ["--ignore-files", "--sort=size"], ["--ignore-files", "--tree", "-l"]]
    for k, f in enumerate(ig):
        for kk, extra in enumerate([["-r", "-l"], ["-r", "-c"], ["-R", "-n"], ["-r", "--stats", "-l"]]):
            c("cov-ig-%d-%d" % (k, kk), f + extra + ["foo", "."], tree)
            c("cov-ig2-%d-%d" % (k, kk), f + extra + ["foo", "sub", "d"], tree)
    for k, f in enumerate([["foo", "*.txt"], ["foo", "?.txt"], ["foo", "d/*.txt"], ["foo", "d\\*.txt"], ["foo", ".*"], ["foo", ".h*"], ["--hidden", "foo", "*"], ["foo", "*"], ["foo", "*.nomatch"], ["foo", "nope/*.txt"],
                           ["-c", "foo", "*.txt", "d/a.txt"], ["-l", "foo", "d/*"], ["-r", "foo", "d/*"], ["-r", "foo", "*"], ["-r", "--hidden", "foo", "*"], ["-r", "-l", "foo", "d/s/*.txt"], ["foo", "a*", "b*"],
                           ["-H", "foo", "*.t?t"], ["-n", "foo", "?.txt", "?.log"], ["foo", "sub/*"], ["foo", "*/*.txt"], ["foo", "./*.txt"], ["-d", "skip", "foo", "*"], ["-d", "read", "foo", "*"],
                           ["foo", "d\\"], ["foo", "d/"], ["foo", "d//"], ["-r", "foo", "d/"], ["-r", "foo", "./d//"], ["-r", "foo", "d\\s\\"], ["-r", "foo", "."], ["-r", "foo", "./"], ["-r", "foo", "d/.."], ["-r", "foo"]]):
        c("cov-wild-%d" % k, f, tree)
    for k in range(n):
        a = []
        a += rnd.choice([[], [], ["-r"], ["-r", "-l"], ["-r", "-c"], ["-r", "-o"], ["-r", "-n"], ["-r", "--tree", "-l"]])
        a += rnd.choice([[], [], ["--tag"], ["--tag=<,>"], ["--tag=A,B,C"], ["--tag=[,]", "-o"], ["--tag=a,b", "-v"], ["--tag=<<", "-C1"], ["--tag", "-X"], ["--tag=%", "-c"]])
        a += rnd.choice([[], [], ["--color=always", "--colors=" + "".join(rnd.choice(["mt=", "ms=", "mc=", "sl=", "cx=", "fn=", "ln=", "cn=", "bn=", "se=", "ma=", "mn=", "hl"]) + "".join(rnd.choice("hHuUiInfkrgybmcwKRGYBMCW+0123456789;,") for _ in range(rnd.randint(1, 8))) + ":" for _ in range(rnd.randint(1, 4)))]])
        a += rnd.choice([[], [], ["-A1"], ["-B1"], ["-C1"], ["-C2", "-v"], ["-y"], ["-y", "-C1"]])
        a += rnd.choice([[], [], ["-n"], ["-b"], ["-k"], ["-H"], ["-T"], ["--break"], ["-+"], ["-nbkHT"], ["-c"], ["-c", "-v"]])
        a += rnd.choice([[], [], [], ["-j"], ["-i"], ["-w"], ["-x"], ["-F"], ["-F", "-w"], ["-F", "-x"], ["-F", "-i"], ["-U"], ["-a"], ["-I"], ["-X"], ["-W"], ["--hexdump=2"]])
        a += rnd.choice([[], [], ["--min-count=2"], ["-m1"], ["-m2"], ["--max-count=1"], ["--max-line=4"], ["--range=2,6"], ["--files"], ["-q"], ["-l"], ["-L"], ["-0", "-l"], ["--no-messages"], ["--stats"]])
        pat = rnd.choice([["foo"], ["foo"], ["-e", ""], ["-e", "", "-e", "foo"], ["-e", "foo", "-e", ""], [""], [".*"], [".+"], ["^"], ["$"], ["^$"], ["-e", "^$", "-e", "foo"], ["-e", ".*", "--and", "foo"], ["-e", "foo", "--and", ""],
                          ["-e", "", "--not", "foo"], ["--not", "foo"], ["--and", "foo"], ["-e", "foo", "--not", ""], ["-e", "", "--and", ""], ["--bool", ""], ["--bool", "foo -"], ["--bool", "-foo"], ["--bool", "foo|"], ["-f", "pats.txt"],
                          ["-f", "empty.txt"], ["-f", "empty.txt", "-e", "foo"], ["-f", "empty.txt", "-e", ""], ["-f", "pats.txt", "-e", "foo", "--and", "bar"], ["-f", "pats.txt", "--not", "baz"], ["-f", "pats.txt", "-x"], ["-f", "pats.txt", "-w", "-F"],
                          ["-f", "blank.txt"], ["-f", "blank.txt", "-e", "foo"], ["-f", "pats.txt", "-f", "blank.txt"], ["-F", "-f", "pats.txt", "-e", "foo"], ["-f", "crlf.txt"], ["-E", "-e", "a|b"], ["-G", "-e", "a\\|b"], ["-e", "(a|b)+"]])
        if "--files" in a and ("-e" not in pat and "--bool" not in pat):
            continue
        fl = rnd.choice([["t.txt"], ["c.txt"], ["a.bin"], ["m.bin"], ["ml.txt"], ["d"], ["t.txt", "a.txt"], ["e.txt"], ["nonl.txt"], ["crlf.txt", "t.txt"], ["."]])
        c("cov-rand2-%d" % k, a + pat + fl, plus(tree, **{"pats.txt": "foo\nbaz\n", "empty.txt": "", "blank.txt": "\n", "m.bin": {"hex": "666f6f2062617200626172206261720a0a666f6f0a626100666f6f0a6c696e650a"},
                                                       "ml.txt": "foo\nbar\nbaz\n\nfoo bar\nbar\nfoo\nlast\n"}))


def g_fmt3():
    """every field of --format, --format-begin/end/open/close, with and without arguments, widths, -v, -o, -c, several files"""
    import itertools
    se = ["%T", "%S", "%[;]$%S", "%[<]S", "%[ab]T", "%$", "%[x]$%s", "%t", "%s", "%R", "%~", "%m", "%{5}m", "%U", "%[263a]U", "%=", "%[ms]=", "%<", "%[one]<", "%>", "%[many]>",
          "%,", "%:", "%;", "%|", "%%", "%z", "%", "%[", "%[x", "%{3}", "%{x}m", "%[a]$%[b]S%S", "%m%,%m%:"]
    k = 0
    for tok in se:
        for extra in (["a.txt"], ["a.txt", "b.txt"], ["-T", "a.txt", "b.txt"], ["--break", "a.txt", "b.txt"]):
            k += 1
            c("cov-f3-be-%d" % k, ["--format-begin=B" + tok + "|", "--format-end=E" + tok + "|", "--format-open=O" + tok + "|", "--format-close=C" + tok + "|", "--format=%f:%n%~"] + [x for x in extra if x.startswith("-")] + ["foo"] + [x for x in extra if not x.startswith("-")], B)
    fields = ["%f", "%H", "%h", "%N", "%n", "%k", "%K", "%b", "%B", "%d", "%e", "%O", "%o", "%q", "%Q", "%C", "%c", "%a", "%A", "%w", "%W", "%x", "%X", "%y", "%Y", "%z", "%Z", "%u", "%[x]u", "%I", "%i", "%L", "%l",
              "%F", "%G", "%[,]G", "%[x]F", "%[n]H", "%[ ]N", "%[;]k", "%{4}n", "%{4}b", "%{4}k", "%{9}d", "%1", "%2", "%[1]#", "%#", "%[1]#%[2]#", "%[foo]#", "%[bar]#", "%v", "%V", "%+", "%[=]+", "%p", "%P", "%M", "%1$", "%[1,2]#", "%[1]b", "%[1]d",
              "%[1]e", "%[2]o", "%[1]O", "%[1]q", "%[1]Q", "%[1]c", "%[1]C", "%[1]a", "%[1]A", "%[1]x", "%[1]X", "%[1]y", "%[1]Y", "%[1]z", "%[1]Z", "%[1]w", "%[1]W", "%[1]l", "%[1]L", "%[1]i", "%[1]I", "%[1]k", "%[1]K", "%[1]B", "%[1]N", "%[1]n", "%[1]u", "%[1]U"]
    k = 0
    pats = [["(f)(o+)"], ["-F", "foo"], ["-e", "o", "--and", "f"], ["-w", "foo"]]
    for fld in fields:
        for fl in (["-o"], [], ["-v"], ["-c"], ["-H", "-n"]):
            k += 1
            p = pats[k % len(pats)]
            f = ["--format=" + fld + "%~"]
            c("cov-f3-f-%d" % k, f + fl + p + ["a.txt", "b.txt"], plus(B, **{"a.txt": "foo bar foo\nxfoo\nbar\nfoo\tfoo\r\nfoo \u00e9 \x01\x7f\x08\x0c\n"}))
    for i, fmt in enumerate(["%f%~", "%n:%b:%k:%O%~", "%q%~", "%Q%~", "%C%~", "%c%~", "%a%~", "%A%~", "%w%~", "%x%~", "%X%~", "%y%~", "%Y%~", "%z%~", "%Z%~", "%o%~", "%e%~", "%d%~", "%[|]F%~", "%[|]G%~", "%1%2%~"]):
        for j, fl in enumerate([[], ["-o"], ["-v"], ["-C1"], ["-A1", "-v"], ["-B1", "-o"], ["-c"], ["--break", "-H"], ["-b"], ["-y"], ["-m1"], ["-q"], ["-l"], ["-L"], ["--max-count=2", "-v"]]):
            c("cov-f3-g-%d-%d" % (i, j), ["--format=" + fmt] + fl + ["(f)(o+)", "t.txt", "a.txt", "u.txt"], plus(B, **{"t.txt": T, "u.txt": U8, "a.txt": "foo\x00bar foo\nxx\nfoo\n"}))
    for i, fmt in enumerate(["%f%~", "%n%~", "%o%~", "%O%~", "%k%~", "%[;]F%~", "%m%~", "%Z%~"]):
        for j, fl in enumerate([[], ["-v"], ["-o"], ["-C1"], ["-c"], ["-x"], ["--and", "bar"], ["--not", "bar"]]):
            c("cov-f3-h-%d-%d" % (i, j), ["--format=" + fmt] + fl + ["-e", "foo"] + ["c.txt" if False else "t.txt"], plus(B, **{"t.txt": T}))
    for i, fmt in enumerate(["[%f]%~", "<%n>%~", "%H %n %k%~", "{%o}%~", "%m %,%~"]):
        for j, fl in enumerate([["-J1"], ["--sort=size"], ["-r"], ["--stats"], ["-z"], ["--tree"], ["-Z"], ["-0"], ["--null"], ["--hexdump"], ["-X"], ["-W"], ["-y"], ["--color=always"], ["--json"], ["--csv"]]):
            c("cov-f3-i-%d-%d" % (i, j), ["--format=" + fmt] + fl + ["foo", "d"], plus(D2, **{"e.txt": "foo\n"}))


def g_fmt4():
    """format fields that depend on -H/--heading/--break/-n/-k/-b/-T/-Z"""
    flagsets = [["-H", "--heading"], ["-H", "--heading", "--break"], ["-H", "-n", "-k", "-b", "-T"], ["-H", "-Z", "--heading"], ["-H", "-Z"], ["-H", "-n", "-k", "-b"], ["--break", "-H"], ["--no-filename", "-n"],
                ["-H", "-n", "-T", "--heading"], ["-H", "-nkb", "--break", "--heading", "-Z"]]
    toks = ["%+", "%[=]+", "%[-]F", "%F", "%[a]H", "%H", "%[|]$%H", "%[|]$%F", "%[|]$%N", "%[|]$%K", "%[|]$%B", "%[|]$%I", "%[a]I", "%I", "%N", "%K", "%B", "%[x]N", "%[x]K", "%[x]B", "%[x]+", "%p", "%a", "%z", "%i", "%h", "%f",
            "%[|]$%[:]N%[:]K%[:]B", "%{5}N", "%{4}K", "%{6}B", "%+%f", "%[ab]F%a", "%[sep]$%[x]H"]
    for i, tok in enumerate(toks):
        for j, fl in enumerate(flagsets):
            for pat in ((["foo"], "o"), (["-v", "foo"], "v")):
                c("cov-f4-%d-%d-%s" % (i, j, pat[1]), ["--format=" + tok + "%~"] + fl + pat[0] + ["a.txt", "b.txt", "d/s/c.txt"], plus(B, **D2))


def g_hex(n=2500, seed=4242):
    import random
    rnd = random.Random(seed)
    files = {"m.bin": {"hex": "666f6f2062617200626172206261720a0a666f6f0a626100666f6f0a6c696e650a"}, "t.txt": T, "nonl.txt": "foo\nbar foo", "ml.txt": "foo\nbar\nbaz\n\nfoo bar\nbar\nfoo\nlast\n",
             "w.txt": "foo " * 30 + "\nbar\n" + "foo bar baz qux " * 20 + "\nfoo\n"}
    for k in range(n):
        a = []
        a += rnd.choice([["-X"], ["--hexdump"], ["--hexdump=1"], ["--hexdump=4b"], ["--hexdump=2a"], ["-W"], ["--hexdump=8ahc"], ["--hexdump=1c"], ["-y"], ["--csv"], ["--json"], ["--xml"], ["--format=%o%~"], ["-o"], ["-c"], [], []])
        a += rnd.choice([[], [], ["-v"], ["-A1"], ["-B1"], ["-C1"], ["-C2", "-v"], ["-A2", "-v"], ["-B2", "-v"], ["-y", "-v"], ["-A1", "-B2"]])
        a += rnd.choice([[], [], ["-o"], ["-ob"], ["-ok"], ["-on"], ["-onkb"], ["-n"], ["-b"], ["-k"], ["-T"], ["-nbk", "-T"], ["-H"]])
        a += rnd.choice([[], ["--color=always"], ["--color=always", "--colors=mt=1;31:ms=4:cx=2:sl=3:se=7:bn=34"], ["--color=always", "-v"], ["--color=always", "--colors=rv,sl=1:cx=2"], ["--color=always", "--colors=ne"], ["--color=always", "--colors=hl,sl=1;4"]])
        a += rnd.choice([[], [], [], ["-a"], ["-I"], ["-U"], ["--binary-files=without-match"], ["--binary-files=hex"], ["--binary-files=text"], ["--max-line=8"], ["-m1"], ["-m2", "-v"], ["--break"], ["--heading"], ["--tag"]])
        a += rnd.choice([[], [], ["-j"], ["-w"], ["-x"], ["-i"], ["-F"], ["-z"]])
        pat = rnd.choice([["foo"], ["foo"], ["o"], ["bar|foo"], ["^"], ["^$"], [""], ["a"], ["foo$"], ["-e", "foo", "--and", "bar"], ["-e", "foo", "--not", "bar"], ["x"]])
        fl = rnd.choice([["m.bin"], ["m.bin", "t.txt"], ["t.txt"], ["nonl.txt"], ["ml.txt"], ["w.txt"], ["w.txt", "m.bin"], ["m.bin", "nonl.txt", "ml.txt"]])
        c("cov-hex-%d" % k, a + pat + fl, files)


def _sorted_variant(fn, tag, *a, **kw):
    """run a random group with another seed; ids renamed, --sort=name added so that the output order is deterministic"""
    s = len(gen_cases.cases)
    fn(*a, **kw)
    for d in gen_cases.cases[s:]:
        d["id"] = d["id"].replace("cov-", "cov-" + tag + "-", 1)
        if not any(x.startswith("--sort") or x == "-J1" for x in d["args"]):
            d["args"] = ["--sort=name"] + d["args"]


def g_rand3():
    _sorted_variant(g_rand, "s1", 6000, 31337)
    _sorted_variant(g_rand2, "s2", 4000, 9191)
    _sorted_variant(g_hex, "s3", 2500, 777)


def g_quiet6():
    _sorted_variant(g_rand, "q6a", 1500, 60601)
    _sorted_variant(g_rand2, "q6b", 1000, 60602)
    _sorted_variant(g_hex, "q6c", 600, 60603)


def g_quiet7():
    _sorted_variant(g_rand, "q7a", 1500, 70701)
    _sorted_variant(g_rand2, "q7b", 1000, 70702)
    _sorted_variant(g_hex, "q7c", 600, 70703)


def g_quiet8():
    _sorted_variant(g_rand, "q8a", 1500, 80801)
    _sorted_variant(g_rand2, "q8b", 1000, 80802)
    _sorted_variant(g_hex, "q8c", 600, 80803)


def g_quiet9():
    _sorted_variant(g_rand, "q9a", 1500, 90901)
    _sorted_variant(g_rand2, "q9b", 1000, 90902)
    _sorted_variant(g_hex, "q9c", 600, 90903)


def g_quiet10():
    _sorted_variant(g_rand, "qaa", 1500, 101001)
    _sorted_variant(g_rand2, "qab", 1000, 101002)
    _sorted_variant(g_hex, "qac", 600, 101003)


def g_quiet11():
    _sorted_variant(g_rand, "qba", 1500, 111101)
    _sorted_variant(g_rand2, "qbb", 1000, 111102)
    _sorted_variant(g_hex, "qbc", 600, 111103)


GROUPS = {"quiet11": g_quiet11, "quiet9": g_quiet9, "quiet10": g_quiet10, "quiet8": g_quiet8, "quiet6": g_quiet6, "quiet7": g_quiet7, "rand3": g_rand3, "hex": g_hex, "fmt4": g_fmt4, "fmt3": g_fmt3, "rand2": g_rand2, "rand": g_rand, "ctx": g_ctx, "fmt": g_fmt, "opts": g_opts, "tree": g_tree, "enc": g_enc, "config": g_config, "big": g_big}

if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    for name in sys.argv[1:]:
        GROUPS[name]()
    seen = set()
    for d in gen_cases.cases[start:]:
        assert d["id"] not in seen, d["id"]
        seen.add(d["id"])
        print(json.dumps(d, ensure_ascii=True, separators=(",", ":")))
