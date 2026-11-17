"""Appends the .br (brotli) and .bz3 (bzip3) surface cases of the zstream corpus (D-S6-1) to
migration/corpus/src_leaves/zstream/cases.jsonl: inputs only. The expected observations are recorded by
`cpp2rust-tools run-cases` against the codec-enabled C++ reference msvc/bin/CodecRef-x64/ugrep.exe (built by
build-codec-ref.ps1 with brotli 1.1.0 and bzip3 1.5.1).

Append-only: existing lines are never rewritten; a case id that is already present is skipped, so a second run adds
nothing. Run from the repository root after build-zenc-br-bz3.ps1:
    python migration/harness/zstream/gen_br_bz3_cases.py [--out FILE]   (default: append to the corpus; with --out,
    write only the new cases to FILE, which is how they are recorded before being appended)
"""
import json
import os
import struct
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
ENC = os.path.join(ROOT, "migration", "out", "zstream", "zenc_br_bz3.exe")
CORPUS = os.path.join(ROOT, "migration", "corpus", "src_leaves", "zstream", "cases.jsonl")

TEXT = b"alpha foo one\nbeta two\nfoo gamma foo\ndelta\nlast foo line\n"
TEXT2 = b"second foo member\nnothing here\n"


def lcg_text(lines, every):
    out, s = [], 12345
    for i in range(lines):
        s = (s * 1103515245 + 12345) & 0x7FFFFFFF
        out.append(("line %d %08x%s\n" % (i, s, " foo" if i % every == 0 else "")).encode())
    return b"".join(out)


BIG = lcg_text(9000, 997)  # ~200 KB: several 64 KiB decoder buffers and, with the 65 KiB bzip3 block size, three blocks
BS = 66560  # 65 KiB, the smallest bzip3 block size accepted by ugrep


def enc(kind, data, *extra):
    return subprocess.run([ENC, kind, *map(str, extra)], input=data, capture_output=True, check=True).stdout


def br(data):
    return enc("br", data)


def bz3(data, bs=BS):
    return enc("bz3", data, bs)


def hexf(d):
    return {"hex": d.hex()}


def flip(d, i, mask=0xFF):
    b = bytearray(d)
    b[i] ^= mask
    return bytes(b)


def u32(v):
    return struct.pack("<I", v)


def tar(entries):
    import io
    import tarfile
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w", format=tarfile.PAX_FORMAT) as t:
        for n, d in entries:
            ti = tarfile.TarInfo(n)
            ti.size = len(d)
            ti.mtime = 1577934246
            t.addfile(ti, io.BytesIO(d))
    return buf.getvalue()


cases = []


def case(cid, files, args=None, **kw):
    c = {"id": "zs-" + cid, "args": args if args is not None else ["-z", "foo"] + list(files), "files": {k: hexf(v) for k, v in files.items()}}
    c.update(kw)
    cases.append(c)


