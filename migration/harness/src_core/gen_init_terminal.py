#!/usr/bin/env python3
"""Candidate surface cases for init, terminal, set_color, trim and set_terminal_hyperlink (inputs only, no expected values):
gen_init_terminal.py <out.jsonl>
The expected observations are recorded by running the C++ build (run-cases); nothing here is judged."""
import json
import sys

FILES = {
    "a.txt": "foo bar\nbar foo\nfoo\nbar\nbaz qux\nFoo Bar\nfoobar\n",
    "b.txt": "one foo\ntwo\nthree foo foo\n",
    "sub/c.txt": "sub foo\nnothing\n",
    "sub/d.log": "log foo\n",
    "pats.txt": "foo\nbar\n",
}
cases = []


def case(cid, args, files=None, env=None, stdin=None):
    c = {"id": cid, "args": args, "files": dict(FILES if files is None else files)}
    if files is not None and files == {}:
        c["files"] = {}
    if env:
        c["env"] = env
    if stdin is not None:
        c["stdin"] = stdin
    cases.append(c)


def extra(**kw):
    f = dict(FILES)
    f.update(kw)
    return f


# terminal(): --color=WHEN normalization (stdout is a pipe: never a TTY)
for w in ["never", "no", "none", "always", "yes", "force", "auto", "tty", "if-tty", "", "bogus", "Always", "alway"]:
    case("core-term-color-%s" % (w or "empty"), ["--color=%s" % w, "foo", "a.txt"])
    case("core-term-color-%s-sep" % (w or "empty"), ["--color", w, "-n", "foo", "a.txt"])
for w in ["never", "no", "none", "always", "yes", "force", "auto", "tty", "if-tty", "", "bogus"]:
    case("core-term-pretty-%s" % (w or "empty"), ["--pretty=%s" % w, "foo", "a.txt", "b.txt"])
    case("core-term-pretty-%s-r" % (w or "empty"), ["--pretty=%s" % w, "-r", "foo", "."])
case("core-term-pretty-bare", ["--pretty", "foo", "a.txt", "b.txt"])
case("core-term-pretty-always-color-never", ["--pretty=always", "--color=never", "foo", "a.txt", "b.txt"])
case("core-term-pretty-always-color-auto", ["--pretty=always", "--color=auto", "foo", "a.txt", "b.txt"])
case("core-term-pretty-always-tag", ["--pretty=always", "--tag", "foo", "a.txt", "b.txt"])
case("core-term-pretty-always-noheading", ["--pretty=always", "--no-heading", "--no-line-number", "--no-initial-tab", "foo", "a.txt", "b.txt"])
case("core-term-pretty-always-sort", ["--pretty=always", "--sort=size", "foo", "b.txt", "a.txt"])
case("core-term-pretty-always-notree", ["--pretty=always", "--no-tree", "-r", "foo", "."])
case("core-term-pretty-always-quiet", ["--pretty=always", "-q", "foo", "a.txt"])
case("core-term-pretty-always-q-color", ["--pretty=always", "--color=always", "-c", "foo", "a.txt", "b.txt"])
case("core-term-pretty-always-l", ["--pretty=always", "-l", "-r", "foo", "."])
case("core-term-pretty-always-sub", ["--pretty=always", "-r", "-t", "text", "foo", "sub"])
case("core-term-tag-color-always", ["--tag", "--color=always", "foo", "a.txt", "b.txt"])
case("core-term-tag-nocolor", ["--tag", "foo", "a.txt", "b.txt"])
case("core-term-tag-color-env", ["--tag", "--color=always", "foo", "a.txt"], env={"GREP_COLORS": "mt=1;32"})
case("core-term-quiet-color", ["-q", "--color=always", "foo", "a.txt"])
case("core-term-quiet-nomatch", ["-q", "--color=always", "nomatch", "a.txt"])
case("core-term-quiet-pager", ["-q", "--pager=more", "foo", "a.txt"])
case("core-term-pager-nontty", ["--pager=type", "foo", "a.txt"])
case("core-term-pager-default", ["--pager", "foo", "a.txt"])
case("core-term-pager-color", ["--pager=type", "--color=always", "foo", "a.txt"])
case("core-term-auto-env-noterm", ["--color=auto", "foo", "a.txt"], env={"COLORTERM": "truecolor", "TERM": "xterm-256color"})
case("core-term-auto-no-color", ["--color=auto", "foo", "a.txt"], env={"NO_COLOR": "1", "COLORTERM": "1"})
case("core-term-auto-save-config", ["--color=auto", "--save-config=out.cfg", "foo", "a.txt"])
case("core-term-auto-save-config-default", ["--color=auto", "--save-config", "foo", "a.txt"])
case("core-term-always-save-config", ["--color=always", "--colors=mt=1;32", "--save-config=out.cfg", "foo", "a.txt"])
case("core-term-never-save-config", ["--color=never", "--pretty=always", "--save-config=out.cfg", "foo", "a.txt"])
case("core-term-pretty-save-config", ["--pretty=always", "--save-config=out.cfg", "foo", "a.txt"])
case("core-term-tag-save-config", ["--tag", "--color=always", "--save-config=out.cfg", "foo", "a.txt"])
case("core-term-quiet-save-config", ["-q", "--color=always", "--save-config=out.cfg", "foo", "a.txt"])
case("core-term-hl-save-config", ["--color=always", "--colors=hl", "--save-config=out.cfg", "foo", "a.txt"])

