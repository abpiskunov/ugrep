"""Generates the surface cases of src_indexer for generated trees (inputs only) and appends them to cases.jsonl.

Cases that start from an existing index are seeded with index files that the C++ ugrep-indexer wrote itself
(seed(): the C++ run is the source of the bytes, nothing is typed). Run from the repository root:
  python migration/harness/src_indexer/gen_cases.py [--exe path] [--check]
Existing lines of cases.jsonl are never changed (append-only); a case id that is already there is skipped.
"""
import json, os, subprocess, sys, tempfile

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
CASES = os.path.join(ROOT, "migration", "corpus", "src_indexer", "cases.jsonl")
EXE = os.path.join(ROOT, "msvc", "bin", "Base-Release-x64", "ugrep-indexer.exe")
if "--exe" in sys.argv:
    EXE = sys.argv[sys.argv.index("--exe") + 1]

LOREM = ("The quick brown fox jumps over the lazy dog.\nPack my box with five dozen liquor jugs.\n"
         "Sphinx of black quartz, judge my vow.\nfoo bar baz qux 0123456789 ugrep indexer\n") * 6
UTF8 = "caf\u00e9 na\u00efve \u65e5\u672c\u8a9e \u00fcber\nsecond line \u20ac\n".encode("utf-8")
UTF16 = b"\xff\xfe" + "hello utf16 text\n".encode("utf-16-le")
BIN = bytes([0, 1, 2, 255, 254, 0, 7, 8, 0, 200, 201, 0]) * 8
NESTED = {
    "a.txt": LOREM, "s/b.txt": "bar\n", "s/t/c.txt": "baz qux\n", "e/.keep.dir": "", "s/t/u/d.md": "# doc\nfoo\n",
}


def raw(c):
    return c if isinstance(c, bytes) else c.encode("utf-8")


def put(d, files):
    for p, c in files.items():
        fp = os.path.join(d, p)
        os.makedirs(os.path.dirname(fp), exist_ok=True)
        open(fp, "wb").write(raw(c))


def seed(files, args=None):
    """Run the C++ indexer on files and return the index files it wrote (hex)."""
    d = tempfile.mkdtemp()
    put(d, files)
    env = {"LC_ALL": "C", "TZ": "UTC", "HOME": d, "USERPROFILE": d, "SystemRoot": os.environ.get("SystemRoot", "")}
    r = subprocess.run([EXE, "-q"] + (args or []), cwd=d, env=env, capture_output=True)
    assert r.returncode == 0, r
    out = {}
    for root, _, fs in os.walk(d):
        for f in fs:
            if f == "._UG#_Store":
                p = os.path.join(root, f)
                out[os.path.relpath(p, d).replace("\\", "/")] = open(p, "rb").read()
    return out


def enc(files):
    return {k: v if isinstance(v, str) else {"hex": v.hex()} for k, v in files.items()}


cases = []


def case(cid, args, files=None):
    c = {"id": cid, "args": args}
    if files is not None:
        c["files"] = enc(files)
    cases.append(c)


# default indexing of generated trees
case("indexer-g-default-nested", ["-v", "."], NESTED)
case("indexer-g-default-nested-quiet", ["-q"], NESTED)
case("indexer-g-default-path-arg", ["-v", "s"], NESTED)
case("indexer-g-default-mixed-content", ["-v", "."], {"t.txt": LOREM, "u8.txt": UTF8, "u16.txt": UTF16, "bin.dat": BIN, "empty.txt": ""})
case("indexer-g-default-nonascii-name", ["-v", "."], {"caf\u00e9.txt": "cafe\n", "d\u00efr/x.txt": "x\n"})
case("indexer-g-default-file-arg", ["-v", "a.txt"], {"a.txt": "foo\n"})
case("indexer-g-default-long-options", ["--verbose", "--accuracy=2", "--force", "."], NESTED)
for n in (0, 2, 7, 9):
    case("indexer-g-accuracy-%d" % n, ["-%d" % n, "-v", "."], {"t.txt": LOREM, "u8.txt": UTF8})
