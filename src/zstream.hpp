/******************************************************************************\
* Copyright (c) 2019, Robert van Engelen, Genivia Inc. All rights reserved.    *
*                                                                              *
* Redistribution and use in source and binary forms, with or without           *
* modification, are permitted provided that the following conditions are met:  *
*                                                                              *
*   (1) Redistributions of source code must retain the above copyright notice, *
*       this list of conditions and the following disclaimer.                  *
*                                                                              *
*   (2) Redistributions in binary form must reproduce the above copyright      *
*       notice, this list of conditions and the following disclaimer in the    *
*       documentation and/or other materials provided with the distribution.   *
*                                                                              *
*   (3) The name of the author may not be used to endorse or promote products  *
*       derived from this software without specific prior written permission.  *
*                                                                              *
* THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR IMPLIED *
* WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF         *
* MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO   *
* EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,       *
* SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, *
* PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;  *
* OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,     *
* WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR      *
* OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF       *
* ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.                                   *
\******************************************************************************/

/**
@file      zstream.hpp
@brief     file decompression streams - zstreambuf extends std::streambuf
@author    Robert van Engelen - engelen@genivia.com
@copyright (c) 2019-2025, Robert van Engelen, Genivia Inc. All rights reserved.
@copyright (c) BSD-3 License - see LICENSE.txt
*/

#ifndef ZSTREAM_HPP
#define ZSTREAM_HPP

#include <cstdio>
#include <cstdlib>
#include <cstdint>
#include <cstring>
#include <cctype>
#include <ctime>
#include <climits>
#include <exception>
#include <stdexcept>
#include <streambuf>
#include <zlib.h>

typedef z_stream zlib_stream;

// Z decompress z_open(), z_read(), z_close()
#include "zopen.h"

// if we have libbz2 (bzip2), otherwise declare incomplete bzip_stream for ZipInfo
#ifdef HAVE_LIBBZ2
#include <bzlib.h>
typedef bz_stream bzip_stream;
#else
struct bzip_stream;
#endif

// if we have liblzma (xz utils), otherwise declare incomplete lzma_stream for ZipInfo
#ifdef HAVE_LIBLZMA
#include <lzma.h>
#else
struct lzma_stream;
#endif

// if we have liblz4, otherwise declare incomplete lz4_stream for ZipInfo
#ifdef HAVE_LIBLZ4
#include <lz4.h>
typedef LZ4_streamDecode_t *lz4_stream;
// define LZ4_DECODER_RING_BUFFER_SIZE when not defined by old lz4
#ifndef LZ4_DECODER_RING_BUFFER_SIZE
#define LZ4_DECODER_RING_BUFFER_SIZE(maxBlockSize) (65536 + 14 + (maxBlockSize))
#endif
#else
struct lz4_stream;
#endif

// if we have libzstd, otherwise declare incomplete zstd_stream for ZipInfo
#ifdef HAVE_LIBZSTD
#include <zstd.h>
typedef ZSTD_DStream zstd_stream;
#else
struct zstd_stream;
#endif

// if we have libbrotlidec
#ifdef HAVE_LIBBROTLI
#include <brotli/decode.h>
#endif

// if we have libbzip3
#ifdef HAVE_LIBBZIP3
extern "C" {
#include <libbz3.h>
}
#endif

// use 7zip LZMA SDK using a viizip decompressor wrapper
#ifndef WITH_NO_7ZIP
#include "viizip.h"
// 7zip archive part pathname max length if limits.h doesn't define it
#ifndef PATH_MAX
#define PATH_MAX 4096
#endif
#endif

// zip decompression crc check disabled as this is too slow, we should optimize crc32() with a table
// use zip crc integrity check at the cost of a significant slow down?
// #define WITH_ZIP_CRC32

// buffer size to hold compressed data that is block-wise copied from compressed files
#ifndef Z_BUF_LEN
#define Z_BUF_LEN 65536
#endif

extern void cannot_decompress(const char *pathname, const char *message);
extern void warning(const char *message, const char *arg);

class zstreambuf : public std::streambuf {

 public:

  // zip decompression state and info
  class ZipInfo {

    friend class zstreambuf;

   public:

