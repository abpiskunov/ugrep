// C++ side of the output seam (CPP_RUST_IMPL_OUTPUT=rust): builds the plain-data snapshots of the retained C++
// objects (reflex::AbstractMatcher, Output state) for the cxx bridge of rust/src_core. See rust/src_core/SEAM.md,
// section "Seam: output". Only included by src/output.cpp when CPP_RUST_IMPL_OUTPUT_RUST is defined.
#ifndef OUTPUT_RUST_HPP
#define OUTPUT_RUST_HPP

#include "output.hpp"
#include "src_core/src/lib.rs.h"

#include <cstring>
#include <string>

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
};

// mirrors the parsing of Output::format_cpp and the conditions under which its inline mat/quote/... overloads
// call eol() and bol()
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
      while (*s != '\0' && *s != ']')
        ++s;
      if (*s == '\0' || *(s + 1) == '\0')
      {
        need.groups = true;
        break;
      }
      ++s;
    }
    int c = *s;
    if (has_arg)
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
  window.data = b;
  window.size = static_cast<size_t>(e - b);
  m.first = matcher->first();
  m.last = matcher->last();
  m.size = matcher->size();
  m.wsize = matcher->wsize();
  m.lineno = matcher->lineno();
  m.accept = matcher->accept();
  m.begin_off = static_cast<size_t>(begin - b);
  m.text_off = m.begin_off;
  m.end_off = static_cast<size_t>(end - b);
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
  // captures of every group the pattern defines, in group_id()/group_next_id() order (group 0 is the match itself)
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
      if (c.found)
        c.offset = static_cast<size_t>(cap.first - begin);
      c.size = cap.second;
      m.groups.push_back(std::move(c));
      id = matcher->group_next_id();
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

#endif
