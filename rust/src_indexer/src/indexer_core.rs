//! Unit `indexer_core`: src/ugrep-indexer.cpp lines 1-1050 (Stream, noise_percentage, is_binary, indexhash, `index`).
//! `index` is ONE coarse seam call per file (or per archive part): reading, the 8 staggered Bloom filters and the
//! table compression all run on the Rust side of the call.
//!
//! The reference build has no `HAVE_LIBZ` (Q-indexer-5): `-z` does not decompress and there are no archive parts,
//! so the stream is a plain `FILE*` read through `reflex::Input` (UTF BOM detection and UTF-16/32 to UTF-8
//! conversion, reproduced by `Input` below). The stream state never outlives one call, so the returned handle is
//! always 0 and `stream_drop` has nothing to release.

use super::{Bytes, IndexerFlags, Indexed};
use std::fs::File;
use std::io::Read;

/// number of bytes to gulp into the buffer to index a file
const BUF_SIZE: usize = 65536;
/// fixed window size
const WIN_SIZE: usize = 8;
/// smallest possible power-of-two size of an index of a file
const MIN_SIZE: usize = 128;
/// `REFLEX_NONCHAR` (0x200000) is out of range and is written as these 5 bytes
const NONCHAR_UTF8: [u8; 5] = [0xf8, 0x88, 0x80, 0x80, 0x80];
const NONCHAR: i32 = 0x20_0000;

/// Seam function `src_indexer.index` (`index()` of src/ugrep-indexer.cpp).
/// `handle` is the stream state of the previous call for the same file (0 for a new file), `archive` is the
/// in/out flag of `index()`. Reads flags: `accuracy`, `ignore_binary` (`decompress`, `hidden` and `zmax` only
/// matter with `HAVE_LIBZ`).
pub fn index(flags: &IndexerFlags, _handle: u64, pathname: &[u8], archive: bool) -> Indexed {
    let mut r = Indexed {
        ok: false,
        hashes: Vec::new(),
        noise: 0.0,
        compressed: false,
        archive,
        binary: false,
        size: 0,
        partname: Vec::new(),
        handle: 0,
    };

    let Some(mut input) = Input::open(pathname) else {
        return r;
    };
    r.ok = true;

    let mut buffer = vec![0u8; BUF_SIZE + WIN_SIZE];
    let mut buflen = input.get(&mut buffer[..BUF_SIZE]);
    if buflen == 0 {
        return r;
    }

    // check buffer for binary data, the buffer is a window over the input file
    let mut checklen = buflen;
    if buffer[checklen - 1] & 0x80 == 0x80 {
        // do not cut off the last UTF-8 sequence, ignore it, otherwise we risk failing the UTF-8 check
        let mut n = checklen.min(4);
        while n > 0 {
            checklen -= 1;
            if buffer[checklen] & 0xc0 != 0x80 {
                break;
            }
            n -= 1;
        }
        if buffer[checklen] & 0xc0 != 0xc0 {
            r.binary = true;
        }
    }
    r.binary = r.binary || is_binary(&buffer[..checklen]);

    if r.binary && flags.ignore_binary {
        return r;
    }

    let max_noise = noise_percentage(flags.accuracy);
    let mut window = 0usize;
    let winlen = buflen.min(WIN_SIZE);
    let mut size = buflen as u64;
    buflen -= winlen;
    let mut hashes = new_table();

    if buflen > 0 {
        loop {
            // compute 8 staggered Bloom filters, hashing 1-grams to 8-grams for N^2 = 64 Bloom hash functions
            let win = &buffer[window..window + WIN_SIZE];
            let mut h = u32::from(win[0]);
            for (j, &b) in win.iter().enumerate().skip(1) {
                hashes[h as usize] &= !(1u8 << (j - 1));
                h = indexhash(h, b);
            }
            hashes[h as usize] &= !0x80;

            window += 1;
            buflen -= 1;

            // refill buffer[] when empty
            if buflen == 0 {
                // move the remainder of the last window to the front of the buffer[] and append
                buffer.copy_within(window..window + WIN_SIZE, 0);
                buflen = input.get(&mut buffer[WIN_SIZE..WIN_SIZE + BUF_SIZE]);
                window = 0;
                if buflen == 0 {
                    break;
                }
                size += buflen as u64;
            }
        }
    }

    let tail = &buffer[window..window + winlen];
    for (i, &first) in tail.iter().enumerate() {
        let mut h = u32::from(first);
        hashes[h as usize] &= !0x01;
        let mut k = 0x02u8;
        for &b in &tail[i + 1..] {
            h = indexhash(h, b);
            hashes[h as usize] &= !k;
            k = k.wrapping_shl(1);
        }
    }
    drop(input);

    let mut hashes_size = hashes.len();
    let zeros: u32 = hashes.iter().map(|b| b.count_zeros()).sum();
    let mut noise = zeros as f32 / (8 * hashes_size) as f32;

    // compress the table in place until the given accuracy max noise is reached or exceeded
    while hashes_size > MIN_SIZE {
        // compute noise of halved hashes table (zero bits are hits)
        let half = hashes_size / 2;
        let (lo, hi) = hashes[..hashes_size].split_at_mut(half);
        let hits: u32 = lo.iter().zip(hi.iter()).map(|(a, b)| (a & b).count_zeros()).sum();
        let half_noise = hits as f32 / (8 * half) as f32;

        // stop at desired accuracy
        if 100.0 * f64::from(half_noise) >= f64::from(max_noise) {
            break;
        }

        for (a, b) in lo.iter_mut().zip(hi.iter()) {
            *a &= *b;
        }
        hashes_size = half;
        noise = half_noise;
    }

    hashes.truncate(hashes_size);
    r.hashes = hashes;
    r.noise = noise;
    r.size = size;
    r
}

