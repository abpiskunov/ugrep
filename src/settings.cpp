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
*   (3) The name of the author may not be used to                              *
*       endorse or promote products derived from this software without         *
*       specific prior written permission.                                     *
*                                                                              *
* THIS SOFTWARE IS PROVIDED BY THE AUTHOR "AS IS" AND ANY EXPRESS OR IMPLIED   *
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
@file      settings.cpp
@brief     C++-owned option state that cannot be owned by Rust by value, and the layout check of the Rust settings
*/

#define SETTINGS_NO_MACROS
#include "ugrep.hpp"

#include <cstddef>
#include <cstdio>
#include <cstdlib>

extern "C" size_t ugrep_settings_size();
extern "C" size_t ugrep_settings_count();
extern "C" size_t ugrep_settings_offset(size_t index);

// the Flag tri-state and the enums are int sized, as the Rust side assumes
static_assert(sizeof(Flag) == sizeof(int32_t), "Flag must be 32 bits");
static_assert(sizeof(Sort) == sizeof(int32_t), "Sort must be 32 bits");
static_assert(sizeof(Action) == sizeof(int32_t), "Action must be 32 bits");
static_assert(sizeof(Mode) == sizeof(int32_t), "Mode must be 32 bits");
static_assert(sizeof(reflex::Input::file_encoding_type) == sizeof(uint16_t), "file_encoding_type must be 16 bits");

std::string              flag_filter;
std::string              flag_hyperlink_prefix;
std::string              flag_hyperlink_host;
std::string              flag_hyperlink_path;
std::string              flag_regexp;
std::set<std::string>    flag_config_files;
std::set<std::string>    flag_ignore_files;
std::vector<std::string> flag_file;
std::vector<std::string> flag_file_type;
std::vector<std::string> flag_file_extension;
std::vector<std::string> flag_file_magic;
std::vector<std::string> flag_filter_magic_label;
std::vector<std::string> flag_from;
std::vector<std::string> flag_glob;
std::vector<std::string> flag_iglob;
std::vector<std::string> flag_include;
std::vector<std::string> flag_include_dir;
std::vector<std::string> flag_include_from;
std::vector<std::string> flag_include_fs;
std::vector<std::string> flag_exclude;
std::vector<std::string> flag_exclude_dir;
std::vector<std::string> flag_exclude_from;
std::vector<std::string> flag_exclude_fs;
std::vector<std::string> flag_all_include;
std::vector<std::string> flag_all_include_dir;
std::vector<std::string> flag_all_exclude;
std::vector<std::string> flag_all_exclude_dir;

