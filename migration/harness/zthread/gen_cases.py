"""Generates migration/corpus/zthread/cases.jsonl: surface cases of the decompression thread of ugrep.cpp
(struct Zthread: -z, --zmax, zip/tar parts, cancellation, corrupt input). Inputs only; the expected
observations are recorded by `cpp2rust-tools run-cases` against the codec-enabled C++ reference
msvc/bin/CodecRef-x64/ugrep.exe (migration/harness/zstream/build-codec-ref.ps1).

Every case is single-threaded (-J1) and sorted (--sort=name) so output order is deterministic.
Run from the repository root:  python migration/harness/zthread/gen_cases.py
"""
import bz2
import gzip
import io
import json
import lzma
import os
import tarfile
import zipfile

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
OUT = os.path.join(ROOT, "migration", "corpus", "zthread", "cases.jsonl")

T1 = b"alpha foo one\nbeta two\nfoo gamma foo\ndelta\nlast foo line\n"
T2 = b"second foo member\nnothing here\n"
T3 = b"third member\nhas no match\n"
BIN = b"foo\x00\x01\x02binary\xff\n"


def lcg(lines, every):
    out, s = [], 12345
    for i in range(lines):
        s = (s * 1103515245 + 12345) & 0x7FFFFFFF
        out.append(("line %d %08x%s\n" % (i, s, " foo" if i % every == 0 else "")).encode())
    return b"".join(out)


BIG = lcg(30000, 997)  # ~700 KB, crosses many pipe and decoder buffers


def gz(d):
    return gzip.compress(d, mtime=0)


def tar(members, fmt=tarfile.USTAR_FORMAT):
    b = io.BytesIO()
    with tarfile.open(fileobj=b, mode="w", format=fmt) as t:
        for n, c in members:
            i = tarfile.TarInfo(n)
            i.mtime = 0
            if c is None:
                i.type = tarfile.DIRTYPE
                t.addfile(i)
            else:
                i.size = len(c)
                t.addfile(i, io.BytesIO(c))
    return b.getvalue()


def zipf(members, method=zipfile.ZIP_DEFLATED):
    b = io.BytesIO()
    with zipfile.ZipFile(b, "w", method) as z:
        for n, c in members:
            zi = zipfile.ZipInfo(n, (2020, 1, 1, 0, 0, 0))
            zi.compress_type = method
            z.writestr(zi, c)
    return b.getvalue()


cases = []


def case(cid, args, files=None, stdin=None):
    c = {"id": cid, "args": ["-J1", "--sort=name"] + args}
    if files:
        c["files"] = {k: {"hex": v.hex()} for k, v in files.items()}
    if stdin is not None:
        c["stdin"] = stdin
    cases.append(c)


# single-stage decompression, one per codec
case("gz", ["-z", "foo", "a.gz"], {"a.gz": gz(T1)})
case("gz-no-z", ["foo", "a.gz"], {"a.gz": gz(T1)})
case("gz-count", ["-zc", "foo", "a.gz"], {"a.gz": gz(T1)})
case("gz-fname-n", ["-zn", "--with-filename", "foo", "a.gz"], {"a.gz": gz(T1)})
case("gz-nomatch", ["-z", "zzz", "a.gz"], {"a.gz": gz(T1)})
case("bz2", ["-z", "foo", "a.bz2"], {"a.bz2": bz2.compress(T1)})
case("xz", ["-z", "foo", "a.xz"], {"a.xz": lzma.compress(T1)})
case("lzma-alone", ["-z", "foo", "a.lzma"], {"a.lzma": lzma.compress(T1, format=lzma.FORMAT_ALONE)})
case("gz-big", ["-zc", "foo", "big.gz"], {"big.gz": gz(BIG)})
case("bz2-big", ["-zc", "foo", "big.bz2"], {"big.bz2": bz2.compress(BIG)})
case("xz-big", ["-zc", "foo", "big.xz"], {"big.xz": lzma.compress(BIG)})
case("gz-multi-files", ["-z", "--with-filename", "foo", "a.gz", "b.bz2", "c.xz", "plain.txt"],
     {"a.gz": gz(T1), "b.bz2": bz2.compress(T2), "c.xz": lzma.compress(T1), "plain.txt": T2})
case("gz-recursive", ["-zr", "--with-filename", "foo", "."],
     {"d/a.gz": gz(T1), "d/b.bz2": bz2.compress(T2), "d/e/c.xz": lzma.compress(T1), "d/plain.txt": T2})
