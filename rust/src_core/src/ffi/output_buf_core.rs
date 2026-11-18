//! The buffer of `Output` (src/output.hpp): block buffers owned here, their flush to the CRT `FILE*`, line buffering,
//! the hold/ordered modes, the hex dump state (`Output::Dump`) and the `--tree` state (`Output::Tree`).
//!
//! Hot path: C++ appends bytes with inline code straight into the current block through the `Regs` it shares with this
//! object (`cur`, `end`, `mode`): no bridge call per character. Rust is entered only when the block is full
//! (`next`), to flush, and per hex dump or tree request. A block never moves: `Box<[u8]>` storage stays put while the
//! list of blocks grows.
//!
//! Flush uses the CRT `fwrite` and `fflush` on the same `FILE*` that C++ opened (`/MT` on both sides, see
//! msvc/rust.targets), so the bytes are the C++ ones, including the text-mode `\r\n` translation of Windows.

use crate::output::{dump_line, truncate_lines, TreeGlyphs, TreeState, TruncState};
use crate::output_sync::OutputSync;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// `Output::SIZE`: bytes in each block.
pub const SIZE: usize = 32768;
/// `Output::FLUSH`: mode bit, flush each line of output.
pub const FLUSH: usize = 1;
/// `Output::HOLD`: mode bit, hold output.
pub const HOLD: usize = 2;
/// `MAX_HEX_COLUMNS` of src/output.hpp.
pub const MAX_HEX_COLUMNS: usize = 64;

/// The registers C++ and Rust share: the write position in the current block, its end, and the mode bits. C++ owns the
/// memory (a member of `Output`, declared with the same layout in src/output.hpp); only the owner thread touches it.
#[repr(C)]
pub struct Regs {
    pub cur: *mut u8,
    pub end: *mut u8,
    pub mode: usize,
}

extern "C" {
    fn fwrite(buf: *const c_void, size: usize, count: usize, file: *mut c_void) -> usize;
    fn fflush(file: *mut c_void) -> i32;
}

/// The state of `Output::Dump`.
struct Dump {
    offset: usize,
    bytes: [i16; MAX_HEX_COLUMNS],
    prevb: [i16; MAX_HEX_COLUMNS],
    pstar: bool,
}

impl Dump {
    fn new() -> Self {
        Self { offset: 0, bytes: [-1; MAX_HEX_COLUMNS], prevb: [-1; MAX_HEX_COLUMNS], pstar: false }
    }
}

/// The columns of a hex dump line, `--hex-columns`, at least 1 and at most `MAX_HEX_COLUMNS` (the arrays' size).
fn hex_columns() -> usize {
    crate::output_ffi::settings().flag_hex_columns.clamp(1, MAX_HEX_COLUMNS)
}

/// One `Output`: the blocks, the file, the Sync lock and the hex dump state.
pub struct OutputBuf {
    regs: *mut Regs,
    file: *mut c_void,
    eof: *const AtomicBool,
    blocks: Vec<Box<[u8]>>,
    /// Index of the current block in `blocks`.
    index: usize,
    sync: Option<Arc<OutputSync>>,
    /// This output owns the sync (what `std::unique_lock::owns_lock()` was).
    owned: bool,
    slot: usize,
    trunc: TruncState,
    dump: Dump,
}

// SAFETY: an `OutputBuf` is used by one thread at a time (the worker that owns the `Output`; it is created on the main
// thread and moved to the worker before any use). The raw pointers refer to the owner's `Output` members and its
// `FILE*`, which outlive it.
unsafe impl Send for OutputBuf {}

