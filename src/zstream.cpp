// Seam facade of zstream.hpp (component src_leaves): the member functions of zstreambuf and its ZipInfo
// that used to be defined in the header. The functions in "Facade" select one implementation at build time:
// CPP_RUST_IMPL_ZSTREAM forwards to the Rust-owned stream (rust/src_leaves, C ABI of src/zstream_seam.h and the
// cxx bridge for the pure predicates), otherwise the legacy bodies below run, moved from the header unchanged
// (name suffix _cpp). zstreambuf itself stays the std::streambuf adapter of the header: the buffer, cur_ and len_,
// underflow/uflow/xsgetn and the diagnostics (cannot_decompress, warning) are C++ in both builds.
// The legacy bodies are grouped in the two port units, see rust/src_leaves/SEAM.md.
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

#ifdef CPP_RUST_IMPL_ZSTREAM
#include "src_leaves/src/lib.rs.h"
#endif

#ifdef CPP2RUST_TAP
#include "../rust/src_leaves/cpp2rust_tap.hpp"

// object identity without addresses: "#n" in first-seen order per case (TAP_OBJ without its quotes)
static std::string zs_tap_obj(const void *p)
{
  if (p == nullptr)
    return "null";
  std::string s = TAP_OBJ(p);
  return s.substr(1, s.size() - 2);
}

static std::string zs_tap_bytes(const unsigned char *buf, std::streamsize n)
{
  return n > 0 && buf != nullptr ? std::string(reinterpret_cast<const char*>(buf), static_cast<size_t>(n)) : std::string();
}
#else
#define TAP_CALL(fn, ...) ((void)0)
#define TAP_STATE(v) ((void)0)
#define TAP_RET(v) ((void)0)
#define TAP_OUT(callee, ...) ((void)0)
#endif

#ifdef CPP_RUST_IMPL_ZSTREAM

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

// report what the Rust stream queued with the exact calls the legacy code made, so the messages stay in C++
static void zs_diagnostics(void *rust, const char *pathname)
{
  int kind = 0;
  const char *message = NULL;
  while (rust != NULL && ugrep_rust_zs_diag(rust, &kind, &message))
  {
    TAP_OUT(kind == ZS_DIAG_WARNING ? "ugrep.warning" : "ugrep.cannot_decompress", message);
    if (kind == ZS_DIAG_WARNING)
      warning(message, pathname);
    else
      cannot_decompress(pathname, message);
  }
}

#endif

// ---------------------------------------------------------------------------------------------------------------
// Facade: the seam functions of the unit zstream_core, ids src_leaves.zs_*
// ---------------------------------------------------------------------------------------------------------------


bool zstreambuf::is_bz(const char *pathname)
{
  TAP_CALL("src_leaves.zs_is_bz", pathname);
#ifdef CPP_RUST_IMPL_ZSTREAM
  bool result = ugrep_rust::zs_is_bz(zs_bytes(pathname));
#else
  bool result = is_bz_cpp(pathname);
#endif
  TAP_RET(result);
  return result;
}

bool zstreambuf::is_xz(const char *pathname)
{
  TAP_CALL("src_leaves.zs_is_xz", pathname);
#ifdef CPP_RUST_IMPL_ZSTREAM
  bool result = ugrep_rust::zs_is_xz(zs_bytes(pathname));
#else
  bool result = is_xz_cpp(pathname);
#endif
  TAP_RET(result);
  return result;
}

bool zstreambuf::is_lz4(const char *pathname)
{
  TAP_CALL("src_leaves.zs_is_lz4", pathname);
#ifdef CPP_RUST_IMPL_ZSTREAM
  bool result = ugrep_rust::zs_is_lz4(zs_bytes(pathname));
#else
  bool result = is_lz4_cpp(pathname);
#endif
  TAP_RET(result);
  return result;
}

bool zstreambuf::is_zstd(const char *pathname)
{
  TAP_CALL("src_leaves.zs_is_zstd", pathname);
#ifdef CPP_RUST_IMPL_ZSTREAM
  bool result = ugrep_rust::zs_is_zstd(zs_bytes(pathname));
#else
  bool result = is_zstd_cpp(pathname);
#endif
  TAP_RET(result);
  return result;
}

bool zstreambuf::is_br(const char *pathname)
{
  TAP_CALL("src_leaves.zs_is_br", pathname);
#ifdef CPP_RUST_IMPL_ZSTREAM
  bool result = ugrep_rust::zs_is_br(zs_bytes(pathname));
#else
  bool result = is_br_cpp(pathname);
#endif
  TAP_RET(result);
  return result;
}

bool zstreambuf::is_bz3(const char *pathname)
{
  TAP_CALL("src_leaves.zs_is_bz3", pathname);
#ifdef CPP_RUST_IMPL_ZSTREAM
  bool result = ugrep_rust::zs_is_bz3(zs_bytes(pathname));
#else
  bool result = is_bz3_cpp(pathname);
#endif
  TAP_RET(result);
  return result;
}

bool zstreambuf::is_Z(const char *pathname)
{
  TAP_CALL("src_leaves.zs_is_Z", pathname);
#ifdef CPP_RUST_IMPL_ZSTREAM
  bool result = ugrep_rust::zs_is_z(zs_bytes(pathname));
#else
  bool result = is_Z_cpp(pathname);
#endif
  TAP_RET(result);
  return result;
}

bool zstreambuf::is_zip(const char *pathname)
{
  TAP_CALL("src_leaves.zs_is_zip", pathname);
#ifdef CPP_RUST_IMPL_ZSTREAM
  bool result = ugrep_rust::zs_is_zip(zs_bytes(pathname));
#else
  bool result = is_zip_cpp(pathname);
#endif
  TAP_RET(result);
  return result;
}

bool zstreambuf::is_7z(const char *pathname)
{
  TAP_CALL("src_leaves.zs_is_7z", pathname);
#ifdef CPP_RUST_IMPL_ZSTREAM
  bool result = ugrep_rust::zs_is_7z(zs_bytes(pathname));
#else
  bool result = is_7z_cpp(pathname);
#endif
  TAP_RET(result);
  return result;
}

bool zstreambuf::is_rar(const char *pathname)
{
  TAP_CALL("src_leaves.zs_is_rar", pathname);
#ifdef CPP_RUST_IMPL_ZSTREAM
  bool result = ugrep_rust::zs_is_rar(zs_bytes(pathname));
#else
  bool result = is_rar_cpp(pathname);
#endif
  TAP_RET(result);
  return result;
}

void zstreambuf::open(const char *pathname, FILE *file)
{
  TAP_CALL("src_leaves.zs_open", pathname, zs_tap_obj(file));
#ifdef CPP_RUST_IMPL_ZSTREAM
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
#else
  open_cpp(pathname, file);
#endif
  TAP_RET(0);
}

void zstreambuf::close()
{
  TAP_CALL("src_leaves.zs_close");
#ifdef CPP_RUST_IMPL_ZSTREAM
  if (rust_ != NULL)
  {
    ugrep_rust_zs_close(rust_);
    rust_ = NULL;
  }
  delete zipinfo_;
  zipinfo_ = NULL;
#else
  close_cpp();
#endif
  TAP_RET(0);
}

bool zstreambuf::decompressing() const
{
  TAP_CALL("src_leaves.zs_decompressing");
#ifdef CPP_RUST_IMPL_ZSTREAM
  bool result = rust_ != NULL && ugrep_rust_zs_decompressing(rust_) != 0;
#else
  bool result = decompressing_cpp();
#endif
  TAP_RET(result);
  return result;
}

std::streamsize zstreambuf::next(unsigned char *buf, size_t len)
{
  TAP_CALL("src_leaves.zs_next", static_cast<uint64_t>(len));
#ifdef CPP_RUST_IMPL_ZSTREAM
  std::streamsize num = 0;
  if (rust_ != NULL)
  {
    num = static_cast<std::streamsize>(ugrep_rust_zs_next(rust_, buf, len));
    zs_diagnostics(rust_, pathname_);
    // the Rust stream reports the end of the input where the legacy code clears file_
    if (ugrep_rust_zs_at_end(rust_))
      file_ = NULL;
  }
#else
  std::streamsize num = next_cpp(buf, len);
#endif
  TAP_STATE(zs_tap_bytes(buf, num));
  TAP_RET(static_cast<int64_t>(num));
  return num;
}

