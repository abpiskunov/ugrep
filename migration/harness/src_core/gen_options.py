#!/usr/bin/env python3
"""Surface cases for every long option of ugrep (src/ugrep.cpp), found by reading the option parser; inputs only, never
expected values (the C++ is run to record them):
   gen_options.py > migration/corpus/src_core/opts.cases.jsonl
Each option without an argument runs once on a file and once on a tree; each option with an argument runs with a valid
value and with an invalid one. Options that need a terminal, an editor, a helper program or the clock are left out."""
import json
import re
import sys

import gen_cases
from gen_cases import A, BIN, D2, c, plus

start = len(gen_cases.cases)
src = open(sys.argv[1] if len(sys.argv) > 1 else "src/ugrep.cpp", encoding="utf-8", errors="replace").read()
flags = sorted(set(re.findall(r'strcmp\(arg, "([a-z0-9-]+)"\)', src)))
withs = sorted(set(re.findall(r'strncmp\(arg, "([a-z0-9-]+)=', src)))

# need a terminal, an editor or a pager program, run helper programs, print the clock, or are the legacy forms
SKIP = {"query", "view", "filter", "stats", "no-stats", "free-space", "mmap", "no-mmap", "line-buffered", "delay", "max-queue", "min-steal"}

VALID = {
    "after-context": "1", "before-context": "1", "binary-files": "text", "color": "always", "colour": "always",
    "colors": "mt=1;31", "colours": "mt=1;31", "config": "none.conf", "context": "1", "context-separator": "##",
    "depth": "1", "devices": "skip", "directories": "recurse", "encoding": "latin1", "exclude": "*.log",
    "exclude-dir": "s", "exclude-from": "ex.txt", "exclude-fs": "/", "file": "pats.txt", "file-extension": "txt",
    "file-magic": "text", "file-type": "txt", "filter-magic-label": "x:text", "format": "%f:%n:%O%~",
    "format-begin": "<", "format-close": "]", "format-end": ">", "format-open": "[", "from": "pats.txt",
    "fuzzy": "1", "glob": "*.txt", "group-separator": "==", "hexdump": "2", "hyperlink": "+", "iglob": "*.TXT",
    "ignore-files": ".gitignore", "include": "*.txt", "include-dir": "s", "include-from": "ex.txt", "include-fs": "/",
    "index": "", "jobs": "2", "label": "LBL", "max-count": "1", "max-depth": "1", "max-files": "1", "max-line": "10",
    "max-size": "10", "min-count": "1", "min-depth": "1", "min-line": "1", "min-size": "1", "neg-regexp": "bar",
    "not": "bar", "pager": "cat", "pretty": "never", "range": "1,2", "regexp": "foo", "replace": "X",
    "save-config": "saved.conf", "separator": "|", "sort": "name", "tabs": "4", "tag": "T", "width": "10", "zmax": "2",
    "and": "bar", "andnot": "bar",
}
FILES = plus(A, **{"pats.txt": "foo\nbar\n", "ex.txt": "*.log\n", ".gitignore": "*.log\n"})
FILES.update(D2)
FILES.update(BIN)

for name in flags:
    if name in SKIP:
        continue
    c("optf-" + name, ["--" + name, "foo", "a.txt"], FILES)
    c("optf-tree-" + name, ["--" + name, "-r", "foo", "d"], FILES)

for name in withs:
    if name in SKIP:
        continue
    val = VALID.get(name, "1")
    c("opta-" + name, ["--%s=%s" % (name, val), "foo", "a.txt"], FILES)
    c("opta-tree-" + name, ["--%s=%s" % (name, val), "-r", "foo", "d"], FILES)
    c("opta-bad-" + name, ["--%s=zz" % name, "foo", "a.txt"], FILES)
    c("opta-empty-" + name, ["--%s=" % name, "foo", "a.txt"], FILES)
    c("opta-sep-" + name, ["--" + name, val or "1", "foo", "a.txt"], FILES)

if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    seen = set()
    for d in gen_cases.cases[start:]:
        assert d["id"] not in seen, d["id"]
        seen.add(d["id"])
        print(json.dumps(d, ensure_ascii=True, separators=(",", ":")))
