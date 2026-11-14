#!/usr/bin/env python3
"""Adds the coverage profile file to surface cases: cov_cases.py <profile-dir> <cases.jsonl>... > cov.cases.jsonl
Each run of the product writes its own raw profile (%p is the process id) into <profile-dir>; run-cases only runs it."""
import json
import os
import sys

pdir = os.path.abspath(sys.argv[1])
sys.stdout.reconfigure(encoding="utf-8", newline="\n")
for path in sys.argv[2:]:
    for line in open(path, encoding="utf-8"):
        if not line.strip():
            continue
        case = json.loads(line)
        env = dict(case.get("env", {}))
        env["LLVM_PROFILE_FILE"] = os.path.join(pdir, "%p.profraw")
        case["env"] = env
        print(json.dumps(case, ensure_ascii=True, separators=(",", ":")))