impl OutputBuf {
    /// # Safety
    /// `regs` and `eof` point to members of the C++ `Output` that outlive this object and do not move; `file` is the
    /// `FILE*` of the output (or NULL if nothing is ever flushed).
    pub unsafe fn new(regs: *mut Regs, file: *mut c_void, eof: *mut bool) -> Self {
        let mut b = Self {
            regs,
            file,
            // SAFETY: `eof` is a `std::atomic_bool` of C++, which has the layout of `bool`, per the contract above.
            eof: unsafe { AtomicBool::from_ptr(eof) },
            blocks: Vec::new(),
            index: 0,
            sync: None,
            owned: false,
            slot: 0,
            trunc: TruncState { skip: false, cols: 0, ansi: 0 },
            dump: Dump::new(),
        };
        b.grow();
        b
    }

    fn regs<'a>(&self) -> &'a mut Regs {
        // SAFETY: `new` guarantees `regs` is valid for the life of this object, and no other reference to it is live
        // while a method runs (C++ calls in from the owner thread only); the lifetime is not tied to `self` so that the
        // block fields stay usable meanwhile.
        unsafe { &mut *self.regs }
    }

    fn eof(&self) -> bool {
        // SAFETY: `new` guarantees the flag outlives this object.
        unsafe { &*self.eof }.load(Ordering::SeqCst)
    }

    /// The bytes used in the current block.
    fn used(&self) -> usize {
        let start = self.blocks[self.index].as_ptr() as usize;
        (self.regs().cur as usize).wrapping_sub(start)
    }

    fn set_position(&mut self, index: usize, offset: usize) {
        let regs = self.regs();
        self.index = index;
        let base = self.blocks[index].as_mut_ptr();
        // SAFETY: `offset <= SIZE`, and the block has `SIZE` bytes, so both pointers are in or one past the block.
        unsafe {
            regs.cur = base.add(offset);
            regs.end = base.add(SIZE);
        }
    }

    /// Allocate a new block to grow the list and make it the current one.
    fn grow(&mut self) {
        self.blocks.push(vec![0u8; SIZE].into_boxed_slice());
        self.set_position(self.blocks.len() - 1, 0);
    }

    pub fn sync_on(&mut self, sync: Arc<OutputSync>) {
        if let Some(old) = self.sync.take() {
            old.drop_owner(self.owned);
        }
        self.owned = false;
        self.sync = Some(sync);
    }

    pub fn begin(&mut self, slot: usize) {
        self.slot = slot;
    }

    pub fn acquire(&mut self) {
        if let Some(s) = &self.sync {
            self.owned = s.acquire(self.owned, self.slot);
        }
    }

    /// Without a Sync the output is always ours.
    fn try_acquire(&mut self) -> bool {
        match &self.sync {
            None => true,
            Some(s) => {
                self.owned = s.try_acquire(self.owned);
                self.owned
            }
        }
    }

    pub fn cancel(&mut self) {
        // SAFETY: see `eof`.
        unsafe { &*self.eof }.store(true, Ordering::SeqCst);
        if let Some(s) = &self.sync {
            s.cancel();
        }
    }

    pub fn cancelled(&self) -> bool {
        self.sync.as_ref().is_some_and(|s| s.cancelled())
    }

    pub fn end(&mut self) {
        if let Some(s) = &self.sync {
            self.owned = s.finish(self.owned, self.slot);
        }
    }

    /// Flush the output and release the sync slot, if one was assigned with `sync_on()`.
    pub fn release(&mut self) {
        if self.regs().mode & HOLD == 0 {
            self.flush();
        } else {
            self.discard();
        }
        self.regs().mode = if crate::output_ffi::settings().flag_line_buffered { FLUSH } else { 0 };
        if let Some(s) = &self.sync {
            self.owned = s.release(self.owned);
        }
    }

    /// Discard the buffered output.
    pub fn discard(&mut self) {
        self.set_position(0, 0);
    }

    /// Write to the file; false when it did not take all the bytes.
    fn write(&self, data: &[u8]) -> bool {
        // SAFETY: `data` is a live slice; `file` is the open `FILE*` of the output, per `new`.
        unsafe { fwrite(data.as_ptr().cast(), 1, data.len(), self.file) >= data.len() }
    }

