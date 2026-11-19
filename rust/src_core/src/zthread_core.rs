//! The Zthread unit: the decompression thread of src/ugrep.cpp (`struct ZthreadCpp`) on the zstream core of
//! `src_leaves`. One [`Zthread`] per `Grep`; `start` opens a pipe and starts or wakes the decompression thread, which
//! decompresses (and unpacks zip, tar and cpio parts) into the pipe; `open_next` opens the pipe of the next archive
//! part; `cancel` stops the decompression gracefully. With `--zmax>1` a chain of stages decompresses multi-compressed
//! files: each stage reads the pipe of the next stage and names its parts after it.
//!
//! The handshake of the C++ is kept: `pipe_close` (the thread is waiting, or the receiver closed), `pipe_ready` (the
//! receiver made a new pipe), `pipe_zstrm` (a new job), `part_ready` (the partname was assigned, passed along the chain),
//! with the flags `quit`, `stop`, `is_waiting`, `is_assigned`, `is_extracting`. Differences, all fixing data races or
//! hangs of the C++ (decision D-zthread-races in migration/decisions.md):
//! - the flags `is_extracting`, `is_waiting` and `is_assigned` (`volatile` in C++, no ordering) are under the one
//!   mutex, and every wait has a predicate, so a notification that arrives before the wait is not lost;
//! - `join` wakes both `pipe_zstrm` and `pipe_ready` (the C++ woke only `pipe_zstrm`, a thread waiting in
//!   `wait_pipe_ready` was never woken), and `wait_pipe_ready` wakes on `quit`;
//! - the pipes and streams a failing `start` or a finished job leaves are closed (the C++ leaked them);
//! - a thread that exits sets `exited` and `is_assigned`, so nobody waits for it in vain.
//!
//! Send/Sync: [`Zthread`] holds an `Arc<Stage>`; all shared state of a stage is in a `Mutex`, an atomic or a
//! `Condvar`, so the stage is `Send + Sync` without `unsafe`. The one thing that is not `Send` is the job (a zstream
//! over a `FILE*` that the C++ caller keeps open): it is built by the calling thread, handed over under the state mutex
//! and then used by the decompression thread only (`AssertSend`, `ffi/zthread_sys.rs`). `cancel` only stores an atomic
//! and may run on any thread. There is no `unsafe` in this file and no panic path: indexing is within buffers sized
//! here, and the C-ABI entries (`ffi/zthread.rs`) catch a panic anyway.

use crate::signatures::zthread::{Host, Outcome, ZstreamSettings, NO_FD};
use crate::zthread_sys::{self, AssertSend, CFile, FilePtr};
use src_leaves::signatures::zstream_codecs::{Config, DiagKind, Z_BUF_LEN};
use src_leaves::signatures::zstream_core::ZStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

const MAXLEN: usize = Z_BUF_LEN;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn wait<'a, T>(cv: &Condvar, g: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    cv.wait(g).unwrap_or_else(PoisonError::into_inner)
}

/// The bytes up to the first NUL, as C reads a string.
fn cstr(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().position(|&c| c == 0).unwrap_or(bytes.len());
    &bytes[..end]
}

