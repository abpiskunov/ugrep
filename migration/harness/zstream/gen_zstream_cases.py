"""Generates migration/corpus/src_leaves/zstream/cases.jsonl: surface cases of the archive and compression
surface of zstream.hpp, run through the codec-enabled C++ reference (ugrep -z). Inputs only; the expected
observations are recorded by `cpp2rust-tools run-cases` against msvc/bin/CodecRef-x64/ugrep.exe.

Codecs built in the reference: zlib (gzip, zip deflate), bzip2, lzma/xz, lz4, zstd, 7z (lzma/C), .Z (zopen),
tar/zip. Brotli and bzip3 are not built, no case exists for them.

Run from the repository root after build-zenc.ps1:  python migration/harness/zstream/gen_zstream_cases.py
"""
import bz2
import gzip
import io
import json
import lzma
import os
import struct
import subprocess
import sys
import tarfile
import zipfile
import zlib

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
ZENC = os.path.join(ROOT, "migration", "out", "zstream", "zenc.exe")
OUT = os.path.join(ROOT, "migration", "corpus", "src_leaves", "zstream", "cases.jsonl")

TEXT = b"alpha foo one\nbeta two\nfoo gamma foo\ndelta\nlast foo line\n"
TEXT2 = b"second foo member\nnothing here\n"


def lcg_text(lines, every):
    out, s = [], 12345
    for i in range(lines):
        s = (s * 1103515245 + 12345) & 0x7FFFFFFF
        out.append(("line %d %08x%s\n" % (i, s, " foo" if i % every == 0 else "")).encode())
    return b"".join(out)


BIG = lcg_text(9000, 997)  # ~200 KB, crosses several 64 KiB decoder buffers


def zenc(kind, data):
    return subprocess.run([ZENC, kind], input=data, capture_output=True, check=True).stdout


def gz(data, name=None, extra=None, mtime=0):
    flg = 0
    hdr = b""
    if extra is not None:
        flg |= 4
        hdr += struct.pack("<H", len(extra)) + extra
    if name is not None:
        flg |= 8
        hdr += name + b"\0"
    c = zlib.compressobj(6, zlib.DEFLATED, -15)
    body = c.compress(data) + c.flush()
    return b"\x1f\x8b\x08" + bytes([flg]) + struct.pack("<I", mtime) + b"\x00\x03" + hdr + body + struct.pack("<II", zlib.crc32(data), len(data) & 0xFFFFFFFF)


def xz(data, check=lzma.CHECK_CRC64):
    return lzma.compress(data, format=lzma.FORMAT_XZ, check=check)


def lzma_alone(data):
    return lzma.compress(data, format=lzma.FORMAT_ALONE)


def lzw9(codes, header=b"\x1f\x9d\x90"):
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


def z_literals(data):
    return lzw9(list(data))


def make_zip(entries, method=zipfile.ZIP_DEFLATED, stream=False):
    buf = io.BytesIO()
    if stream:
        class NoSeek(io.RawIOBase):
            def __init__(self):
                self.b = io.BytesIO()
            def writable(self):
                return True
            def write(self, d):
                return self.b.write(d)
            def seekable(self):
                return False
            def tell(self):
                return self.b.tell()
        ns = NoSeek()
        z = zipfile.ZipFile(ns, "w", method)
        for n, d in entries:
            with z.open(zipfile.ZipInfo(n, (2020, 1, 2, 3, 4, 6)), "w") as f:
                f.write(d)
        z.close()
        return ns.b.getvalue()
    z = zipfile.ZipFile(buf, "w", method)
    for n, d in entries:
        zi = zipfile.ZipInfo(n, (2020, 1, 2, 3, 4, 6))
        zi.compress_type = method
        z.writestr(zi, d)
    z.close()
    return buf.getvalue()


def num7z(v):
    if v < 0x80:
        return bytes([v])
    for n in range(1, 9):
        if v < (1 << (7 * (n + 1))):
            mask = (0xFF << (8 - n)) & 0xFF
            return bytes([mask | (v >> (8 * n))]) + (v & ((1 << (8 * n)) - 1)).to_bytes(n, "little")
    return b"\xff" + v.to_bytes(8, "little")


