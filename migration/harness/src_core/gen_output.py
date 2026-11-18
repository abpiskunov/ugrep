#!/usr/bin/env python3
"""Surface cases for the Output of src_core (src/output.cpp, output.hpp): plain, colour/ANSI, hex dump, JSON, XML, CSV,
C++ quoting, --format fields and %[...] arguments, --replace, -o/-b/-k/-n/-H/--heading/--tree, context and separators,
binary handling, CRLF input, --width, --line-buffered, ordered output with -J > 1 and cancel (-m, -q, -l).
Inputs only, never expected values:
   gen_output.py <ugrep.exe> <out-dir> [seed]
It writes <out-dir>/cand.cases.jsonl, runs the C++ 3x, keeps a candidate only when its observation (stdout, stderr,
exit, files left behind) differs from every recorded case and from every earlier candidate, and writes
<out-dir>/admitted.cases.jsonl and <out-dir>/admitted.trace.jsonl (the recorded C++ observations)."""
import itertools
import json
import os
import random
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
TOOLS = ["cargo", "run", "-q", "--release", "--locked", "--manifest-path",
         os.path.join(ROOT, "migration", "tools", "cpp2rust-tools", "Cargo.toml"), "--"]
exe, out = sys.argv[1], sys.argv[2]
seed = int(sys.argv[3]) if len(sys.argv) > 3 else 0
rnd = random.Random(seed)
os.makedirs(out, exist_ok=True)

TXT = ("foo bar\nbar foo foo\nplain line\n\tfoo tab\nFoo Bar\nfoobar foo\nlast foo\n"
       "ctx1\nctx2\nctx3\nfoo again\nctx4\nctx5\nctx6\nctx7\nfoo end\nz\n")
UNI = "caf\u00e9 foo na\u00efve\n\u65e5\u672c\u8a9e foo \u00e9\u00e8\nplain foo\n"
CRLF = "foo one\r\nbar two\r\nfoo three\r\nlast\r\n"
SPECIAL = 'say "foo" & <b>foo</b> \\ \'x\'\ttab foo\nfoo,comma;semi|pipe\nplain foo\n\x01ctl foo\n'
LONG = ("x" * 60 + " foo " + "y" * 120 + " foo " + "z" * 80) + "\nshort foo\n"
BIN = {"hex": "666f6f00626172200a666f6f2062617a0a00ff666f6f0a"}
BIN2 = {"hex": "612066 6f6f 0a ff fe 00 66 6f 6f 0a".replace(" ", "")}
FILES = {"t.txt": TXT, "u.txt": UNI, "crlf.txt": CRLF, "s.txt": SPECIAL, "long.txt": LONG, "b.bin": BIN, "b2.bin": BIN2}
TREE = {"d/a.txt": "foo a\n", "d/b.txt": "bar\nfoo b\n", "d/s/c.txt": "foo c\nfoo c2\n", "d/s/t/e.txt": "foo e\n",
        "d/s/n.txt": "none\n", "e/f.txt": "foo f\n", "g.txt": "foo g\n"}
MANY = {"m/f%02d.txt" % i: "".join("line %d foo %d\nother\n" % (j, i) for j in range(1 + i % 4)) for i in range(14)}
cases = []
seen_ids = set()
forced = set()  # admitted even when an earlier case observed the same output (thread counts, --replace %n)


def c(id, args, files=None, stdin=None, env=None, force=False):
    id = "out-" + id
    if id in seen_ids:
        return
    seen_ids.add(id)
    if force:
        forced.add(id)
    d = {"id": id, "args": args}
    if files and len(files) > 1 and not any(a.startswith(("-J", "--sort")) for a in args):
        d["args"] = ["-J1"] + args  # unordered multi-file output is thread timing; -J1 keeps the case deterministic
    if files:
        d["files"] = files
    if stdin is not None:
        d["stdin"] = stdin
    if env:
        d["env"] = env
    cases.append(d)


def san(s):
    return "".join(ch if ch.isalnum() else "_" for ch in s).strip("_")[:60]


F1 = {k: FILES[k] for k in ("t.txt",)}
F2 = {k: FILES[k] for k in ("t.txt", "u.txt")}
F3 = {k: FILES[k] for k in ("t.txt", "s.txt", "crlf.txt")}
FB = {k: FILES[k] for k in ("t.txt", "b.bin", "b2.bin")}

