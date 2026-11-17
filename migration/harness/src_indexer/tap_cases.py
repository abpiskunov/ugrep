#!/usr/bin/env python3
"""Adds the seam tap environment to surface cases: tap_cases.py <tap-out.jsonl> <cases.jsonl>... > tapped.jsonl
Each case gets CPP2RUST_TAP_OUT (absolute) and CPP2RUST_TAP_CASE (its id), so the tap build of the product records
the seam events of that case under the case's id. The product writes the events itself; run-cases only runs it."""
import json
import os
import sys

out = os.path.abspath(sys.argv[1])
sys.stdout.reconfigure(encoding="utf-8", newline="\n")
for path in sys.argv[2:]:
    for line in open(path, encoding="utf-8"):
        if not line.strip():
            continue
        case = json.loads(line)
        env = dict(case.get("env", {}))
        env["CPP2RUST_TAP_OUT"] = out
        env["CPP2RUST_TAP_CASE"] = case["id"]
        case["env"] = env
        print(json.dumps(case, ensure_ascii=True, separators=(",", ":")))
