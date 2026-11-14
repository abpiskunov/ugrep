#!/usr/bin/env python3
"""Writes migration/corpus/src_leaves/seam/zopen.cases.jsonl, the seam cases of the zopen unit.

Inputs only. Write cases are authored here; the compressed streams that the read cases decompress
are produced by running the C++ (zopen_driver, write mode) and taking the bytes it left in the FILE;
hand-crafted streams are inputs too. No expected value is typed: the traces are recorded from the C++
by capture.sh.

Usage: python gen_zopen_cases.py   (run from the repository root; builds the driver first)
"""
import json
import os
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
OUT_DIR = os.path.join(ROOT, "migration", "out", "src_leaves")
CASES = os.path.join(ROOT, "migration", "corpus", "src_leaves", "seam", "zopen.cases.jsonl")


def case(cid, mode="r", bits=0, plain=0, data=b"", ops=(), ro=False):
    c = {"id": "zo-" + cid, "mode": mode, "bits": bits, "plain": plain}
    if data:
        c["input_hex"] = data.hex()
    if ro:
        c["ro"] = True
    c["ops"] = list(ops)
    return c


def lzw9(codes, header=b"\x1f\x9d\x90"):
    """Packs 9-bit codes little-endian, as an input for hand-crafted streams."""
    acc, n, out = 0, 0, bytearray()
    for c in codes:
        acc |= c << n
        n += 9
        while n >= 8:
            out.append(acc & 0xFF)
            acc >>= 8
            n -= 8
    if n:
        out.append(acc & 0xFF)
    return header + bytes(out)


def write_cases():
    w = []
    w.append(case("w-open-close-only", "w"))
    w.append(case("w-zero-length", "w", ops=["w:"]))
    w.append(case("w-one-byte", "w", ops=["w:41"]))
    w.append(case("w-two-bytes", "w", ops=["w:4142"]))
    w.append(case("w-text-small", "w", ops=["g:text:300:1:300"]))
    w.append(case("w-text-chunks", "w", ops=["g:text:3000:2:97"]))
    w.append(case("w-zero-run", "w", ops=["g:zero:2000:0:2000"]))
    w.append(case("w-ramp", "w", ops=["g:ramp:1500:0:700"]))
    w.append(case("w-plain-no-magic", "w", plain=1, ops=["g:text:400:3:400"]))
    w.append(case("w-bits-12-rand", "w", bits=12, ops=["g:rand:22000:4:4096"]))
    w.append(case("w-bits-12-text-clear", "w", bits=12, ops=["g:text:40000:5:8192", "g:rand:15000:6:5000"]))
    w.append(case("w-bits-13-rand", "w", bits=13, ops=["g:rand:20000:7:6000"]))
    w.append(case("w-bits-16-rand-full", "w", bits=16, ops=["g:rand:70000:8:35000"]))
    w.append(case("w-bits-14-mixed", "w", bits=14, ops=["g:text:30000:9:10000", "g:rand:25000:10:12500"]))
    w.append(case("w-bits-0-default", "w", bits=0, ops=["g:text:1200:11:600"]))
    w.append(case("w-bits-11-below-min", "w", bits=11, ops=["g:text:800:12:800"]))
    w.append(case("w-bits-9", "w", bits=9, ops=["g:rand:3000:13:1000"]))
    w.append(case("w-bits-1", "w", bits=1, ops=["g:text:500:14:500"]))
    w.append(case("w-after-zero-write", "w", ops=["w:", "w:4142434445"]))
    w.append(case("w-on-read-handle", "r", ops=["w:414243"]))
    w.append(case("w-read-only-file", "w", ro=True, ops=["w:414243"]))
    w.append(case("w-read-only-file-plain", "w", plain=1, ops=["g:text:200:15:200"], ro=True))
    return w


