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
@file      output.cpp
@brief     Output management
@author    Robert van Engelen - engelen@genivia.com
@copyright (c) 2019-2025, Robert van Engelen, Genivia Inc. All rights reserved.
@copyright (c) BSD-3 License - see LICENSE.txt
*/

#include "output.hpp"

#include "output_rust.hpp"

// dump matching data in hex
void Output::Dump::hex(short mode, size_t byte_offset, const char *data, size_t size)
{
  offset = byte_offset;
  while (size > 0)
  {
    bytes[offset++ % flag_hex_columns] = (mode << 8) | *reinterpret_cast<const unsigned char*>(data++);
    if (offset % flag_hex_columns == 0)
      line();
    --size;
  }
}

// get a group capture's string pointer and size specified by %[ARG] as arg, if any
std::pair<const char*,size_t> Output::capture(reflex::AbstractMatcher *matcher, const char *arg)
{
  if (arg != NULL)
  {
    while (true)
    {
      const char *bar = strchr(arg, '|');
      const char *end = strchr(arg, ']');

      if (end == NULL)
        break;

      if (bar == NULL || bar > end)
        bar = end;

      if (isdigit(static_cast<unsigned char>(*arg)))
      {
        size_t index = strtoul(arg, NULL, 10);

        if (index == 0 || *bar == ']')
          return (*matcher)[index];

        std::pair<size_t,const char*> id = matcher->group_id();

        while (id.first != 0 && id.first != index)
          id = matcher->group_next_id();

        if (id.first == index)
          return (*matcher)[index];
      }
      else
      {
        std::pair<size_t,const char*> id = matcher->group_id();

        while (id.first != 0
            && (id.second == NULL || strncmp(id.second, arg, bar - arg) != 0 || id.second[bar - arg] != '\0'))
          id = matcher->group_next_id();

        if (id.first != 0)
          return (*matcher)[id.first];
      }

      if (*bar == ']')
        break;

      arg = bar + 1;
    }
  }

  return std::pair<const char*,size_t>(NULL, 0);
}

// output format with option --format-begin and --format-end
void Output::format_cpp(const char *format, size_t matches)
{
  const char *sep = NULL;
  size_t len = 0;
  const char *s = format;
  while (*s != '\0')
  {
    int width = 0;
    const char *arg = NULL;
    const char *t = s;

    while (*s != '\0' && *s != '%')
      ++s;
    str(t, s - t);
    if (*s == '\0' || *(s + 1) == '\0')
      break;
    ++s;
    if (*s == '{')
    {
      char *r = NULL;
      width = strtol(s + 1, &r, 10);
      if (r != NULL && *r == '}')
        s = r + 1;
    }
    if (*s == '[')
    {
      arg = ++s;
      while (*s != '\0' && *s != ']')
        ++s;
      if (*s == '\0' || *(s + 1) == '\0')
        break;
      ++s;
    }

    int c = *s;

    switch (c)
    {
      case 'T':
        if (flag_initial_tab)
        {
          if (arg)
            str(arg, s - arg - 1);
          chr('\t');
        }
        break;

      case 'S':
        if (matches > 1)
        {
          if (arg)
            str(arg, s - arg - 1);
          if (sep != NULL)
            str(sep, len);
          else
            str(flag_separator);
        }
        break;

      case '$':
        sep = arg;
        if (arg != NULL)
          len = s - arg - 1;
        break;

      case 't':
        chr('\t');
        break;

      case 's':
        if (sep != NULL)
          str(sep, len);
        else
          str(flag_separator);
        break;

      case 'R':
        if (!flag_break)
          break;
        // FALLTHROUGH

      case '~':
#ifdef OS_WIN
        chr('\r');
#endif
        chr('\n');
        break;

      case 'm':
        num(matches, width);
        break;

      case 'U':
        if (arg != NULL)
          wchr(strtol(arg, NULL, 16));
        break;

      case '=':
        color(arg);
        break;

      case '<':
        if (matches <= 1 && arg != NULL)
          str(arg, s - arg - 1);
        break;

      case '>':
        if (matches > 1 && arg != NULL)
          str(arg, s - arg - 1);
        break;

      case ',':
      case ':':
      case ';':
      case '|':
        if (matches > 1)
          chr(c);
        break;

      default:
        chr(c);
    }
    ++s;
  }
}

