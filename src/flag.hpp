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
@file      flag.hpp
@brief     Tri-state Flag class and global flags set by options
@author    Robert van Engelen - engelen@genivia.com
@copyright (c) 2019-2025, Robert van Engelen, Genivia Inc. All rights reserved.
@copyright (c) BSD-3 License - see LICENSE.txt
*/

#ifndef FLAG_HPP
#define FLAG_HPP

#include <reflex/input.h>
#include <string>
#include <set>
#include <vector>

// three-valued logic flags that behave as bool; this allows us to check if a flag was undefined (default) and explicitly enabled/disabled
class Flag {

 public:

  Flag()                     : value(UNDEFINED)  { }
  Flag(bool flag)            : value(flag ? T : F)  { }
  Flag& operator=(bool flag) { value = flag ? T : F; return *this; }
       operator bool() const { return is_true(); }
  bool is_undefined()  const { return value == UNDEFINED; }
  bool is_defined()    const { return value != UNDEFINED; }
  bool is_false()      const { return value == F; }
  bool is_true()       const { return value == T; }

 private:

  enum { UNDEFINED = -1, F = 0, T = 1 } value;

};

// --sort=KEY is n/a or by list, name, score, size, used time, changed time, created time
enum class Sort { NA, NAME, BEST, SIZE, USED, CHANGED, CREATED, LIST };

// -D, --devices and -d, --directories
enum class Action { UNSP, SKIP, READ, RECURSE };

// -E (default), -G, -F, -P matching modes
enum class Mode { UNSP, ERE, BRE, FIXED, PERL };

// ugrep command-line options, colors and the other option state:
// the scalars and C string pointers are owned by Rust (rust/src_core/src/settings.rs, one #[repr(C)] struct
// exported as `ugrep_settings`); the macros below keep the historical `flag_*` and `color_*` names so every
// use is a plain load or store of a field of that struct. `std::string` and container options stay in settings.cpp.
#ifndef COLORLEN
# define COLORLEN 32
#endif

struct Settings {
  char color_sl[COLORLEN];
  char color_cx[COLORLEN];
  char color_mt[COLORLEN];
  char color_ms[COLORLEN];
  char color_mc[COLORLEN];
  char color_fn[COLORLEN];
  char color_ln[COLORLEN];
  char color_cn[COLORLEN];
  char color_bn[COLORLEN];
  char color_se[COLORLEN];
  char color_qp[COLORLEN];
  char color_qe[COLORLEN];
  char color_qr[COLORLEN];
  char color_qm[COLORLEN];
  char color_ql[COLORLEN];
  char color_qb[COLORLEN];
  char match_ms[COLORLEN];
  char match_mc[COLORLEN];
  char match_off[COLORLEN];
  const char *color_hl;
  const char *color_st;
  const char *color_del;
  const char *color_off;
  const char *color_high;
  const char *color_error;
  const char *color_warning;
  const char *color_message;
  bool flag_all_threads;
  bool flag_any_line;
  bool flag_best_match;
  bool flag_bool;
  bool flag_color_term;
  bool flag_confirm;
  bool flag_count;
  bool flag_cpp;
  bool flag_csv;
  bool flag_decompress;
  bool flag_dereference;
  bool flag_dereference_files;
  bool flag_files;
  bool flag_files_with_matches;
  bool flag_files_without_match;
  bool flag_glob_ignore_case;
  bool flag_grep;
  bool flag_hex;
  bool flag_hex_star;
  bool flag_hex_cbr;
  bool flag_hex_chr;
  bool flag_hex_hbr;
  bool flag_hidden;
  bool flag_hyperlink_line;
  bool flag_invert_match;
  bool flag_json;
  bool flag_line_buffered;
  bool flag_line_regexp;
  bool flag_match;
  bool flag_multiline;
  bool flag_no_dereference;
  bool flag_no_filename;
  bool flag_no_header;
  bool flag_no_messages;
  bool flag_not;
  bool flag_null;
  bool flag_null_data;
  bool flag_only_line_number;
  bool flag_only_matching;
  bool flag_query;
  bool flag_quiet;
  bool flag_sort_rev;
  bool flag_split;
  bool flag_stdin;
  bool flag_tty_term;
  bool flag_usage_warnings;
  bool flag_word_regexp;
  bool flag_xml;
  bool flag_with_hex;
  bool flag_with_filename;
  Flag flag_binary;
  Flag flag_binary_without_match;
  Flag flag_break;
  Flag flag_byte_offset;
  Flag flag_column_number;
  Flag flag_empty;
  Flag flag_dotall;
  Flag flag_free_space;
  Flag flag_heading;
  Flag flag_ignore_case;
  Flag flag_initial_tab;
  Flag flag_line_number;
  Flag flag_smart_case;
  Flag flag_text;
  Flag flag_tree;
  Flag flag_ungroup;
  Sort flag_sort_key;
  Action flag_devices_action;
  Action flag_directories_action;
  Mode flag_mode;
  size_t flag_after_context;
  size_t flag_before_context;
  size_t flag_delay;
  size_t flag_exclude_iglob_size;
  size_t flag_exclude_iglob_dir_size;
  size_t flag_fuzzy;
  size_t flag_hex_after;
  size_t flag_hex_before;
  size_t flag_hex_columns;
  size_t flag_include_iglob_size;
  size_t flag_include_iglob_dir_size;
  size_t flag_jobs;
  size_t flag_max_count;
  size_t flag_max_depth;
  size_t flag_max_files;
  size_t flag_max_line;
  size_t flag_max_size;
  size_t flag_max_mmap;
  size_t flag_max_queue;
  size_t flag_min_count;
  size_t flag_min_depth;
  size_t flag_min_line;
  size_t flag_min_size;
  size_t flag_min_magic;
  size_t flag_min_steal;
  size_t flag_not_magic;
  size_t flag_tabs;
  size_t flag_width;
  size_t flag_zmax;
  const char *flag_binary_files;
  const char *flag_color;
  const char *flag_color_query;
  const char *flag_colors;
  const char *flag_config;
  const char *flag_devices;
  const char *flag_directories;
  const char *flag_encoding;
  const char *flag_format;
  const char *flag_format_begin;
  const char *flag_format_close;
  const char *flag_format_end;
  const char *flag_format_open;
  const char *flag_group_separator;
  const char *flag_hexdump;
  const char *flag_hyperlink;
  const char *flag_index;
  const char *flag_label;
  const char *flag_pager;
  const char *flag_pretty;
  const char *flag_replace;
  const char *flag_save_config;
  const char *flag_separator;
  const char *flag_separator_dash;
  const char *flag_separator_bar;
  const char *flag_sort;
  const char *flag_stats;
  const char *flag_tag;
  const char *flag_view;
  reflex::Input::file_encoding_type flag_encoding_type;
};