/// `strtoul`/`strtoull`: the value and the index after the digits, `0` when there are no digits.
fn strto(s: &[u8], base: u64, max: u64) -> (u64, usize) {
    let mut i = 0;
    while matches!(s.get(i), Some(b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')) {
        i += 1;
    }
    let mut neg = false;
    match s.get(i) {
        Some(b'-') => {
            neg = true;
            i += 1;
        }
        Some(b'+') => i += 1,
        _ => {}
    }
    let digit = |c: u8| -> Option<u64> {
        let d = match c {
            b'0'..=b'9' => (c - b'0') as u64,
            b'a'..=b'f' => (c - b'a') as u64 + 10,
            b'A'..=b'F' => (c - b'A') as u64 + 10,
            _ => return None,
        };
        (d < base).then_some(d)
    };
    if base == 16 && s.get(i) == Some(&b'0') && matches!(s.get(i + 1), Some(b'x' | b'X')) && s.get(i + 2).and_then(|&c| digit(c)).is_some() {
        i += 2;
    }
    let first = i;
    let mut value: u64 = 0;
    let mut overflow = false;
    while let Some(d) = s.get(i).and_then(|&c| digit(c)) {
        if !overflow {
            match value.checked_mul(base).and_then(|v| v.checked_add(d)) {
                Some(v) if v <= max => value = v,
                _ => overflow = true,
            }
        }
        i += 1;
    }
    if i == first {
        return (0, 0);
    }
    if overflow {
        return (max, i);
    }
    if neg {
        value = value.wrapping_neg() & max;
    }
    (value, i)
}

/// `strtoul` of the C++ on this platform: `unsigned long` is 32 bits on Windows.
fn strtoul(s: &[u8], base: u64) -> (u64, usize) {
    strto(s, base, if cfg!(windows) { u32::MAX as u64 } else { u64::MAX })
}

/// A numeric header field `buf[at..at+n]` as a C string, `None` when `strtoul` does not consume all of it.
fn field(buf: &[u8], at: usize, n: usize, base: u64) -> Option<u64> {
    let s = cstr(buf.get(at..at + n).unwrap_or(&[]));
    let (value, rest) = strtoul(s, base);
    (rest == s.len()).then_some(value)
}

const USTAR_MAGIC: [u8; 8] = [b'u', b's', b't', b'a', b'r', 0, b'0', b'0'];
const GNUTAR_MAGIC: [u8; 8] = [b'u', b's', b't', b'a', b'r', b' ', b' ', 0];

/// A `zstreambuf` (src/zstream.hpp): the buffer and `cur_`/`len_` of the adapter around the Rust `ZStream`.
struct Zs {
    host: Arc<dyn Host>,
    stream: Option<ZStream>,
    /// One spare byte: `-M` matching reads `buf[len]`.
    buf: Vec<u8>,
    cur: usize,
    len: usize,
    pathname: Vec<u8>,
}

/// What the Rust stream queued with `cannot_decompress()` and `warning()`.
fn drain(host: &dyn Host, pathname: &[u8], stream: &mut ZStream) {
    while let Some((kind, message)) = stream.diag.queue.pop_front() {
        match kind {
            DiagKind::Warning => host.warning(&message, Some(pathname)),
            DiagKind::CannotDecompress => host.cannot_decompress(pathname, &message),
        }
    }
}

impl Zs {
    fn new(host: Arc<dyn Host>) -> Zs {
        Zs { host, stream: None, buf: vec![0; Z_BUF_LEN + 1], cur: 0, len: 0, pathname: Vec::new() }
    }

    /// `zstreambuf::open`: a NULL `file` leaves no stream.
    fn open(&mut self, pathname: &[u8], file: FilePtr) {
        self.stream = None;
        if file.is_null() {
            return;
        }
        self.pathname = cstr(pathname).to_vec();
        self.cur = 0;
        self.len = 0;
        let ZstreamSettings { codecs, zip_crc32, max_7zip_size } = self.host.zstream_config();
        let mut stream = ZStream::open(pathname, file, Config { codecs, zip_crc32, max_7zip_size });
        drain(self.host.as_ref(), &self.pathname, &mut stream);
        let n = stream.initial.len();
        if n > 0 && n <= Z_BUF_LEN {
            self.buf[..n].copy_from_slice(&stream.initial);
            self.len = n;
        }
        self.stream = Some(stream);
    }

    /// `zstreambuf::next`
    fn next(&mut self, off: usize, max: usize) -> isize {
        let Some(stream) = self.stream.as_mut() else {
            return 0;
        };
        let Some(slice) = self.buf.get_mut(off..off.saturating_add(max)) else {
            return -1;
        };
        let num = stream.next(slice);
        drain(self.host.as_ref(), &self.pathname, stream);
        num
    }

    /// `zstreambuf::decompress`: copy or decompress a block into `buf[off..off+max]`, the number of bytes, zero on EOF,
    /// negative on error.
    fn decompress(&mut self, off: usize, max: usize) -> isize {
        if self.cur >= self.len {
            return self.next(off, max);
        }
        let num = (self.len - self.cur).min(max);
        if off.saturating_add(num) > self.buf.len() {
            return -1;
        }
        self.buf.copy_within(self.cur..self.cur + num, off);
        self.cur += num;
        num as isize
    }

    /// `zstreambuf::zipinfo`: the name of the next zip entry.
    fn zipinfo(&mut self) -> Option<Vec<u8>> {
        let stream = self.stream.as_mut()?;
        let name = stream.zipinfo().map(|info| info.name.clone());
        drain(self.host.as_ref(), &self.pathname, stream);
        name
    }
}

/// The work of one `start`: the stream, the pipe from the next stage, the part to find.
pub(crate) struct Job {
    zs: Zs,
    zpipe_in: Option<CFile>,
    findpart: Option<Vec<u8>>,
}

#[derive(Default)]
struct State {
    is_extracting: bool,
    is_waiting: bool,
    is_assigned: bool,
    quit: bool,
    /// The thread has ended.
    exited: bool,
    /// `pipe_zstrm` was notified: a job is waiting.
    zstrm_signal: bool,
    /// `pipe_ready` was notified: the receiver made a new pipe, or failed to.
    ready_signal: bool,
    /// `open_next` failed to make a pipe: the thread gives up the archive, and has not yet gone idle in
    /// `close_wait_zstream_open`, which would close the pipe of a `start` that came in between.
    settling: bool,
    pipe_r: i32,
    pipe_w: i32,
    job: Option<AssertSend<Job>>,
}

/// Where the partname goes: `Grep::partname` through the host, or the `partname` of the previous stage.
enum Sink {
    Host,
    Chain(Arc<Mutex<Vec<u8>>>),
}

struct Stage {
    host: Arc<dyn Host>,
    sink: Sink,
    is_chained: bool,
    /// `partname`: the name of the archive part extracted by the next stage in the chain.
    own_part: Arc<Mutex<Vec<u8>>>,
    stop: AtomicBool,
    chain: Mutex<Option<Arc<Stage>>>,
    st: Mutex<State>,
    pipe_zstrm: Condvar,
    pipe_ready: Condvar,
    pipe_close: Condvar,
    part_ready: Condvar,
    thread: Mutex<Option<JoinHandle<()>>>,
    /// `fdopen` of a pipe read end; replaced in tests to inject its failure.
    fdopen: Opener,
}

/// How the read end of a pipe becomes a `FILE*`.
type Opener = Arc<dyn Fn(i32) -> Option<CFile> + Send + Sync>;

/// Marks the end of the thread however it ends, and wakes everybody who may wait for it.
struct ExitGuard<'a>(&'a Stage);

impl Drop for ExitGuard<'_> {
    fn drop(&mut self) {
        let stage = self.0;
        {
            let mut g = lock(&stage.st);
            g.exited = true;
            zthread_sys::close(g.pipe_w);
            g.pipe_w = NO_FD;
            if stage.is_chained {
                g.is_assigned = true;
            }
            g.job = None;
        }
        stage.pipe_zstrm.notify_all();
        stage.pipe_ready.notify_all();
        stage.pipe_close.notify_all();
        stage.part_ready.notify_all();
    }
}

