// C++ side of the output seam: builds the plain-data snapshots of the retained C++
// objects (reflex::AbstractMatcher, Output state) for the cxx bridge of rust/src_core. See rust/src_core/SEAM.md,
// section "Seam: output". Included at the end of src/output.hpp; the Rust code is always linked (no build switch).
#ifndef OUTPUT_RUST_HPP
#define OUTPUT_RUST_HPP

#include "output.hpp"
#include "src_core/src/lib.rs.h"

#include <cstring>
#include <string>
#include <vector>

namespace output_rust {

inline rust::Slice<const uint8_t> slice(const char *s, size_t n)
{
  return rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t*>(s), n);
}

// a NULL pointer is the empty slice (the Rust functions are given has_* flags where NULL is distinct)
inline rust::Slice<const uint8_t> slice(const char *s)
{
  return s != NULL ? slice(s, strlen(s)) : slice("", 0);
}

inline rust::Slice<const uint8_t> slice(const std::string& s)
{
  return slice(s.data(), s.size());
}

inline rust::Vec<uint8_t> to_vec(const char *s, size_t n)
{
  rust::Vec<uint8_t> v;
  v.reserve(n);
  for (size_t i = 0; i < n; ++i)
    v.push_back(static_cast<uint8_t>(s[i]));
  return v;
}

// the bytes of the matcher's line, from bol() to the end of the line or of the match, whichever is later
struct Window {
  const char *data;
  size_t size;
};

// what a format string (or a header) reads from the matcher, so that the snapshot computes only that: eol() may refill
// and grow the buffer, bol(), columno(), lines() and the group walk cost a scan or an allocation per match
struct Need {
  bool eol = false;
  bool bol = false;
  bool columno = false;
  bool lines = false;
  bool fuzzy = false;
  bool groups = false;
  std::vector<size_t> indices; // group indices the format reads directly with (*matcher)[index]
};

// mirrors the parsing of the format strings and the conditions under which the Rust format code calls eol() and bol()
inline Need scan_format(const char *format, bool body_lines)
{
  Need need;
  need.lines = body_lines;
  const char *s = format;
  while (*s != '\0')
  {
    while (*s != '\0' && *s != '%')
      ++s;
    if (*s == '\0' || *(s + 1) == '\0')
      break;
    ++s;
    bool plus = false;
    int width = 0;
    bool has_arg = false;
    if (*s == '{')
    {
      char *r = NULL;
      plus = (s[1] == '+');
      width = strtol(s + 1, &r, 10);
      if (r != NULL && *r == '}')
        s = r + 1;
    }
    if (*s == '[')
    {
      has_arg = true;
      ++s;
      const char *arg = s;
      while (*s != '\0' && *s != ']')
        ++s;
      // every alternative that starts with a digit names a group index
      for (const char *a = arg; a < s; )
      {
        if (isdigit(static_cast<unsigned char>(*a)))
          need.indices.push_back(strtoul(a, NULL, 10));
        while (a < s && *a != '|')
          ++a;
        if (a < s)
          ++a;
      }
      if (*s == '\0' || *(s + 1) == '\0')
      {
        need.groups = true;
        break;
      }
      ++s;
    }
    int c = *s;
    // only the fields that read captures need the group walk (b d e o # q c v j x y G g 0-9)
    if (has_arg && c != '\0' && strchr("bdeo#qcvjxyGg0123456789", c) != NULL)
      need.groups = true;
    switch (c)
    {
      case 'K':
      case 'k':
        need.columno = true;
        break;
      case 'L':
      case 'l':
        need.lines = true;
        break;
      case 'Z':
        need.fuzzy = true;
        break;
      case 'G':
      case 'g':
      case '0':
      case '1':
      case '2':
      case '3':
      case '4':
      case '5':
      case '6':
      case '7':
      case '8':
      case '9':
        need.groups = true;
        if (c >= '0' && c <= '9')
          need.indices.push_back(static_cast<size_t>(c - '0'));
        break;
      case 'O':
      case 'Q':
        if (!flag_only_matching)
          need.eol = need.bol = true;
        else if (width < 0)
          need.bol = true;
        break;
      case 'C':
      case 'V':
      case 'J':
      case 'X':
      case 'Y':
        if (!flag_files_with_matches && !flag_count)
        {
          if (!flag_only_matching)
            need.eol = need.bol = true;
          else if (width < 0)
            need.bol = true;
        }
        break;
      case 'o':
      case '#':
      case 'q':
      case 'c':
      case 'v':
      case 'j':
      case 'x':
      case 'y':
        if (!has_arg)
        {
          if (plus && width > 0)
            need.eol = true;
          else if (width < 0)
            need.bol = true;
        }
        break;
      default:
        break;
    }
    ++s;
  }
  return need;
}