# terminal(): --tree and the UTF-8 guess on LANG, LC_CTYPE, LC_ALL
for i, (nm, env) in enumerate([("none", {}), ("lang", {"LANG": "en_US.UTF-8"}), ("lang-latin", {"LANG": "en_US.ISO-8859-1"}),
                               ("ctype", {"LC_CTYPE": "en_US.UTF-8"}), ("lcall", {"LC_ALL": "en_US.UTF-8"}),
                               ("lang-then-ctype", {"LANG": "C", "LC_CTYPE": "C.UTF-8"}),
                               ("all-latin-ctype-utf", {"LANG": "C", "LC_CTYPE": "x", "LC_ALL": "C.UTF-8"})]):
    for opt in ["-c", "-l", "-L", ""]:
        a = ["--tree", "-r", "--sort=name"] + ([opt] if opt else []) + ["foo", "."]
        case("core-term-tree-%s%s" % (nm, opt.replace("-", "-o") if opt else "-plain"), a, env=env)
case("core-term-tree-nosort", ["--tree", "-r", "-c", "foo", "."], env={"LANG": "C.UTF-8"})
case("core-term-tree-implied-sort", ["--tree", "-r", "foo", "."])
case("core-term-tree-files-only", ["--tree", "-r", "--sort=name", "--files-with-matches", "foo", "sub", "."], env={"LC_ALL": "C.UTF-8"})

# terminal(): the colour environment on a non-TTY with --color=always/auto
for i, env in enumerate([
    {"GREP_COLORS": ""}, {"GREP_COLORS": "mt="}, {"GREP_COLORS": "ms=1;32:mc=1;33"},
    {"GREP_COLOR": "1;32"}, {"GREP_COLOR": "4"}, {"GREP_COLOR": ""},
    {"GREP_COLOR": "1;32", "GREP_COLORS": "fn=34"}, {"GREP_COLOR": "mt=1;35"},
    {"GREP_COLORS": "sl=1:cx=2:rv"}, {"GREP_COLORS": "sl=1:cx=2"}, {"GREP_COLORS": "ne"}, {"GREP_COLORS": "mt=1;31:ne"},
    {"GREP_COLORS": "hl"}, {"GREP_COLORS": "hl=vscode://file"}, {"GREP_COLORS": "hl=+"}, {"GREP_COLORS": "hl=x+:mt=1"},
    {"GREP_COLORS": "fn=35:ln=32:cn=33:bn=36:se=31"},
    {"GREP_COLORS": "sl=1;34:cx=0;90:ms=1;31:mc=1;35"},
]):
    for j, args in enumerate([["-n", "-b", "-H", "-k", "foo", "a.txt", "b.txt"], ["-v", "-C1", "-n", "foo", "a.txt"]]):
        case("core-color-env-%d-%d" % (i, j), ["--color=always", "--sort=name"] + args, env=env)
case("core-color-env-rv-v", ["--color=always", "-v", "-C1", "-n", "foo", "a.txt"], env={"GREP_COLORS": "sl=1:cx=2:rv"})
case("core-color-env-auto", ["--color=auto", "foo", "a.txt"], env={"GREP_COLORS": "mt=1;32"})
case("core-color-env-pretty-auto", ["--pretty=always", "--color=auto", "foo", "a.txt"], env={"GREP_COLORS": "mt=1;32"})

# set_color(): --colors values (deprecated SGR form, easy letters, overflow, repeats, unknown parameters)
for i, v in enumerate([
    "1;32", "4", "", ";", "01;31", "1;2;3;4;5;6;7;8;9;10;11;12;13", "0;1;2;3;4;5;6;7;8;9;10;11;12;13;14;15;16;17;18;19",
    "mt=1;32", "mt=", "mt=;", "mt=x", "ms=1;32", "mc=1;33", "ms=1;32:mc=1;33:mt=4",
    "mt=hb", "mt=rK", "mt=+r", "mt=+R", "mt=+w+B", "mt=1+r", "mt=+rh", "mt=hu", "mt=hH", "mt=uU", "mt=iI", "mt=fn", "mt=kK",
    "mt=krgybmcw", "mt=KRGYBMCW", "mt=+k+r+g+y+b+m+c+w", "mt=+K+R+G+Y+B+M+C+W", "mt=r,g;b y", "mt=,", "mt=  ", "mt=r g", "mt=h,u",
    "mt=1;31:mt=1;32", "mt=1:ms=2:mc=3", "ms=h:mc=u", "mt=r:ms=:mc=",
    "sl=1", "cx=2", "sl=r:cx=b", "sl=1:cx=2:rv", "rv", "fn=h+g", "ln=y", "cn=c", "bn=m", "se=r", "fn=:ln=:cn=:bn=:se=",
    "qp=1:qe=2:qr=3:qm=4:ql=5:qb=6", "ne", "mt=1:ne", "ne:mt=1", "hl", "hl=", "hl=+", "hl=vscode", "hl=vscode+", "hl=a1b2+", "hl=:mt=1",
    "mt=1:hl", "xx=1", "mt", "mt:", "=", "==", "mt=1;31=", "MT=1", "mt=hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh",
    "mt=rrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrr", "mt=1234567890123456789012345678901234567890",
    "mt=+", "mt=+x", "mt=h+", "mt=n", "mt=nh", "mt=1n2", "mt=+1", "mt=1+1", "mt=r1", "mt=1r",
]):
    case("core-color-colors-%d" % i, ["--color=always", "--sort=name", "--colors=%s" % v, "-n", "-b", "-H", "foo", "a.txt", "b.txt"])