impl Stage {
    fn new(host: Arc<dyn Host>, sink: Sink, is_chained: bool, fdopen: Opener) -> Arc<Stage> {
        Arc::new(Stage {
            fdopen,
            host,
            sink,
            is_chained,
            own_part: Arc::new(Mutex::new(Vec::new())),
            stop: AtomicBool::new(false),
            chain: Mutex::new(None),
            st: Mutex::new(State { pipe_r: NO_FD, pipe_w: NO_FD, ..State::default() }),
            pipe_zstrm: Condvar::new(),
            pipe_ready: Condvar::new(),
            pipe_close: Condvar::new(),
            part_ready: Condvar::new(),
            thread: Mutex::new(None),
        })
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    fn set_extracting(&self, on: bool) {
        lock(&self.st).is_extracting = on;
    }

    fn pipe_w(&self) -> i32 {
        lock(&self.st).pipe_w
    }

    fn own_part(&self) -> Vec<u8> {
        lock(&self.own_part).clone()
    }

    fn assign_part(&self, name: Vec<u8>) {
        match &self.sink {
            Sink::Host => self.host.set_partname(&name),
            Sink::Chain(part) => *lock(part) = name,
        }
    }

    /// Tell the receiver in the chain that the partname was assigned, also when nothing was selected.
    fn notify_assigned(&self) {
        if self.is_chained {
            lock(&self.st).is_assigned = true;
            self.part_ready.notify_all();
        }
    }

    /// The "cannot create pipe" warning of `open_next`: a chained stage reports no pathname, as `ZthreadCpp::open_next`.
    fn pipe_warning(&self, pathname: &[u8]) {
        self.host.warning(b"cannot create pipe to decompress", (!self.is_chained).then_some(pathname));
    }

    /// `ZthreadCpp::start`: the read end of the new pipe as a `FILE*`, or `None` after reporting the failure. The
    /// `fdopen` happens here, before the job is handed to the thread, so that its failure leaves no thread behind.
    fn start(self: &Arc<Self>, ztstage: usize, pathname: &[u8], file_in: FilePtr, find: Option<&[u8]>) -> Option<CFile> {
        {
            let mut g = lock(&self.st);
            while g.settling && !g.exited {
                g = wait(&self.pipe_close, g);
            }
            g.pipe_r = NO_FD;
            g.pipe_w = NO_FD;
            g.is_assigned = false;
        }

        // a colon separator means we should find the part in a nested archive corresponding to the stage
        let mut findpart = find;
        if let Some(mut part) = find {
            for _ in 1..ztstage {
                match part.iter().position(|&c| c == b':') {
                    Some(colon) => part = &part[colon + 1..],
                    None => break,
                }
            }
            findpart = Some(part);
        }

        // `ZthreadCpp::start` names the file even in a chained stage; only `open_next` drops it there
        let Some((pipe_r, pipe_w)) = zthread_sys::pipe() else {
            self.host.warning(b"cannot create pipe to decompress", Some(pathname));
            return None;
        };
        let Some(pipe_file) = (self.fdopen)(pipe_r) else {
            zthread_sys::close(pipe_r);
            zthread_sys::close(pipe_w);
            self.host.warning(b"cannot create pipe to decompress", Some(pathname));
            return None;
        };
        {
            let mut g = lock(&self.st);
            g.pipe_r = pipe_r;
            g.pipe_w = pipe_w;
        }

        let mut zs = Zs::new(Arc::clone(&self.host));
        let mut zpipe_in = None;
        if ztstage > 1 {
            let chain = Arc::clone(lock(&self.chain).get_or_insert_with(|| {
                Stage::new(Arc::clone(&self.host), Sink::Chain(Arc::clone(&self.own_part)), true, Arc::clone(&self.fdopen))
            }));
            let Some(file) = chain.start(ztstage - 1, pathname, file_in, find) else {
                drop(pipe_file);
                self.close_pipe();
                return None;
            };

            // wait for the partname to be assigned by the next decompression thread in the decompression chain
            {
                let mut g = lock(&chain.st);
                while !g.is_assigned {
                    g = wait(&chain.part_ready, g);
                }
            }
            zs.open(&self.own_part(), file.as_ptr());
            zpipe_in = Some(file);
        } else {
            zs.open(pathname, file_in);
        }

        let job = AssertSend::job(Job { zs, zpipe_in, findpart: findpart.map(<[u8]>::to_vec) });
        let joinable = lock(&self.thread).is_some();
        let mut g = lock(&self.st);
        g.is_extracting = false;
        g.is_waiting = false;
        g.ready_signal = false;
        g.job = Some(job);
        if joinable {
            // wake the decompression thread waiting in close_wait_zstream_open(), there is work to do
            g.zstrm_signal = true;
            drop(g);
            self.pipe_zstrm.notify_all();
        } else {
            g.quit = false;
            g.exited = false;
            g.zstrm_signal = false;
            drop(g);
            self.stop.store(false, Ordering::SeqCst);
            let stage = Arc::clone(self);
            match std::thread::Builder::new().name("ugrep-zthread".into()).spawn(move || stage.run()) {
                Ok(handle) => *lock(&self.thread) = Some(handle),
                Err(_) => {
                    lock(&self.st).job = None;
                    drop(pipe_file);
                    self.close_pipe();
                    self.host.warning(b"cannot create thread to decompress", Some(pathname));
                    return None;
                }
            }
        }
        Some(pipe_file)
    }

    /// Closes the write end of the pipe of a start that failed; the caller drops the read end's `FILE*`.
    fn close_pipe(&self) {
        let mut g = lock(&self.st);
        zthread_sys::close(g.pipe_w);
        g.pipe_r = NO_FD;
        g.pipe_w = NO_FD;
    }

    /// `ZthreadCpp::open_next`: the read end of the pipe to the next file or part in the archive as a `FILE*`, or
    /// `None`. As in `start`, the `fdopen` is done before the thread is told about the pipe; on failure the thread
    /// and the receiver are signalled as for a pipe that could not be created.
    fn open_next(&self, pathname: &[u8]) -> Option<CFile> {
        let mut g = lock(&self.st);
        if g.pipe_r == NO_FD {
            return None;
        }
        // our end of the pipe was closed earlier, before open_next() was called
        g.pipe_r = NO_FD;

        // if extracting and the decompression filter thread is not yet waiting, then wait until it closed its end
        while !g.is_waiting && !g.exited {
            g = wait(&self.pipe_close, g);
        }
        g.is_assigned = false;

        if !g.is_extracting {
            return None;
        }
        if let Some((pipe_r, pipe_w)) = zthread_sys::pipe() {
            if let Some(pipe_file) = (self.fdopen)(pipe_r) {
                g.pipe_r = pipe_r;
                g.pipe_w = pipe_w;
                // notify the decompression filter thread of the new pipe
                g.ready_signal = true;
                self.pipe_ready.notify_all();
                if self.is_chained {
                    // wait for the partname to be set by the next decompression thread in the chain
                    while !g.is_assigned {
                        g = wait(&self.part_ready, g);
                    }
                }
                return Some(pipe_file);
            }
            zthread_sys::close(pipe_r);
            zthread_sys::close(pipe_w);
        }
        drop(g);

        self.pipe_warning(pathname);

        // notify the decompression thread of the closed pipe, and the receiver waiting on the partname
        let mut g = lock(&self.st);
        g.pipe_r = NO_FD;
        g.pipe_w = NO_FD;
        g.ready_signal = true;
        g.is_assigned = true;
        g.settling = true;
        drop(g);
        self.pipe_ready.notify_all();
        self.part_ready.notify_all();
        None
    }

    /// `ZthreadCpp::cancel`
    fn cancel(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let chain = lock(&self.chain).clone();
        if let Some(chain) = chain {
            chain.cancel();
        }
    }

    /// `ZthreadCpp::join`: recursively join all stages of the chain, release the stream.
    fn join(&self) {
        let handle = lock(&self.thread).take();
        // this thread should quit as soon as it is idle or waits for a pipe, so that it releases the pipe of the next
        // stage, which may block in a write to it while it is joined
        if handle.is_some() {
            lock(&self.st).quit = true;
            self.pipe_zstrm.notify_all();
            self.pipe_ready.notify_all();
        }
        let chain = lock(&self.chain).clone();
        if let Some(chain) = chain {
            chain.join();
        }
        if let Some(handle) = handle {
            // wait until the decompression thread closed its end of the pipe, unless it already waits
            let mut g = lock(&self.st);
            while !g.is_waiting && !g.exited {
                g = wait(&self.pipe_close, g);
            }
            drop(g);
            self.pipe_zstrm.notify_all();
            self.pipe_ready.notify_all();
            let _ = handle.join();
        }
        lock(&self.st).job = None;
    }

    /// `ZthreadCpp::wait_pipe_ready`: if the pipe was closed, wait until the receiver opens a new pipe to search the
    /// next file or part in an archive.
    fn wait_pipe_ready(&self) -> bool {
        let mut g = lock(&self.st);
        if g.pipe_w == NO_FD {
            // signal close and wait until a new zstream pipe is ready
            self.pipe_close.notify_all();
            g.is_waiting = true;
            while !g.ready_signal && !g.quit {
                g = wait(&self.pipe_ready, g);
            }
            g.is_waiting = false;
            g.ready_signal = false;
        }
        g.pipe_w != NO_FD
    }

    /// `ZthreadCpp::close_wait_zstream_open`: close the pipe and wait for the next job, unless quitting.
    fn close_wait_zstream_open(&self) {
        let mut g = lock(&self.st);
        zthread_sys::close(g.pipe_w);
        g.pipe_w = NO_FD;
        g.settling = false;
        self.pipe_close.notify_all();
        if !g.quit {
            g.is_waiting = true;
            while !g.zstrm_signal && !g.quit {
                g = wait(&self.pipe_zstrm, g);
            }
            g.is_waiting = false;
            g.zstrm_signal = false;
        }
    }

    /// The decompression thread.
    fn run(self: Arc<Self>) {
        let _exit = ExitGuard(&self);
        loop {
            let job = {
                let mut g = lock(&self.st);
                if g.quit {
                    break;
                }
                g.is_extracting = false;
                g.is_waiting = false;
                g.job.take()
            };
            let Some(job) = job else {
                break;
            };
            let mut job = job.into_inner();
            self.decompress(&mut job);
            // release the stream and the pipe from the next stage before waiting for the next job
            drop(job);

            self.set_extracting(false);
            // when an error occurred or nothing was selected, the receiver may still wait on the partname
            self.notify_assigned();

            self.close_wait_zstream_open();
        }
    }

    /// `ZthreadCpp::decompress`: one job.
    fn decompress(&self, job: &mut Job) {
        let chain = lock(&self.chain).clone();
        while !self.stopped() {
            // to hold the path (prefix + name) extracted from a zip file
            let mut path: Vec<u8> = Vec::new();
            let mut is_regular = true;

            let zipname = job.zs.zipinfo();
            let has_zip = zipname.is_some();
            if let Some(name) = zipname {
                self.set_extracting(true);
                if name.last() == Some(&b'/') {
                    // skip zip directories
                    is_regular = false;
                } else {
                    path = name;
                    // produce headers with zip file pathnames for each archived part
                    self.host.archive_found();
                }
            }

            let mut is_selected = false;
            let mut len = job.zs.decompress(0, MAXLEN);

            if len >= 0 {
                is_selected = true;

                if !self.filter_tar(job, chain.as_ref(), &path, &mut len, &mut is_selected)
                    && !self.filter_cpio(job, chain.as_ref(), &path, &mut len, &mut is_selected)
                {
                    is_selected = is_regular
                        && (!has_zip || {
                            let head = &job.zs.buf[..(len as usize).min(MAXLEN)];
                            self.select_matching(job.findpart.as_deref(), None, &path, head, true)
                        });

                    if is_selected {
                        // if pipe is closed, then wait until receiver reopens it, break if failed
                        if !self.wait_pipe_ready() {
                            if chain.is_some() {
                                job.zpipe_in = None;
                            }
                            break;
                        }

                        // assign the partname before sending to the new pipe
                        let part = match &chain {
                            None => path,
                            Some(_) if path.is_empty() => self.own_part(),
                            Some(_) => join_part(&[&self.own_part(), &path]),
                        };
                        self.assign_part(part);
                        self.notify_assigned();
                    }

                    // push decompressed data into pipe
                    let mut drained = false;
                    while len > 0 && !self.stopped() {
                        // if the pipe is broken the receiver waits for this thread to join, so drain the rest
                        if is_selected && !drained && zthread_sys::write(self.pipe_w(), &job.zs.buf[..len as usize]) < len {
                            // no next stage and decompressing a single file (not zip): stop immediately
                            if chain.is_none() && !has_zip {
                                break;
                            }
                            drained = true;
                        }
                        len = job.zs.decompress(0, MAXLEN);
                    }
                }
            }

            // break if not unzipping or if no more files to unzip
            if job.zs.zipinfo().is_none() {
                let Some(chain) = &chain else {
                    break;
                };
                job.zpipe_in = None;

                // open pipe to the next file in an archive if there is a next file to extract
                let Some(file) = chain.open_next(&self.own_part()) else {
                    break;
                };
                job.zs.open(&self.own_part(), file.as_ptr());
                job.zpipe_in = Some(file);
            }

            // extracting a file
            self.set_extracting(true);

            // after extracting files from an archive, close our end of the pipe and loop for the next file
            if is_selected {
                self.close_pipe_w();
            }
        }
    }

    fn close_pipe_w(&self) {
        let mut g = lock(&self.st);
        zthread_sys::close(g.pipe_w);
        g.pipe_w = NO_FD;
    }

    /// The partname of a part of `archive` in the stage: `[stage:]archive:path`.
    fn tar_part(&self, chain: Option<&Arc<Stage>>, archive: &[u8], path: Vec<u8>) -> Vec<u8> {
        let own = if chain.is_some() { Some(self.own_part()) } else { None };
        let mut names: Vec<&[u8]> = Vec::new();
        if let Some(own) = &own {
            names.push(own);
        }
        if !archive.is_empty() {
            names.push(archive);
        }
        names.push(&path);
        join_part(&names)
    }

    /// `ZthreadCpp::filter_tar`: if a tar/pax file, extract regular file contents and push into pipes one by one,
    /// return true when done.
    fn filter_tar(&self, job: &mut Job, chain: Option<&Arc<Stage>>, archive: &[u8], len: &mut isize, is_selected: &mut bool) -> bool {
        const BLOCKSIZE: isize = 512;
        if *len <= BLOCKSIZE {
            return false;
        }
        let buf = &job.zs.buf;
        let is_ustar = buf[0] != 0 && buf[257..265] == USTAR_MAGIC;
        let is_gnutar = buf[0] != 0 && buf[257..265] == GNUTAR_MAGIC;
        if !is_ustar && !is_gnutar {
            return false;
        }

        // produce headers with tar file pathnames for each archived part
        self.host.archive_found();
        self.set_extracting(true);

        let mut path: Vec<u8> = Vec::new();
        // a long path from the previous header block that is marked with typeflag 'x' or 'L'
        let mut long_path: Vec<u8> = Vec::new();

        while !self.stopped() {
            let buf = &mut job.zs.buf;

            // tar header fields name, size and prefix and make them \0-terminated by overwriting fields we do not use
            buf[100] = 0;
            // path prefix is up to 155 bytes (ustar) or up to 131 bytes (gnutar)
            let prefix_end = 345 + if is_ustar { 155 } else { 131 };
            buf[prefix_end] = 0;

            // gnutar size with leading byte 0x80 (unsigned positive) or leading byte 0xff (negative)
            let mut size: u64 = 0;
            if buf[124] == 0x80 {
                for &b in &buf[125..136] {
                    size = (size << 8).wrapping_add(b as u64);
                }
            } else if buf[124] == 0xff {
                for &b in &buf[124..136] {
                    size = (size << 8).wrapping_add(b as u64);
                }
            } else {
                // v7/ustar size stored in 11 octal digits padded with leading zeros or spaces followed by a space or NUL
                buf[135] = 0;
                size = strto(cstr(&buf[124..136]), 8, u64::MAX).0;
            }

            let typeflag = buf[156];
            let is_regular = typeflag == b'0' || typeflag == 0;
            let is_xhd = typeflag == b'x';
            let is_extended = typeflag == b'L';

            let padding = ((BLOCKSIZE as u64 - size % BLOCKSIZE as u64) % BLOCKSIZE as u64) as isize;

            // assign the (long) tar pathname, name and prefix are now \0-terminated
            path.clear();
            if long_path.is_empty() {
                let prefix = cstr(&buf[345..=prefix_end]);
                if !prefix.is_empty() {
                    path.extend_from_slice(prefix);
                    path.push(b'/');
                }
                path.extend_from_slice(cstr(&buf[..=100]));
            } else {
                path = std::mem::take(&mut long_path);
            }

            // remove header to advance to the body
            *len -= BLOCKSIZE;
            buf.copy_within(BLOCKSIZE as usize..BLOCKSIZE as usize + *len as usize, 0);

            // check if archived file meets selection criteria
            let minlen = (*len as u64).min(size) as usize;
            *is_selected = self.select_matching(job.findpart.as_deref(), Some(archive), &path, &buf[..minlen], is_regular);

            let body = &buf[..minlen];
            if is_xhd {
                // typeflag 'x': extract the long path from the pax extended header block in the body
                if let Some(at) = body.windows(5).position(|w| w == b"path=") {
                    let rest = &body[at..];
                    if let Some(nl) = rest.iter().position(|&c| c == b'\n') {
                        long_path = rest[5..nl].to_vec();
                    }
                }
            } else if is_extended {
                // typeflag 'L': get long name from the body
                long_path = cstr(body).to_vec();
            }

            // if the pipe is closed, then get a new pipe to search the next part in the archive
            if *is_selected {
                if !self.wait_pipe_ready() {
                    break;
                }
                let part = self.tar_part(chain, archive, std::mem::take(&mut path));
                self.assign_part(part);
                self.notify_assigned();
            }

            // it is ok to push the body into the pipe for the main thread to search
            let mut ok = *is_selected;
            while *len > 0 && !self.stopped() {
                let len_out = (*len as u64).min(size) as usize;
                if ok && zthread_sys::write(self.pipe_w(), &job.zs.buf[..len_out]) < len_out as isize {
                    ok = false;
                }
                size -= len_out as u64;

                // reached the end of the tar body?
                if size == 0 {
                    *len -= len_out as isize;
                    job.zs.buf.copy_within(len_out..len_out + *len as usize, 0);
                    break;
                }

                // decompress the next block of data into the buffer
                *len = job.zs.decompress(0, MAXLEN);
            }

            // error?
            if *len < 0 || self.stopped() {
                break;
            }

            // fill the rest of the buffer with decompressed data
            while *len < BLOCKSIZE || (*len as usize) < MAXLEN {
                let len_in = job.zs.decompress(*len as usize, MAXLEN - *len as usize);
                // error or EOF?
                if len_in <= 0 {
                    break;
                }
                *len += len_in;
            }

            // skip padding
            if *len > padding {
                *len -= padding;
                job.zs.buf.copy_within(padding as usize..padding as usize + *len as usize, 0);
            }

            // rest of the file is too short, something is wrong
            if *len <= BLOCKSIZE {
                break;
            }

            // no more parts to extract?
            let buf = &job.zs.buf;
            if buf[0] == 0 || (buf[257..265] != USTAR_MAGIC && buf[257..265] != GNUTAR_MAGIC) {
                break;
            }

            // get a new pipe to search the next part in the archive, if the previous part was a regular file
            if *is_selected {
                self.close_pipe_w();
                *is_selected = false;
            }
        }

        // if we're stopping we still need to notify the receiver in case it is waiting on the partname
        self.notify_assigned();
        true
    }

    /// `ZthreadCpp::filter_cpio`: if a cpio file, extract regular file contents and push into pipes one by one,
    /// return true when done.
    fn filter_cpio(&self, job: &mut Job, chain: Option<&Arc<Stage>>, archive: &[u8], len: &mut isize, is_selected: &mut bool) -> bool {
        const HEADERSIZE: isize = 110;
        const ODC_MAGIC: &[u8; 6] = b"070707";
        const NEWC_MAGIC: &[u8; 6] = b"070701";
        const NEWC_CRC_MAGIC: &[u8; 6] = b"070702";
        let has_magic = |buf: &[u8]| buf[..6] == ODC_MAGIC[..] || buf[..6] == NEWC_MAGIC[..] || buf[..6] == NEWC_CRC_MAGIC[..];

        if *len <= HEADERSIZE || !has_magic(&job.zs.buf) {
            return false;
        }

        // produce headers with cpio file pathnames for each archived part
        self.host.archive_found();
        self.set_extracting(true);

        let mut path: Vec<u8> = Vec::new();
        // need a new pipe, close current pipe first to create a new pipe
        let mut in_progress = false;

        while !self.stopped() {
            let buf = &job.zs.buf;
            // true if odc format, false if newc format
            let is_odc = buf[5] == b'7';
            // odc header length is 76, newc header length is 110
            let header_len: isize = if is_odc { 76 } else { 110 };

            let (namesize, filesize, mode) = if is_odc {
                (field(buf, 59, 6, 8), field(buf, 65, 11, 8), field(buf, 18, 6, 8))
            } else {
                (field(buf, 94, 8, 16), field(buf, 54, 8, 16), field(buf, 14, 8, 16))
            };
            // if not a valid number, then something is wrong: stop when data was read, else this is not a cpio file
            let namesize = match namesize {
                Some(v) => v as usize,
                None if in_progress => break,
                None => return false,
            };
            // pathnames with trailing \0 cannot be empty or too large
            if namesize <= 1 || namesize >= 65536 {
                break;
            }
            let filesize = match filesize {
                Some(v) => v as usize,
                None if in_progress => break,
                None => return false,
            };
            // true if this is a regular file when (mode & 0170000) == 0100000
            let is_regular = match mode {
                Some(v) => v & 0o170000 == 0o100000,
                None if in_progress => break,
                None => return false,
            };

            // remove header to advance to the body
            *len -= header_len;
            job.zs.buf.copy_within(header_len as usize..header_len as usize + *len as usize, 0);

            // assign the cpio pathname
            path.clear();
            let mut size = namesize;
            while *len > 0 && !self.stopped() {
                let n = (*len as usize).min(size);
                path.extend_from_slice(&job.zs.buf[..n]);
                size -= n;
                if size == 0 {
                    // remove pathname to advance to the body
                    *len -= n as isize;
                    job.zs.buf.copy_within(n..n + *len as usize, 0);
                    break;
                }
                // decompress the next block of data into the buffer
                *len = job.zs.decompress(0, MAXLEN);
            }

            // error?
            if *len < 0 || self.stopped() {
                break;
            }

            // remove trailing \0
            if path.last() == Some(&0) {
                path.pop();
            }

            // reached the end of the cpio archive?
            if path == b"TRAILER!!!" {
                break;
            }

            // fill the rest of the buffer with decompressed data
            if (*len as usize) < MAXLEN {
                let len_in = job.zs.decompress(*len as usize, MAXLEN - *len as usize);
                if len_in < 0 {
                    break;
                }
                *len += len_in;
            }

            // skip newc format \0 padding after the pathname
            if !is_odc && *len > 3 {
                let n = 4 - (110 + namesize) % 4;
                *len -= n as isize;
                job.zs.buf.copy_within(n..n + *len as usize, 0);
            }

            // check if archived file meets selection criteria
            let minlen = (*len as usize).min(filesize);
            *is_selected = self.select_matching(job.findpart.as_deref(), Some(archive), &path, &job.zs.buf[..minlen], is_regular);

            // if the pipe is closed, then get a new pipe to search the next part in the archive
            if *is_selected {
                if !self.wait_pipe_ready() {
                    break;
                }
                let part = self.tar_part(chain, archive, std::mem::take(&mut path));
                self.assign_part(part);
                self.notify_assigned();
            }

            // it is ok to push the body into the pipe for the main thread to search
            let mut ok = *is_selected;
            let mut size = filesize;
            while *len > 0 && !self.stopped() {
                let len_out = (*len as usize).min(size);
                if ok && zthread_sys::write(self.pipe_w(), &job.zs.buf[..len_out]) < len_out as isize {
                    ok = false;
                }
                size -= len_out;

                // reached the end of the cpio body?
                if size == 0 {
                    *len -= len_out as isize;
                    job.zs.buf.copy_within(len_out..len_out + *len as usize, 0);
                    break;
                }

                // decompress the next block of data into the buffer
                *len = job.zs.decompress(0, MAXLEN);
            }

            // error?
            if *len < 0 || self.stopped() {
                break;
            }

            if (*len as usize) < MAXLEN {
                // fill the rest of the buffer with decompressed data
                let len_in = job.zs.decompress(*len as usize, MAXLEN - *len as usize);
                if len_in < 0 {
                    break;
                }
                *len += len_in;
            }

            // skip newc format \0 padding
            if !is_odc && *len > 2 {
                let n = (4 - filesize % 4) % 4;
                *len -= n as isize;
                job.zs.buf.copy_within(n..n + *len as usize, 0);
            }

            // rest of the file is too short, something is wrong
            if *len <= HEADERSIZE {
                break;
            }

            // quit if this is not valid cpio header magic
            if !has_magic(&job.zs.buf) {
                break;
            }

            // get a new pipe to search the next part in the archive, if the previous part was a regular file
            if *is_selected {
                self.close_pipe_w();
                in_progress = true;
                *is_selected = false;
            }
        }

        // if we're stopping we still need to notify the receiver in case it is waiting on the partname
        self.notify_assigned();
        true
    }

    /// `ZthreadCpp::select_matching`: true if the path matches the search constraints or `head` contains matching magic
    /// bytes. `head` is a prefix of the stream buffer, which has one spare byte after it for `-M`.
    fn select_matching(&self, findpart: Option<&[u8]>, archive: Option<&[u8]>, path: &[u8], head: &[u8], is_regular: bool) -> bool {
        if !is_regular {
            return false;
        }
        let path = cstr(path);

        // select one specific archive part to search, up to a colon when present
        if let Some(findpart) = findpart {
            let colon = findpart.iter().position(|&c| c == b':');

            // empty archive name means no nested archive, coding artifact
            let archive = archive.map(cstr).filter(|a| !a.is_empty());

            // check if this is a nested archive that is also in findpart up to the first colon
            if let Some(archive) = archive {
                match colon {
                    Some(colon) if archive == &findpart[..colon] => {}
                    _ => return false,
                }
            }

            // if this is not a nested archive, but path is a nested archive, then compare to findpart up to the colon
            if archive.is_none() {
                if let Some(colon) = colon {
                    return path == &findpart[..colon];
                }
            }

            // path to compare to starts after the colon of the archive in findpart
            let mut start = findpart;
            if let Some(colon) = colon {
                start = &findpart[colon + 1..];
                // now compare the path between the colons in findpart
                if let Some(colon) = start.iter().position(|&c| c == b':') {
                    return path == &start[..colon];
                }
            }
            return path == start;
        }

        self.host.select_by_flags(path, head)
    }
}

/// `a:b:c` of the non-empty names given.
fn join_part(names: &[&[u8]]) -> Vec<u8> {
    let mut part = Vec::new();
    for (i, name) in names.iter().enumerate() {
        if i > 0 {
            part.push(b':');
        }
        part.extend_from_slice(name);
    }
    part
}

/// The decompression thread (chain) of one `Grep`.
pub struct Zthread {
    stage: Arc<Stage>,
}

impl Zthread {
    /// `ZthreadCpp::ZthreadCpp`: `is_chained` is true for a stage that feeds another stage; `Grep` passes false.
    pub fn new(host: Arc<dyn Host>, is_chained: bool) -> Zthread {
        Zthread { stage: Stage::new(host, Sink::Host, is_chained, Arc::new(CFile::from_fd)) }
    }