// snapshot of one match: offsets in the result are relative to the returned window, which starts at bol() if the
// format needs it (else begin()) and ends at max(eol(), end()) if the format needs the line (else end()); the matcher
// and its buffer are not changed (no text(), which writes a NUL, and eol() only when the format needs the line)
inline ugrep_rust::OutputMatch snapshot(reflex::AbstractMatcher *matcher, Window& window, const Need& need)
{
  ugrep_rust::OutputMatch m{};
  window.data = "";
  window.size = 0;
  if (matcher == NULL)
    return m;
  m.present = true;
  const char *l = NULL;
  if (need.eol)
  {
    l = matcher->eol(); // warning: must call eol() before bol(); it may move the buffer, so take pointers after it
    m.has_eol = true;
  }
  const char *begin = matcher->begin();
  const char *end = matcher->end();
  const char *b = begin;
  const char *e = end;
  if (l != NULL && l > e)
    e = l;
  if (need.bol)
  {
    b = matcher->bol();
    m.has_bol = true;
  }
  // the line bounds before the window is widened for captures
  const char *line_b = b;
  const char *line_e = e;
  // captures of every group the pattern defines, in group_id()/group_next_id() order (group 0 is the match itself)
  // a capture may start before begin() (PCRE2 lookbehind) or end after end(): widen the window to hold it
  std::vector<const char*> cap_first;
  if (need.groups)
  {
    std::pair<size_t,const char*> id = matcher->group_id();
    while (id.first != 0)
    {
      ugrep_rust::OutputCapture c{};
      c.index = id.first;
      c.has_name = id.second != NULL;
      if (id.second != NULL)
        c.name = to_vec(id.second, strlen(id.second));
      std::pair<const char*,size_t> cap = (*matcher)[id.first];
      c.found = cap.first != NULL;
      c.listed = true;
      if (c.found)
      {
        if (cap.first < b)
          b = cap.first;
        if (cap.first + cap.second > e)
          e = cap.first + cap.second;
      }
      c.size = cap.second;
      cap_first.push_back(cap.first);
      m.groups.push_back(std::move(c));
      id = matcher->group_next_id();
    }
    // indices the format reads directly work without the walk: (*matcher)[n] is the match for n == 0 or n == accept()
    for (size_t n : need.indices)
    {
      bool seen = false;
      for (const ugrep_rust::OutputCapture& g : m.groups)
        if (g.index == n)
          seen = true;
      if (seen)
        continue;
      ugrep_rust::OutputCapture c{};
      c.index = n;
      std::pair<const char*,size_t> cap = (*matcher)[n];
      c.found = cap.first != NULL;
      if (c.found)
      {
        if (cap.first < b)
          b = cap.first;
        if (cap.first + cap.second > e)
          e = cap.first + cap.second;
      }
      c.size = cap.second;
      cap_first.push_back(cap.first);
      m.groups.push_back(std::move(c));
    }
  }
  window.data = b;
  window.size = static_cast<size_t>(e - b);
  m.bol_off = static_cast<size_t>(line_b - b);
  m.eol_off = static_cast<size_t>(line_e - b);
  m.first = matcher->first();
  m.last = matcher->last();
  m.size = matcher->size();
  m.wsize = matcher->wsize();
  m.lineno = matcher->lineno();
  m.accept = matcher->accept();
  m.begin_off = static_cast<size_t>(begin - b);
  m.text_off = m.begin_off;
  m.end_off = static_cast<size_t>(end - b);
  for (size_t i = 0; i < cap_first.size(); ++i)
    if (cap_first[i] != NULL)
      m.groups[i].offset = static_cast<size_t>(cap_first[i] - b);
  if (need.columno)
    m.columno = matcher->columno();
  if (need.lines)
  {
    m.lines = matcher->lines();
    m.lineno_end = matcher->lineno_end();
  }
  if (need.fuzzy)
  {
    reflex::FuzzyMatcher *fuzzy = dynamic_cast<reflex::FuzzyMatcher*>(matcher);
    if (fuzzy != NULL)
    {
      m.has_fuzzy = true;
      m.edits = fuzzy->edits();
      m.distance = fuzzy->distance();
    }
  }
  return m;
}

inline void append(Output& out, const rust::Vec<uint8_t>& bytes)
{
  if (!bytes.empty())
    out.lines(reinterpret_cast<const char*>(bytes.data()), bytes.size());
}

