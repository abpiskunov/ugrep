// C++ side of the src_indexer seam: conversions between the flag_* globals of src/ugrep-indexer.cpp and the cxx bridge
// types of rust/src_indexer/src/lib.rs. Included only when CPP_RUST_IMPL_SRC_INDEXER is defined (msvc/rust.targets,
// /p:CPP_RUST_IMPL_SRC_INDEXER=rust). See rust/src_indexer/SEAM.md.
//
// The flag_* globals stay owned by C++: every crossing takes a read-only snapshot (indexer_rust::flags) and Rust never
// writes a flag; what a function changes comes back in its result (indexer_rust::apply).
#ifndef INDEXER_RUST_HPP
#define INDEXER_RUST_HPP

#ifdef CPP_RUST_IMPL_SRC_INDEXER

#include "ugrep-indexer_seam.h"
#include "src_indexer/src/lib.rs.h"

#include <cstring>
#include <string>

namespace indexer_rust {

inline rust::Vec<uint8_t> to_vec(const std::string& s)
{
  rust::Vec<uint8_t> v;
  v.reserve(s.size());
  for (char c : s)
    v.push_back(static_cast<uint8_t>(c));
  return v;
}

inline rust::Slice<const uint8_t> to_slice(const char *s)
{
  return rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t*>(s), strlen(s));
}

inline std::string to_string(const rust::Vec<uint8_t>& v)
{
  return std::string(reinterpret_cast<const char*>(v.data()), v.size());
}

// snapshot of the flag_* globals
inline ugrep_rust::IndexerFlags flags()
{
  ugrep_rust::IndexerFlags f;
  f.accuracy = flag_accuracy;
  f.check = flag_check;
  f.decompress = flag_decompress;
  f.delete_indexes = flag_delete;
  f.dereference_files = flag_dereference_files;
  f.force = flag_force;
  f.hidden = flag_hidden;
  f.ignore_binary = flag_ignore_binary;
  f.no_messages = flag_no_messages;
  f.quiet = flag_quiet;
  f.usage_warnings = flag_usage_warnings;
  f.verbose = flag_verbose;
  f.zmax = flag_zmax;
  for (const auto& file : flag_ignore_files)
    f.ignore_files.push_back(ugrep_rust::IndexerString{to_vec(file)});
  return f;
}

// store the flags a seam function returned back into the globals
inline void apply(const ugrep_rust::IndexerFlags& f)
{
  flag_accuracy = f.accuracy;
  flag_check = f.check;
  flag_decompress = f.decompress;
  flag_delete = f.delete_indexes;
  flag_dereference_files = f.dereference_files;
  flag_force = f.force;
  flag_hidden = f.hidden;
  flag_ignore_binary = f.ignore_binary;
  flag_no_messages = f.no_messages;
  flag_quiet = f.quiet;
  flag_usage_warnings = f.usage_warnings;
  flag_verbose = f.verbose;
  flag_zmax = f.zmax;
  flag_ignore_files.clear();
  for (const auto& file : f.ignore_files)
    flag_ignore_files.emplace_back(to_string(file.data));
}

} // namespace indexer_rust

#endif // CPP_RUST_IMPL_SRC_INDEXER

#endif