# ---- brotli (.br): no magic bytes, so every error case is a decoder error
b = br(TEXT)
case("br-basic", {"a.br": b})
case("br-count", {"a.br": b}, ["-z", "-c", "foo", "a.br"])
case("br-files-with-matches", {"a.br": b}, ["-z", "-l", "foo", "a.br"])
case("br-line-numbers", {"a.br": b}, ["-z", "-n", "-o", "foo", "a.br"])
case("br-no-match", {"a.br": b}, ["-z", "zzz", "a.br"])
case("br-without-z", {"a.br": b}, ["foo", "a.br"])
case("br-context", {"a.br": b}, ["-z", "-C1", "gamma", "a.br"])
case("br-invert", {"a.br": b}, ["-z", "-v", "foo", "a.br"])
case("br-no-extension", {"data": b})
case("br-wrong-extension-gz", {"a.gz": b})
case("br-wrong-extension-txt", {"a.txt": b})
case("br-multi-stream", {"a.br": b + br(TEXT2)})
case("br-three-streams-empty-middle", {"a.br": b + br(b"") + br(TEXT2)})
case("br-empty-stream", {"a.br": br(b"")})
case("br-zero-length-file", {"a.br": b""})
case("br-single-zero-byte", {"a.br": b"\x00"})
case("br-large", {"a.br": br(BIG)}, ["-z", "-c", "foo", "a.br"])
case("br-large-lines", {"a.br": br(BIG)}, ["-z", "-n", "foo", "a.br"])
case("br-large-multi-stream", {"a.br": br(BIG) + br(BIG)}, ["-z", "-c", "foo", "a.br"])
case("br-trunc-half", {"a.br": b[: len(b) // 2]})
case("br-trunc-one-byte", {"a.br": b[:1]})
case("br-trunc-last-byte", {"a.br": b[:-1]})
case("br-trunc-large", {"a.br": br(BIG)[:70000]}, ["-z", "-c", "foo", "a.br"])
case("br-trunc-second-stream", {"a.br": b + br(TEXT2)[:5]})
case("br-corrupt-body", {"a.br": flip(b, 4)})
case("br-corrupt-header", {"a.br": flip(b, 0)})
case("br-corrupt-large", {"a.br": flip(br(BIG), 30000)}, ["-z", "-c", "foo", "a.br"])
case("br-bad-text", {"a.br": TEXT})
case("br-bad-ff", {"a.br": b"\xff" * 32})
case("br-trailing-garbage", {"a.br": b + b"garbage foo trailer"})
case("br-trailing-zeros", {"a.br": b + b"\0" * 7})
case("br-two-files", {"a.br": b, "b.br": br(TEXT2)}, ["-z", "-J1", "--sort=name", "foo", "a.br", "b.br"])
case("br-mixed-plain-and-br", {"a.br": b, "b.txt": TEXT2}, ["-z", "-J1", "--sort=name", "foo", "a.br", "b.txt"])
case("br-recursive", {"d/a.br": b, "d/b.txt": TEXT2, "d/c.br": br(TEXT2)}, ["-z", "-r", "-J1", "--sort=name", "foo", "d"])
case("br-good-then-corrupt-file", {"a.br": b, "b.br": flip(br(TEXT2), 0)}, ["-z", "-J1", "--sort=name", "foo", "a.br", "b.br"])
case("br-binary-after-decompress", {"a.br": br(b"foo\x00bin\n")}, ["-z", "foo", "a.br"])
case("br-text-binary", {"a.br": br(b"foo\x00bin\n")}, ["-z", "-a", "foo", "a.br"])
case("br-zmax0", {"a.br": b}, ["-z", "--zmax=0", "foo", "a.br"])
case("br-ext-tbr", {"a.tbr": br(tar([("one.txt", TEXT)]))}, ["-z", "-J1", "foo", "a.tbr"])
case("tar-br-basic", {"a.tar.br": br(tar([("one.txt", TEXT), ("two.txt", TEXT2)]))}, ["-z", "-J1", "foo", "a.tar.br"])
case("tar-br-trunc", {"a.tar.br": br(tar([("one.txt", TEXT), ("two.txt", TEXT2)]))[:60]}, ["-z", "-J1", "foo", "a.tar.br"])
case("br-in-all-codecs-count", {"a.br": b, "b.bz3": bz3(TEXT2)}, ["-z", "-c", "-J1", "--sort=name", "foo", "a.br", "b.bz3"])

# ---- bzip3 (.bz3): "BZ3v1", u32 block size (65 KiB .. 511 MiB), blocks of u32 compressed size, u32 original size, data
z = bz3(TEXT)
case("bz3-basic", {"a.bz3": z})
case("bz3-count", {"a.bz3": z}, ["-z", "-c", "foo", "a.bz3"])
case("bz3-files-with-matches", {"a.bz3": z}, ["-z", "-l", "foo", "a.bz3"])
case("bz3-line-numbers", {"a.bz3": z}, ["-z", "-n", "-o", "foo", "a.bz3"])
case("bz3-no-match", {"a.bz3": z}, ["-z", "zzz", "a.bz3"])
case("bz3-without-z", {"a.bz3": z}, ["foo", "a.bz3"])
case("bz3-context", {"a.bz3": z}, ["-z", "-C1", "gamma", "a.bz3"])
case("bz3-invert", {"a.bz3": z}, ["-z", "-v", "foo", "a.bz3"])
case("bz3-no-extension", {"data": z})
case("bz3-wrong-extension-gz", {"a.gz": z})
case("bz3-wrong-extension-txt", {"a.txt": z})
case("bz3-multi-block", {"a.bz3": bz3(BIG)}, ["-z", "-c", "foo", "a.bz3"])
case("bz3-multi-block-lines", {"a.bz3": bz3(BIG)}, ["-z", "-n", "foo", "a.bz3"])
case("bz3-block-size-2-blocks", {"a.bz3": bz3(BIG, 131072)}, ["-z", "-c", "foo", "a.bz3"])
case("bz3-block-size-1-block", {"a.bz3": bz3(BIG, 1 << 20)}, ["-z", "-c", "foo", "a.bz3"])
case("bz3-block-boundary", {"a.bz3": bz3(BIG[:BS * 2])}, ["-z", "-c", "foo", "a.bz3"])
case("bz3-tiny-last-block", {"a.bz3": bz3(BIG[:BS + 5])}, ["-z", "-c", "foo", "a.bz3"])
case("bz3-concatenated-files", {"a.bz3": z + bz3(TEXT2)})
case("bz3-header-only", {"a.bz3": bz3(b"")})
case("bz3-trunc-block-header", {"a.bz3": z[:13]})
case("bz3-trunc-half", {"a.bz3": z[: len(z) // 2]})
case("bz3-trunc-last-byte", {"a.bz3": z[:-1]})
case("bz3-trunc-large", {"a.bz3": bz3(BIG)[:70000]}, ["-z", "-c", "foo", "a.bz3"])
case("bz3-trunc-second-block", {"a.bz3": bz3(BIG)[:80000]}, ["-z", "-c", "foo", "a.bz3"])
case("bz3-corrupt-body", {"a.bz3": flip(z, 20)})
case("bz3-corrupt-large", {"a.bz3": flip(bz3(BIG), 30000)}, ["-z", "-c", "foo", "a.bz3"])
case("bz3-block-size-too-small", {"a.bz3": z[:5] + u32(BS - 1) + z[9:]})
case("bz3-block-size-zero", {"a.bz3": z[:5] + u32(0) + z[9:]})
case("bz3-block-size-too-large", {"a.bz3": z[:5] + u32((511 << 20) + 1) + z[9:]})
case("bz3-block-size-min-accepted", {"a.bz3": z[:5] + u32(BS) + z[9:]})
case("bz3-compressed-size-over-max", {"a.bz3": z[:9] + u32(0x7FFFFFFF) + z[13:]})
case("bz3-original-size-over-max", {"a.bz3": z[:13] + u32(0x7FFFFFFF) + z[17:]})
case("bz3-original-size-zero", {"a.bz3": z[:13] + u32(0) + z[17:]})
case("bz3-original-size-smaller", {"a.bz3": z[:13] + u32(len(TEXT) - 10) + z[17:]})
case("bz3-original-size-larger", {"a.bz3": z[:13] + u32(len(TEXT) + 10) + z[17:]})
case("bz3-bad-text", {"a.bz3": TEXT})
case("bz3-trailing-garbage", {"a.bz3": z + b"garbage foo trailer"})
case("bz3-trailing-zeros", {"a.bz3": z + b"\0" * 7})
case("bz3-trailing-seven-bytes", {"a.bz3": z + b"1234567"})
case("bz3-two-files", {"a.bz3": z, "b.bz3": bz3(TEXT2)}, ["-z", "-J1", "--sort=name", "foo", "a.bz3", "b.bz3"])
case("bz3-mixed-plain-and-bz3", {"a.bz3": z, "b.txt": TEXT2}, ["-z", "-J1", "--sort=name", "foo", "a.bz3", "b.txt"])
case("bz3-recursive", {"d/a.bz3": z, "d/b.txt": TEXT2, "d/c.bz3": bz3(TEXT2)}, ["-z", "-r", "-J1", "--sort=name", "foo", "d"])
case("bz3-binary-after-decompress", {"a.bz3": bz3(b"foo\x00bin\n")}, ["-z", "foo", "a.bz3"])
case("bz3-text-binary", {"a.bz3": bz3(b"foo\x00bin\n")}, ["-z", "-a", "foo", "a.bz3"])
case("bz3-zmax0", {"a.bz3": z}, ["-z", "--zmax=0", "foo", "a.bz3"])
case("bz3-ext-tbz3", {"a.tbz3": bz3(tar([("one.txt", TEXT)]))}, ["-z", "-J1", "foo", "a.tbz3"])
case("tar-bz3-basic", {"a.tar.bz3": bz3(tar([("one.txt", TEXT), ("two.txt", TEXT2)]))}, ["-z", "-J1", "foo", "a.tar.bz3"])
case("tar-bz3-trunc", {"a.tar.bz3": bz3(tar([("one.txt", TEXT), ("two.txt", TEXT2)]))[:60]}, ["-z", "-J1", "foo", "a.tar.bz3"])

# Not admitted (found by exploration, see migration/behavior-zstream.md): the C++ header check clears file_ and falls through.
#   crash, Windows fast-fail exit 0xC0000409: bz3-corrupt-magic, bz3-bad-version, bz3-bad-magic-long-text, bz3-good-then-corrupt-file
#   result depends on stale bytes of the 9-byte header buffer (fewer than 9 bytes read): bz3-zero-length-file, bz3-trunc-magic, bz3-trunc-block-size


def main():
    if not os.path.exists(ENC):
        sys.exit("build migration/out/zstream/zenc_br_bz3.exe first (build-zenc-br-bz3.ps1)")
    ids = [c["id"] for c in cases]
    assert len(ids) == len(set(ids)), "duplicate ids"
    if "--out" in sys.argv:
        out = sys.argv[sys.argv.index("--out") + 1]
        new = cases
    else:
        out = CORPUS
        have = set()
        with open(CORPUS, encoding="utf-8") as f:
            for line in f:
                have.add(json.loads(line)["id"])
        new = [c for c in cases if c["id"] not in have]
    with open(out, "a" if out == CORPUS else "w", newline="\n") as f:
        for c in new:
            f.write(json.dumps(c, sort_keys=False, separators=(",", ":")) + "\n")
    print(len(new), "cases ->", out)


if __name__ == "__main__":
    main()
