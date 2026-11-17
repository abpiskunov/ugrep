#!/usr/bin/env python3
"""Orders a seam trace file in place: cases in order of first appearance, then thread, then seq."""
import json
import sys

path = sys.argv[1]
events = [json.loads(line) for line in open(path, encoding="utf-8") if line.strip()]
first = {}
for e in events:
    first.setdefault(e["case"], len(first))
for e in events:
    # argv[0] is the machine specific path of the executable
    if e.get("fn") == "src_indexer.options" and e.get("kind") == "call":
        e["args"][2][0] = "<ugrep-indexer>"
events.sort(key=lambda e: (first[e["case"]], e.get("thread", 0), e.get("seq", 0)))
with open(path, "w", encoding="utf-8", newline="\n") as f:
    for e in events:
        f.write(json.dumps(e, ensure_ascii=True, separators=(",", ":")) + "\n")