extern "C" Settings ugrep_settings;

#ifndef SETTINGS_NO_MACROS // settings.cpp checks the layout with the member names
#define color_sl (ugrep_settings.color_sl)
#define color_cx (ugrep_settings.color_cx)
#define color_mt (ugrep_settings.color_mt)
#define color_ms (ugrep_settings.color_ms)
#define color_mc (ugrep_settings.color_mc)
#define color_fn (ugrep_settings.color_fn)
#define color_ln (ugrep_settings.color_ln)
#define color_cn (ugrep_settings.color_cn)
#define color_bn (ugrep_settings.color_bn)
#define color_se (ugrep_settings.color_se)
#define color_qp (ugrep_settings.color_qp)
#define color_qe (ugrep_settings.color_qe)
#define color_qr (ugrep_settings.color_qr)
#define color_qm (ugrep_settings.color_qm)
#define color_ql (ugrep_settings.color_ql)
#define color_qb (ugrep_settings.color_qb)
#define match_ms (ugrep_settings.match_ms)
#define match_mc (ugrep_settings.match_mc)
#define match_off (ugrep_settings.match_off)
#define color_hl (ugrep_settings.color_hl)
#define color_st (ugrep_settings.color_st)
#define color_del (ugrep_settings.color_del)
#define color_off (ugrep_settings.color_off)
#define color_high (ugrep_settings.color_high)
#define color_error (ugrep_settings.color_error)
#define color_warning (ugrep_settings.color_warning)
#define color_message (ugrep_settings.color_message)
#define flag_all_threads (ugrep_settings.flag_all_threads)
#define flag_any_line (ugrep_settings.flag_any_line)
#define flag_best_match (ugrep_settings.flag_best_match)
#define flag_bool (ugrep_settings.flag_bool)
#define flag_color_term (ugrep_settings.flag_color_term)
#define flag_confirm (ugrep_settings.flag_confirm)
#define flag_count (ugrep_settings.flag_count)
#define flag_cpp (ugrep_settings.flag_cpp)
#define flag_csv (ugrep_settings.flag_csv)
#define flag_decompress (ugrep_settings.flag_decompress)
#define flag_dereference (ugrep_settings.flag_dereference)
#define flag_dereference_files (ugrep_settings.flag_dereference_files)
#define flag_files (ugrep_settings.flag_files)
#define flag_files_with_matches (ugrep_settings.flag_files_with_matches)
#define flag_files_without_match (ugrep_settings.flag_files_without_match)
#define flag_glob_ignore_case (ugrep_settings.flag_glob_ignore_case)
#define flag_grep (ugrep_settings.flag_grep)
#define flag_hex (ugrep_settings.flag_hex)
#define flag_hex_star (ugrep_settings.flag_hex_star)
#define flag_hex_cbr (ugrep_settings.flag_hex_cbr)
#define flag_hex_chr (ugrep_settings.flag_hex_chr)
#define flag_hex_hbr (ugrep_settings.flag_hex_hbr)
#define flag_hidden (ugrep_settings.flag_hidden)
#define flag_hyperlink_line (ugrep_settings.flag_hyperlink_line)
#define flag_invert_match (ugrep_settings.flag_invert_match)
#define flag_json (ugrep_settings.flag_json)
#define flag_line_buffered (ugrep_settings.flag_line_buffered)
#define flag_line_regexp (ugrep_settings.flag_line_regexp)
#define flag_match (ugrep_settings.flag_match)
#define flag_multiline (ugrep_settings.flag_multiline)
#define flag_no_dereference (ugrep_settings.flag_no_dereference)
#define flag_no_filename (ugrep_settings.flag_no_filename)
#define flag_no_header (ugrep_settings.flag_no_header)
#define flag_no_messages (ugrep_settings.flag_no_messages)
#define flag_not (ugrep_settings.flag_not)
#define flag_null (ugrep_settings.flag_null)
#define flag_null_data (ugrep_settings.flag_null_data)
#define flag_only_line_number (ugrep_settings.flag_only_line_number)
#define flag_only_matching (ugrep_settings.flag_only_matching)
#define flag_query (ugrep_settings.flag_query)
#define flag_quiet (ugrep_settings.flag_quiet)
#define flag_sort_rev (ugrep_settings.flag_sort_rev)
#define flag_split (ugrep_settings.flag_split)
#define flag_stdin (ugrep_settings.flag_stdin)
#define flag_tty_term (ugrep_settings.flag_tty_term)
#define flag_usage_warnings (ugrep_settings.flag_usage_warnings)
#define flag_word_regexp (ugrep_settings.flag_word_regexp)
#define flag_xml (ugrep_settings.flag_xml)
#define flag_with_hex (ugrep_settings.flag_with_hex)
#define flag_with_filename (ugrep_settings.flag_with_filename)
#define flag_binary (ugrep_settings.flag_binary)
#define flag_binary_without_match (ugrep_settings.flag_binary_without_match)
#define flag_break (ugrep_settings.flag_break)
#define flag_byte_offset (ugrep_settings.flag_byte_offset)
#define flag_column_number (ugrep_settings.flag_column_number)
#define flag_empty (ugrep_settings.flag_empty)
#define flag_dotall (ugrep_settings.flag_dotall)
#define flag_free_space (ugrep_settings.flag_free_space)
#define flag_heading (ugrep_settings.flag_heading)
#define flag_ignore_case (ugrep_settings.flag_ignore_case)
#define flag_initial_tab (ugrep_settings.flag_initial_tab)
#define flag_line_number (ugrep_settings.flag_line_number)
#define flag_smart_case (ugrep_settings.flag_smart_case)
#define flag_text (ugrep_settings.flag_text)
#define flag_tree (ugrep_settings.flag_tree)
#define flag_ungroup (ugrep_settings.flag_ungroup)
#define flag_sort_key (ugrep_settings.flag_sort_key)
#define flag_devices_action (ugrep_settings.flag_devices_action)
#define flag_directories_action (ugrep_settings.flag_directories_action)
#define flag_mode (ugrep_settings.flag_mode)
#define flag_after_context (ugrep_settings.flag_after_context)
#define flag_before_context (ugrep_settings.flag_before_context)
#define flag_delay (ugrep_settings.flag_delay)
#define flag_exclude_iglob_size (ugrep_settings.flag_exclude_iglob_size)
#define flag_exclude_iglob_dir_size (ugrep_settings.flag_exclude_iglob_dir_size)
#define flag_fuzzy (ugrep_settings.flag_fuzzy)
#define flag_hex_after (ugrep_settings.flag_hex_after)
#define flag_hex_before (ugrep_settings.flag_hex_before)
#define flag_hex_columns (ugrep_settings.flag_hex_columns)
#define flag_include_iglob_size (ugrep_settings.flag_include_iglob_size)
#define flag_include_iglob_dir_size (ugrep_settings.flag_include_iglob_dir_size)
#define flag_jobs (ugrep_settings.flag_jobs)
#define flag_max_count (ugrep_settings.flag_max_count)
#define flag_max_depth (ugrep_settings.flag_max_depth)
#define flag_max_files (ugrep_settings.flag_max_files)
#define flag_max_line (ugrep_settings.flag_max_line)
#define flag_max_size (ugrep_settings.flag_max_size)
#define flag_max_mmap (ugrep_settings.flag_max_mmap)
#define flag_max_queue (ugrep_settings.flag_max_queue)
#define flag_min_count (ugrep_settings.flag_min_count)
#define flag_min_depth (ugrep_settings.flag_min_depth)
#define flag_min_line (ugrep_settings.flag_min_line)
#define flag_min_size (ugrep_settings.flag_min_size)
#define flag_min_magic (ugrep_settings.flag_min_magic)
#define flag_min_steal (ugrep_settings.flag_min_steal)
#define flag_not_magic (ugrep_settings.flag_not_magic)
#define flag_tabs (ugrep_settings.flag_tabs)
#define flag_width (ugrep_settings.flag_width)
#define flag_zmax (ugrep_settings.flag_zmax)
#define flag_binary_files (ugrep_settings.flag_binary_files)
#define flag_color (ugrep_settings.flag_color)
#define flag_color_query (ugrep_settings.flag_color_query)
#define flag_colors (ugrep_settings.flag_colors)
#define flag_config (ugrep_settings.flag_config)
#define flag_devices (ugrep_settings.flag_devices)
#define flag_directories (ugrep_settings.flag_directories)
#define flag_encoding (ugrep_settings.flag_encoding)
#define flag_format (ugrep_settings.flag_format)
#define flag_format_begin (ugrep_settings.flag_format_begin)
#define flag_format_close (ugrep_settings.flag_format_close)
#define flag_format_end (ugrep_settings.flag_format_end)
#define flag_format_open (ugrep_settings.flag_format_open)
#define flag_group_separator (ugrep_settings.flag_group_separator)
#define flag_hexdump (ugrep_settings.flag_hexdump)
#define flag_hyperlink (ugrep_settings.flag_hyperlink)
#define flag_index (ugrep_settings.flag_index)
#define flag_label (ugrep_settings.flag_label)
#define flag_pager (ugrep_settings.flag_pager)
#define flag_pretty (ugrep_settings.flag_pretty)
#define flag_replace (ugrep_settings.flag_replace)
#define flag_save_config (ugrep_settings.flag_save_config)
#define flag_separator (ugrep_settings.flag_separator)
#define flag_separator_dash (ugrep_settings.flag_separator_dash)
#define flag_separator_bar (ugrep_settings.flag_separator_bar)
#define flag_sort (ugrep_settings.flag_sort)
#define flag_stats (ugrep_settings.flag_stats)
#define flag_tag (ugrep_settings.flag_tag)
#define flag_view (ugrep_settings.flag_view)
#define flag_encoding_type (ugrep_settings.flag_encoding_type)
#endif

