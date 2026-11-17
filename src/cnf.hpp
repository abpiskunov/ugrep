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
@file      cnf.hpp
@brief     CNF class for normalization of Boolean search queries
@author    Robert van Engelen - engelen@genivia.com
@copyright (c) 2019-2025, Robert van Engelen, Genivia Inc. All rights reserved.
@copyright (c) BSD-3 License - see LICENSE.txt
*/

#ifndef CNF_HPP
#define CNF_HPP

#include "flag.hpp"
#include <cstring>
#include <cctype>
#include <list>
#include <memory>

#ifdef HAVE_CONFIG_H
# include "config.h"
#endif

// normalize Boolean search queries to CNF
class CNF {

 public:

  // a pattern in the CNF is a string or undefined (a NULL smart pointer)
  typedef std::unique_ptr<std::string> Pattern;

  // a term in the CNF is a string/NULL, where the first is an OR pattern (with alternations) or NULL and the rest are ALT-NOT alternate patterns
  typedef std::list<Pattern> Term;

  // a CNF is a collection of terms, an AND-list of ALT-term lists of (NOT-)patterns
  typedef std::list<Term> Terms;

  // pattern mask values to indicate the type of regex pattern argument to populate CNF
  struct PATTERN {
    enum {
      ALT = 0, // -e PATTERN
      NEG = 1, // -N PATTERN
      NOT = 2, // --not [-e] PATTERN
      AND = 4, // to create a new AND-term with empty ALT-list in the CNF
    };
    PATTERN(int mask) : mask(mask) { }
    operator int() const { return mask; }
    int mask;
  };

  // clear the CNF
  void clear()
  {
    terms.clear();
  }

  // return true if CNF has any patterns defined with new_pattern()
  bool defined() const
  {
    return !terms.empty();
  }

  // return true if CNF only defines one empty pattern, after normalization and prune()
  bool empty() const
  {
    return terms.empty() || (terms.size() == 1 && terms.front().empty());
  }

  // return true if CNF is undefined or a singleton with just one pattern w/o ALT-NOT patterns, after prune()
  bool singleton_or_undefined() const
  {
    return terms.empty() || (terms.size() == 1 && terms.front().size() == 1 && terms.front().front());
  }

  // return true if the first ALT-list term is an empty pattern
  bool first_empty() const
  {
    return !terms.empty() && !terms.front().empty() && terms.front().front() && terms.front().front()->empty();
  }

  // add a new ALT-list term to CNF AND-list
  void new_term()
  {
    if (terms.empty())
      terms.emplace_back();

    terms.emplace_back();
  }

  // add an ALT pattern or ALT-NOT pattern, optionally negated (option -N)
  void new_pattern(PATTERN mask, const char *pattern);

  // compile --bool search query into operator tree, normalize to CNF, and populate CNF AND-list of ALT-term lists
  void compile(const char *pattern);

  // return the CNF AND-list of ALT-term lists
  const Terms& lists() const
  {
    return terms;
  }

  // prune empty ALT-terms and ALT-terms with empty patterns that match anything
  void prune();

  // split the patterns at newlines, when present
  void split();

  // report the CNF in readable form
  void report(FILE *output) const;

  // return all ALT-terms of the CNF adjoined
  std::string adjoin() const;

  // return the first ALT-terms of the CNF
  std::string first() const;

  // quote a pattern with \Q and \E
  static void quote(std::string& pattern);

  // anchor a pattern, when specified with -w or -x
  static void anchor(std::string& pattern);

 protected:

  // CNF terms, an AND-list of ALT-term lists of string/NULL patterns
  Terms terms;

};

#endif