# the Output formats × their option sets × inputs
FORMATS = {"plain": [], "color": ["--color=always"], "hex": ["-X"], "hexc": ["-X", "--color=always"], "hexw": ["-W"],
           "json": ["--json"], "xml": ["--xml"], "csv": ["--csv"], "cpp": ["--cpp"]}
EXTRA = [[], ["-n"], ["-H"], ["-nH"], ["-b"], ["-k"], ["-nbkH"], ["-o"], ["-on"], ["-onbk"], ["-c"], ["-l"], ["-L"],
         ["-v"], ["-vn"], ["-C1"], ["-A1"], ["-B2"], ["-C2", "-n"], ["-C1", "-nH", "--color=always"], ["-T"], ["-T", "-nk"],
         ["-m1"], ["-m2", "-n"], ["-i"], ["-w"], ["-x"], ["-y"], ["-u"], ["--ungroup"], ["-N", "-n"], ["-h"],
         ["--line-buffered"], ["--width=20"], ["--width=40", "-n"], ["--tabs=4", "-T"], ["-0", "-l"], ["--null", "-c"],
         ["-z"], ["-U"], ["-a"], ["-I"], ["--hex"], ["--no-hex"], ["-q"], ["-s"], ["-A2", "-B1", "-nb"], ["--separator=="],
         ["--group-separator=XX", "-C0"], ["--no-group-separator", "-C1"], ["-oc"], ["-ob"], ["-ok"], ["-ov"]]
INPUTS = {"t": (F1, ["foo"]), "t2": (F2, ["foo"]), "t3": (F3, ["foo"]), "bin": (FB, ["foo"]), "re": (F1, ["fo*|bar"]),
          "z": (F1, ["zzzz"]), "uni": (F2, ["\u00e9|foo"])}
for (fn, fa), (ex), (iname, (files, pat)) in itertools.product(FORMATS.items(), range(len(EXTRA)), INPUTS.items()):
    # the full cross-product is too large; a seeded selection keeps every format, option set and input represented
    if rnd.random() > 0.17:
        continue
    e = EXTRA[ex]
    args = fa + e + ["-e", pat[0]] + sorted(files)
    c("m-%s-%s-%s-%d" % (fn, iname, san("".join(e)) or "none", ex), args, files)
# guarantee coverage of every format with every input at least once
for (fn, fa), (iname, (files, pat)) in itertools.product(FORMATS.items(), INPUTS.items()):
    c("m0-%s-%s" % (fn, iname), fa + ["-n", "-e", pat[0]] + sorted(files), files)
    c("m1-%s-%s" % (fn, iname), fa + ["-nbkH", "-C1", "-e", pat[0]] + sorted(files), files)

# --format fields and %[...] arguments
FIELDS = ["%%", "%~", "%a", "%A", "%b", "%B", "%[<]B", "%c", "%C", "%d", "%e", "%f", "%F", "%[>>]F", "%h", "%H", "%[#]H",
          "%i", "%I", "%[<>]I", "%j", "%J", "%k", "%K", "%[+]K", "%l", "%L", "%m", "%M", "%n", "%N", "%[-]N", "%o", "%O",
          "%p", "%q", "%Q", "%R", "%s", "%S", "%[;]S", "%t", "%T", "%[<>]T", "%u", "%[41]U", "%[1F600]U", "%v", "%V", "%w",
          "%x", "%X", "%y", "%Y", "%z", "%Z", "%,", "%:", "%;", "%|", "%[,]>", "%[,]<", "%[:]$", "%$", "%[1;31m]=ab%=",
          "%{5}n", "%{-5}n", "%{8}b", "%{3}k", "%0", "%1", "%g", "%G", "%[x|y]g", "%[x|y]G", "%[1]#", "%[0]b", "%[2]q",
          "%[1|2]v", "%[1]x", "%[1]y", "%[1]j", "%[1]d", "%[1]e", "%[a]#", "%[a|b]#", "%zz", "%", "%[", "%[a", "%{", "%{3"]
for i, fld in enumerate(FIELDS):
    for tag, extra in (("", []), ("o", ["-o"]), ("n", ["-nHbk"]), ("v", ["-v"]), ("c", ["-c"]), ("l", ["-l"]), ("X", ["-X"]),
                       ("W", ["-W"])):
        if tag in ("c", "l", "X", "W") and rnd.random() > 0.4:
            continue
        c("f-%d-%s-%s" % (i, tag, san(fld)), ["--format=[" + fld + "]%~"] + extra + ["foo"] + sorted(F3), F3)