// options that cannot be owned by Rust by value, defined in settings.cpp
extern std::string flag_filter;
extern std::string flag_hyperlink_prefix;
extern std::string flag_hyperlink_host;
extern std::string flag_hyperlink_path;
extern std::string flag_regexp;
extern std::set<std::string> flag_config_files;
extern std::set<std::string> flag_ignore_files;
extern std::vector<std::string> flag_file;
extern std::vector<std::string> flag_file_type;
extern std::vector<std::string> flag_file_extension;
extern std::vector<std::string> flag_file_magic;
extern std::vector<std::string> flag_filter_magic_label;
extern std::vector<std::string> flag_from;
extern std::vector<std::string> flag_glob;
extern std::vector<std::string> flag_iglob;
extern std::vector<std::string> flag_include;
extern std::vector<std::string> flag_include_dir;
extern std::vector<std::string> flag_include_from;
extern std::vector<std::string> flag_include_fs;
extern std::vector<std::string> flag_exclude;
extern std::vector<std::string> flag_exclude_dir;
extern std::vector<std::string> flag_exclude_from;
extern std::vector<std::string> flag_exclude_fs;
extern std::vector<std::string> flag_all_include;
extern std::vector<std::string> flag_all_include_dir;
extern std::vector<std::string> flag_all_exclude;
extern std::vector<std::string> flag_all_exclude_dir;

#endif