for i, v in enumerate(["mt=1;32", "sl=1:cx=2", "sl=1:cx=2:rv", "ms=1;32:mc=1;33", "1;35", "ne", "hl", "mt=+r:fn=h:ln=u"]):
    case("core-color-colors-v-%d" % i, ["--color=always", "--colors=%s" % v, "-v", "-C1", "-n", "foo", "a.txt"])
    case("core-color-colors-over-env-%d" % i, ["--color=always", "--colors=%s" % v, "-n", "foo", "a.txt"],
         env={"GREP_COLORS": "mt=1;34:fn=33:ln=36"})
    case("core-color-colors-o-%d" % i, ["--color=always", "--colors=%s" % v, "-o", "-n", "foo", "a.txt"])
case("core-color-colors-twice", ["--color=always", "--colors=mt=1;32", "--colors=fn=34", "-H", "foo", "a.txt"])
case("core-color-colors-with-grep-color", ["--color=always", "--colors=4", "foo", "a.txt"], env={"GREP_COLOR": "1;33"})
case("core-color-colors-deprecated-env-colors", ["--color=always", "--colors=4", "foo", "a.txt"], env={"GREP_COLORS": "mt=1;33"})
case("core-color-colors-save", ["--color=always", "--colors=mt=1;32:fn=h", "--save-config=o.cfg", "foo", "a.txt"])

# set_terminal_hyperlink(): prefix, +line suffix, empty, cwd path, colour interplay
for i, v in enumerate(["", "+", "file", "file+", "vscode", "vscode+", "vscode://file", "vscode://file+", "a1B2", "a1B2+", "+x", "x y", ":", "-x", "_+", "9+"]):
    case("core-hl-arg-%d" % i, ["--color=always", "--sort=name", "--hyperlink=%s" % v, "foo", "a.txt", "sub/c.txt"])
    case("core-hl-arg-n-%d" % i, ["--color=always", "--sort=name", "-n", "-r", "--hyperlink=%s" % v, "foo", "."])
case("core-hl-bare", ["--color=always", "--hyperlink", "foo", "a.txt"])
case("core-hl-nocolor", ["--hyperlink", "foo", "a.txt"])
case("core-hl-color-never", ["--color=never", "--hyperlink=+", "foo", "a.txt"])
case("core-hl-color-auto", ["--color=auto", "--hyperlink=+", "foo", "a.txt"])
case("core-hl-tag", ["--tag", "--color=always", "--hyperlink=+", "foo", "a.txt"])
case("core-hl-quiet", ["-q", "--color=always", "--hyperlink=+", "foo", "a.txt"])
case("core-hl-count", ["--color=always", "--hyperlink", "-c", "foo", "a.txt", "b.txt"])
case("core-hl-files", ["--color=always", "--hyperlink", "-l", "foo", "a.txt", "b.txt"])
case("core-hl-heading", ["--color=always", "--hyperlink=+", "--heading", "-n", "foo", "a.txt", "b.txt"])
case("core-hl-format", ["--color=always", "--hyperlink", "--format=%f:%n:%O%~", "foo", "a.txt"])
case("core-hl-column", ["--color=always", "--hyperlink=+", "-k", "-n", "foo", "a.txt"])
case("core-hl-pretty", ["--pretty=always", "--hyperlink=+", "foo", "a.txt", "b.txt"])
case("core-hl-colors-hl-before-flag", ["--color=always", "--colors=hl=vscode+", "foo", "a.txt"])
case("core-hl-flag-over-colors", ["--color=always", "--hyperlink=a+", "--colors=hl=b+", "foo", "a.txt"], env={"GREP_COLORS": "hl=c"})
case("core-hl-env-hl-eq-first", ["--color=always", "foo", "a.txt"], env={"GREP_COLORS": "mt=1:hl=z+:hl"})
case("core-hl-colors-hl-env-hl-eq", ["--color=always", "--colors=hl", "foo", "a.txt"], env={"GREP_COLORS": "hl=q"})
case("core-hl-save-config", ["--color=always", "--hyperlink=+", "--save-config=o.cfg", "foo", "a.txt"])
case("core-hl-stdin", ["--color=always", "--hyperlink=+", "foo"], stdin="foo\n")
case("core-hl-stdin-dash", ["--color=always", "--hyperlink=+", "foo", "-"], stdin="foo\n")
case("core-hl-subdir-path", ["--color=always", "--hyperlink=+", "-r", "--sort=name", "-n", "foo", "sub"])
case("core-hl-dotdot", ["--color=always", "--hyperlink=+", "foo", "sub/../a.txt"])
case("core-hl-space-dir", ["--color=always", "--hyperlink=+", "-r", "foo", "d d"], files=extra(**{"d d/x y.txt": "foo\n"}))