// output formatted match with options --format, --format-open, --format-close
bool Output::format_cpp(const char *format, const char *pathname, const std::string& partname, size_t matches, size_t *matching, reflex::AbstractMatcher *matcher, bool& heading, bool body, bool next)
{
  if (!body)
    lineno_ = 0;
  else if (lineno_ > 0 && lineno_ == matcher->lineno() && matcher->lines() == 1)
    return false;

  if (matching != NULL)
    ++*matching;

  size_t len = 0;
  const char *sep = NULL;
  const char *s = format;

  while (*s != '\0')
  {
    bool plus = false;
    int width = 0;
    const char *arg = NULL;
    const char *t = s;

    while (*s != '\0' && *s != '%')
      ++s;
    str(t, s - t);
    if (*s == '\0' || *(s + 1) == '\0')
      break;
    ++s;
    if (*s == '{')
    {
      char *r = NULL;
      plus = (s[1] == '+');
      width = strtol(s + 1, &r, 10);
      if (r != NULL && *r == '}')
        s = r + 1;
    }
    if (*s == '[')
    {
      arg = ++s;
      while (*s != '\0' && *s != ']')
        ++s;
      if (*s == '\0' || *(s + 1) == '\0')
        break;
      ++s;
    }

    int c = *s;

    switch (c)
    {
      case '+':
        if (flag_heading)
        {
          if (flag_with_filename)
          {
            if (heading)
            {
              if (arg != NULL)
                str(arg, s - arg - 1);
              str(pathname);
              if (flag_null)
                chr('\0');
              nl();
              heading = false;
            }
            else if (flag_break)
            {
              nl();
            }
          }
        }
        break;

      case 'F':
        if (flag_with_filename && (heading || !partname.empty()))
        {
          if (arg != NULL)
            str(arg, s - arg - 1);
          if (heading)
            str(pathname);
          if (!partname.empty())
          {
            chr('{');
            str(partname);
            chr('}');
          }
          if (flag_null)
            chr('\0');
          else if (sep != NULL)
            str(sep, len);
          else
            str(flag_separator);
        }
        break;

      case 'f':
        str(pathname);
        if (!partname.empty())
        {
          chr('{');
          str(partname);
          chr('}');
        }
        break;

      case 'a':
        {
          const char *basename = strrchr(pathname, PATHSEPCHR);
          if (basename == NULL)
            str(pathname);
          else
            str(basename + 1);
        }
        break;

      case 'p':
        {
          const char *basename = strrchr(pathname, PATHSEPCHR);
          if (basename != NULL)
            str(pathname, basename - pathname);
        }
        break;

      case 'z':
        str(partname);
        break;

      case 'H':
        if (flag_with_filename && (heading || !partname.empty()))
        {
          if (arg != NULL)
            str(arg, s - arg - 1);
          if (!partname.empty())
          {
            std::string name;
            if (heading)
              name = pathname;
            name.push_back('{');
            name.append(partname);
            name.push_back('}');
            quote(name.c_str(), name.size());
          }
          else
          {
            quote(pathname, strlen(pathname));
          }
          if (sep != NULL)
            str(sep, len);
          else
            str(flag_separator);
        }
        break;

      case 'h':
        if (!partname.empty())
        {
          std::string name(pathname);
          name.push_back('{');
          name.append(partname);
          name.push_back('}');
          quote(name.c_str(), name.size());
        }
        else
        {
          quote(pathname, strlen(pathname));
        }
        break;

      case 'I':
        if (flag_with_filename && (heading || !partname.empty()))
        {
          if (arg != NULL)
            str(arg, s - arg - 1);
          if (!partname.empty())
          {
            std::string name;
            if (heading)
              name = pathname;
            name.push_back('{');
            name.append(partname);
            name.push_back('}');
            xml(name.c_str(), name.size());
          }
          else
          {
            xml(pathname, strlen(pathname));
          }
          if (sep != NULL)
            str(sep, len);
          else
            str(flag_separator);
        }
        break;

      case 'i':
        if (!partname.empty())
        {
          std::string name(pathname);
          name.push_back('{');
          name.append(partname);
          name.push_back('}');
          xml(name.c_str(), name.size());
        }
        else
        {
          xml(pathname, strlen(pathname));
        }
        break;

      case 'N':
        if (flag_line_number)
        {
          if (arg != NULL)
            str(arg, s - arg - 1);
          num(matcher->lineno(), (arg == NULL && flag_initial_tab ? 6 : width));
          if (sep != NULL)
            str(sep, len);
          else
            str(flag_separator);
        }
        break;

      case 'n':
        num(matcher->lineno(), width);
        break;

      case 'L':
        num(matcher->lines(), width);
        break;

      case 'l':
        num(matcher->lineno_end(), width);
        break;

      case 'K':
        if (flag_column_number)
        {
          if (arg != NULL)
            str(arg, s - arg - 1);
          num(matcher->columno() + 1, (arg == NULL && flag_initial_tab ? 3 : width));
          if (sep != NULL)
            str(sep, len);
          else
            str(flag_separator);
        }
        break;

      case 'k':
        num(matcher->columno() + 1, width);
        break;

      case 'A':
        hex(matcher->first(), 8);
        chr('-');
        hex(matcher->last() - 1, 8);
        break;

      case 'B':
        if (flag_byte_offset)
        {
          if (arg != NULL)
            str(arg, s - arg - 1);
          num(matcher->first(), width);
          if (sep != NULL)
            str(sep, len);
          else
            str(flag_separator);
        }
        break;

      case 'b':
        if (arg != NULL)
        {
          std::pair<const char*,size_t> cap = capture(matcher, arg);
          if (cap.first != NULL)
            num(cap.first - matcher->text() + matcher->first(), width);
        }
        else
        {
          num(matcher->first(), width);
        }
        break;

      case 'T':
        if (flag_initial_tab)
        {
          if (arg != NULL)
            str(arg, s - arg - 1);
          chr('\t');
        }
        break;

      case 't':
        chr('\t');
        break;

      case 'S':
        if (next)
        {
          if (arg != NULL)
            str(arg, s - arg - 1);
          if (sep != NULL)
            str(sep, len);
          else
            str(flag_separator);
        }
        break;

      case 's':
        if (sep != NULL)
          str(sep, len);
        else
          str(flag_separator);
        break;

      case 'w':
        num(matcher->wsize(), width);
        break;

      case 'd':
        if (arg != NULL)
        {
          std::pair<const char*,size_t> cap = capture(matcher, arg);
          if (cap.first != NULL)
            num(cap.second, width);
        }
        else
        {
          num(matcher->size(), width);
        }
        break;

      case 'e':
        if (arg != NULL)
        {
          std::pair<const char*,size_t> cap = capture(matcher, arg);
          if (cap.first != NULL)
            num(cap.first + cap.second - matcher->text() + matcher->first(), width);
        }
        else
        {
          num(matcher->last(), width);
        }
        break;

      case 'G':
        {
          bool colon = false;
          std::pair<size_t,const char*> id = matcher->group_id();

          while (id.first != 0)
          {
            if (colon)
            {
              if (sep != NULL)
                str(sep, len);
              else
                str(flag_separator);
            }

            colon = true;

            if (arg != NULL)
            {
              size_t n = id.first;
              const char *bar;
              const char *end;

              while (true)
              {
                bar = strchr(arg, '|');
                end = strchr(arg, ']');

                --n;

                if (bar == NULL || (end != NULL && bar > end))
                {
                  bar = end;
                  break;
                }

                if (n == 0)
                  break;

                arg = bar + 1;
              }

              if (n == 0 && bar != NULL)
                str(arg, bar - arg);
              else if (id.second != NULL)
                str(id.second);
              else
                num(id.first);
            }
            else
            {
              if (id.second != NULL)
                str(id.second);
              else
                num(id.first);
            }

            id = matcher->group_next_id();
          }
        }
        break;

      case 'g':
        if (arg != NULL)
        {
          std::pair<size_t,const char*> id = matcher->group_id();
          size_t n = id.first;

          if (n > 0)
          {
            const char *bar;
            const char *end;

            while (true)
            {
              bar = strchr(arg, '|');
              end = strchr(arg, ']');

              --n;

              if (bar == NULL || (end != NULL && bar > end))
              {
                bar = end;
                break;
              }

              if (n == 0)
                break;

              arg = bar + 1;
            }

            if (n == 0 && bar != NULL)
              str(arg, bar - arg);
            else if (id.second != NULL)
              str(id.second);
            else
              num(id.first);
          }
        }
        else
        {
          num(matcher->accept());
        }
        break;

      case 'M':
        num(matches, width);
        break;

      case 'm':
        num(matching == NULL ? matches : *matching, width);
        break;

      case 'O':
        mat(matcher, width);
        break;

      case 'o':
      case '#':
        if (arg != NULL)
        {
          std::pair<const char*,size_t> cap = capture(matcher, arg);
          if (cap.first != NULL)
          {
            const char *p = cap.first;
            size_t n = cap.second;
            if (flag_hex || (flag_with_hex && !reflex::isutf8(p, p + n)))
            {
              if (width > 0 && static_cast<size_t>(width) < n)
                n = width;
              hex(p, n);
            }
            else
            {
              if (width > 0)
                utf8strn(p, n, width);
              else
                str(p, n);
            }
          }
        }
        else
        {
          const char *p = matcher->begin();
          size_t n = matcher->size();
          if (flag_hex || (flag_with_hex && !reflex::isutf8(p, p + n)))
          {
            if (width > 0 && static_cast<size_t>(width) < n)
              n = width;
            hex(p, n);
          }
          else
          {
            if (width != 0)
              p = match_context(matcher, plus, width, n);
            str(p, n);
          }
        }
        break;

      case 'Q':
        quote(matcher, width);
        break;

      case 'q':
        if (arg != NULL)
        {
          std::pair<const char*,size_t> cap = capture(matcher, arg);
          if (cap.first != NULL)
            quote(cap.first, width > 0 ? utf8cut(cap.first, cap.second, width) : cap.second);
        }
        else
        {
          size_t size;
          const char *context = match_context(matcher, plus, width, size);
          quote(context, size);
        }
        break;

      case 'C':
        if (flag_files_with_matches)
          str(flag_invert_match ? "\"false\"" : "\"true\"");
        else if (flag_count)
          chr('"'), num(matches), chr('"');
        else
          cpp(matcher, width);
        break;

      case 'c':
        if (flag_files_with_matches)
        {
          str(flag_invert_match ? "\"false\"" : "\"true\"");
        }
        else if (flag_count)
        {
          chr('"'), num(matches), chr('"');
        }
        else if (arg != NULL)
        {
          std::pair<const char*,size_t> cap = capture(matcher, arg);
          if (cap.first != NULL)
            cpp(cap.first, width > 0 ? utf8cut(cap.first, cap.second, width) : cap.second);
        }
        else
        {
          size_t size;
          const char *context = match_context(matcher, plus, width, size);
          cpp(context, size);
        }
        break;

      case 'V':
        if (flag_files_with_matches)
          str(flag_invert_match ? "false" : "true");
        else if (flag_count)
          num(matches);
        else
          csv(matcher, width);
        break;

      case 'v':
        if (flag_files_with_matches)
        {
          str(flag_invert_match ? "false" : "true");
        }
        else if (flag_count)
        {
          num(matches);
        }
        else if (arg != NULL)
        {
          std::pair<const char*,size_t> cap = capture(matcher, arg);
          if (cap.first != NULL)
            csv(cap.first, width > 0 ? utf8cut(cap.first, cap.second, width) : cap.second);
        }
        else
        {
          size_t size;
          const char *context = match_context(matcher, plus, width, size);
          csv(context, size);
        }
        break;

      case 'J':
        if (flag_files_with_matches)
          str(flag_invert_match ? "false" : "true");
        else if (flag_count)
          num(matches);
        else
          json(matcher, width);
        break;

      case 'j':
        if (flag_files_with_matches)
        {
          str(flag_invert_match ? "false" : "true");
        }
        else if (flag_count)
        {
          num(matches);
        }
        else if (arg != NULL)
        {
          std::pair<const char*,size_t> cap = capture(matcher, arg);
          if (cap.first != NULL)
            json(cap.first, width > 0 ? utf8cut(cap.first, cap.second, width) : cap.second);
        }
        else
        {
          size_t size;
          const char *context = match_context(matcher, plus, width, size);
          json(context, size);
        }
        break;

      case 'X':
        if (flag_files_with_matches)
          str(flag_invert_match ? "false" : "true");
        else if (flag_count)
          num(matches);
        else
          xml(matcher, width);
        break;

      case 'x':
        if (flag_files_with_matches)
        {
          str(flag_invert_match ? "false" : "true");
        }
        else if (flag_count)
        {
          num(matches);
        }
        else if (arg != NULL)
        {
          std::pair<const char*,size_t> cap = capture(matcher, arg);
          if (cap.first != NULL)
            xml(cap.first, width > 0 ? utf8cut(cap.first, cap.second, width) : cap.second);
        }
        else
        {
          size_t size;
          const char *context = match_context(matcher, plus, width, size);
          xml(context, size);
        }
        break;

      case 'Y':
        if (flag_files_with_matches)
          hex(flag_invert_match ? 0 : 1);
        else if (flag_count)
          hex(matches);
        else
          hex(matcher, width);
        break;

      case 'y':
        if (flag_files_with_matches)
        {
          str(flag_invert_match ? "false" : "true");
        }
        else if (flag_count)
        {
          num(matches);
        }
        else if (arg != NULL)
        {
          std::pair<const char*,size_t> cap = capture(matcher, arg);
          if (cap.first != NULL)
            hex(cap.first, width > 0 ? utf8cut(cap.first, cap.second, width) : cap.second);
        }
        else
        {
          size_t size;
          const char *context = match_context(matcher, plus, width, size);
          hex(context, size);
        }
        break;

      case 'Z':
        if (flag_fuzzy > 0)
        {
          if (!flag_match)
          {
            // -Z: we used the fuzzy matcher to search, so a dynamic cast is fine
            reflex::FuzzyMatcher *fuzzy_matcher = dynamic_cast<reflex::FuzzyMatcher*>(matcher);
            if (!flag_files_with_matches && !flag_count)
              num(fuzzy_matcher->edits(), width);
            else
              num(fuzzy_matcher->distance() & 0xff, width);
          }
          else
          {
            // --match all
            chr('0');
          }
        }
        break;

      case 'u':
        if (!flag_ungroup)
          lineno_ = matcher->lineno();
        break;

      case '$':
        sep = arg;
        if (arg != NULL)
          len = s - arg - 1;
        break;

      case 'R':
        if (!flag_break)
          break;
        // FALLTHROUGH

      case '~':
#ifdef OS_WIN
        chr('\r');
#endif
        chr('\n');
        break;

      case 'U':
        if (arg != NULL)
          wchr(strtol(arg, NULL, 16));
        break;

      case '=':
        color(arg);
        break;

      case '<':
        if (!next && arg != NULL)
          str(arg, s - arg - 1);
        break;

      case '>':
        if (next && arg != NULL)
          str(arg, s - arg - 1);
        break;

      case ',':
      case ':':
      case ';':
      case '|':
        if (next)
          chr(c);
        break;

      default:
        chr(c);
        break;

      case '0':
      case '1':
      case '2':
      case '3':
      case '4':
      case '5':
      case '6':
      case '7':
      case '8':
      case '9':
        std::pair<const char*,size_t> capture = (*matcher)[c - '0'];
        str(capture.first, capture.second);
        break;
    }
    ++s;
  }

  return true;
}

