//! Replay harness for `src_leaves`: replays recorded C++ seam traces against the Rust functions.
//! Usage: src_leaves-harness <recorded.jsonl> <out.jsonl> [function ids...]
//! Defaults to every seam function. Not linked into any product binary.
//!
//! `src_leaves.z_*` (unit zopen) work on a `FILE*`: the recorded `src_leaves.z_file` call carries the bytes of
//! the file the C++ driver (`migration/harness/src_leaves/zopen_driver.cpp`) created, the harness creates the
//! same file, runs the Rust on it and records what the driver records: errno after every call, and at the end
//! the file position (and the file's bytes if the case wrote).
//!
//! `src_leaves.zs_*` (units zstream_core and zstream_codecs) are replayed by `zstream.rs`. The codec libraries the Rust
//! stream calls through FFI are linked by build.rs.
mod ffi;
mod replay;
mod tap;
mod zstream;

#[path = "../../src_leaves/src/signatures.rs"]
mod signatures;

use core::ffi::c_void;
use replay::Phase;
use serde_json::Value;
use signatures::zopen::{self, ZState};
use tap::{quote, Tap, ToNeutral};

/// Trace strings carry one char per byte (U+0000..U+00FF).
fn bytes(v: &Value) -> Vec<u8> {
    v.as_str().unwrap().chars().map(|c| c as u32 as u8).collect()
}

/// Neutral JSON text already encoded.
struct Raw(String);
impl ToNeutral for Raw {
    fn to_neutral(&self) -> String {
        self.0.clone()
    }
}

const SCRATCH: &str = "src_leaves_zopen_case.tmp";

/// The scratch FILE lives in the temp directory, not in the repository.
fn scratch_path() -> String {
    std::env::temp_dir().join(SCRATCH).to_string_lossy().into_owned()
}
const FILE_SETUP: &str = "src_leaves.z_file";

/// One zopen case: the FILE, the object ids in first-seen order ("#n", as the C++ tap numbers them) and the handle.
struct ZCase {
    file: *mut c_void,
    ids: Vec<String>,
    handle: Option<(String, Box<ZState>)>,
    wrote: bool,
}

impl ZCase {
    fn new() -> ZCase {
        ZCase { file: std::ptr::null_mut(), ids: Vec::new(), handle: None, wrote: false }
    }
    fn finish(&mut self, tap: &mut Tap, func: &str) {
        let end = ffi::end_state(self.file, self.wrote, quote);
        tap.state_after(func, &Raw(end));
        ffi::close(self.file);
        self.file = std::ptr::null_mut();
    }
}

/// The handle `id`, or None if the Rust `z_open` returned no handle where the C++ did (a divergence the trace shows).
fn handle<'a>(z: &'a mut ZCase, id: &str) -> Option<&'a mut ZState> {
    match z.handle.as_mut() {
        Some((hid, h)) if hid == id => Some(h),
        _ => None,
    }
}