# trim(): white space around the lines of a configuration file and of the glob files
CFG = {
    "plain.cfg": "ignore-case\nline-number\n",
    "pad.cfg": "   ignore-case   \n\tline-number\t\n",
    "crlf.cfg": "ignore-case\r\nline-number\r\n",
    "mixed.cfg": " \t ignore-case \t \r\n\v\fno-filename\f\v\n",
    "blank.cfg": "\n   \n\t\t\n   # a comment\n  ignore-case\n\n",
    "comment.cfg": "# ignore-case\n   # line-number\nfilename\n",
    "bang.cfg": "  ignore-case=\n",
    "ws-eq.cfg": "   color = always\n",
    "empty.cfg": "",
    "onlyws.cfg": " \t\r\n \n",
    "nonl.cfg": "   ignore-case   ",
    "opt-arg.cfg": "  max-count=1  \n  sort=name  \n",
    "utf8.cfg": "\u00a0ignore-case\u00a0\n",
    "bad.cfg": "  no-such-option  \n",
    "bad2.cfg": "   ignore-case=foo   \n",
    "tabs-inside.cfg": "colors=mt=1;32  \n",
    "long.cfg": ("  " + "x" * 3000 + "  \n"),
    "pat.cfg": "  regexp=foo  \n",
    "file.cfg": "  file=pats.txt  \n",
}
for k in CFG:
    case("core-trim-config-%s" % k.replace(".cfg", ""), ["--config=%s" % k, "foo", "a.txt", "b.txt"], files=extra(**CFG))
    case("core-trim-config-dash-%s" % k.replace(".cfg", ""), ["---%s" % k, "-c", "FOO", "a.txt"], files=extra(**CFG))
GLOBS = {
    "ex1.txt": "  b.txt  \n",
    "ex2.txt": "\tb.*\t\r\n\r\n   \n # c\n",
    "ex3.txt": "   \n\t\n",
    "ex4.txt": "! b.txt\n  *.log  \n",
    "ex5.txt": "\u00a0b.txt\u00a0\n",
    "ex6.txt": "   b.txt   ",
    "ex7.txt": " #b.txt\n   b.txt\n",
    "ex8.txt": "\v\fb.txt\f\v\n",
    "ex9.txt": "b.txt  \\ \n",
}
for k in GLOBS:
    n = k.replace(".txt", "")
    case("core-trim-excl-from-%s" % n, ["-r", "--sort=name", "--exclude-from=%s" % k, "foo", "."], files=extra(**GLOBS))
    case("core-trim-incl-from-%s" % n, ["-r", "--sort=name", "--include-from=%s" % k, "foo", "."], files=extra(**GLOBS))
    case("core-trim-ignore-file-%s" % n, ["-r", "--sort=name", "--ignore-files=%s" % k, "foo", "."], files=extra(**GLOBS))
case("core-trim-gitignore", ["-r", "--sort=name", "--ignore-files", "foo", "."],
     files=extra(**{".gitignore": "  b.txt  \r\n  \n\t# x\n"}))
case("core-trim-gitignore-sub", ["-r", "--sort=name", "--ignore-files", "foo", "."],
     files=extra(**{"sub/.gitignore": "\t\tc.txt\t\t\n", ".gitignore": "*.log\n"}))