// output formatted match with options -v --format
void Output::format_invert_cpp(const char *format, const char *pathname, const std::string& partname, size_t matches, size_t lineno, size_t offset, const char *ptr, size_t size, bool& heading, bool next)
{
  size_t len = 0;
  const char *sep = NULL;
  const char *s = format;

  while (*s != '\0')
  {
    int width = 0;
    const char *arg = NULL;
    const char *t = s;

    while (*s != '\0' && *s != '%')
      ++s;
    str(t, s - t);
    if (*s == '\0' || *(s + 1) == '\0')
      break;
    ++s;
    if (*s == '{')
    {
      char *r = NULL;
      width = strtol(s + 1, &r, 10);
      if (r != NULL && *r == '}')
        s = r + 1;
    }
    if (*s == '[')
    {
      arg = ++s;
      while (*s != '\0' && *s != ']')
        ++s;
      if (*s == '\0' || *(s + 1) == '\0')
        break;
      ++s;
    }

    int c = *s;

    switch (c)
    {
      case '+':
        if (flag_heading)
        {
          if (flag_with_filename)
          {
            if (heading)
            {
              if (arg != NULL)
                str(arg, s - arg - 1);
              str(pathname);
              if (flag_null)
                chr('\0');
              nl();
              heading = false;
            }
            else if (flag_break)
            {
              nl();
            }
          }
        }
        break;

      case 'F':
        if (flag_with_filename && (heading || !partname.empty()))
        {
          if (arg != NULL)
            str(arg, s - arg - 1);
          if (heading)
            str(pathname);
          if (!partname.empty())
          {
            chr('{');
            str(partname);
            chr('}');
          }
          if (flag_null)
            chr('\0');
          else if (sep != NULL)
            str(sep, len);
          else
            str(flag_separator);
        }
        break;

      case 'f':
        str(pathname);
        if (!partname.empty())
        {
          chr('{');
          str(partname);
          chr('}');
        }
        break;

      case 'a':
        {
          const char *basename = strrchr(pathname, PATHSEPCHR);
          if (basename == NULL)
            str(pathname);
          else
            str(basename + 1);
        }
        break;

      case 'p':
        {
          const char *basename = strrchr(pathname, PATHSEPCHR);
          if (basename != NULL)
            str(pathname, basename - pathname);
        }
        break;

      case 'z':
        str(partname);
        break;

      case 'H':
        if (flag_with_filename && (heading || !partname.empty()))
        {
          if (arg != NULL)
            str(arg, s - arg - 1);
          if (!partname.empty())
          {
            std::string name;
            if (heading)
              name = pathname;
            name.push_back('{');
            name.append(partname);
            name.push_back('}');
            quote(name.c_str(), name.size());
          }
          else
          {
            quote(pathname, strlen(pathname));
          }
          if (sep != NULL)
            str(sep, len);
          else
            str(flag_separator);
        }
        break;

      case 'h':
        if (!partname.empty())
        {
          std::string name(pathname);
          name.push_back('{');
          name.append(partname);
          name.push_back('}');
          quote(name.c_str(), name.size());
        }
        else
        {
          quote(pathname, strlen(pathname));
        }
        break;

      case 'I':
        if (flag_with_filename && (heading || !partname.empty()))
        {
          if (arg != NULL)
            str(arg, s - arg - 1);
          if (!partname.empty())
          {
            std::string name;
            if (heading)
              name = pathname;
            name.push_back('{');
            name.append(partname);
            name.push_back('}');
            xml(name.c_str(), name.size());
          }
          else
          {
            xml(pathname, strlen(pathname));
          }
          if (sep != NULL)
            str(sep, len);
          else
            str(flag_separator);
        }
        break;

      case 'i':
        if (!partname.empty())
        {
          std::string name(pathname);
          name.push_back('{');
          name.append(partname);
          name.push_back('}');
          xml(name.c_str(), name.size());
        }
        else
        {
          xml(pathname, strlen(pathname));
        }
        break;

      case 'N':
        if (flag_line_number)
        {
          if (arg != NULL)
            str(arg, s - arg - 1);
          num(lineno, (arg == NULL && flag_initial_tab ? 6 : width));
          if (sep != NULL)
            str(sep, len);
          else
            str(flag_separator);
        }
        break;

      case 'n':
        num(lineno, width);
        break;

      case 'K':
        if (flag_column_number)
        {
          if (arg != NULL)
            str(arg, s - arg - 1);
          chr('1');
          if (sep != NULL)
            str(sep, len);
          else
            str(flag_separator);
        }
        break;

      case 'k':
        chr('1');
        break;

      case 'A':
        hex(offset, 8);
        chr('-');
        hex(offset + size - 1, 8);
        break;

      case 'B':
        if (flag_byte_offset)
        {
          if (arg != NULL)
            str(arg, s - arg - 1);
          num(offset, width);
          if (sep != NULL)
            str(sep, len);
          else
            str(flag_separator);
        }
        break;

      case 'b':
        num(offset, width);
        break;

      case 'T':
        if (flag_initial_tab)
        {
          if (arg != NULL)
            str(arg, s - arg - 1);
          chr('\t');
        }
        break;

      case 't':
        chr('\t');
        break;

      case 'S':
        if (next)
        {
          if (arg != NULL)
            str(arg, s - arg - 1);
          if (sep != NULL)
            str(sep, len);
          else
            str(flag_separator);
        }
        break;

      case 's':
        if (sep != NULL)
          str(sep, len);
        else
          str(flag_separator);
        break;

      case 'w':
      {
        size_t n = 0;
        for (const char *p = ptr; p < ptr + size; ++p)
          n += (*p & 0xC0) != 0x80;
        num(n, width);
        break;
      }

      case 'd':
        num(size, width);
        break;

      case 'e':
        num(offset + size, width);
        break;

      case 'G':
      case 'g':
        break;

      case 'm':
        num(matches, width);
        break;

      case 'O':
      case 'o':
        str(ptr, size);
        break;

      case 'Q':
      case 'q':
        quote(ptr, size);
        break;

      case 'C':
      case 'c':
        if (flag_files_with_matches)
          str(flag_invert_match ? "\"false\"" : "\"true\"");
        else if (flag_count)
          chr('"'), num(matches), chr('"');
        else
          cpp(ptr, size);
        break;

      case 'V':
      case 'v':
        if (flag_files_with_matches)
          str(flag_invert_match ? "false" : "true");
        else if (flag_count)
          num(matches);
        else
          csv(ptr, size);
        break;

      case 'J':
      case 'j':
        if (flag_files_with_matches)
          str(flag_invert_match ? "false" : "true");
        else if (flag_count)
          num(matches);
        else
          json(ptr, size);
        break;

      case 'X':
      case 'x':
        if (flag_files_with_matches)
          str(flag_invert_match ? "false" : "true");
        else if (flag_count)
          num(matches);
        else
          xml(ptr, size);
        break;

      case 'Y':
      case 'y':
        if (flag_files_with_matches)
          str(flag_invert_match ? "false" : "true");
        else if (flag_count)
          num(matches);
        else
          hex(ptr, size);
        break;

      case 'Z':
        break;

      case 'u':
        break;

      case '$':
        sep = arg;
        if (arg != NULL)
          len = s - arg - 1;
        break;

      case 'R':
        if (!flag_break)
          break;
        // FALLTHROUGH

      case '~':
#ifdef OS_WIN
        chr('\r');
#endif
        chr('\n');
        break;

      case 'U':
        if (arg != NULL)
          wchr(strtol(arg, NULL, 16));
        break;

      case '=':
        color(arg);
        break;

      case '<':
        if (!next && arg != NULL)
          str(arg, s - arg - 1);
        break;

      case '>':
        if (next && arg != NULL)
          str(arg, s - arg - 1);
        break;

      case ',':
      case ':':
      case ';':
      case '|':
        if (next)
          chr(c);
        break;

      case '#':
      case '0':
      case '1':
      case '2':
      case '3':
      case '4':
      case '5':
      case '6':
      case '7':
      case '8':
      case '9':
        break;

      default:
        chr(c);
        break;
    }
    ++s;
  }
}

