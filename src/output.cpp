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
@file      output.cpp
@brief     Output management
@author    Robert van Engelen - engelen@genivia.com
@copyright (c) 2019-2025, Robert van Engelen, Genivia Inc. All rights reserved.
@copyright (c) BSD-3 License - see LICENSE.txt
*/

#include "output.hpp"

#include "output_rust.hpp"

// dump matching data in hex
void Output::Dump::hex(short mode, size_t byte_offset, const char *data, size_t size)
{
  offset = byte_offset;
  while (size > 0)
  {
    bytes[offset++ % flag_hex_columns] = (mode << 8) | *reinterpret_cast<const unsigned char*>(data++);
    if (offset % flag_hex_columns == 0)
      line();
    --size;
  }
}

const char *Output::Dump::color_hex[4] = { match_ms, color_sl, match_mc, color_cx };

const char *Output::Tree::bar = "|  ";
const char *Output::Tree::ptr = "|_ ";
const char *Output::Tree::end = "~  ";

std::string Output::Tree::path;
int Output::Tree::depth;


// Facade of the output seam (rust/src_core/SEAM.md, section Seam: output): the ported members call Rust, which
// produces the bytes while C++ keeps the buffer.


namespace {

inline ugrep_rust::OutputCtx output_ctx(size_t lineno, bool binary, bool dump_incomplete, const char *pathname)
{
  ugrep_rust::OutputCtx ctx;
  ctx.lineno = lineno;
  ctx.binary = binary;
  ctx.dump_incomplete = dump_incomplete;
  ctx.stdin_label = pathname != NULL && pathname == Static::LABEL_STANDARD_INPUT;
  return ctx;
}

}

void Output::Dump::line()
{
  ugrep_rust::OutputDumpLine r = ugrep_rust::output_dump_line(
      rust::Slice<const int16_t>(bytes, MAX_HEX_COLUMNS),
      rust::Slice<const int16_t>(prevb, MAX_HEX_COLUMNS),
      pstar,
      offset);
  output_rust::append(out, r.bytes);
  pstar = r.pstar;
  if (!r.repeated)
    for (int i = 0; i < MAX_HEX_COLUMNS; ++i)
      prevb[i] = bytes[i];
  for (int i = 0; i < MAX_HEX_COLUMNS; ++i)
    bytes[i] = -1;
}

void Output::color(const char *arg)
{
  output_rust::append(*this, ugrep_rust::output_color(arg != NULL, output_rust::slice(arg)));
}

void Output::header(const char *pathname, const std::string& partname, bool& heading, size_t lineno, reflex::AbstractMatcher *matcher, size_t byte_offset, const char *sep, bool newline)
{
  output_rust::Window window;
  output_rust::Need need;
  need.columno = flag_column_number;
  ugrep_rust::OutputMatch m = output_rust::snapshot(matcher, window, need);
  std::string link = output_rust::link();
  ugrep_rust::OutputArgs args{};
  args.heading = heading;
  args.lineno = lineno;
  args.byte_offset = byte_offset;
  args.has_separator = sep != NULL;
  args.newline = newline;
  ugrep_rust::OutputHeaderResult r = ugrep_rust::output_header_line(
      output_rust::slice(pathname),
      output_rust::slice(partname),
      output_rust::slice(sep),
      output_rust::slice(link),
      args,
      m,
      output_ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), pathname));
  if (r.complete_dump)
    dump.done();
  heading = r.heading;
  output_rust::append(*this, r.bytes);
}

void Output::header(const char *pathname, const std::string& partname)
{
  std::string link = output_rust::link();
  if (flag_tree)
  {
    // acquire lock on output and to access global Tree::path and Tree::depth
    acquire();
    std::string glyphs(Tree::bar);
    glyphs.push_back('\0');
    glyphs.append(Tree::ptr);
    glyphs.push_back('\0');
    glyphs.append(Tree::end);
    ugrep_rust::OutputTreeResult t = ugrep_rust::output_header_tree(
        output_rust::slice(pathname),
        output_rust::slice(partname),
        output_rust::slice(link),
        output_rust::slice(glyphs),
        output_rust::slice(Tree::path),
        Tree::depth,
        output_ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), pathname));
    Tree::path.assign(reinterpret_cast<const char*>(t.path.data()), t.path.size());
    Tree::depth = t.depth;
    output_rust::append(*this, t.bytes);
    return;
  }
  ugrep_rust::OutputHeaderResult r = ugrep_rust::output_header_file(
      output_rust::slice(pathname),
      output_rust::slice(partname),
      output_rust::slice(link),
      output_ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), pathname));
  if (r.complete_dump)
    dump.done();
  output_rust::append(*this, r.bytes);
}