case("gz-concatenated", ["-z", "foo", "cat.gz"], {"cat.gz": gz(T1) + gz(T2)})
case("gz-binary-member", ["-z", "foo", "b.gz"], {"b.gz": gz(BIN)})
case("gz-binary-text", ["-za", "foo", "b.gz"], {"b.gz": gz(BIN)})
case("gz-empty-content", ["-z", "foo", "e.gz"], {"e.gz": gz(b"")})
case("gz-zero-length-file", ["-z", "foo", "z.gz"], {"z.gz": b""})

# archives, --zmax=1 default
MEM = [("x.txt", T1), ("y.txt", T2), ("z.txt", T3)]
case("zip", ["-z", "--with-filename", "foo", "a.zip"], {"a.zip": zipf(MEM)})
case("zip-count", ["-zc", "--with-filename", "foo", "a.zip"], {"a.zip": zipf(MEM)})
case("zip-files-with-matches", ["-zl", "foo", "a.zip"], {"a.zip": zipf(MEM)})
case("zip-files-without-match", ["-zL", "foo", "a.zip"], {"a.zip": zipf(MEM)})
case("zip-stored", ["-z", "--with-filename", "foo", "a.zip"], {"a.zip": zipf(MEM, zipfile.ZIP_STORED)})
case("zip-bzip2", ["-z", "--with-filename", "foo", "a.zip"], {"a.zip": zipf(MEM, zipfile.ZIP_BZIP2)})
case("zip-lzma", ["-z", "--with-filename", "foo", "a.zip"], {"a.zip": zipf(MEM, zipfile.ZIP_LZMA)})
case("zip-dirs-empty-binary", ["-z", "-n", "--with-filename", "foo", "a.zip"],
     {"a.zip": zipf([("d/", b""), ("d/x.txt", T1), ("empty.txt", b""), ("bin.dat", BIN)])})
case("zip-include", ["-z", "--include=*.txt", "--with-filename", "foo", "a.zip"],
     {"a.zip": zipf(MEM + [("k.log", T1)])})
case("zip-exclude", ["-z", "--exclude=y*", "--with-filename", "foo", "a.zip"], {"a.zip": zipf(MEM)})
case("zip-hidden", ["-z", "--with-filename", "foo", "a.zip"], {"a.zip": zipf([(".hid.txt", T1), ("v.txt", T2)])})
case("zip-no-hidden", ["-z", "--no-hidden", "--with-filename", "foo", "a.zip"],
     {"a.zip": zipf([(".hid.txt", T1), ("v.txt", T2)])})
case("zip-no-filename", ["-z", "-H", "foo", "a.zip"], {"a.zip": zipf(MEM)})
case("zip-no-header", ["-z", "--no-filename", "-c", "foo", "a.zip"], {"a.zip": zipf(MEM)})
case("zip-big-member", ["-zc", "--with-filename", "foo", "a.zip"], {"a.zip": zipf([("big.txt", BIG), ("s.txt", T1)])})
case("zip-many-members", ["-zc", "--with-filename", "foo", "a.zip"],
     {"a.zip": zipf([("m%03d.txt" % i, T1 if i % 3 == 0 else T3) for i in range(60)])})
case("tar", ["-z", "--with-filename", "foo", "a.tar"], {"a.tar": tar(MEM)})
case("tar-count", ["-zc", "--with-filename", "foo", "a.tar"], {"a.tar": tar(MEM)})
case("tar-l", ["-zl", "foo", "a.tar"], {"a.tar": tar(MEM)})
case("tar-dirs-empty-binary", ["-z", "-n", "--with-filename", "foo", "a.tar"],
     {"a.tar": tar([("d", None), ("d/x.txt", T1), ("empty.txt", b""), ("bin.dat", BIN)])})
case("tar-pax", ["-z", "--with-filename", "foo", "a.tar"], {"a.tar": tar(MEM, tarfile.PAX_FORMAT)})
case("tar-gnu-longname", ["-z", "--with-filename", "foo", "a.tar"],
     {"a.tar": tar([("l" * 130 + ".txt", T1)], tarfile.GNU_FORMAT)})
