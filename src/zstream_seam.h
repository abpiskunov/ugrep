// Seam declarations for zstream (component src_leaves): the Rust-owned stream behind zstreambuf, C linkage.
// The facade in src/zstream.cpp calls these when CPP_RUST_IMPL_ZSTREAM is defined (sub-switch
// CPP_RUST_IMPL_SRC_LEAVES_ZSTREAM); the pure filename predicates go through the cxx bridge instead
// (rust/src_leaves/src/lib.rs, "src_leaves/src/lib.rs.h"). The Rust side is rust/src_leaves/src/ffi/zstream.rs.
//
// Ownership: ugrep_rust_zs_open returns a handle owned by Rust until ugrep_rust_zs_close; the FILE* and the
// pathname are borrowed from the caller for the whole life of the handle (the pathname is copied). Buffers are
// borrowed for one call. Pointers in ugrep_rust_zs_zipinfo_t and from ugrep_rust_zs_zippeek stay valid until the next
// call on the same handle. Errors are return values; the messages the legacy code printed with cannot_decompress()
// and warning() are queued by Rust and drained by the adapter through ugrep_rust_zs_diag, so Rust never calls C++.
#ifndef ZSTREAM_SEAM_H
#define ZSTREAM_SEAM_H

#include <stddef.h>
#include <stdint.h>
#include <stdio.h>

#ifdef __cplusplus
extern "C" {
#endif

// codec set of the C++ build that opens the stream: the HAVE_LIB* and WITH_NO_7ZIP macros of zstream.hpp
#define ZS_CODEC_ZLIB   0x01u  // zlib is always built in
#define ZS_CODEC_BZ2    0x02u  // HAVE_LIBBZ2
#define ZS_CODEC_LZMA   0x04u  // HAVE_LIBLZMA
#define ZS_CODEC_LZ4    0x08u  // HAVE_LIBLZ4
#define ZS_CODEC_ZSTD   0x10u  // HAVE_LIBZSTD
#define ZS_CODEC_BROTLI 0x20u  // HAVE_LIBBROTLI
#define ZS_CODEC_BZIP3  0x40u  // HAVE_LIBBZIP3
#define ZS_CODEC_7ZIP   0x80u  // not WITH_NO_7ZIP

typedef struct ugrep_rust_zs_config {
  uint32_t codecs;         // ZS_CODEC_* bits
  uint32_t zip_crc32;      // 1 when WITH_ZIP_CRC32 is defined
  uint64_t max_7zip_size;  // WITH_MAX_7ZIP_SIZE, 0 when not defined
} ugrep_rust_zs_config;

// what ZipInfo of zstream.hpp exposes
typedef struct ugrep_rust_zs_zipinfo_t {
  const char *name;   // not NUL-terminated
  size_t name_len;
  int64_t mtime;      // time_t
  uint64_t size;
  uint64_t usize;
  uint32_t crc;
  uint16_t version;
  uint16_t flag;
  uint16_t method;    // ZipInfo::Compression
} ugrep_rust_zs_zipinfo_t;

#define ZS_DIAG_CANNOT_DECOMPRESS 1  // cannot_decompress(pathname, message)
#define ZS_DIAG_WARNING           2  // warning(message, pathname)

// open the stream for a pathname and file; never NULL for a non-NULL file, a plain file is passed through
void *ugrep_rust_zs_open(const char *pathname, FILE *file, const ugrep_rust_zs_config *config);
void ugrep_rust_zs_close(void *handle);
// true if the stream decompresses (zstreambuf::decompressing)
int ugrep_rust_zs_decompressing(const void *handle);
// zstreambuf::next: bytes into buf[0..len-1], 0 at EOF, negative on error
ptrdiff_t ugrep_rust_zs_next(void *handle, unsigned char *buf, size_t len);
// true after the stream reached the end of its input or failed, where the legacy code sets file_ to NULL
int ugrep_rust_zs_at_end(const void *handle);
// the bytes of a plain file read while the format was detected, to be handed out before the rest of the file
int ugrep_rust_zs_initial(const void *handle, const unsigned char **ptr, size_t *len);
// zstreambuf::zipinfo: advance to the next zip/7z entry, true and *out filled when there is one
int ugrep_rust_zs_zipinfo(void *handle, ugrep_rust_zs_zipinfo_t *out);
// zstreambuf::zippeek: true and the pending compressed zip data when unzipping
int ugrep_rust_zs_zippeek(void *handle, const unsigned char **ptr, size_t *len);
// pop one queued diagnostic, true with *kind (ZS_DIAG_*) and a NUL-terminated *message valid until the next call
int ugrep_rust_zs_diag(void *handle, int *kind, const char **message);

#ifdef __cplusplus
}
#endif

#endif