for i, fld in enumerate(FIELDS):
    if rnd.random() > 0.5:
        continue
    c("r-%d-%s" % (i, san(fld)), ["--replace=<" + fld + ">", "foo", "t.txt"], F1)
    c("rn-%d-%s" % (i, san(fld)), ["--replace=<" + fld + ">", "-nH", "-e", "fo*|bar", "t.txt", "u.txt"], F2)
for i, fmt in enumerate(["%f:%n:%O%~", "%H%N%K%B%T%O%~", "%[,]>%m%,%o%~", "%F%[ ]N%[ ]K%[ ]B%S%O%~", "%+%n%s%O%~", "%[%S]o|%~",
                         "%[ms]=%o%=%~", "%u|%O%~", "%J%~", "%V%~", "%X%~", "%Y%~", "%C%~", "%c,%j,%v,%x,%y,%q%~", "%o%R%~",
                         "%n%s%[|]>%k%~", "%[1;32m]=%f%=%s%O%~", "no fields", "", "%~%~", "%%n %%%n%~", "%m/%M/%L/%l%~"]):
    for tag, extra in (("", []), ("o", ["-o"]), ("v", ["-v"]), ("cx", ["-C1"]), ("br", ["--break"]), ("hd", ["--heading"]),
                       ("ml", ["-e", "foo.*\\n.*bar"]), ("col", ["--color=always"]), ("j", ["-J3"]), ("z", ["-z"]),
                       ("bin", ["-a"])):
        files = FB if tag in ("bin",) else F3
        c("ff-%d-%s" % (i, tag), ["--format=" + fmt] + extra + ["foo"] + sorted(files), files)
        c("rf-%d-%s" % (i, tag), ["--replace=" + fmt] + extra + ["foo"] + sorted(files), files)

# multi-line matches, -o with context, lines
ML = {"ml.txt": "start foo\nbar end\nmid\nfoo\n\nbar\nfoo bar\nbar\n"}
for i, fa in enumerate([[], ["--color=always"], ["--json"], ["--xml"], ["--csv"], ["--cpp"], ["-X"], ["-W"], ["-o"], ["-c"],
                         ["-n"], ["-nb"], ["-nk"], ["--format=%n:%l:%L:%O%~"], ["--replace=[%o]"], ["-C1"], ["-v"], ["-z"]]):
    for j, pat in enumerate(["foo\\nbar", "foo\\n", "\\nbar", "foo.*\\n.*bar", "^$", "\\n\\n"]):
        c("ml-%d-%d" % (i, j), fa + ["-U", "-e", pat, "ml.txt"], ML) if j % 2 else c("ml-%d-%d" % (i, j), fa + ["-e", pat, "ml.txt"], ML)

# colours
COLS = ["ms=1;31", "mc=4", "sl=2", "cx=3", "fn=36", "ln=33", "cn=34", "bn=35", "se=37", "mt=1;4", "rv", "ne", "hl",
        "ms=1;31:ne", "sl=7:cx=7:rv", "fn=32:ln=33:se=1:ms=0", "ms=:mc=", "bn=:cn=:ln=:fn=:se=", "ms=38;5;200:ln=48;2;1;2;3",
        "ms=1;31:ms=1;32", "mt=1;32", "hl:fn=1", "ne:rv:ms=4"]
for i, col in enumerate(COLS):
    for tag, extra in (("p", ["-nH"]), ("c", ["-C1", "-nbkH"]), ("v", ["-v", "-n"]), ("o", ["-o", "-nb"]), ("x", ["-X"]),
                       ("h", ["--heading", "-nr"]), ("t", ["-T", "-nk"]), ("w", ["--width=30", "-n"]), ("l", ["-l"]),
                       ("c2", ["-c"]), ("bin", ["-a"]), ("pr", ["--pretty"])):
        files = FB if tag in ("bin",) else F2
        c("col-%d-%s" % (i, tag), ["--color=always", "--colors=" + col] + extra + ["foo"] + sorted(files), files)
