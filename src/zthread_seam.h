// Seam declarations for the Zthread decompression thread (component src_core, src/ugrep.cpp), C linkage.
// Used by the struct Zthread facade of src/ugrep.cpp only when CPP_RUST_IMPL_ZTHREAD=rust (msvc/rust.targets defines
// CPP_RUST_IMPL_ZTHREAD_RUST). The Rust side is rust/src_core/src/ffi/zthread.rs; the notes are in
// rust/src_core/SEAM.md ("Zthread seam").
//
// Ownership: ugrep_core_zthread_new returns a handle owned by Rust until ugrep_core_zthread_close. The env struct is
// copied by new(); its ctx (the C++ facade) must outlive the handle. The FILE* file_in and the strings are borrowed
// for one call, except that the FILE* stays in use by the decompression thread until the next start() or close()
// (as with the legacy zstreambuf). The pipe read end returned by start/open_next is a file descriptor owned by
// the C++ caller from then on (the facade fdopen()s it, Grep fclose()s it); the write end stays with Rust.
//
// Threads: start, open_next, cancel and close are called by the one thread that owns the Grep object. The env
// callbacks are called by the decompression threads that Rust creates (set_partname, warning, select_by_flags),
// and by the caller thread (warning). Chained (--zmax>1) stages are internal to Rust.
#ifndef ZTHREAD_SEAM_H
#define ZTHREAD_SEAM_H

#include <stddef.h>
#include <stdio.h>

#ifdef __cplusplus
extern "C" {
#endif

#define ZTHREAD_OK          0   // *fd is the read end of a new pipe
#define ZTHREAD_NONE        1   // no pipe: the legacy functions return NULL (failure already reported, or no next file)
#define ZTHREAD_NOT_PORTED -1   // the stub: nothing was done

// what stays in C++ and is called back by Rust
typedef struct ugrep_core_zthread_env {
  void *ctx;
  // Grep::partname (or the partname of the previous stage) = name[0..len-1], called before data enters a new pipe
  void (*set_partname)(void *ctx, const char *name, size_t len);
  // warning(message, arg) of ugrep.hpp, arg may be NULL; errno is that of the calling thread
  void (*warning)(void *ctx, const char *message, const char *arg);
  // the flags part of select_matching(): hidden files, --exclude/--include globs and -M magic bytes, non-zero if selected
  int (*select_by_flags)(void *ctx, const char *path, const unsigned char *buf, size_t len);
} ugrep_core_zthread_env;

void *ugrep_core_zthread_new(const ugrep_core_zthread_env *env, int is_chained);
// Zthread::start: returns ZTHREAD_*; find may be NULL
int ugrep_core_zthread_start(void *handle, size_t ztstage, const char *pathname, FILE *file_in, const char *find, int *fd);
// Zthread::open_next: returns ZTHREAD_*
int ugrep_core_zthread_open_next(void *handle, const char *pathname, int *fd);
// Zthread::cancel
void ugrep_core_zthread_cancel(void *handle);
// join the decompression threads and free the handle (~Zthread); handle may be NULL
void ugrep_core_zthread_close(void *handle);

#ifdef __cplusplus
}
#endif

#endif
