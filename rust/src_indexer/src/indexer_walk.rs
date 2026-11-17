//! Unit `indexer_walk`: src/ugrep-indexer.cpp lines 1050-1843 (trim, getline, import_globs, include_dir,
//! include_file, `cat`, `deleter`, `indexer`). One coarse seam call each: the whole tree walk, the index file
//! reads and writes and the progress output happen inside the call.
//!
//! Output goes through the CRT `stdout` (`fwrite`) on Windows, like the `printf` of the C++, so the order relative to
//! what the retained C++ printed before the call, and the flush at `exit`, are the ones of the C++. `warning` and
//! `error` flush, the other lines do not, as in the C++. Directories are listed with the Win32 `FindFirstFileW`
//! family on Windows (attributes and reparse tags as the C++ reads them) and with `std::fs::read_dir` elsewhere.
//!
//! The reference build has no `HAVE_LIBZ` (Q-indexer-5), so every file is one `index` call and `archive` stays false.

use super::indexer_core::{self, noise_percentage, Input};
use super::{Exit, IndexerFlags};
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

#[path = "ffi.rs"]
mod ffi;
#[path = "glob.rs"]
mod glob;

#[cfg(windows)]
use ffi::win;
use ffi::sys;

#[cfg(windows)]
const PATHSEP: u8 = b'\\';
#[cfg(not(windows))]
const PATHSEP: u8 = b'/';

const INDEX_FILENAME: &[u8] = b"._UG#_Store";
/// `char ugrep_index_file_magic[5] = "UG#\x03"`: written with its terminating NUL
const INDEX_MAGIC: [u8; 5] = *b"UG#\x03\0";
const HEADER_SIZE: usize = 4;
const MAX_HASHES: usize = 65536;

#[cfg(windows)]
const ENOENT: i32 = 2;
#[cfg(windows)]
const EACCES: i32 = 13;
const EINVAL: i32 = 22;

/// Seam function `src_indexer.indexer` (`indexer()`): recursively index the tree at `path` (None is `.`).
/// `warnings` is the C++ counter at the call. `--check` ends the process: `Exit::exited` with the exit code.
pub fn indexer(flags: &IndexerFlags, path: Option<&[u8]>, warnings: usize) -> Exit {
    Walker::new(flags, warnings).indexer(path)
}

/// Seam function `src_indexer.deleter` (`deleter()`): recursively delete the index files under `path`.
pub fn deleter(flags: &IndexerFlags, path: Option<&[u8]>, warnings: usize) -> Exit {
    let mut walker = Walker::new(flags, warnings);
    walker.deleter(path);
    walker.exit(false, 0)
}

/// `std::path::PathBuf` of a UTF-8 (Windows) or byte (elsewhere) pathname
fn os_path(path: &[u8]) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        std::path::PathBuf::from(String::from_utf8_lossy(path).into_owned())
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStrExt;
        std::path::PathBuf::from(std::ffi::OsStr::from_bytes(path))
    }
}

/// `fopenw_s(&file, path, mode)` for the modes the walk uses ("r", "rb", "r+b", "wb")
fn open_mode(path: &[u8], mode: &str) -> std::io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    match mode {
        "wb" => options.write(true).create(true).truncate(true),
        "r+b" => options.read(true).write(true),
        _ => options.read(true),
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;
        options.share_mode(FILE_SHARE_READ);
    }
    options.open(os_path(path))
}

/// The `errno` the C++ has after the failed open of `fopenw_s`
fn open_errno(e: &std::io::Error) -> i32 {
    #[cfg(windows)]
    {
        if e.raw_os_error() == Some(win::ERROR_ACCESS_DENIED) {
            EACCES
        } else {
            ENOENT
        }
    }
    #[cfg(not(windows))]
    {
        e.raw_os_error().unwrap_or(EINVAL)
    }
}

/// The `errno` of a failed CRT call such as `lstat`
#[cfg(not(windows))]
fn crt_errno(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(EINVAL)
}

