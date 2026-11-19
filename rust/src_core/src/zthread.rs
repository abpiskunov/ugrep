//! Types and signatures of the Zthread seam (the decompression thread of src/ugrep.cpp, `struct ZthreadCpp`),
//! fixed by the Prepare seam task. The bodies are stubs that report `Outcome::NotPorted`; the Port task replaces
//! them and never edits a signature. Declared from `signatures.rs` with `#[path]`, so the replay harness that
//! includes `signatures.rs` alone builds it; it names no `cxx` type and uses no crate path.
//!
//! The notes (encoding, re-entrancy, thread affinity, owner side, error kinds) are in `SEAM.md`, "Zthread seam".

use std::ffi::c_void;
use std::sync::Arc;

/// A C `FILE*`, borrowed from the caller. Opaque here; `ffi/zthread.rs` and `ffi/cfile`-style wrappers read it.
pub type FilePtr = *mut c_void;

/// The "no descriptor" value.
pub const NO_FD: i32 = -1;

/// What a crossing hands back instead of a `FILE*`: the legacy functions return NULL on failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The read end of a new pipe, a file descriptor that the caller owns from now on.
    Pipe(i32),
    /// The legacy NULL: failure already reported through [`Host::warning`], or no next file in the archive.
    None,
    /// The stub: nothing was done.
    NotPorted,
}

/// What stays in C++ and is called back; the facade of src/ugrep.cpp implements it. Bytes carry no NUL and no
/// encoding is assumed (file names are OS bytes).
pub trait Host: Send + Sync {
    /// `Grep::partname` (or the partname of the previous stage) becomes `name`; called before data enters a new pipe.
    fn set_partname(&self, name: &[u8]);
    /// `warning(message, arg)` of ugrep.hpp, `arg` is NULL when `None`.
    fn warning(&self, message: &[u8], arg: Option<&[u8]>);
    /// The flags part of `select_matching()`: hidden files, `--exclude`/`--include` globs, `-M` magic bytes.
    fn select_by_flags(&self, path: &[u8], head: &[u8]) -> bool;
}

/// The decompression thread state of one `Grep` (and, inside Rust, of each stage of a `--zmax>1` chain).
pub struct Zthread {
    pub host: Arc<dyn Host>,
    pub is_chained: bool,
}

impl Zthread {
    /// `ZthreadCpp::ZthreadCpp(is_chained, partname)`; the partname lives behind [`Host::set_partname`].
    pub fn new(host: Arc<dyn Host>, is_chained: bool) -> Zthread {
        Zthread { host, is_chained }
    }

    /// `ZthreadCpp::start`: start the decompression thread if it is not running and open a new pipe.
    /// `ztstage` is `flag_zmax`; `find` selects a part of a (nested) archive. `file_in` is borrowed until the next
    /// `start` or `close`.
    pub fn start(&mut self, _ztstage: usize, _pathname: &[u8], _file_in: FilePtr, _find: Option<&[u8]>) -> Outcome {
        Outcome::NotPorted
    }

    /// `ZthreadCpp::open_next`: open the pipe to the next file or part of the archive.
    pub fn open_next(&mut self, _pathname: &[u8]) -> Outcome {
        Outcome::NotPorted
    }

    /// `ZthreadCpp::cancel`: stop decompressing gracefully, recursively down the chain.
    pub fn cancel(&self) {}

    /// `ZthreadCpp::join` and the destructor: join all stages and release the stream. Safe to call again.
    pub fn close(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoHost;

    impl Host for NoHost {
        fn set_partname(&self, _name: &[u8]) {}
        fn warning(&self, _message: &[u8], _arg: Option<&[u8]>) {}
        fn select_by_flags(&self, _path: &[u8], _head: &[u8]) -> bool {
            false
        }
    }

    #[test]
    fn stub_reports_not_ported() {
        let mut z = Zthread::new(Arc::new(NoHost), false);
        assert_eq!(z.start(1, b"a.gz", std::ptr::null_mut(), None), Outcome::NotPorted);
        assert_eq!(z.open_next(b"a.gz"), Outcome::NotPorted);
        z.cancel();
        z.close();
        z.close();
        assert!(!z.is_chained);
    }
}