case("tar-include", ["-z", "--include=x*", "--with-filename", "foo", "a.tar"], {"a.tar": tar(MEM)})
case("tar-big-member", ["-zc", "--with-filename", "foo", "a.tar"], {"a.tar": tar([("big.txt", BIG), ("s.txt", T1)])})
case("tgz", ["-z", "--with-filename", "foo", "a.tgz"], {"a.tgz": gz(tar(MEM))})
case("tar-gz-ext", ["-z", "--with-filename", "foo", "a.tar.gz"], {"a.tar.gz": gz(tar(MEM))})
case("tbz", ["-z", "--with-filename", "foo", "a.tbz"], {"a.tbz": bz2.compress(tar(MEM))})
case("txz", ["-z", "--with-filename", "foo", "a.txz"], {"a.txz": lzma.compress(tar(MEM))})
case("tgz-big", ["-zc", "--with-filename", "foo", "a.tgz"], {"a.tgz": gz(tar([("big.txt", BIG), ("s.txt", T1)]))})
case("multi-archives", ["-z", "-c", "--with-filename", "foo", "a.zip", "b.tar", "c.tgz", "d.gz"],
     {"a.zip": zipf(MEM), "b.tar": tar(MEM), "c.tgz": gz(tar(MEM)), "d.gz": gz(T1)})
case("recursive-archives", ["-zr", "-c", "--with-filename", "foo", "."],
     {"d/a.zip": zipf(MEM), "d/b.tar": tar(MEM), "e/c.tgz": gz(tar(MEM)), "e/f.txt": T1})

# --zmax > 1: nested compression and archives in archives
NEST_TAR = tar([("in.txt", T1), ("in2.txt", T2)])
case("zmax1-gz-gz", ["-z", "--zmax=1", "foo", "a.gz"], {"a.gz": gz(gz(T1))})
case("zmax2-gz-gz", ["-z", "--zmax=2", "foo", "a.gz"], {"a.gz": gz(gz(T1))})
case("zmax3-gz-gz-gz", ["-z", "--zmax=3", "foo", "a.gz"], {"a.gz": gz(gz(gz(T1)))})
case("zmax2-gz-gz-gz-short", ["-z", "--zmax=2", "foo", "a.gz"], {"a.gz": gz(gz(gz(T1)))})
case("zmax2-gz-bz2", ["-z", "--zmax=2", "foo", "a.gz"], {"a.gz": gz(bz2.compress(T1))})
case("zmax3-xz-gz-bz2", ["-z", "--zmax=3", "foo", "a.xz"], {"a.xz": lzma.compress(gz(bz2.compress(T1)))})
case("zmax1-zip-of-gz", ["-z", "--with-filename", "foo", "a.zip"], {"a.zip": zipf([("in.gz", gz(T1)), ("p.txt", T2)])})
case("zmax2-zip-of-gz", ["-z", "--zmax=2", "--with-filename", "foo", "a.zip"],
     {"a.zip": zipf([("in.gz", gz(T1)), ("p.txt", T2), ("in2.bz2", bz2.compress(T2))])})
case("zmax2-tar-of-gz", ["-z", "--zmax=2", "--with-filename", "foo", "a.tar"],
     {"a.tar": tar([("in.gz", gz(T1)), ("p.txt", T2)])})
case("zmax1-tar-of-tar", ["-z", "--with-filename", "foo", "a.tar"], {"a.tar": tar([("in.tar", NEST_TAR), ("p.txt", T3)])})
case("zmax2-tar-of-tar", ["-z", "--zmax=2", "--with-filename", "foo", "a.tar"],
     {"a.tar": tar([("in.tar", NEST_TAR), ("p.txt", T3)])})
case("zmax2-zip-of-zip", ["-z", "--zmax=2", "--with-filename", "foo", "a.zip"],
     {"a.zip": zipf([("in.zip", zipf(MEM)), ("p.txt", T3)])})
case("zmax2-zip-of-tgz", ["-z", "--zmax=2", "--with-filename", "foo", "a.zip"],
     {"a.zip": zipf([("in.tgz", gz(tar(MEM))), ("p.txt", T3)])})
case("zmax3-zip-of-tgz", ["-z", "--zmax=3", "--with-filename", "foo", "a.zip"],
     {"a.zip": zipf([("in.tgz", gz(tar(MEM))), ("p.txt", T3)])})
case("zmax2-tgz-of-tgz", ["-z", "--zmax=2", "--with-filename", "foo", "a.tgz"],
     {"a.tgz": gz(tar([("in.tgz", gz(tar(MEM)))]))})
case("zmax3-tgz-of-tgz", ["-z", "--zmax=3", "--with-filename", "foo", "a.tgz"],
     {"a.tgz": gz(tar([("in.tgz", gz(tar(MEM)))]))})
case("zmax2-count", ["-zc", "--zmax=2", "--with-filename", "foo", "a.zip"],
     {"a.zip": zipf([("in.tgz", gz(tar(MEM))), ("p.txt", T1)])})
