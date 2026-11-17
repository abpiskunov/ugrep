// Seam facade of zstream.hpp (component src_leaves): the member functions of zstreambuf forward to the Rust-owned
// stream (rust/src_leaves, C ABI of src/zstream_seam.h and the cxx bridge for the pure filename predicates).
// zstreambuf itself stays the std::streambuf adapter of the header: the buffer, cur_ and len_, underflow/uflow/xsgetn
// and the diagnostics (cannot_decompress, warning) are C++, the Rust stream queues the messages and the adapter reports them.
#ifdef HAVE_LIBZ

#ifndef ZSTREAM_FACADE
#define ZSTREAM_FACADE
#endif
#ifdef ZSTREAM_MAX_7ZIP_SIZE
// ugrep.cpp defines this limit before including zstream.hpp, the separate facade TU must agree
#define WITH_MAX_7ZIP_SIZE ZSTREAM_MAX_7ZIP_SIZE
#endif
#include <string>
#include "zstream.hpp"
#include "zstream_seam.h"
#include "src_leaves/src/lib.rs.h"

static rust::Slice<const uint8_t> zs_bytes(const char *s)
{
  return rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t*>(s), std::strlen(s));
}

// the codec set and limits this build was compiled with, handed to the Rust stream when it is opened
static ugrep_rust_zs_config zs_config()
{
  ugrep_rust_zs_config cfg;
  std::memset(&cfg, 0, sizeof(cfg));
  cfg.codecs = ZS_CODEC_ZLIB;
#ifdef HAVE_LIBBZ2
  cfg.codecs |= ZS_CODEC_BZ2;
#endif
#ifdef HAVE_LIBLZMA
  cfg.codecs |= ZS_CODEC_LZMA;
#endif
#ifdef HAVE_LIBLZ4
  cfg.codecs |= ZS_CODEC_LZ4;
#endif
#ifdef HAVE_LIBZSTD
  cfg.codecs |= ZS_CODEC_ZSTD;
#endif
#ifdef HAVE_LIBBROTLI
  cfg.codecs |= ZS_CODEC_BROTLI;
#endif
#ifdef HAVE_LIBBZIP3
  cfg.codecs |= ZS_CODEC_BZIP3;
#endif
#ifndef WITH_NO_7ZIP
  cfg.codecs |= ZS_CODEC_7ZIP;
#endif
#ifdef WITH_ZIP_CRC32
  cfg.zip_crc32 = 1;
#endif
#ifdef WITH_MAX_7ZIP_SIZE
  cfg.max_7zip_size = WITH_MAX_7ZIP_SIZE;
#endif
  return cfg;
}

// report what the Rust stream queued with cannot_decompress() and warning()
static void zs_diagnostics(void *rust, const char *pathname)
{
  int kind = 0;
  const char *message = NULL;
  while (rust != NULL && ugrep_rust_zs_diag(rust, &kind, &message))
  {
    if (kind == ZS_DIAG_WARNING)
      warning(message, pathname);
    else
      cannot_decompress(pathname, message);
  }
}

bool zstreambuf::is_bz(const char *pathname)
{
  return ugrep_rust::zs_is_bz(zs_bytes(pathname));
}

bool zstreambuf::is_xz(const char *pathname)
{
  return ugrep_rust::zs_is_xz(zs_bytes(pathname));
}

bool zstreambuf::is_lz4(const char *pathname)
{
  return ugrep_rust::zs_is_lz4(zs_bytes(pathname));
}

bool zstreambuf::is_zstd(const char *pathname)
{
  return ugrep_rust::zs_is_zstd(zs_bytes(pathname));
}

bool zstreambuf::is_br(const char *pathname)
{
  return ugrep_rust::zs_is_br(zs_bytes(pathname));
}

bool zstreambuf::is_bz3(const char *pathname)
{
  return ugrep_rust::zs_is_bz3(zs_bytes(pathname));
}

