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
@file      output.hpp
@brief     Output management
@author    Robert van Engelen - engelen@genivia.com
@copyright (c) 2019-2025, Robert van Engelen, Genivia Inc. All rights reserved.
@copyright (c) BSD-3 License - see LICENSE.txt
*/

#ifndef OUTPUT_HPP
#define OUTPUT_HPP

#include "ugrep.hpp"
#include <reflex/bits.h>
#include <reflex/utf8.h>
#include <reflex/matcher.h>
#include <reflex/fuzzymatcher.h>
#include <list>
#include <mutex>
#include <condition_variable>
#include <cstdint>

// the output buffer, hex dump state and --tree state of rust/src_core/src/output_buf.rs (C ABI, ffi/output_buf.rs)
// and the ordered/unordered output synchronization of rust/src_core/src/output_sync.rs (ffi/output_sync.rs)
extern "C" {
  void *ugrep_output_sync_new(bool ordered, size_t stop);
  void  ugrep_output_sync_free(void *sync);
  bool  ugrep_output_sync_cancelled(const void *sync);

  void *ugrep_output_new(void *regs, FILE *file, void *eof);
  void  ugrep_output_free(void *out);
  void  ugrep_output_sync_on(void *out, const void *sync);
  void  ugrep_output_begin(void *out, size_t slot);
  void  ugrep_output_acquire(void *out);
  void  ugrep_output_release(void *out);
  void  ugrep_output_end(void *out);
  void  ugrep_output_cancel(void *out);
  bool  ugrep_output_cancelled(void *out);
  void  ugrep_output_discard(void *out);
  void  ugrep_output_flush(void *out);
  void  ugrep_output_next(void *out);
  void  ugrep_output_try_flush(void *out);
  void  ugrep_output_dump_hex(void *out, short mode, size_t byte_offset, const char *data, size_t size);
  void  ugrep_output_dump_next(void *out, size_t byte_offset);
  bool  ugrep_output_dump_incomplete(void *out);
  void  ugrep_output_dump_complete(void *out, size_t off);
  void  ugrep_output_dump_done(void *out);
  void  ugrep_output_tree_reset();
  int   ugrep_output_tree_depth();
  const char *ugrep_output_tree_end();
  void  ugrep_output_tree_glyphs(const char *bar, const char *ptr, const char *end);
}

// max hexadecimal columns of bytes per line = 8*8
#ifndef MAX_HEX_COLUMNS
# define MAX_HEX_COLUMNS 64
#endif

// output buffering and synchronization
class Output {

 protected:

  static constexpr size_t STOP = UNDEFINED_SIZE; // if last == STOP, cancel output
  static constexpr size_t FLUSH   = 1;           // mode bit: flush each line of output
  static constexpr size_t HOLD    = 2;           // mode bit: hold output
  static constexpr size_t BINARY  = 4;           // mode bit: binary file found

 public:

  // sync state to synchronize output produced by multiple threads, UNORDERED or ORDERED by slot number (Rust)
  struct Sync {

    enum class Mode { UNORDERED, ORDERED };

    Sync(Mode mode)
      :
        next(0),
        handle(ugrep_output_sync_new(mode == Mode::ORDERED, STOP))
    { }

    ~Sync()
    {
      ugrep_output_sync_free(handle);
    }

    Sync(const Sync&) = delete;
    Sync& operator=(const Sync&) = delete;

    size_t next; // ORDERED: next slot assigned to thread
    void  *handle;

  };


  // hex dump state
  struct Dump {

    // hex dump mode for color highlighting
    static constexpr short HEX_MATCH         = 0;
    static constexpr short HEX_LINE          = 1;
    static constexpr short HEX_CONTEXT_MATCH = 2;
    static constexpr short HEX_CONTEXT_LINE  = 3;
    static constexpr short HEX_MAX           = 4;

    // constructor
    Dump(Output& out) : out(out) { }

