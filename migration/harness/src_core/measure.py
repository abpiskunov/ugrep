#!/usr/bin/env python3
"""Totals of the coverage of the baseline profiles plus the candidates in the given files: measure.py a.jsonl b.jsonl...
(a quick look between rounds; the gate's coverage.sh is the judge)"""
import json
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(__file__))
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
WORK = os.path.join(ROOT, "migration", "out", "src_core", "coverage")
LLVM = open(os.path.join(WORK, "llvm-dir.txt")).read().strip()
P2 = os.path.join(WORK, "profiles2")
os.makedirs(P2, exist_ok=True)
for f in os.listdir(P2):
    os.remove(os.path.join(P2, f))
out = os.path.join(WORK, "measure.cases.jsonl")
n = 0
with open(out, "w", encoding="utf-8", newline="\n") as o:
    for path in sys.argv[1:]:
        for line in open(path, encoding="utf-8"):
            if line.strip():
                c = json.loads(line)
                env = dict(c.get("env", {}))
                env["LLVM_PROFILE_FILE"] = os.path.join(P2, "%p.profraw")
                c["env"] = env
                o.write(json.dumps(c, ensure_ascii=True, separators=(",", ":")) + "\n")
                n += 1
tools = ["cargo", "run", "-q", "--release", "--locked", "--manifest-path", os.path.join(ROOT, "migration", "tools", "cpp2rust-tools", "Cargo.toml"), "--"]
subprocess.run(tools + ["run-cases", "--bin", os.path.join(WORK, "ugrep-cov.exe"), "--impl", "cpp", "--cases", out, "--env-matrix",
                        os.path.join(ROOT, "migration", "env-matrix.json"), "--out", os.path.join(WORK, "measure.surface.jsonl"), "--timeout", "120"],
               cwd=ROOT, capture_output=True)
lst = [os.path.join(WORK, "profiles", f) for f in os.listdir(os.path.join(WORK, "profiles"))] + [os.path.join(P2, f) for f in os.listdir(P2)]
open(os.path.join(WORK, "measure.txt"), "w").write("\n".join(lst))
subprocess.run([LLVM + "/llvm-profdata.exe", "merge", "-sparse", "-f", os.path.join(WORK, "measure.txt"), "-o", os.path.join(WORK, "cov.profdata")], check=True)
r = subprocess.run([LLVM + "/llvm-cov.exe", "export", os.path.join(WORK, "ugrep-cov.exe"), "-instr-profile=" + os.path.join(WORK, "cov.profdata"), "-summary-only",
                    "src/ugrep.cpp", "src/output.cpp", "src/stats.cpp", "src/cnf.cpp"], cwd=ROOT, capture_output=True, text=True)
t = json.loads(r.stdout)["data"][0]["totals"]
print("cases", n, "functions %.2f (%d/%d)" % (t["functions"]["percent"], t["functions"]["covered"], t["functions"]["count"]), "lines %.2f (%d/%d)" % (t["lines"]["percent"], t["lines"]["covered"], t["lines"]["count"]))
