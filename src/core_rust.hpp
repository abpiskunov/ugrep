// C++ side of the src_core seam: conversions between the C++ types of src/cnf.hpp, src/stats.hpp and the flag_*
// globals, and the cxx bridge types of rust/src_core/src/lib.rs. Included only when CPP_RUST_IMPL_SRC_CORE is
// defined (msvc/rust.targets, /p:CPP_RUST_IMPL_SRC_CORE=rust). See rust/src_core/SEAM.md.
//
// The flag_* globals stay owned by C++: every crossing takes a read-only snapshot (cnf_flags, stats_input) and
// Rust never writes a flag; what a function writes comes back in its result.
#ifndef CORE_RUST_HPP
#define CORE_RUST_HPP

#ifdef CPP_RUST_IMPL_SRC_CORE

#include "ugrep.hpp"
#include "cnf.hpp"
#include "stats.hpp"
#include "src_core/src/lib.rs.h"

#include <cstdio>
#include <string>
#include <vector>

namespace core_rust {

inline rust::Vec<uint8_t> to_vec(const char *s, size_t n)
{
  rust::Vec<uint8_t> v;
  v.reserve(n);
  for (size_t i = 0; i < n; ++i)
    v.push_back(static_cast<uint8_t>(s[i]));
  return v;
}

inline rust::Vec<uint8_t> to_vec(const std::string& s)
{
  return to_vec(s.data(), s.size());
}

inline rust::Slice<const uint8_t> to_slice(const char *s)
{
  return rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t*>(s), strlen(s));
}

inline rust::Slice<const uint8_t> to_slice(const std::string& s)
{
  return rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t*>(s.data()), s.size());
}

inline std::string to_string(const rust::Vec<uint8_t>& v)
{
  return std::string(reinterpret_cast<const char*>(v.data()), v.size());
}

template <class Container>
inline rust::Vec<ugrep_rust::Bytes> to_bytes_vec(const Container& v)
{
  rust::Vec<ugrep_rust::Bytes> r;
  r.reserve(v.size());
  for (const auto& s : v)
    r.push_back(ugrep_rust::Bytes{to_vec(s)});
  return r;
}

// the CNF terms copied into the bridge type
inline rust::Vec<ugrep_rust::CnfTerm> to_terms(const CNF::Terms& terms)
{
  rust::Vec<ugrep_rust::CnfTerm> r;
  r.reserve(terms.size());
  for (const auto& term : terms)
  {
    ugrep_rust::CnfTerm t;
    for (const auto& pattern : term)
    {
      if (pattern)
        t.patterns.push_back(ugrep_rust::CnfPattern{true, to_vec(*pattern)});
      else
        t.patterns.push_back(ugrep_rust::CnfPattern{false, rust::Vec<uint8_t>()});
    }
    r.push_back(std::move(t));
  }
  return r;
}

// the bridge terms copied back into CNF terms (replacing the old contents)
inline void from_terms(CNF::Terms& terms, const rust::Vec<ugrep_rust::CnfTerm>& v)
{
  CNF::Terms result;
  for (const auto& t : v)
  {
    result.emplace_back();
    for (const auto& p : t.patterns)
    {
      if (p.defined)
        result.back().emplace_back(new std::string(to_string(p.text)));
      else
        result.back().emplace_back();
    }
  }
  terms.swap(result);
}

// snapshot of the flags the CNF code reads
inline ugrep_rust::CnfFlags cnf_flags()
{
  ugrep_rust::CnfFlags f;
  f.mode = static_cast<uint8_t>(flag_mode);
  f.line_regexp = flag_line_regexp;
  f.word_regexp = flag_word_regexp;
  f.boolean = flag_bool;
  f.files = flag_files;
  f.invert_match = flag_invert_match;
  f.ignore_case = flag_ignore_case;
  f.best_match = flag_best_match;
  f.fuzzy = flag_fuzzy;
  f.file = to_bytes_vec(flag_file);
#ifdef HAVE_PCRE2
  f.have_pcre2 = true;
#else
  f.have_pcre2 = false;
#endif
  return f;
}

// snapshot for Stats::report, the counters are passed in by the caller (they are protected members of Stats)
inline ugrep_rust::StatsInput stats_input()
{
  ugrep_rust::StatsInput s;
  s.query = flag_query;
  s.pager = flag_pager != NULL;
  s.quiet = flag_quiet;
  s.files_with_matches = flag_files_with_matches;
  s.files_without_match = flag_files_without_match;
  s.ungroup = flag_ungroup;
  s.count = flag_count;
  s.only_matching = flag_only_matching;
  s.format = flag_format != NULL;
  s.index = flag_index != NULL;
  s.config_given = flag_config != NULL;
  if (flag_config != NULL)
    s.config = to_vec(flag_config, strlen(flag_config));
  s.config_files = to_bytes_vec(flag_config_files);
  s.fuzzy = flag_fuzzy;
  s.boolean = flag_bool;
  s.files = flag_files;
  s.mode = static_cast<uint8_t>(flag_mode);
  s.decompress = flag_decompress;
  s.zmax = flag_zmax;
  s.min_depth = flag_min_depth;
  s.max_depth = flag_max_depth;
  s.dereference = flag_dereference;
  s.no_dereference = flag_no_dereference;
  s.devices_action = static_cast<uint8_t>(flag_devices_action);
  s.directories_action = static_cast<uint8_t>(flag_directories_action);
  s.glob_ignore_case = flag_glob_ignore_case;
  s.hidden = flag_hidden;
#ifdef WITH_HIDDEN
  s.hidden_default = true;
#else
  s.hidden_default = false;
#endif
  s.ignore_files = to_bytes_vec(flag_ignore_files);
  s.min_count = flag_min_count;
  s.max_count = flag_max_count;
  s.max_files = flag_max_files;
  s.min_line = flag_min_line;
  s.max_line = flag_max_line;
  s.sort_given = flag_sort != NULL;
  if (flag_sort != NULL)
    s.sort = to_vec(flag_sort, strlen(flag_sort));
  s.min_size = flag_min_size;
  s.max_size = flag_max_size;
  s.file_magic = to_bytes_vec(flag_file_magic);
  s.include_fs = to_bytes_vec(flag_include_fs);
  s.exclude_fs = to_bytes_vec(flag_exclude_fs);
  s.all_include = to_bytes_vec(flag_all_include);
  s.all_exclude = to_bytes_vec(flag_all_exclude);
  s.all_include_dir = to_bytes_vec(flag_all_include_dir);
  s.all_exclude_dir = to_bytes_vec(flag_all_exclude_dir);
  s.include_iglob_size = flag_include_iglob_size;
  s.exclude_iglob_size = flag_exclude_iglob_size;
  s.include_iglob_dir_size = flag_include_iglob_dir_size;
  s.exclude_iglob_dir_size = flag_exclude_iglob_dir_size;
  s.crlf = NEWLINESTR[0] == '\r';
  s.pathsep = static_cast<uint8_t>(PATHSEPCHR);
  return s;
}

// write bytes returned by Rust to a FILE* (a FILE* cannot cross the cxx bridge)
inline void write_to(FILE *output, const rust::Vec<uint8_t>& text)
{
  if (!text.empty())
    fwrite(text.data(), 1, text.size(), output);
}

} // namespace core_rust

#endif // CPP_RUST_IMPL_SRC_CORE

#endif