    // dump matching data in hex, mode is
    void hex(short mode, size_t byte_offset, const char *data, size_t size)
    {
      ugrep_output_dump_hex(out.state_, mode, byte_offset, data, size);
    }

    // jump to the next hex dump location (option -o)
    inline void next(size_t byte_offset)
    {
      ugrep_output_dump_next(out.state_, byte_offset);
    }

    // hex line is incomplete: to complete invoke done()
    bool incomplete()
    {
      return ugrep_output_dump_incomplete(out.state_);
    }

    // if hex line is incomplete: complete it with done()
    inline void complete(size_t off)
    {
      ugrep_output_dump_complete(out.state_, off);
    }

    // done dumping hex
    inline void done()
    {
      ugrep_output_dump_done(out.state_);
    }

    Output& out; // reference to the output state of this hex dump state; the dump state itself is Rust's

  };

  // global directory tree state for output, protected by acquire(): the path, the depth and the drawing strings are Rust's
  struct Tree {

    // forget the directory path, depth 0
    static void reset()
    {
      ugrep_output_tree_reset();
    }

    // the directory depth
    static int depth()
    {
      return ugrep_output_tree_depth();
    }

    // the string to display a vertical line ending
    static const char *end()
    {
      return ugrep_output_tree_end();
    }

    // set the strings to display a vertical line, a vertical line and connector, and a vertical line ending
    static void glyphs(const char *bar, const char *ptr, const char *end)
    {
      ugrep_output_tree_glyphs(bar, ptr, end);
    }

  };

  // constructor
  Output(FILE *file)
    :
      file(file),
      eof(false),
      sync(NULL),
      dump(*this),
      lineno_(0),
      regs_{NULL, NULL, static_cast<size_t>(flag_line_buffered ? FLUSH : 0)},
      state_(NULL)
  {
    state_ = ugrep_output_new(&regs_, file, &eof);
  }

  // destructor: flushes the output
  ~Output()
  {
    ugrep_output_free(state_);
  }

  // output an 8-bit character c
  inline void chr(int c)
  {
    if (regs_.cur >= regs_.end)
      next();
    *regs_.cur++ = c;
  }

  // output a wide character c
  inline void wchr(int c)
  {
    char buf[8];
    size_t size = reflex::utf8(c, buf);
    str(buf, size);
  }

  // output a std::string s
  inline void str(const std::string& s)
  {
    str(s.c_str(), s.size());
  }

  // output a \0-terminated string s
  inline void str(const char *s)
  {
    if (s[0] != '\0')
    {
      if (s[1] != '\0')
        str(s, strlen(s));
      else
        chr(s[0]);
    }
  }

  // output a string s of byte length n
  inline void str(const char *s, size_t n)
  {
    while (regs_.cur + n >= regs_.end)
    {
      size_t k = static_cast<size_t>(regs_.end - regs_.cur);
      memcpy(regs_.cur, s, k);
      s += k;
      n -= k;
      regs_.cur += k;
      next();
    }
    memcpy(regs_.cur, s, n);
    regs_.cur += n;
  }

  // get pointer to position k (not beyond s+n) in a UTF-8 multibyte string s of byte length n
  inline const char * utf8pos(const char *s, size_t n, size_t k)
  {
    while (n-- > 0 && k-- > 0)
      while (static_cast<uint8_t>(*++s & 0xc0) == 0x80 && n > 0)
        --n;
    return s;
  }

  // get pointer to position -k (not before b) before a UTF-8 multibyte string s
  inline const char * utf8back(const char *b, const char *s, size_t k)
  {
    while (s > b && k-- > 0)
      while (s > b && static_cast<uint8_t>(*--s & 0xc0) == 0x80)
        continue;
    return s;
  }

  // get length of string s of byte length n cut to length k
  inline size_t utf8cut(const char *s, size_t n, size_t k)
  {
    return utf8pos(s, n, k) - s;
  }