    #[cfg(test)]
    fn with_fdopen(host: Arc<dyn Host>, fdopen: Opener) -> Zthread {
        Zthread { stage: Stage::new(host, Sink::Host, false, fdopen) }
    }

    /// Start the decompression thread if not running, open a new pipe: the read end as a `FILE*` that the caller
    /// closes, or [`Outcome::None`] on failure.
    /// `file_in` is a `FILE*` that must stay open until the next `start` or `close`.
    pub fn start(&self, ztstage: usize, pathname: &[u8], file_in: FilePtr, find: Option<&[u8]>) -> Outcome {
        match self.stage.start(ztstage, pathname, file_in, find) {
            Some(file) => Outcome::Pipe(file.into_raw()),
            None => Outcome::None,
        }
    }

    /// Open the pipe to the next file or part in the archive, or [`Outcome::None`].
    pub fn open_next(&self, pathname: &[u8]) -> Outcome {
        match self.stage.open_next(pathname) {
            Some(file) => Outcome::Pipe(file.into_raw()),
            None => Outcome::None,
        }
    }

    /// Cancel decompression gracefully; may run on any thread.
    pub fn cancel(&self) {
        self.stage.cancel();
    }

    /// Join all stages of the chain and release the streams (`~ZthreadCpp`).
    pub fn close(&mut self) {
        self.stage.join();
    }
}

impl Drop for Zthread {
    fn drop(&mut self) {
        self.stage.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[derive(Default)]
    struct TestHost {
        parts: Mutex<Vec<Vec<u8>>>,
        warnings: Mutex<Vec<Vec<u8>>>,
    }

    impl Host for TestHost {
        fn set_partname(&self, name: &[u8]) {
            lock(&self.parts).push(name.to_vec());
        }
        fn warning(&self, message: &[u8], _arg: Option<&[u8]>) {
            lock(&self.warnings).push(message.to_vec());
        }
        fn cannot_decompress(&self, _pathname: &[u8], message: &[u8]) {
            lock(&self.warnings).push(message.to_vec());
        }
        fn archive_found(&self) {}
        fn select_by_flags(&self, _path: &[u8], _head: &[u8]) -> bool {
            true
        }
        fn zstream_config(&self) -> ZstreamSettings {
            ZstreamSettings { codecs: 1, zip_crc32: false, max_7zip_size: 0 }
        }
    }

    /// A `FILE*` that reads `data`, which must fit the pipe buffer (4096 bytes on Windows).
    fn input(data: &[u8]) -> CFile {
        let (r, w) = zthread_sys::pipe().expect("pipe");
        assert_eq!(zthread_sys::write(w, data), data.len() as isize);
        zthread_sys::close(w);
        CFile::from_fd(r).expect("fdopen")
    }

    fn read_all(file: CFile) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf = [0u8; 512];
        loop {
            let n = file.read(&mut buf);
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        out
    }

    fn pipe_file(outcome: Outcome) -> CFile {
        CFile::from_outcome(outcome)
    }

    /// An `fdopen` that fails for the calls listed (counted from 1) and leaves the descriptor open, as the C library.
    fn failing_fdopen(fail: &'static [usize]) -> Opener {
        let calls = AtomicUsize::new(0);
        Arc::new(move |fd| {
            let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
            if fail.contains(&n) {
                None
            } else {
                CFile::from_fd(fd)
            }
        })
    }

    fn tar_member(out: &mut Vec<u8>, name: &str, body: &[u8]) {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[100..107].copy_from_slice(b"0000644");
        let size = format!("{:011o}", body.len());
        header[124..135].copy_from_slice(size.as_bytes());
        header[156] = b'0';
        header[257..265].copy_from_slice(&USTAR_MAGIC);
        out.extend_from_slice(&header);
        out.extend_from_slice(body);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }

    fn tar_two_members() -> Vec<u8> {
        let mut tar = Vec::new();
        tar_member(&mut tar, "a.txt", b"hello\n");
        tar_member(&mut tar, "b.txt", b"world\n");
        tar.resize(tar.len() + 1024, 0);
        tar
    }

    fn tar_three_members() -> Vec<u8> {
        let mut tar = Vec::new();
        tar_member(&mut tar, "a.txt", b"hello\n");
        tar_member(&mut tar, "b.txt", b"world\n");
        tar_member(&mut tar, "c.txt", b"again\n");
        tar.resize(tar.len() + 1024, 0);
        tar
    }

    #[test]
    fn plain_file_passes_through() {
        let host = Arc::new(TestHost::default());
        let mut z = Zthread::new(host.clone(), false);
        let file = input(b"hello\nworld\n");
        let pipe = pipe_file(z.start(1, b"plain.txt", file.as_ptr(), None));
        assert_eq!(read_all(pipe), b"hello\nworld\n");
        assert_eq!(z.open_next(b"plain.txt"), Outcome::None);
        assert_eq!(*lock(&host.parts), vec![Vec::<u8>::new()]);
        z.close();
    }

    #[test]
    fn tar_parts_are_opened_one_by_one() {
        let tar = tar_two_members();

        let host = Arc::new(TestHost::default());
        let z = Zthread::new(host.clone(), false);
        let file = input(&tar);
        let pipe = pipe_file(z.start(1, b"in.tar", file.as_ptr(), None));
        assert_eq!(read_all(pipe), b"hello\n");
        let pipe = pipe_file(z.open_next(b"in.tar"));
        assert_eq!(read_all(pipe), b"world\n");
        assert_eq!(z.open_next(b"in.tar"), Outcome::None);
        assert_eq!(*lock(&host.parts), vec![b"a.txt".to_vec(), b"b.txt".to_vec()]);
    }

    #[test]
    fn chain_decompresses_in_two_stages() {
        let host = Arc::new(TestHost::default());
        let z = Zthread::new(host.clone(), false);
        let file = input(b"nested\n");
        let pipe = pipe_file(z.start(2, b"x.gz", file.as_ptr(), None));
        assert_eq!(read_all(pipe), b"nested\n");
        assert_eq!(z.open_next(b"x.gz"), Outcome::None);
    }

    #[test]
    fn cancel_then_close_does_not_hang() {
        let host = Arc::new(TestHost::default());
        let mut z = Zthread::new(host, false);
        let file = input(b"cancelled\n");
        let pipe = pipe_file(z.start(1, b"c.txt", file.as_ptr(), None));
        z.cancel();
        let _ = read_all(pipe);
        z.close();
    }

    #[test]
    fn repeated_starts_reuse_the_thread_and_leak_no_descriptor() {
        let host = Arc::new(TestHost::default());
        let z = Zthread::new(host, false);
        let mut first = None;
        for _ in 0..100 {
            let file = input(b"again\n");
            let pipe = pipe_file(z.start(1, b"r.txt", file.as_ptr(), None));
            let fd = pipe.fileno();
            first.get_or_insert(fd);
            assert_eq!(read_all(pipe), b"again\n");
            assert_eq!(z.open_next(b"r.txt"), Outcome::None);
            // the pipe and the input are closed again: the descriptor numbers do not grow
            assert!(fd < first.unwrap_or(0) + 64, "descriptors leak: {fd}");
        }
    }

    #[test]
    fn fdopen_failure_in_start_reports_and_the_next_file_runs() {
        let host = Arc::new(TestHost::default());
        let z = Zthread::with_fdopen(host.clone(), failing_fdopen(&[1]));
        let file = input(b"lost\n");
        assert_eq!(z.start(1, b"bad.txt", file.as_ptr(), None), Outcome::None);
        assert_eq!(*lock(&host.warnings), vec![b"cannot create pipe to decompress".to_vec()]);
        drop(file);

        let file = input(b"second\n");
        let pipe = pipe_file(z.start(1, b"ok.txt", file.as_ptr(), None));
        assert_eq!(read_all(pipe), b"second\n");
        assert_eq!(z.open_next(b"ok.txt"), Outcome::None);
        // only the second file reached the thread: the failed start handed it no job (and no `file_in` to read after
        // the caller closed it)
        assert_eq!(*lock(&host.parts), vec![Vec::<u8>::new()]);
    }

    #[test]
    fn fdopen_failure_in_open_next_reports_and_the_next_file_runs() {
        let host = Arc::new(TestHost::default());
        let z = Zthread::with_fdopen(host.clone(), failing_fdopen(&[2]));
        // a member after the failed one: a thread still extracting would block on it, waiting for a pipe nobody
        // will open, and the next start would never be served
        let file = input(&tar_three_members());
        let pipe = pipe_file(z.start(1, b"in.tar", file.as_ptr(), None));
        assert_eq!(read_all(pipe), b"hello\n");
        assert_eq!(z.open_next(b"in.tar"), Outcome::None);
        assert_eq!(*lock(&host.warnings), vec![b"cannot create pipe to decompress".to_vec()]);
        drop(file);

        // the thread gave up the archive and takes the next file instead of waiting for the lost pipe
        let file = input(b"second\n");
        let pipe = pipe_file(z.start(1, b"ok.txt", file.as_ptr(), None));
        assert_eq!(read_all(pipe), b"second\n");
        assert_eq!(z.open_next(b"ok.txt"), Outcome::None);
    }

    #[test]
    fn find_part_selects_one_member() {
        let host = TestHost::default();
        let stage = Stage::new(Arc::new(host), Sink::Host, false, Arc::new(CFile::from_fd));
        assert!(!stage.select_matching(Some(b"b.txt"), Some(b"in.tar"), b"b.txt", b"", true));
        assert!(stage.select_matching(Some(b"in.tar:b.txt"), Some(b"in.tar"), b"b.txt", b"", true));
        assert!(!stage.select_matching(Some(b"in.tar:b.txt"), Some(b"in.tar"), b"a.txt", b"", true));
        assert!(!stage.select_matching(Some(b"in.tar:b.txt"), Some(b"other.tar"), b"b.txt", b"", true));
        assert!(!stage.select_matching(None, None, b"x", b"", false));
    }

    #[test]
    fn strto_matches_c() {
        assert_eq!(strto(b"0000644", 8, u64::MAX), (0o644, 7));
        assert_eq!(strto(b"  12", 10, u64::MAX), (12, 4));
        assert_eq!(strto(b"zz", 16, u64::MAX), (0, 0));
        assert_eq!(strto(b"1ffffffff", 16, u32::MAX as u64), (u32::MAX as u64, 9));
        assert_eq!(field(b"0000001F", 0, 8, 16), Some(0x1f));
        assert_eq!(field(b"00000x1F", 0, 8, 16), None);
    }
}
