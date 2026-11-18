//! C ABI of the output buffer, `Output::Tree` and `Output::Dump::hex` for src/output.hpp. The buffer handle is a
//! `Box<OutBuf>`; C++ keeps `cur`/`end` cursors into the current block and calls in only when a block is full, to
//! flush or to reset. Panics abort (panic=abort), so none unwinds into C++.

use crate::output::{Opts, TreeState};
use crate::output_buf::{dump_hex, DumpState, OutBuf, Sink, Tree, SIZE};
use std::ffi::{c_char, c_int, c_void};
use std::sync::{Mutex, MutexGuard};

extern "C" {
    fn fwrite(ptr: *const c_void, size: usize, count: usize, stream: *mut c_void) -> usize;
    fn fflush(stream: *mut c_void) -> c_int;
}

/// The cursor C++ appends with: the next free byte and the end of the current block.
#[repr(C)]
pub struct ObCursor {
    pub cur: *mut u8,
    pub end: *mut u8,
}

fn set(c: &mut ObCursor, block: *mut u8) {
    c.cur = block;
    // SAFETY: `block` points at an allocation of SIZE bytes, so one past its end is a valid pointer.
    c.end = unsafe { block.add(SIZE) };
}

struct FileSink(*mut c_void);

impl Sink for FileSink {
    fn write(&mut self, data: &[u8]) -> bool {
        // SAFETY: `self.0` is the live C stream of the `Output`, and `data` is a valid slice.
        unsafe { fwrite(data.as_ptr().cast(), 1, data.len(), self.0) == data.len() }
    }
    fn flush(&mut self) -> bool {
        // SAFETY: as for `write`.
        unsafe { fflush(self.0) == 0 }
    }
}

/// Creates a buffer and sets `c` to its first block.
///
/// # Safety
/// `c` is a valid pointer to an `ObCursor`.
#[no_mangle]
pub unsafe extern "C" fn ugrep_ob_new(c: *mut ObCursor) -> *mut OutBuf {
    let mut b = Box::new(OutBuf::new());
    // SAFETY: `c` is valid, per the contract.
    set(unsafe { &mut *c }, b.current());
    Box::into_raw(b)
}

/// # Safety
/// `ob` comes from `ugrep_ob_new` and is not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn ugrep_ob_free(ob: *mut OutBuf) {
    if !ob.is_null() {
        // SAFETY: the pointer came from `Box::into_raw` and is freed once, per the contract.
        drop(unsafe { Box::from_raw(ob) });
    }
}

/// Moves on to the next block (allocating one if needed) and sets `c` to it.
///
/// # Safety
/// `ob` is a live buffer used by one thread, `c` is a valid cursor.
#[no_mangle]
pub unsafe extern "C" fn ugrep_ob_advance(ob: *mut OutBuf, c: *mut ObCursor) {
    // SAFETY: valid and exclusive, per the contract.
    let (ob, c) = unsafe { (&mut *ob, &mut *c) };
    set(c, ob.advance());
}

/// Discards the buffered bytes and sets `c` to the first block.
///
/// # Safety
/// As for `ugrep_ob_advance`.
#[no_mangle]
pub unsafe extern "C" fn ugrep_ob_reset(ob: *mut OutBuf, c: *mut ObCursor) {
    // SAFETY: valid and exclusive, per the contract.
    let (ob, c) = unsafe { (&mut *ob, &mut *c) };
    set(c, ob.reset());
}

/// True if there is buffered output to flush.
///
/// # Safety
/// `ob` is a live buffer and `c` is its cursor.
#[no_mangle]
pub unsafe extern "C" fn ugrep_ob_pending(ob: *mut OutBuf, c: *const ObCursor) -> bool {
    // SAFETY: valid, per the contract.
    let (ob, c) = unsafe { (&mut *ob, &*c) };
    // SAFETY: the cursor points into the current block, which starts at `ob.current()`.
    let used = unsafe { c.cur.offset_from(ob.current()) };
    ob.pending(usize::try_from(used).unwrap_or(0))
}