case("zmax2-l", ["-zl", "--zmax=2", "foo", "a.zip"], {"a.zip": zipf([("in.tgz", gz(tar(MEM))), ("p.txt", T1)])})
case("zmax2-include", ["-z", "--zmax=2", "--include=*.txt", "--with-filename", "foo", "a.zip"],
     {"a.zip": zipf([("in.tgz", gz(tar(MEM))), ("p.txt", T1)])})
case("zmax2-big", ["-zc", "--zmax=2", "foo", "a.gz"], {"a.gz": gz(gz(BIG))})
case("zmax2-multi", ["-z", "--zmax=2", "-c", "--with-filename", "foo", "a.zip", "b.gz", "c.tgz"],
     {"a.zip": zipf([("in.gz", gz(T1))]), "b.gz": gz(gz(T1)), "c.tgz": gz(tar([("i.gz", gz(T2))]))})
case("zmax0", ["-z", "--zmax=0", "foo", "a.gz"], {"a.gz": gz(T1)})
case("zmax-invalid", ["-z", "--zmax=x", "foo", "a.gz"], {"a.gz": gz(T1)})
case("zmax-large", ["-z", "--zmax=99", "foo", "a.gz"], {"a.gz": gz(gz(T1))})
case("zmax-option-short", ["-z1", "foo", "a.gz"], {"a.gz": gz(T1)})
case("zmax-option-short2", ["-z2", "foo", "a.gz"], {"a.gz": gz(gz(T1))})

# cancellation during extraction: -m, -l, -q with early exit and the pipe closed by the reader
case("cancel-m1-gz-big", ["-z", "-m1", "foo", "big.gz"], {"big.gz": gz(BIG)})
case("cancel-m3-gz-big", ["-z", "-m3", "-n", "foo", "big.gz"], {"big.gz": gz(BIG)})
case("cancel-l-gz-big", ["-zl", "foo", "big.gz"], {"big.gz": gz(BIG)})
case("cancel-q-gz-big", ["-zq", "foo", "big.gz"], {"big.gz": gz(BIG)})
case("cancel-m1-bz2-big", ["-z", "-m1", "foo", "big.bz2"], {"big.bz2": bz2.compress(BIG)})
case("cancel-m1-xz-big", ["-z", "-m1", "foo", "big.xz"], {"big.xz": lzma.compress(BIG)})
BIGZIP = zipf([("a.txt", BIG), ("b.txt", BIG), ("c.txt", BIG)])
case("cancel-m1-zip-big", ["-z", "-m1", "-c", "--with-filename", "foo", "a.zip"], {"a.zip": BIGZIP})
case("cancel-m1-zip-big-text", ["-z", "-m1", "--with-filename", "foo", "a.zip"], {"a.zip": BIGZIP})
case("cancel-l-zip-big", ["-zl", "foo", "a.zip"], {"a.zip": BIGZIP})
case("cancel-q-zip-big", ["-zq", "foo", "a.zip"], {"a.zip": BIGZIP})
case("cancel-q-nomatch-zip", ["-zq", "zzz", "a.zip"], {"a.zip": BIGZIP})
BIGTAR = tar([("a.txt", BIG), ("b.txt", BIG), ("c.txt", BIG)])
case("cancel-m1-tar-big", ["-z", "-m1", "-c", "--with-filename", "foo", "a.tar"], {"a.tar": BIGTAR})
case("cancel-l-tar-big", ["-zl", "foo", "a.tar"], {"a.tar": BIGTAR})
case("cancel-q-tar-big", ["-zq", "foo", "a.tar"], {"a.tar": BIGTAR})
case("cancel-l-tgz-big", ["-zl", "foo", "a.tgz"], {"a.tgz": gz(BIGTAR)})
case("cancel-m1-tgz-big", ["-z", "-m1", "-c", "--with-filename", "foo", "a.tgz"], {"a.tgz": gz(BIGTAR)})
case("cancel-q-tgz-big", ["-zq", "foo", "a.tgz"], {"a.tgz": gz(BIGTAR)})
case("cancel-l-zmax2-gz-big", ["-zl", "--zmax=2", "foo", "a.gz"], {"a.gz": gz(gz(BIG))})
case("cancel-m1-zmax2-gz-big", ["-z", "-m1", "--zmax=2", "foo", "a.gz"], {"a.gz": gz(gz(BIG))})
case("cancel-q-zmax2-gz-big", ["-zq", "--zmax=2", "foo", "a.gz"], {"a.gz": gz(gz(BIG))})
case("cancel-l-zmax2-zip-tgz", ["-zl", "--zmax=2", "foo", "a.zip"], {"a.zip": zipf([("in.tgz", gz(BIGTAR)), ("p.txt", T1)])})
case("cancel-m1-zmax3-zip-tgz", ["-z", "-m1", "-c", "--zmax=3", "--with-filename", "foo", "a.zip"],
     {"a.zip": zipf([("in.tgz", gz(BIGTAR)), ("p.txt", T1)])})