/// Releases the stream state behind `handle`; called by `~Stream` when a handle is still held.
/// The stream of `index` never outlives its call, so there is nothing to release.
pub fn stream_drop(_handle: u64) {}

/// `noise_percentage()`: noise level in percent for accuracy 0 to 9. Internal to the unit, not a seam function.
pub fn noise_percentage(accuracy: i32) -> u32 {
    let percentage = (10.0 + 70.0 * f64::from(9 - accuracy) / 9.0) as f32;
    100u32.wrapping_sub((100.0 - f64::from(percentage)) as u32)
}

/// `is_binary()`: true if the bytes contain a NUL or an invalid UTF-8 sequence. Internal to the unit.
pub fn is_binary(bytes: &[u8]) -> bool {
    let e = bytes.len();
    let mut s = 0usize;
    // a continuation byte 10xxxxxx at s, consumed either way
    let bad_cont = |s: &mut usize| {
        if *s >= e {
            return true;
        }
        let bad = bytes[*s] & 0xc0 != 0x80;
        *s += 1;
        bad
    };
    while s < e {
        let mut c: i8 = 0;
        while s < e {
            c = bytes[s] as i8;
            if c <= 0 {
                break;
            }
            s += 1;
        }
        let at_end = s >= e;
        s += 1;
        if at_end {
            break;
        }
        // U+0080 ~ U+07ff <-> c2 80 ~ df bf (disallow 2 byte overlongs)
        if !(-62..=-12).contains(&c) || bad_cont(&mut s) {
            return true;
        }
        // U+0800 ~ U+ffff <-> e0 a0 80 ~ ef bf bf (quick but allows surrogates and 3 byte overlongs)
        if c >= -32 && bad_cont(&mut s) {
            return true;
        }
        // U+010000 ~ U+10ffff <-> f0 90 80 80 ~ f4 8f bf bf (quick but allows 4 byte overlongs)
        if c >= -16 && bad_cont(&mut s) {
            return true;
        }
    }
    false
}

/// The 65536-byte table `index()` fills, before compression. Internal to the unit.
pub fn new_table() -> Bytes {
    vec![0xff; 65536]
}

/// prime 61 mod 2^16 file indexing hash function
fn indexhash(h: u32, b: u8) -> u32 {
    u32::from((h << 6).wrapping_sub(h).wrapping_sub(h).wrapping_sub(h).wrapping_add(u32::from(b)) as u16)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Plain,
    Utf8,
    Utf16Be,
    Utf16Le,
    Utf32Be,
    Utf32Le,
}

