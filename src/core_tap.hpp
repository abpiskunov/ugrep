// Seam tap glue of the src_core component: tap macros for the C++ facade functions and the unit entry functions,
// and the C++ neutral encoders (types of src/cnf.hpp and src/stats.hpp rendered as cpp2rust-trace/1 JSON values).
//
// Without CPP2RUST_TAP this header only defines the tap macros as no-ops: product builds are unchanged. The tap
// header itself needs C++17 (std::optional), so it is not included in that case, exactly like src/glob.cpp does.
// Use only at the entry of facade functions and unit entry functions, never in business code. The Rust tap, the
// capture script and the replay harness are Behavior-task judge files and are not part of this seam change.
#ifndef CORE_TAP_HPP
#define CORE_TAP_HPP

#ifdef CPP2RUST_TAP

#include "../rust/src_core/cpp2rust_tap.hpp"

#include <cstdio>
#include <functional>
#include <list>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "ugrep.hpp"
#include "stats.hpp"

namespace core_tap {

// a CNF::Terms as a nested JSON array: [[ "pattern" | null, ... ], ...]; the NULL smart pointer is null
inline std::vector<std::vector<std::optional<std::string>>> terms(const CNF::Terms& t)
{
  std::vector<std::vector<std::optional<std::string>>> v;
  for (const auto& term : t)
  {
    v.emplace_back();
    for (const auto& pattern : term)
    {
      if (pattern)
        v.back().emplace_back(*pattern);
      else
        v.back().emplace_back();
    }
  }
  return v;
}

inline std::string join(const std::vector<std::string>& v)
{
  std::string s;
  for (const auto& i : v)
    s.append(i).push_back('\n');
  return s;
}

// the flag snapshot the CNF facade functions pass to Rust, as one stable "name=value;" string
inline std::string cnf_flags()
{
  std::string s;
  s.append("mode=").append(std::to_string(static_cast<int>(flag_mode)));
  s.append(";line_regexp=").append(flag_line_regexp ? "1" : "0");
  s.append(";word_regexp=").append(flag_word_regexp ? "1" : "0");
  s.append(";bool=").append(flag_bool ? "1" : "0");
  s.append(";files=").append(flag_files ? "1" : "0");
  s.append(";invert_match=").append(flag_invert_match ? "1" : "0");
  s.append(";ignore_case=").append(flag_ignore_case ? "1" : "0");
  s.append(";best_match=").append(flag_best_match ? "1" : "0");
  s.append(";fuzzy=").append(std::to_string(flag_fuzzy));
  s.append(";file=").append(join(flag_file));
  return s;
}

// run a legacy FILE*-writing body against a scratch stream and return what it wrote (tap build only)
inline std::string capture(const std::function<void(FILE*)>& body)
{
#ifdef OS_WIN_OR_MINGW
  char *name = _tempnam(NULL, "c2r");
  FILE *f = name != NULL ? fopen(name, "w+bTD") : NULL;
  free(name);
#else
  FILE *f = tmpfile();
#endif
  std::string text;
  if (f != NULL)
  {
    body(f);
    fflush(f);
    long n = ftell(f);
    if (n > 0)
    {
      text.resize(static_cast<size_t>(n));
      rewind(f);
      text.resize(fread(&text[0], 1, text.size(), f));
    }
    fclose(f);
  }
  return text;
}

inline std::optional<bool> none()
{
  return std::nullopt;
}

} // namespace core_tap

#else // no tap

#define TAP_CALL(fn, ...) ((void)0)
#define TAP_STATE(v) ((void)0)
#define TAP_RET(v) ((void)0)
#define TAP_ERR(v) ((void)0)
#define TAP_OUT(callee, ...) ((void)0)
#define TAP_OUT_RET(callee, v) ((void)0)

#endif

#endif