case("indexer-g-accuracy-bad", ["--accuracy=10", "."], {"a.txt": "foo\n"})
case("indexer-g-hidden-tree", ["-.", "-v", "."], {".h/x.txt": "x\n", "a.txt": "a\n", ".top": "t\n"})
case("indexer-g-hidden-default", ["-v", "."], {".h/x.txt": "x\n", "a.txt": "a\n", ".top": "t\n"})
case("indexer-g-nonexistent-silent", ["-s", "nodir"])
case("indexer-g-store-garbage", ["-v", "."], {"._UG#_Store": "not an index\n", "a.txt": "foo\n"})
case("indexer-g-store-garbage-check", ["-c", "."], {"._UG#_Store": "not an index\n", "a.txt": "foo\n"})

# -I ignore binary
case("indexer-g-ignore-binary-mixed", ["-I", "-v", "."], {"t.txt": LOREM, "u16.txt": UTF16, "bin.dat": BIN, "s/b2.bin": BIN})
case("indexer-g-ignore-binary-long", ["--ignore-binary", "-v", "-f", "."], {"t.txt": LOREM, "bin.dat": BIN})

# --check / -c
case("indexer-g-check-unindexed", ["-c"], NESTED)
case("indexer-g-check-unindexed-verbose", ["-c", "-v"], NESTED)
# stale-index cases (seeded index vs. mtimes) are racy on one filesystem tick and are not recorded
case("indexer-g-check-empty-dir", ["-c"])
# FRESH: every source name sorts before "._UG#_Store" (run-cases writes files in key order), so each seeded index is
# newer than the files it covers and the C++ reports the tree as up to date
FRESH = {"!a.txt": LOREM, "!s/!b.txt": "bar\n", "!s/!t/!c.txt": "baz qux\n", "!s/!t/!u/!d.md": "# doc\nfoo\n", "!e/!k": ""}
FRESH_DROP = {k: v for k, v in FRESH.items() if k != "!s/!b.txt"}
case("indexer-g-check-fresh", ["-c"], {**seed(FRESH), **FRESH})
case("indexer-g-check-fresh-verbose", ["--check", "--verbose"], {**seed(FRESH), **FRESH})
case("indexer-g-check-fresh-extra-file", ["-c", "-v"], {**seed(FRESH), **FRESH, "!n.txt": "new\n"})
case("indexer-g-check-fresh-deleted-file", ["-c", "-v"], {**seed(FRESH), **FRESH_DROP})
case("indexer-g-update-fresh", ["-v", "."], {**seed(FRESH), **FRESH})
case("indexer-g-update-fresh-new-file", ["-v", "."], {**seed(FRESH), **FRESH, "!n.txt": "new\n"})
case("indexer-g-update-fresh-deleted-file", ["-v", "."], {**seed(FRESH), **FRESH_DROP})
case("indexer-g-update-fresh-accuracy-change", ["-7", "-v", "."], {**seed(FRESH), **FRESH})
case("indexer-g-update-fresh-ignore-binary", ["-I", "-v", "."], {**seed(FRESH), **FRESH, "!bin.dat": BIN})
case("indexer-g-force-fresh", ["-f", "-v", "."], {**seed(FRESH), **FRESH})
case("indexer-g-check-quiet", ["-c", "-q"], NESTED)

# incremental update, -f, -d
case("indexer-g-force-accuracy-change", ["-f", "-7", "-v", "."], {**seed(NESTED), **NESTED})
case("indexer-g-delete-seeded", ["-d", "-v", "."], {**seed(NESTED), **NESTED})
case("indexer-g-delete-seeded-quiet", ["-d", "-q", "."], {**seed(NESTED), **NESTED})
case("indexer-g-delete-subdir", ["-d", "-v", "s"], {**seed(NESTED), **NESTED})
case("indexer-g-delete-long", ["--delete", "."], {**seed(NESTED), **NESTED})
case("indexer-g-delete-none", ["-d", "-v", "."], NESTED)
case("indexer-g-delete-garbage-store", ["-d", "-v", "."], {"._UG#_Store": "not an index\n", "a.txt": "foo\n"})