def make_7z(files, method="copy", claim_usize=None, corrupt_crc=False):
    unpacked = b"".join(d for _, d in files)
    if method == "copy":
        packed, coder = unpacked, b"\x01\x00"
    else:
        f = [{"id": lzma.FILTER_LZMA2, "dict_size": 2 << 20}]
        packed = lzma.compress(unpacked, format=lzma.FORMAT_RAW, filters=f)
        coder = b"\x21\x21\x01\x12"
    usize = len(unpacked) if claim_usize is None else claim_usize
    crc = zlib.crc32(unpacked) ^ (1 if corrupt_crc else 0)
    h = b"\x01\x04"
    h += b"\x06" + num7z(0) + num7z(1) + b"\x09" + num7z(len(packed)) + b"\x00"
    h += b"\x07\x0b" + num7z(1) + b"\x00" + num7z(1) + coder + b"\x0c" + num7z(usize)
    h += b"\x0a\x01" + struct.pack("<I", crc) + b"\x00"
    h += b"\x08"
    if len(files) > 1:
        h += b"\x0d" + num7z(len(files)) + b"\x09" + b"".join(num7z(len(d)) for _, d in files[:-1])
        h += b"\x0a\x01" + b"".join(struct.pack("<I", zlib.crc32(d)) for _, d in files)
    h += b"\x00\x00"
    names = b"".join(n.encode("utf-16-le") + b"\0\0" for n, _ in files)
    h += b"\x05" + num7z(len(files)) + b"\x11" + num7z(len(names) + 1) + b"\x00" + names + b"\x00"
    h += b"\x00"
    start = struct.pack("<QQI", len(packed), len(h), zlib.crc32(h))
    return b"7z\xbc\xaf\x27\x1c\x00\x04" + struct.pack("<I", zlib.crc32(start)) + start + packed + h


def make_tar(entries, fmt=tarfile.PAX_FORMAT):
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w", format=fmt) as t:
        for n, d in entries:
            ti = tarfile.TarInfo(n)
            ti.size = len(d)
            ti.mtime = 1577934246
            t.addfile(ti, io.BytesIO(d))
    return buf.getvalue()


def hexf(d):
    return {"hex": d.hex()}


cases = []


def case(cid, files, args=None, **kw):
    c = {"id": "zs-" + cid, "args": args if args is not None else ["-z", "foo"] + list(files), "files": {k: hexf(v) for k, v in files.items()}}
    c.update(kw)
    cases.append(c)


def flip(d, i, mask=0xFF):
    b = bytearray(d)
    b[i] ^= mask
    return bytes(b)