    // C++ wrapper for our C viizip 7zip decompressor
    struct viiz_stream {
#ifndef WITH_NO_7ZIP
      viiz_stream(const char *pathname, FILE *file)
      {
        viizip = viinew(file);

        /* non-seekable 7zip files cannot be decompressed */
        if (viizip == NULL)
          throw std::invalid_argument(pathname);
      }

      ~viiz_stream()
      {
        viifree(viizip);
      }

      // next 7zip file to decompress and get its ZipInfo, return 0 if OK, if none return 1 or -1 on error
      int get(ZipInfo& zipinfo)
      {
        char buf[PATH_MAX];
        int res = viiget(viizip, buf, PATH_MAX, &zipinfo.mtime, &zipinfo.usize);
        if (res)
          return res;

        zipinfo.name.assign(buf);

        return 0;
      }

      // read and decompress 7zip file data into buf[0..len-1], return number of bytes decompressed, 0 for EOF or -1 for error
      inline std::streamsize decompress(unsigned char *buf, size_t len)
      {
        return viidec(viizip, buf, len);
      }

      struct viizip *viizip;
#endif
    };

    // zip compression methods, STORE and DEFLATE are common. others are less common and some are specific to WinZip (.zipx)
    enum class Compression : uint16_t { STORE = 0, DEFLATE = 8, BZIP2 = 12, LZMA = 14, ZSTD = 93, XZ = 95, /* PPMD = 98 not supported */ };

    // constructor
    ZipInfo(const char *pathname, FILE *file, const unsigned char *buf = NULL, size_t len = 0)
      :
        version(0),
        flag(0),
        method(Compression::STORE),
        mtime(0),
        crc(0),
        size(0),
        usize(0),
        pathname_(pathname),
        file_(file),
        zlib_strm_(NULL),
        bzip_strm_(NULL),
        lzma_strm_(NULL),
        zstd_strm_(NULL),
        viiz_strm_(NULL),
        zcur_(0),
        zlen_(0),
        zcrc_(0xffffffff),
        znew_(true),
        zend_(false)
    {
      // copy initial buffer data into zbuf_[], when specified
      if (buf != NULL && len > 0)
      {
        zlen_ = (len < Z_BUF_LEN ? len : Z_BUF_LEN);
        memcpy(zbuf_, buf, zlen_);
      }
    }

    // no copy constructor
    ZipInfo(const ZipInfo&) = delete;

    // destructor
    ~ZipInfo()
    {
      if (zlib_strm_ != NULL)
        delete zlib_strm_;
#ifdef HAVE_LIBBZ2
      if (bzip_strm_ != NULL)
        delete bzip_strm_;
#endif
#ifdef HAVE_LIBLZMA
      if (lzma_strm_ != NULL)
        delete lzma_strm_;
#endif
#ifdef HAVE_LIBZSTD
      if (zstd_strm_ != NULL)
        ZSTD_freeDStream(zstd_strm_);
#endif
#ifndef WITH_NO_7ZIP
      if (viiz_strm_ != NULL)
        delete viiz_strm_;
#endif
    }

    uint16_t    version; // zip version (unused)
    uint16_t    flag;    // zip general purpose bit flag
    Compression method;  // zip compression method
    time_t      mtime;   // zip file modification time and date converted to local time
    uint32_t    crc;     // zip crc-32
    uint64_t    size;    // zip compressed file size, if known
    uint64_t    usize;   // zip uncompressed file size
    std::string name;    // zip file name extracted from local file header or zip extra field

   protected:

    static const size_t ZIPBLOCK = 65536; // block size to read zip data, at least 64K to fit long 64K pathnames

    static const uint16_t COMPRESS_HEADER_MAGIC = 0x9d1f; // compress header magic
    static const uint16_t DEFLATE_HEADER_MAGIC  = 0x8b1f; // zlib deflate header magic

    static const uint32_t ZIP_HEADER_MAGIC     = 0x04034b50; // zip local file header magic
    static const uint32_t ZIP_EMPTY_MAGIC      = 0x06054b50; // zip empty archive header magic
    static const uint32_t ZIP_DESCRIPTOR_MAGIC = 0x08074b50; // zip descriptor magic

    static const uint32_t ZSTD_HEADER_MAGIC = 0xfd2fb528; // zstd header magic