for i, ev in enumerate([{"GREP_COLORS": "ms=1;34:ln=31"}, {"GREP_COLOR": "1;32"}, {"UGREP_COLORS": "mt=1;33:fn=35"},
                        {"GREP_COLORS": "mt=7:ne", "NO_COLOR": "1"}, {"NO_COLOR": "1"}, {"TERM": "dumb"}]):
    for tag, extra in (("n", ["-nH"]), ("c", ["-C1", "-n"]), ("p", ["--pretty"])):
        c("cenv-%d-%s" % (i, tag), ["--color=always"] + extra + ["foo", "t.txt", "u.txt"], F2, env=ev)
        c("cenva-%d-%s" % (i, tag), ["--color=auto"] + extra + ["foo", "t.txt", "u.txt"], F2, env=ev)

# hex dump variants
HEX = ["-X", "-W", "--hexdump", "--hexdump=1a", "--hexdump=2a", "--hexdump=4aC", "--hexdump=8B", "--hexdump=1ab1", "--hexdump=3c", "--hexdump=2bA",
       "--hexdump=1h", "--hexdump=4ch", "--hexdump=2aC1", "--hexdump=1aB2", "--hexdump=1aA2C3", "--hexdump=1cbA1", "--hexdump=6"]
BIGBIN = {"hex": "".join("%02x" % ((i * 7) % 256) for i in range(300)) + "666f6f" + "00" * 20 + "666f6f" + "ff" * 40}
HB = {"big.bin": BIGBIN, "t.txt": TXT, "e.txt": ""}
for i, hx in enumerate(HEX):
    for tag, extra in (("", []), ("n", ["-n"]), ("c1", ["-C1"]), ("v", ["-v"]), ("col", ["--color=always"]),
                       ("o", ["-o"]), ("nbk", ["-nbkH"]), ("m1", ["-m1"]), ("z", ["-z"]), ("w", ["--width=30"])):
        c("hx-%d-%s" % (i, tag), [hx] + extra + ["-e", "foo", "big.bin", "t.txt"], HB)
    c("hxe-%d" % i, [hx, "-e", "", "e.txt"], HB)
    c("hxs-%d" % i, [hx, "-e", "foo"], stdin="foo bar\n\x00foo\n")
for i, bf in enumerate(["binary", "text", "hex", "with-matches", "without-match", "bogus"]):
    for tag, extra in (("", []), ("n", ["-nH"]), ("c", ["-c"]), ("l", ["-l"]), ("o", ["-o"]), ("col", ["--color=always"]),
                       ("j", ["--json"]), ("x", ["--xml"]), ("v", ["--csv"]), ("f", ["--format=%f:%O%~"]), ("q", ["-q"]),
                       ("C", ["-C1", "-n"]), ("r", ["-r", "--sort=name"]), ("I", ["-I"]), ("a", ["-a"])):
        c("bf-%d-%s" % (i, tag), ["--binary-files=" + bf] + extra + ["foo", "b.bin", "b2.bin", "t.txt"],
          {"b.bin": BIN, "b2.bin": BIN2, "t.txt": TXT})

# headings, trees, break, files, separators
for i, ex in enumerate([["--heading"], ["--heading", "-n"], ["--heading", "--break"], ["--break"], ["--break", "-n"], ["-+"],
                        ["--heading", "--color=always"], ["--break", "--color=always", "-nH"], ["--heading", "-c"], ["--heading", "-o"],
                        ["--heading", "-C1"], ["--break", "-C1", "-n"], ["--heading", "--break", "-C1", "-nb", "--color=always"],
                        ["--heading", "-X"], ["--break", "-X"], ["--heading", "--json"], ["--break", "--csv"], ["--heading", "--xml"],
                        ["--pretty"], ["--pretty", "-C1"], ["--pretty", "-X"], ["--tree", "-l"], ["--tree", "-c"], ["--tree", "-L"],
                        ["--tree", "-l", "--color=always"], ["--tree", "-c", "--color=always"], ["--tree", "-l", "-0"],
                        ["--tree", "-l", "--ascii"] , ["--tree", "-l", "-v"], ["--tree", "-l", "--files-with-matches", "--null"],
                        ["-l", "--color=always"], ["-c", "--color=always"], ["-L", "--color=always"], ["-l", "--null"],
                        ["-c", "-H"], ["-c", "--null"], ["-c", "-v"], ["-c", "--format=%f:%m%~"], ["--files-with-matches", "-n"],
                        ["-l", "--json"], ["-c", "--json"], ["-l", "--xml"], ["-c", "--xml"], ["-l", "--csv"], ["-c", "--csv"],
                        ["-l", "--cpp"], ["-c", "--cpp"], ["-lc"], ["-Lc"]]):
    for k, sortarg in enumerate(["--sort=name", "--sort=size"]):
        c("hd-%d-%d" % (i, k), ex + [sortarg, "-r", "foo", "."], TREE)
        c("hdd-%d-%d" % (i, k), ex + [sortarg, "-r", "foo", "d", "g.txt"], TREE)