/// Text mode of the CRT stream the ignore files are opened with ("r"): CRLF becomes LF, Ctrl-Z ends the file
fn text_mode(raw: Vec<u8>) -> Vec<u8> {
    if cfg!(not(windows)) {
        return raw;
    }
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        match raw[i] {
            0x1a => break,
            b'\r' if raw.get(i + 1) == Some(&b'\n') => {
                out.push(b'\n');
                i += 2;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// `static_cast<unsigned>(x)` of a double
fn to_unsigned(x: f64) -> u32 {
    (x as i64) as u32
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t'..=b'\r')
}

/// trim white space from either end of the line
fn trim(line: &[u8]) -> &[u8] {
    let start = line.iter().position(|&c| !is_space(c)).unwrap_or(line.len());
    let end = line.iter().rposition(|&c| !is_space(c)).map_or(start, |p| p + 1);
    &line[start..end]
}

/// read the next line of `data` from `*pos`, None at the end of the input (`getline()` of the C++ returns true)
fn getline(data: &[u8], pos: &mut usize) -> Option<Vec<u8>> {
    let mut line = Vec::new();
    let mut eof = true;
    while *pos < data.len() {
        let c = data[*pos];
        *pos += 1;
        if c == b'\n' {
            eof = false;
            break;
        }
        line.push(c);
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    if eof && line.is_empty() {
        None
    } else {
        Some(line)
    }
}

/// the whole text of an ignore file as `reflex::BufferedInput` over the `FILE*` delivers it
fn read_text(mut file: File) -> Vec<u8> {
    let mut raw = Vec::new();
    let _ = file.read_to_end(&mut raw);
    let mut input = Input::from_reader(Box::new(Cursor::new(text_mode(raw))));
    let mut data = Vec::new();
    let mut buf = [0u8; 16384];
    loop {
        let n = input.get(&mut buf);
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
    }
    data
}

/// ignore (exclude) files/dirs globs, a glob prefixed with ! means override to include
#[derive(Default)]
struct Ignore {
    files: Vec<Vec<u8>>,
    dirs: Vec<Vec<u8>>,
}

/// read globs from a file and split them into files or dirs to include or exclude by pushing them onto the vectors
fn import_globs(data: &[u8], files: &mut Vec<Vec<u8>>, dirs: &mut Vec<Vec<u8>>) {
    let mut pos = 0;
    while let Some(line) = getline(data, &mut pos) {
        let mut line = trim(&line).to_vec();
        // add glob to files or dirs using gitignore glob pattern rules
        if !line.is_empty() && line[0] != b'#' && (line[0] != b'!' || line.len() > 1) {
            if line.last() == Some(&b'/') {
                if line.len() > 1 {
                    line.pop();
                }
                dirs.push(line);
            } else {
                files.push(line.clone());
                dirs.push(line);
            }
        }
    }
}

/// true if `pathname` is not excluded by the globs, a glob prefixed with ! overrides the exclusion
fn included(globs: Option<&Vec<Vec<u8>>>, pathname: &[u8], basename: &[u8]) -> bool {
    let mut ok = true;
    for glob in globs.into_iter().flatten() {
        if glob[0] == b'!' {
            if !ok && glob::glob_match(pathname, basename, &glob[1..], false, false, false) {
                ok = true;
            }
        } else if ok && glob::glob_match(pathname, basename, glob, false, false, false) {
            ok = false;
        }
    }
    ok
}

/// entry data extracted from directory contents
struct Entry {
    pathname: Vec<u8>,
    /// length of the basename in the pathname
    base: usize,
    mtime: u64,
    size: u64,
}

impl Entry {
    /// indexing is initiated with the pathname to the root of the directory to index
    fn root(pathname: &[u8]) -> Entry {
        let base = pathname.iter().rposition(|&c| c == PATHSEP).map_or(0, |p| pathname.len() - p - 1);
        Entry { pathname: pathname.to_vec(), base, mtime: u64::MAX, size: 0 }
    }

    fn basename(&self) -> &[u8] {
        &self.pathname[self.pathname.len() - self.base..]
    }
}

#[derive(Default)]
struct Counts {
    num_dirs: u64,
    num_links: u64,
    num_other: u64,
    ign_dirs: i64,
    ign_files: i64,
}

/// the tallies of `indexer()`
#[derive(Default)]
struct Stats {
    num_files: u64,
    add_dirs: i64,
    add_files: i64,
    mod_files: i64,
    del_files: i64,
    bin_files: i64,
    not_files: i64,
    zip_files: i64,
    sum_hashes_size: i64,
    sum_files_size: i64,
    sum_noise: f32,
}

/// modification times found by `cat`: of the index file and the newest of the files
#[derive(Default)]
struct Times {
    index_time: u64,
    last_time: u64,
}

/// what `index()` leaves in its out-parameters for one file or archive part
struct FileState {
    hashes: Vec<u8>,
    noise: f32,
    compressed: bool,
    archive: bool,
    binary: bool,
    size: u64,
    partname: Vec<u8>,
}

/// the entry pathname of `name` in the directory `pathname`
fn join(pathname: &[u8], name: &[u8]) -> Vec<u8> {
    if pathname.is_empty() || pathname == b"." {
        return name.to_vec();
    }
    let mut path = pathname.to_vec();
    if pathname.last() != Some(&PATHSEP) {
        path.push(PATHSEP);
    }
    path.extend_from_slice(name);
    path
}

fn index_filename_of(pathname: &[u8]) -> Vec<u8> {
    let mut name = pathname.to_vec();
    name.push(PATHSEP);
    name.extend_from_slice(INDEX_FILENAME);
    name
}

/// the bytes of a C string stored in `bytes`
fn c_str(bytes: &[u8]) -> &[u8] {
    bytes.iter().position(|&c| c == 0).map_or(bytes, |n| &bytes[..n])
}

struct Walker<'a> {
    flags: &'a IndexerFlags,
    warnings: usize,
    /// stack of ignore file/dir globs per ignore-file found
    ignore_stack: Vec<Ignore>,
}

impl<'a> Walker<'a> {
    fn new(flags: &'a IndexerFlags, warnings: usize) -> Walker<'a> {
        Walker { flags, warnings, ignore_stack: Vec::new() }
    }

    fn exit(&self, exited: bool, exit_code: i32) -> Exit {
        Exit { exited, exit_code, warnings: self.warnings }
    }

    fn message(&mut self, kind: &str, message: &str, arg: &[u8], errno: Option<i32>) {
        self.warnings += 1;
        if self.flags.no_messages {
            return;
        }
        let mut line = format!("ugrep-indexer: {kind}: {message} ").into_bytes();
        line.extend_from_slice(arg);
        if let Some(errno) = errno {
            line.extend_from_slice(b": ");
            line.extend_from_slice(&sys::strerror_bytes(errno));
        }
        line.push(b'\n');
        sys::write_stdout(&line);
        sys::flush_stdout();
    }

    /// display a warning message unless option -s (--no-messages)
    #[cfg(windows)]
    fn warning(&mut self, message: &str, arg: &[u8]) {
        self.message("warning", message, arg, None);
    }

    /// display an error message unless option -s (--no-messages)
    fn error(&mut self, message: &str, arg: &[u8], errno: i32) {
        self.message("error", message, arg, Some(errno));
    }

    /// return true if pathname is a non-excluded directory
    fn include_dir(&self, pathname: &[u8], basename: &[u8]) -> bool {
        included(self.ignore_stack.last().map(|i| &i.dirs), pathname, basename)
    }

    /// return true if pathname is a non-excluded file
    fn include_file(&self, pathname: &[u8], basename: &[u8]) -> bool {
        included(self.ignore_stack.last().map(|i| &i.files), pathname, basename)
    }

    /// check for ignore files, read them and push globs on the ignore_stack
    fn import_ignore_files(&mut self, pathname: &[u8], dir_entries: &mut Vec<Entry>) {
        let flags = self.flags;
        for ignore in &flags.ignore_files {
            let mut filepath = pathname.to_vec();
            filepath.push(PATHSEP);
            filepath.extend_from_slice(ignore);
            if let Ok(file) = open_mode(&filepath, "r") {
                // push globs imported from the ignore file to the back of the vectors
                self.ignore_stack.push(Ignore::default());
                // mark dir_entries stack with an empty pathname as a sentinel to pop the ignore_stack afterwards
                dir_entries.push(Entry::root(b""));
                let top = self.ignore_stack.last_mut().unwrap();
                import_globs(&read_text(file), &mut top.files, &mut top.dirs);
            }
        }
    }

    fn pop_sentinels(&mut self, dir_entries: &mut Vec<Entry>) {
        while !self.ignore_stack.is_empty() && dir_entries.last().is_some_and(|e| e.pathname.is_empty()) {
            dir_entries.pop();
            self.ignore_stack.pop();
        }
    }

    /// catalog directory contents: file entries are replaced, directory entries are appended
    #[cfg(windows)]
    fn cat(
        &mut self,
        pathname: &[u8],
        dir_entries: &mut Vec<Entry>,
        file_entries: &mut Vec<Entry>,
        counts: &mut Counts,
        dir_only: bool,
    ) -> Times {
        file_entries.clear();
        let mut times = Times::default();

        let glob = if pathname != b"." { [pathname, b"/*"].concat() } else { b"*".to_vec() };
        let mut ffd = win::FindData::new();
        let mut find = match win::Find::first(&glob, &mut ffd) {
            Ok(find) => find,
            Err(code) => {
                if code != win::ERROR_FILE_NOT_FOUND {
                    self.warning("cannot open directory", pathname);
                }
                return times;
            }
        };

        if !dir_only {
            self.import_ignore_files(pathname, dir_entries);
        }

        counts.num_dirs += 1;

        loop {
            let name = ffd.file_name();
            let entry_pathname = join(pathname, &name);
            let attr = win::file_attributes(&entry_pathname);

            if attr == win::INVALID_FILE_ATTRIBUTES {
                self.error("cannot read", &entry_pathname, ENOENT);
            } else if attr & (win::ATTR_DIRECTORY | win::ATTR_DEVICE) == 0 && name == INDEX_FILENAME {
                // get index file modification time
                times.index_time = ffd.modified_time();
            } else {
                // search directory entries that aren't hidden
                let first = name.first().copied().unwrap_or(0);
                let second = name.get(1).copied().unwrap_or(0);
                if (first != b'.' && attr & (win::ATTR_HIDDEN | win::ATTR_SYSTEM) == 0)
                    || (self.flags.hidden && second != 0 && second != b'.')
                {
                    let symlink = ffd.attributes & win::ATTR_REPARSE_POINT != 0 && ffd.reserved0 == win::IO_REPARSE_TAG_SYMLINK;
                    if attr & win::ATTR_DIRECTORY != 0 {
                        // check if this is a dir to index and not a symlink
                        if !symlink && (dir_only || self.include_dir(&entry_pathname, &name)) {
                            dir_entries.push(Entry {
                                pathname: entry_pathname,
                                base: name.len(),
                                mtime: ffd.modified_time(),
                                size: ffd.file_size(),
                            });
                        } else {
                            counts.ign_dirs += 1;
                        }
                    } else if attr & win::ATTR_DEVICE == 0 && !dir_only {
                        // check if this is a file to index and not a symlink unless -S is specified
                        if (self.flags.dereference_files || !symlink) && self.include_file(&entry_pathname, &name) {
                            let file_time = ffd.modified_time();
                            times.last_time = times.last_time.max(file_time);
                            file_entries.push(Entry {
                                pathname: entry_pathname,
                                base: name.len(),
                                mtime: file_time,
                                size: ffd.file_size(),
                            });
                        } else {
                            counts.ign_files += 1;
                        }
                    } else {
                        counts.num_other += 1;
                    }
                }
            }

            if !find.next(&mut ffd) {
                break;
            }
        }
        drop(find);

        self.pop_sentinels(dir_entries);
        times
    }

    /// catalog directory contents: file entries are replaced, directory entries are appended
    #[cfg(not(windows))]
    fn cat(
        &mut self,
        pathname: &[u8],
        dir_entries: &mut Vec<Entry>,
        file_entries: &mut Vec<Entry>,
        counts: &mut Counts,
        dir_only: bool,
    ) -> Times {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::MetadataExt;

        file_entries.clear();
        let mut times = Times::default();

        let dir = match std::fs::read_dir(os_path(pathname)) {
            Ok(dir) => dir,
            Err(e) => {
                self.error("cannot open directory", pathname, crt_errno(&e));
                return times;
            }
        };

        if !dir_only {
            self.import_ignore_files(pathname, dir_entries);
        }

        counts.num_dirs += 1;

        // microseconds, as `modified_time(const struct stat&)`
        let modified_time = |m: &std::fs::Metadata| (m.mtime() as u64).wrapping_mul(1_000_000).wrapping_add((m.mtime_nsec() / 1000) as u64);

        for dirent in dir.flatten() {
            let name = dirent.file_name().as_bytes().to_vec();
            let entry_pathname = join(pathname, &name);

            let meta = match std::fs::symlink_metadata(os_path(&entry_pathname)) {
                Ok(meta) => meta,
                Err(e) => {
                    self.error("cannot stat", &entry_pathname, crt_errno(&e));
                    continue;
                }
            };
            let kind = meta.file_type();

            if kind.is_file() && name == INDEX_FILENAME {
                // get index file modification time
                times.index_time = modified_time(&meta);
            } else if name[0] != b'.' || (self.flags.hidden && name.get(1).is_some_and(|&c| c != b'.')) {
                if kind.is_dir() {
                    if dir_only || self.include_dir(&entry_pathname, &name) {
                        dir_entries.push(Entry {
                            pathname: entry_pathname,
                            base: name.len(),
                            mtime: modified_time(&meta),
                            size: meta.len(),
                        });
                    } else {
                        counts.ign_dirs += 1;
                    }
                } else if kind.is_file() && !dir_only {
                    if self.include_file(&entry_pathname, &name) {
                        let file_time = modified_time(&meta);
                        times.last_time = times.last_time.max(file_time);
                        file_entries.push(Entry { pathname: entry_pathname, base: name.len(), mtime: file_time, size: meta.len() });
                    } else {
                        counts.ign_files += 1;
                    }
                } else if kind.is_symlink() && !dir_only {
                    match std::fs::metadata(os_path(&entry_pathname)) {
                        Ok(target) if self.flags.dereference_files && target.is_file() => {
                            if self.include_file(&entry_pathname, &name) {
                                let file_time = modified_time(&target);
                                times.last_time = times.last_time.max(file_time);
                                file_entries.push(Entry {
                                    pathname: entry_pathname,
                                    base: name.len(),
                                    mtime: file_time,
                                    size: target.len(),
                                });
                            } else {
                                counts.ign_files += 1;
                            }
                        }
                        _ => counts.num_links += 1,
                    }
                } else {
                    counts.num_other += 1;
                }
            }
        }

        self.pop_sentinels(dir_entries);
        times
    }

    /// recursively delete index files
    fn deleter(&mut self, pathname: Option<&[u8]>) {
        let mut dir_entries: Vec<Entry> = Vec::new();
        let mut file_entries: Vec<Entry> = Vec::new();
        let mut counts = Counts::default();
        let mut num_removed: u64 = 0;

        // pathname to the directory tree to index or .
        dir_entries.push(Entry::root(pathname.unwrap_or(b".")));

        // recurse subdirectories breadth-first to remove index files
        while let Some(visit) = dir_entries.pop() {
            let times = self.cat(&visit.pathname, &mut dir_entries, &mut file_entries, &mut counts, true);

            // if index time is nonzero, there is a valid index file in this directory that we should remove
            if times.index_time > 0 {
                let index_filename = index_filename_of(&visit.pathname);
                if let Err(e) =                 sys::remove_file(&index_filename) {
                                    self.error("cannot remove", &index_filename, e);
                } else {
                    num_removed += 1;
                    if self.flags.verbose {
                        let mut line = format!("D{num_removed:>12} ").into_bytes();
                        line.extend_from_slice(c_str(&index_filename));
                        line.push(b'\n');
                        sys::write_stdout(&line);
                    }
                }
            }
        }

        if !self.flags.quiet {
            sys::write_stdout(format!("\n{num_removed:>13} indexes removed from {} directories\n\n", counts.num_dirs).as_bytes());
        }
    }

    /// the `errno` of the failed `index()` of `pathname`: the open of the file failed
    fn index_errno(pathname: &[u8]) -> i32 {
        match open_mode(pathname, "rb") {
            Ok(_) => EINVAL,
            Err(e) => open_errno(&e),
        }
    }

    /// one `index()` call: false when it fails, otherwise the out-parameters are stored in `st`
    fn index(flags: &IndexerFlags, handle: &mut u64, pathname: &[u8], st: &mut FileState) -> bool {
        let r = indexer_core::index(flags, *handle, pathname, st.archive);
        *handle = r.handle;
        if !r.ok {
            return false;
        }
        // `index()` sets size to 0 on entry, so its early returns leave size == 0
        st.size = r.size;
        st.hashes = r.hashes;
        st.noise = r.noise;
        st.compressed = r.compressed;
        st.archive = r.archive;
        st.binary = r.binary;
        st.partname = r.partname;
        true
    }

    /// walk the entries of an existing index file with the magic header: keep the entries of the files that are
    /// present and not updated and compact the file in place (not when just checking), tally the others and remove
    /// the cataloged file entries that are indexed
    fn update_index(&mut self, file: &mut File, visit: &Entry, file_entries: &mut Vec<Entry>, index_time: u64, st: &mut Stats) {
        let check = self.flags.check;
        let mut basename = vec![0u8; 65536];
        let mut hashes = vec![0u8; MAX_HASHES];
        let mut inpos = INDEX_MAGIC.len() as u64;
        let mut outpos = inpos;
        // the postponed archive entry of the cat file entries
        let mut archive_entry: Option<usize> = None;

        loop {
            let mut header = [0u8; HEADER_SIZE];
            if file.seek(SeekFrom::Start(inpos)).is_err() || file.read_exact(&mut header).is_err() {
                break;
            }

            // hashes table size, zero to skip empty files and binary files when -I is specified
            let logsize = header[1] & 0x1f;
            let hashes_size = if logsize > 0 { 1usize << logsize } else { 0 };

            // sanity check
            if hashes_size > MAX_HASHES {
                break;
            }

            let basename_size = usize::from(header[2]) | (usize::from(header[3]) << 8);
            if file.read_exact(&mut basename[..basename_size]).is_err() {
                break;
            }
            let name = &basename[..basename_size];

            let mut entry = archive_entry;

            // if not the same archive filename, then remove the postponed archive entry from the cat file entries
            if let Some(e) = entry {
                if file_entries[e].base != basename_size || file_entries[e].basename() != name {
                    file_entries.remove(e);
                    entry = None;
                    archive_entry = None;
                }
            }

            // search the directory contents to find the indexed file
            if entry.is_none() {
                entry = file_entries.iter().position(|e| e.base == basename_size && e.basename() == name);
            }

            let archive = header[1] & 0x40 != 0;
            let binary = header[1] & 0x80 != 0;
            let record = (HEADER_SIZE + basename_size + hashes_size) as u64;

            match entry {
                // if file is present in the directory and not updated, then preserve entry in the index
                Some(e) if file_entries[e].mtime <= index_time => {
                    st.num_files += 1;

                    // binary files registered but not indexed
                    st.bin_files += i64::from(binary);
                    st.not_files += i64::from(binary && hashes_size == 0);

                    if inpos > outpos {
                        if file.read_exact(&mut hashes[..hashes_size]).is_err() {
                            break;
                        }

                        // move header, basename, and hashes to the front of the index file (only happens when not just checking)
                        if file.seek(SeekFrom::Start(outpos)).is_err()
                            || file.write_all(&header).is_err()
                            || file.write_all(name).is_err()
                            || file.write_all(&hashes[..hashes_size]).is_err()
                        {
                            self.error("cannot update index file in", &visit.pathname, EINVAL);
                            break;
                        }
                    }

                    // remove file entry from the cat file entries unless multi-part archive
                    if archive {
                        // postpone removing this archive entry
                        archive_entry = Some(e);
                    } else {
                        file_entries.remove(e);
                        archive_entry = None;
                    }

                    outpos += record;
                }
                None => {
                    st.del_files += 1;

                    if check {
                        outpos += record;
                    } else {
                        if self.flags.verbose {
                            let mut line = b"D           -  -% ".to_vec();
                            line.extend_from_slice(c_str(name));
                            line.push(b'\n');
                            sys::write_stdout(&line);
                        }

                        st.sum_hashes_size -= record as i64;
                    }
                }
                Some(_) => {
                    // modified indexed file, when not ignored binary
                    if !binary || hashes_size != 0 {
                        st.mod_files += 1;
                        st.add_files -= 1;
                    }

                    if check {
                        outpos += record;
                    } else {
                        st.sum_hashes_size -= record as i64;
                    }
                }
            }

            inpos += record;
        }

        // make sure to remove postponed archive file entry
        if let Some(e) = archive_entry {
            file_entries.remove(e);
        }

        if inpos > outpos && (file.seek(SeekFrom::Start(outpos)).is_err() || file.set_len(outpos).is_err()) {
            self.error("cannot update index file in", &visit.pathname, EINVAL);
        }
    }

    /// index the cataloged files of one directory by appending their entries to the open index file
    fn index_files(&mut self, file: &mut File, visit: &Entry, file_entries: &[Entry], st: &mut Stats) {
        let flags = self.flags;
        let mut handle = 0u64;

        for entry in file_entries {
            // if the file is a a zip archive, then index archived content for each part
            let mut fs = FileState {
                hashes: Vec::new(),
                noise: 0.0,
                compressed: false,
                archive: false,
                binary: false,
                size: entry.size,
                partname: Vec::new(),
            };
            let pathname = &entry.pathname;

            if fs.size == 0 || Self::index(flags, &mut handle, pathname, &mut fs) {
                loop {
                    // binary files registered but not indexed
                    st.bin_files += i64::from(fs.binary);
                    st.not_files += i64::from(fs.binary && fs.size == 0);

                    if !fs.archive || fs.size > 0 {
                        let percent = to_unsigned(100.0 * f64::from(fs.noise) + 0.5);
                        if flags.verbose {
                            let mut classification = b' ';
                            if fs.compressed {
                                classification = b'C';
                            }
                            if fs.archive {
                                classification = b'A';
                            }
                            if fs.binary {
                                classification = if fs.size == 0 { b'I' } else { b'B' };
                            }
                            let mut line = format!("{}{:>12}{percent:>3}% ", classification as char, fs.size).into_bytes();
                            line.extend_from_slice(c_str(pathname));
                            if fs.archive {
                                line.push(b'{');
                                line.extend_from_slice(c_str(&fs.partname));
                                line.push(b'}');
                            }
                            line.push(b'\n');
                            sys::write_stdout(&line);
                        }

                        // log2 of the hashes table size, zero to skip empty files and binary files when -I is specified
                        let hashes_size = fs.hashes.len();
                        let mut logsize = 0u8;
                        let mut k = hashes_size;
                        while k > 1 {
                            logsize += 1;
                            k >>= 1;
                        }

                        // mark high bits
                        logsize |= (u8::from(fs.binary) << 7) | (u8::from(fs.archive) << 6) | (u8::from(fs.compressed) << 5);

                        let basename = entry.basename();
                        let basename_size = basename.len().min(65535);
                        let header = [
                            (flags.accuracy as u8).wrapping_add(b'0'),
                            logsize,
                            basename_size as u8,
                            (basename_size >> 8) as u8,
                        ];

                        // write header with basename, log of the hashes size and hashes
                        let written = file
                            .write_all(&header)
                            .and_then(|_| file.write_all(&basename[..basename_size]))
                            .and_then(|_| file.write_all(&fs.hashes));
                        if written.is_err() {
                            self.error("cannot write index file in", &visit.pathname, EINVAL);
                            if !fs.archive {
                                break;
                            }
                        }

                        st.zip_files += i64::from(fs.archive);
                        st.num_files += 1;
                        st.add_files += i64::from(!fs.binary || hashes_size != 0);
                        st.sum_files_size += fs.size as i64;
                        st.sum_noise += fs.noise;
                        st.sum_hashes_size += (HEADER_SIZE + basename_size + hashes_size) as i64;
                    }

                    if !(fs.archive && Self::index(flags, &mut handle, pathname, &mut fs)) {
                        break;
                    }
                }
            } else {
                self.error("cannot index", pathname, Self::index_errno(pathname));
            }
        }

        if handle != 0 {
            indexer_core::stream_drop(handle);
        }
    }

    /// recursively index files
    fn indexer(&mut self, path: Option<&[u8]>) -> Exit {
        let flags = self.flags;
        let yes_no = |b: bool| if b { "yes" } else { "no" };

        if !flags.no_messages && !flags.check && !flags.quiet {
            let mut banner = format!(
                "\n> index accuracy: {} ({}%~{}% noise)",
                flags.accuracy,
                noise_percentage(flags.accuracy + 1),
                noise_percentage(flags.accuracy)
            )
            .into_bytes();
            banner.extend_from_slice(format!("\n> decompress:     {}", yes_no(flags.decompress)).as_bytes());
            if flags.decompress {
                banner.extend_from_slice(format!(" (zmax={})", flags.zmax).as_bytes());
            }
            banner.extend_from_slice(format!("\n> ignore binary:  {}", yes_no(flags.ignore_binary)).as_bytes());
            if flags.ignore_files.is_empty() {
                banner.extend_from_slice(b"\n> ignore files:   no");
            }
            for ignore in &flags.ignore_files {
                banner.extend_from_slice(b"\n> ignore files:   \"");
                banner.extend_from_slice(c_str(ignore));
                banner.push(b'"');
            }
            banner.extend_from_slice(format!("\n> index hidden:   {}\n\n", yes_no(flags.hidden)).as_bytes());
            sys::write_stdout(&banner);
        }

        let mut dir_entries: Vec<Entry> = Vec::new();
        let mut file_entries: Vec<Entry> = Vec::new();
        let mut counts = Counts::default();
        let mut st = Stats::default();

        // argument path to the directory tree to index or .
        dir_entries.push(Entry::root(path.unwrap_or(b".")));

        // recurse subdirectories
        while let Some(visit) = dir_entries.pop() {
            let mut index_file: Option<File> = None;

            let times = self.cat(&visit.pathname, &mut dir_entries, &mut file_entries, &mut counts, false);
            let (index_time, last_time) = (times.index_time, times.last_time);

            let index_filename = index_filename_of(&visit.pathname);

            if !flags.force {
                if index_time > 0 {
                    // if the index file was the last modified file in this directory, then visit the next directory
                    if last_time <= index_time && visit.mtime <= index_time {
                        st.num_files += file_entries.len() as u64;
                        continue;
                    }

                    match open_mode(&index_filename, if flags.check { "rb" } else { "r+b" }) {
                        Ok(mut file) => {
                            let mut check_magic = [0u8; INDEX_MAGIC.len()];
                            if file.read_exact(&mut check_magic).is_ok() && check_magic == INDEX_MAGIC {
                                self.update_index(&mut file, &visit, &mut file_entries, index_time, &mut st);
                                index_file = Some(file);
                            } else {
                                st.add_dirs += 1;
                            }
                        }
                        Err(_) => st.add_dirs += 1,
                    }
                } else {
                    st.add_dirs += 1;
                }
            }

            // create a new index file when none is present
            if index_file.is_none() && !flags.check {
                match open_mode(&index_filename, "wb") {
                    Ok(mut file) => {
                        if file.write_all(&INDEX_MAGIC).is_ok() {
                            index_file = Some(file);
                        } else {
                            self.error("cannot create index file in", &visit.pathname, EINVAL);
                        }
                    }
                    Err(e) => self.error("cannot create index file in", &visit.pathname, open_errno(&e)),
                }
            }

            match index_file.as_mut() {
                Some(file) if !flags.check => self.index_files(file, &visit, &file_entries, &mut st),
                _ => st.add_files += file_entries.len() as i64,
            }
        }

        let average = |count: i64| to_unsigned(100.0 * f64::from(st.sum_noise) / count as f64 + 0.5);

        if st.sum_files_size > 0 {
            if flags.verbose {
                sys::write_stdout(format!(" ------------ ---\n{:>13}{:>3}%\n", st.sum_files_size, average(st.mod_files + st.add_files)).as_bytes());
            } else if !flags.no_messages {
                sys::write_stdout(
                    format!(
                        "\n{:>13} bytes scanned and indexed with {}% noise on average",
                        st.sum_files_size,
                        average(st.mod_files + st.add_files)
                    )
                    .as_bytes(),
                );
            }
        }

        let stale = !(st.add_dirs == 0 && st.add_files == 0 && st.mod_files == 0 && st.del_files == 0);
        let ignored = format!(
            "{:>13} directories ignored with --ignore-files\n{:>13} files ignored with --ignore-files\n",
            counts.ign_dirs, counts.ign_files
        );
        let skipped = format!("{:>13} symbolic links skipped\n{:>13} devices skipped\n", counts.num_links, counts.num_other);
        let warned = format!("{:>13} warnings and errors\n", self.warnings);

        if flags.check {
            if !flags.quiet {
                let mut text = format!(
                    "\n{:>13} files indexed in {} directories\n{:>13} directories not indexed\n{:>13} new files not indexed\n{:>13} modified files not indexed\n{:>13} deleted files are needlessly indexed\n{:>13} binary files indexed\n{:>13} binary files ignored with --ignore-binary\n",
                    st.num_files,
                    counts.num_dirs,
                    st.add_dirs,
                    st.add_files,
                    st.mod_files,
                    st.del_files - counts.ign_files,
                    st.bin_files - st.not_files,
                    st.not_files
                );
                if !flags.ignore_files.is_empty() {
                    text.push_str(&ignored);
                }
                text.push_str(&skipped);
                if self.warnings > 0 {
                    text.push_str(&warned);
                }
                if stale {
                    text.push_str("\nWarning: some indexes appear to be stale and are outdated or missing\n\n");
                } else {
                    text.push_str("\nChecked: indexes are fresh and up to date\n\n");
                }
                sys::write_stdout(text.as_bytes());
            }
            // --check exits with 0 for up to date or 1 missing or outdated index files
            return self.exit(true, if stale { 1 } else { 0 });
        }

        if !flags.quiet {
            let mut text = format!("\n{:>13} files indexed in {} directories\n{:>13} new directories indexed\n", st.num_files, counts.num_dirs, st.add_dirs);
            if flags.decompress && st.zip_files > 0 {
                text.push_str(&format!("{:>13} new files indexed ({} in archives)\n", st.add_files, st.zip_files));
            } else {
                text.push_str(&format!("{:>13} new files indexed\n", st.add_files));
            }
            text.push_str(&format!(
                "{:>13} modified files indexed\n{:>13} deleted files removed from indexes\n{:>13} binary files indexed\n{:>13} binary files ignored with --ignore-binary\n",
                st.mod_files,
                st.del_files,
                st.bin_files - st.not_files,
                st.not_files
            ));
            if !flags.ignore_files.is_empty() {
                text.push_str(&ignored);
            }
            text.push_str(&skipped);
            if self.warnings > 0 {
                text.push_str(&warned);
            }
            if st.sum_hashes_size > 0 {
                // int64_t / uint64_t divides as unsigned
                text.push_str(&format!(
                    "{:>13} bytes indexing storage increase at {} bytes/file\n\n",
                    st.sum_hashes_size,
                    st.sum_hashes_size as u64 / st.num_files
                ));
            } else {
                text.push_str(&format!("{:>13} bytes indexing storage decrease\n\n", st.sum_hashes_size));
            }
            text.push_str("Indexes are fresh and up to date\n\n");
            sys::write_stdout(text.as_bytes());
        }
        self.exit(false, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags() -> IndexerFlags {
        IndexerFlags {
            accuracy: 4,
            check: false,
            decompress: false,
            delete: false,
            dereference_files: false,
            force: false,
            hidden: false,
            ignore_binary: false,
            no_messages: true,
            quiet: true,
            usage_warnings: false,
            verbose: false,
            zmax: 1,
            ignore_files: Vec::new(),
        }
    }

    fn temp(name: &str) -> Vec<u8> {
        let dir = std::env::temp_dir().join(format!("walk_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().into_owned().into_bytes()
    }

    #[test]
    fn trim_and_getline() {
        assert_eq!(trim(b" \t a b \r\n"), b"a b");
        assert_eq!(trim(b"  "), b"");
        let data = b"one\r\ntwo\n\nlast";
        let mut pos = 0;
        let lines: Vec<Vec<u8>> = std::iter::from_fn(|| getline(data, &mut pos)).collect();
        assert_eq!(lines, vec![b"one".to_vec(), b"two".to_vec(), b"".to_vec(), b"last".to_vec()]);
    }

    #[test]
    fn globs_split_into_files_and_dirs() {
        let (mut files, mut dirs) = (Vec::new(), Vec::new());
        import_globs(b"# comment\n*.o\nbuild/\n!\n!keep.o\n\n/\n", &mut files, &mut dirs);
        assert_eq!(files, vec![b"*.o".to_vec(), b"!keep.o".to_vec()]);
        assert_eq!(dirs, vec![b"*.o".to_vec(), b"build".to_vec(), b"!keep.o".to_vec(), b"/".to_vec()]);
    }

    #[test]
    fn index_then_check_then_delete() {
        let root = temp("cycle");
        let dir = String::from_utf8(root.clone()).unwrap();
        std::fs::write(format!("{dir}/a.txt"), b"hello world, hello index").unwrap();
        std::fs::create_dir_all(format!("{dir}/sub")).unwrap();
        std::fs::write(format!("{dir}/sub/b.txt"), b"second file").unwrap();

        let r = indexer(&flags(), Some(&root), 0);
        assert!(!r.exited && r.warnings == 0);
        let store = std::fs::read(format!("{dir}/._UG#_Store")).unwrap();
        assert_eq!(&store[..5], b"UG#\x03\0");
        assert!(std::path::Path::new(&format!("{dir}/sub/._UG#_Store")).exists());

        let mut check = flags();
        check.check = true;
        let r = indexer(&check, Some(&root), 0);
        assert!(r.exited && r.exit_code == 0);

        let r = deleter(&flags(), Some(&root), 0);
        assert!(!r.exited);
        assert!(!std::path::Path::new(&format!("{dir}/._UG#_Store")).exists());
        let r = indexer(&check, Some(&root), 0);
        assert!(r.exited && r.exit_code == 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn index_early_return_leaves_size_zero() {
        let root = temp("size0");
        let dir = String::from_utf8(root).unwrap();
        let path = format!("{dir}/bin.dat");
        std::fs::write(&path, b"\0\x01\x02binary\0data\0").unwrap();
        let mut f = flags();
        f.ignore_binary = true;
        let mut st = FileState {
            hashes: Vec::new(),
            noise: 0.0,
            compressed: false,
            archive: false,
            binary: false,
            size: 12345,
            partname: Vec::new(),
        };
        let mut handle = 0u64;
        assert!(Walker::index(&f, &mut handle, path.as_bytes(), &mut st));
        assert!(st.binary && st.hashes.is_empty());
        assert_eq!(st.size, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deleter_fails_on_read_only_index() {
        let root = temp("readonly");
        let dir = String::from_utf8(root.clone()).unwrap();
        std::fs::write(format!("{dir}/a.txt"), b"hello world, hello index").unwrap();
        indexer(&flags(), Some(&root), 0);
        let store = format!("{dir}/._UG#_Store");
        let original = std::fs::metadata(&store).unwrap().permissions();
        let mut perms = original.clone();
        perms.set_readonly(true);
        std::fs::set_permissions(&store, perms).unwrap();

        let r = deleter(&flags(), Some(&root), 0);
        assert_eq!(r.warnings, 1);
        assert!(std::path::Path::new(&store).exists());

        std::fs::set_permissions(&store, original).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_directory_is_a_warning() {
        let root = temp("missing");
        let mut gone = root.clone();
        gone.extend_from_slice(b"/nope");
        let mut f = flags();
        f.no_messages = false;
        f.quiet = true;
        let r = indexer(&f, Some(&gone), 0);
        assert!(r.warnings >= 1);
        let _ = std::fs::remove_dir_all(String::from_utf8(root).unwrap());
    }
}
