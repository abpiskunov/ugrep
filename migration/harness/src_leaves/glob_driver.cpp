// Seam driver for src_leaves.glob_match: reads seam cases (JSON Lines), calls the glob_match facade
// of src/glob.cpp for each, and lets the tap (CPP2RUST_TAP, CPP2RUST_TAP_OUT) record call and ret.
// Case fields: id, pathname, basename, glob (JSON strings; \u00XX is the raw byte 0xXX, bytes are
// never UTF-8 decoded), icase, lead, path (bools, default false).
// Usage: glob_driver <cases.jsonl>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <map>
#include <string>

#include "../../../rust/src_leaves/cpp2rust_tap.hpp"

bool glob_match(const char *pathname, const char *basename, const char *glob, bool icase, bool lead, bool path);

static std::string parse_string(const std::string& s, size_t& i)
{
  std::string out;
  ++i; // opening quote
  while (i < s.size() && s[i] != '"')
  {
    if (s[i] == '\\')
    {
      ++i;
      switch (s[i])
      {
        case 'u':
          out += static_cast<char>(std::strtol(s.substr(i + 1, 4).c_str(), nullptr, 16));
          i += 4;
          break;
        case 'n': out += '\n'; break;
        case 't': out += '\t'; break;
        default: out += s[i]; break; // \\ \" \/
      }
    }
    else
    {
      out += s[i];
    }
    ++i;
  }
  ++i; // closing quote
  return out;
}

static std::map<std::string, std::string> parse_object(const std::string& s)
{
  std::map<std::string, std::string> m;
  size_t i = 0;
  while (i < s.size())
  {
    if (s[i] != '"')
    {
      ++i;
      continue;
    }
    std::string key = parse_string(s, i);
    while (i < s.size() && (s[i] == ':' || s[i] == ' ')) ++i;
    if (s[i] == '"')
    {
      m[key] = parse_string(s, i);
    }
    else
    {
      size_t j = i;
      while (j < s.size() && s[j] != ',' && s[j] != '}') ++j;
      m[key] = s.substr(i, j - i);
      i = j;
    }
  }
  return m;
}

int main(int argc, char **argv)
{
  if (argc != 2)
  {
    std::fprintf(stderr, "usage: glob_driver cases.jsonl\n");
    return 2;
  }
  std::ifstream in(argv[1], std::ios::binary);
  if (!in)
  {
    std::fprintf(stderr, "cannot open %s\n", argv[1]);
    return 2;
  }
  std::string line;
  while (std::getline(in, line))
  {
    if (line.empty())
      continue;
    auto c = parse_object(line);
    TAP_SET_CASE(c["id"]);
    glob_match(c["pathname"].c_str(), c["basename"].c_str(), c["glob"].c_str(), c["icase"] == "true", c["lead"] == "true", c["path"] == "true");
  }
  return 0;
}