/// `reflex::Input` over a `FILE*`: UTF BOM detection and UTF-16/32 to UTF-8 conversion, as `lib/input.cpp`.
struct Input {
    file: File,
    utf8: [u8; 8],
    uidx: usize,
    ulen: usize,
    enc: Encoding,
}

/// UTF-8 encoding of `c` as `reflex::utf8()`: out of range code points give `NONCHAR_UTF8`.
fn utf8(c: i32, out: &mut [u8; 8]) -> usize {
    if c < 0x80 {
        out[0] = c as u8;
        return 1;
    }
    if c > 0x10_ffff {
        out[..5].copy_from_slice(&NONCHAR_UTF8);
        return 5;
    }
    let mut n = 0;
    let mut put = |b: i32| {
        out[n] = b as u8;
        n += 1;
    };
    if c < 0x0800 {
        put(0xc0 | ((c >> 6) & 0x1f));
    } else {
        if c < 0x1_0000 {
            put(0xe0 | ((c >> 12) & 0x0f));
        } else {
            put(0xf0 | ((c >> 18) & 0x07));
            put(0x80 | ((c >> 12) & 0x3f));
        }
        put(0x80 | ((c >> 6) & 0x3f));
    }
    put(0x80 | (c & 0x3f));
    n
}

impl Input {
    /// Opens the file like `fopenw_s(file, pathname, "rb")` and runs `Input::file_init`; None when it cannot be opened.
    fn open(pathname: &[u8]) -> Option<Input> {
        let file = open_read(pathname)?;
        let mut input = Input { file, utf8: [0; 8], uidx: 0, ulen: 0, enc: Encoding::Plain };
        input.init();
        Some(input)
    }