// the --hyperlink prefix, host and path as three NUL-terminated strings in a row (see output_link in lib.rs)
inline std::string link()
{
  if (color_hl == NULL)
    return std::string();
  std::string s(flag_hyperlink_prefix);
  s.push_back('\0');
  s.append(flag_hyperlink_host);
  s.push_back('\0');
  s.append(flag_hyperlink_path);
  return s;
}

} // namespace output_rust

// Facade of the output seam (rust/src_core/SEAM.md, section "Seam: output"): the members of Output that call Rust, which
// produces the bytes; C++ appends them to the Rust-owned buffer. Defined here, inline, so that no object of its own is
// linked for Output (src/output.cpp is gone).

namespace output_rust {

inline ugrep_rust::OutputCtx ctx(size_t lineno, bool binary, bool dump_incomplete, const char *pathname)
{
  ugrep_rust::OutputCtx c;
  c.lineno = lineno;
  c.binary = binary;
  c.dump_incomplete = dump_incomplete;
  c.stdin_label = pathname != NULL && pathname == Static::LABEL_STANDARD_INPUT;
  return c;
}

} // namespace output_rust

inline void Output::Dump::line()
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

inline void Output::color(const char *arg)
{
  output_rust::append(*this, ugrep_rust::output_color(arg != NULL, output_rust::slice(arg)));
}

inline void Output::header(const char *pathname, const std::string& partname, bool& heading, size_t lineno, reflex::AbstractMatcher *matcher, size_t byte_offset, const char *sep, bool newline)
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
      output_rust::ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), pathname));
  if (r.complete_dump)
    dump.done();
  heading = r.heading;
  output_rust::append(*this, r.bytes);
}

inline void Output::header(const char *pathname, const std::string& partname)
{
  std::string link = output_rust::link();
  if (flag_tree)
  {
    // acquire lock on output and to access the global Tree state
    acquire();
    output_rust::append(*this, ugrep_rust::output_header_tree_state(
        output_rust::slice(pathname),
        output_rust::slice(partname),
        output_rust::slice(link),
        output_rust::ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), pathname)));
    return;
  }
  ugrep_rust::OutputHeaderResult r = ugrep_rust::output_header_file(
      output_rust::slice(pathname),
      output_rust::slice(partname),
      output_rust::slice(link),
      output_rust::ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), pathname));
  if (r.complete_dump)
    dump.done();
  output_rust::append(*this, r.bytes);
}

inline void Output::binary_file_matches(const char *pathname, const std::string& partname)
{
  std::string link = output_rust::link();
  ugrep_rust::OutputBinaryResult r = ugrep_rust::output_binary_file_matches(
      output_rust::slice(pathname),
      output_rust::slice(partname),
      output_rust::slice(link),
      output_rust::ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), pathname));
  output_rust::append(*this, r.bytes);
  if (r.set_binary)
    mode_ |= BINARY;
}

// format, format_invert and the quote/cpp/csv/hex/json/xml emitters: Rust returns the bytes, C++ appends them
inline void Output::format(const char *format, size_t matches)
{
  ugrep_rust::OutputCtx c = output_rust::ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), NULL);
  output_rust::append(*this, ugrep_rust::output_format_count(output_rust::slice(format), matches, c));
}

inline bool Output::format(const char *format, const char *pathname, const std::string& partname, size_t matches, size_t *matching, reflex::AbstractMatcher *matcher, bool& heading, bool body, bool next)
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
      output_rust::ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), pathname));
  heading = r.heading;
  if (matching != NULL)
    *matching = r.matching;
  if (r.lineno_set)
    lineno_ = r.lineno;
  output_rust::append(*this, r.bytes);
  return r.ret;
}

inline void Output::format_invert(const char *format, const char *pathname, const std::string& partname, size_t matches, size_t lineno, size_t offset, const char *ptr, size_t size, bool& heading, bool next)
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
      output_rust::ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), pathname));
  heading = r.heading;
  output_rust::append(*this, r.bytes);
}

inline void Output::quote(const char *data, size_t size) { output_rust::append(*this, ugrep_rust::output_quote(output_rust::slice(data, size))); }
inline void Output::cpp(const char *data, size_t size) { output_rust::append(*this, ugrep_rust::output_cpp(output_rust::slice(data, size))); }
inline void Output::csv(const char *data, size_t size) { output_rust::append(*this, ugrep_rust::output_csv(output_rust::slice(data, size))); }
inline void Output::hex(const char *data, size_t size) { output_rust::append(*this, ugrep_rust::output_hex(output_rust::slice(data, size))); }
inline void Output::json(const char *data, size_t size) { output_rust::append(*this, ugrep_rust::output_json(output_rust::slice(data, size))); }
inline void Output::xml(const char *data, size_t size) { output_rust::append(*this, ugrep_rust::output_xml(output_rust::slice(data, size))); }

#endif
