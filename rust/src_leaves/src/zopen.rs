//! Types and signatures of the port unit `zopen` (src/zopen.c, .Z compress/decompress), fixed by the
//! Prepare seam task. Stubs only: the bodies belong to the Port task. The module is reached through
//! `signatures.rs` (`mod zopen;`), the only file the replay harness includes.
use core::ffi::c_void;

/// Opaque state of one open stream (`struct s_zstate` in C).
pub struct ZState {
    _private: (),
}

/// `z_open(FILE *file, const char *mode, int bits, int plain)`. `file` is the C `FILE*` (owned by the
/// caller until `z_close`), `mode` the bytes of the C string without NUL. `None` is a NULL return
/// (errno is set by the C++ on its failure paths).
pub fn z_open(_file: *mut c_void, _mode: &[u8], _bits: i32, _plain: i32) -> Option<Box<ZState>> {
    None
}

/// `z_close(void *)`: 0 on success, -1 on error.
pub fn z_close(_state: ZState) -> i32 {
    -1
}

/// `z_read(void *, unsigned char *, int)`: bytes read, 0 at end of file, negative on error.
pub fn z_read(_state: &mut ZState, _buf: &mut [u8]) -> i32 {
    -1
}

/// `z_write(void *, const unsigned char *, int)`: bytes written or negative on error.
pub fn z_write(_state: &mut ZState, _buf: &[u8]) -> i32 {
    -1
}