// output a quoted string with escapes for \ and "
void Output::quote_cpp(const char *data, size_t size)
{
  const char *s = data;
  const char *e = data + size;
  const char *t = s;

  chr('"');

  while (s < e)
  {
    if (*s == '\\' || *s == '"')
    {
      str(t, s - t);
      t = s;
      chr('\\');
    }
    ++s;
  }
  str(t, s - t);

  chr('"');
}

// output string in C/C++
void Output::cpp_cpp(const char *data, size_t size)
{
  const char *s = data;
  const char *e = data + size;
  const char *t = s;

  chr('"');

  while (s < e)
  {
    int c = *s;

    if ((c & 0x80) == 0)
    {
      if (c < 0x20 || c == '"' || c == '\\')
      {
        str(t, s - t);
        t = s + 1;

        switch (c)
        {
          case '\b':
            c = 'b';
            break;

          case '\f':
            c = 'f';
            break;

          case '\n':
            c = 'n';
            break;

          case '\r':
            c = 'r';
            break;

          case '\t':
            c = 't';
            break;
        }

        chr('\\');

        if (c > 0x20)
          chr(c);
        else
          oct(c);
      }
    }
    ++s;
  }
  str(t, s - t);

  chr('"');
}

// output quoted string in CSV
void Output::csv_cpp(const char *data, size_t size)
{
  const char *s = data;
  const char *e = data + size;
  const char *t = s;

  chr('"');

  while (s < e)
  {
    int c = *s;

    if ((c & 0x80) == 0)
    {
      if (c == '"')
      {
        str(t, s - t);
        t = s + 1;
        str("\"\"", 2);
      }
      else if ((c < 0x20 && c != '\t') || c == '\\')
      {
        str(t, s - t);
        t = s + 1;

        switch (c)
        {
          case '\b':
            c = 'b';
            break;

          case '\f':
            c = 'f';
            break;

          case '\n':
            c = 'n';
            break;

          case '\r':
            c = 'r';
            break;

          case '\t':
            c = 't';
            break;
        }

        if (c > 0x20)
        {
          chr('\\');
          chr(c);
        }
        else
        {
          str("\\x", 2);
          hex(c, 2);
        }
      }
    }
    ++s;
  }
  str(t, s - t);

  chr('"');
}