const zstreambuf::ZipInfo *zstreambuf::zipinfo()
{
  TAP_CALL("src_leaves.zs_zipinfo");
#ifdef CPP_RUST_IMPL_ZSTREAM
  const ZipInfo *result = NULL;
  ugrep_rust_zs_zipinfo_t view;
  if (rust_ != NULL)
  {
    int present = ugrep_rust_zs_zipinfo(rust_, &view);
    zs_diagnostics(rust_, pathname_);
    if (present)
    {
      // the ZipInfo is only a view of what the Rust stream reports, it decompresses nothing
      if (zipinfo_ == NULL)
        zipinfo_ = new ZipInfo(pathname_, NULL);
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
#else
  const ZipInfo *result = zipinfo_cpp();
#endif
  if (result != NULL)
    TAP_STATE(result->name);
  TAP_RET(result != NULL);
  return result;
}

std::pair<const unsigned char*,size_t> zstreambuf::zippeek()
{
  TAP_CALL("src_leaves.zs_zippeek");
#ifdef CPP_RUST_IMPL_ZSTREAM
  std::pair<const unsigned char*,size_t> result(NULL, 0);
  const unsigned char *ptr = NULL;
  size_t len = 0;
  if (rust_ != NULL && ugrep_rust_zs_zippeek(rust_, &ptr, &len))
    result = std::pair<const unsigned char*,size_t>(ptr, len);
  zs_diagnostics(rust_, pathname_);
#else
  std::pair<const unsigned char*,size_t> result = zippeek_cpp();
#endif
  TAP_STATE(zs_tap_bytes(result.first, static_cast<std::streamsize>(result.second)));
  TAP_RET(static_cast<uint64_t>(result.second));
  return result;
}

// ---------------------------------------------------------------------------------------------------------------
// Legacy bodies of the unit zstream_codecs: zip and 7zip decompression state (ZipInfo) and its helpers
// ---------------------------------------------------------------------------------------------------------------

bool zstreambuf::ZipInfo::header()
{
#ifndef WITH_NO_7ZIP
  // if 7zip then get the next file name and info, start decompressing, return false if none
  if (viiz_strm_ != NULL)
  {
    // if info was already retrieved, then return
    if (!znew_)
      return true;

    int res = viiz_strm_->get(*this);
    if (res < 0)
    {
      cannot_decompress(pathname_, "corrupt 7zip archive");
      return false;
    }

#ifdef WITH_MAX_7ZIP_SIZE
    if (res == 0 && usize > WITH_MAX_7ZIP_SIZE)
    {
      cannot_decompress(pathname_, std::string("7zip archived file size exceeds the set limit: ").append(name).c_str());
      return znew_ = zend_ = true;
    }
#endif

    /* don't advance to the next file */
    znew_ = false;

    zend_ = res;
    return !zend_;
  }
#endif

  // are we at a new header? If not, do nothing and return true
  if (!znew_)
  {
    // try to read the descriptor, if present, before the next header
    if (!descriptor())
      return false;

    if (!znew_)
      return true;
  }

  // read the header data and check header magic
  const unsigned char *data = read_num(30);
  if (data == NULL || u32(data) != ZIP_HEADER_MAGIC)
    return false;

  // we're reading the zip local file header
  znew_ = false;

  uint16_t ziptime;
  uint16_t zipdate;

  // get the zip local file header info
  version = u16(data + 4);
  flag    = u16(data + 6);
  method  = static_cast<Compression>(u16(data + 8));
  ziptime = u16(data + 10);
  zipdate = u16(data + 12);
  crc     = u32(data + 14);
  size    = u32(data + 18);
  usize   = u32(data + 22);

  // convert zip time and date
  struct tm ziptm;
  memset(&ziptm, 0, sizeof(ziptm));
  ziptm.tm_sec   = 2 * (ziptime & 0x1f);
  ziptm.tm_min   = (ziptime >> 5) & 0x3f;
  ziptm.tm_hour  = ziptime >> 11;
  ziptm.tm_mday  = zipdate & 0x1f;
  ziptm.tm_mon   = (zipdate >> 5) & 0x3f;
  ziptm.tm_year  = 80 + (zipdate >> 9);
  ziptm.tm_isdst = -1;
  mtime = mktime(&ziptm);

  uint16_t namelen  = u16(data + 26);
  uint16_t extralen = u16(data + 28);

  // if zip data is encrypted, we give up
  if ((flag & 1) != 0)
  {
    cannot_decompress(pathname_, "zip data is encrypted");
    return false;
  }

  // read the file name
  data = read_num(namelen);
  if (data == NULL)
  {
    cannot_decompress(pathname_, "corrupt zip archive");
    return false;
  }

  name.assign(reinterpret_cast<const char*>(data), namelen);

  // read the extra field
  data = read_num(extralen);
  if (data == NULL)
  {
    cannot_decompress(pathname_, "corrupt zip archive");
    return false;
  }

  for (uint16_t num = 0; num < extralen; num += 4 + u16(data + num + 2))
  {
    uint16_t id = u16(data + num);

    if (id == 0x0001)
    {
      // Zip64 Extended Information Extra Field
      usize = u64(data + num + 4);
      size = u64(data + num + 12);
      // reset flag bit 3, we check this bit later
      flag &= ~8;
    }
    else if (id == 0x7075)
    {
      // Info-ZIP Unicode Path Extra Field
      size_t len = u16(data + num + 2);
      if (len >= 9 && num + len - 9 < extralen)
        name.assign(reinterpret_cast<const char*>(data + num + 9), len - 9);
    }
  }

  if (method == Compression::DEFLATE)
  {
    // Zip deflate method
    if (zlib_strm_ == NULL)
    {
      try
      {
        zlib_strm_ = new zlib_stream;
      }

      catch (const std::bad_alloc&)
      {
        cannot_decompress(pathname_, "out of memory");
        return false;
      }

      zlib_strm_->zalloc = Z_NULL;
      zlib_strm_->zfree  = Z_NULL;
      zlib_strm_->opaque = Z_NULL;
    }

    // prepare to inflate the remainder of the buffered data
    zlib_strm_->next_in   = zbuf_ + zcur_;
    zlib_strm_->avail_in  = static_cast<uInt>(zlen_ - zcur_);
    zlib_strm_->next_out  = Z_NULL;
    zlib_strm_->avail_out = 0;

    // initialize zlib inflate
    if (inflateInit2(zlib_strm_, -MAX_WBITS) != Z_OK)
    {
      cannot_decompress(pathname_, zlib_strm_->msg != NULL ? zlib_strm_->msg : "inflateInit2 failed");
      return false;
    }
  }
  else if (method == Compression::BZIP2)
  {
#ifdef HAVE_LIBBZ2

    // Zip bzip2 method
    if (bzip_strm_ == NULL)
    {
      try
      {
        bzip_strm_ = new bzip_stream;
      }

      catch (const std::bad_alloc&)
      {
        cannot_decompress(pathname_, "out of memory");
        return false;
      }

      bzip_strm_->bzalloc = NULL;
      bzip_strm_->bzfree  = NULL;
      bzip_strm_->opaque  = NULL;
    }

    // prepare to decompress the remainder of the buffered data
    bzip_strm_->next_in   = reinterpret_cast<char*>(zbuf_ + zcur_);
    bzip_strm_->avail_in  = static_cast<unsigned int>(zlen_ - zcur_);
    bzip_strm_->next_out  = NULL;
    bzip_strm_->avail_out = 0;

    // initialize bzip2 decompress
    if (BZ2_bzDecompressInit(bzip_strm_, 0, 0) != BZ_OK)
    {
      cannot_decompress(pathname_, "BZ2_bzDecompressInit failed");
      return false;
    }

#else

    cannot_decompress(pathname_, "unsupported zip compression method bzip2");
    return false;

#endif
  }
  else if (method == Compression::ZSTD)
  {
#ifdef HAVE_LIBZSTD

    // Zip zstd method
    if (zstd_strm_ == NULL)
    {
      zstd_strm_ = ZSTD_createDStream();

      if (zstd_strm_ == NULL)
      {
        cannot_decompress(pathname_, "out of memory");
        return false;
      }
    }
    else
    {
      // reinitialize zstd decompress
      ZSTD_initDStream(zstd_strm_);
    }

#else

    cannot_decompress(pathname_, "unsupported zip compression method zstd");
    return false;

#endif
  }
  else if (method == Compression::LZMA || method == Compression::XZ)
  {
#ifdef HAVE_LIBLZMA

    // TODO should we support bit 1 clear, i.e. use the size field to terminate the lzma stream?
    // if bit 1 is clear, then there is no EOS in the lzma stream that we require to terminate lzma decompression
    if (method == Compression::LZMA && (flag & 2) == 0)
    {
      cannot_decompress(pathname_, "unsupported zip compression method lzma without EOS");
      return false;
    }

    // Zip lzma method with bit 1 set (EOS in lzma)
    if (lzma_strm_ == NULL)
    {
      try
      {
        lzma_strm_ = new lzma_stream;
      }

      catch (const std::bad_alloc&)
      {
        cannot_decompress(pathname_, "out of memory");
        return false;
      }
    }

    *lzma_strm_ = LZMA_STREAM_INIT;

    // prepare to decompress the remainder of the buffered data
    lzma_strm_->next_in  = zbuf_ + zcur_;
    lzma_strm_->avail_in = zlen_ - zcur_;

    // initialize lzma decompress
    lzma_ret ret = lzma_auto_decoder(lzma_strm_, XZ::MEMLIMIT, LZMA_TELL_UNSUPPORTED_CHECK | LZMA_CONCATENATED);
    if (ret != LZMA_OK)
    {
      cannot_decompress(pathname_, "lzma_auto_decoder failed");
      return false;
    }

#else

    cannot_decompress(pathname_, "unsupported zip compression method lzma");
    return false;

#endif
  }
  else if (method == Compression::STORE)
  {
    if ((flag & 8) != 0)
    {
      cannot_decompress(pathname_, "zip STORE data size unknown");
      return false;
    }
  }
  else
  {
    std::string message("unsupported zip compression method ");
    message.append(std::to_string(static_cast<uint16_t>(method)));
    cannot_decompress(pathname_, message.c_str());
    return false;
  }

  // init crc32 to the complement of 0
  zcrc_ = 0xffffffff;

  zend_ = false;

  return true;
}

std::streamsize zstreambuf::ZipInfo::decompress(unsigned char *buf, size_t len)
{
  // if header() was not called or no more data to decompress, then return 0 to indicate EOF
  if (znew_ || zend_)
    return 0;

  std::streamsize num = 0;

#ifndef WITH_NO_7ZIP
  // 7zip decompression
  if (viiz_strm_ != NULL)
  {
    num = viiz_strm_->decompress(buf, len);

    if (num < static_cast<std::streamsize>(len))
      znew_ = zend_ = true;

    return num;
  }
#endif

  if (method == Compression::DEFLATE && zlib_strm_ != NULL)
  {
    while (true)
    {
      int ret = Z_OK;

      // decompress non-empty zbuf_[] into the given buf[]
      if (zlib_strm_->avail_in > 0 && zlib_strm_->avail_out == 0)
      {
        zlib_strm_->next_out  = buf;
        zlib_strm_->avail_out = static_cast<uInt>(len);

        ret = inflate(zlib_strm_, Z_NO_FLUSH);

        zcur_ = zlen_ - zlib_strm_->avail_in;

        if (ret != Z_OK && ret != Z_STREAM_END && ret != Z_BUF_ERROR)
        {
          cannot_decompress(pathname_, "a zlib decompression error was detected in the zip compressed data");
          zend_ = true;
          num = -1;
        }
        else
        {
          num = len - zlib_strm_->avail_out;
          zend_ = ret == Z_STREAM_END;
        }
      }

      // read compressed data into zbuf_[] and decompress zbuf_[] into the given buf[]
      if (ret == Z_OK && zlib_strm_->avail_in == 0 && num < static_cast<int>(len))
      {
        if (read())
        {
          zlib_strm_->next_in  = zbuf_;
          zlib_strm_->avail_in = static_cast<uInt>(zlen_);

          if (num == 0)
          {
            zlib_strm_->next_out  = buf;
            zlib_strm_->avail_out = static_cast<uInt>(len);
          }

          ret = inflate(zlib_strm_, Z_NO_FLUSH);

          zcur_ = zlen_ - zlib_strm_->avail_in;

          if (ret != Z_OK && ret != Z_STREAM_END && ret != Z_BUF_ERROR)
          {
            cannot_decompress(pathname_, "a zlib decompression error was detected in the zip compressed data");
            zend_ = true;
            num = -1;
          }
          else
          {
            num = len - zlib_strm_->avail_out;

            if (num == 0 && ret == Z_OK)
              continue;

            zend_ = ret == Z_STREAM_END;
          }
        }
        else
        {
          cannot_decompress(pathname_, "EOF detected in the zip compressed data");
          zend_ = true;
          num = -1;
        }
      }

      if (zend_)
      {
        ret = inflateEnd(zlib_strm_);
        if (ret != Z_OK)
        {
          cannot_decompress(pathname_, "a zlib decompression error was detected in the zip compressed data");
          num = -1;
        }
      }

      break;
    }
  }
#ifdef HAVE_LIBBZ2
  else if (method == Compression::BZIP2 && bzip_strm_ != NULL)
  {
    while (true)
    {
      int ret = BZ_OK;

      // decompress non-empty zbuf_[] into the given buf[]
      if (bzip_strm_->avail_in > 0 && bzip_strm_->avail_out == 0)
      {
        bzip_strm_->next_out  = reinterpret_cast<char*>(buf);
        bzip_strm_->avail_out = static_cast<unsigned int>(len);

        ret = BZ2_bzDecompress(bzip_strm_);

        zcur_ = zlen_ - bzip_strm_->avail_in;

        if (ret != BZ_OK && ret != BZ_STREAM_END)
        {
          cannot_decompress(pathname_, "a bzip2 decompression error was detected in the zip compressed data");
          zend_ = true;
          num = -1;
        }
        else
        {
          num = len - bzip_strm_->avail_out;
          zend_ = ret == BZ_STREAM_END;
        }
      }

      // read compressed data into zbuf_[] and decompress zbuf_[] into the given buf[]
      if (ret == BZ_OK && bzip_strm_->avail_in == 0 && num < static_cast<int>(len))
      {
        if (read())
        {
          bzip_strm_->next_in  = reinterpret_cast<char*>(zbuf_);
          bzip_strm_->avail_in = static_cast<unsigned int>(zlen_);

          if (num == 0)
          {
            bzip_strm_->next_out  = reinterpret_cast<char*>(buf);
            bzip_strm_->avail_out = static_cast<unsigned int>(len);
          }

          ret = BZ2_bzDecompress(bzip_strm_);

          zcur_ = zlen_ - bzip_strm_->avail_in;

          if (ret != BZ_OK && ret != BZ_STREAM_END)
          {
            cannot_decompress(pathname_, "a bzip2 decompression error was detected in the zip compressed data");
            zend_ = true;
            num = -1;
          }
          else
          {
            num = len - bzip_strm_->avail_out;

            if (num == 0 && ret == BZ_OK)
              continue;

            zend_ = ret == BZ_STREAM_END;
          }
        }
        else
        {
          cannot_decompress(pathname_, "EOF detected in the zip compressed data");
          zend_ = true;
          num = -1;
        }
      }

      if (zend_)
      {
        ret = BZ2_bzDecompressEnd(bzip_strm_);
        if (ret != BZ_OK)
        {
          cannot_decompress(pathname_, "a bzip2 decompression error was detected in the zip compressed data");
          num = -1;
        }
      }

      break;
    }
  }
#endif
#ifdef HAVE_LIBLZMA
  else if ((method == Compression::LZMA || method == Compression::XZ) && lzma_strm_ != NULL)
  {
    lzma_ret ret = LZMA_OK;

    // decompress non-empty zbuf_[] into the given buf[]
    if (lzma_strm_->avail_in > 0 && lzma_strm_->avail_out == 0)
    {
      lzma_strm_->next_out  = buf;
      lzma_strm_->avail_out = len;

      ret = lzma_code(lzma_strm_, LZMA_RUN);

      zcur_ = zlen_ - lzma_strm_->avail_in;

      if (ret != LZMA_OK && ret != LZMA_STREAM_END)
      {
        cannot_decompress(pathname_, "a lzma decompression error was detected in the zip compressed data");
        zend_ = true;
        num = -1;
      }
      else
      {
        num = len - lzma_strm_->avail_out;
        zend_ = ret == LZMA_STREAM_END;
      }
    }

    // read compressed data into zbuf_[] and decompress zbuf_[] into the given buf[]
    if (ret == LZMA_OK && lzma_strm_->avail_in == 0 && num < static_cast<std::streamsize>(len))
    {
      if (read())
      {
        lzma_strm_->next_in  = zbuf_;
        lzma_strm_->avail_in = zlen_;

        if (num == 0)
        {
          lzma_strm_->next_out  = buf;
          lzma_strm_->avail_out = len;
        }

        ret = lzma_code(lzma_strm_, LZMA_RUN);

        zcur_ = zlen_ - lzma_strm_->avail_in;

        if (ret != LZMA_OK && ret != LZMA_STREAM_END)
        {
          cannot_decompress(pathname_, "a lzma decompression error was detected in the zip compressed data");
          zend_ = true;
          num = -1;
        }
        else
        {
          num = len - lzma_strm_->avail_out;
          zend_ = ret == LZMA_STREAM_END;
        }
      }
      else
      {
        cannot_decompress(pathname_, "EOF detected in the zip compressed data");
        zend_ = true;
        num = -1;
      }
    }
    
    if (zend_)
      lzma_end(lzma_strm_);
  }
#endif
#ifdef HAVE_LIBZSTD
  else if (method == Compression::ZSTD && zstd_strm_ != NULL)
  {
    ZSTD_outBuffer out = { buf, len, 0 };
    size_t ret = 1;

    if (zcur_ < zlen_)
    {
      ZSTD_inBuffer in = { zbuf_, zlen_, zcur_ };
      ret = ZSTD_decompressStream(zstd_strm_, &out, &in);

      if (ZSTD_isError(ret))
      {
        cannot_decompress(pathname_, "a zstd decompression error was detected in the zip compressed data");
        zend_ = true;
        num = -1;
      }
      else
      {
        zcur_ = in.pos;
        num = static_cast<std::streamsize>(out.pos);
      }
    }

    if (ret != 0 && num < static_cast<std::streamsize>(len) && !zend_)
    {
      if (read())
      {
        ZSTD_inBuffer in = { zbuf_, zlen_, zcur_ };
        ret = ZSTD_decompressStream(zstd_strm_, &out, &in);

        if (ZSTD_isError(ret))
        {
          cannot_decompress(pathname_, "a zstd decompression error was detected in the zip compressed data");
          zend_ = true;
          num = -1;
        }
        else
        {
          zcur_ = in.pos;
          num = static_cast<std::streamsize>(out.pos);
        }
      }
      else
      {
        cannot_decompress(pathname_, "EOF detected in the zip compressed data");
        zend_ = true;
        num = -1;
      }
    }
  }
#endif
  else
  {
    // STORE method: copy the stored zip data until the stored data size is reduced to zero
    if (size > 0)
    {
      if (zcur_ < zlen_ || read())
      {
        num = zlen_ - zcur_;
        if (size < static_cast<uint64_t>(num))
          num = static_cast<std::streamsize>(size);

        memcpy(buf, zbuf_ + zcur_, static_cast<size_t>(num));

        zcur_ += static_cast<size_t>(num);
        size -= num;
      }
      else
      {
        cannot_decompress(pathname_, "EOF detected in the zip compressed data");
        zend_ = true;
        num = -1;
      }
    }

    if (size == 0)
      zend_ = true;
  }

#ifdef WITH_ZIP_CRC32
  // update zip crc32
  crc32(buf, static_cast<size_t>(num));
#endif

  return num;
}

bool zstreambuf::ZipInfo::descriptor()
{
  if (zend_)
  {
    // if we are at the end of the zip data and there is a descriptor, then read it
    if ((flag & 8) != 0)
    {
      // read data descriptor data and check descriptor magic
      const unsigned char *data = read_num(16);
      if (data == NULL)
        return false;

      if (u32(data) != ZIP_DESCRIPTOR_MAGIC)
      {
        cannot_decompress(pathname_, "an error was detected in the zip compressed data");
        return false;
      }

      crc   = u32(data + 4);
      size  = u32(data + 8);
      usize = u32(data + 12);
    }

    // we arrived at a new zip local file header
    znew_ = true;

#ifdef WITH_ZIP_CRC32
    // now that we have the crc32 value (from the header or the descriptor), check the integrity of the decompressed data
    if (~zcrc_ != crc)
    {
      cannot_decompress(pathname_, "a crc error was detected in the zip compressed data");
      return false;
    }
#endif
  }

  return true;
}

std::pair<const unsigned char*,size_t> zstreambuf::ZipInfo::peek()
{
  if (zcur_ > 0)
  {
    zlen_ -= zcur_;
    memmove(zbuf_, zbuf_ + zcur_, zlen_);
    zcur_ = 0;
    size_t ret = file_read(file_, zbuf_ + zlen_, ZIPBLOCK - zlen_);
    zlen_ += ret;
    if (zlib_strm_ != NULL)
    {
      zlib_strm_->next_in  = zbuf_;
      zlib_strm_->avail_in = static_cast<uInt>(zlen_);
    }
#ifdef HAVE_LIBBZ2
    else if (bzip_strm_ != NULL)
    {
      bzip_strm_->next_in  = reinterpret_cast<char*>(zbuf_);
      bzip_strm_->avail_in = static_cast<unsigned int>(zlen_);
    }
#endif
#ifdef HAVE_LIBLZMA
    else if (lzma_strm_ != NULL)
    {
      lzma_strm_->next_in  = zbuf_;
      lzma_strm_->avail_in = zlen_;
    }
#endif
#ifdef HAVE_LIBZSTD
    // no action is needed
#endif
  }

  return std::pair<const unsigned char*,size_t>(zbuf_, zlen_);
}

const unsigned char * zstreambuf::ZipInfo::read_num(size_t num)
{
  if (num > ZIPBLOCK)
    num = ZIPBLOCK;

  if (zlen_ - zcur_ >= num)
  {
    const unsigned char *ptr = zbuf_ + zcur_;
    zcur_ += num;
    return ptr;
  }

  zlen_ -= zcur_;
  memmove(zbuf_, zbuf_ + zcur_, zlen_);
  zcur_ = 0;
  size_t ret = file_read(file_, zbuf_ + zlen_, ZIPBLOCK - zlen_);
  zlen_ += ret;
  if (zlen_ >= num)
  {
    zcur_ = num;
    return zbuf_;
  }

  if (ferror(file_))
    warning("cannot read", pathname_);
  else
    cannot_decompress(pathname_, "an error was detected in the zip compressed data");
  return NULL;
}

bool zstreambuf::ZipInfo::read()
{
  zcur_ = 0;
  zlen_ = file_read(file_, zbuf_, ZIPBLOCK);
  return zlen_ > 0;
}

#ifdef WITH_ZIP_CRC32
void zstreambuf::ZipInfo::crc32(const unsigned char *buf, size_t len)
{
  while (len-- > 0)
  {
    zcrc_ ^= *buf++;
    for (int k = 0; k < 8; k++)
      zcrc_ = (zcrc_ >> 1) ^ (0xedb88320 & (0 - (zcrc_ & 1)));
  }
}
#endif

size_t zstreambuf::file_read(FILE *file, unsigned char *buf, size_t len)
{
  return fread(buf, 1, len, file);
}

bool zstreambuf::has_ext(const char *pathname, const char *extensions)
{
  const char *dot = strrchr(pathname, '.');
  if (dot == NULL)
    return false;
  const char *match = strstr(extensions, dot);
  if (match == NULL)
    return false;
  size_t end = strlen(dot);
  if (match + end > extensions + strlen(extensions))
    return false;
  return match[end] == '.' || match[end] == '\0';
}

// ---------------------------------------------------------------------------------------------------------------
// Legacy bodies of the unit zstream_core: format selection and open/close, stream dispatch and decoding loops
// ---------------------------------------------------------------------------------------------------------------

bool zstreambuf::is_bz_cpp(const char *pathname)
{
  return has_ext(pathname, ".bz.bz2.bzip2.tb2.tbz.tbz2.tz2");
}

bool zstreambuf::is_xz_cpp(const char *pathname)
{
  return has_ext(pathname, ".lzma.xz.tlz.txz");
}

bool zstreambuf::is_lz4_cpp(const char *pathname)
{
  return has_ext(pathname, ".lz4");
}

bool zstreambuf::is_zstd_cpp(const char *pathname)
{
  return has_ext(pathname, ".zst.zstd.tzst");
}

bool zstreambuf::is_br_cpp(const char *pathname)
{
  return has_ext(pathname, ".br");
}

bool zstreambuf::is_bz3_cpp(const char *pathname)
{
  return has_ext(pathname, ".bz3");
}

bool zstreambuf::is_Z_cpp(const char *pathname)
{
  return has_ext(pathname, ".Z.taZ.tZ");
}

bool zstreambuf::is_zip_cpp(const char *pathname)
{
  return has_ext(pathname, ".zip.zipx.ZIP");
}

bool zstreambuf::is_7z_cpp(const char *pathname)
{
  return has_ext(pathname, ".7z.7Z");
}

bool zstreambuf::is_rar_cpp(const char *pathname)
{
  return has_ext(pathname, ".rar.RAR");
}

void zstreambuf::open_cpp(const char *pathname, FILE *file)
{
  // close old stream, if still open
  close();

  if (file == NULL)
    return;

  pathname_ = pathname;
  file_ = file;

  cur_ = 0;
  len_ = 0;

  if (is_bz(pathname))
  {
#ifdef HAVE_LIBBZ2
    // open bzip/bzip2 compressed file
    try
    {
      bzfile_ = new BZ();
      int ret = BZ2_bzDecompressInit(&bzfile_->strm, 0, 0);
      if (ret != BZ_OK)
      {
        warning("BZ2_bzDecompressInit failed", pathname);

        delete bzfile_;
        bzfile_ = NULL;
        file_ = NULL;
      }
    }

    catch (const std::bad_alloc&)
    {
      cannot_decompress(pathname_, "out of memory");
      file_ = NULL;
    }
#else
    cannot_decompress(pathname, "unsupported compression format");
    file_ = NULL;
#endif
  }
  else if (is_xz(pathname))
  {
#ifdef HAVE_LIBLZMA
    // open xz/lzma compressed file
    try
    {
      xzfile_ = new XZ();
      lzma_ret ret = lzma_auto_decoder(&xzfile_->strm, XZ::MEMLIMIT, LZMA_TELL_UNSUPPORTED_CHECK | LZMA_CONCATENATED);
      if (ret != LZMA_OK)
      {
        warning("lzma_stream_decoder failed", pathname);

        delete xzfile_;
        xzfile_ = NULL;
        file_ = NULL;
      }
    }

    catch (const std::bad_alloc&)
    {
      cannot_decompress(pathname_, "out of memory");
      file_ = NULL;
    }
#else
    cannot_decompress(pathname, "unsupported compression format");
    file_ = NULL;
#endif
  }
  else if (is_lz4(pathname))
  {
#ifdef HAVE_LIBLZ4
    // open lz4 compressed file
    try
    {
      lz4file_ = new LZ4();
      if (lz4file_->strm == NULL || lz4file_->buf == NULL || lz4file_->zbuf == NULL)
      {
        warning("LZ4_createStreamDecode failed", pathname);

        delete lz4file_;
        lz4file_ = NULL;
        file_ = NULL;
      }
    }

    catch (const std::bad_alloc&)
    {
      cannot_decompress(pathname_, "out of memory");
      file_ = NULL;
    }
#else
    cannot_decompress(pathname, "unsupported compression format");
    file_ = NULL;
#endif
  }
  else if (is_zstd(pathname))
  {
#ifdef HAVE_LIBZSTD
    // open zstd compressed file
    try
    {
      zstdfile_ = new ZSTD();
      if (zstdfile_->strm == NULL || zstdfile_->zbuf == NULL)
      {
        warning("ZSTD_createDStream failed", pathname);

        delete zstdfile_;
        zstdfile_ = NULL;
        file_ = NULL;
      }
    }

    catch (const std::bad_alloc&)
    {
      cannot_decompress(pathname_, "out of memory");
      file_ = NULL;
    }
#else
    cannot_decompress(pathname, "unsupported compression format");
    file_ = NULL;
#endif
  }
  else if (is_br(pathname))
  {
#ifdef HAVE_LIBBROTLI
    // open brotli compressed file
    try
    {
      brfile_ = new BR();
      if (brfile_->strm == NULL)
      {
        warning("BrotliDecoderCreateInstance failed", pathname);

        delete brfile_;
        brfile_ = NULL;
        file_ = NULL;
      }
    }

    catch (const std::bad_alloc&)
    {
      cannot_decompress(pathname_, "out of memory");
      file_ = NULL;
    }
#else
    cannot_decompress(pathname, "unsupported compression format");
    file_ = NULL;
#endif
  }
  else if (is_bz3(pathname))
  {
#ifdef HAVE_LIBBZIP3
    // try to read the compression format magic bytes and block size
    if (file_read(file_, buf_, 9) < 9 || strncmp(reinterpret_cast<char*>(buf_), "BZ3v1", 5) != 0)
    {
      cannot_decompress(pathname_, "an error was detected in the bzip3 compressed data");
      delete bz3file_;
      bz3file_ = NULL;
      file_ = NULL;
    }

    uint32_t block_size = u32(buf_ + 5);

    if (block_size < 65 * 1024 || block_size > 511 * 1024 * 1024)
    {
      cannot_decompress(pathname_, "an error was detected in the bzip3 compressed data");
      delete bz3file_;
      bz3file_ = NULL;
      file_ = NULL;
    }

    // open bzip3 compressed file
    try
    {
      bz3file_ = new BZ3(block_size);
      if (bz3file_->strm == NULL || bz3file_->buf == NULL)
      {
        warning("bz3_new failed", pathname);

        delete bz3file_;
        bz3file_ = NULL;
        file_ = NULL;
      }
    }

    catch (const std::bad_alloc&)
    {
      cannot_decompress(pathname_, "out of memory");
      file_ = NULL;
    }
#else
    cannot_decompress(pathname, "unsupported compression format");
    file_ = NULL;
#endif
  }
  else if (is_7z(pathname))
  {
#ifndef WITH_NO_7ZIP
    // open 7zip compressed file
    try
    {
      zipinfo_ = new ZipInfo(pathname, file);
      zipinfo_->viiz_strm_ = new ZipInfo::viiz_stream(pathname, file);
    }

    catch (const std::invalid_argument&)
    {
      /* non-seekable and password-protected 7zip files cannot be decompressed */
      cannot_decompress(pathname, "7zip archive: encrypted data or 7z input is not seekable");
      file_ = NULL;
    }

    catch (const std::bad_alloc&)
    {
      if (zipinfo_ != NULL)
      {
        delete zipinfo_;
        zipinfo_ = NULL;
      }
      errno = ENOMEM;
      warning("out of memory", pathname);
      file_ = NULL;
    }
#else
    cannot_decompress(pathname, "unsupported compression format");
    file_ = NULL;
#endif
  }
  else if (is_rar(pathname))
  {
    // perhaps RAR can be supported sometime in the future?
    cannot_decompress(pathname, "unsupported compression format");
    file_ = NULL;
  }
  else
  {
    // try to read two compression format magic bytes
    size_t num = file_read(file_, buf_, 2);

    if (num == 2 && u16(buf_) == ZipInfo::DEFLATE_HEADER_MAGIC)
    {
      // open zlib compressed file
      try
      {
        zfile_ = new Z();

        // copy the gzip header's magic bytes to zbuf[], needed by inflate()
        zfile_->zbuf[0] = buf_[0];
        zfile_->zbuf[1] = buf_[1];
        zfile_->zlen = 2;
        zfile_->strm.next_in  = zfile_->zbuf;
        zfile_->strm.avail_in = static_cast<uInt>(zfile_->zlen);

        // inflate gzip compressed data starting with a gzip header
        if (inflateInit2(&zfile_->strm, 16 + MAX_WBITS) != Z_OK)
        {
          cannot_decompress(pathname_, zfile_->strm.msg != NULL ? zfile_->strm.msg : "inflateInit2 failed");

          delete zfile_;
          zfile_ = NULL;
          file_ = NULL;
        }
      }

      catch (const std::bad_alloc&)
      {
        errno = ENOMEM;
        warning("out of memory", pathname);
        file_ = NULL;
      }
    }
    else if (num == 2 && u16(buf_) == ZipInfo::COMPRESS_HEADER_MAGIC)
    {
      // open compress (Z) compressed file
      if ((zzfile_ = z_open(file, "r", 0, 1)) == NULL)
      {
        warning("zopen failed", pathname);
        file_ = NULL;
      }
    }
    else
    {
      // read up to four bytes of the compression format's magic bytes to check for zip
      num += file_read(file_, buf_ + num, 4 - num);

      if (num == 4 && u32(buf_) == ZipInfo::ZIP_HEADER_MAGIC)
      {
        // open zip compressed file
        try
        {
          zipinfo_ = new ZipInfo(pathname, file, buf_, 4);

          // read the zip header of the first compressed file, if none then end
          if (!zipinfo_->header())
          {
            delete zipinfo_;
            zipinfo_ = NULL;
            file_ = NULL;
          }
        }

        catch (const std::bad_alloc&)
        {
          errno = ENOMEM;
          warning("out of memory", pathname);
          file_ = NULL;
        }
      }
      else if (num == 4 && u32(buf_) == ZipInfo::ZIP_EMPTY_MAGIC)
      {
        // skip empty zip file without warning
        file_ = NULL;
      }
      else if (num == 4 && u32(buf_) == ZipInfo::ZIP_DESCRIPTOR_MAGIC)
      {
        // cannot decompress split zip files
        cannot_decompress(pathname, "spanned zip fragment of a split zip archive");
        file_ = NULL;
      }
      else
      {
#ifdef HAVE_LIBZSTD
        // compare 4 bytes to check for zstd
        if (num == 4 && u32(buf_) == ZipInfo::ZSTD_HEADER_MAGIC)
        {
          // open zstd compressed file and pass the first 4 bytes to its input buffer
          try
          {
            zstdfile_ = new ZSTD();
            if (zstdfile_->strm == NULL || zstdfile_->zbuf == NULL)
            {
              warning("ZSTD_createDStream failed", pathname);

              delete zstdfile_;
              zstdfile_ = NULL;
              file_ = NULL;
            }
            else
            {
              memcpy(zstdfile_->zbuf, buf_, 4);
              zstdfile_->zlen = 4;
            }
          }

          catch (const std::bad_alloc&)
          {
            cannot_decompress(pathname_, "out of memory");
            file_ = NULL;
          }
        }
        else
#endif
        {
#ifdef HAVE_LIBLZMA
          // read up to 6 bytes and compare to check for xz
          num += file_read(file_, buf_ + num, 6 - num);

          if (num == 6 && u32(buf_) == ZipInfo::XZ_HEADER_MAGIC_1 && u16(buf_ + 4) == ZipInfo::XZ_HEADER_MAGIC_2)
          {
            // open xz compressed file and pass the first 6 bytes to its input buffer
            try
            {
              xzfile_ = new XZ();
              xzfile_->strm.next_in  = buf_;
              xzfile_->strm.avail_in = 6;
              lzma_ret ret = lzma_auto_decoder(&xzfile_->strm, XZ::MEMLIMIT, LZMA_TELL_UNSUPPORTED_CHECK | LZMA_CONCATENATED);
              if (ret != LZMA_OK)
              {
                warning("lzma_stream_decoder failed", pathname);

                delete xzfile_;
                xzfile_ = NULL;
                file_ = NULL;
              }
            }

            catch (const std::bad_alloc&)
            {
              cannot_decompress(pathname_, "out of memory");
              file_ = NULL;
            }
          }
          else
#endif
          {
#ifdef HAVE_LIBBZ2
            // read up to 10 bytes and compare to check for bzip2
            num += file_read(file_, buf_ + num, 10 - num);

            if (num == 10 && memcmp(buf_, "BZh", 3) == 0 && isdigit(buf_[3]) && memcmp(buf_ + 4, "\x31\x41\x59\x26\x53\x59", 6) == 0)
            {
              // open bzip2 compressed file
              try
              {
                bzfile_ = new BZ();
                bzfile_->strm.next_in  = reinterpret_cast<char*>(buf_);
                bzfile_->strm.avail_in = 10;
                int ret = BZ2_bzDecompressInit(&bzfile_->strm, 0, 0);
                if (ret != BZ_OK)
                {
                  warning("BZ2_bzDecompressInit failed", pathname);

                  delete bzfile_;
                  bzfile_ = NULL;
                  file_ = NULL;
                }
              }

              catch (const std::bad_alloc&)
              {
                cannot_decompress(pathname_, "out of memory");
                file_ = NULL;
              }
            }
            else
#endif

            {
              // no compression: pass through
              num += file_read(file_, buf_ + num, Z_BUF_LEN - num);
              len_ = static_cast<std::streamsize>(num);
            }
          }
        }
      }
    }
  }
}

void zstreambuf::close_cpp()
{
  if (zfile_ != NULL)
  {
    // close zlib compressed file
    delete zfile_;
    zfile_ = NULL;
  }
  else if (zzfile_ != NULL)
  {
    // close compress (Z) compressed file
    z_close(zzfile_);
    zzfile_ = NULL;
  }
#ifdef HAVE_LIBBZ2
  else if (bzfile_ != NULL)
  {
    // close bzlib compressed file
    delete bzfile_;
    bzfile_ = NULL;
  }
#endif
#ifdef HAVE_LIBLZMA
  else if (xzfile_ != NULL)
  {
    // close lzma compressed file
    delete xzfile_;
    xzfile_ = NULL;
  }
#endif
#ifdef HAVE_LIBLZ4
  else if (lz4file_ != NULL)
  {
    // close lz4 compressed file
    delete lz4file_;
    lz4file_ = NULL;
  }
#endif
#ifdef HAVE_LIBZSTD
  else if (zstdfile_ != NULL)
  {
    // close zstd compressed file
    delete zstdfile_;
    zstdfile_ = NULL;
  }
#endif
#ifdef HAVE_LIBBROTLI
  else if (brfile_ != NULL)
  {
    // close brotli compressed file
    delete brfile_;
    brfile_ = NULL;
  }
#endif
#ifdef HAVE_LIBBZIP3
  else if (bz3file_ != NULL)
  {
    delete bz3file_;
    bz3file_ = NULL;
  }
#endif
  else if (zipinfo_ != NULL)
  {
    // close zip compressed file
    delete zipinfo_;
    zipinfo_ = NULL;
  }
}

bool zstreambuf::decompressing_cpp() const
{
  return
    zfile_    != NULL ||
    zzfile_   != NULL ||
    bzfile_   != NULL ||
    xzfile_   != NULL ||
    lz4file_  != NULL ||
    zstdfile_ != NULL ||
    brfile_   != NULL ||
    bz3file_  != NULL ||
    zipinfo_  != NULL;
}

const zstreambuf::ZipInfo * zstreambuf::zipinfo_cpp()
{
  if (zipinfo_ && !zipinfo_->header())
  {
    delete zipinfo_;
    zipinfo_ = NULL;
  }

  return zipinfo_;
}

std::pair<const unsigned char*,size_t> zstreambuf::zippeek_cpp()
{
  if (zipinfo_)
    return zipinfo_->peek();

  return std::pair<const unsigned char*,size_t>(NULL, 0);
}

std::streamsize zstreambuf::next_cpp(unsigned char *buf, size_t len)
{
  std::streamsize num = 0;

  if (zfile_ != NULL)
  {
    while (true)
    {
      int ret = Z_OK;

      // decompress non-empty zfile_->zbuf[] into the given buf[]
      if (zfile_->strm.avail_in > 0 && zfile_->strm.avail_out == 0)
      {
        zfile_->strm.next_out  = buf;
        zfile_->strm.avail_out = static_cast<uInt>(len);

        ret = inflate(&zfile_->strm, Z_NO_FLUSH);

        if (ret != Z_OK && ret != Z_STREAM_END && ret != Z_BUF_ERROR)
        {
          cannot_decompress(pathname_, "an error was detected in the gzip compressed data");
          num = -1;
        }
        else
        {
          num = len - zfile_->strm.avail_out;
        }
      }

      // read compressed data into zfile_->zbuf[] and decompress zfile_->zbuf[] into the given buf[]
      if (ret == Z_OK && zfile_->strm.avail_in == 0 && num < static_cast<std::streamsize>(len) && !zfile_->zend)
      {
        zfile_->zlen = file_read(file_, zfile_->zbuf, Z_BUF_LEN);

        if (ferror(file_))
        {
          warning("cannot read", pathname_);
          zfile_->zend = true;
          num = -1;
        }
        else
        {
          if (feof(file_))
            zfile_->zend = true;

          zfile_->strm.next_in  = zfile_->zbuf;
          zfile_->strm.avail_in = static_cast<uInt>(zfile_->zlen);

          if (num == 0)
          {
            zfile_->strm.next_out  = buf;
            zfile_->strm.avail_out = static_cast<uInt>(len);
          }

          ret = inflate(&zfile_->strm, Z_NO_FLUSH);

          if (ret != Z_OK && ret != Z_STREAM_END && ret != Z_BUF_ERROR)
          {
            cannot_decompress(pathname_, "an error was detected in the gzip compressed data");
            num = -1;
          }
          else
          {
            num = len - zfile_->strm.avail_out;

            if (num == 0 && ret == Z_OK)
              continue;
          }
        }
      }

      if (num >= 0 && ret == Z_STREAM_END)
      {
        // try to decompress the next concatenated gzip data
        if (inflateReset(&zfile_->strm) == Z_OK)
        {
          zfile_->strm.next_out  = Z_NULL;
          zfile_->strm.avail_out = 0;

          ret = Z_OK;

          if (num == 0)
            continue;
        }
      }

      // decompressed the last block or there was an error?
      if (num <= 0 || ret == Z_STREAM_END)
      {
        delete zfile_;
        zfile_ = NULL;
        file_ = NULL;
      }

      break;
    }
  }
  else if (zzfile_ != NULL)
  {
    // decompress a compress (Z) compressed block into the given buf[]
    num = z_read(zzfile_, buf, static_cast<int>(len)); 

    // an error occurred?
    if (num < 0)
      cannot_decompress(pathname_, "an error was detected in the compressed data");

    // decompressed the last block or there was an error?
    if (num <= 0)
    {
      z_close(zzfile_);
      zzfile_ = NULL;
      file_ = NULL;
    }
  }
#ifdef HAVE_LIBBZ2
  else if (bzfile_ != NULL)
  {
    while (true)
    {
      int ret = BZ_OK;

      // decompress non-empty bzfile_->zbuf[] into the given buf[]
      if (bzfile_->strm.avail_in > 0 && bzfile_->strm.avail_out == 0)
      {
        bzfile_->strm.next_out  = reinterpret_cast<char*>(buf);
        bzfile_->strm.avail_out = static_cast<unsigned int>(len);

        ret = BZ2_bzDecompress(&bzfile_->strm);

        if (ret != BZ_OK && ret != BZ_STREAM_END)
        {
          cannot_decompress(pathname_, "an error was detected in the bzip2 compressed data");
          num = -1;
        }
        else
        {
          num = len - bzfile_->strm.avail_out;
        }
      }

      // read compressed data into bzfile_->zbuf[] and decompress bzfile_->zbuf[] into the given buf[]
      if (ret == BZ_OK && bzfile_->strm.avail_in == 0 && num < static_cast<std::streamsize>(len) && !bzfile_->zend)
      {
        bzfile_->zlen = file_read(file_, bzfile_->zbuf, Z_BUF_LEN);

        if (ferror(file_))
        {
          warning("cannot read", pathname_);
          bzfile_->zend = true;
          num = -1;
        }
        else
        {
          if (feof(file_))
            bzfile_->zend = true;

          bzfile_->strm.next_in  = reinterpret_cast<char*>(bzfile_->zbuf);
          bzfile_->strm.avail_in = static_cast<unsigned int>(bzfile_->zlen);

          if (num == 0)
          {
            bzfile_->strm.next_out  = reinterpret_cast<char*>(buf);
            bzfile_->strm.avail_out = static_cast<unsigned int>(len);
          }

          ret = BZ2_bzDecompress(&bzfile_->strm);

          if (ret != BZ_OK && ret != BZ_STREAM_END)
          {
            cannot_decompress(pathname_, "an error was detected in the bzip2 compressed data");
            num = -1;
          }
          else
          {
            num = len - bzfile_->strm.avail_out;

            if (num == 0 && ret == BZ_OK)
              continue;
          }
        }
      }

      if (num >= 0 && ret == BZ_STREAM_END)
      {
        // try to decompress the next concatenated bzip2 data
        if (BZ2_bzDecompressEnd(&bzfile_->strm) == BZ_OK && BZ2_bzDecompressInit(&bzfile_->strm, 0, 0) == BZ_OK)
        {
          bzfile_->strm.next_out  = NULL;
          bzfile_->strm.avail_out = 0;

          ret = BZ_OK;

          if (num == 0)
            continue;
        }
      }

      // decompressed the last block or there was an error?
      if (num <= 0 || ret == BZ_STREAM_END)
      {
        delete bzfile_;
        bzfile_ = NULL;
        file_ = NULL;
      }

      break;
    }
  }
#endif
#ifdef HAVE_LIBLZMA
  else if (xzfile_ != NULL)
  {
    lzma_ret ret = LZMA_OK;

    // decompress non-empty xzfile_->zbuf[] into the given buf[]
    if (xzfile_->strm.avail_in > 0 && xzfile_->strm.avail_out == 0)
    {
      xzfile_->strm.next_out  = buf;
      xzfile_->strm.avail_out = len;

      ret = lzma_code(&xzfile_->strm, xzfile_->zend ? LZMA_FINISH : LZMA_RUN);

      if (ret != LZMA_OK && ret != LZMA_STREAM_END)
      {
        cannot_decompress(pathname_, "an error was detected in the xz/lzma compressed data");
        num = -1;
      }
      else
      {
        num = len - xzfile_->strm.avail_out;
      }
    }

    // read compressed data into xzfile_->zbuf[] and decompress xzfile_->zbuf[] into the given buf[]
    if (ret == LZMA_OK && xzfile_->strm.avail_in == 0 && num < static_cast<std::streamsize>(len) && !xzfile_->zend)
    {
      xzfile_->zlen = file_read(file_, xzfile_->zbuf, Z_BUF_LEN);

      if (ferror(file_))
      {
        warning("cannot read", pathname_);
        xzfile_->zend = true;
        num = -1;
      }
      else
      {
        if (feof(file_))
          xzfile_->zend = true;

        xzfile_->strm.next_in  = xzfile_->zbuf;
        xzfile_->strm.avail_in = xzfile_->zlen;

        if (num == 0)
        {
          xzfile_->strm.next_out  = buf;
          xzfile_->strm.avail_out = len;
        }

        ret = lzma_code(&xzfile_->strm, xzfile_->zend ? LZMA_FINISH : LZMA_RUN);

        if (ret != LZMA_OK && ret != LZMA_STREAM_END)
        {
          cannot_decompress(pathname_, "an error was detected in the xz/lzma compressed data");
          num = -1;
        }
        else
        {
          num = len - xzfile_->strm.avail_out;
        }
      }
    }

    // decompressed the last block or there was an error?
    if (num <= 0 || ret == LZMA_STREAM_END)
    {
      delete xzfile_;
      xzfile_ = NULL;
      file_ = NULL;
    }
  }
#endif
#ifdef HAVE_LIBLZ4
  else if (lz4file_ != NULL)
  {
    if (lz4file_->strm == NULL || lz4file_->buf == NULL || lz4file_->zbuf == NULL)
    {
      // LZ4 instantiation failed to allocate the required structures
      cannot_decompress(pathname_, "out of memory");
      delete lz4file_;
      lz4file_ = NULL;
      file_ = NULL;
      num = -1;
    }
    else if (lz4file_->loc < lz4file_->len)
    {
      // copy decompressed data from lz4file_->buf[] into the given buf[]
      if (lz4file_->loc + len > lz4file_->len)
        len = lz4file_->len - lz4file_->loc;
      memcpy(buf, lz4file_->buf + lz4file_->loc, len);
      lz4file_->loc += len;
      num = static_cast<std::streamsize>(len);
    }
    else
    {
      // loop over frames
      while (true)
      {
        // get next frame if we do not have a valid frame
        if (lz4file_->zflg == 0)
        {
          // move incomplete frame header to the start of lz4file_->zbuf[]
          lz4file_->zlen -= lz4file_->zloc;
          if (lz4file_->zlen > 0)
            memmove(lz4file_->zbuf, lz4file_->zbuf + lz4file_->zloc, lz4file_->zlen);
          lz4file_->zloc = 0;

          // frame header is 7 to 19 bytes long
          if (lz4file_->zlen < 19)
            lz4file_->zlen += file_read(file_, lz4file_->zbuf + lz4file_->zlen, Z_BUF_LEN);

          // if not enough frame data, error if not at EOF, otherwise EOF is OK
          if (lz4file_->zlen < 7)
          {
            if (lz4file_->zlen > 0 || ferror(file_))
            {
              warning("cannot read", pathname_);
              num = -1;
            }
            break;
          }

          // check magic bytes and flags
          uint32_t magic = u32(lz4file_->zbuf);
          if (magic >= 0x184D2A50 && magic <= 0x184D2A5F)
          {
            // skippable frame with 4 byte frame size
            if (lz4file_->zlen < 8)
            {
              warning("cannot read", pathname_);
              num = -1;
              break;
            }

            // skip this skippable frame, then continue with next frame
            size_t size = u32(lz4file_->zbuf + 4);
            lz4file_->zloc = 8;
            if (lz4file_->zloc + size > lz4file_->zlen)
            {
              size -= lz4file_->zlen - lz4file_->zloc;
              lz4file_->zloc = lz4file_->zlen;

              while (size > 0)
              {
                size_t ret = file_read(file_, lz4file_->zbuf, size < Z_BUF_LEN ? size : Z_BUF_LEN);
                if (ret == 0)
                  break;
                size -= ret;
              }

              if (size > 0)
              {
                warning("cannot read", pathname_);
                num = -1;
                break;
              }

              lz4file_->zloc = lz4file_->zlen = 0;
            }
            else
            {
              lz4file_->zloc += size;
            }
            continue;
          }
          lz4file_->zflg = lz4file_->zbuf[4];
          if (magic != 0x184D2204 || (lz4file_->zflg & 0xc0) != 0x40)
          {
            cannot_decompress(pathname_, "an error was detected in the lz4 compressed data");
            num = -1;
            break;
          }

          // get decompressed content size and dict, position to the start of the block
          lz4file_->zloc = 7;
          if ((lz4file_->zbuf[4] & 0x08))
          {
            lz4file_->size = u64(lz4file_->zbuf + lz4file_->zloc);
            lz4file_->zloc += 8;
          }
          if ((lz4file_->zbuf[4] & 0x01))
          {
            lz4file_->dict = u32(lz4file_->zbuf + lz4file_->zloc);
            lz4file_->zloc += 4;
          }

          // error if header was too short
          if (lz4file_->zlen < lz4file_->zloc)
          {
            lz4file_->zloc = lz4file_->zlen;
            cannot_decompress(pathname_, "an error was detected in the lz4 compressed data");
            num = -1;
            break;
          }
        }

        // move incomplete block data to the start of lz4file_->zbuf
        lz4file_->zlen -= lz4file_->zloc;
        if (lz4file_->zlen > 0)
          memmove(lz4file_->zbuf, lz4file_->zbuf + lz4file_->zloc, lz4file_->zlen);
        lz4file_->zloc = 0;

        // need 4 bytes with block size
        if (lz4file_->zlen < 4)
          lz4file_->zlen += file_read(file_, lz4file_->zbuf + lz4file_->zlen, Z_BUF_LEN);
        if (lz4file_->zlen < 4)
        {
          warning("cannot read", pathname_);
          num = -1;
          break;
        }

        // get the block size
        uint32_t size = u32(lz4file_->zbuf);
        lz4file_->zloc = 4;

        if (size == 0)
        {
          // reached the end marker, get footer with crc when present
          if ((lz4file_->zflg & 0x04))
          {
            lz4file_->crc = u32(lz4file_->zbuf +lz4file_->zloc);
            lz4file_->zloc += 4;
          }

          // reset flags to start a new frame and continue to read the frame
          lz4file_->zflg = 0;
          continue;
        }

        // if MSB(size) is set then data is uncompressed
        bool compressed = !(size & 0x80000000);
        if (!compressed)
          size &= 0x7fffffff;

        // block cannot be larger than the documented max block size of 4MB
        if (size > LZ4::MAX_BLOCK_SIZE)
        {
          cannot_decompress(pathname_, "an error was detected in the lz4 compressed data");
          num = -1;
          break;
        }

        // read the rest of the block, may overshoot by up to Z_BUF_LEN bytes
        while (lz4file_->zloc + size > lz4file_->zlen)
        {
          size_t ret = file_read(file_, lz4file_->zbuf + lz4file_->zlen, Z_BUF_LEN);
          if (ret == 0)
            break;
          lz4file_->zlen += ret;
        }
        if (lz4file_->zloc + size > lz4file_->zlen)
        {
          warning("cannot read", pathname_);
          num = -1;
          break;
        }

        if (compressed)
        {
          // decompress lz4file_->zbuf[] block into lz4file_->buf[]
          if (lz4file_->loc >= LZ4::RING_BUFFER_SIZE - LZ4::MAX_BLOCK_SIZE)
            lz4file_->loc = 0;
          int ret = LZ4_decompress_safe_continue(lz4file_->strm, reinterpret_cast<char*>(lz4file_->zbuf + lz4file_->zloc), reinterpret_cast<char*>(lz4file_->buf + lz4file_->loc), size, LZ4::MAX_BLOCK_SIZE);
          if (ret <= 0)
          {
            cannot_decompress(pathname_, "an error was detected in the lz4 compressed data");
            num = -1;
            break;
          }

          lz4file_->len = lz4file_->loc + ret;
        }
        else
        {
          // copy uncompressed data into lz4file_->buf[]
          memcpy(lz4file_->buf + lz4file_->loc, lz4file_->zbuf + lz4file_->zloc, size);
          lz4file_->len = lz4file_->loc + size;
        }

        // move over processed data
        lz4file_->zloc += size;

        // copy data from lz4file_->buf[] into the given buf[]
        if (lz4file_->loc + len > lz4file_->len)
          len = lz4file_->len - lz4file_->loc;
        memcpy(buf, lz4file_->buf + lz4file_->loc, len);
        lz4file_->loc += len;
        num = static_cast<std::streamsize>(len);

        if ((lz4file_->zflg & 0x10))
        {
          // need 4 bytes with block checksum
          if (lz4file_->zloc + 4 > lz4file_->zlen)
            lz4file_->zlen += file_read(file_, lz4file_->zbuf + lz4file_->zlen, Z_BUF_LEN);
          if (lz4file_->zloc + 4 > lz4file_->zlen)
          {
            warning("cannot read", pathname_);
            num = -1;
            break;
          }

          // get block checksum
          lz4file_->zcrc = u32(lz4file_->zbuf + lz4file_->zloc);
          lz4file_->zloc += 4;
        }

        if (num > 0)
          break;
      }
    }
  }
#endif
#ifdef HAVE_LIBZSTD
  else if (zstdfile_ != NULL)
  {
    if (zstdfile_->strm == NULL || zstdfile_->zbuf == NULL)
    {
      // ZSTD instantiation failed to allocate the required structures
      cannot_decompress(pathname_, "out of memory");
      delete zstdfile_;
      zstdfile_ = NULL;
      file_ = NULL;
      num = -1;
    }
    else
    {
      ZSTD_outBuffer out = { buf, len, 0 };

      // decompress zstdfile_->zbuf[] into buf[]
      if (zstdfile_->zloc < zstdfile_->zlen)
      {
        ZSTD_inBuffer in = { zstdfile_->zbuf, zstdfile_->zlen, zstdfile_->zloc };
        size_t ret = ZSTD_decompressStream(zstdfile_->strm, &out, &in);

        if (ZSTD_isError(ret))
        {
          cannot_decompress(pathname_, "an error was detected in the zstd compressed data");
          num = -1;
        }
        else
        {
          zstdfile_->zloc = in.pos;
          num = static_cast<std::streamsize>(out.pos);
        }
      }

      // read compressed data into zstdfile_->zbuf[] and decompress zstdfile_->zbuf[] into the given buf[]
      if (num >= 0 && num < static_cast<std::streamsize>(len) && zstdfile_->zloc >= zstdfile_->zlen && !zstdfile_->zend)
      {
        zstdfile_->zloc = 0;
        zstdfile_->zlen = file_read(file_, zstdfile_->zbuf, ZSTD_DStreamInSize());

        if (ferror(file_))
        {
          warning("cannot read", pathname_);
          zstdfile_->zend = true;
          num = -1;
        }
        else
        {
          if (feof(file_))
            zstdfile_->zend = true;
        }

        ZSTD_inBuffer in = { zstdfile_->zbuf, zstdfile_->zlen, zstdfile_->zloc };
        size_t ret = ZSTD_decompressStream(zstdfile_->strm, &out, &in);

        if (ZSTD_isError(ret))
        {
          cannot_decompress(pathname_, "an error was detected in the zstd compressed data");
          num = -1;
        }
        else
        {
          zstdfile_->zloc = in.pos;
          num = static_cast<std::streamsize>(out.pos);
        }
      }

      // decompressed the last block or there was an error?
      if (num <= 0)
      {
        delete zstdfile_;
        zstdfile_ = NULL;
        file_ = NULL;
      }
    }
  }
#endif
#ifdef HAVE_LIBBROTLI
  else if (brfile_ != NULL)
  {
    while (true)
    {
      BrotliDecoderResult ret = BROTLI_DECODER_RESULT_SUCCESS;

      // decompress non-empty brfile_->zbuf[] into the given buf[], strange we can't limit this to brfile_->avail_in > 0
      if (brfile_->avail_out == 0)
      {
        brfile_->next_out  = buf;
        brfile_->avail_out = len;

        ret = BrotliDecoderDecompressStream(brfile_->strm, &brfile_->avail_in, &brfile_->next_in, &brfile_->avail_out, &brfile_->next_out, NULL);

        if (ret == BROTLI_DECODER_RESULT_ERROR)
        {
          cannot_decompress(pathname_, "an error was detected in the brotli compressed data");
          num = -1;
        }
        else
        {
          num = len - brfile_->avail_out;
        }
      }

      // read compressed data into brfile_->zbuf[] and decompress brfile_->zbuf[] into the given buf[]
      if (ret != BROTLI_DECODER_RESULT_ERROR && brfile_->avail_in == 0 && num < static_cast<std::streamsize>(len) && !brfile_->zend)
      {
        brfile_->zlen = file_read(file_, brfile_->zbuf, Z_BUF_LEN);

        if (ferror(file_))
        {
          warning("cannot read", pathname_);
          brfile_->zend = true;
          num = -1;
        }
        else
        {
          if (feof(file_))
            brfile_->zend = true;

          brfile_->next_in  = brfile_->zbuf;
          brfile_->avail_in = brfile_->zlen;

          if (num == 0)
          {
            brfile_->next_out  = buf;
            brfile_->avail_out = len;
          }

          ret = BrotliDecoderDecompressStream(brfile_->strm, &brfile_->avail_in, &brfile_->next_in, &brfile_->avail_out, &brfile_->next_out, NULL);

          if (ret == BROTLI_DECODER_RESULT_ERROR)
          {
            cannot_decompress(pathname_, "an error was detected in the brotli compressed data");
            num = -1;
          }
          else
          {
            num = len - brfile_->avail_out;

            if (num == 0 && ret != BROTLI_DECODER_RESULT_ERROR)
              continue;
          }
        }
      }

      // decompressed the last block or there was an error?
      if (num <= 0)
      {
        delete brfile_;
        brfile_ = NULL;
        file_ = NULL;
      }

      break;
    }
  }
#endif
#ifdef HAVE_LIBBZIP3
  else if (bz3file_ != NULL)
  {
    while (bz3file_->loc >= bz3file_->len)
    {
      // read decompressed and compressed size
      if (file_read(file_, bz3file_->buf, 8) < 8)
      {
        if (ferror(file_))
        {
          warning("cannot read", pathname_);
          num = -1;
        }

        break;
      }

      // compressed ("new") size
      uint32_t block_size = u32(bz3file_->buf);

      // decompressed ("old") size
      bz3file_->len = u32(bz3file_->buf + 4);
      bz3file_->loc = 0;

      if (block_size > bz3file_->max ||
          bz3file_->len > bz3file_->max ||
          file_read(file_, bz3file_->buf, block_size) < block_size ||
          bz3_decode_block(bz3file_->strm, bz3file_->buf, bz3file_->max, block_size, bz3file_->len) < 0)
      {
        if (ferror(file_))
          warning("cannot read", pathname_);
        else
          cannot_decompress(pathname_, "an error was detected in the bzip3 compressed data");

        num = -1;

        break;
      }
    }

    // if all OK then copy the next part of the decompressed block into buf[]
    if (num != -1 && bz3file_->loc < bz3file_->len)
    {
      num = bz3file_->len - bz3file_->loc;
      if (num > static_cast<std::streamsize>(len))
        num = len;
      memcpy(buf, bz3file_->buf + bz3file_->loc, num);
      bz3file_->loc += num;
    }
    else if (num <= 0)
    {
      delete bz3file_;
      bz3file_ = NULL;
      file_ = NULL;
    }
  }
#endif
  else if (zipinfo_ != NULL)
  {
    // decompress a zip compressed block into the given buf[]
    num = zipinfo_->decompress(buf, len);

    // there was an error?
    if (num < 0)
    {
      delete zipinfo_;
      zipinfo_ = NULL;
      file_ = NULL;
    }
  }
  else if (file_ != NULL)
  {
    // pass through, without decompression
    num = file_read(file_, buf, len);

    // end of file or error?
    if (num == 0 || (num < static_cast<std::streamsize>(len) && feof(file_)))
      file_ = NULL;
  }

  return num;
}

#endif // HAVE_LIBZ
