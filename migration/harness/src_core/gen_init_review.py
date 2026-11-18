#!/usr/bin/env python3
"""Review round 13: candidate cases (inputs only, no expected values) for hyperlinks from GREP_COLORS/--colors, the
home directory lookup of the default configuration file, GREP_COLOR with --colors and trailing path separators:
gen_init_review.py <out.jsonl>
The expected observations are recorded by running the C++ build (run-cases)."""
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


def case(cid, args, files=None, env=None):
    c = {"id": cid, "args": ["--sort=name"] + args, "files": dict(FILES if files is None else files)}
    if env:
        c["env"] = env
    cases.append(c)


def extra(**kw):
    f = dict(FILES)
    f.update(kw)
    return f


TWO = ["-H", "foo", "a.txt", "b.txt"]
# hl=X sets the hyperlink prefix from offset 3 of the whole GREP_COLORS / --colors string
case("core-hl-env-hl-eq-first-H", ["--color=always"] + TWO, env={"GREP_COLORS": "mt=1:hl=z+:hl"})
case("core-hl-env-hl-eq-late-H", ["--color=always"] + TWO, env={"GREP_COLORS": "mt=1:hl=z+"})
case("core-hl-env-hl-eq-late-plain-H", ["--color=always"] + TWO, env={"GREP_COLORS": "mt=1:hl=z"})
case("core-hl-env-hl-eq-lead-H", ["--color=always"] + TWO, env={"GREP_COLORS": "hl=z+:mt=1"})
case("core-hl-env-hl-eq-lead-plain-H", ["--color=always"] + TWO, env={"GREP_COLORS": "hl=q"})
case("core-hl-env-hl-eq-lead-empty-H", ["--color=always"] + TWO, env={"GREP_COLORS": "hl="})
case("core-hl-env-hl-eq-lead-file-H", ["--color=always"] + TWO, env={"GREP_COLORS": "hl=file+:mt=1"})
case("core-hl-env-hl-bare-H", ["--color=always"] + TWO, env={"GREP_COLORS": "hl"})
case("core-hl-env-hl-bare-mt-H", ["--color=always"] + TWO, env={"GREP_COLORS": "mt=1:hl"})
case("core-hl-colors-hl-eq-lead-H", ["--color=always", "--colors=hl=z+"] + TWO)
case("core-hl-colors-hl-eq-late-H", ["--color=always", "--colors=mt=1:hl=z+"] + TWO)
case("core-hl-colors-hl-bare-H", ["--color=always", "--colors=hl"] + TWO)
case("core-hl-colors-hl-env-hl-eq-H", ["--color=always", "--colors=hl"] + TWO, env={"GREP_COLORS": "hl=q"})
case("core-hl-colors-hl-before-flag-H", ["--color=always", "--colors=hl=vscode+"] + TWO)
case("core-hl-flag-over-colors-H", ["--color=always", "--hyperlink=a+", "--colors=hl=b+"] + TWO, env={"GREP_COLORS": "hl=c"})
case("core-hl-flag-over-env-hl-eq-H", ["--color=always", "--hyperlink=w"] + TWO, env={"GREP_COLORS": "hl=q"})
# --save-config prints hyperlink=%s from the string that init frees
case("core-hl-env-hl-eq-save-config", ["--color=always", "--save-config=out.cfg"] + TWO, env={"GREP_COLORS": "hl=z+:mt=1"})
case("core-hl-env-hl-eq-late-save-config", ["--color=always", "--save-config=out.cfg"] + TWO, env={"GREP_COLORS": "mt=1:hl=z+"})
case("core-hl-colors-hl-eq-save-config", ["--color=always", "--colors=hl=z+", "--save-config=out.cfg"] + TWO)
case("core-hl-flag-save-config", ["--color=always", "--hyperlink=z+", "--save-config=out.cfg"] + TWO)
case("core-hl-env-hl-bare-save-config", ["--color=always", "--save-config=out.cfg"] + TWO, env={"GREP_COLORS": "hl"})

# the printed name of a path with .. and a link
case("core-hl-dotdot-H", ["--color=always", "--hyperlink=+", "-H", "foo", "sub/../a.txt"])
case("core-hl-dotdot-two", ["--color=always", "--hyperlink=+", "foo", "sub/../a.txt", "b.txt"])
case("core-hl-dotdot-two-dirs", ["--color=always", "--hyperlink", "foo", "./a.txt", "sub/../b.txt"])

# the default configuration file: working directory first, then the home directory
HOME = {"USERPROFILE": ".home", "HOME": ".home"}
case("core-init-home-sub-only-home", ["--config", "FOO", "a.txt"], files=extra(**{".home/.ugrep": "ignore-case\n"}), env=HOME)
case("core-init-home-sub-only-home-no-config", ["FOO", "a.txt"], files=extra(**{".home/.ugrep": "ignore-case\n"}), env=HOME)
case("core-init-home-sub-only-wdir", ["--config", "FOO", "a.txt"], files=extra(**{".ugrep": "ignore-case\n", ".home/x": "x\n"}), env=HOME)
case("core-init-home-sub-both-wdir-first", ["--config", "foo", "a.txt"],
     files=extra(**{".ugrep": "ignore-case\n", ".home/.ugrep": "invert-match\n"}), env=HOME)
case("core-init-home-sub-both-wdir-first-i", ["--config", "FOO", "a.txt"],
     files=extra(**{".ugrep": "ignore-case\n", ".home/.ugrep": "invert-match\n"}), env=HOME)
case("core-init-home-sub-neither", ["--config", "foo", "a.txt"], files=extra(**{".home/x": "x\n"}), env=HOME)
case("core-init-home-sub-named-config", ["--config=my.cfg", "FOO", "a.txt"], files=extra(**{".home/my.cfg": "ignore-case\n"}), env=HOME)
case("core-init-home-sub-named-both", ["--config=my.cfg", "foo", "a.txt"],
     files=extra(**{"my.cfg": "ignore-case\n", ".home/my.cfg": "invert-match\n"}), env=HOME)

# GREP_COLOR over --colors
GC = {"GREP_COLOR": "1;33"}
case("core-color-colors-eq-with-grep-color", ["--color=always", "--colors=mt=4", "foo", "a.txt"], env=GC)
case("core-color-colors-ms-with-grep-color", ["--color=always", "--colors=ms=4", "foo", "a.txt"], env=GC)
case("core-color-colors-without-grep-color", ["--color=always", "--colors=4", "foo", "a.txt"])
case("core-color-colors-eq-without-grep-color", ["--color=always", "--colors=mt=4", "foo", "a.txt"])
case("core-color-colors-with-grep-color-and-colors-env", ["--color=always", "--colors=4", "foo", "a.txt"],
     env={"GREP_COLOR": "1;33", "GREP_COLORS": "mt=1;35"})

# trim_pathname_arg removes only the native separator
for n, a in [("bs1", "sub\\"), ("bs3", "sub\\\\\\"), ("mix", "sub/\\"), ("mix2", "sub\\/"), ("fs1", "sub/"), ("fs3", "sub///"),
             ("dot-bs", ".\\"), ("dot-fs", "./")]:
    case("core-init-dir-arg-trim-" + n, ["-r", "foo", a])

with open(sys.argv[1], "w", encoding="utf-8", newline="\n") as f:
    seen = set()
    for c in cases:
        assert c["id"] not in seen, c["id"]
        seen.add(c["id"])
        f.write(json.dumps(c, ensure_ascii=True, separators=(",", ":")) + "\n")
print(len(cases), "candidates", file=sys.stderr)
