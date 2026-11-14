//! Port of the unit `zopen` (src/zopen.c, .Z compress/decompress, LZW). Reached through `signatures.rs`
//! (`pub mod zopen;`), the only file the replay harness includes. The stream is the caller's `FILE*`,
//! read and written through the C runtime (`ffi/cfile.rs`); no codec of the retained C libraries is involved.
//! Integer types follow the C: `long` is `c_long` (32 bit on Windows), so wrapping matches the C++ build.
use core::ffi::{c_long, c_void};

#[path = "ffi/cfile.rs"]
mod cfile;

const BITS: u32 = 16;
const HSIZE: usize = 69001;
const BIT_MASK: u8 = 0x1f;
const BLOCK_MASK: u8 = 0x80;
const INIT_BITS: u32 = 9;
const CHECK_GAP: c_long = 10000;
const FIRST: c_long = 257;
const CLEAR: c_long = 256;
const MAGIC_HEADER: [u8; 2] = [0x1f, 0x9d];
const LMASK: [u8; 9] = [0xff, 0xfe, 0xfc, 0xf8, 0xf0, 0xe0, 0xc0, 0x80, 0x00];
const RMASK: [u8; 9] = [0x00, 0x01, 0x03, 0x07, 0x0f, 0x1f, 0x3f, 0x7f, 0xff];

fn maxcode_of(n_bits: u32) -> c_long {
    (1 << n_bits) - 1
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Start,
    Middle,
    Eof,
}

/// Opaque state of one open stream (`struct s_zstate` in C). The C overlays the read and write tables
/// in one union; here each side has its own fields, and a handle used both ways is refused (see `z_read`).
pub struct ZState {
    file: *mut c_void,
    /// `zs_mode`: the C never sets it, so `z_close` never flushes (catalog B-src_leaves-030).
    mode: u8,
    magic: i32,
    state: Phase,
    reading: bool,
    n_bits: u32,
    maxbits: u32,
    maxcode: c_long,
    maxmaxcode: c_long,
    htab: Vec<c_long>,
    codetab: Vec<u16>,
    suffix: Vec<u8>,
    hsize: c_long,
    free_ent: c_long,
    block_compress: i32,
    clear_flg: i32,
    ratio: c_long,
    checkpoint: c_long,
    offset: u32,
    in_count: c_long,
    bytes_out: c_long,
    out_count: c_long,
    buf: [u8; BITS as usize],
    fcode: c_long,
    ent: c_long,
    hsize_reg: c_long,
    hshift: i32,
    stack: Vec<u8>,
    finchar: i32,
    oldcode: c_long,
    incode: c_long,
    roffset: i32,
    size: i32,
    gbuf: [u8; BITS as usize],
}

/// `z_open(FILE *file, const char *mode, int bits, int plain)`. `file` is the C `FILE*` (owned by the
/// caller until `z_close`), `mode` the bytes of the C string without NUL. `None` is a NULL return
/// (errno is set on the failure path).
pub fn z_open(file: *mut c_void, mode: &[u8], bits: i32, plain: i32) -> Option<Box<ZState>> {
    let mode0 = mode.first().copied().unwrap_or(0);
    if (mode0 != b'r' && mode0 != b'w') || mode.len() > 1 || !(0..=BITS as i32).contains(&bits) {
        cfile::set_errno(cfile::EINVAL);
        return None;
    }
    let maxbits = if bits != 0 { bits as u32 } else { BITS };
    Some(Box::new(ZState {
        file,
        mode: 0,
        magic: if plain != 0 { 0 } else { 2 },
        state: Phase::Start,
        reading: false,
        n_bits: 0,
        maxbits,
        maxcode: 0,
        maxmaxcode: 1 << maxbits,
        htab: vec![0; HSIZE],
        codetab: vec![0; HSIZE],
        suffix: vec![0; 1 << BITS],
        hsize: HSIZE as c_long,
        free_ent: 0,
        block_compress: BLOCK_MASK as i32,
        clear_flg: 0,
        ratio: 0,
        checkpoint: CHECK_GAP,
        offset: 0,
        in_count: 1,
        bytes_out: 0,
        out_count: 0,
        buf: [0; BITS as usize],
        fcode: 0,
        ent: 0,
        hsize_reg: 0,
        hshift: 0,
        stack: Vec::new(),
        finchar: 0,
        oldcode: 0,
        incode: 0,
        roffset: 0,
        size: 0,
        gbuf: [0; BITS as usize],
    }))
}

