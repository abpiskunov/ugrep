#!/usr/bin/env python3
"""Quiet round of the help/usage/version/errors discovery: independent flags are added to recorded hm-* cases and the
C++ is run on the variants; a variant that observes exactly what its base case observed adds nothing.
   quiet_help_main.py <ugrep.exe> <seed> <out-dir>
Inputs are only run; the verdict compares the C++ with its own recording (no expected value is typed)."""
import json
import os
import random
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
TOOLS = ["cargo", "run", "-q", "--release", "--locked", "--manifest-path",
         os.path.join(ROOT, "migration", "tools", "cpp2rust-tools", "Cargo.toml"), "--"]
exe, seed, out = sys.argv[1], int(sys.argv[2]), sys.argv[3]
FLAGS = {1: [["--no-config"], ["-J1"], ["--no-ignore-files"]], 2: [["--tabs=8"], ["--max-line=100000"], ["--no-hidden"]]}
flags = FLAGS[seed % 2 + 1] if seed in (1, 2) else FLAGS[1]
rnd = random.Random(seed)
base = [json.loads(l) for l in open(os.path.join(ROOT, "migration", "corpus", "src_core", "cases.jsonl"), encoding="utf-8") if '"id":"hm-' in l]
skip = ("--color", "--stats", "--sort", "-J", "--no-config", "--tabs", "--max-line", "--hidden", "-r", "-R", "--config", "--pager", "-Q")
cand = [b for b in base if not any(a.startswith(skip) for a in b["args"]) and not b["id"].startswith("hm-help-")]
rnd.shuffle(cand)
os.makedirs(out, exist_ok=True)
vs = []
for b in cand[:150]:
    v = dict(b)
    v["id"] = b["id"] + "-q%d" % seed
    v["args"] = rnd.choice(flags) + b["args"]
    vs.append(v)
cases = os.path.join(out, "quiet%d.cases.jsonl" % seed)
with open(cases, "w", encoding="utf-8", newline="\n") as f:
    for v in vs:
        f.write(json.dumps(v, ensure_ascii=True, separators=(",", ":")) + "\n")
tr = os.path.join(out, "quiet%d.trace.jsonl" % seed)
r = subprocess.run(TOOLS + ["run-cases", "--bin", exe, "--impl", "cpp", "--repeat", "3", "--cases", cases, "--env-matrix",
                            os.path.join(ROOT, "migration", "env-matrix.json"), "--out", tr, "--timeout", "60"], cwd=ROOT,
                   capture_output=True, text=True)
print(r.stdout.strip().splitlines()[-1:], r.stderr.strip()[-300:])


def obs(path, suffix):
    d = {}
    for line in open(path, encoding="utf-8"):
        e = json.loads(line)
        cid = e["case"].split("@")[0]
        if suffix and cid.endswith(suffix):
            cid = cid[: -len(suffix)]
        if e["kind"] in ("stdout", "stderr", "exit"):
            d.setdefault(cid, {})[e["kind"]] = e["ret"]
    return d


basetrace = os.path.join(ROOT, "migration", "corpus", "src_core", "surface", "cases.trace.jsonl")
a = obs(basetrace, "")
b = obs(tr, "-q%d" % seed)
diff = [k for k in b if k in a and a[k] != b[k]]
print("variants", len(vs), "compared", len(b), "differ", len(diff), diff[:10])