for i, sep in enumerate(["--separator=@", "--separator=", "--separator=::", "--group-separator=##", "--group-separator=",
                         "--no-group-separator", "--group-separator=%x", "--tabs=2", "--tabs=3", "--tabs=8"]):
    for tag, extra in (("", ["-C1", "-nH"]), ("t", ["-T", "-nbkH"]), ("c", ["-C1", "-nb", "--color=always"]), ("o", ["-o", "-nb"]),
                       ("v", ["-v", "-A1", "-n"]), ("x", ["--xml", "-C1"]), ("j", ["--json", "-C1"]), ("s", ["--csv", "-C1"])):
        c("sp-%d-%s" % (i, tag), [sep] + extra + ["foo", "t.txt", "u.txt"], F2)

# context
for a in (0, 1, 2, 3, 20):
    for b in (0, 1, 3):
        for tag, extra in (("", ["-n"]), ("v", ["-vn"]), ("h", ["-nH", "--color=always"]), ("m", ["-m2", "-n"]),
                           ("o", ["-on"]), ("br", ["--break", "--heading", "-n"]), ("j", ["--json"]), ("c", ["--csv"]),
                           ("u", ["-u", "-n"]), ("ug", ["--ungroup", "-n"]), ("bi", ["-nb", "-k"]), ("x", ["-X"])):
            c("cx-%d-%d-%s" % (a, b, tag), ["-A%d" % a, "-B%d" % b] + extra + ["foo", "t.txt", "u.txt"], F2)
for n in (1, 2, 5):
    c("cxc-%d" % n, ["-C%d" % n, "foo"], stdin=TXT)
    c("cxy-%d" % n, ["-y", "-C%d" % n, "-n", "foo", "t.txt"], F1)
    c("cxn-%d" % n, ["-%d" % n, "-n", "foo", "t.txt"], F1)
    c("cxmc-%d" % n, ["-m%d" % n, "-C2", "-nH", "foo", "t.txt", "u.txt"], F2)

# CRLF input and line ending rules
for i, fa in enumerate([[], ["-n"], ["-o"], ["-c"], ["--json"], ["--xml"], ["--csv"], ["--cpp"], ["-X"], ["--color=always"],
                        ["--format=%O|%o|%Q|%q|%~"], ["--format=%J%V%X%Y%C"], ["--replace=<%o>"], ["-v"], ["-C1"], ["-nb"], ["-ok"],
                        ["-U"], ["-z"], ["--tabs=4", "-T"]]):
    for pat in ("foo", "foo.*", "^foo", "three$", "^$", "\\r"):
        c("crlf-%d-%s" % (i, san(pat) or "e"), fa + ["-e", pat, "crlf.txt"], {"crlf.txt": CRLF}) if pat != "\\r" else \
            c("crlf-%d-r" % i, fa + ["-U", "-e", "\\r", "crlf.txt"], {"crlf.txt": CRLF})
c("crlf-stdin", ["-n", "foo"], stdin=CRLF)
c("crlf-nonl", ["-n", "foo"], stdin="foo\r\nbar foo")
c("nonl-json", ["--json", "foo"], stdin="foo")
c("nonl-xml", ["--xml", "foo"], stdin="foo")
c("nonl-csv", ["--csv", "foo"], stdin="foo")
c("nonl-hex", ["-X", "foo"], stdin="foo")
c("nonl-format", ["--format=%O|%~", "foo"], stdin="foo")
c("nonl-plain", ["-n", "foo"], stdin="foo")
c("nonl-color", ["--color=always", "-n", "foo"], stdin="foo")

# --width, --line-buffered, long lines, max-line
for w in (0, 1, 5, 20, 40, 80, 200):
    for tag, extra in (("", []), ("n", ["-n"]), ("c", ["--color=always"]), ("o", ["-o"]), ("cx", ["-C1", "-n"]), ("j", ["--json"]),
                       ("x", ["--xml"]), ("s", ["--csv"]), ("hx", ["-X"]), ("f", ["--format=%O%~"]), ("v", ["-v", "-n"]),
                       ("u", ["-nH"]), ("t", ["-T", "-nk"]), ("k", ["-k", "-nb"])):
        c("w-%d-%s" % (w, tag), ["--width=%d" % w] + extra + ["foo", "long.txt", "u.txt"], {"long.txt": LONG, "u.txt": UNI})