/// Writes the buffered output to `file`; true if the stream failed. The caller holds the output lock and resets the
/// buffer afterwards.
///
/// # Safety
/// `ob` is a live buffer and `c` its cursor, `file` is an open C `FILE*`.
#[no_mangle]
pub unsafe extern "C" fn ugrep_ob_write(ob: *mut OutBuf, c: *const ObCursor, file: *mut c_void) -> bool {
    // SAFETY: valid and exclusive, per the contract.
    let (ob, c) = unsafe { (&mut *ob, &*c) };
    // SAFETY: the cursor points into the current block.
    let used = usize::try_from(unsafe { c.cur.offset_from(ob.current()) }).unwrap_or(0);
    let o = crate::output_ffi::format_opts();
    let null_data = settings_null_data();
    ob.write(&o, used, null_data, &mut FileSink(file))
}

fn settings_null_data() -> bool {
    // SAFETY: the settings are written by C++ option parsing before any output and only read here.
    unsafe { (*crate::settings_ffi::UGREP_SETTINGS.as_ptr()).flag_null_data }
}

/// The result of `ugrep_dump_hex`: the lines to append, valid until the next call on this thread.
#[repr(C)]
pub struct ObBytes {
    pub data: *const u8,
    pub len: usize,
}

thread_local! {
    static HEX_OUT: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// `Output::Dump::hex`: updates the dump state and returns the completed hex lines.
///
/// # Safety
/// `st` is a valid dump state; `data` points to `size` readable bytes (or `size` is 0).
#[no_mangle]
pub unsafe extern "C" fn ugrep_dump_hex(st: *mut DumpState, mode: i16, byte_offset: usize, data: *const u8, size: usize) -> ObBytes {
    // SAFETY: valid, per the contract.
    let st = unsafe { &mut *st };
    let data: &[u8] = if size == 0 {
        &[]
    } else {
        // SAFETY: valid, per the contract.
        unsafe { std::slice::from_raw_parts(data, size) }
    };
    let o: Opts = crate::output_ffi::format_opts();
    let bytes = dump_hex(&o, st, mode, byte_offset, data);
    HEX_OUT.with(|h| {
        let mut h = h.borrow_mut();
        *h = bytes;
        ObBytes { data: h.as_ptr(), len: h.len() }
    })
}

/// The `--tree` state shared by all outputs; C++ takes the output lock (`acquire`) around its use.
pub static TREE: Mutex<Option<Tree>> = Mutex::new(None);

pub fn tree() -> MutexGuard<'static, Option<Tree>> {
    TREE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn with_tree<R>(f: impl FnOnce(&mut Tree) -> R) -> R {
    f(tree().get_or_insert_with(Tree::new))
}

/// `Tree::path.clear(); Tree::depth = 0`.
#[no_mangle]
pub extern "C" fn ugrep_tree_reset() {
    with_tree(|t| t.state = TreeState { path: Vec::new(), depth: 0 });
}

#[no_mangle]
pub extern "C" fn ugrep_tree_depth() -> c_int {
    with_tree(|t| t.state.depth)
}

/// Sets the drawing strings (`Tree::bar`, `Tree::ptr`, `Tree::end`).
///
/// # Safety
/// The three pointers are NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn ugrep_tree_set_glyphs(bar: *const c_char, ptr: *const c_char, end: *const c_char) {
    // SAFETY: NUL-terminated, per the contract.
    let (b, p, e) = unsafe { (std::ffi::CStr::from_ptr(bar), std::ffi::CStr::from_ptr(ptr), std::ffi::CStr::from_ptr(end)) };
    with_tree(|t| {
        t.bar = b.to_bytes().to_vec();
        t.ptr = p.to_bytes().to_vec();
        t.end = e.to_bytes().to_vec();
        t.end_c = e.to_bytes_with_nul().to_vec();
    });
}

/// `Tree::end` as a NUL-terminated string that stays valid until the glyphs are set again.
#[no_mangle]
pub extern "C" fn ugrep_tree_end() -> *const c_char {
    with_tree(|t| t.end_c.as_ptr().cast())
}