const NO_HANDLE: i32 = i32::MIN;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: src_leaves-harness <recorded.jsonl> <out.jsonl> [functions...]");
        std::process::exit(2);
    }
    let mut all = vec![
        "src_leaves.glob_match",
        "src_leaves.z_open",
        "src_leaves.z_close",
        "src_leaves.z_read",
        "src_leaves.z_write",
    ];
    all.extend(zstream::FUNCTIONS);
    let mut only: Vec<&str> = if a.len() > 3 { a[3..].iter().flat_map(|s| s.split(',')).collect() } else { all.clone() };
    // the setup call of a zopen case is replayed whenever any z_* function is
    if only.iter().any(|f| f.starts_with("src_leaves.z_")) {
        only.push(FILE_SETUP);
    }
    let mut z = ZCase::new();
    let mut zs = zstream::ZsReplay::new(a[1].as_ref());
    let n = replay::replay_trace(a[1].as_ref(), a[2].as_ref(), &only, |phase, func, args, _replay, tap| {
        if func.starts_with(zstream::PREFIX) {
            zs.dispatch(phase, func, args, tap);
            return;
        }
        if phase == Phase::Ret {
            return;
        }
        match func {
            "src_leaves.glob_match" => {
                let (pathname, basename, glob) = (bytes(&args[0]), bytes(&args[1]), bytes(&args[2]));
                let (icase, lead, path) = (args[3].as_bool().unwrap(), args[4].as_bool().unwrap(), args[5].as_bool().unwrap());
                tap.call(func, &[&pathname.as_slice(), &basename.as_slice(), &glob.as_slice(), &icase, &lead, &path]);
                let r = signatures::glob_match(&pathname, &basename, &glob, icase, lead, path);
                tap.ret(func, &r);
            }
            FILE_SETUP => {
                if !z.file.is_null() {
                    ffi::close(z.file);
                }
                z = ZCase::new();
                let (data, ro) = (bytes(&args[0]), args[1].as_bool().unwrap());
                z.file = ffi::open_scratch(&scratch_path(), &data, ro);
                tap.call(func, &[&data.as_slice(), &ro]);
                tap.ret(func, &true);
            }
            "src_leaves.z_open" => {
                let file_id = args[0].as_str().unwrap().to_string();
                if !z.ids.contains(&file_id) {
                    z.ids.push(file_id.clone());
                }
                let mode = bytes(&args[1]);
                let (bits, plain) = (args[2].as_i64().unwrap() as i32, args[3].as_i64().unwrap() as i32);
                tap.call(func, &[&file_id, &mode.as_slice(), &bits, &plain]);
                ffi::set_errno(0);
                let r = zopen::z_open(z.file, &mode, bits, plain);
                let err = ffi::errno();
                let id = match r {
                    Some(h) => {
                        let id = format!("#{}", z.ids.len() + 1);
                        z.ids.push(id.clone());
                        z.handle = Some((id.clone(), h));
                        id
                    }
                    None => "null".to_string(),
                };
                tap.ret(func, &id);
                tap.state_after(func, &Raw(format!("[{err}]")));
                if z.handle.is_none() {
                    z.finish(tap, func);
                }
            }
            "src_leaves.z_read" => {
                let (id, len) = (args[0].as_str().unwrap().to_string(), args[1].as_i64().unwrap() as i32);
                tap.call(func, &[&id, &len]);
                let mut buf = vec![0u8; len.max(0) as usize];
                ffi::set_errno(0);
                let r = handle(&mut z, &id).map_or(NO_HANDLE, |h| zopen::z_read(h, &mut buf));
                let err = ffi::errno();
                let got: &[u8] = if r > 0 { &buf[..r as usize] } else { &[] };
                tap.state(func, &got);
                tap.ret(func, &r);
                tap.state_after(func, &Raw(format!("[{err}]")));
            }
            "src_leaves.z_write" => {
                let (id, data, len) = (args[0].as_str().unwrap().to_string(), bytes(&args[1]), args[2].as_i64().unwrap() as i32);
                tap.call(func, &[&id, &data.as_slice(), &len]);
                z.wrote = true;
                ffi::set_errno(0);
                let r = handle(&mut z, &id).map_or(NO_HANDLE, |h| zopen::z_write(h, &data));
                let err = ffi::errno();
                tap.ret(func, &r);
                tap.state_after(func, &Raw(format!("[{err}]")));
            }
            "src_leaves.z_close" => {
                let id = args[0].as_str().unwrap().to_string();
                tap.call(func, &[&id]);
                let h = z.handle.take().filter(|(hid, _)| *hid == id).map(|(_, h)| h);
                ffi::set_errno(0);
                let r = h.map_or(NO_HANDLE, |h| zopen::z_close(*h));
                let err = ffi::errno();
                tap.ret(func, &r);
                tap.state_after(func, &Raw(format!("[{err}]")));
                z.finish(tap, func);
            }
            other => panic!("unknown seam function {other}"),
            }
    })
    .unwrap();
    zs.finish();
    eprintln!("replayed {n} calls");
    let _ = std::fs::remove_file(scratch_path());
}
