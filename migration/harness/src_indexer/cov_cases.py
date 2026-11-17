#!/usr/bin/env python3
"""Adds the coverage profile file to surface cases: cov_cases.py <profile-dir> <cases.jsonl>... > cov.cases.jsonl
Each case writes its own raw profile, named by its position in the input and the process id, into <profile-dir>."""
import json
import os
import sys

pdir = os.path.abspath(sys.argv[1])
sys.stdout.reconfigure(encoding="utf-8", newline="\n")
n = 0
for path in sys.argv[2:]:
    for line in open(path, encoding="utf-8"):
        if not line.strip():
            continue
        case = json.loads(line)
        env = dict(case.get("env", {}))
        n += 1
        env["LLVM_PROFILE_FILE"] = os.path.join(pdir, "%06d-%%p.profraw" % n)
        case["env"] = env
        print(json.dumps(case, ensure_ascii=True, separators=(",", ":")))
