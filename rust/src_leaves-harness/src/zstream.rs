//! Replay of the zstream seam (`src_leaves.zs_*`, units `zstream_core` and `zstream_codecs`) against the Rust
//! `signatures::zstream_core` and `signatures::zstream_codecs`, recording what `src/zstream.cpp` taps for
//! `migration/harness/zstream/zstream_driver.cpp`.
//!
//! The driver opens every case's stream on a FILE holding the case's `input_hex`. The trace names that FILE only as an
//! object id (`#1`), so the bytes come from the zstream seam cases, `CPP2RUST_ZSTREAM_CASES` or else
//! `<directory of the recorded trace>/../zstream/seam/cases.jsonl` (migration/corpus/src_leaves/zstream/seam/cases.jsonl
//! for the component trace). The legacy `open` calls `close` and the `is_*` predicates through the facade, so their
//! calls are recorded inside `zs_open`; `replay::Phase` replays them there against the Rust predicates, then the Rust
//! `ZStream::open` runs. The diagnostics the Rust queues are the outgoing calls `ugrep.cannot_decompress` and
//! `ugrep.warning` (out_call only, as the C++ tap records them).
use crate::replay::Phase;
use crate::signatures::zstream_codecs::{
    Config, DiagKind, CODEC_7ZIP, CODEC_BZ2, CODEC_LZ4, CODEC_LZMA, CODEC_ZLIB, CODEC_ZSTD,
};
use crate::signatures::zstream_core::{self as zcore, ZStream};
use crate::tap::Tap;
use crate::{bytes, ffi};
use core::ffi::c_void;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const PREFIX: &str = "src_leaves.zs_";

pub const FUNCTIONS: [&str; 16] = [
    "src_leaves.zs_is_bz",
    "src_leaves.zs_is_xz",
    "src_leaves.zs_is_lz4",
    "src_leaves.zs_is_zstd",
    "src_leaves.zs_is_br",
    "src_leaves.zs_is_bz3",
    "src_leaves.zs_is_Z",
    "src_leaves.zs_is_zip",
    "src_leaves.zs_is_7z",
    "src_leaves.zs_is_rar",
    "src_leaves.zs_open",
    "src_leaves.zs_close",
    "src_leaves.zs_decompressing",
    "src_leaves.zs_next",
    "src_leaves.zs_zipinfo",
    "src_leaves.zs_zippeek",
];

const SCRATCH: &str = "src_leaves_zstream_case.tmp";

fn scratch_path() -> String {
    std::env::temp_dir().join(SCRATCH).to_string_lossy().into_owned()
}

/// The build the trace was recorded with (migration/harness/zstream/build-zstream-driver.ps1): HAVE_LIBZ, HAVE_LIBBZ2,
/// HAVE_LIBLZMA, HAVE_LIBLZ4, HAVE_LIBZSTD, 7zip on, WITH_ZIP_CRC32 off, WITH_MAX_7ZIP_SIZE 1 GiB.
fn driver_config() -> Config {
    Config {
        codecs: CODEC_ZLIB | CODEC_BZ2 | CODEC_LZMA | CODEC_LZ4 | CODEC_ZSTD | CODEC_7ZIP,
        zip_crc32: false,
        max_7zip_size: 1 << 30,
    }
}

fn predicate(func: &str) -> Option<fn(&[u8]) -> bool> {
    Some(match func {
        "src_leaves.zs_is_bz" => zcore::is_bz,
        "src_leaves.zs_is_xz" => zcore::is_xz,
        "src_leaves.zs_is_lz4" => zcore::is_lz4,
        "src_leaves.zs_is_zstd" => zcore::is_zstd,
        "src_leaves.zs_is_br" => zcore::is_br,
        "src_leaves.zs_is_bz3" => zcore::is_bz3,
        "src_leaves.zs_is_Z" => zcore::is_z,
        "src_leaves.zs_is_zip" => zcore::is_zip,
        "src_leaves.zs_is_7z" => zcore::is_7z,
        "src_leaves.zs_is_rar" => zcore::is_rar,
        _ => return None,
    })
}

fn from_hex(h: &str) -> Vec<u8> {
    (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect()
}

fn load_inputs(path: &Path) -> HashMap<String, Vec<u8>> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("zstream seam cases {}: {e} (set CPP2RUST_ZSTREAM_CASES)", path.display()));
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let c: Value = serde_json::from_str(l).unwrap();
            (c["id"].as_str().unwrap().to_string(), from_hex(c["input_hex"].as_str().unwrap_or("")))
        })
        .collect()
}

/// One zstream case: the FILE of the driver and the Rust stream opened on it.
pub struct ZsReplay {
    cases_path: PathBuf,
    inputs: Option<HashMap<String, Vec<u8>>>,
    case: String,
    file: *mut c_void,
    stream: Option<Box<ZStream>>,
}