// abort at startup if the C++ and Rust layouts of Settings differ
static const struct SettingsLayoutCheck {
  SettingsLayoutCheck()
  {
    static const size_t offsets[] = {
      offsetof(Settings, color_sl),
      offsetof(Settings, color_cx),
      offsetof(Settings, color_mt),
      offsetof(Settings, color_ms),
      offsetof(Settings, color_mc),
      offsetof(Settings, color_fn),
      offsetof(Settings, color_ln),
      offsetof(Settings, color_cn),
      offsetof(Settings, color_bn),
      offsetof(Settings, color_se),
      offsetof(Settings, color_qp),
      offsetof(Settings, color_qe),
      offsetof(Settings, color_qr),
      offsetof(Settings, color_qm),
      offsetof(Settings, color_ql),
      offsetof(Settings, color_qb),
      offsetof(Settings, match_ms),
      offsetof(Settings, match_mc),
      offsetof(Settings, match_off),
      offsetof(Settings, color_hl),
      offsetof(Settings, color_st),
      offsetof(Settings, color_del),
      offsetof(Settings, color_off),
      offsetof(Settings, color_high),
      offsetof(Settings, color_error),
      offsetof(Settings, color_warning),
      offsetof(Settings, color_message),
      offsetof(Settings, flag_all_threads),
      offsetof(Settings, flag_any_line),
      offsetof(Settings, flag_best_match),
      offsetof(Settings, flag_bool),
      offsetof(Settings, flag_color_term),
      offsetof(Settings, flag_confirm),
      offsetof(Settings, flag_count),
      offsetof(Settings, flag_cpp),
      offsetof(Settings, flag_csv),
      offsetof(Settings, flag_decompress),
      offsetof(Settings, flag_dereference),
      offsetof(Settings, flag_dereference_files),
      offsetof(Settings, flag_files),
      offsetof(Settings, flag_files_with_matches),
      offsetof(Settings, flag_files_without_match),
      offsetof(Settings, flag_glob_ignore_case),
      offsetof(Settings, flag_grep),
      offsetof(Settings, flag_hex),
      offsetof(Settings, flag_hex_star),
      offsetof(Settings, flag_hex_cbr),
      offsetof(Settings, flag_hex_chr),
      offsetof(Settings, flag_hex_hbr),
      offsetof(Settings, flag_hidden),
      offsetof(Settings, flag_hyperlink_line),
      offsetof(Settings, flag_invert_match),
      offsetof(Settings, flag_json),
      offsetof(Settings, flag_line_buffered),
      offsetof(Settings, flag_line_regexp),
      offsetof(Settings, flag_match),
      offsetof(Settings, flag_multiline),
      offsetof(Settings, flag_no_dereference),
      offsetof(Settings, flag_no_filename),
      offsetof(Settings, flag_no_header),
      offsetof(Settings, flag_no_messages),
      offsetof(Settings, flag_not),
      offsetof(Settings, flag_null),
      offsetof(Settings, flag_null_data),
      offsetof(Settings, flag_only_line_number),
      offsetof(Settings, flag_only_matching),
      offsetof(Settings, flag_query),
      offsetof(Settings, flag_quiet),
      offsetof(Settings, flag_sort_rev),
      offsetof(Settings, flag_split),
      offsetof(Settings, flag_stdin),
      offsetof(Settings, flag_tty_term),
      offsetof(Settings, flag_usage_warnings),
      offsetof(Settings, flag_word_regexp),
      offsetof(Settings, flag_xml),
      offsetof(Settings, flag_with_hex),
      offsetof(Settings, flag_with_filename),
      offsetof(Settings, flag_binary),
      offsetof(Settings, flag_binary_without_match),
      offsetof(Settings, flag_break),
      offsetof(Settings, flag_byte_offset),
      offsetof(Settings, flag_column_number),
      offsetof(Settings, flag_empty),
      offsetof(Settings, flag_dotall),
      offsetof(Settings, flag_free_space),
      offsetof(Settings, flag_heading),
      offsetof(Settings, flag_ignore_case),
      offsetof(Settings, flag_initial_tab),
      offsetof(Settings, flag_line_number),
      offsetof(Settings, flag_smart_case),
      offsetof(Settings, flag_text),
      offsetof(Settings, flag_tree),
      offsetof(Settings, flag_ungroup),
      offsetof(Settings, flag_sort_key),
      offsetof(Settings, flag_devices_action),
      offsetof(Settings, flag_directories_action),
      offsetof(Settings, flag_mode),
      offsetof(Settings, flag_after_context),
      offsetof(Settings, flag_before_context),
      offsetof(Settings, flag_delay),
      offsetof(Settings, flag_exclude_iglob_size),
      offsetof(Settings, flag_exclude_iglob_dir_size),
      offsetof(Settings, flag_fuzzy),
      offsetof(Settings, flag_hex_after),
      offsetof(Settings, flag_hex_before),
      offsetof(Settings, flag_hex_columns),
      offsetof(Settings, flag_include_iglob_size),
      offsetof(Settings, flag_include_iglob_dir_size),
      offsetof(Settings, flag_jobs),
      offsetof(Settings, flag_max_count),
      offsetof(Settings, flag_max_depth),
      offsetof(Settings, flag_max_files),
      offsetof(Settings, flag_max_line),
      offsetof(Settings, flag_max_size),
      offsetof(Settings, flag_max_mmap),
      offsetof(Settings, flag_max_queue),
      offsetof(Settings, flag_min_count),
      offsetof(Settings, flag_min_depth),
      offsetof(Settings, flag_min_line),
      offsetof(Settings, flag_min_size),
      offsetof(Settings, flag_min_magic),
      offsetof(Settings, flag_min_steal),
      offsetof(Settings, flag_not_magic),
      offsetof(Settings, flag_tabs),
      offsetof(Settings, flag_width),
      offsetof(Settings, flag_zmax),
      offsetof(Settings, flag_binary_files),
      offsetof(Settings, flag_color),
      offsetof(Settings, flag_color_query),
      offsetof(Settings, flag_colors),
      offsetof(Settings, flag_config),
      offsetof(Settings, flag_devices),
      offsetof(Settings, flag_directories),
      offsetof(Settings, flag_encoding),
      offsetof(Settings, flag_format),
      offsetof(Settings, flag_format_begin),
      offsetof(Settings, flag_format_close),
      offsetof(Settings, flag_format_end),
      offsetof(Settings, flag_format_open),
      offsetof(Settings, flag_group_separator),
      offsetof(Settings, flag_hexdump),
      offsetof(Settings, flag_hyperlink),
      offsetof(Settings, flag_index),
      offsetof(Settings, flag_label),
      offsetof(Settings, flag_pager),
      offsetof(Settings, flag_pretty),
      offsetof(Settings, flag_replace),
      offsetof(Settings, flag_save_config),
      offsetof(Settings, flag_separator),
      offsetof(Settings, flag_separator_dash),
      offsetof(Settings, flag_separator_bar),
      offsetof(Settings, flag_sort),
      offsetof(Settings, flag_stats),
      offsetof(Settings, flag_tag),
      offsetof(Settings, flag_view),
      offsetof(Settings, flag_encoding_type),
    };
    bool ok = ugrep_settings_size() == sizeof(Settings) && ugrep_settings_count() == sizeof(offsets) / sizeof(offsets[0]);
    for (size_t i = 0; ok && i < sizeof(offsets) / sizeof(offsets[0]); ++i)
      ok = ugrep_settings_offset(i) == offsets[i];
    if (!ok)
    {
      fputs("ugrep: internal error: C++ and Rust settings layouts differ\n", stderr);
      abort();
    }
  }
} settings_layout_check;