# -z (not available in this build: recorded as is), --zmax
case("indexer-g-decompress-unavailable", ["-z", "."], {"a.txt": "foo\n"})
case("indexer-g-decompress-zmax", ["-z", "--zmax=2", "-v", "."], {"a.txt": "foo\n"})
case("indexer-g-zmax-alone", ["--zmax=3", "-v", "."], {"a.txt": "foo\n"})
case("indexer-g-zmax-bad", ["--zmax=0", "."], {"a.txt": "foo\n"})
case("indexer-g-zmax-bad-high", ["--zmax=100", "."], {"a.txt": "foo\n"})
case("indexer-g-decompress-config", ["-v", "."], {".ugrep-indexer": "decompress\n", "a.txt": "foo\n"})

# -X ignore files
GIT = {".gitignore": "*.log\nbuild/\n", "a.txt": "a\n", "b.log": "b\n", "build/x.txt": "x\n", "s/.gitignore": "c.txt\n", "s/c.txt": "c\n", "s/d.txt": "d\n"}
case("indexer-g-ignore-files-default", ["-X", "-v", "."], GIT)
case("indexer-g-ignore-files-custom", ["--ignore-files=.myignore", "-v", "."], {".myignore": "*.tmp\n", "a.txt": "a\n", "b.tmp": "b\n", ".gitignore": "a.txt\n"})
case("indexer-g-ignore-files-two", ["-X", "--ignore-files=.myignore", "-v", "."], {**GIT, ".myignore": "d.txt\n"})
case("indexer-g-ignore-files-off", ["-v", "."], GIT)
case("indexer-g-ignore-files-check", ["-X", "-c", "-v"], {**seed(GIT, ["-X"]), **GIT})

# .ugrep-indexer configuration file
case("indexer-g-config-accuracy", ["-v", "."], {".ugrep-indexer": "accuracy=7\n", "a.txt": LOREM})
case("indexer-g-config-verbose", ["."], {".ugrep-indexer": "verbose\n", "a.txt": "foo\n"})
case("indexer-g-config-quiet", ["."], {".ugrep-indexer": "quiet\n", "a.txt": "foo\n"})
case("indexer-g-config-comments-blank", ["-v", "."], {".ugrep-indexer": "# comment\n\n   # indented comment\n  accuracy=2  \n\n", "a.txt": LOREM})
case("indexer-g-config-bad-option", ["-v", "."], {".ugrep-indexer": "bogus\n", "a.txt": "foo\n"})
case("indexer-g-config-bad-value", ["-v", "."], {".ugrep-indexer": "accuracy=11\n", "a.txt": "foo\n"})
case("indexer-g-config-two-errors", ["-v", "."], {".ugrep-indexer": "bogus\nnope=1\n", "a.txt": "foo\n"})
case("indexer-g-config-cli-overrides", ["-2", "-v", "."], {".ugrep-indexer": "accuracy=7\n", "a.txt": LOREM})
case("indexer-g-config-hidden", ["-v", "."], {".ugrep-indexer": "hidden\n", ".h.txt": "h\n", "a.txt": "a\n"})
case("indexer-g-config-ignore-binary", ["-v", "."], {".ugrep-indexer": "ignore-binary\n", "bin.dat": BIN, "t.txt": LOREM})
case("indexer-g-config-ignore-files", ["-v", "."], {".ugrep-indexer": "ignore-files\n", ".gitignore": "*.log\n", "a.txt": "a\n", "b.log": "b\n"})
case("indexer-g-config-force", ["-v", "."], {**seed(NESTED), ".ugrep-indexer": "force\n", **NESTED})
case("indexer-g-config-check", ["."], {".ugrep-indexer": "check\n", **NESTED})
case("indexer-g-config-delete", ["-v", "."], {**seed(NESTED), ".ugrep-indexer": "delete\n", **NESTED})
case("indexer-g-config-crlf", ["-v", "."], {".ugrep-indexer": b"accuracy=6\r\nverbose\r\n", "a.txt": LOREM})
case("indexer-g-config-in-subdir-ignored", ["-v", "s"], {"s/.ugrep-indexer": "accuracy=9\n", "s/a.txt": LOREM})
case("indexer-g-config-no-messages", ["."], {".ugrep-indexer": "no-messages\nverbose\n", "a.txt": "foo\n"})