def open_error_cases():
    e = []
    e.append(case("open-bad-mode-x", "x"))
    e.append(case("open-bad-mode-empty", ""))
    e.append(case("open-bad-mode-rb", "rb"))
    e.append(case("open-bad-mode-ww", "ww"))
    e.append(case("open-bad-mode-wplus", "w+"))
    e.append(case("open-bits-negative", "w", bits=-1))
    e.append(case("open-bits-17", "w", bits=17))
    e.append(case("open-bits-17-read", "r", bits=17))
    e.append(case("open-bits-16", "w", bits=16))
    e.append(case("open-uppercase-mode", "R"))
    return e


def run_driver(cases):
    os.makedirs(OUT_DIR, exist_ok=True)
    tmp = os.path.join(OUT_DIR, "gen.cases.jsonl")
    out = os.path.join(OUT_DIR, "gen.trace.jsonl")
    with open(tmp, "w", newline="\n") as f:
        for c in cases:
            f.write(json.dumps(c) + "\n")
    if os.path.exists(out):
        os.remove(out)
    env = dict(os.environ, CPP2RUST_TAP_OUT=out)
    subprocess.run([os.path.join(OUT_DIR, "zopen_driver.exe"), tmp], check=True, cwd=OUT_DIR, env=env)
    files = {}
    with open(out, encoding="ascii") as f:
        for line in f:
            e = json.loads(line)
            st = e.get("state")
            if e["kind"] == "state" and e["fn"] == "src_leaves.z_close" and isinstance(st, list) and len(st) == 2:
                files[e["case"]] = bytes(ord(ch) for ch in st[1])
    return files


