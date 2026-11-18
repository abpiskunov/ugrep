//! The output buffer of `Output` (src/output.hpp), owned by Rust: the chain of fixed-size blocks the C++ call sites
//! append to, the flush of the blocks to the stream (`--null-data`, `--width`), the state of the `--width` cut, the
//! global `--tree` state and `Dump::hex`. C++ writes into the current block through a cursor (`cur`, `end`) that this
//! module hands out; the module is entered only when a block is full, to flush, or to reset (see SEAM.md, "Seam:
//! output buffer").

use crate::output::{dump_line, truncate_lines, Opts, TreeGlyphs, TreeState, TruncState};

/// `Output::SIZE`: the size of each block.
pub const SIZE: usize = 32768;

/// Where the flushed bytes go (a C `FILE*` in the product).
pub trait Sink {
    /// Writes all of `data`; false if the stream could not take it all.
    fn write(&mut self, data: &[u8]) -> bool;
    /// Flushes the stream; false on error.
    fn flush(&mut self) -> bool;
}

/// The chain of blocks. A block is a heap allocation of `SIZE` bytes that never moves while the buffer lives (the
/// `Vec` holds boxes), so the cursors C++ holds into the current block stay valid when blocks are added.
pub struct OutBuf {
    blocks: Vec<Box<[u8]>>,
    index: usize,
    trunc: TruncState,
}

fn new_block() -> Box<[u8]> {
    vec![0u8; SIZE].into_boxed_slice()
}

impl Default for OutBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl OutBuf {
    pub fn new() -> Self {
        OutBuf { blocks: vec![new_block()], index: 0, trunc: TruncState { skip: false, cols: 0, ansi: 0 } }
    }

    /// The start of the current block.
    pub fn current(&mut self) -> *mut u8 {
        self.blocks[self.index].as_mut_ptr()
    }

    /// `Output::next` when the output cannot be flushed: moves on to the next block, allocating one if needed.
    pub fn advance(&mut self) -> *mut u8 {
        self.index += 1;
        if self.index == self.blocks.len() {
            self.blocks.push(new_block());
        }
        self.current()
    }

    /// Discards the buffered bytes: back to the first block.
    pub fn reset(&mut self) -> *mut u8 {
        self.index = 0;
        self.current()
    }

    /// True if there is anything to flush, given the number of bytes used in the current block.
    pub fn pending(&self, used: usize) -> bool {
        self.index != 0 || used > 0
    }

    /// `Output::flush` after the lock is acquired: writes the full blocks and the `used` bytes of the current block to
    /// the sink, swapping NUL and LF with `--null-data` and cutting the lines to `--width` columns. Returns true if
    /// the stream failed, in which case the caller cancels the output. The buffer is left as it was: call `reset`.
    pub fn write(&mut self, o: &Opts, used: usize, null_data: bool, sink: &mut dyn Sink) -> bool {
        let last = self.index;
        if null_data {
            for i in 0..=last {
                let len = if i == last { used } else { SIZE };
                for b in &mut self.blocks[i][..len] {
                    if *b == 0 {
                        *b = b'\n';
                    } else if *b == b'\n' {
                        *b = 0;
                    }
                }
            }
        }
        for i in 0..=last {
            let len = if i == last { used } else { SIZE };
            if len == 0 {
                continue;
            }
            let data = &self.blocks[i][..len];
            if o.width == 0 {
                if !sink.write(data) {
                    return true;
                }
            } else {
                let bytes = truncate_lines(o, data, &mut self.trunc);
                if !bytes.is_empty() && !sink.write(&bytes) {
                    return true;
                }
            }
        }
        !sink.flush()
    }

    /// Test access to a block.
    #[cfg(test)]
    fn block(&mut self, i: usize) -> &mut [u8] {
        &mut self.blocks[i]
    }
}

/// `Output::Dump` state, laid out as the C++ struct fields the C ABI shares (`bytes`, `prevb`, `pstar`, `offset`).
#[repr(C)]
pub struct DumpState {
    pub offset: usize,
    pub bytes: [i16; 64],
    pub prevb: [i16; 64],
    pub pstar: bool,
}

/// `Output::Dump::line`: appends one hex line to `out` and clears the row.
pub fn dump_flush_line(o: &Opts, st: &mut DumpState, out: &mut Vec<u8>) {
    let r = dump_line(o, &st.bytes, &st.prevb, st.pstar, st.offset);
    out.extend_from_slice(&r.bytes);
    st.pstar = r.pstar;
    if !r.repeated {
        st.prevb = st.bytes;
    }
    st.bytes = [-1; 64];
}