# option parsing
case("indexer-g-opt-combined", ["-vf5", "."], NESTED)
case("indexer-g-opt-version-short", ["-V"])
case("indexer-g-opt-help-short", ["-?"])
case("indexer-g-opt-missing-value", ["--accuracy"], {"a.txt": "foo\n"})
case("indexer-g-opt-ambiguous", ["--ig", "."], {"a.txt": "foo\n"})
case("indexer-g-opt-dash-dash", ["-v", "--", "s"], NESTED)
case("indexer-g-opt-dereference", ["-S", "-v", "."], NESTED)
case("indexer-g-opt-no-messages-v", ["-s", "-v", "."], NESTED)

# round 2 (coverage-driven): option spellings, negated ignore globs, UTF-8 edge cases of the binary check
case("indexer-g-opt-invalid-short", ["-Z", "."], {"a.txt": "foo\n"})
case("indexer-g-opt-zmax-not-number", ["--zmax=abc", "."], {"a.txt": "foo\n"})
case("indexer-g-opt-silent", ["--silent", "."], {"a.txt": "foo\n"})
case("indexer-g-opt-no-messages-long", ["--no-messages", "-v", "."], {"a.txt": "foo\n"})
case("indexer-g-opt-dereference-long", ["--dereference-files", "-v", "."], {"a.txt": "foo\n"})
case("indexer-g-opt-hidden-long", ["--hidden", "-v", "."], {".h.txt": "h\n", "a.txt": "a\n"})
case("indexer-g-opt-ignore-files-plain", ["--ignore-files", "-v", "."], {".gitignore": "*.log\n", "a.txt": "a\n", "b.log": "b\n"})
case("indexer-g-opt-short-ignore-files-dirs", ["-X", "-v", "."], {".gitignore": "d*/\n!dk/\n", "a.txt": "a\n", "dx/x.txt": "x\n", "dk/k.txt": "k\n"})
case("indexer-g-opt-ignore-files-negate", ["-X", "-v", "."], {".gitignore": "*.txt\n!keep.txt\n", "a.txt": "a\n", "keep.txt": "k\n", "b.md": "b\n"})
case("indexer-g-config-dereference", ["-v", "."], {".ugrep-indexer": "dereference-files\nsilent\n", "a.txt": "foo\n"})
case("indexer-g-config-ignore-files-named", ["-v", "."], {".ugrep-indexer": "ignore-files=.myignore\n", ".myignore": "*.tmp\n", "a.txt": "a\n", "b.tmp": "b\n"})
case("indexer-g-config-help", ["-v", "."], {".ugrep-indexer": "help\n", "a.txt": "foo\n"})
case("indexer-g-utf8-truncated-tail", ["-I", "-v", "."], {"t.txt": LOREM.encode() + b"\xe2\x82", "ok.txt": UTF8})
case("indexer-g-utf8-invalid-lead", ["-I", "-v", "."], {"a.txt": b"abc \xc0\x80 def\n", "b.txt": b"abc \xe2\x28\xa1 def\n", "c.txt": b"abc \xf0\x90\x28\xbc def\n", "d.txt": b"abc \xf8\x88 def\n"})
case("indexer-g-utf8-valid-sequences", ["-I", "-v", "."], {"a.txt": "\u00e9\u20ac\U0001f600 text\n".encode("utf-8"), "b.txt": b"abc \xc3\xa9 \xe2\x82\xac \xf0\x9f\x98\x80\n"})
case("indexer-g-large-file", ["-v", "-3", "."], {"big.txt": LOREM * 400, "small.txt": "x\n"})
case("indexer-g-no-trailing-newline", ["-v", "."], {"a.txt": "no newline at end", "b.txt": "\n\n\n"})

known = set()
if os.path.exists(CASES):
    for line in open(CASES, encoding="utf-8"):
        if line.strip():
            known.add(json.loads(line)["id"])
new = [c for c in cases if c["id"] not in known]
if "--check" in sys.argv:
    print(len(cases), "cases,", len(new), "new")
    sys.exit(0)
with open(CASES, "a", encoding="utf-8", newline="\n") as f:
    for c in new:
        f.write(json.dumps(c, ensure_ascii=False, separators=(",", ":")) + "\n")
print("appended", len(new))