case("cancel-q-zmax3-zip-tgz", ["-zq", "--zmax=3", "foo", "a.zip"], {"a.zip": zipf([("in.tgz", gz(BIGTAR)), ("p.txt", T1)])})
case("cancel-l-many-files", ["-zl", "foo", "1.gz", "2.zip", "3.tar", "4.tgz"],
     {"1.gz": gz(BIG), "2.zip": BIGZIP, "3.tar": BIGTAR, "4.tgz": gz(BIGTAR)})
case("cancel-m1-many-files", ["-z", "-m1", "-c", "--with-filename", "foo", "1.gz", "2.zip", "3.tar", "4.tgz"],
     {"1.gz": gz(BIG), "2.zip": BIGZIP, "3.tar": BIGTAR, "4.tgz": gz(BIGTAR)})
case("cancel-q-many-files", ["-zq", "foo", "1.gz", "2.zip", "3.tar"], {"1.gz": gz(BIG), "2.zip": BIGZIP, "3.tar": BIGTAR})
case("cancel-recursive-l", ["-zrl", "foo", "."],
     {"d/1.gz": gz(BIG), "d/2.zip": BIGZIP, "e/3.tar": BIGTAR, "e/4.tgz": gz(BIGTAR)})
case("cancel-max-files", ["-zr", "--max-files=1", "-c", "foo", "."], {"d/1.gz": gz(BIG), "d/2.zip": BIGZIP})
case("cancel-late-match-l", ["-zl", "-e", "zzz", "-e", "line 29999", "big.gz"], {"big.gz": gz(BIG)})