/// `Output::Dump::hex`: dumps `data` in hex with the colour `mode` from `byte_offset`, returns the completed lines.
pub fn dump_hex(o: &Opts, st: &mut DumpState, mode: i16, byte_offset: usize, data: &[u8]) -> Vec<u8> {
    let cols = o.hex_columns;
    let mut out = Vec::new();
    st.offset = byte_offset;
    for &b in data {
        st.bytes[st.offset % cols] = (mode << 8) | i16::from(b);
        st.offset = st.offset.wrapping_add(1);
        if st.offset.is_multiple_of(cols) {
            dump_flush_line(o, st, &mut out);
        }
    }
    out
}

/// The global `--tree` state of `Output::Tree`: the directory path and depth, and the drawing strings.
pub struct Tree {
    pub state: TreeState,
    pub bar: Vec<u8>,
    pub ptr: Vec<u8>,
    pub end: Vec<u8>,
    pub end_c: Vec<u8>,
}

impl Tree {
    pub fn new() -> Self {
        Tree { state: TreeState { path: Vec::new(), depth: 0 }, bar: b"|  ".to_vec(), ptr: b"|_ ".to_vec(), end: b"~  ".to_vec(), end_c: b"~  \0".to_vec() }
    }

    pub fn glyphs(&self) -> TreeGlyphs<'_> {
        TreeGlyphs { bar: &self.bar, ptr: &self.ptr, end: &self.end }
    }
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mem {
        data: Vec<u8>,
        limit: usize,
    }

    impl Sink for Mem {
        fn write(&mut self, d: &[u8]) -> bool {
            if self.data.len() + d.len() > self.limit {
                return false;
            }
            self.data.extend_from_slice(d);
            true
        }
        fn flush(&mut self) -> bool {
            true
        }
    }

    #[test]
    fn writes_full_blocks_then_the_partial_one() {
        let o = Opts { hex_columns: 16, ..Opts::default() };
        let mut b = OutBuf::new();
        b.block(0).fill(b'a');
        b.advance();
        b.block(1)[..3].fill(b'b');
        assert!(b.pending(3));
        let mut m = Mem { data: Vec::new(), limit: usize::MAX };
        assert!(!b.write(&o, 3, false, &mut m));
        assert_eq!(m.data.len(), SIZE + 3);
        assert_eq!(&m.data[SIZE..], b"bbb");
        b.reset();
        assert!(!b.pending(0));
    }

    #[test]
    fn null_data_swaps_nul_and_lf() {
        let o = Opts { hex_columns: 16, ..Opts::default() };
        let mut b = OutBuf::new();
        b.block(0)[..4].copy_from_slice(b"a\0b\n");
        let mut m = Mem { data: Vec::new(), limit: usize::MAX };
        assert!(!b.write(&o, 4, true, &mut m));
        assert_eq!(m.data, b"a\nb\0");
        assert_eq!(&b.block(0)[..4], b"a\nb\0");
    }

    #[test]
    fn a_failing_stream_is_reported() {
        let o = Opts { hex_columns: 16, ..Opts::default() };
        let mut b = OutBuf::new();
        b.block(0)[..10].fill(b'x');
        let mut m = Mem { data: Vec::new(), limit: 5 };
        assert!(b.write(&o, 10, false, &mut m));
    }

    #[test]
    fn hex_dump_matches_the_line_by_line_form() {
        let o = Opts { hex_columns: 16, ..Opts::default() };
        let mut st = DumpState { offset: 0, bytes: [-1; 64], prevb: [-1; 64], pstar: false };
        let data: Vec<u8> = (0..40).collect();
        let out = dump_hex(&o, &mut st, 0, 0, &data);
        let mut st2 = DumpState { offset: 0, bytes: [-1; 64], prevb: [-1; 64], pstar: false };
        let mut expect = Vec::new();
        st2.offset = 0;
        for &b in &data {
            st2.bytes[st2.offset % o.hex_columns] = i16::from(b);
            st2.offset += 1;
            if st2.offset.is_multiple_of(o.hex_columns) {
                dump_flush_line(&o, &mut st2, &mut expect);
            }
        }
        assert_eq!(out, expect);
        assert_eq!(st.offset, 40);
    }
}