def read_cases(streams):
    r = []
    text, text_z = "w-text-small", streams.get("zo-w-text-small", b"")
    r.append(case("r-empty-file", "r", ops=["r:10"]))
    r.append(case("r-one-byte-file", "r", data=b"\x1f", ops=["r:10"]))
    r.append(case("r-magic-only", "r", data=b"\x1f\x9d", ops=["r:10"]))
    r.append(case("r-bad-magic", "r", data=b"\x1f\x8b\x90abc", ops=["r:10"]))
    r.append(case("r-bad-magic-first-byte", "r", data=b"\x00\x9d\x90abc", ops=["r:10"]))
    r.append(case("r-maxbits-11", "r", data=b"\x1f\x9d\x8b\x00", ops=["r:10"]))
    r.append(case("r-maxbits-17", "r", data=b"\x1f\x9d\x91\x00", ops=["r:10"]))
    r.append(case("r-maxbits-0", "r", data=b"\x1f\x9d\x80", ops=["r:10"]))
    for b in (12, 13, 14, 15, 16):
        r.append(case("r-header-only-bits-%d" % b, "r", data=bytes([0x1F, 0x9D, 0x80 | b]),         ops=["r:10"]))
    r.append(case("r-header-only-no-block", "r", data=b"\x1f\x9d\x10", ops=["r:10"]))
    r.append(case("r-plain-empty", "r", plain=1, ops=["r:10"]))
    r.append(case("r-plain-header-only", "r", plain=1, data=b"\x90", ops=["r:10"]))
    r.append(case("r-plain-with-magic-present", "r", plain=1, data=text_z, ops=["r:10"]))
    r.append(case("r-len-zero", "r", data=text_z, ops=["r:0"]))
    # hand-crafted streams (9-bit codes): 'A' 'B' then ...
    A, B = 0x41, 0x42
    r.append(case("r-craft-ab", "r", data=lzw9([A, B]), ops=["r:10", "r:10"]))
    r.append(case("r-craft-kwkwk", "r", data=lzw9([A, 257]), ops=["r:10"]))
    r.append(case("r-craft-kwkwk-chain", "r", data=lzw9([A, 257, 258, 259]), ops=["r:20"]))
    r.append(case("r-craft-code-beyond-free", "r", data=lzw9([A, 300]), ops=["r:10"]))
    r.append(case("r-craft-code-just-beyond-free", "r", data=lzw9([A, 258]), ops=["r:10"]))
    r.append(case("r-craft-clear-then-code", "r", data=lzw9([A, 256, 258]), ops=["r:10"]))
    r.append(case("r-craft-clear-then-literal", "r", data=lzw9([A, 256, B, A]), ops=["r:10"]))
    r.append(case("r-craft-clear-then-free-ent", "r", data=lzw9([A, 256, 257]), ops=["r:10"]))
    r.append(case("r-craft-clear-first", "r", data=lzw9([256, A, B]), ops=["r:10"]))
    r.append(case("r-craft-clear-no-block", "r", data=lzw9([A, 256, B], header=b"\x1f\x9d\x10"), ops=["r:10"]))
    r.append(case("r-craft-no-block-ab", "r", data=lzw9([A, B, 256], header=b"\x1f\x9d\x10"), ops=["r:10"]))
    r.append(case("r-craft-plain-ab", "r", plain=1, data=lzw9([A, B], header=b"\x90"), ops=["r:10"]))
    r.append(case("r-craft-trailing-partial", "r", data=lzw9([A, B]) + b"\x01", ops=["r:10", "r:10"]))
    r.append(case("r-craft-one-code-one-byte", "r", data=lzw9([A])[:4], ops=["r:10"]))
    for cid, z in sorted(streams.items()):
        short = cid[len("zo-w-"):]
        if short in ("open-close-only", "zero-length", "read-only-file", "read-only-file-plain", "on-read-handle", "bits-11-below-min", "bits-1", "bits-9", "then-read-on-write-handle", "after-zero-write", "plain-no-magic"):
            continue
        if not z:
            continue
        r.append(case("r-stream-%s-all" % short, "r", data=z, ops=["ra:4096:100000"]))
        r.append(case("r-stream-%s-tiny-reads" % short, "r", data=z[:3000], ops=["ra:7:600"]))
    r.append(case("r-stream-text-small-eof-repeat", "r", data=text_z, ops=["r:1000", "r:1000", "r:1"]))
    r.append(case("r-stream-text-small-one-byte", "r", data=text_z, ops=["ra:1:400"]))
    for n in (3, 4, 5, 6, 10, 40, 100, len(text_z) - 1):
        if 0 < n < len(text_z):
            r.append(case("r-stream-text-small-cut-%d" % n, "r", data=text_z[:n], ops=["ra:64:100"]))
    r.append(case("r-stream-text-small-corrupt-mid", "r", data=text_z[:20] + b"\xff\xff" + text_z[22:], ops=["ra:64:100"]))
    plain_z = streams.get("zo-w-plain-no-magic", b"")
    if plain_z:
        r.append(case("r-stream-plain-all", "r", plain=1, data=plain_z, ops=["ra:128:100"]))
        r.append(case("r-stream-plain-read-as-magic", "r", plain=0, data=plain_z, ops=["r:10"]))
    for cid, z in sorted(streams.items()):
        if cid in ("zo-w-bits-1", "zo-w-bits-9", "zo-w-bits-11-below-min") and z:
            r.append(case("r-stream-%s" % cid[len("zo-w-"):], "r", data=z, ops=["r:100"]))
    zr = streams.get("zo-w-zero-run", b"")
    if zr:
        r.append(case("r-stream-zero-run-small-reads", "r", data=zr, ops=["ra:3:100"]))
    return r


def main():
    subprocess.run(["powershell", "-NoProfile", "-File", os.path.join(ROOT, "migration", "harness", "src_leaves", "build-zopen-driver.ps1")], check=True, stdout=subprocess.DEVNULL)
    writes = write_cases()
    streams = run_driver(writes)
    cases = open_error_cases() + writes + read_cases(streams)
    ids = [c["id"] for c in cases]
    assert len(ids) == len(set(ids)), "duplicate case ids"
    os.makedirs(os.path.dirname(CASES), exist_ok=True)
    with open(CASES, "w", newline="\n") as f:
        for c in cases:
            f.write(json.dumps(c) + "\n")
    print("wrote %d cases to %s" % (len(cases), CASES))


if __name__ == "__main__":
    sys.exit(main())