    static const uint32_t XZ_HEADER_MAGIC_1 = 0x587a37fd; // 4 bytes of xz header magic
    static const uint32_t XZ_HEADER_MAGIC_2 = 0x005a;     // next 2 bytes of xz header magic

    // read zip local file header if we are at a header, read the header, file name, and extra field
    bool header();

    // read and decompress zip file data into buf[0..len-1], return number of bytes decompressed, 0 for EOF or -1 for error
    std::streamsize decompress(unsigned char *buf, size_t len);

    // read zip data descriptor, if present, and check crc
    bool descriptor();
    
    // peek zip data block, return pointer to buffer and length of data available (max ZIPBLOCK when available)
    std::pair<const unsigned char*,size_t> peek();

    // read zip data block of num bytes, return pointer to buffer with at least num bytes or NULL when unsuccessful
    const unsigned char *read_num(size_t num);

    // read zip data into buffer zbuf_[], should be called when zcur_ >= zlen_
    bool read();

#ifdef WITH_ZIP_CRC32
    // update zip crc32
    void crc32(const unsigned char *buf, size_t len);
#endif

    const char   *pathname_;       // the pathname of the zip file
    FILE         *file_;           // the input file pointer
    zlib_stream  *zlib_strm_;      // zlib stream handle
    bzip_stream  *bzip_strm_;      // bzip/bzip2 stream handle
    lzma_stream  *lzma_strm_;      // xz/lzma stream handle
    zstd_stream  *zstd_strm_;      // zstd stream handle
    viiz_stream  *viiz_strm_;      // 7zip stream handle
    unsigned char zbuf_[ZIPBLOCK]; // buffer with compressed zip file data to decompress
    size_t        zcur_;           // current position in the zbuf_[] buffer, less or equal to zlen_
    size_t        zlen_;           // length of the compressed data in the zbuf_[] buffer
    uint32_t      zcrc_;           // crc32 of the decompressed data
    bool          znew_;           // true when reached a zip local file header
    bool          zend_;           // true when reached the end of compressed data, a descriptor and/or header follows

  };

  // return true if pathname has a (tar) bzlib2 filename extension
  static bool is_bz(const char *pathname);

  // return true if pathname has a (tar) xz/lzma filename extension
  static bool is_xz(const char *pathname);

  // return true if pathname has a lz4 filename extension
  static bool is_lz4(const char *pathname);

  // return true if pathname has a (tar) zstd filename extension
  static bool is_zstd(const char *pathname);

  // return true if pathname has a br filename extension
  static bool is_br(const char *pathname);

  // return true if pathname has a bz3 filename extension
  static bool is_bz3(const char *pathname);

  // return true if pathname has a (tar) compress (Z) filename extension
  static bool is_Z(const char *pathname);

  // return true if pathname has a zip filename extension
  static bool is_zip(const char *pathname);

  // return true if pathname has a 7z filename extension
  static bool is_7z(const char *pathname);

  // return true if pathname has a RAR filename extension
  static bool is_rar(const char *pathname);

  // check if pathname extension matches on of the given filename extensions
  static bool has_ext(const char *pathname, const char *extensions);

  // default constructor
  zstreambuf()
    :
      pathname_(NULL),
      file_(NULL),
      zfile_(NULL),
      zzfile_(NULL),
      bzfile_(NULL),
      xzfile_(NULL),
      lz4file_(NULL),
      zstdfile_(NULL),
      brfile_(NULL),
      bz3file_(NULL),
      zipinfo_(NULL),
      rust_(NULL),
      cur_(0),
      len_(0)
  { }

  // constructor
  zstreambuf(const char *pathname, FILE *file)
    :
      pathname_(pathname),
      file_(file),
      zfile_(NULL),
      zzfile_(NULL),
      bzfile_(NULL),
      xzfile_(NULL),
      lz4file_(NULL),
      zstdfile_(NULL),
      brfile_(NULL),
      bz3file_(NULL),
      zipinfo_(NULL),
      rust_(NULL),
      cur_(0),
      len_(0)
  {
    open(pathname, file);
  }

  // no copy constructor
  zstreambuf(const zstreambuf&) = delete;

  // destructor
  virtual ~zstreambuf()
  {
    close();
  }

  // open the decompression stream
  void open(const char *pathname, FILE *file);

  // close the decompression stream
  void close();

  // return true if decompressing a file
  bool decompressing() const;

