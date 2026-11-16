// Seam tap glue of the src_indexer component: tap macros for the C++ facade functions, and the C++ neutral encoders
// (the flag snapshot and argv rendered as cpp2rust-trace/1 JSON values).
//
// Without CPP2RUST_TAP this header only defines the tap macros as no-ops: product builds are unchanged. The tap
// header itself needs C++17 (std::optional), so it is not included in that case, exactly like src/core_tap.hpp.
// Use only at the entry of facade functions, never in business code. The Rust tap, the capture script and the
// replay harness are Behavior-task judge files and are not part of this seam change.
#ifndef INDEXER_TAP_HPP
#define INDEXER_TAP_HPP

#ifdef CPP2RUST_TAP

#include "../rust/src_indexer/cpp2rust_tap.hpp"

#include <optional>
#include <string>
#include <vector>

#include "ugrep-indexer_seam.h"

namespace indexer_tap {

// the flag snapshot the facade functions pass to Rust, as one stable "name=value;" string
inline std::string flags()
{
  std::string s;
  s.append("accuracy=").append(std::to_string(flag_accuracy));
  s.append(";check=").append(flag_check ? "1" : "0");
  s.append(";decompress=").append(flag_decompress ? "1" : "0");
  s.append(";delete=").append(flag_delete ? "1" : "0");
  s.append(";dereference_files=").append(flag_dereference_files ? "1" : "0");
  s.append(";force=").append(flag_force ? "1" : "0");
  s.append(";hidden=").append(flag_hidden ? "1" : "0");
  s.append(";ignore_binary=").append(flag_ignore_binary ? "1" : "0");
  s.append(";no_messages=").append(flag_no_messages ? "1" : "0");
  s.append(";quiet=").append(flag_quiet ? "1" : "0");
  s.append(";usage_warnings=").append(flag_usage_warnings ? "1" : "0");
  s.append(";verbose=").append(flag_verbose ? "1" : "0");
  s.append(";zmax=").append(std::to_string(flag_zmax));
  s.append(";ignore_files=");
  for (const auto& file : flag_ignore_files)
    s.append(file).push_back('\n');
  return s;
}

// argv[0..argc-1] as a JSON array of strings
inline std::vector<std::string> args(int argc, const char **argv)
{
  std::vector<std::string> v;
  for (int i = 0; i < argc; ++i)
    v.emplace_back(argv[i] != NULL ? argv[i] : "");
  return v;
}

// a C string that may be NULL: null or the string
inline std::optional<std::string> opt(const char *s)
{
  return s != NULL ? std::optional<std::string>(s) : std::nullopt;
}

// result of a void seam function
inline std::optional<bool> none()
{
  return std::nullopt;
}

// the out-parameters of index() as one stable "name=value;" string; the table itself is summarized by its size and
// a 64-bit FNV-1a hash of its bytes
inline std::string indexed(const uint8_t *hashes, size_t hashes_size, float noise, bool compressed, bool archive, bool binary, uint64_t size)
{
  uint64_t h = 14695981039346656037ull;
  for (size_t i = 0; i < hashes_size; ++i)
    h = (h ^ hashes[i]) * 1099511628211ull;
  char buf[40];
  std::snprintf(buf, sizeof buf, "%.17g", static_cast<double>(noise));
  std::string s;
  s.append("hashes_size=").append(std::to_string(hashes_size));
  s.append(";hashes_fnv=").append(std::to_string(h));
  s.append(";noise=").append(buf);
  s.append(";compressed=").append(compressed ? "1" : "0");
  s.append(";archive=").append(archive ? "1" : "0");
  s.append(";binary=").append(binary ? "1" : "0");
  s.append(";size=").append(std::to_string(size));
  return s;
}

} // namespace indexer_tap

#else // no tap

#define TAP_CALL(fn, ...) ((void)0)
#define TAP_STATE(v) ((void)0)
#define TAP_RET(v) ((void)0)
#define TAP_ERR(v) ((void)0)
#define TAP_OUT(callee, ...) ((void)0)
#define TAP_OUT_RET(callee, v) ((void)0)

#endif

#endif