for tag, extra in (("", []), ("n", ["-nH"]), ("c", ["--color=always", "-C1"]), ("j", ["--json"]), ("x", ["--xml"]), ("s", ["--csv"]),
                   ("hx", ["-X"]), ("f", ["--format=%f:%O%~"]), ("o", ["-o"]), ("v", ["-v"]), ("J", ["-J4", "--sort=name", "-r"])):
    c("lb-" + tag, ["--line-buffered"] + extra + ["foo", "." if tag == "J" else "t.txt"], dict(MANY, **F1) if tag == "J" else F1)
for ml in (1, 10, 50, 100000):
    for tag, extra in (("", []), ("o", ["-o"]), ("c", ["--color=always", "-n"]), ("j", ["--json"]), ("v", ["-v"])):
        c("ml-max-%d-%s" % (ml, tag), ["--max-line=%d" % ml] + extra + ["foo", "long.txt"], {"long.txt": LONG})

# ordered output, threads, cancel and stop
for j in (1, 2, 3, 4, 8, 16):
    for tag, extra in (("", []), ("n", ["-nH"]), ("c", ["-c"]), ("l", ["-l"]), ("L", ["-L"]), ("co", ["--color=always", "-nH"]),
                       ("hd", ["--heading", "--break", "-n"]), ("j", ["--json"]), ("x", ["--xml"]), ("s", ["--csv"]),
                       ("hx", ["-X"]), ("f", ["--format=%f:%n:%O%~"]), ("cx", ["-C1", "-nH"]), ("v", ["-v"]), ("o", ["-o", "-nH"]),
                       ("m1", ["-m1"]), ("m2", ["-m2", "-nH"]), ("q", ["-q"]), ("ql", ["-q", "-l"]), ("fm", ["--max-files=3"]),
                       ("lm", ["-l", "--max-files=2"]), ("cm", ["-c", "-m1"]), ("tr", ["--tree", "-l"]), ("tc", ["--tree", "-c"]),
                       ("lb", ["--line-buffered", "-n"]), ("nul", ["-l", "-0"]), ("u", ["-u", "-o"]), ("z", ["-z", "-n"]),
                       ("bin", ["-a", "-n"]), ("pr", ["--pretty"]), ("ug", ["--ungroup", "-nH"])):
        files = dict(MANY)
        files["m/bin.dat"] = BIN
        c("th-%d-%s" % (j, tag), ["-J%d" % j, "--sort=name", "-r"] + extra + ["foo", "m"], files, force=j > 1)
for tag, extra in (("m1", ["-m1"]), ("m2", ["-m2"]), ("m3n", ["-m3", "-nH"]), ("m1c", ["-m1", "-C1", "-n"]), ("m1v", ["-m1", "-v"]),
                   ("m1o", ["-m1", "-o"]), ("m1j", ["-m1", "--json"]), ("m1x", ["-m1", "--xml"]), ("m1s", ["-m1", "--csv"]),
                   ("m1hx", ["-m1", "-X"]), ("m1f", ["-m1", "--format=%f:%O%~"]), ("m1co", ["-m1", "--color=always", "-nH"]),
                   ("q", ["-q"]), ("qs", ["-q", "-s"]), ("ql", ["-q", "-l"]), ("qv", ["-q", "-v"]), ("qc", ["-q", "-c"]),
                   ("qn", ["-q", "nofile"]), ("qnf", ["-q", "-s", "nofile.txt"]), ("lq", ["-l", "-m1"]), ("mc", ["-c", "-m2"]),
                   ("mz", ["-m0"]), ("m1b", ["-m1", "-a", "--binary-files=text"]), ("m1u", ["-m1", "-u", "-o"]),
                   ("m1z", ["-m1", "-z"]), ("mf1", ["--max-files=1"]), ("mf2", ["--max-files=2", "-c"]), ("mf1l", ["--max-files=1", "-l"])):
    c("st-" + tag, extra + ["foo", "t.txt", "u.txt", "nofile.txt"], F2)
    c("stj-" + tag, ["-J3", "--sort=name"] + extra + ["foo", "m", "t.txt"], dict(MANY, **F1))