# init(): the early config scan tests argv[1] (not argv[i]) for -e, -g, -N, --regexp
cfgf = extra(**CFG)
case("core-init-cfgscan-plain", ["--config=plain.cfg", "foo", "a.txt"], files=cfgf)
case("core-init-cfgscan-argv1-e", ["-e", "foo", "--config=plain.cfg", "a.txt"], files=cfgf)
case("core-init-cfgscan-argv1-e-dash", ["-e", "FOO", "---plain.cfg", "a.txt"], files=cfgf)
case("core-init-cfgscan-argv1-e-noconfig", ["-e", "FOO", "--no-config", "a.txt"], files=cfgf)
case("core-init-cfgscan-argv1-g", ["-g", "*.txt", "--sort=name", "--config=plain.cfg", "foo", "."], files=cfgf)
case("core-init-cfgscan-argv1-N", ["-N", "x", "--config=plain.cfg", "foo", "a.txt"], files=cfgf)
case("core-init-cfgscan-argv1-regexp", ["--regexp", "FOO", "--config=plain.cfg", "a.txt"], files=cfgf)
case("core-init-cfgscan-argv1-regexp-eq", ["--regexp=FOO", "--config=plain.cfg", "a.txt"], files=cfgf)
case("core-init-cfgscan-argv1-e-attached", ["-eFOO", "--config=plain.cfg", "a.txt"], files=cfgf)
case("core-init-cfgscan-e-second", ["-i", "-e", "foo", "--config=plain.cfg", "a.txt"], files=cfgf)
case("core-init-cfgscan-e-second-c", ["-c", "-e", "FOO", "--config=plain.cfg", "a.txt"], files=cfgf)
case("core-init-cfgscan-e-skips-arg", ["foo", "-e", "--config=plain.cfg", "a.txt"], files=cfgf)
case("core-init-cfgscan-e-then-config", ["-e", "FOO", "--config=plain.cfg", "--config=pad.cfg", "a.txt"], files=cfgf)
case("core-init-cfgscan-two-configs", ["--config=plain.cfg", "--config=pad.cfg", "FOO", "a.txt"], files=cfgf)
case("core-init-cfgscan-two-configs-dash", ["---plain.cfg", "---pad.cfg", "FOO", "a.txt"], files=cfgf)
case("core-init-cfgscan-config-and-dash", ["--config=plain.cfg", "---pad.cfg", "FOO", "a.txt"], files=cfgf)
case("core-init-cfgscan-e-two-configs", ["-e", "FOO", "--config=plain.cfg", "--config=pad.cfg", "a.txt"], files=cfgf)
case("core-init-cfgscan-argv1-e-no-config-late", ["-e", "FOO", "a.txt", "--no-config"], files=cfgf)
case("core-init-cfgscan-doubledash", ["--", "--config=plain.cfg", "a.txt"], files=cfgf)
case("core-init-cfgscan-doubledash-late", ["FOO", "--", "--config=plain.cfg"], files=cfgf)
case("core-init-cfgscan-doubledash-after-e", ["-e", "FOO", "--", "--config=plain.cfg"], files=cfgf)
case("core-init-cfgscan-config-empty", ["--config", "foo", "a.txt"], files=cfgf)
case("core-init-cfgscan-config-empty-eq", ["--config=", "foo", "a.txt"], files=cfgf)
case("core-init-cfgscan-dash3-empty", ["---", "foo", "a.txt"], files=cfgf)
case("core-init-cfgscan-config-prefix-longer", ["--configx=plain.cfg", "foo", "a.txt"], files=cfgf)
case("core-init-cfgscan-config-prefix-flag", ["--config-x", "foo", "a.txt"], files=cfgf)
case("core-init-cfgscan-missing", ["--config=nonexistent.cfg", "foo", "a.txt"], files=cfgf)
case("core-init-cfgscan-missing-dash", ["---nonexistent.cfg", "foo", "a.txt"], files=cfgf)
case("core-init-cfgscan-missing-after-e", ["-e", "foo", "--config=nonexistent.cfg", "a.txt"], files=cfgf)
case("core-init-cfgscan-noconfig", ["--no-config", "foo", "a.txt"], files=cfgf)
case("core-init-cfgscan-noconfig-with-config", ["--no-config", "--config=plain.cfg", "FOO", "a.txt"], files=cfgf)
case("core-init-cfgscan-badopt", ["--config=bad.cfg", "foo", "a.txt"], files=cfgf)
case("core-init-cfgscan-badopt2", ["--config=bad2.cfg", "foo", "a.txt"], files=cfgf)
case("core-init-cfgscan-pat-from-config", ["--config=pat.cfg", "a.txt"], files=cfgf)
case("core-init-cfgscan-file-from-config", ["--config=file.cfg", "a.txt"], files=cfgf)
case("core-init-cfgscan-nested-config", ["--config=nest.cfg", "foo", "a.txt"], files=extra(**{"nest.cfg": "config=plain.cfg\n", "plain.cfg": "ignore-case\n"}))
case("core-init-cfgscan-self-config", ["--config=self.cfg", "foo", "a.txt"], files=extra(**{"self.cfg": "config=self.cfg\nignore-case\n"}))
case("core-init-cfgscan-home-config", ["--config", "foo", "a.txt"], files=extra(**{".ugrep": "ignore-case\n"}))
case("core-init-cfgscan-xdg", ["--config", "FOO", "a.txt"], files=extra(**{".config/ugrep/config": "ignore-case\n"}))
case("core-init-cfgscan-argv-e-then-e", ["-e", "-e", "foo", "a.txt"])
case("core-init-cfgscan-regexp-last", ["foo", "a.txt", "--regexp"])
case("core-init-cfgscan-e-last", ["foo", "a.txt", "-e"])
case("core-init-cfgscan-g-last", ["foo", "a.txt", "-g"])
case("core-init-cfgscan-e-dashdash", ["-e", "--", "a.txt"])
case("core-init-cfgscan-noargs", [])
case("core-init-cfgscan-noargs-config", ["--config=plain.cfg"], files=cfgf)
case("core-init-cfgscan-only-e", ["-e"])
case("core-init-cfgscan-only-N", ["-N"])