  // copy or decompress a block of data into buf[0..len-1], return number of bytes decompressed, zero on EOF or negative on error
  std::streamsize decompress(unsigned char *buf, size_t len)
  {
    if (cur_ >= len_)
      return next(buf, len);

    size_t num = static_cast<size_t>(len_ - cur_);
    if (num > len)
      num = len;

    // move decompressed data to destination buf, which can be the same as the source buffer after get_buffer()
    if (buf != buf_ + cur_)
      memmove(buf, buf_ + cur_, num);

    cur_ += num;

    return static_cast<std::streamsize>(num);
  }

  // get pointer to the internal buffer and its max size
  void get_buffer(unsigned char *& buffer, size_t& maxlen)
  {
    buffer = buf_;
    maxlen = Z_BUF_LEN;
  }

  // return zip info when unzipping a file, if at end then proceed to the next file to unzip, otherwise return NULL, 
  const ZipInfo *zipinfo();

  // return pointer and length to current data when unzipping a file, NULL otherwise
  std::pair<const unsigned char*,size_t> zippeek();

 protected:

  // legacy C++ bodies of the seam functions above, selected by the facade in zstream.cpp
  static bool is_bz_cpp(const char *pathname);
  static bool is_xz_cpp(const char *pathname);
  static bool is_lz4_cpp(const char *pathname);
  static bool is_zstd_cpp(const char *pathname);
  static bool is_br_cpp(const char *pathname);
  static bool is_bz3_cpp(const char *pathname);
  static bool is_Z_cpp(const char *pathname);
  static bool is_zip_cpp(const char *pathname);
  static bool is_7z_cpp(const char *pathname);
  static bool is_rar_cpp(const char *pathname);
  void open_cpp(const char *pathname, FILE *file);
  void close_cpp();
  bool decompressing_cpp() const;
  std::streamsize next_cpp(unsigned char *buf, size_t len);
  const ZipInfo *zipinfo_cpp();
  std::pair<const unsigned char*,size_t> zippeek_cpp();

  // zlib decompression state data
  struct Z {

    Z()
      :
        zlen(0),
        zend(false)
    {
      strm.zalloc    = Z_NULL;
      strm.zfree     = Z_NULL;
      strm.opaque    = Z_NULL;
      strm.next_in   = Z_NULL;
      strm.avail_in  = 0;
      strm.next_out  = Z_NULL;
      strm.avail_out = 0;
      strm.msg       = NULL;
    }

    ~Z()
    {
      inflateEnd(&strm);
    }

    zlib_stream   strm;
    unsigned char zbuf[Z_BUF_LEN];
    size_t        zlen;
    bool          zend;

  };
  
  // zlib file handle
  typedef struct Z *zFile;

  // compress (Z) file handle
  typedef void *zzFile;

#ifdef HAVE_LIBBZ2

  // bzip/bzip2 decompression state data
  struct BZ {

    BZ()
      :
        zlen(0),
        zend(false)
    {
      strm.bzalloc   = NULL;
      strm.bzfree    = NULL;
      strm.opaque    = NULL;
      strm.next_in   = NULL;
      strm.avail_in  = 0;
      strm.next_out  = NULL;
      strm.avail_out = 0;
    }

    ~BZ()
    {
      BZ2_bzDecompressEnd(&strm);
    }

    bzip_stream   strm;
    unsigned char zbuf[Z_BUF_LEN];
    size_t        zlen;
    bool          zend;

  };
  
  // bzip2 file handle
  typedef struct BZ *bzFile;

#else

  // unused bzip2 file handle
  typedef void *bzFile;

#endif

#ifdef HAVE_LIBLZMA

  // lzma and xz decompression state data
  struct XZ {

    XZ()
      :
        strm(LZMA_STREAM_INIT),
        zlen(0),
        zend(false)
    { }

    ~XZ()
    {
      lzma_end(&strm);
    }

    static const uint64_t MEMLIMIT = 100 * 1024 * 1024; // 100 MB: files compressed with 'xz -9' require 65 MB to decompress

    lzma_stream   strm;
    unsigned char zbuf[Z_BUF_LEN];
    size_t        zlen;
    bool          zend;

  };
  
  // lzma/xz file handle
  typedef struct XZ *xzFile;

#else