  // output a UTF-8 multibyte string s of byte length n for up to k UTF-8-encoded characters
  inline void utf8strn(const char *s, size_t n, size_t k)
  {
    str(s, utf8cut(s, n, k));
  }

  // output a URI-encoded string s
  inline void uri(const std::string& s)
  {
    uri(s.c_str());
  }

  // output a URI-encoded string s
  inline void uri(const char *s)
  {
    while (*s != '\0')
    {
      if (*s >= 0x20 && *s <= 0x7e && *s != '%' && *s != ';')
      {
        chr(*s++);
      }
      else
      {
        chr('%');
        hex(static_cast<uint8_t>(*s++), 2);
      }
    }
  }

  // output an unsigned integer i with field width w (padded with spaces)
  inline void num(size_t i, int w = 0)
  {
    char tmp[24];
    int k = sizeof(tmp);

    do
      tmp[--k] = i % 10 + '0';
    while ((i /= 10) > 0);

    int n = sizeof(tmp) - k;

    w -= n;
    while (w-- > 0)
      chr(' ');

    str(&tmp[k], n);
  }

  // output a number in hex with width w (padded with digit '0')
  inline void hex(size_t i, int w = 0)
  {
    char tmp[16];
    int k = sizeof(tmp);

    do
      tmp[--k] = "0123456789abcdef"[i & 0xf];
    while ((i >>= 4) > 0);

    int n = sizeof(tmp) - k;

    w -= n;
    while (w-- > 0)
      chr('0');

    str(&tmp[k], n);
  }

  // output a single byte in octal
  inline void oct(int i)
  {
    uint8_t b = static_cast<uint8_t>(i);
    chr('0' + (b >> 6));
    chr('0' + ((b >> 3) & 7));
    chr('0' + (b & 7));
  }

  // open a hyperlink
  inline void open_hyperlink(const char *pathname, bool linkpos = false, size_t lineno = 0, size_t columno = 0)
  {
    str(color_hl);
    str(flag_hyperlink_prefix);
    str("://", 3);
    uri(flag_hyperlink_host);
    if (*pathname != '/')
    {
      chr('/');
      uri(flag_hyperlink_path);
      chr('/');
    }
    uri(pathname);
    if (linkpos)
    {
      chr(':');
      num(lineno);
      if (flag_column_number)
      {
        chr(':');
        num(columno);
      }
    }
    str(color_st);
  }

  // close the hyperlink
  inline void close_hyperlink()
  {
    str(color_hl);
    str(color_st);
  }

  // output a newline (platform-specific conditional "\r\n" or "\n"); flush if --line-buffered
  inline void nl(bool lf_only = false)
  {
    if (!lf_only)
#ifdef OS_WIN
      chr('\r');
#else
      { /* do not emit \r */ }
#endif
    chr('\n');
    check_flush();
  }

  // output bytes produced elsewhere (the Rust bodies of the output seam), flushing after each newline like nl() does
  inline void lines(const char *s, size_t n)
  {
    while (n > 0)
    {
      const char *eol = static_cast<const char*>(memchr(s, '\n', n));
      size_t k = eol != NULL ? static_cast<size_t>(eol - s) + 1 : n;
      str(s, k);
      if (eol != NULL)
        check_flush();
      s += k;
      n -= k;
    }
  }

  // enable line buffered mode to flush each line to output
  void set_flush()
  {
    regs_.mode |= FLUSH;
  }

  // try to flush if we do not have to wait to acquire the output lock
  inline void try_flush()
  {
    ugrep_output_try_flush(state_);
  }

  // flush if output is line buffered, unless we have to wait to acquire the output lock
  inline void check_flush()
  {
    if (regs_.mode == FLUSH)
      try_flush();
  }

  // hold the output and do not flush, buffer all output until further notice
  void hold()
  {
    regs_.mode |= HOLD;
  }

