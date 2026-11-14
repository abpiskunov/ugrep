//! Unit `zstream_core` of the component `src_leaves`: the format selection of `zstreambuf::open`, the filename
//! predicates, `close`, `decompressing`, `zipinfo`, `zippeek` and the `next` decoding loops of src/zstream.hpp. The
//! `std::streambuf` part (buffer, `cur_`/`len_`, underflow/uflow/xsgetn) stays C++ in the adapter. C++ line ranges:
//! rust/src_leaves/SEAM.md. The decoder state and the zip/7zip parsing are in `zstream_codecs`.
use crate::signatures::zstream_codecs::{
    file_eof, file_read, open_compress, u16le, u32le, Bzip2, Config, Decoder, Diagnostics, FilePtr, Gzip, Lz4, Xz,
    ZipInfo, ZipState, Zstd, CODEC_7ZIP, CODEC_BZ2, CODEC_LZ4, CODEC_LZMA, CODEC_ZSTD, COMPRESS_HEADER_MAGIC,
    DEFLATE_HEADER_MAGIC, XZ_HEADER_MAGIC_1, XZ_HEADER_MAGIC_2, ZIP_DESCRIPTOR_MAGIC, ZIP_EMPTY_MAGIC,
    ZIP_HEADER_MAGIC, ZSTD_HEADER_MAGIC, Z_BUF_LEN,
};

/// `zstreambuf::has_ext`: the extension of `pathname` is one of the dot separated extensions in `exts`.
fn has_ext(pathname: &[u8], exts: &[u8]) -> bool {
    let end = pathname.iter().position(|&c| c == 0).unwrap_or(pathname.len());
    let pathname = &pathname[..end];
    let Some(dot) = pathname.iter().rposition(|&c| c == b'.') else {
        return false;
    };
    let dot = &pathname[dot..];
    if dot.len() > exts.len() {
        return false;
    }
    match exts.windows(dot.len()).position(|w| w == dot) {
        Some(at) => matches!(exts.get(at + dot.len()), None | Some(&b'.')),
        None => false,
    }
}

/// `.bz .bz2 .bzip2 .tb2 .tbz .tbz2 .tz2`
pub fn is_bz(pathname: &[u8]) -> bool {
    has_ext(pathname, b".bz.bz2.bzip2.tb2.tbz.tbz2.tz2")
}

/// `.lzma .xz .tlz .txz`
pub fn is_xz(pathname: &[u8]) -> bool {
    has_ext(pathname, b".lzma.xz.tlz.txz")
}

/// `.lz4`
pub fn is_lz4(pathname: &[u8]) -> bool {
    has_ext(pathname, b".lz4")
}

/// `.zst .zstd .tzst`
pub fn is_zstd(pathname: &[u8]) -> bool {
    has_ext(pathname, b".zst.zstd.tzst")
}

/// `.br`
pub fn is_br(pathname: &[u8]) -> bool {
    has_ext(pathname, b".br")
}

/// `.bz3`
pub fn is_bz3(pathname: &[u8]) -> bool {
    has_ext(pathname, b".bz3")
}

/// `.Z .taZ .tZ`
pub fn is_z(pathname: &[u8]) -> bool {
    has_ext(pathname, b".Z.taZ.tZ")
}

/// `.zip .zipx .ZIP`
pub fn is_zip(pathname: &[u8]) -> bool {
    has_ext(pathname, b".zip.zipx.ZIP")
}

/// `.7z .7Z`
pub fn is_7z(pathname: &[u8]) -> bool {
    has_ext(pathname, b".7z.7Z")
}

/// `.rar .RAR`
pub fn is_rar(pathname: &[u8]) -> bool {
    has_ext(pathname, b".rar.RAR")
}

/// The Rust-owned stream behind `zstreambuf` (`rust_`): one per `open`, a leaked `Box` at the C boundary.
#[derive(Debug)]
pub struct ZStream {
    pub pathname: Vec<u8>,
    pub file: FilePtr,
    pub config: Config,
    /// `None` for a plain file, which is passed through.
    pub decoder: Option<Decoder>,
    /// True once the input is exhausted or failed: the legacy code sets `file_` to NULL.
    pub at_end: bool,
    pub diag: Diagnostics,
    /// Message handed out by the last `diag` pop, NUL-terminated, valid until the next pop.
    pub last_message: Vec<u8>,
    /// Bytes of a plain file read while sniffing the format: the adapter puts them in its buffer.
    pub initial: Vec<u8>,
}