    /// Write a block cut to the `--width` columns; false when the file did not take all the bytes.
    fn write_truncated(&mut self, index: usize, len: usize) -> bool {
        let o = crate::output_ffi::opts(&[], &[], &[]);
        let bytes = truncate_lines(&o, &self.blocks[index][..len], &mut self.trunc);
        bytes.is_empty() || self.write(&bytes)
    }

    /// Swap NUL with LF in the data (`--null-data`).
    fn swap_nul_lf(data: &mut [u8]) {
        for b in data {
            match *b {
                0 => *b = b'\n',
                b'\n' => *b = 0,
                _ => {}
            }
        }
    }

    /// Acquire the lock and flush the blocks, if not held back.
    pub fn flush(&mut self) {
        let used = self.used();
        if self.index == 0 && used == 0 {
            return;
        }
        if !self.eof() {
            // if multi-threaded and the lock is not already owned, wait for our turn
            self.acquire();
            let s = crate::output_ffi::settings();
            if s.flag_null_data {
                for block in &mut self.blocks[..self.index] {
                    Self::swap_nul_lf(block);
                }
                let index = self.index;
                Self::swap_nul_lf(&mut self.blocks[index][..used]);
            }
            for i in 0..self.index {
                let ok = if s.flag_width == 0 {
                    self.write(&self.blocks[i])
                } else {
                    self.write_truncated(i, SIZE)
                };
                if !ok {
                    self.cancel();
                    break;
                }
            }
            if !self.eof() {
                if used > 0 {
                    let ok = if s.flag_width == 0 {
                        self.write(&self.blocks[self.index][..used])
                    } else {
                        self.write_truncated(self.index, used)
                    };
                    if !ok {
                        self.cancel();
                    }
                }
                // SAFETY: `file` is the open `FILE*` of the output.
                if !self.eof() && unsafe { fflush(self.file) } != 0 {
                    self.cancel();
                }
            }
        }
        self.set_position(0, 0);
    }

    /// The current block is full: flush, or move to the next block when we must not wait for the output lock.
    pub fn next(&mut self) {
        if self.regs().mode & HOLD == 0 && self.try_acquire() {
            self.flush();
        } else if self.index + 1 == self.blocks.len() {
            self.grow();
        } else {
            self.set_position(self.index + 1, 0);
        }
    }

    /// Try to flush if we do not have to wait to acquire the output lock.
    pub fn try_flush(&mut self) {
        if self.try_acquire() {
            self.flush();
        }
    }

    /// Flush if output is line buffered, unless we have to wait to acquire the output lock.
    pub fn check_flush(&mut self) {
        if self.regs().mode == FLUSH {
            self.try_flush();
        }
    }

    /// Append bytes to the blocks (`Output::str(s, n)`).
    pub fn str(&mut self, mut s: &[u8]) {
        loop {
            let regs = self.regs();
            let room = (regs.end as usize).wrapping_sub(regs.cur as usize);
            if s.len() < room {
                break;
            }
            // SAFETY: `cur..end` is the unused part of the current block, `room` bytes; `s` has at least that many.
            unsafe { std::ptr::copy_nonoverlapping(s.as_ptr(), regs.cur, room) };
            regs.cur = regs.end;
            s = &s[room..];
            self.next();
        }
        let regs = self.regs();
        // SAFETY: fewer than `room` bytes remain in `s`, so they fit before `end`.
        unsafe {
            std::ptr::copy_nonoverlapping(s.as_ptr(), regs.cur, s.len());
            regs.cur = regs.cur.add(s.len());
        }
    }

    /// Append bytes produced elsewhere, flushing after each newline like `nl()` does (`Output::lines`).
    pub fn lines(&mut self, mut s: &[u8]) {
        while !s.is_empty() {
            let eol = s.iter().position(|&b| b == b'\n');
            let k = eol.map_or(s.len(), |p| p + 1);
            self.str(&s[..k]);
            if eol.is_some() {
                self.check_flush();
            }
            s = &s[k..];
        }
    }

