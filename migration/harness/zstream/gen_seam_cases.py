#!/usr/bin/env python3
"""Generates zstream seam cases (migration/corpus/src_leaves/zstream/seam/cases.jsonl) for zstream_driver."""
import bz2, gzip, io, json, lzma, os, zipfile

here = os.path.dirname(os.path.abspath(__file__))
out = os.path.join(here, "..", "..", "corpus", "src_leaves", "zstream", "seam", "cases.jsonl")
text = b"".join(b"line %d alpha beta\n" % i for i in range(200))
cases = []


def add(cid, path, data, ops):
    cases.append({"id": cid, "path": path, "input_hex": data.hex(), "ops": ops})


def zipof(method, name=b"a.txt"):
    b = io.BytesIO()
    with zipfile.ZipFile(b, "w", method) as z:
        z.writestr("a.txt", text)
        z.writestr("b.txt", b"second\n")
    return b.getvalue()


for n in ("a.bz2", "a.xz", "a.lz4", "a.zst", "a.br", "a.bz3", "a.Z", "a.zip", "a.7z", "a.rar", "a.txt", "a.tbz", "a.tgz", "a.tzst"):
    add("zss-pred-" + n.replace(".", "-"), n, b"", ["is:" + n])
add("zss-gz-all", "a.gz", gzip.compress(text), ["d", "na:4096:100", "c", "d"])
add("zss-gz-small", "a.gz", gzip.compress(text), ["na:7:1000"])
add("zss-gz-sgetn", "a.gz", gzip.compress(text), ["sg:300", "sg:300"])
add("zss-gz-trunc", "a.gz", gzip.compress(text)[:60], ["na:4096:10"])
add("zss-gz-garbage", "a.gz", gzip.compress(text) + b"garbage", ["na:4096:10"])
add("zss-gz-plain", "a.gz", text, ["na:4096:10"])
add("zss-gz-reopen", "a.gz", gzip.compress(text), ["n:100", "c", "o", "n:100"])
add("zss-bz2-all", "a.bz2", bz2.compress(text), ["na:4096:100"])
add("zss-bz2-trunc", "a.bz2", bz2.compress(text)[:40], ["na:4096:10"])
add("zss-xz-all", "a.xz", lzma.compress(text), ["na:4096:100"])
add("zss-xz-trunc", "a.xz", lzma.compress(text)[:40], ["na:4096:10"])
add("zss-lzma-all", "a.lzma", lzma.compress(text, format=lzma.FORMAT_ALONE), ["na:4096:100"])
add("zss-zip-loop", "a.zip", zipof(zipfile.ZIP_DEFLATED), ["Z:4096"])
add("zss-zip-stored", "a.zip", zipof(zipfile.ZIP_STORED), ["Z:4096"])
add("zss-zip-peek", "a.zip", zipof(zipfile.ZIP_DEFLATED), ["z", "p", "n:50", "p"])
add("zss-zip-trunc", "a.zip", zipof(zipfile.ZIP_DEFLATED)[:80], ["Z:4096"])
add("zss-zip-empty", "a.zip", b"", ["Z:4096"])
add("zss-empty-gz", "a.gz", b"", ["na:4096:10"])
add("zss-empty-xz", "a.xz", b"", ["na:4096:10"])

with open(out, "w", newline="\n") as f:
    for c in cases:
        c["ops"] = c["ops"]
        f.write(json.dumps(c, separators=(",", ":")) + "\n")
print(len(cases), "cases")