/// `z_close(void *)`: 0 on success, -1 on error. The state is freed either way; the caller closes the `FILE*`.
pub fn z_close(mut zs: ZState) -> i32 {
    if zs.mode == b'w' {
        let ent = zs.ent;
        if output(&mut zs, ent) == -1 {
            return -1;
        }
        zs.out_count = zs.out_count.wrapping_add(1);
        if output(&mut zs, -1) == -1 {
            return -1;
        }
    }
    0
}

/// `z_write(void *, const unsigned char *, int)`: bytes written or negative on error. A handle already used
/// for reading is refused with -1 (the C crashes on it, plan difference D-zopen-1).
pub fn z_write(zs: &mut ZState, data: &[u8]) -> i32 {
    let num = data.len();
    if num == 0 {
        return 0;
    }
    if zs.state == Phase::Middle && zs.reading {
        return -1;
    }
    let mut bp = 0;
    if zs.state != Phase::Middle {
        zs.state = Phase::Middle;
        zs.reading = false;
        zs.maxmaxcode = 1 << zs.maxbits;
        if zs.magic != 0 && cfile::write(zs.file, &MAGIC_HEADER) != MAGIC_HEADER.len() {
            return -1;
        }
        let tmp = (zs.maxbits as u8) | zs.block_compress as u8;
        if cfile::write(zs.file, &[tmp]) != 1 {
            return -1;
        }
        zs.offset = 0;
        zs.bytes_out = 3;
        zs.out_count = 0;
        zs.clear_flg = 0;
        zs.ratio = 0;
        zs.in_count = 1;
        zs.checkpoint = CHECK_GAP;
        zs.n_bits = INIT_BITS;
        zs.maxcode = maxcode_of(INIT_BITS);
        zs.free_ent = if zs.block_compress != 0 { FIRST } else { 256 };
        zs.ent = data[0] as c_long;
        bp = 1;
        zs.hshift = 0;
        zs.fcode = zs.hsize;
        while zs.fcode < 65536 {
            zs.hshift += 1;
            zs.fcode *= 2;
        }
        zs.hshift = 8 - zs.hshift;
        zs.hsize_reg = zs.hsize;
        let n = zs.hsize_reg;
        cl_hash(zs, n);
    }

    'bytes: while bp < num {
        let c = data[bp] as i32;
        bp += 1;
        zs.in_count = zs.in_count.wrapping_add(1);
        zs.fcode = ((c as c_long) << zs.maxbits).wrapping_add(zs.ent);
        let mut i = ((c << zs.hshift) as c_long) ^ zs.ent;

        if zs.htab[i as usize] == zs.fcode {
            zs.ent = zs.codetab[i as usize] as c_long;
            continue;
        } else if zs.htab[i as usize] >= 0 {
            let mut disp = (zs.hsize_reg as i64 - i as i64) as i32;
            if i == 0 {
                disp = 1;
            }
            loop {
                i -= disp as c_long;
                if i < 0 {
                    i += zs.hsize_reg;
                }
                if zs.htab[i as usize] == zs.fcode {
                    zs.ent = zs.codetab[i as usize] as c_long;
                    continue 'bytes;
                }
                if zs.htab[i as usize] < 0 {
                    break;
                }
            }
        }
        let ent = zs.ent;
        if output(zs, ent) == -1 {
            return -1;
        }
        zs.out_count = zs.out_count.wrapping_add(1);
        zs.ent = c as c_long;
        if zs.free_ent < zs.maxmaxcode {
            zs.codetab[i as usize] = zs.free_ent as u16;
            zs.free_ent += 1;
            zs.htab[i as usize] = zs.fcode;
        } else if zs.in_count >= zs.checkpoint && zs.block_compress != 0 && cl_block(zs) == -1 {
            return -1;
        }
    }
    num as i32
}

