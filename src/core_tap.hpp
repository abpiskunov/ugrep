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
#ifdef HAVE_PCRE2
  s.append(";have_pcre2=1");
#else
  s.append(";have_pcre2=0");
#endif
  s.append(";file=").append(join(flag_file));
  return s;
}

// the flag globals Stats::report reads (see StatsInput of rust/src_core/src/signatures.rs), as one stable string:
// entries "name=value" separated by '\n'; list values are items separated by '\x1f'; an optional string is "-"
// when NULL, else "+" followed by the value
inline std::string stats_flags()
{
  std::string s;
  auto put = [&s](const char *name, const std::string& v) { s.append(name).append("=").append(v).push_back('\n'); };
  auto num = [](size_t n) { return std::to_string(n); };
  auto list = [](const std::vector<std::string>& v) {
    std::string r;
    for (size_t i = 0; i < v.size(); ++i)
    {
      if (i > 0)
        r.push_back('\x1f');
      r.append(v[i]);
    }
    return r;
  };
  auto opt = [](const char *p) { return p == NULL ? std::string("-") : std::string("+") + p; };
  put("query", flag_query ? "1" : "0");
  put("pager", flag_pager != NULL ? "1" : "0");
  put("quiet", flag_quiet ? "1" : "0");
  put("files_with_matches", flag_files_with_matches ? "1" : "0");
  put("files_without_match", flag_files_without_match ? "1" : "0");
  put("ungroup", flag_ungroup ? "1" : "0");
  put("count", flag_count ? "1" : "0");
  put("only_matching", flag_only_matching ? "1" : "0");
  put("format", flag_format != NULL ? "1" : "0");
  put("index", flag_index != NULL ? "1" : "0");
  put("config", opt(flag_config));
  put("config_files", list(std::vector<std::string>(flag_config_files.begin(), flag_config_files.end())));
  put("fuzzy", num(flag_fuzzy));
  put("boolean", flag_bool ? "1" : "0");
  put("files", flag_files ? "1" : "0");
  put("mode", std::to_string(static_cast<int>(flag_mode)));
  put("decompress", flag_decompress ? "1" : "0");
  put("zmax", num(flag_zmax));
  put("min_depth", num(flag_min_depth));
  put("max_depth", num(flag_max_depth));
  put("dereference", flag_dereference ? "1" : "0");
  put("no_dereference", flag_no_dereference ? "1" : "0");
  put("devices_action", std::to_string(static_cast<int>(flag_devices_action)));
  put("directories_action", std::to_string(static_cast<int>(flag_directories_action)));
  put("glob_ignore_case", flag_glob_ignore_case ? "1" : "0");
  put("hidden", flag_hidden ? "1" : "0");
#ifdef WITH_HIDDEN
  put("hidden_default", "1");
#else
  put("hidden_default", "0");
#endif
  put("ignore_files", list(std::vector<std::string>(flag_ignore_files.begin(), flag_ignore_files.end())));
  put("min_count", num(flag_min_count));
  put("max_count", num(flag_max_count));
  put("max_files", num(flag_max_files));
  put("min_line", num(flag_min_line));
  put("max_line", num(flag_max_line));
  put("sort", opt(flag_sort));
  put("min_size", num(flag_min_size));
  put("max_size", num(flag_max_size));
  put("file_magic", list(std::vector<std::string>(flag_file_magic.begin(), flag_file_magic.end())));
  put("include_fs", list(std::vector<std::string>(flag_include_fs.begin(), flag_include_fs.end())));
  put("exclude_fs", list(std::vector<std::string>(flag_exclude_fs.begin(), flag_exclude_fs.end())));
  put("all_include", list(std::vector<std::string>(flag_all_include.begin(), flag_all_include.end())));
  put("all_exclude", list(std::vector<std::string>(flag_all_exclude.begin(), flag_all_exclude.end())));
  put("all_include_dir", list(std::vector<std::string>(flag_all_include_dir.begin(), flag_all_include_dir.end())));
  put("all_exclude_dir", list(std::vector<std::string>(flag_all_exclude_dir.begin(), flag_all_exclude_dir.end())));
  put("include_iglob_size", num(flag_include_iglob_size));
  put("exclude_iglob_size", num(flag_exclude_iglob_size));
  put("include_iglob_dir_size", num(flag_include_iglob_dir_size));
  put("exclude_iglob_dir_size", num(flag_exclude_iglob_dir_size));
  put("crlf", NEWLINESTR[0] == '\r' ? "1" : "0");
  put("pathsep", std::to_string(static_cast<int>(PATHSEPCHR)));
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