/// Reads into `buf[*num..limit]` like the legacy `file_read(file_, buf_ + num, limit - num)`.
fn read_more(file: FilePtr, buf: &mut [u8], num: &mut usize, limit: usize) {
    *num += file_read(file, &mut buf[*num..limit]);
}

impl ZStream {
    /// `zstreambuf::open(pathname, file)` for a non-NULL `file` (the adapter handles NULL): pick the format from the
    /// name and the magic bytes, set up the decoder, queue the diagnostics the legacy code printed.
    pub fn open(pathname: &[u8], file: FilePtr, config: Config) -> ZStream {
        let end = pathname.iter().position(|&c| c == 0).unwrap_or(pathname.len());
        let name = pathname[..end].to_vec();
        let mut s = ZStream {
            pathname: name.clone(),
            file,
            config,
            decoder: None,
            at_end: false,
            diag: Diagnostics::default(),
            last_message: Vec::new(),
            initial: Vec::new(),
        };
        if is_bz(&name) {
            if config.has(CODEC_BZ2) {
                match Bzip2::new(&[]) {
                    Some(z) => s.decoder = Some(Decoder::Bzip2(z)),
                    None => s.fail_warning("BZ2_bzDecompressInit failed"),
                }
            } else {
                s.unsupported();
            }
        } else if is_xz(&name) {
            if config.has(CODEC_LZMA) {
                match Xz::new(&[]) {
                    Some(z) => s.decoder = Some(Decoder::Xz(z)),
                    None => s.fail_warning("lzma_stream_decoder failed"),
                }
            } else {
                s.unsupported();
            }
        } else if is_lz4(&name) {
            if config.has(CODEC_LZ4) {
                match Lz4::new() {
                    Some(z) => s.decoder = Some(Decoder::Lz4(Box::new(z))),
                    None => s.fail_warning("LZ4_createStreamDecode failed"),
                }
            } else {
                s.unsupported();
            }
        } else if is_zstd(&name) {
            if config.has(CODEC_ZSTD) {
                match Zstd::new(&[]) {
                    Some(z) => s.decoder = Some(Decoder::Zstd(z)),
                    None => s.fail_warning("ZSTD_createDStream failed"),
                }
            } else {
                s.unsupported();
            }
        } else if is_br(&name) || is_bz3(&name) || is_rar(&name) {
            // brotli and bzip3 are not linked into the Rust build
            s.unsupported();
        } else if is_7z(&name) {
            if config.has(CODEC_7ZIP) {
                let mut zip = ZipState::new(&name, file, config, &[]);
                if !zip.attach_7zip() {
                    s.diag.cannot_decompress("7zip archive: encrypted data or 7z input is not seekable");
                    s.at_end = true;
                }
                s.decoder = Some(Decoder::Zip(Box::new(zip)));
            } else {
                s.unsupported();
            }
        } else {
            s.sniff();
        }
        s
    }

    fn unsupported(&mut self) {
        self.diag.cannot_decompress("unsupported compression format");
        self.at_end = true;
    }

    fn fail_warning(&mut self, message: &str) {
        self.diag.warning(message);
        self.at_end = true;
    }