/// Writes the code `ocode` (n_bits wide) into the bit buffer, flushing it when full; `-1` flushes the rest.
fn output(zs: &mut ZState, mut ocode: c_long) -> i32 {
    let mut r_off = zs.offset as usize;
    let mut bits = zs.n_bits;
    let n = zs.n_bits as usize;
    if ocode >= 0 {
        let mut bp = r_off >> 3;
        r_off &= 7;
        zs.buf[bp] = (zs.buf[bp] & RMASK[r_off]) | (((ocode << r_off) & LMASK[r_off] as c_long) as u8);
        bp += 1;
        bits -= 8 - r_off as u32;
        ocode >>= 8 - r_off;
        if bits >= 8 {
            zs.buf[bp] = ocode as u8;
            bp += 1;
            ocode >>= 8;
            bits -= 8;
        }
        if bits != 0 {
            zs.buf[bp] = ocode as u8;
        }
        zs.offset += zs.n_bits;
        if zs.offset == zs.n_bits << 3 {
            zs.bytes_out = zs.bytes_out.wrapping_add(zs.n_bits as c_long);
            if cfile::write(zs.file, &zs.buf[..n]) != n {
                return -1;
            }
            zs.offset = 0;
        }
        if zs.free_ent > zs.maxcode || zs.clear_flg > 0 {
            if zs.offset > 0 {
                if cfile::write(zs.file, &zs.buf[..n]) != n {
                    return -1;
                }
                zs.bytes_out = zs.bytes_out.wrapping_add(zs.n_bits as c_long);
            }
            zs.offset = 0;
            if zs.clear_flg != 0 {
                zs.n_bits = INIT_BITS;
                zs.maxcode = maxcode_of(INIT_BITS);
                zs.clear_flg = 0;
            } else {
                zs.n_bits += 1;
                zs.maxcode = if zs.n_bits == zs.maxbits { zs.maxmaxcode } else { maxcode_of(zs.n_bits) };
            }
        }
    } else {
        if zs.offset > 0 {
            zs.offset = zs.offset.div_ceil(8);
            let m = zs.offset as usize;
            if cfile::write(zs.file, &zs.buf[..m]) != m {
                return -1;
            }
            zs.bytes_out = zs.bytes_out.wrapping_add(zs.offset as c_long);
        }
        zs.offset = 0;
    }
    0
}

/// Table clear for block compression.
fn cl_block(zs: &mut ZState) -> i32 {
    zs.checkpoint = zs.in_count.wrapping_add(CHECK_GAP);
    let rat = if zs.in_count > 0x007f_ffff {
        let r = zs.bytes_out >> 8;
        if r == 0 {
            0x7fff_ffff
        } else {
            zs.in_count / r
        }
    } else {
        (zs.in_count << 8).checked_div(zs.bytes_out).unwrap_or(0x7fff_ffff)
    };
    if rat > zs.ratio {
        zs.ratio = rat;
    } else {
        zs.ratio = 0;
        let n = zs.hsize;
        cl_hash(zs, n);
        zs.free_ent = FIRST;
        zs.clear_flg = 1;
        if output(zs, CLEAR) == -1 {
            return -1;
        }
    }
    0
}

/// Resets the first `n` hash table entries to empty (-1).
fn cl_hash(zs: &mut ZState, n: c_long) {
    zs.htab[..n as usize].fill(-1);
}

/// `z_read(void *, unsigned char *, int)`: bytes read, 0 at end of file, negative on error (errno EINVAL for a
/// bad stream). A handle that is not at a readable position (already written, or read after a failure or
/// after the first read found an empty stream) is refused with -1; the C crashes there (D-zopen-1).
pub fn z_read(zs: &mut ZState, out: &mut [u8]) -> i32 {
    let num = out.len();
    if num == 0 {
        return 0;
    }
    let mut count = num;
    let mut bp = 0;
    let mut resume = false;
    match zs.state {
        Phase::Start => {
            zs.state = Phase::Middle;
            zs.reading = true;
            let mut header = [0u8; 3];
            let magic = zs.magic as usize;
            let want = magic + 1;
            // the C compares the fread result truncated to char
            if (cfile::read(zs.file, &mut header[..want]) as u8 as i8) as i32 != want as i32
                || header[..magic] != MAGIC_HEADER[..magic]
            {
                cfile::set_errno(cfile::EINVAL);
                return -1;
            }
            zs.maxbits = header[magic] as u32;
            zs.block_compress = (zs.maxbits & BLOCK_MASK as u32) as i32;
            zs.maxbits &= BIT_MASK as u32;
            zs.maxmaxcode = 1 << zs.maxbits;
            if !(12..=BITS).contains(&zs.maxbits) {
                cfile::set_errno(cfile::EINVAL);
                return -1;
            }
            zs.n_bits = INIT_BITS;
            zs.maxcode = maxcode_of(INIT_BITS);
            for code in 0..256 {
                zs.codetab[code] = 0;
                zs.suffix[code] = code as u8;
            }
            zs.free_ent = if zs.block_compress != 0 { FIRST } else { 256 };
            zs.oldcode = getcode(zs);
            zs.finchar = zs.oldcode as i64 as i32;
            if zs.oldcode == -1 {
                return 0;
            }
            out[bp] = zs.finchar as u8;
            bp += 1;
            count -= 1;
            zs.stack.clear();
        }
        Phase::Middle => {
            if !zs.reading || zs.stack.is_empty() {
                return -1;
            }
            resume = true;
        }
        Phase::Eof => return 0,
    }

    loop {
        if !resume {
            let mut code = getcode(zs);
            if code <= -1 {
                break;
            }
            if code == CLEAR && zs.block_compress != 0 {
                zs.codetab[..256].fill(0);
                zs.clear_flg = 1;
                zs.free_ent = FIRST;
                zs.oldcode = -1;
                continue;
            }
            zs.incode = code;
            if code >= zs.free_ent {
                if code > zs.free_ent || zs.oldcode == -1 {
                    cfile::set_errno(cfile::EINVAL);
                    return -1;
                }
                zs.stack.push(zs.finchar as u8);
                code = zs.oldcode;
            }
            while code >= 256 {
                // a crafted stream can make the prefix chain a cycle; the C overruns its stack here
                if zs.stack.len() >= 1 << BITS {
                    cfile::set_errno(cfile::EINVAL);
                    return -1;
                }
                zs.stack.push(zs.suffix[code as usize]);
                code = zs.codetab[code as usize] as c_long;
            }
            zs.finchar = zs.suffix[code as usize] as i32;
            zs.stack.push(zs.finchar as u8);
        }
        resume = false;

        loop {
            if count == 0 {
                return num as i32;
            }
            count -= 1;
            out[bp] = zs.stack.pop().unwrap_or(0);
            bp += 1;
            if zs.stack.is_empty() {
                break;
            }
        }

        let code = zs.free_ent;
        if code < zs.maxmaxcode && zs.oldcode != -1 {
            zs.codetab[code as usize] = zs.oldcode as u16;
            zs.suffix[code as usize] = zs.finchar as u8;
            zs.free_ent = code + 1;
        }
        zs.oldcode = zs.incode;
    }
    zs.state = Phase::Eof;
    (num - count) as i32
}

