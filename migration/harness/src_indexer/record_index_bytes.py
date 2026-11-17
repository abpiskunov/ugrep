#!/usr/bin/env python3
"""Records the bytes of every index file (._UG#_Store) the C++ ugrep-indexer leaves behind, per surface case:
  python migration/harness/src_indexer/record_index_bytes.py [--exe path] [--out file] [--check]
It runs each case of migration/corpus/src_indexer/cases.jsonl in a fresh directory (same environment as run-cases)
and writes one line per index file to migration/corpus/src_indexer/index-bytes/index.bytes.jsonl:
  {"case": id, "path": rel path, "size": n, "magic": "UG#\\x03", "header_hex": first 16 bytes, "sha256": ..., "hex": all bytes}
--check records again and fails when the result differs from the file (the byte-for-byte stability check).
The surface traces keep size and SHA-256 of every file; this file adds the readable bytes (header and table)."""
import hashlib, json, os, subprocess, sys, tempfile

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
CASES = os.path.join(ROOT, "migration", "corpus", "src_indexer", "cases.jsonl")
OUT = os.path.join(ROOT, "migration", "corpus", "src_indexer", "index-bytes", "index.bytes.jsonl")
EXE = os.path.join(ROOT, "msvc", "bin", "Base-Release-x64", "ugrep-indexer.exe")
if "--exe" in sys.argv:
    EXE = sys.argv[sys.argv.index("--exe") + 1]
if "--out" in sys.argv:
    OUT = sys.argv[sys.argv.index("--out") + 1]


def raw(v):
    return bytes.fromhex(v["hex"]) if isinstance(v, dict) else v.encode("utf-8")


def record():
    lines = []
    for line in open(CASES, encoding="utf-8"):
        if not line.strip():
            continue
        c = json.loads(line)
        d = tempfile.mkdtemp()
        for p, v in sorted(c.get("files", {}).items()):
            fp = os.path.join(d, p)
            os.makedirs(os.path.dirname(fp), exist_ok=True)
            open(fp, "wb").write(raw(v))
        env = {"LC_ALL": "C", "TZ": "UTC", "HOME": d, "USERPROFILE": d, "SystemRoot": os.environ.get("SystemRoot", "")}
        subprocess.run([EXE] + c["args"], cwd=d, env=env, capture_output=True, timeout=60)
        for root, _, fs in sorted(os.walk(d)):
            for f in sorted(fs):
                if f != "._UG#_Store":
                    continue
                p = os.path.join(root, f)
                b = open(p, "rb").read()
                lines.append(json.dumps({
                    "case": c["id"], "path": os.path.relpath(p, d).replace("\\", "/"), "size": len(b),
                    "magic": "UG#\\x03" if b[:4] == b"UG#\x03" else "other", "header_hex": b[:16].hex(),
                    "sha256": hashlib.sha256(b).hexdigest(), "hex": b.hex()}, separators=(",", ":")))
    return "\n".join(lines) + "\n"


text = record()
if "--check" in sys.argv:
    same = open(OUT, encoding="utf-8").read() == text
    print("index bytes stable:", same)
    sys.exit(0 if same else 1)
os.makedirs(os.path.dirname(OUT), exist_ok=True)
open(OUT, "w", encoding="utf-8", newline="\n").write(text)
print("recorded", text.count("\n"), "index files")