bool zstreambuf::is_Z(const char *pathname)
{
  return ugrep_rust::zs_is_z(zs_bytes(pathname));
}

bool zstreambuf::is_zip(const char *pathname)
{
  return ugrep_rust::zs_is_zip(zs_bytes(pathname));
}

bool zstreambuf::is_7z(const char *pathname)
{
  return ugrep_rust::zs_is_7z(zs_bytes(pathname));
}

bool zstreambuf::is_rar(const char *pathname)
{
  return ugrep_rust::zs_is_rar(zs_bytes(pathname));
}

void zstreambuf::open(const char *pathname, FILE *file)
{
  // close old stream, if still open
  if (rust_ != NULL)
  {
    ugrep_rust_zs_close(rust_);
    rust_ = NULL;
  }
  delete zipinfo_;
  zipinfo_ = NULL;

  if (file != NULL)
  {
    pathname_ = pathname;
    file_ = file;

    cur_ = 0;
    len_ = 0;

    ugrep_rust_zs_config cfg = zs_config();
    rust_ = ugrep_rust_zs_open(pathname, file, &cfg);
    zs_diagnostics(rust_, pathname);
    if (rust_ != NULL)
    {
      const unsigned char *initial = NULL;
      size_t initial_len = 0;
      if (ugrep_rust_zs_initial(rust_, &initial, &initial_len) && initial_len > 0 && initial_len <= sizeof(buf_))
      {
        memcpy(buf_, initial, initial_len);
        len_ = initial_len;
      }
      if (ugrep_rust_zs_at_end(rust_))
        file_ = NULL;
    }
  }
}

void zstreambuf::close()
{
  if (rust_ != NULL)
  {
    ugrep_rust_zs_close(rust_);
    rust_ = NULL;
  }
  delete zipinfo_;
  zipinfo_ = NULL;
}

bool zstreambuf::decompressing() const
{
  return rust_ != NULL && ugrep_rust_zs_decompressing(rust_) != 0;
}

std::streamsize zstreambuf::next(unsigned char *buf, size_t len)
{
  std::streamsize num = 0;
  if (rust_ != NULL)
  {
    num = static_cast<std::streamsize>(ugrep_rust_zs_next(rust_, buf, len));
    zs_diagnostics(rust_, pathname_);
    // the Rust stream reports the end of the input where file_ is cleared
    if (ugrep_rust_zs_at_end(rust_))
      file_ = NULL;
  }
  return num;
}

const zstreambuf::ZipInfo *zstreambuf::zipinfo()
{
  const ZipInfo *result = NULL;
  ugrep_rust_zs_zipinfo_t view;
  if (rust_ != NULL)
  {
    int present = ugrep_rust_zs_zipinfo(rust_, &view);
    zs_diagnostics(rust_, pathname_);
    if (present)
    {
      if (zipinfo_ == NULL)
        zipinfo_ = new ZipInfo();
      zipinfo_->version = view.version;
      zipinfo_->flag = view.flag;
      zipinfo_->method = static_cast<ZipInfo::Compression>(view.method);
      zipinfo_->mtime = static_cast<time_t>(view.mtime);
      zipinfo_->crc = view.crc;
      zipinfo_->size = view.size;
      zipinfo_->usize = view.usize;
      zipinfo_->name.assign(view.name, view.name_len);
      result = zipinfo_;
    }
    else
    {
      delete zipinfo_;
      zipinfo_ = NULL;
    }
  }
  return result;
}

std::pair<const unsigned char*,size_t> zstreambuf::zippeek()
{
  std::pair<const unsigned char*,size_t> result(NULL, 0);
  const unsigned char *ptr = NULL;
  size_t len = 0;
  if (rust_ != NULL && ugrep_rust_zs_zippeek(rust_, &ptr, &len))
    result = std::pair<const unsigned char*,size_t>(ptr, len);
  zs_diagnostics(rust_, pathname_);
  return result;
}

#endif // HAVE_LIBZ
