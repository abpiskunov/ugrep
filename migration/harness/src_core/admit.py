#!/usr/bin/env python3
"""Admission of candidate surface cases by coverage: admit.py <candidates.jsonl> <admitted.jsonl> [--baseline DIR]
Every candidate runs once against the clang-cl coverage build (migration/out/src_core/coverage/ugrep-cov.exe, made by
build-coverage.ps1) with its own raw profile; a case is admitted when it executes a counter (function, region) that no
earlier admitted case and none of the baseline profiles (the profiles of the corpus recorded so far) executed.
Inputs are only run, never judged: no expected value is typed. Stable output is checked later by run-cases --repeat."""
import hashlib
import json
import os
import re
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
WORK = os.path.join(ROOT, "migration", "out", "src_core", "coverage")
LLVM = open(os.path.join(WORK, "llvm-dir.txt")).read().strip()
TOOLS = ["cargo", "run", "-q", "--release", "--locked", "--manifest-path",
         os.path.join(ROOT, "migration", "tools", "cpp2rust-tools", "Cargo.toml"), "--"]


def counters(path):
    out = subprocess.run([LLVM + "/llvm-profdata.exe", "show", "--all-functions", "--counts", path],
                         capture_output=True, text=True).stdout
    hit = set()
    name = None
    for line in out.splitlines():
        s = line.strip()
        if line.startswith("  ") and not line.startswith("    ") and s.endswith(":"):
            name = s[:-1]
        elif s.startswith("Function count:") and int(s.split(":")[1]) > 0:
            hit.add((name, -1))
        elif s.startswith("Block counts:"):
            for i, v in enumerate(re.findall(r"\d+", s)):
                if int(v) > 0:
                    hit.add((name, i))
    return hit


def main():
    cand, admitted = sys.argv[1], sys.argv[2]
    base = sys.argv[4] if len(sys.argv) > 4 and sys.argv[3] == "--baseline" else os.path.join(WORK, "profiles")
    known = set()
    cache = os.path.join(WORK, "baseline.counters.json")
    if os.path.exists(cache) and "--fresh" not in sys.argv:
        known = {tuple(x) for x in json.load(open(cache))}
    else:
        for f in os.listdir(base):
            if f.endswith(".profraw"):
                known |= counters(os.path.join(base, f))
        json.dump(sorted(known, key=str), open(cache, "w"))
    print("baseline counters", len(known), file=sys.stderr)
    pdir = os.path.join(WORK, "cand-profiles")
    os.makedirs(pdir, exist_ok=True)
    for f in os.listdir(pdir):
        os.remove(os.path.join(pdir, f))
    cases = [json.loads(l) for l in open(cand, encoding="utf-8") if l.strip()]
    run = []
    for c in cases:
        c2 = dict(c)
        env = dict(c.get("env", {}))
        env["LLVM_PROFILE_FILE"] = os.path.join(pdir, hashlib.md5(c["id"].encode()).hexdigest() + ".profraw")
        c2["env"] = env
        run.append(c2)
    tmp = os.path.join(WORK, "cand.cases.jsonl")
    with open(tmp, "w", encoding="utf-8", newline="\n") as f:
        for c in run:
            f.write(json.dumps(c, ensure_ascii=True, separators=(",", ":")) + "\n")
    r = subprocess.run(TOOLS + ["run-cases", "--bin", os.path.join(WORK, "ugrep-cov.exe"), "--impl", "cpp", "--cases", tmp,
                                "--env-matrix", os.path.join(ROOT, "migration", "env-matrix.json"), "--out",
                                os.path.join(WORK, "cand.surface.jsonl"), "--timeout", "120"], cwd=ROOT, capture_output=True, text=True)
    if r.returncode not in (0, 3):
        print(r.stdout, r.stderr, file=sys.stderr)
        sys.exit(r.returncode)
    keep = []
    for c in cases:
        p = os.path.join(pdir, hashlib.md5(c["id"].encode()).hexdigest() + ".profraw")
        if not os.path.exists(p):
            continue
        hit = counters(p)
        new = hit - known
        if new:
            known |= hit
            keep.append(c)
            print("admit %-50s +%d" % (c["id"], len(new)), file=sys.stderr)
    json.dump(sorted(known, key=str), open(cache, "w"))
    with open(admitted, "w", encoding="utf-8", newline="\n") as f:
        for c in keep:
            f.write(json.dumps(c, ensure_ascii=True, separators=(",", ":")) + "\n")
    print("candidates", len(cases), "admitted", len(keep), file=sys.stderr)


if __name__ == "__main__":
    main()
