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

// snapshot of one match: offsets in the result are relative to the returned window
inline ugrep_rust::OutputMatch snapshot(reflex::AbstractMatcher *matcher, Window& window)
{
  ugrep_rust::OutputMatch m{};
  window.data = "";
  window.size = 0;
  if (matcher == NULL)
    return m;
  m.present = true;
  const char *e = matcher->eol(); // warning: must call eol() before bol()
  const char *b = matcher->bol();
  const char *end = matcher->end();
  if (end > e)
    e = end;
  window.data = b;
  window.size = static_cast<size_t>(e - b);
  m.first = matcher->first();
  m.last = matcher->last();
  m.size = matcher->size();
  m.wsize = matcher->wsize();
  m.lineno = matcher->lineno();
  m.lineno_end = matcher->lineno_end();
  m.columno = matcher->columno();
  m.lines = matcher->lines();
  m.accept = matcher->accept();
  m.begin_off = static_cast<size_t>(matcher->begin() - b);
  m.text_off = static_cast<size_t>(matcher->text() - b);
  m.end_off = static_cast<size_t>(end - b);
  reflex::FuzzyMatcher *fuzzy = dynamic_cast<reflex::FuzzyMatcher*>(matcher);
  if (fuzzy != NULL)
  {
    m.has_fuzzy = true;
    m.edits = fuzzy->edits();
    m.distance = fuzzy->distance();
  }
  // captures of every group the pattern defines, in group_id()/group_next_id() order (group 0 is the match itself)
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
      c.offset = static_cast<size_t>(cap.first - matcher->text());
    c.size = cap.second;
    m.groups.push_back(std::move(c));
    id = matcher->group_next_id();
  }
  return m;
}

inline void append(Output& out, const rust::Vec<uint8_t>& bytes)
{
  if (!bytes.empty())
    out.str(reinterpret_cast<const char*>(bytes.data()), bytes.size());
}

} // namespace output_rust

#endif
