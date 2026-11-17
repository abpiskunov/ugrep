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
#include <string>
#include <utility>
#include <zlib.h>

// buffer size to hold compressed data that is block-wise copied from compressed files
#ifndef Z_BUF_LEN
#define Z_BUF_LEN 65536
#endif

extern void cannot_decompress(const char *pathname, const char *message);
extern void warning(const char *message, const char *arg);

class zstreambuf : public std::streambuf {

 public:

  // zip/7zip entry info reported by the Rust stream, a plain view that decompresses nothing
  class ZipInfo {

   public:

    // zip compression methods, STORE and DEFLATE are common. others are less common and some are specific to WinZip (.zipx)
    enum class Compression : uint16_t { STORE = 0, DEFLATE = 8, BZIP2 = 12, LZMA = 14, ZSTD = 93, XZ = 95, /* PPMD = 98 not supported */ };

    ZipInfo()
      :
        version(0),
        flag(0),
        method(Compression::STORE),
        mtime(0),
        crc(0),
        size(0),
        usize(0)
    { }

    // no copy constructor
    ZipInfo(const ZipInfo&) = delete;

    uint16_t    version; // zip version (unused)
    uint16_t    flag;    // zip general purpose bit flag
    Compression method;  // zip compression method
    time_t      mtime;   // zip file modification time and date converted to local time
    uint32_t    crc;     // zip crc-32
    uint64_t    size;    // zip compressed file size, if known
    uint64_t    usize;   // zip uncompressed file size
    std::string name;    // zip file name extracted from local file header or zip extra field

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

  // default constructor
  zstreambuf()
    :
      pathname_(NULL),
      file_(NULL),
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

  const char     *pathname_;       // the pathname of the compressed file
  FILE           *file_;           // the compressed file
  ZipInfo        *zipinfo_;        // zip file and zip info handle
  void           *rust_;           // Rust-owned stream, see zstream.cpp
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