// output in hex
void Output::hex_cpp(const char *data, size_t size)
{
  const char *s = data;
  const char *e = data + size;

  if (s < e)
  {
    hex(static_cast<uint8_t>(*s++), 2);
    while (s < e)
    {
      chr(' ');
      hex(static_cast<uint8_t>(*s++), 2);
    }
  }
}

// output quoted string in JSON
void Output::json_cpp(const char *data, size_t size)
{
  const char *s = data;
  const char *e = data + size;
  const char *t = s;

  chr('"');

  while (s < e)
  {
    int c = *s;

    if ((c & 0x80) == 0)
    {
      if (c < 0x20 || c == '"' || c == '\\')
      {
        str(t, s - t);
        t = s + 1;

        switch (c)
        {
          case '\b':
            c = 'b';
            break;

          case '\f':
            c = 'f';
            break;

          case '\n':
            c = 'n';
            break;

          case '\r':
            c = 'r';
            break;

          case '\t':
            c = 't';
            break;
        }

        if (c > 0x20)
        {
          chr('\\');
          chr(c);
        }
        else
        {
          str("\\u", 2);
          hex(c, 4);
        }
      }
    }
    ++s;
  }
  str(t, s - t);

  chr('"');
}

// output in XML
void Output::xml_cpp(const char *data, size_t size)
{
  const char *s = data;
  const char *e = data + size;
  const char *t = s;

  while (s < e)
  {
    int c = *s;

    if ((c & 0x80) == 0)
    {
      switch (c)
      {
        case '&':
          str(t, s - t);
          t = s + 1;
          str("&amp;", 5);
          break;

        case '<':
          str(t, s - t);
          t = s + 1;
          str("&lt;", 4);
          break;

        case '>':
          str(t, s - t);
          t = s + 1;
          str("&gt;", 4);
          break;

        case '"':
          str(t, s - t);
          t = s + 1;
          str("&quot;", 6);
          break;

        case 0x7f:
          str(t, s - t);
          t = s + 1;
          str("&#x7f;", 6);
          break;

        default:
          if (c < 0x20)
          {
            str(t, s - t);
            t = s + 1;
            str("&#", 2);
            num(c);
            chr(';');
          }
      }
    }
    ++s;
  }
  str(t, s - t);
}