    /// The magic byte detection of `zstreambuf::open`.
    fn sniff(&mut self) {
        let file = self.file;
        let config = self.config;
        let mut buf = vec![0u8; Z_BUF_LEN];
        let mut num = 0usize;
        read_more(file, &mut buf, &mut num, 2);

        if num == 2 && u16le(&buf) == DEFLATE_HEADER_MAGIC {
            match Gzip::new(&buf[..2]) {
                Ok(z) => self.decoder = Some(Decoder::Gzip(z)),
                Err(message) => {
                    self.diag.cannot_decompress(&message);
                    self.at_end = true;
                }
            }
            return;
        }
        if num == 2 && u16le(&buf) == COMPRESS_HEADER_MAGIC {
            match open_compress(file) {
                Some(d) => self.decoder = Some(d),
                None => self.fail_warning("zopen failed"),
            }
            return;
        }

        read_more(file, &mut buf, &mut num, 4);
        if num == 4 {
            let magic = u32le(&buf);
            if magic == ZIP_HEADER_MAGIC {
                let mut zip = ZipState::new(&self.pathname, file, config, &buf[..4]);
                if zip.header(&mut self.diag) {
                    self.decoder = Some(Decoder::Zip(Box::new(zip)));
                } else {
                    self.at_end = true;
                }
                return;
            }
            if magic == ZIP_EMPTY_MAGIC {
                self.at_end = true;
                return;
            }
            if magic == ZIP_DESCRIPTOR_MAGIC {
                self.diag.cannot_decompress("spanned zip fragment of a split zip archive");
                self.at_end = true;
                return;
            }
        }

        if config.has(CODEC_ZSTD) && num == 4 && u32le(&buf) == ZSTD_HEADER_MAGIC {
            match Zstd::new(&buf[..4]) {
                Some(z) => self.decoder = Some(Decoder::Zstd(z)),
                None => self.fail_warning("ZSTD_createDStream failed"),
            }
            return;
        }

        if config.has(CODEC_LZMA) {
            read_more(file, &mut buf, &mut num, 6);
            if num == 6 && u32le(&buf) == XZ_HEADER_MAGIC_1 && u16le(&buf[4..]) == XZ_HEADER_MAGIC_2 {
                match Xz::new(&buf[..6]) {
                    Some(z) => self.decoder = Some(Decoder::Xz(z)),
                    None => self.fail_warning("lzma_stream_decoder failed"),
                }
                return;
            }
        }

        if config.has(CODEC_BZ2) {
            read_more(file, &mut buf, &mut num, 10);
            if num == 10
                && &buf[..3] == b"BZh"
                && buf[3].is_ascii_digit()
                && buf[4..10] == [0x31, 0x41, 0x59, 0x26, 0x53, 0x59]
            {
                match Bzip2::new(&buf[..10]) {
                    Some(z) => self.decoder = Some(Decoder::Bzip2(z)),
                    None => self.fail_warning("BZ2_bzDecompressInit failed"),
                }
                return;
            }
        }

        // plain file: read the rest of the first block, the adapter hands it out first
        read_more(file, &mut buf, &mut num, Z_BUF_LEN);
        buf.truncate(num);
        self.initial = buf;
    }

    /// `zstreambuf::decompressing`.
    pub fn decompressing(&self) -> bool {
        self.decoder.is_some()
    }

    /// `zstreambuf::next`: decompress or copy the next block into `buf`; 0 at EOF, negative on error.
    pub fn next(&mut self, buf: &mut [u8]) -> isize {
        if self.at_end {
            return 0;
        }
        match self.decoder.as_mut() {
            Some(decoder) => {
                let step = decoder.next(self.file, &mut self.diag, buf);
                if step.done {
                    if let Some(d) = self.decoder.take() {
                        d.close();
                    }
                    self.at_end = true;
                }
                step.num
            }
            None => {
                let num = file_read(self.file, buf);
                if num == 0 || num < buf.len() && file_eof(self.file) {
                    self.at_end = true;
                }
                num as isize
            }
        }
    }

    /// `zstreambuf::zipinfo`: advance to the next zip/7zip entry; `None` when there is none or not unzipping.
    pub fn zipinfo(&mut self) -> Option<&ZipInfo> {
        let ok = match self.decoder.as_mut() {
            Some(Decoder::Zip(zip)) => zip.header(&mut self.diag),
            _ => return None,
        };
        if !ok {
            self.decoder = None;
            return None;
        }
        match self.decoder.as_ref() {
            Some(Decoder::Zip(zip)) => Some(&zip.info),
            _ => None,
        }
    }

    /// `zstreambuf::zippeek`: the pending compressed data when unzipping.
    pub fn zippeek(&mut self) -> Option<&[u8]> {
        match self.decoder.as_mut() {
            Some(Decoder::Zip(zip)) => Some(zip.peek()),
            _ => None,
        }
    }
}