void Output::binary_file_matches(const char *pathname, const std::string& partname)
{
  std::string link = output_rust::link();
  ugrep_rust::OutputBinaryResult r = ugrep_rust::output_binary_file_matches(
      output_rust::slice(pathname),
      output_rust::slice(partname),
      output_rust::slice(link),
      output_ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), pathname));
  output_rust::append(*this, r.bytes);
  if (r.set_binary)
    mode_ |= BINARY;
}

// format, format_invert and the quote/cpp/csv/hex/json/xml emitters: Rust returns the bytes, C++ appends them
void Output::format(const char *format, size_t matches)
{
  ugrep_rust::OutputCtx ctx = output_ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), NULL);
  output_rust::append(*this, ugrep_rust::output_format_count(output_rust::slice(format), matches, ctx));
}

bool Output::format(const char *format, const char *pathname, const std::string& partname, size_t matches, size_t *matching, reflex::AbstractMatcher *matcher, bool& heading, bool body, bool next)
{
  output_rust::Window window;
  output_rust::Need need = output_rust::scan_format(format, body);
  ugrep_rust::OutputMatch m = output_rust::snapshot(matcher, window, need);
  ugrep_rust::OutputArgs args{};
  args.matches = matches;
  args.has_matching = matching != NULL;
  args.matching = matching != NULL ? *matching : 0;
  args.heading = heading;
  args.body = body;
  args.next = next;
  ugrep_rust::OutputFormatResult r = ugrep_rust::output_format_match(
      output_rust::slice(format),
      output_rust::slice(pathname),
      output_rust::slice(partname),
      output_rust::slice(window.data, window.size),
      args,
      m,
      output_ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), pathname));
  heading = r.heading;
  if (matching != NULL)
    *matching = r.matching;
  if (r.lineno_set)
    lineno_ = r.lineno;
  output_rust::append(*this, r.bytes);
  return r.ret;
}

void Output::format_invert(const char *format, const char *pathname, const std::string& partname, size_t matches, size_t lineno, size_t offset, const char *ptr, size_t size, bool& heading, bool next)
{
  ugrep_rust::OutputArgs args{};
  args.matches = matches;
  args.lineno = lineno;
  args.offset = offset;
  args.heading = heading;
  args.next = next;
  ugrep_rust::OutputFormatResult r = ugrep_rust::output_format_invert(
      output_rust::slice(format),
      output_rust::slice(pathname),
      output_rust::slice(partname),
      output_rust::slice(ptr, size),
      args,
      output_ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), pathname));
  heading = r.heading;
  output_rust::append(*this, r.bytes);
}

void Output::quote(const char *data, size_t size) { output_rust::append(*this, ugrep_rust::output_quote(output_rust::slice(data, size))); }
void Output::cpp(const char *data, size_t size) { output_rust::append(*this, ugrep_rust::output_cpp(output_rust::slice(data, size))); }
void Output::csv(const char *data, size_t size) { output_rust::append(*this, ugrep_rust::output_csv(output_rust::slice(data, size))); }
void Output::hex(const char *data, size_t size) { output_rust::append(*this, ugrep_rust::output_hex(output_rust::slice(data, size))); }
void Output::json(const char *data, size_t size) { output_rust::append(*this, ugrep_rust::output_json(output_rust::slice(data, size))); }
void Output::xml(const char *data, size_t size) { output_rust::append(*this, ugrep_rust::output_xml(output_rust::slice(data, size))); }

// flush a block of data as truncated lines limited to --width columns: the Rust body returns the bytes to write
bool Output::flush_truncated_lines(const char *data, size_t size)
{
  ugrep_rust::OutputTruncResult r = ugrep_rust::output_truncate_lines(
      output_rust::slice(data, size),
      skip_,
      cols_,
      static_cast<uint8_t>(ansi_));
  skip_ = r.skip;
  cols_ = r.cols;
  ansi_ = static_cast<ANSI>(r.ansi);
  return !r.bytes.empty() && fwrite(r.bytes.data(), 1, r.bytes.size(), file) < r.bytes.size();
}