const char *Output::Dump::color_hex[4] = { match_ms, color_sl, match_mc, color_cx };

const char *Output::Tree::bar = "|  ";
const char *Output::Tree::ptr = "|_ ";
const char *Output::Tree::end = "~  ";

std::string Output::Tree::path;
int Output::Tree::depth;


// Facade of the output seam (rust/src_core/SEAM.md, section Seam: output): the ported members call Rust, which
// produces the bytes while C++ keeps the buffer.


namespace {

inline ugrep_rust::OutputCtx output_ctx(size_t lineno, bool binary, bool dump_incomplete, const char *pathname)
{
  ugrep_rust::OutputCtx ctx;
  ctx.lineno = lineno;
  ctx.binary = binary;
  ctx.dump_incomplete = dump_incomplete;
  ctx.stdin_label = pathname != NULL && pathname == Static::LABEL_STANDARD_INPUT;
  return ctx;
}

}

void Output::Dump::line()
{
  ugrep_rust::OutputDumpLine r = ugrep_rust::output_dump_line(
      rust::Slice<const int16_t>(bytes, MAX_HEX_COLUMNS),
      rust::Slice<const int16_t>(prevb, MAX_HEX_COLUMNS),
      pstar,
      offset);
  output_rust::append(out, r.bytes);
  pstar = r.pstar;
  if (!r.repeated)
    for (int i = 0; i < MAX_HEX_COLUMNS; ++i)
      prevb[i] = bytes[i];
  for (int i = 0; i < MAX_HEX_COLUMNS; ++i)
    bytes[i] = -1;
}