c("st-stdin-m1", ["-m1", "-n", "foo"], stdin=TXT)
c("st-stdin-q", ["-q", "foo"], stdin=TXT)
c("st-stdin-q-nomatch", ["-q", "zzz"], stdin=TXT)
c("st-q-err-first", ["-q", "foo", "nofile.txt", "t.txt"], F1)
c("st-q-err-last", ["-q", "foo", "t.txt", "nofile.txt"], F1)
c("st-m1-J4-bin", ["-m1", "-J4", "--sort=name", "-r", "foo", "m"], dict(MANY, **{"m/b.bin": BIN}))

# %n under --replace and --format, with and without -n, -v, -o, -k, -b, --max-line
RN = {"t.txt": "foo bar\nplain\nplain2\nfoo\nx\nfoo x\n"}
for tag, extra in (("replace-n-plain", ["--replace=<%n>"]), ("replace-n-with-n", ["-n", "--replace=<%n>"]),
                   ("replace-n-maxline", ["--max-line=100000", "--replace=<%n>"]), ("replace-n-only", ["-o", "--replace=<%n>"]),
                   ("replace-nk", ["-k", "--replace=<%n>"]), ("replace-nb", ["-b", "--replace=<%n>"]),
                   ("format-n-plain", ["--format=%n,%l,%k,%b%~"]), ("format-n-invert", ["-v", "--format=%n,%l,%k,%b%~"]),
                   ("replace-invert", ["-v", "--replace=<%n>"])):
    c("rn2-" + tag, extra + ["foo", "t.txt"], dict(RN), force=True)

# JSON / XML / CSV / C++ escapes
SP = {"s.txt": SPECIAL, "u.txt": UNI, "bin.dat": BIN}
for fa in ("--json", "--xml", "--csv", "--cpp"):
    for i, pat in enumerate(["foo", "\"foo\"", "<b>", "&", "\\\\", "'x'", "foo,comma", "\\x01", "caf\u00e9", "\u65e5\u672c", "semi|pipe", "."]):
        for tag, extra in (("", []), ("o", ["-o"]), ("n", ["-nbkH"]), ("c", ["-C1", "-n"]), ("v", ["-v"]), ("l", ["-l"]),
                           ("m", ["-m1"]), ("a", ["-a"]), ("T", ["-T"])):
            if rnd.random() > 0.5:
                continue
            c("esc-%s-%d-%s" % (fa[2:], i, tag), [fa] + extra + ["-e", pat, "s.txt", "u.txt"], SP)
    for tag, extra in (("e", ["-e", "foo", "empty.txt"]), ("e2", ["-e", "", "empty.txt"]), ("nf", ["foo", "nofile.txt"]),
                       ("dir", ["-r", "foo", "d"]), ("name", ["-H", "foo", "we ird,name&.txt"]), ("stdin", ["-H", "foo", "-"]),
                       ("lab", ["-H", "--label=lab\"<&>", "foo"])):
        c("esc-%s-0-%s" % (fa[2:], tag), [fa] + extra, {"empty.txt": "", "d/a.txt": "foo\n", "we ird,name&.txt": "foo\n"}
          if tag != "stdin" and tag != "lab" else None, stdin="foo\n" if tag in ("stdin", "lab") else None)
for i, label in enumerate(["x", "a b", "a\"b", "<&>", "a,b", "\\", "caf\u00e9", ""]):
    for fa in ("--json", "--xml", "--csv", "--cpp", "--format=%f|%h|%i|%F|%H|%I%~", "-H", "--heading"):
        c("lab-%s-%d" % (san(fa), i), [fa, "-H", "--label=" + label, "foo"], stdin="foo bar\n")