    /// Dump matching data in hex, `mode` is `HEX_MATCH`, `HEX_LINE`, `HEX_CONTEXT_MATCH` or `HEX_CONTEXT_LINE`.
    pub fn dump_hex(&mut self, mode: i16, byte_offset: usize, data: &[u8]) {
        let cols = hex_columns();
        self.dump.offset = byte_offset;
        for &b in data {
            self.dump.bytes[self.dump.offset % cols] = (mode << 8) | i16::from(b);
            self.dump.offset = self.dump.offset.wrapping_add(1);
            if self.dump.offset.is_multiple_of(cols) {
                self.dump_line();
            }
        }
    }

    /// Jump to the next hex dump location (option -o).
    pub fn dump_next(&mut self, byte_offset: usize) {
        let cols = hex_columns();
        if self.dump.offset - self.dump.offset % cols != byte_offset - byte_offset % cols {
            self.dump_done();
        }
    }

    /// The hex line is incomplete: to complete it call `dump_done`.
    pub fn dump_incomplete(&self) -> bool {
        !self.dump.offset.is_multiple_of(hex_columns())
    }

    /// If the hex line is incomplete: complete it with `dump_done`.
    pub fn dump_complete(&mut self, off: usize) {
        if self.dump.offset > 0 && self.dump.offset < off {
            self.dump_done();
        }
    }

    /// Done dumping hex.
    pub fn dump_done(&mut self) {
        if self.dump_incomplete() {
            let cols = hex_columns();
            self.dump_line();
            self.dump.offset = self.dump.offset.wrapping_add(cols - 1);
            self.dump.offset -= self.dump.offset % cols;
        }
        self.dump.prevb = [-1; MAX_HEX_COLUMNS];
        self.dump.bytes = [-1; MAX_HEX_COLUMNS];
    }

    /// Dump one line of hex.
    fn dump_line(&mut self) {
        let o = crate::output_ffi::opts(&[], &[], &[]);
        let r = dump_line(&o, &self.dump.bytes, &self.dump.prevb, self.dump.pstar, self.dump.offset);
        self.lines(&r.bytes);
        self.dump.pstar = r.pstar;
        if !r.repeated {
            self.dump.prevb = self.dump.bytes;
        }
        self.dump.bytes = [-1; MAX_HEX_COLUMNS];
    }
}

impl Drop for OutputBuf {
    /// `~Output`: flush, then the lock goes away (a lock that owns the output releases it).
    fn drop(&mut self) {
        self.flush();
        if let Some(s) = self.sync.take() {
            s.drop_owner(self.owned);
        }
    }
}

/// `Output::Tree`: the global directory tree state of `--tree` and its drawing strings, protected by `acquire()`.
pub struct Tree {
    pub state: TreeState,
    bar: Vec<u8>,
    ptr: Vec<u8>,
    end: Vec<u8>,
}

impl Tree {
    pub fn glyphs(&self) -> TreeGlyphs<'_> {
        TreeGlyphs { bar: &self.bar, ptr: &self.ptr, end: &self.end[..self.end.len() - 1] }
    }

    /// The `end` string with its NUL terminator, for C++.
    pub fn end_c_str(&self) -> *const u8 {
        self.end.as_ptr()
    }

    pub fn set_glyphs(&mut self, bar: &[u8], ptr: &[u8], end: &[u8]) {
        self.bar = bar.to_vec();
        self.ptr = ptr.to_vec();
        self.end = [end, &[0]].concat();
    }
}