impl ZsReplay {
    pub fn new(recorded: &Path) -> ZsReplay {
        let cases_path = std::env::var_os("CPP2RUST_ZSTREAM_CASES").map(PathBuf::from).unwrap_or_else(|| {
            recorded.parent().unwrap_or(Path::new(".")).join("..").join("zstream").join("seam").join("cases.jsonl")
        });
        ZsReplay { cases_path, inputs: None, case: String::new(), file: std::ptr::null_mut(), stream: None }
    }

    fn input(&mut self, case: &str) -> Vec<u8> {
        let path = &self.cases_path;
        let inputs = self.inputs.get_or_insert_with(|| load_inputs(path));
        match inputs.get(case) {
            Some(data) => data.clone(),
            None => panic!("no zstream seam input for case {case} in {}", self.cases_path.display()),
        }
    }

    /// Ends the case as the driver does: the stream is destroyed, then the FILE is closed.
    pub fn finish(&mut self) {
        self.stream = None;
        if !self.file.is_null() {
            ffi::close(self.file);
            self.file = std::ptr::null_mut();
        }
        let _ = std::fs::remove_file(scratch_path());
    }

    /// The diagnostics the Rust queued, reported as the adapter reports them (`zs_diagnostics` in src/zstream.cpp).
    fn diagnostics(&mut self, tap: &mut Tap) {
        let Some(s) = self.stream.as_mut() else { return };
        while let Some((kind, message)) = s.diag.queue.pop_front() {
            let callee = match kind {
                DiagKind::Warning => "ugrep.warning",
                DiagKind::CannotDecompress => "ugrep.cannot_decompress",
            };
            tap.out_call(callee, &[&message.as_slice()]);
        }
    }

    pub fn dispatch(&mut self, phase: Phase, func: &str, args: &Value, tap: &mut Tap) {
        if self.case != tap.case() {
            self.finish();
            self.case = tap.case().to_string();
        }
        if let Some(pred) = predicate(func) {
            if phase == Phase::Call {
                let pathname = bytes(&args[0]);
                tap.call(func, &[&pathname.as_slice()]);
                let r = pred(&pathname);
                tap.ret(func, &r);
            }
            return;
        }
        match (func, phase) {
            ("src_leaves.zs_open", Phase::Call) => {
                let (pathname, file_id) = (bytes(&args[0]), args[1].as_str().unwrap().to_string());
                tap.call(func, &[&pathname.as_slice(), &file_id]);
            }
            ("src_leaves.zs_open", Phase::Ret) => {
                tap.resume(func);
                // open closes the old stream first, like the adapter, also when zs_close is not replayed
                self.stream = None;
                if args[1].as_str() != Some("null") {
                    if self.file.is_null() {
                        let case = self.case.clone();
                        let data = self.input(&case);
                        self.file = ffi::open_scratch(&scratch_path(), &data, false);
                    } else {
                        ffi::rewind(self.file);
                    }
                    self.stream = Some(Box::new(ZStream::open(&bytes(&args[0]), self.file, driver_config())));
                    self.diagnostics(tap);
                }
                tap.ret(func, &0i32);
            }
            ("src_leaves.zs_close", Phase::Call) => {
                tap.call(func, &[]);
                self.stream = None;
                tap.ret(func, &0i32);
            }
            ("src_leaves.zs_decompressing", Phase::Call) => {
                tap.call(func, &[]);
                let r = self.stream.as_ref().is_some_and(|s| s.decompressing());
                tap.ret(func, &r);
            }
            ("src_leaves.zs_next", Phase::Call) => {
                let len = args[0].as_u64().unwrap();
                tap.call(func, &[&len]);
                let mut buf = vec![0u8; len as usize];
                let n = self.stream.as_mut().map_or(0, |s| s.next(&mut buf));
                self.diagnostics(tap);
                let got: &[u8] = if n > 0 { &buf[..(n as usize).min(buf.len())] } else { &[] };
                tap.state(func, &got);
                tap.ret(func, &(n as i64));
            }
            ("src_leaves.zs_zipinfo", Phase::Call) => {
                tap.call(func, &[]);
                let name = self.stream.as_mut().and_then(|s| s.zipinfo()).map(|info| info.name.clone());
                self.diagnostics(tap);
                if let Some(name) = &name {
                    tap.state(func, &name.as_slice());
                }
                tap.ret(func, &name.is_some());
            }
            ("src_leaves.zs_zippeek", Phase::Call) => {
                tap.call(func, &[]);
                let data = self.stream.as_mut().and_then(|s| s.zippeek()).map(<[u8]>::to_vec).unwrap_or_default();
                self.diagnostics(tap);
                tap.state(func, &data.as_slice());
                tap.ret(func, &(data.len() as u64));
            }
            (_, Phase::Ret) => {}
            (other, Phase::Call) => panic!("unknown seam function {other}"),
        }
    }
}
