// Seam declarations for the Zthread decompression thread (component src_core, src/ugrep.cpp), C linkage.
// Used by the struct Zthread facade of src/ugrep.cpp. The Rust side is rust/src_core/src/ffi/zthread.rs; the notes are in
// rust/src_core/SEAM.md ("Zthread seam").
//
// Ownership: ugrep_core_zthread_new returns a handle owned by Rust until ugrep_core_zthread_close. The env struct is
// copied by new(); its ctx (the C++ facade) must outlive the handle. The FILE* file_in and the strings are borrowed
// for one call, except that the FILE* stays in use by the decompression thread until the next start() or close()
// (as with the legacy zstreambuf). The pipe read end returned by start/open_next is a file descriptor owned by
// the C++ caller from then on (the facade fdopen()s it, Grep fclose()s it); the write end stays with Rust.
//
// Threads: start, open_next and close are called by the one thread that owns the Grep object (the search thread).
// cancel may be called from ANY thread at the same time, e.g. the query UI thread (Static::cancel_ugrep ->
// Grep::cancel -> Zthread::cancel) while the search thread is inside start/open_next (never close: it runs from ~Grep after the grep handle is cleared); grep_handle_mutex does not
// serialize it. Rust therefore forms only a shared reference to the handle in start/open_next/cancel and keeps its
// state in atomics and Mutexes; the owner must not call close() while another thread can still call cancel(), as with
// the legacy ZthreadCpp. The env
// callbacks are called by the decompression threads that Rust creates (set_partname, warning, select_by_flags),
// and by the caller thread (warning). Chained (--zmax>1) stages are internal to Rust.
#ifndef ZTHREAD_SEAM_H
#define ZTHREAD_SEAM_H

#include <stddef.h>
#include <stdio.h>

#ifdef __cplusplus
extern "C" {
#endif

#define ZTHREAD_OK          0   // *file is the read end of a new pipe, an open FILE* that the caller closes with fclose()
#define ZTHREAD_NONE        1   // no pipe: the legacy functions return NULL (failure already reported, or no next file)

// what stays in C++ and is called back by Rust
typedef struct ugrep_core_zthread_env {
  void *ctx;
  // Grep::partname (or the partname of the previous stage) = name[0..len-1], called before data enters a new pipe
  void (*set_partname)(void *ctx, const char *name, size_t len);
  // warning(message, arg) of ugrep.hpp, arg may be NULL; errno is that of the calling thread
  void (*warning)(void *ctx, const char *message, const char *arg);
  // cannot_decompress(pathname, message) of ugrep.cpp
  void (*cannot_decompress)(void *ctx, const char *pathname, const char *message);
  // an archive (zip, tar, cpio) was detected: if (!flag_no_filename) flag_no_header = false
  void (*archive_found)(void *ctx);
  // the flags part of select_matching(): hidden files, --exclude/--include globs and -M magic bytes, non-zero if selected;
  // buf[len] is readable (the -M matcher scans len + 1 bytes)
  int (*select_by_flags)(void *ctx, const char *path, const unsigned char *buf, size_t len);
  // the build configuration of the zstream (zstream.cpp zs_config()): ZS_CODEC_* bits, WITH_ZIP_CRC32, WITH_MAX_7ZIP_SIZE
  unsigned int zs_codecs;
  int zs_zip_crc32;
  unsigned long long zs_max_7zip_size;
} ugrep_core_zthread_env;

void *ugrep_core_zthread_new(const ugrep_core_zthread_env *env, int is_chained);
// Zthread::start: returns ZTHREAD_*; find may be NULL; the pipe is fdopen()ed by Rust before the thread is told about it
int ugrep_core_zthread_start(void *handle, size_t ztstage, const char *pathname, FILE *file_in, const char *find, FILE **file);
// Zthread::open_next: returns ZTHREAD_*
int ugrep_core_zthread_open_next(void *handle, const char *pathname, FILE **file);
// Zthread::cancel
void ugrep_core_zthread_cancel(void *handle);
// join the decompression threads and free the handle (~Zthread); handle may be NULL
void ugrep_core_zthread_close(void *handle);

#ifdef __cplusplus
}
#endif

#endif