  // launch output when held back
  void launch()
  {
    if ((regs_.mode & HOLD) != 0)
    {
      regs_.mode &= ~HOLD;
      check_flush();
    }
  }

  // return true when holding the output
  bool holding()
  {
    return regs_.mode & HOLD;
  }

  // synchronize output by threads on the given sync object
  void sync_on(Sync *s)
  {
    sync = s;
    ugrep_output_sync_on(state_, s->handle);
  }

  // start synchronizing output for this slot in ORDERED mode (--sort)
  void begin(size_t slot)
  {
    ugrep_output_begin(state_, slot);
  }

  // acquire output synchronization lock
  void acquire()
  {
    if (sync != NULL)
      ugrep_output_acquire(state_);
  }

  // discard buffered output
  void discard()
  {
    ugrep_output_discard(state_);
  }

  // flush output and release sync slot, if one was assigned with sync_on()
  void release()
  {
    ugrep_output_release(state_);
  }

  // end output in ORDERED mode (--sort)
  void end()
  {
    if (sync != NULL)
      ugrep_output_end(state_);
  }

  // cancel output
  void cancel()
  {
    ugrep_output_cancel(state_);
  }

  // true if output was cancelled()
  bool cancelled()
  {
    return sync != NULL && ugrep_output_cancelled(state_);
  }

  // output color when set
  void color(const char *arg);

  // output the header part of the match, preceding the matched line
  void header(const char *pathname, const std::string& partname, bool& heading, size_t lineno, reflex::AbstractMatcher *matcher, size_t byte_offset, const char *sep, bool newline);

  // output the pathname header for --files_with_matches and --count
  void header(const char *pathname, const std::string& partname);

  // output "Binary file ... matches"
  void binary_file_matches(const char *pathname, const std::string& partname);

  // output format with option --format-begin and --format-end
  void format(const char *format, size_t matches);

  // output formatted match with options --format, --format-open, --format-close, returns false when nothing is output
  bool format(const char *format, const char *pathname, const std::string& partname, size_t matches, size_t *matching, reflex::AbstractMatcher *matcher, bool& heading, bool body, bool next);

  // output formatted inverted match with options -v --format, --format-open, --format-close
  void format_invert(const char *format, const char *pathname, const std::string& partname, size_t matches, size_t lineno, size_t offset, const char *ptr, size_t size, bool& heading, bool next);

  // output a quoted string with escapes for \ and "
  void quote(const char *data, size_t size);

  // output quoted string in C/C++
  void cpp(const char *data, size_t size);

  // output quoted string in CSV
  void csv(const char *data, size_t size);

  // output in hex
  void hex(const char *data, size_t size);

  // output quoted string in JSON
  void json(const char *data, size_t size);

  // output in XML
  void xml(const char *data, size_t size);

 protected:

  // the Rust side flushes the buffers, if not held back: acquire the lock, write the blocks to the file
  void flush()
  {
    ugrep_output_flush(state_);
  }

  // next buffer when the current block is full: flush, or allocate one if needed (when multi-threaded and lock is owned by another thread)
  void next()
  {
    ugrep_output_next(state_);
  }

 public:

  FILE            *file; // output stream
  std::atomic_bool eof;  // the other end closed or has an error
  Sync            *sync; // synchronization object
  Dump             dump; // hex dump state

 protected:

  // registers shared with Rust (rust/src_core/src/output_buf.rs, Regs, same layout): the write position and the end of the
  // current block, and the bitmask 1 if line buffered 2 if hold 4 if binary file found; the block memory is Rust's
  struct Regs {
    char   *cur;
    char   *end;
    size_t  mode;
  };

  size_t  lineno_; // last line number matched, when --format field %u (unique) is used
  Regs    regs_;   // write position in the current Rust-owned block, mode bits
  void   *state_;  // the Rust output state: blocks, sync lock, hex dump state, --width state

};

#endif