# init(): program-name handling for the name fixed by the harness, option post-processing and usage errors
case("core-init-nopattern", ["a.txt"])
case("core-init-nopattern-file", ["-q"])
case("core-init-nopattern-e-missing-file", ["-e", "foo"], stdin="foo\n")
case("core-init-match-option", ["--match", "a.txt"])
case("core-init-match-option-e", ["--match", "-e", "foo", "a.txt"])
case("core-init-pattern-after-e", ["-e", "foo", "bar", "a.txt"])
case("core-init-pattern-after-f", ["-f", "pats.txt", "bar", "a.txt"])
case("core-init-and-first", ["--and", "foo", "a.txt"])
case("core-init-andnot-first", ["--andnot", "foo", "a.txt"])
case("core-init-not-first", ["--not", "foo", "a.txt"])
case("core-init-bool-e-and", ["-%", "-e", "foo", "--and", "bar", "a.txt"])
case("core-init-bool-percent-and", ["-%", "foo bar", "a.txt"])
case("core-init-bool-percent-percent", ["-%%", "foo bar", "a.txt"])
case("core-init-bool-e-only", ["-%", "-e", "foo", "a.txt"])
case("core-init-bool-e-N", ["-%", "-e", "foo", "-N", "bar", "a.txt"])
case("core-init-bool-F-N", ["-F", "-%", "-e", "foo", "-N", "bar", "a.txt"])
case("core-init-F-N", ["-F", "-e", "foo", "-N", "bar", "a.txt"])
case("core-init-e-N-empty", ["-e", "foo", "-N", "", "a.txt"])
case("core-init-e-not-empty", ["-e", "foo", "--not", "", "a.txt"])
case("core-init-and-or-mix", ["-e", "foo", "-e", "bar", "--and", "baz", "a.txt"])
case("core-init-not-with-e", ["-e", "foo", "--not", "bar", "a.txt"])
case("core-init-bool-not-e", ["-%", "-e", "foo", "--not", "bar", "a.txt"])
case("core-init-bool-andnot-e", ["-%", "-e", "foo", "--andnot", "bar", "a.txt"])
case("core-init-o-clears-Y", ["-Y", "-o", "foo", "a.txt"])
case("core-init-u-clears-Y", ["-Y", "-u", "foo", "a.txt"])
case("core-init-Y-alone", ["-Y", "-c", "", "a.txt"])
case("core-init-tab-bad", ["--tabs=3", "foo", "a.txt"])
case("core-init-tab-ok", ["--tabs=8", "-T", "foo", "a.txt"])
case("core-init-tab-0", ["--tabs=0", "foo", "a.txt"])
case("core-init-binary-bad", ["--binary-files=x", "foo", "a.txt"])
case("core-init-binary-opts", ["--binary-files=without-match", "foo", "a.txt"])
case("core-init-binary-with-hex", ["--binary-files=with-hex", "foo", "a.txt"])
case("core-init-hexdump-bad", ["--hexdump=9", "foo", "a.txt"])
case("core-init-hexdump-bad2", ["--hexdump=1x", "foo", "a.txt"])
case("core-init-hexdump-ok", ["--hexdump=2ah", "foo", "a.txt"])
case("core-init-hex-priority", ["--hex", "-a", "-I", "foo", "a.txt"])
case("core-init-withhex-priority", ["--with-hex", "-a", "-I", "foo", "a.txt"])
case("core-init-I-over-a", ["-I", "-a", "foo", "a.txt"])
case("core-init-zmax-100", ["--zmax=100", "foo", "a.txt"])
case("core-init-zmax-99", ["-z", "--zmax=99", "foo", "a.txt"])
case("core-init-z", ["-z", "foo", "a.txt"])
case("core-init-P", ["-P", "foo", "a.txt"])
case("core-init-P-Z", ["-P", "-Z", "foo", "a.txt"])
case("core-init-P-N", ["-P", "-N", "x", "foo", "a.txt"])
case("core-init-type-list", ["-t", "list"])
case("core-init-type-list-more", ["-t", "list", "-t", "c"])
case("core-init-type-bad", ["-t", "nosuchtype", "foo", "a.txt"])
case("core-init-type-text", ["-r", "--sort=name", "-t", "text", "foo", "."])
case("core-init-type-neg", ["-r", "--sort=name", "-t", "^text", "foo", "."])
case("core-init-type-extension-O", ["-r", "--sort=name", "-O", "log", "foo", "."])
case("core-init-type-M", ["-r", "--sort=name", "-M", "foo", "foo", "."])
case("core-init-type-M-bad", ["-r", "-M", "(", "foo", "."])
case("core-init-type-M-neg", ["-r", "--sort=name", "-M", "!foo", "foo", "."])
case("core-init-filter-magic-bad", ["--filter-magic-label=nolabel", "foo", "a.txt"])
case("core-init-filter-magic-bad2", ["--filter-magic-label=x:(", "foo", "a.txt"])
case("core-init-filter-magic-ok", ["--filter-magic-label=txt:foo", "foo", "a.txt"])
case("core-init-D-bad", ["-D", "x", "foo", "a.txt"])
case("core-init-D-skip", ["-D", "skip", "foo", "a.txt"])
case("core-init-d-bad", ["-d", "x", "foo", "a.txt"])
case("core-init-d-recurse", ["-d", "recurse", "--sort=name", "foo", "."])
case("core-init-d-skip", ["-d", "skip", "foo", ".", "a.txt"])
case("core-init-index-bad", ["--index=x", "foo", "a.txt"])
case("core-init-index-safe", ["--index=safe", "foo", "a.txt"])
case("core-init-from-missing", ["--from=nofile.txt", "foo"])
case("core-init-from-empty", ["--from=empty.txt", "foo"], files=extra(**{"empty.txt": ""}))
case("core-init-from-files", ["--sort=name", "--from=list.txt", "foo"], files=extra(**{"list.txt": "a.txt\nb.txt\n"}))
case("core-init-from-files-pad", ["--sort=name", "--from=list.txt", "foo"], files=extra(**{"list.txt": "  a.txt  \r\n\tb.txt\t\r\n"}))
case("core-init-from-files-missing-entry", ["--sort=name", "--from=list.txt", "foo"], files=extra(**{"list.txt": "a.txt\nnofile.txt\n"}))
case("core-init-file-missing", ["foo", "nofile.txt"])
case("core-init-file-missing-s", ["-s", "foo", "nofile.txt", "a.txt"])
case("core-init-file-missing-S", ["-S", "foo", "nofile.txt", "a.txt"])
case("core-init-dir-arg", ["foo", "sub"])
case("core-init-dir-arg-trailing-sep", ["-r", "--sort=name", "foo", "sub/"])
case("core-init-dir-arg-trailing-seps", ["-r", "--sort=name", "foo", "sub///"])
case("core-init-dir-arg-backslash", ["-r", "--sort=name", "foo", "sub\\"])
case("core-init-dir-arg-drive", ["-r", "--sort=name", "foo", "./sub/"])
case("core-init-wildcard", ["--sort=name", "foo", "*.txt"])
case("core-init-wildcard-sub", ["--sort=name", "foo", "sub/*"])
case("core-init-wildcard-nomatch", ["foo", "*.nomatch"])
case("core-init-wildcard-q", ["foo", "?.txt"])
case("core-init-wildcard-bracket", ["--sort=name", "foo", "[ab].txt"])
case("core-init-wildcard-dir", ["--sort=name", "foo", "su*/*.txt"])
case("core-init-wildcard-dash-dash", ["--sort=name", "--", "foo", "*.txt"])
case("core-init-wildcard-quoted-literal", ["foo", "\"*.txt\""])
case("core-init-format-replace", ["--format=%f", "--replace=x", "foo", "a.txt"])
case("core-init-invert-files", ["-v", "--files", "foo", "a.txt"])
case("core-init-min-count-files", ["--min-count=1", "--files", "foo", "a.txt"])
case("core-init-min-count-v", ["--min-count=1", "-v", "foo", "a.txt"])
case("core-init-min-count-v-c", ["--min-count=1", "-v", "-c", "foo", "a.txt"])
case("core-init-min-count-v-l", ["--min-count=1", "-v", "-l", "foo", "a.txt"])
case("core-init-min-count-v-q", ["--min-count=1", "-v", "-q", "foo", "a.txt"])
case("core-init-cpp-format", ["--cpp", "foo", "a.txt"])
case("core-init-csv-format", ["--csv", "-n", "foo", "a.txt"])
case("core-init-json-format", ["--json", "-n", "foo", "a.txt"])
case("core-init-xml-format", ["--xml", "foo", "a.txt"])
case("core-init-exclude-fs", ["--exclude-fs=.", "-r", "foo", "."])
case("core-init-include-fs", ["--include-fs=.", "-r", "--sort=name", "foo", "."])
case("core-init-exclude-fs-bad", ["--exclude-fs=nosuchdir", "-r", "foo", "."])
case("core-init-include-fs-bad", ["--include-fs=nosuchdir", "-r", "foo", "."])
case("core-init-exclude-from-missing", ["--exclude-from=nofile", "-r", "foo", "."])
case("core-init-include-from-missing", ["--include-from=nofile", "-r", "foo", "."])
case("core-init-ignore-files-missing", ["--ignore-files=nofile", "-r", "foo", "."])
case("core-init-encoding-list", ["--encoding=list"])
case("core-init-encoding-bad", ["--encoding=nosuch", "foo", "a.txt"])
case("core-init-encoding-ok", ["--encoding=latin1", "foo", "a.txt"])
case("core-init-separator-empty", ["--separator=", "-n", "foo", "a.txt", "b.txt"])
case("core-init-separator-custom", ["--separator=@", "-n", "-H", "foo", "a.txt", "b.txt"])
case("core-init-separator-default-b", ["--separator", "-n", "-H", "foo", "a.txt"])
case("core-init-save-config-exit", ["--save-config=o.cfg", "foo", "a.txt"])
case("core-init-save-config-and-search", ["--save-config=o.cfg", "-n", "-i", "--sort=name", "foo", "a.txt"])
case("core-init-save-config-warn", ["--save-config=o.cfg", "-x", "foo", "a.txt"])
case("core-init-save-config-nofile", ["--save-config=nodir/o.cfg", "foo", "a.txt"])
case("core-init-save-config-stdout-pat", ["--save-config=o.cfg", "-e", "foo", "-e", "bar", "a.txt"])
case("core-init-save-config-pretty", ["--save-config=o.cfg", "--pretty=always", "foo", "a.txt"])
case("core-init-warnings-then-usage", ["-x", "foo", "a.txt"])
case("core-init-unknown-long", ["--no-such-option", "foo", "a.txt"])
case("core-init-unknown-short", ["-8", "foo", "a.txt"])
case("core-init-version-with-config", ["--config=plain.cfg", "--version"], files=cfgf)
case("core-init-help-with-config", ["--config=plain.cfg", "--help"], files=cfgf)
case("core-init-help-what", ["--help", "format"])
case("core-init-stdout-mode-binary", ["-a", "foo", "a.txt"])
case("core-init-home-dir-tilde", ["--exclude-from=~/ex.txt", "-r", "--sort=name", "foo", "."], files=extra(**{"ex.txt": "b.txt\n"}))
case("core-init-home-dir-tilde-config", ["--config=~/h.cfg", "foo", "a.txt"], files=extra(**{"h.cfg": "ignore-case\n"}))
case("core-init-grep-compat-flag", ["--grep", "foo", "a.txt"])
case("core-init-query-flag-no-tty", ["-Q", "--no-config", "foo"], stdin="")