void Output::color(const char *arg)
{
  output_rust::append(*this, ugrep_rust::output_color(arg != NULL, output_rust::slice(arg)));
}

void Output::header(const char *pathname, const std::string& partname, bool& heading, size_t lineno, reflex::AbstractMatcher *matcher, size_t byte_offset, const char *sep, bool newline)
{
  output_rust::Window window;
  output_rust::Need need;
  need.columno = flag_column_number;
  ugrep_rust::OutputMatch m = output_rust::snapshot(matcher, window, need);
  std::string link = output_rust::link();
  ugrep_rust::OutputArgs args{};
  args.heading = heading;
  args.lineno = lineno;
  args.byte_offset = byte_offset;
  args.has_separator = sep != NULL;
  args.newline = newline;
  ugrep_rust::OutputHeaderResult r = ugrep_rust::output_header_line(
      output_rust::slice(pathname),
      output_rust::slice(partname),
      output_rust::slice(sep),
      output_rust::slice(link),
      args,
      m,
      output_ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), pathname));
  if (r.complete_dump)
    dump.done();
  heading = r.heading;
  output_rust::append(*this, r.bytes);
}

void Output::header(const char *pathname, const std::string& partname)
{
  std::string link = output_rust::link();
  if (flag_tree)
  {
    // acquire lock on output and to access global Tree::path and Tree::depth
    acquire();
    std::string glyphs(Tree::bar);
    glyphs.push_back('\0');
    glyphs.append(Tree::ptr);
    glyphs.push_back('\0');
    glyphs.append(Tree::end);
    ugrep_rust::OutputTreeResult t = ugrep_rust::output_header_tree(
        output_rust::slice(pathname),
        output_rust::slice(partname),
        output_rust::slice(link),
        output_rust::slice(glyphs),
        output_rust::slice(Tree::path),
        Tree::depth,
        output_ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), pathname));
    Tree::path.assign(reinterpret_cast<const char*>(t.path.data()), t.path.size());
    Tree::depth = t.depth;
    output_rust::append(*this, t.bytes);
    return;
  }
  ugrep_rust::OutputHeaderResult r = ugrep_rust::output_header_file(
      output_rust::slice(pathname),
      output_rust::slice(partname),
      output_rust::slice(link),
      output_ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), pathname));
  if (r.complete_dump)
    dump.done();
  output_rust::append(*this, r.bytes);
}