  // unused lzma/xz file handle
  typedef void *xzFile;

#endif

#ifdef HAVE_LIBLZ4

  // lz4 decompression state data
  struct LZ4 {

    static const size_t MAX_BLOCK_SIZE   = 4194304; // lz4 4MB max block size
    static const size_t RING_BUFFER_SIZE = LZ4_DECODER_RING_BUFFER_SIZE(MAX_BLOCK_SIZE);

    LZ4()
      :
        strm(LZ4_createStreamDecode()),
        buf(static_cast<unsigned char*>(malloc(RING_BUFFER_SIZE))),
        loc(0),
        len(0),
        crc(0),
        size(0),
        dict(0),
        zbuf(static_cast<unsigned char*>(malloc(LZ4_COMPRESSBOUND(MAX_BLOCK_SIZE) + Z_BUF_LEN + 8))),
        zflg(0),
        zloc(0),
        zlen(0),
        zcrc(0)
    {
      if (buf == NULL || zbuf == NULL)
        throw std::bad_alloc();
    }

    ~LZ4()
    {
      if (zbuf != NULL)
        free(zbuf);
      if (buf != NULL)
        free(buf);
      if (strm != NULL)
        LZ4_freeStreamDecode(strm);
    }

    lz4_stream     strm; // lz4 decompression stream state
    unsigned char *buf;  // decompressed data buffer
    size_t         loc;
    size_t         len;
    uint32_t       crc;  // decompressed data xxHash-32 - optional, unused
    uint64_t       size; // decompressed size - optional, unused
    uint32_t       dict; // dict id - optional, unused
    unsigned char *zbuf; // compressed data buffer with extra room for 64K input buffer + b.size (4 bytes) + b.checksum (4 bytes)
    unsigned char  zflg; // frame header flags
    size_t         zloc;
    size_t         zlen;
    uint32_t       zcrc; // comoressed block xxHash-32 - optional, unused

  };
  
  // lz4 file handle
  typedef struct LZ4 *lz4File;

#else

  // unused lz4 file handle
  typedef void *lz4File;

#endif

#ifdef HAVE_LIBZSTD

  // zstd decompression state data
  struct ZSTD {

    ZSTD()
      :
        strm(ZSTD_createDStream()),
        zbuf(static_cast<unsigned char*>(malloc(ZSTD_DStreamInSize()))),
        zloc(0),
        zlen(0),
        zend(false)
    {
      if (zbuf == NULL)
        throw std::bad_alloc();
    }

    ~ZSTD()
    {
      if (zbuf != NULL)
        free(zbuf);
      if (strm != NULL)
        ZSTD_freeDStream(strm);
    }

    ZSTD_DStream  *strm; // zstd decompression stream state
    unsigned char *zbuf; // compressed data buffer
    size_t         zloc;
    size_t         zlen;
    bool           zend;

  };

  // zstd file handle
  typedef struct ZSTD *zstdFile;

#else

  // unused zstd file handle
  typedef void *zstdFile;

#endif

#ifdef HAVE_LIBBROTLI

  // brotli decompression state data
  struct BR {

    BR()
      :
        strm(BrotliDecoderCreateInstance(NULL, NULL, NULL)),
        next_in(NULL),
        avail_in(0),
        next_out(NULL),
        avail_out(0),
        zlen(0),
        zend(false)
    { }

    ~BR()
    {
      if (strm != NULL)
        BrotliDecoderDestroyInstance(strm);
    }

    BrotliDecoderState *strm; // brotli decompression stream state
    const uint8_t      *next_in;
    size_t              avail_in;
    uint8_t            *next_out;
    size_t              avail_out;
    unsigned char       zbuf[Z_BUF_LEN];
    size_t              zlen;
    bool                zend;

  };

  // brotli file handle
  typedef struct BR *brotliFile;

#else

  // unused brotli file handle
  typedef void *brotliFile;

#endif

#ifdef HAVE_LIBBZIP3

  // bzip3 decompression state data
  struct BZ3 {

    BZ3(uint32_t block_size)
      :
        strm(bz3_new(block_size)),
        max(bz3_bound(block_size)),
        buf(static_cast<uint8_t*>(malloc(max))),
        loc(0),
        len(0)
    {
      if (buf == NULL)
        throw std::bad_alloc();
    }