    /// `fread(s, 1, s.len(), file)`: reads until `s` is full or the end of the file, returns the count.
    fn fread(&mut self, s: &mut [u8]) -> usize {
        let mut total = 0;
        while total < s.len() {
            match self.file.read(&mut s[total..]) {
                Ok(0) => break,
                Ok(k) => total += k,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        total
    }

    /// `Input::file_read` into `utf8[at..at + n]`: false when fewer than `n` bytes were read; those stay consumed.
    fn file_read_utf8(&mut self, at: usize, n: usize) -> bool {
        let mut tmp = [0u8; 4];
        let k = self.fread(&mut tmp[..n]);
        self.utf8[at..at + k].copy_from_slice(&tmp[..k]);
        k == n
    }

    /// `Input::file_read` into `buf[at..at + n]`.
    fn file_read_buf(&mut self, buf: &mut [u8; 4], at: usize, n: usize) -> bool {
        self.fread(&mut buf[at..at + n]) == n
    }

    /// `Input::file_init`: detect a UTF BOM from the first bytes of the file.
    fn init(&mut self) {
        self.enc = Encoding::Plain;
        if !self.file_read_utf8(0, 1) {
            return;
        }
        self.ulen = 1;
        let b0 = self.utf8[0];
        if !matches!(b0, 0x00 | 0xef | 0xfe | 0xff) || !self.file_read_utf8(1, 1) {
            return;
        }
        self.ulen = 2;
        let b1 = self.utf8[1];
        if b0 == 0x00 && b1 == 0x00 {
            // UTF-32 big endian BOM 0000FEFF?
            if self.file_read_utf8(2, 2) {
                self.ulen = 4;
                if self.utf8[2] == 0xfe && self.utf8[3] == 0xff {
                    self.ulen = 0;
                    self.enc = Encoding::Utf32Be;
                }
            }
        } else if b0 == 0xfe && b1 == 0xff {
            // UTF-16 big endian BOM FEFF
            self.ulen = 0;
            self.enc = Encoding::Utf16Be;
        } else if b0 == 0xff && b1 == 0xfe {
            // UTF-16 or UTF-32 little endian BOM FFFE
            if self.file_read_utf8(2, 2) {
                if self.utf8[2] == 0 && self.utf8[3] == 0 {
                    self.ulen = 0;
                    self.enc = Encoding::Utf32Le;
                } else {
                    let mut c = i32::from(self.utf8[2]) | i32::from(self.utf8[3]) << 8;
                    if c < 0x80 {
                        self.uidx = 2;
                        self.ulen = 1;
                    } else {
                        if (0xd800..0xe000).contains(&c) {
                            // UTF-16 surrogate pair
                            if c < 0xdc00 && self.file_read_utf8(0, 2) && self.utf8[1] & 0xfc == 0xdc {
                                let w = i32::from(self.utf8[0]) | i32::from(self.utf8[1]) << 8;
                                c = 0x1_0000 - 0xdc00 + ((c - 0xd800) << 10) + w;
                            } else {
                                c = NONCHAR;
                            }
                        }
                        self.ulen = utf8(c, &mut self.utf8);
                    }
                    self.enc = Encoding::Utf16Le;
                }
            }
        } else if b0 == 0xef && b1 == 0xbb {
            // UTF-8 BOM EFBBBF?
            if self.file_read_utf8(2, 1) {
                self.ulen = 3;
                if self.utf8[2] == 0xbf {
                    self.ulen = 0;
                    self.enc = Encoding::Utf8;
                }
            }
        }
    }

    /// Converts `c` into `utf8` and copies it to `out[*t..]`, up to the room left `*n`; the rest stays pending.
    fn emit(&mut self, c: i32, out: &mut [u8], t: &mut usize, n: &mut usize) {
        let l = utf8(c, &mut self.utf8);
        if *n < l {
            out[*t..*t + *n].copy_from_slice(&self.utf8[..*n]);
            self.uidx = *n;
            self.ulen = l - *n;
            *t += *n;
            *n = 0;
        } else {
            out[*t..*t + l].copy_from_slice(&self.utf8[..l]);
            *t += l;
            *n -= l;
        }
    }

    /// `Input::get(s, n)` for a `FILE*` with `n = out.len()`: the number of bytes written, 0 at the end of the file.
    fn get(&mut self, out: &mut [u8]) -> usize {
        let mut n = out.len();
        let mut t = 0usize;
        if self.ulen > 0 {
            let k = n.min(self.ulen);
            for _ in 0..k {
                out[t] = self.utf8[self.uidx];
                t += 1;
                self.uidx += 1;
            }
            n -= k;
            if n == 0 {
                self.ulen -= k;
                return k;
            }
            self.ulen = 0;
        }
        let mut buf = [0u8; 4];
        match self.enc {
            Encoding::Plain | Encoding::Utf8 => {
                t += self.fread(&mut out[t..t + n]);
            }
            Encoding::Utf16Be | Encoding::Utf16Le => {
                let be = self.enc == Encoding::Utf16Be;
                while n > 0 && self.file_read_buf(&mut buf, 0, 1) && self.file_read_buf(&mut buf, 1, 1) {
                    let mut c = if be {
                        i32::from(buf[0]) << 8 | i32::from(buf[1])
                    } else {
                        i32::from(buf[0]) | i32::from(buf[1]) << 8
                    };
                    if c < 0x80 {
                        out[t] = c as u8;
                        t += 1;
                        n -= 1;
                        continue;
                    }
                    if (0xd800..0xe000).contains(&c) {
                        // UTF-16 surrogate pair, complete the pair
                        let paired = c < 0xdc00 && self.file_read_buf(&mut buf, 2, 2) && {
                            let low = if be { buf[2] } else { buf[3] };
                            low & 0xfc == 0xdc
                        };
                        c = if paired {
                            let w = if be {
                                i32::from(buf[2]) << 8 | i32::from(buf[3])
                            } else {
                                i32::from(buf[2]) | i32::from(buf[3]) << 8
                            };
                            0x1_0000 - 0xdc00 + ((c - 0xd800) << 10) + w
                        } else {
                            NONCHAR
                        };
                    }
                    self.emit(c, out, &mut t, &mut n);
                }
            }
            Encoding::Utf32Be | Encoding::Utf32Le => {
                let be = self.enc == Encoding::Utf32Be;
                while n > 0 && self.file_read_buf(&mut buf, 0, 1) && self.file_read_buf(&mut buf, 1, 3) {
                    let c = if be { u32::from_be_bytes(buf) } else { u32::from_le_bytes(buf) } as i32;
                    if c < 0x80 {
                        out[t] = c as u8;
                        t += 1;
                        n -= 1;
                    } else {
                        self.emit(c, out, &mut t, &mut n);
                    }
                }
            }
        }
        t
    }
}

#[cfg(windows)]
fn open_read(pathname: &[u8]) -> Option<File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
    // MultiByteToWideChar(CP_UTF8): invalid sequences become U+FFFD
    let name = String::from_utf8_lossy(pathname).into_owned();
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)
        .open(name)
        .ok()
}