void Output::binary_file_matches(const char *pathname, const std::string& partname)
{
  std::string link = output_rust::link();
  ugrep_rust::OutputBinaryResult r = ugrep_rust::output_binary_file_matches(
      output_rust::slice(pathname),
      output_rust::slice(partname),
      output_rust::slice(link),
      output_ctx(lineno_, (mode_ & BINARY) != 0, dump.incomplete(), pathname));
  output_rust::append(*this, r.bytes);
  if (r.set_binary)
    mode_ |= BINARY;
}

// format, format_invert and the quote/cpp/csv/hex/json/xml emitters are not ported yet (next unit, output-format):
// the rust build keeps their C++ bodies, which call the ported functions above through the facade
void Output::format(const char *format, size_t matches) { format_cpp(format, matches); }

bool Output::format(const char *format, const char *pathname, const std::string& partname, size_t matches, size_t *matching, reflex::AbstractMatcher *matcher, bool& heading, bool body, bool next)
{
  return format_cpp(format, pathname, partname, matches, matching, matcher, heading, body, next);
}

void Output::format_invert(const char *format, const char *pathname, const std::string& partname, size_t matches, size_t lineno, size_t offset, const char *ptr, size_t size, bool& heading, bool next)
{
  format_invert_cpp(format, pathname, partname, matches, lineno, offset, ptr, size, heading, next);
}

void Output::quote(const char *data, size_t size) { quote_cpp(data, size); }
void Output::cpp(const char *data, size_t size) { cpp_cpp(data, size); }
void Output::csv(const char *data, size_t size) { csv_cpp(data, size); }
void Output::hex(const char *data, size_t size) { hex_cpp(data, size); }
void Output::json(const char *data, size_t size) { json_cpp(data, size); }
void Output::xml(const char *data, size_t size) { xml_cpp(data, size); }

// flush a block of data as truncated lines limited to --width columns: the Rust body returns the bytes to write
bool Output::flush_truncated_lines(const char *data, size_t size)
{
  ugrep_rust::OutputTruncResult r = ugrep_rust::output_truncate_lines(
      output_rust::slice(data, size),
      skip_,
      cols_,
      static_cast<uint8_t>(ansi_));
  skip_ = r.skip;
  cols_ = r.cols;
  ansi_ = static_cast<ANSI>(r.ansi);
  return !r.bytes.empty() && fwrite(r.bytes.data(), 1, r.bytes.size(), file) < r.bytes.size();
}