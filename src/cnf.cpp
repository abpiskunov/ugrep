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
@file      cnf.cpp
@brief     CNF class for normalization of Boolean search queries
@author    Robert van Engelen - engelen@genivia.com
@copyright (c) 2019-2025, Robert van Engelen, Genivia Inc. All rights reserved.
@copyright (c) BSD-3 License - see LICENSE.txt
*/

#include "ugrep.hpp"
#include "core_rust.hpp"

// The CNF functions are implemented in Rust (rust/src_core); the CNF object stays owned by C++ and the facade runs
// on the caller's thread.

void CNF::compile(const char *pattern)
{
  ugrep_rust::CnfCompiled c = ugrep_rust::cnf_compile(core_rust::to_terms(terms), core_rust::to_slice(pattern), core_rust::cnf_flags());
  core_rust::from_terms(terms, std::move(c.terms));
  if (c.enable_empty)
    flag_empty = true;
}

void CNF::new_pattern(PATTERN mask, const char *pattern)
{
  ugrep_rust::CnfCompiled c = ugrep_rust::cnf_new_pattern(core_rust::to_terms(terms), static_cast<int>(mask), core_rust::to_slice(pattern), core_rust::cnf_flags());
  core_rust::from_terms(terms, std::move(c.terms));
  if (c.enable_empty)
    flag_empty = true;
}

void CNF::prune()
{
  core_rust::from_terms(terms, ugrep_rust::cnf_prune(core_rust::to_terms(terms), core_rust::cnf_flags()));
}

void CNF::split()
{
  core_rust::from_terms(terms, ugrep_rust::cnf_split(core_rust::to_terms(terms), core_rust::cnf_flags()));
}

void CNF::report(FILE *output) const
{
  rust::Vec<uint8_t> text = ugrep_rust::cnf_report(core_rust::to_terms(terms), core_rust::cnf_flags());
  core_rust::write_to(output, text);
}

std::string CNF::adjoin() const
{
  return core_rust::to_string(ugrep_rust::cnf_adjoin(core_rust::to_terms(terms), core_rust::cnf_flags()));
}

std::string CNF::first() const
{
  return core_rust::to_string(ugrep_rust::cnf_first(core_rust::to_terms(terms)));
}

void CNF::quote(std::string& pattern)
{
  pattern = core_rust::to_string(ugrep_rust::cnf_quote(core_rust::to_slice(pattern)));
}

void CNF::anchor(std::string& pattern)
{
  ugrep_rust::CnfAnchored a = ugrep_rust::cnf_anchor(core_rust::to_slice(pattern), core_rust::cnf_flags());
  pattern = core_rust::to_string(a.pattern);
  if (a.enable_empty)
    flag_empty = true; // the only flag this seam writes: the C++ side stays the owner, Rust returns the new value
}