# ugrep -o with -z, -U, null data and edit distance
for tag, extra in (("z", ["-z"]), ("zo", ["-z", "-o"]), ("zn", ["-z", "-n", "-b"]), ("zj", ["-z", "--json"]), ("zx", ["-z", "--xml"]),
                   ("zc", ["-z", "--csv"]), ("zh", ["-z", "-X"]), ("zf", ["-z", "--format=%n:%O%~"]), ("zv", ["-z", "-v"]),
                   ("Z", ["-Z1"]), ("Zo", ["-Z1", "-o"]), ("Zf", ["-Z1", "--format=%Z %o%~"]), ("Zc", ["-Z1", "--color=always"]),
                   ("Zj", ["-Z1", "--json"]), ("Zx", ["-Z1", "--xml"]), ("Zs", ["-Z1", "--csv"]), ("Zn", ["-Z1", "-n"]),
                   ("Z2", ["-Z2", "-on"]), ("Zb", ["-Z1", "-nbk", "-H"]), ("best", ["-Zb", "-o"]), ("bestn", ["-Zb", "-n"]),
                   ("zcol", ["-z", "--color=always", "-n"]), ("zcx", ["-z", "-C1"])):
    c("z-" + tag, extra + ["-e", "foo", "z.dat"], {"z.dat": {"hex": "666f6f00626172006661720066 6f6f00666f6f".replace(" ", "")}})
    c("zt-" + tag, extra + ["-e", "foo", "t.txt"], F1)

# multiple patterns, --and, --not, -f, tree of output sections
for i, ex in enumerate([["-e", "foo", "-e", "bar"], ["-e", "foo", "--and", "bar"], ["-e", "foo", "--not", "bar"], ["--bool", "foo bar|plain"],
                        ["-F", "-e", "foo", "-e", "Bar"], ["-w", "foo"], ["-x", "foo.*"], ["-i", "FOO"], ["-G", "fo\\+"], ["-E", "fo+|ba?r"]]):
    for tag, fa in (("", []), ("o", ["-o"]), ("n", ["-nk"]), ("c", ["--color=always", "-n"]), ("j", ["--json"]), ("x", ["--xml"]),
                    ("s", ["--csv"]), ("hx", ["-X"]), ("f", ["--format=%o|%O%~"]), ("cx", ["-C1", "-n"]), ("v", ["-v"]), ("b", ["-onb"])):
        c("pat-%d-%s" % (i, tag), fa + ex + ["t.txt"], F1)


# run, admit on a new observation, write the admitted cases and their recorded trace
def run(path_cases, path_trace, repeat):
    r = subprocess.run(TOOLS + ["run-cases", "--bin", exe, "--impl", "cpp", "--repeat", str(repeat), "--cases", path_cases,
                                "--env-matrix", os.path.join(ROOT, "migration", "env-matrix.json"), "--out", path_trace,
                                "--timeout", "60"], cwd=ROOT, capture_output=True, text=True)
    return r.returncode, (r.stdout + r.stderr).strip()[-400:]


def obs(path):
    d = {}
    for line in open(path, encoding="utf-8"):
        e = json.loads(line)
        cid = e["case"].split("@")[0]
        if e["kind"] in ("stdout", "stderr", "exit", "fs"):
            d.setdefault(cid, {})[e["kind"]] = json.dumps(e.get("ret", e.get("args")), sort_keys=True)
    return d


def write(path, items):
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        for v in items:
            f.write(json.dumps(v, ensure_ascii=True, separators=(",", ":")) + "\n")


recorded = {json.loads(l)["id"] for l in open(os.path.join(ROOT, "migration/corpus/src_core/cases.jsonl"), encoding="utf-8")}
cases = [x for x in cases if x["id"] not in recorded]
cand = os.path.join(out, "cand.cases.jsonl")
write(cand, cases)
print("candidates", len(cases))
once = os.path.join(out, "cand.once.trace.jsonl")
print(run(cand, once, 1))
known = set()
for o in obs(os.path.join(ROOT, "migration/corpus/src_core/surface/cases.trace.jsonl")).values():
    known.add(json.dumps(o, sort_keys=True))
new = obs(once)
admit = []
for x in cases:
    o = new.get(x["id"])
    if o is None or "exit" not in o:
        continue
    k = json.dumps(o, sort_keys=True)
    if k in known and x["id"] not in forced:
        continue
    known.add(k)
    admit.append(x)
print("admitted by observation", len(admit))
adm = os.path.join(out, "admitted.cases.jsonl")
write(adm, admit)
tr = os.path.join(out, "admitted.trace.jsonl")
flaky = []
while True:
    rc, msg = run(adm, tr, 3)
    bad = {l.split()[1].split("@")[0] for l in msg.splitlines() if l.startswith("FLAKY")}
    if rc != 3 or not bad:
        break
    flaky += sorted(bad)
    admit = [x for x in admit if x["id"] not in bad]
    write(adm, admit)
print("record x3", rc, msg[-200:], "admitted", len(admit), "flaky dropped", len(flaky))
write(os.path.join(out, "flaky.json"), flaky)