/// The process-wide tree state; the ASCII drawing strings until `set_glyphs`.
pub fn tree() -> std::sync::MutexGuard<'static, Tree> {
    static TREE: OnceLock<Mutex<Tree>> = OnceLock::new();
    TREE.get_or_init(|| {
        Mutex::new(Tree {
            state: TreeState { path: Vec::new(), depth: 0 },
            bar: b"|  ".to_vec(),
            ptr: b"|_ ".to_vec(),
            end: b"~  \0".to_vec(),
        })
    })
    .lock()
    .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" {
        fn tmpfile() -> *mut c_void;
        fn rewind(file: *mut c_void);
        fn fread(buf: *mut c_void, size: usize, count: usize, file: *mut c_void) -> usize;
        fn fclose(file: *mut c_void) -> i32;
    }

    struct Fixture {
        _regs: Box<Regs>,
        _eof: Box<bool>,
        file: *mut c_void,
        out: Option<OutputBuf>,
    }

    impl Fixture {
        fn new() -> Self {
            // SAFETY: `tmpfile` has no preconditions.
            let file = unsafe { tmpfile() };
            assert!(!file.is_null());
            let mut regs = Box::new(Regs { cur: std::ptr::null_mut(), end: std::ptr::null_mut(), mode: 0 });
            let mut eof = Box::new(false);
            // SAFETY: the boxes outlive `out` (dropped after it in `finish`) and do not move; `file` is open.
            let out = unsafe { OutputBuf::new(&mut *regs, file, &mut *eof) };
            Self { _regs: regs, _eof: eof, file, out: Some(out) }
        }

        fn out(&mut self) -> &mut OutputBuf {
            self.out.as_mut().unwrap()
        }

        /// Drop the output (flushes), then read what reached the file.
        fn finish(mut self) -> Vec<u8> {
            drop(self.out.take());
            let mut data = vec![0u8; 1 << 20];
            // SAFETY: `file` is open, `data` has room for the count.
            let n = unsafe {
                rewind(self.file);
                let n = fread(data.as_mut_ptr().cast(), 1, data.len(), self.file);
                fclose(self.file);
                n
            };
            data.truncate(n);
            data
        }
    }

    #[test]
    fn appends_across_blocks_and_flushes_in_order() {
        let mut f = Fixture::new();
        let data: Vec<u8> = (0..SIZE * 2 + 100).map(|i| u8::try_from(i % 251).unwrap()).collect();
        f.out().str(&data[..10]);
        f.out().str(&data[10..]);
        assert_eq!(f.finish(), data);
    }

    #[test]
    fn hold_buffers_all_output_until_launched() {
        let mut f = Fixture::new();
        f.out().regs().mode = HOLD;
        let data = vec![b'x'; SIZE * 3 + 5];
        f.out().str(&data);
        assert_eq!(f.out().blocks.len(), 4);
        f.out().regs().mode = 0;
        f.out().flush();
        assert_eq!(f.out().index, 0);
        assert_eq!(f.finish(), data);
    }

    #[test]
    fn discard_drops_held_output() {
        let mut f = Fixture::new();
        f.out().regs().mode = HOLD;
        f.out().str(b"held");
        f.out().release();
        assert_eq!(f.finish(), b"");
    }

    #[test]
    fn line_buffered_flushes_each_line() {
        let mut f = Fixture::new();
        f.out().regs().mode = FLUSH;
        f.out().lines(b"one\ntwo");
        assert_eq!(f.out().used(), 3);
        f.out().lines(b"\n");
        assert_eq!(f.out().used(), 0);
        assert_eq!(f.finish(), b"one\ntwo\n");
    }

    #[test]
    fn eof_discards_the_buffer() {
        let mut f = Fixture::new();
        f.out().cancel();
        f.out().str(b"lost");
        f.out().flush();
        assert_eq!(f.out().used(), 0);
        assert_eq!(f.finish(), b"");
    }

    #[test]
    fn dump_hex_line_is_appended_when_complete() {
        let mut f = Fixture::new();
        f.out().dump_hex(1, 0, &[b'a'; 16]);
        assert!(!f.out().dump_incomplete());
        let out = f.finish();
        assert!(out.windows(2).any(|w| w == b"61"), "{out:?}");
    }

    #[test]
    fn tree_defaults_and_reset() {
        let mut t = tree();
        t.state.depth = 3;
        t.state.path = b"a/".to_vec();
        t.state.depth = 0;
        t.state.path.clear();
        assert_eq!(t.glyphs().end, b"~  ");
    }
}
