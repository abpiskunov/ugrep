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
@file      stats.cpp
@brief     collect global statistics - static, partially thread-safe
@author    Robert van Engelen - engelen@genivia.com
@copyright (c) 2019-2025, Robert van Engelen, Genivia Inc. All rights reserved.
@copyright (c) BSD-3 License - see LICENSE.txt
*/

#include "stats.hpp"
#include "core_rust.hpp"
#include <cstdlib>
#include <cstdio>

// report the statistics (implemented in Rust, rust/src_core)
void Stats::report(FILE *output)
{
  ugrep_rust::StatsInput in = core_rust::stats_input();
  in.searched_files = searched_files();
  in.searched_dirs = searched_dirs();
  in.searched_indexed = searched_indexed();
  in.searched_skipped = searched_skipped();
  in.searched_changed = searched_changed();
  in.searched_added = searched_added();
  in.searched_lines = searched_lines();
  in.found_files = found_files();
  in.found_parts = found_parts();
  in.found_matches = found_matches();
  in.warnings = Static::warnings;
  in.threads = Static::threads;
  in.elapsed_ms = reflex::timer_elapsed(timer);
  in.ignore = core_rust::to_bytes_vec(ignore);
  rust::Vec<uint8_t> text = ugrep_rust::stats_report(in);
  core_rust::write_to(output, text);
}

reflex::timer_type       Stats::timer;
std::atomic_size_t       Stats::files;
std::atomic_size_t       Stats::dirs;
std::atomic_size_t       Stats::indexed;
std::atomic_size_t       Stats::skipped;
std::atomic_size_t       Stats::changed;
std::atomic_size_t       Stats::added;
std::atomic_size_t       Stats::fileno;
std::atomic_size_t       Stats::partno;
std::atomic_size_t       Stats::matchno;
std::atomic_size_t       Stats::lineno;
std::vector<std::string> Stats::ignore;