# ---- gzip (zlib)
g = gz(TEXT)
case("gz-basic", {"a.gz": g})
case("gz-count", {"a.gz": g}, ["-z", "-c", "foo", "a.gz"])
case("gz-files-with-matches", {"a.gz": g}, ["-z", "-l", "foo", "a.gz"])
case("gz-line-numbers", {"a.gz": g}, ["-z", "-n", "-o", "foo", "a.gz"])
case("gz-no-match", {"a.gz": g}, ["-z", "zzz", "a.gz"])
case("gz-without-z", {"a.gz": g}, ["foo", "a.gz"])
case("gz-multi-member", {"a.gz": g + gz(TEXT2)})
case("gz-three-members-empty-middle", {"a.gz": g + gz(b"") + gz(TEXT2)})
case("gz-empty-member", {"a.gz": gz(b"")})
case("gz-zero-length-file", {"a.gz": b""})
case("gz-fname-header", {"a.gz": gz(TEXT, name=b"orig.txt")})
case("gz-extra-header", {"a.gz": gz(TEXT, name=b"orig.txt", extra=b"AB\x02\x00xy")})
case("gz-mtime-set", {"a.gz": gz(TEXT, mtime=1577934246)})
case("gz-no-extension", {"data": g})
case("gz-wrong-extension", {"a.txt": g})
case("gz-large", {"a.gz": gz(BIG)}, ["-z", "-c", "foo", "a.gz"])
case("gz-large-lines", {"a.gz": gz(BIG)}, ["-z", "-n", "foo", "a.gz"])
case("gz-trailing-garbage", {"a.gz": g + b"garbage foo trailer"})
case("gz-trailing-zeros", {"a.gz": g + b"\0" * 7})
case("gz-trunc-half", {"a.gz": g[: len(g) // 2]})
case("gz-trunc-header", {"a.gz": g[:5]})
case("gz-trunc-trailer", {"a.gz": g[:-4]})
case("gz-trunc-large", {"a.gz": gz(BIG)[:70000]}, ["-z", "-c", "foo", "a.gz"])
case("gz-bad-crc", {"a.gz": flip(g, len(g) - 8)})
case("gz-bad-length", {"a.gz": flip(g, len(g) - 1)})
case("gz-corrupt-body", {"a.gz": flip(g, 14)})
case("gz-bad-magic-text", {"a.gz": TEXT})
case("gz-bad-method", {"a.gz": flip(g, 2, 0x01)})
case("gz-second-member-corrupt", {"a.gz": g + flip(gz(TEXT2), 14)})
case("gz-two-files", {"a.gz": g, "b.gz": gz(TEXT2)}, ["-z", "-J1", "--sort=name", "foo", "a.gz", "b.gz"])
case("gz-mixed-plain-and-gz", {"a.gz": g, "b.txt": TEXT2}, ["-z", "-J1", "--sort=name", "foo", "a.gz", "b.txt"])
case("gz-recursive", {"d/a.gz": g, "d/b.txt": TEXT2, "d/c.gz": gz(TEXT2)}, ["-z", "-r", "-J1", "--sort=name", "foo", "d"])
case("gz-stdin", {"in.gz": g}, ["-z", "foo"], stdin="")

# ---- bzip2
b = bz2.compress(TEXT)
case("bz2-basic", {"a.bz2": b})
case("bz2-count", {"a.bz2": b}, ["-z", "-c", "foo", "a.bz2"])
case("bz2-multi-stream", {"a.bz2": b + bz2.compress(TEXT2)})
case("bz2-empty-stream", {"a.bz2": bz2.compress(b"")})
case("bz2-zero-length-file", {"a.bz2": b""})
case("bz2-large", {"a.bz2": bz2.compress(BIG)}, ["-z", "-c", "foo", "a.bz2"])
case("bz2-trunc-half", {"a.bz2": b[: len(b) // 2]})
case("bz2-trunc-header", {"a.bz2": b[:3]})
case("bz2-trunc-large", {"a.bz2": bz2.compress(BIG)[:5000]}, ["-z", "-c", "foo", "a.bz2"])
case("bz2-corrupt-body", {"a.bz2": flip(b, 20)})
case("bz2-corrupt-crc", {"a.bz2": flip(b, len(b) - 3)})
case("bz2-bad-magic-text", {"a.bz2": TEXT})
case("bz2-trailing-garbage", {"a.bz2": b + b"garbage foo trailer"})
case("bz2-ext-bzip2", {"a.bzip2": b})
case("bz2-ext-tbz", {"a.tbz": b})
case("bz2-wrong-extension-gz", {"a.gz": b})

# ---- xz / lzma (liblzma, memory limit 100 MB)
x = xz(TEXT)
case("xz-basic", {"a.xz": x})
case("xz-count", {"a.xz": x}, ["-z", "-c", "foo", "a.xz"])
case("xz-multi-stream", {"a.xz": x + xz(TEXT2)})
case("xz-multi-stream-padding", {"a.xz": x + b"\0" * 4 + xz(TEXT2)})
case("xz-empty-stream", {"a.xz": xz(b"")})
case("xz-zero-length-file", {"a.xz": b""})
case("xz-check-none", {"a.xz": xz(TEXT, lzma.CHECK_NONE)})
case("xz-check-crc32", {"a.xz": xz(TEXT, lzma.CHECK_CRC32)})
case("xz-check-sha256", {"a.xz": xz(TEXT, lzma.CHECK_SHA256)})
case("xz-large", {"a.xz": xz(BIG)}, ["-z", "-c", "foo", "a.xz"])
case("xz-trunc-half", {"a.xz": x[: len(x) // 2]})
case("xz-trunc-header", {"a.xz": x[:7]})
case("xz-trunc-footer", {"a.xz": x[:-6]})
case("xz-trunc-large", {"a.xz": xz(BIG)[:3000]}, ["-z", "-c", "foo", "a.xz"])
case("xz-corrupt-body", {"a.xz": flip(x, 20)})
case("xz-corrupt-check", {"a.xz": flip(x, len(x) - 14)})
case("xz-bad-magic-text", {"a.xz": TEXT})
case("xz-trailing-garbage", {"a.xz": x + b"garbage foo trailer"})
case("xz-lzma-alone", {"a.lzma": lzma_alone(TEXT)})
case("xz-lzma-alone-as-xz", {"a.xz": lzma_alone(TEXT)})
case("xz-lzma-alone-trunc", {"a.lzma": lzma_alone(TEXT)[:20]})
case("xz-ext-txz", {"a.txz": x})
# memory limit: dictionary of 1 GiB declared in the LZMA2 filter, a few bytes of data; xz memlimit of the reference is 100 MB
big_dict = lzma.compress(TEXT, format=lzma.FORMAT_XZ, check=lzma.CHECK_CRC32, filters=[{"id": lzma.FILTER_LZMA2, "dict_size": 1 << 30, "preset": 0}])
case("xz-memlimit-1g-dict", {"a.xz": big_dict})
mid_dict = lzma.compress(TEXT, format=lzma.FORMAT_XZ, check=lzma.CHECK_CRC32, filters=[{"id": lzma.FILTER_LZMA2, "dict_size": 64 << 20, "preset": 0}])
case("xz-memlimit-64m-dict", {"a.xz": mid_dict})
case("xz-memlimit-lzma-alone-1g", {"a.lzma": lzma.compress(TEXT, format=lzma.FORMAT_ALONE, filters=[{"id": lzma.FILTER_LZMA1, "dict_size": 1 << 30, "preset": 0}])})

# ---- lz4
l4 = zenc("lz4", TEXT)
case("lz4-basic", {"a.lz4": l4})
case("lz4-count", {"a.lz4": l4}, ["-z", "-c", "foo", "a.lz4"])
case("lz4-multi-frame", {"a.lz4": l4 + zenc("lz4", TEXT2)})
case("lz4-empty-frame", {"a.lz4": zenc("lz4", b"")})
case("lz4-zero-length-file", {"a.lz4": b""})
case("lz4-large", {"a.lz4": zenc("lz4", BIG)}, ["-z", "-c", "foo", "a.lz4"])
case("lz4-trunc-half", {"a.lz4": l4[: len(l4) // 2]})
case("lz4-trunc-header", {"a.lz4": l4[:5]})
case("lz4-trunc-large", {"a.lz4": zenc("lz4", BIG)[:30000]}, ["-z", "-c", "foo", "a.lz4"])
case("lz4-corrupt-body", {"a.lz4": flip(l4, 12)})
case("lz4-bad-magic-text", {"a.lz4": TEXT})
case("lz4-trailing-garbage", {"a.lz4": l4 + b"garbage foo trailer"})

# ---- zstd
zs = zenc("zstd", TEXT)
case("zstd-basic", {"a.zst": zs})
case("zstd-count", {"a.zst": zs}, ["-z", "-c", "foo", "a.zst"])
case("zstd-ext-zstd", {"a.zstd": zs})
case("zstd-multi-frame", {"a.zst": zs + zenc("zstd", TEXT2)})
case("zstd-empty-frame", {"a.zst": zenc("zstd", b"")})
case("zstd-zero-length-file", {"a.zst": b""})
case("zstd-large", {"a.zst": zenc("zstd", BIG)}, ["-z", "-c", "foo", "a.zst"])
case("zstd-trunc-half", {"a.zst": zs[: len(zs) // 2]})
case("zstd-trunc-header", {"a.zst": zs[:5]})
case("zstd-trunc-large", {"a.zst": zenc("zstd", BIG)[:20000]}, ["-z", "-c", "foo", "a.zst"])
case("zstd-corrupt-body", {"a.zst": flip(zs, 12)})
case("zstd-bad-magic-text", {"a.zst": TEXT})
case("zstd-trailing-garbage", {"a.zst": zs + b"garbage foo trailer"})
case("zstd-skippable-frame", {"a.zst": b"\x50\x2a\x4d\x18" + struct.pack("<I", 4) + b"abcd" + zs})

# ---- compress (.Z, LZW through zopen)
case("Z-basic", {"a.Z": z_literals(TEXT)})
case("Z-count", {"a.Z": z_literals(TEXT)}, ["-z", "-c", "foo", "a.Z"])
case("Z-dictionary-code", {"a.Z": lzw9([ord("f"), ord("o"), 257, ord("\n")])}, ["-z", "o", "a.Z"])
case("Z-header-only", {"a.Z": b"\x1f\x9d\x90"})
case("Z-zero-length-file", {"a.Z": b""})
case("Z-trunc-header", {"a.Z": b"\x1f\x9d"})
case("Z-trunc-body", {"a.Z": z_literals(TEXT)[:13]})
case("Z-bad-bits", {"a.Z": lzw9(list(TEXT), header=b"\x1f\x9d\x88")})
case("Z-bad-code", {"a.Z": lzw9([ord("f"), 400])})
case("Z-bad-magic-text", {"a.Z": TEXT})
case("Z-ext-taZ", {"a.taZ": z_literals(TEXT)})

# ---- zip
ent = [("one.txt", TEXT), ("two.txt", TEXT2), ("dir/three.txt", b"third foo\n")]
zp = make_zip(ent)
case("zip-basic-deflate", {"a.zip": zp}, ["-z", "-J1", "foo", "a.zip"])
case("zip-single", {"a.zip": make_zip(ent[:1])})
case("zip-stored", {"a.zip": make_zip(ent, zipfile.ZIP_STORED)}, ["-z", "-J1", "foo", "a.zip"])
case("zip-bzip2-method", {"a.zip": make_zip(ent, zipfile.ZIP_BZIP2)}, ["-z", "-J1", "foo", "a.zip"])
case("zip-lzma-method", {"a.zip": make_zip(ent, zipfile.ZIP_LZMA)}, ["-z", "-J1", "foo", "a.zip"])
case("zip-data-descriptor", {"a.zip": make_zip(ent, stream=True)}, ["-z", "-J1", "foo", "a.zip"])
case("zip-count", {"a.zip": zp}, ["-z", "-c", "-J1", "foo", "a.zip"])
case("zip-files-with-matches", {"a.zip": zp}, ["-z", "-l", "-J1", "foo", "a.zip"])
case("zip-line-numbers", {"a.zip": zp}, ["-z", "-n", "-J1", "foo", "a.zip"])
case("zip-no-match", {"a.zip": zp}, ["-z", "zzz", "a.zip"])
case("zip-glob-include", {"a.zip": zp}, ["-z", "-J1", "--include=*two*", "foo", "a.zip"])
case("zip-empty-archive", {"a.zip": make_zip([])})
case("zip-zero-length-file", {"a.zip": b""})
case("zip-empty-entry", {"a.zip": make_zip([("e.txt", b""), ("f.txt", TEXT)])}, ["-z", "-J1", "foo", "a.zip"])
case("zip-large-entry", {"a.zip": make_zip([("big.txt", BIG)])}, ["-z", "-c", "foo", "a.zip"])
case("zip-trunc-half", {"a.zip": zp[: len(zp) // 2]})
case("zip-trunc-header", {"a.zip": zp[:20]})
case("zip-trunc-first-entry", {"a.zip": zp[:60]})
case("zip-corrupt-deflate", {"a.zip": flip(zp, 45)})
zcrc = bytearray(zp)
zcrc[14:18] = b"\x01\x02\x03\x04"  # CRC32 of the first local header
case("zip-bad-crc32-header", {"a.zip": bytes(zcrc)})
zsz = bytearray(make_zip(ent, zipfile.ZIP_STORED))
zsz[22:26] = struct.pack("<I", 3)  # usize of the first local header smaller than the data
case("zip-bad-usize", {"a.zip": bytes(zsz)}, ["-z", "-J1", "foo", "a.zip"])
zenc_flag = bytearray(zp)
zenc_flag[6] |= 1  # encrypted flag of the first local header
case("zip-encrypted-flag", {"a.zip": bytes(zenc_flag)}, ["-z", "-J1", "foo", "a.zip"])
zmeth = bytearray(zp)
zmeth[8:10] = struct.pack("<H", 99)  # unsupported method
case("zip-unknown-method", {"a.zip": bytes(zmeth)}, ["-z", "-J1", "foo", "a.zip"])
case("zip-bad-magic-text", {"a.zip": TEXT})
case("zip-trailing-garbage", {"a.zip": zp + b"garbage foo trailer"})
case("zip-ext-ZIP", {"a.ZIP": zp}, ["-z", "-J1", "foo", "a.ZIP"])
case("zip-ext-zipx", {"a.zipx": zp}, ["-z", "-J1", "foo", "a.zipx"])
case("zip-unicode-name", {"a.zip": make_zip([("d\u00e9j\u00e0.txt", TEXT)])})
case("zip-in-gz-zmax2", {"a.zip.gz": gz(zp)}, ["-z", "--zmax=2", "-J1", "foo", "a.zip.gz"])
case("zip-in-gz-zmax1", {"a.zip.gz": gz(zp)}, ["-z", "-J1", "foo", "a.zip.gz"])
case("gz-in-zip-zmax2", {"a.zip": make_zip([("n.gz", gz(TEXT))], zipfile.ZIP_STORED)}, ["-z", "--zmax=2", "-J1", "foo", "a.zip"])
case("gz-in-zip-zmax1", {"a.zip": make_zip([("n.gz", gz(TEXT))], zipfile.ZIP_STORED)}, ["-z", "-J1", "foo", "a.zip"])

# ---- 7z (lzma/C viizip)
s7 = make_7z([("one.txt", TEXT)], "lzma2")
case("7z-lzma2-single", {"a.7z": s7})
case("7z-copy-single", {"a.7z": make_7z([("one.txt", TEXT)], "copy")})
case("7z-lzma2-two-files", {"a.7z": make_7z([("one.txt", TEXT), ("two.txt", TEXT2)], "lzma2")}, ["-z", "-J1", "foo", "a.7z"])
case("7z-copy-two-files", {"a.7z": make_7z([("one.txt", TEXT), ("two.txt", TEXT2)], "copy")}, ["-z", "-J1", "foo", "a.7z"])
case("7z-count", {"a.7z": make_7z([("one.txt", TEXT), ("two.txt", TEXT2)], "copy")}, ["-z", "-c", "-J1", "foo", "a.7z"])
case("7z-files-with-matches", {"a.7z": make_7z([("one.txt", TEXT), ("two.txt", TEXT2)], "copy")}, ["-z", "-l", "-J1", "foo", "a.7z"])
case("7z-large", {"a.7z": make_7z([("big.txt", BIG)], "lzma2")}, ["-z", "-c", "foo", "a.7z"])
case("7z-zero-length-file", {"a.7z": b""})
case("7z-signature-only", {"a.7z": s7[:32]})
case("7z-trunc-half", {"a.7z": s7[: len(s7) // 2]})
case("7z-trunc-header", {"a.7z": s7[:12]})
case("7z-trunc-end", {"a.7z": s7[:-3]})
case("7z-bad-signature", {"a.7z": flip(s7, 0)})
case("7z-bad-start-header-crc", {"a.7z": flip(s7, 8)})
case("7z-bad-next-header-crc", {"a.7z": flip(s7, 28)})
case("7z-corrupt-packed", {"a.7z": flip(s7, 34)})
case("7z-bad-file-crc", {"a.7z": make_7z([("one.txt", TEXT)], "copy", corrupt_crc=True)})
case("7z-bad-text", {"a.7z": TEXT})
case("7z-ext-7Z", {"a.7Z": s7})
# WITH_MAX_7ZIP_SIZE of ugrep.cpp is 1 GiB: the declared size of the archived file is compared with it
case("7z-size-at-limit", {"a.7z": make_7z([("one.txt", TEXT)], "copy", claim_usize=1 << 30)})
case("7z-size-over-limit", {"a.7z": make_7z([("one.txt", TEXT)], "copy", claim_usize=(1 << 30) + 1)})
case("7z-size-way-over-limit", {"a.7z": make_7z([("one.txt", TEXT)], "copy", claim_usize=1 << 40)})
case("7z-size-over-limit-second-ok", {"a.7z": make_7z([("one.txt", TEXT), ("two.txt", TEXT2)], "copy", claim_usize=(1 << 30) + 1)}, ["-z", "-J1", "foo", "a.7z"])

# ---- tar and tar with compression (ugrep -z)
tr = make_tar([("one.txt", TEXT), ("two.txt", TEXT2), ("sub/three.txt", b"third foo\n")])
case("tar-basic", {"a.tar": tr}, ["-z", "-J1", "foo", "a.tar"])
case("tar-count", {"a.tar": tr}, ["-z", "-c", "-J1", "foo", "a.tar"])
case("tar-ustar", {"a.tar": make_tar([("one.txt", TEXT)], tarfile.USTAR_FORMAT)})
case("tar-gnu-longname", {"a.tar": make_tar([("d/" + "x" * 120 + ".txt", TEXT)], tarfile.GNU_FORMAT)})
case("tar-pax-longname", {"a.tar": make_tar([("d/" + "y" * 120 + ".txt", TEXT)])})
case("tar-empty", {"a.tar": b"\0" * 1024})
case("tar-zero-length-file", {"a.tar": b""})
case("tar-trunc-half", {"a.tar": tr[: len(tr) // 2]}, ["-z", "-J1", "foo", "a.tar"])
case("tar-trunc-header", {"a.tar": tr[:300]})
case("tar-bad-checksum", {"a.tar": flip(tr, 150)})
case("tar-bad-text", {"a.tar": TEXT})
case("tgz-basic", {"a.tgz": gz(tr)}, ["-z", "-J1", "foo", "a.tgz"])
case("tar-gz-basic", {"a.tar.gz": gz(tr)}, ["-z", "-J1", "foo", "a.tar.gz"])
case("tar-gz-zmax1", {"a.tar.gz": gz(tr)}, ["-z", "--zmax=1", "-J1", "foo", "a.tar.gz"])
case("tar-gz-trunc", {"a.tar.gz": gz(tr)[:100]}, ["-z", "-J1", "foo", "a.tar.gz"])
case("tar-bz2-basic", {"a.tar.bz2": bz2.compress(tr)}, ["-z", "-J1", "foo", "a.tar.bz2"])
case("tar-xz-basic", {"a.tar.xz": xz(tr)}, ["-z", "-J1", "foo", "a.tar.xz"])
case("tar-lz4-basic", {"a.tar.lz4": zenc("lz4", tr)}, ["-z", "-J1", "foo", "a.tar.lz4"])
case("tar-zst-basic", {"a.tar.zst": zenc("zstd", tr)}, ["-z", "-J1", "foo", "a.tar.zst"])
case("tzst-ext", {"a.tzst": zenc("zstd", tr)}, ["-z", "-J1", "foo", "a.tzst"])

# ---- options of the decompression surface
case("opt-zmax0", {"a.gz": g}, ["-z", "--zmax=0", "foo", "a.gz"])
case("opt-zmax2-plain-gz", {"a.gz": g}, ["-z", "--zmax=2", "foo", "a.gz"])
case("opt-zmax-too-large", {"a.gz": g}, ["-z", "--zmax=100", "foo", "a.gz"])
case("opt-binary-after-decompress", {"a.gz": gz(b"foo\x00bin\n")}, ["-z", "foo", "a.gz"])
case("opt-text-binary", {"a.gz": gz(b"foo\x00bin\n")}, ["-z", "-a", "foo", "a.gz"])
case("opt-context", {"a.gz": g}, ["-z", "-C1", "gamma", "a.gz"])
case("opt-invert", {"a.gz": g}, ["-z", "-v", "foo", "a.gz"])
case("opt-count-multi-archives", {"a.gz": g, "b.bz2": bz2.compress(TEXT2), "c.xz": xz(TEXT), "d.zst": zs, "e.lz4": l4},
     ["-z", "-c", "-J1", "--sort=name", "foo", "a.gz", "b.bz2", "c.xz", "d.zst", "e.lz4"])
case("opt-recursive-all-codecs", {"d/a.gz": g, "d/b.bz2": bz2.compress(TEXT2), "d/c.xz": xz(TEXT), "d/d.zst": zs, "d/e.lz4": l4, "d/f.zip": zp, "d/g.7z": s7, "d/h.Z": z_literals(TEXT)},
     ["-z", "-r", "-J1", "--sort=name", "foo", "d"])
case("opt-no-z-recursive", {"d/a.gz": g, "d/b.txt": TEXT}, ["-r", "-J1", "--sort=name", "foo", "d"])
case("opt-glob-filter-archive-name", {"d/a.gz": g, "d/b.gz": gz(TEXT2)}, ["-z", "-r", "-J1", "--sort=name", "--include=a.gz", "foo", "d"])

if __name__ == "__main__":
    if not os.path.exists(ZENC):
        sys.exit("build migration/out/zstream/zenc.exe first (build-zenc.ps1)")
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    ids = [c["id"] for c in cases]
    assert len(ids) == len(set(ids)), "duplicate ids"
    with open(OUT, "w", newline="\n") as f:
        for c in cases:
            f.write(json.dumps(c, sort_keys=False, separators=(",", ":")) + "\n")
    print(len(cases), "cases ->", OUT)