/// Reads one code from the stream, or -1 at end of file.
fn getcode(zs: &mut ZState) -> c_long {
    if zs.clear_flg > 0 || zs.roffset >= zs.size || zs.free_ent > zs.maxcode {
        if zs.free_ent > zs.maxcode {
            zs.n_bits += 1;
            zs.maxcode = if zs.n_bits == zs.maxbits { zs.maxmaxcode } else { maxcode_of(zs.n_bits) };
        }
        if zs.clear_flg > 0 {
            zs.n_bits = INIT_BITS;
            zs.maxcode = maxcode_of(INIT_BITS);
            zs.clear_flg = 0;
        }
        let n = zs.n_bits as usize;
        zs.size = cfile::read(zs.file, &mut zs.gbuf[..n]) as i32;
        if zs.size <= 0 {
            return -1;
        }
        zs.roffset = 0;
        zs.size = (zs.size << 3) - (zs.n_bits as i32 - 1);
    }
    let mut r_off = zs.roffset as usize;
    let mut bits = zs.n_bits as i32;

    let mut bp = r_off >> 3;
    r_off &= 7;

    let mut gcode = (zs.gbuf[bp] >> r_off) as c_long;
    bp += 1;
    bits -= 8 - r_off as i32;
    r_off = 8 - r_off;

    if bits >= 8 {
        gcode |= (zs.gbuf[bp] as c_long) << r_off;
        bp += 1;
        r_off += 8;
        bits -= 8;
    }

    if bp < zs.gbuf.len() {
        gcode |= ((zs.gbuf[bp] & RMASK[bits as usize]) as c_long) << r_off;
    }

    zs.roffset += zs.n_bits as i32;
    gcode
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream_over(data: &[u8]) -> *mut c_void {
        let f = cfile::temp_file();
        assert!(!f.is_null());
        if !data.is_empty() {
            assert_eq!(cfile::write(f, data), data.len());
        }
        cfile::rewind_file(f);
        f
    }

    fn compress(data: &[u8], bits: i32, plain: i32) -> Vec<u8> {
        let f = stream_over(&[]);
        let mut z = z_open(f, b"w", bits, plain).unwrap();
        for chunk in data.chunks(777) {
            assert_eq!(z_write(&mut z, chunk), chunk.len() as i32);
        }
        // the C never flushes on close (B-src_leaves-030); finish the stream as z_close would in w mode
        let ent = z.ent;
        assert_eq!(output(&mut z, ent), 0);
        assert_eq!(output(&mut z, -1), 0);
        cfile::rewind_file(f);
        let mut out = Vec::new();
        loop {
            let mut b = [0u8; 4096];
            let n = cfile::read(f, &mut b);
            if n == 0 {
                break;
            }
            out.extend_from_slice(&b[..n]);
        }
        cfile::close_file(f);
        out
    }

    fn decompress(data: &[u8], plain: i32) -> Result<Vec<u8>, i32> {
        let f = stream_over(data);
        let mut z = z_open(f, b"r", 0, plain).unwrap();
        let mut out = Vec::new();
        let result = loop {
            let mut b = [0u8; 1000];
            let n = z_read(&mut z, &mut b);
            if n < 0 {
                break Err(n);
            }
            if n == 0 {
                break Ok(out);
            }
            out.extend_from_slice(&b[..n as usize]);
        };
        assert_eq!(z_close(*z), 0);
        cfile::close_file(f);
        result
    }

    fn sample() -> Vec<u8> {
        let mut v = Vec::new();
        let mut x: u32 = 12345;
        for i in 0..200_000u32 {
            x = x.wrapping_mul(1103515245).wrapping_add(12345);
            v.push(if i % 5 == 0 { (x >> 16) as u8 } else { b"abcabcabd"[(i % 9) as usize] });
        }
        v
    }

    #[test]
    fn roundtrip_with_and_without_magic() {
        let data = sample();
        for (bits, plain) in [(0, 0), (12, 0), (16, 1), (13, 1)] {
            let z = compress(&data, bits, plain);
            if plain == 0 {
                assert_eq!(z[..2], MAGIC_HEADER);
            }
            assert_eq!(decompress(&z, plain).unwrap(), data, "bits {bits} plain {plain}");
        }
    }

    #[test]
    fn bad_input_is_rejected_with_einval() {
        assert_eq!(decompress(b"", 0), Err(-1));
        assert_eq!(decompress(b"\x1f\x8b\x10", 0), Err(-1));
        assert_eq!(decompress(b"\x1f\x9d\x8b", 0), Err(-1));
    }

    #[test]
    fn open_validates_mode_and_bits() {
        assert!(z_open(std::ptr::null_mut(), b"x", 0, 0).is_none());
        assert!(z_open(std::ptr::null_mut(), b"", 0, 0).is_none());
        assert!(z_open(std::ptr::null_mut(), b"rb", 0, 0).is_none());
        assert!(z_open(std::ptr::null_mut(), b"r", 17, 0).is_none());
        assert!(z_open(std::ptr::null_mut(), b"r", -1, 0).is_none());
        assert!(z_open(std::ptr::null_mut(), b"w", 16, 0).is_some());
    }

    // D-zopen-2: a prefix cycle in a crafted stream fails with -1 instead of growing the decode stack forever
    #[test]
    fn crafted_prefix_cycle_is_bounded() {
        let mut codes: Vec<u32> = vec![300];
        codes.extend(std::iter::repeat_n(65, 42));
        codes.extend([257, 65, 300]);
        let mut data = vec![0x1f, 0x9d, 0x90];
        let (mut acc, mut nbits) = (0u32, 0u32);
        for c in codes {
            acc |= c << nbits;
            nbits += 9;
            while nbits >= 8 {
                data.push(acc as u8);
                acc >>= 8;
                nbits -= 8;
            }
        }
        if nbits > 0 {
            data.push(acc as u8);
        }
        assert_eq!(decompress(&data, 0), Err(-1));
    }

    // D-zopen-1 (Q-zopen-6): handle misuse that crashes the C returns -1 here.
    #[test]
    fn misuse_does_not_crash() {
        let data = compress(b"hello hello hello", 0, 0);
        let f = stream_over(&data);
        let mut z = z_open(f, b"r", 0, 0).unwrap();
        let mut b = [0u8; 4];
        assert_eq!(z_read(&mut z, &mut b), 4);
        assert_eq!(z_write(&mut z, b"x"), -1);
        assert_eq!(z_close(*z), 0);
        cfile::close_file(f);

        let f = stream_over(b"");
        let mut z = z_open(f, b"r", 0, 0).unwrap();
        assert_eq!(z_read(&mut z, &mut b), -1);
        assert_eq!(z_read(&mut z, &mut b), -1);
        z_close(*z);
        cfile::close_file(f);

        let f = stream_over(&[]);
        let mut z = z_open(f, b"w", 0, 0).unwrap();
        assert_eq!(z_write(&mut z, b"abc"), 3);
        assert_eq!(z_read(&mut z, &mut b), -1);
        z_close(*z);
        cfile::close_file(f);
    }
}