# corrupt, truncated and unreadable inputs: warnings
GZ1 = gz(T1)
case("corrupt-gz-garbage", ["-z", "foo", "a.gz"], {"a.gz": b"this is not gzip data\nfoo\n"})
case("corrupt-gz-truncated", ["-z", "foo", "a.gz"], {"a.gz": GZ1[: len(GZ1) // 2]})
case("corrupt-gz-bad-crc", ["-z", "foo", "a.gz"], {"a.gz": GZ1[:-8] + b"\x00\x00\x00\x00" + GZ1[-4:]})
case("corrupt-gz-bad-trailer-len", ["-z", "foo", "a.gz"], {"a.gz": GZ1[:-4] + b"\x01\x00\x00\x00"})
case("corrupt-gz-bad-middle", ["-z", "foo", "a.gz"], {"a.gz": GZ1[:14] + bytes([GZ1[14] ^ 0xFF]) + GZ1[15:]})
case("corrupt-gz-big-truncated", ["-zc", "foo", "big.gz"], {"big.gz": gz(BIG)[:40000]})
case("corrupt-bz2-truncated", ["-z", "foo", "a.bz2"], {"a.bz2": bz2.compress(T1)[:20]})
case("corrupt-bz2-garbage", ["-z", "foo", "a.bz2"], {"a.bz2": b"BZh9garbage garbage garbage"})
case("corrupt-xz-truncated", ["-z", "foo", "a.xz"], {"a.xz": lzma.compress(T1)[:20]})
case("corrupt-xz-garbage", ["-z", "foo", "a.xz"], {"a.xz": b"\xfd7zXZ\x00garbage garbage garbage"})
ZIP1 = zipf(MEM)
case("corrupt-zip-truncated", ["-z", "--with-filename", "foo", "a.zip"], {"a.zip": ZIP1[: len(ZIP1) // 2]})
case("corrupt-zip-garbage", ["-z", "--with-filename", "foo", "a.zip"], {"a.zip": b"PK\x03\x04garbage garbage garbage garbage"})
case("corrupt-zip-bad-member-data", ["-z", "--with-filename", "foo", "a.zip"],
     {"a.zip": ZIP1[:45] + bytes([ZIP1[45] ^ 0xFF]) + ZIP1[46:]})
case("corrupt-zip-no-central-dir", ["-z", "--with-filename", "foo", "a.zip"], {"a.zip": ZIP1[:200]})
TAR1 = tar(MEM)
case("corrupt-tar-truncated", ["-z", "--with-filename", "foo", "a.tar"], {"a.tar": TAR1[:1000]})
case("corrupt-tar-bad-checksum", ["-z", "--with-filename", "foo", "a.tar"],
     {"a.tar": TAR1[:148] + b"0000000\x00" + TAR1[156:]})
case("corrupt-tar-garbage-ext", ["-z", "--with-filename", "foo", "a.tar"], {"a.tar": b"not a tar file at all, foo\n" * 40})
case("corrupt-tgz-truncated", ["-z", "--with-filename", "foo", "a.tgz"], {"a.tgz": gz(TAR1)[:60]})
case("corrupt-zmax2-inner-gz", ["-z", "--zmax=2", "foo", "a.gz"], {"a.gz": gz(GZ1[: len(GZ1) // 2])})
case("corrupt-zmax2-zip-inner-gz", ["-z", "--zmax=2", "--with-filename", "foo", "a.zip"],
     {"a.zip": zipf([("in.gz", GZ1[:20]), ("p.txt", T2)])})
case("corrupt-zmax2-tar-inner-gz", ["-z", "--zmax=2", "--with-filename", "foo", "a.tar"],
     {"a.tar": tar([("in.gz", GZ1[:20]), ("p.txt", T2)])})
case("corrupt-multi-files-one-bad", ["-z", "--with-filename", "foo", "a.gz", "bad.gz", "c.gz"],
     {"a.gz": gz(T1), "bad.gz": b"garbage garbage", "c.gz": gz(T2)})
case("corrupt-q-bad", ["-zq", "foo", "bad.gz", "a.gz"], {"bad.gz": b"garbage", "a.gz": gz(T1)})
case("corrupt-no-messages", ["-z", "-s", "foo", "bad.gz", "a.gz"], {"bad.gz": b"garbage", "a.gz": gz(T1)})
case("corrupt-empty-zip", ["-z", "foo", "e.zip"], {"e.zip": b"PK\x05\x06" + b"\x00" * 18})
case("unreadable-missing", ["-z", "foo", "nonexistent.gz"])
case("unreadable-missing-and-ok", ["-z", "foo", "nonexistent.gz", "a.gz"], {"a.gz": gz(T1)})
case("unreadable-directory", ["-z", "foo", "d"], {"d/a.gz": gz(T1)})
case("unreadable-s", ["-zs", "foo", "nonexistent.gz"])
case("zip-encrypted-flag", ["-z", "--with-filename", "foo", "a.zip"],
     {"a.zip": ZIP1.replace(b"PK\x03\x04\x14\x00\x00\x00", b"PK\x03\x04\x14\x00\x01\x00", 1)})

# output formats carry the part name
case("zip-json", ["-z", "--json", "foo", "a.zip"], {"a.zip": zipf(MEM)})
case("zip-format", ["-z", "--format=%f:%n:%O%~", "foo", "a.zip"], {"a.zip": zipf(MEM)})
case("zip-csv", ["-z", "--csv", "foo", "a.zip"], {"a.zip": zipf(MEM)})
case("zip-l-null", ["-z", "-l", "--null", "foo", "a.zip"], {"a.zip": zipf(MEM)})
case("zip-context", ["-z", "-C1", "-n", "--with-filename", "foo", "a.zip"], {"a.zip": zipf(MEM)})
case("zip-invert", ["-z", "-v", "-c", "--with-filename", "foo", "a.zip"], {"a.zip": zipf(MEM)})
case("zip-o", ["-z", "-o", "--with-filename", "foo", "a.zip"], {"a.zip": zipf(MEM)})
case("zip-heading", ["-z", "--heading", "-n", "foo", "a.zip"], {"a.zip": zipf(MEM)})
case("zip-tree", ["-z", "--tree", "-c", "foo", "a.zip"], {"a.zip": zipf([("d/x.txt", T1), ("d/e/y.txt", T2)])})
case("zip-filter-ext", ["-z", "-t", "txt", "--with-filename", "foo", "a.zip"], {"a.zip": zipf(MEM + [("c.c", T1)])})
case("zip-glob-name", ["-z", "-g", "y*", "--with-filename", "foo", "a.zip"], {"a.zip": zipf(MEM)})
case("gz-glob-name", ["-z", "-g", "*.gz", "-r", "--with-filename", "foo", "."], {"a.gz": gz(T1), "b.txt": T1})
case("gz-count-multiple", ["-zc", "foo", "a.gz", "b.gz"], {"a.gz": gz(T1), "b.gz": gz(T3)})
case("gz-max-count-multiple", ["-z", "-m1", "--with-filename", "foo", "a.gz", "b.gz"], {"a.gz": gz(T1), "b.gz": gz(T2)})
case("zip-max-count-per-part", ["-z", "-m1", "--with-filename", "foo", "a.zip"], {"a.zip": zipf(MEM)})
case("zip-binary-files-without-match", ["-z", "-I", "--with-filename", "foo", "a.zip"], {"a.zip": zipf([("bin.dat", BIN), ("x.txt", T1)])})
case("zip-binary-matches", ["-z", "--with-filename", "foo", "a.zip"], {"a.zip": zipf([("bin.dat", BIN), ("x.txt", T1)])})
case("zip-hexdump", ["-z", "--hexdump", "--with-filename", "foo", "a.zip"], {"a.zip": zipf([("bin.dat", BIN)])})
case("stdin-text-z", ["-z", "foo"], None, "alpha foo\nbeta\n")


# round 2: formats and layouts the first round did not reach (cpio, zstd, bzip2/xz multi-stream, gzip header
# fields, zip data descriptors, tar symlinks)
def cpio(members):
    out = b""
    for i, (n, c) in enumerate(members + [("TRAILER!!!", b"")]):
        nm = n.encode() + b"\0"
        hdr = "070701" + "".join("%08X" % v for v in (i + 1, 0o100644 if c is not None else 0, 0, 0, 1, 0, len(c), 0, 0, 0, 0, len(nm), 0))
        out += hdr.encode() + nm
        out += b"\0" * (-len(out) % 4) + c
        out += b"\0" * (-len(out) % 4)
    return out


class NoSeek(io.RawIOBase):
    def __init__(self):
        self.b = b""

    def writable(self):
        return True

    def write(self, d):
        self.b += bytes(d)
        return len(d)


def zip_stream(members):
    s = NoSeek()
    with zipfile.ZipFile(s, "w", zipfile.ZIP_DEFLATED) as z:
        for n, c in members:
            zi = zipfile.ZipInfo(n, (2020, 1, 1, 0, 0, 0))
            zi.compress_type = zipfile.ZIP_DEFLATED
            with z.open(zi, "w") as w:
                w.write(c)
    return s.b


def tar_symlink():
    b = io.BytesIO()
    with tarfile.open(fileobj=b, mode="w", format=tarfile.USTAR_FORMAT) as t:
        i = tarfile.TarInfo("x.txt")
        i.size = len(T1)
        i.mtime = 0
        t.addfile(i, io.BytesIO(T1))
        l = tarfile.TarInfo("link.txt")
        l.type = tarfile.SYMTYPE
        l.linkname = "x.txt"
        l.mtime = 0
        t.addfile(l)
    return b.getvalue()


def gz_header(d):
    # FNAME and FCOMMENT flags set
    raw = gz(d)
    return raw[:3] + bytes([raw[3] | 0x18]) + raw[4:10] + b"name.txt\0comment\0" + raw[10:]


# CPIO is a bsdtar newc archive of m1.txt ('foo one\nbar\n') and m2.txt ('second foo\n'); the hand-made cpio() is
# not a valid archive for ugrep (kept as the "cpio-handmade" cases: the C++ output on it is recorded too)
CPIO = bytes.fromhex(
    "3037303730313030313431623634303030303831623630303030303030303030303030303030303030303030303136616665"
    "3864663030303030303030633030303030303736353237663030353630303030303030303030303030303030303030303030"
    "303730303030303030306d312e74787400000000666f6f206f6e650a6261720a303730373031303031343162636630303030"
    "3831623630303030303030303030303030303030303030303030303136616665386466303030303030303062303030303030"
    "3736353237663030353630303030303030303030303030303030303030303030303730303030303030306d322e7478740000"
    "00007365636f6e6420666f6f0a00303730373031303030303030303030303030303030303030303030303030303030303030"
    "3030303030303030303130303030303030303030303030303030303030303030303030303030303030303030303030303030"
    "303030303030303030303030303030623030303030303030545241494c455221212100000000"
)
case("cpio", ["-z", "--with-filename", "foo", "a.cpio"], {"a.cpio": CPIO})
case("cpio-count", ["-zc", "--with-filename", "foo", "a.cpio"], {"a.cpio": CPIO})
case("cpio-l", ["-zl", "foo", "a.cpio"], {"a.cpio": CPIO})
case("cpio-gz", ["-z", "--with-filename", "foo", "a.cpio.gz"], {"a.cpio.gz": gz(CPIO)})
case("cpio-truncated", ["-z", "--with-filename", "foo", "a.cpio"], {"a.cpio": CPIO[:150]})
case("zmax2-zip-of-cpio", ["-z", "--zmax=2", "--with-filename", "foo", "a.zip"], {"a.zip": zipf([("in.cpio", CPIO)])})
case("cpio-handmade", ["-z", "--with-filename", "foo", "a.cpio"], {"a.cpio": cpio(MEM)})
case("bz2-multistream", ["-z", "foo", "m.bz2"], {"m.bz2": bz2.compress(T1) + bz2.compress(T2)})
case("xz-multistream", ["-z", "foo", "m.xz"], {"m.xz": lzma.compress(T1) + lzma.compress(T2)})
case("gz-header-fields", ["-z", "foo", "h.gz"], {"h.gz": gz_header(T1)})
case("zip-data-descriptor", ["-z", "--with-filename", "foo", "a.zip"], {"a.zip": zip_stream(MEM)})
case("zip-data-descriptor-big", ["-zc", "--with-filename", "foo", "a.zip"], {"a.zip": zip_stream([("big.txt", BIG), ("s.txt", T1)])})
case("tar-symlink", ["-z", "--with-filename", "foo", "a.tar"], {"a.tar": tar_symlink()})
case("zip-duplicate-names", ["-z", "--with-filename", "foo", "a.zip"], {"a.zip": zipf([("x.txt", T1), ("x.txt", T2)])})
case("tar-empty-archive", ["-z", "foo", "a.tar"], {"a.tar": b"\0" * 10240})
case("zip-member-names-unicode", ["-z", "--with-filename", "foo", "a.zip"], {"a.zip": zipf([("d\u00e9j\u00e0.txt", T1)])})
case("zmax2-cancel-many-parts", ["-zl", "--zmax=2", "foo", "a.zip"],
     {"a.zip": zipf([("i%d.gz" % i, gz(BIG)) for i in range(6)])})
case("zmax2-cancel-q-many-parts", ["-zq", "--zmax=2", "foo", "a.zip"],
     {"a.zip": zipf([("i%d.gz" % i, gz(BIG)) for i in range(6)])})
case("zmax2-m1-many-parts", ["-z", "-m1", "-c", "--zmax=2", "--with-filename", "foo", "a.zip"],
     {"a.zip": zipf([("i%d.gz" % i, gz(BIG)) for i in range(6)])})
case("many-files-cancel-l", ["-zl", "foo"] + ["f%02d.gz" % i for i in range(20)],
     {"f%02d.gz" % i: gz(BIG) for i in range(20)})
case("many-files-cancel-m1", ["-z", "-m1", "-c", "--with-filename", "foo"] + ["f%02d.tgz" % i for i in range(12)],
     {"f%02d.tgz" % i: gz(BIGTAR) for i in range(12)})

# zstd/lzma tar produced once by bsdtar (m1.txt = 'foo one\nbar\n', m2.txt = 'second foo\n'); python has no zstd
case("tzst", ["-z", "--with-filename", "foo", "a.tzst"], {"a.tzst": bytes.fromhex("28b52ffd0458b50300c2441016a0a53a00dfae12bb9b35fadf97b2b689e4b75fbd2903bbca055b22b942d668681889ad77958f0d3d07b8f1a43cefebd19b7388490bf9ff6855fd8f2ff0825c011100fe7315901720ab6807d41b1820538802d420000a1c4819404090805148873539f80dd2800027c64cb382165c472760047d6526c4")})
case("tzst-zmax2", ["-z", "--zmax=2", "--with-filename", "foo", "a.tzst"], {"a.tzst": bytes.fromhex("28b52ffd0458b50300c2441016a0a53a00dfae12bb9b35fadf97b2b689e4b75fbd2903bbca055b22b942d668681889ad77958f0d3d07b8f1a43cefebd19b7388490bf9ff6855fd8f2ff0825c011100fe7315901720ab6807d41b1820538802d420000a1c4819404090805148873539f80dd2800027c64cb382165c472760047d6526c4")})
case("tlzma", ["-z", "--with-filename", "foo", "a.tlzma"], {"a.tlzma": bytes.fromhex("5d00008000ffffffffffffffff00368c41c78430166b02f614d36835dcbd863e5c9cbed0efe4dd06b2b39df7500784f7d65b004a25ae097e3c48abedec5ffa5c49132b2da6aee3db998add0e5516d091cb76b2a9f19cd20f9e0a64f9205eee87fa0c0f5e07325737600c24f6dc4ab906898219b85c8e21d32685c9cb40ad67fce59c82")})

os.makedirs(os.path.dirname(OUT), exist_ok=True)
with open(OUT, "w", newline="\n") as f:
    for c in cases:
        f.write(json.dumps(c, separators=(",", ":")) + "\n")
print(len(cases), "cases ->", OUT)