# init(): home directory (USERPROFILE on Windows, HOME elsewhere) is the base of ~ and of the default configuration file
HOMEENV = {"USERPROFILE": ".", "HOME": "."}
case("core-init-home-default-config", ["--config", "FOO", "a.txt"], files=extra(**{".ugrep": "ignore-case\n"}), env=HOMEENV)
case("core-init-home-default-config-noconfig", ["--no-config", "FOO", "a.txt"], files=extra(**{".ugrep": "ignore-case\n"}), env=HOMEENV)
case("core-init-home-default-config-ug-only", ["FOO", "a.txt"], files=extra(**{".ugrep": "ignore-case\n"}), env=HOMEENV)
case("core-init-home-xdg-config", ["--config", "FOO", "a.txt"], files=extra(**{"xdg/ugrep/config": "ignore-case\n"}), env={"XDG_CONFIG_HOME": "xdg"})
case("core-init-home-tilde-backslash", ["--config=~\\h.cfg", "FOO", "a.txt"], files=extra(**{"h.cfg": "ignore-case\n"}), env=HOMEENV)
case("core-init-home-tilde-slash", ["--config=~/h.cfg", "FOO", "a.txt"], files=extra(**{"h.cfg": "ignore-case\n"}), env=HOMEENV)
case("core-init-home-tilde-exclude", ["-r", "--exclude-from=~\\ex.txt", "foo", "."], files=extra(**{"ex.txt": "b.txt\n"}), env=HOMEENV)
case("core-init-home-tilde-ignore", ["-r", "--ignore-files=~\\ex.txt", "foo", "."], files=extra(**{"ex.txt": "b.txt\n"}), env=HOMEENV)
case("core-init-home-tilde-file", ["-f", "~\\pats.txt", "a.txt"], env=HOMEENV)
def orders_output(c):
    # cases that test the sort default of terminal() keep their arguments; every other multi-file case gets a fixed order
    a = c["args"]
    keep = ("pretty-always", "pretty-empty", "pretty-yes", "pretty-force", "tree", "save-config", "noargs", "help", "version", "encoding-list", "nosort", "cfgscan")
    return any(x.startswith("--sort") for x in a) or any(k in c["id"] for k in keep) or "-q" in a or "--" in a


with open(sys.argv[1], "w", encoding="utf-8", newline="\n") as f:
    seen = set()
    for c in cases:
        if not orders_output(c):
            c["args"] = ["--sort=name"] + c["args"]
        assert c["id"] not in seen, c["id"]
        seen.add(c["id"])
        f.write(json.dumps(c, ensure_ascii=True, separators=(",", ":")) + "\n")
print(len(cases), "candidates", file=sys.stderr)