#[cfg(not(windows))]
fn open_read(pathname: &[u8]) -> Option<File> {
    use std::os::unix::ffi::OsStrExt;
    File::open(std::ffi::OsStr::from_bytes(pathname)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags() -> IndexerFlags {
        IndexerFlags {
            accuracy: 4,
            check: false,
            decompress: false,
            delete: false,
            dereference_files: false,
            force: false,
            hidden: false,
            ignore_binary: false,
            no_messages: false,
            quiet: false,
            usage_warnings: false,
            verbose: false,
            zmax: 1,
            ignore_files: Vec::new(),
        }
    }

    fn temp(name: &str, data: &[u8]) -> Vec<u8> {
        let p = std::env::temp_dir().join(format!("indexer_core_{}_{name}", std::process::id()));
        std::fs::write(&p, data).unwrap();
        p.to_string_lossy().into_owned().into_bytes()
    }

    #[test]
    fn noise_percentage_range() {
        assert_eq!(noise_percentage(4), 49);
        assert_eq!(noise_percentage(9), 10);
        assert_eq!(noise_percentage(0), 80);
    }

    #[test]
    fn binary_detection() {
        assert!(!is_binary(b"plain text\n"));
        assert!(is_binary(b"a\0b"));
        assert!(!is_binary("h\u{e9}llo \u{20ac} \u{1f600}".as_bytes()));
        assert!(is_binary(&[0xc0, 0x80]));
        assert!(is_binary(&[b'a', 0xc3]));
        assert!(is_binary(&[0xf8, 0x88, 0x80, 0x80, 0x80]));
    }

    #[test]
    fn missing_file_fails() {
        let r = index(&flags(), 0, b"no/such/file.txt", false);
        assert!(!r.ok);
        assert!(r.hashes.is_empty());
    }

    #[test]
    fn empty_and_small_files() {
        let p = temp("empty", b"");
        let r = index(&flags(), 0, &p, false);
        assert!(r.ok && r.hashes.is_empty() && r.size == 0);
        let p = temp("small", b"foo\n");
        let r = index(&flags(), 0, &p, false);
        assert!(r.ok && r.size == 4 && r.hashes.len() >= MIN_SIZE && r.hashes.len() <= 65536);
    }

    #[test]
    fn ignore_binary_skips_table() {
        let p = temp("bin", b"a\0b\n");
        let mut f = flags();
        f.ignore_binary = true;
        let r = index(&f, 0, &p, false);
        assert!(r.ok && r.binary && r.hashes.is_empty() && r.size == 0);
    }

    #[test]
    fn utf16_bom_equals_utf8() {
        let text = "h\u{e9}llo w\u{f6}rld \u{20ac}\n";
        let mut u16le = vec![0xff, 0xfe];
        for u in text.encode_utf16() {
            u16le.extend_from_slice(&u.to_le_bytes());
        }
        let a = index(&flags(), 0, &temp("u16", &u16le), false);
        let b = index(&flags(), 0, &temp("u8", text.as_bytes()), false);
        assert_eq!(a.hashes, b.hashes);
        assert_eq!(a.size, b.size);
    }

    #[test]
    fn chunking_is_invisible() {
        let data: Vec<u8> = (0..200_000u32).map(|i| b'a' + (i % 23) as u8).collect();
        let r = index(&flags(), 0, &temp("big", &data), false);
        assert!(r.ok && r.size == 200_000);
    }
}