    ~BZ3()
    {
      if (buf != NULL)
        free(buf);
      if (strm != NULL)
        bz3_free(strm);
    }

    bz3_state *strm;
    uint32_t   max;
    uint8_t   *buf;
    uint32_t   loc;
    uint32_t   len;

  };
  
  // bzip3 file handle
  typedef struct BZ3 *bz3File;

#else

  // unused bzip3 file handle
  typedef void *bz3File;

#endif

  // read from file into buffer, return len when successful or less when EOF reached or error
  static size_t file_read(FILE *file, unsigned char *buf, size_t len);

  // fetch and decompress the next block of data into buf[0..len-1], return number of bytes decompressed, zero on EOF or negative on error
  std::streamsize next(unsigned char *buf, size_t len);

  // read a decompressed block into buf_[], returns pending next character or EOF
  int_type peek()
  {
    cur_ = 0;
    len_ = next(buf_, Z_BUF_LEN);
    return len_ > 0 ? traits_type::to_int_type(*buf_) : traits_type::eof();
  }

  // read a decompressed block into buf_[], reads and returns next character or EOF
  int_type read()
  {
    int_type c = peek();
    if (c != traits_type::eof())
      ++cur_;
    return c;
  }

  // std::streambuf::underflow()
  int_type underflow() override
  {
    return cur_ < len_ ? buf_[cur_] : peek();
  }

  // std::streambuf::uflow()
  int_type uflow() override
  {
    return cur_ < len_ ? buf_[cur_++] : read();
  }

  // std::streambuf::showmanyc()
  std::streamsize showmanyc() override
  {
    return file_ == NULL && cur_ >= len_ ? -1 : 0;
  }

  // std::streambuf::xsgetn(s, n)
  std::streamsize xsgetn(char *s, std::streamsize n) override
  {
    std::streamsize k = n;
    while (k > 0)
    {
      if (cur_ >= len_)
        if (peek() == traits_type::eof())
          return n - k;
      if (k <= len_ - cur_)
      {
        memcpy(s, buf_ + cur_, static_cast<size_t>(k));
        cur_ += k;
        return n;
      }
      memcpy(s, buf_ + cur_, static_cast<size_t>(len_ - cur_));
      s += len_ - cur_;
      k -= len_ - cur_;
      cur_ = len_;
    }
    return n;
  }

  // convert 2 bytes in little endian format to 16 bit
  static uint16_t u16(const unsigned char *buf)
  {
    return buf[0] + (static_cast<uint16_t>(buf[1]) << 8);
  }

  // convert 4 bytes in little endian format to 32 bit
  static uint32_t u32(const unsigned char *buf)
  {
    return u16(buf) + (static_cast<uint32_t>(u16(buf + 2)) << 16);
  }

  // convert 8 bytes in little endian format to to 64 bit
  static uint64_t u64(const unsigned char *buf)
  {
    return u32(buf) + (static_cast<uint64_t>(u32(buf + 4)) << 32);
  }

  const char     *pathname_;       // the pathname of the compressed file
  FILE           *file_;           // the compressed file
  zFile           zfile_;          // zlib file handle
  zzFile          zzfile_;         // compress (Z) file handle
  bzFile          bzfile_;         // bzip/bzip2 file handle
  xzFile          xzfile_;         // xz/lzma file handle
  lz4File         lz4file_;        // lz4 file handle
  zstdFile        zstdfile_;       // zstd file handle
  brotliFile      brfile_;         // brotli file handle
  bz3File         bz3file_;        // bzip3 file handle
  ZipInfo        *zipinfo_;        // zip file and zip info handle
  void           *rust_;           // Rust-owned stream, only used when CPP_RUST_IMPL_ZSTREAM is defined (see zstream.cpp)
  unsigned char   buf_[Z_BUF_LEN]; // buffer with decompressed stream data
  std::streamsize cur_;            // current position in buffer to read the stream data, less or equal to len_
  std::streamsize len_;            // length of decompressed data in the buffer

};

// The MSVC projects compile src/zstream.cpp as a separate file and define ZSTREAM_FACADE. Builds that only compile the
// including file (autotools, src/Makefile.am) get the same definitions from zstream.cpp, included once here.
#ifndef ZSTREAM_FACADE
#include "zstream.cpp"
#endif

#endif
