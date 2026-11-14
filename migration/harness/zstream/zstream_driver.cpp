// Seam driver for src_leaves zstream (src/zstream.hpp through the facade src/zstream.cpp): reads zstream seam cases
// (JSON Lines), runs them and lets the tap (CPP2RUST_TAP, CPP2RUST_TAP_OUT) record the facade calls
// src_leaves.zs_* (predicates, open, close, decompressing, next, zipinfo, zippeek).
//
// Case fields: id; path (the pathname given to open, selects the format by extension); input_hex (the bytes of
// the FILE the stream is opened on); ops (array of strings, run in order on one zstreambuf):
//   is:NAME      the ten predicates is_bz .. is_rar on the pathname NAME (no file)
//   n:LEN        one decompress() into a LEN-byte buffer
//   na:LEN:LIMIT next() of LEN bytes until it returns <= 0 or LIMIT calls
//   z            zipinfo()
//   p            zippeek()
//   Z:LEN        zip/7z loop as zthread does: while (zipinfo()) { next() of LEN bytes until <= 0 }
//   d            decompressing()
//   c            close()
//   o            open(path, FILE) again on the same, rewound FILE
//   sg:N         sgetn (xsgetn) of N bytes, which calls next() for each buffer refill
// The diagnostics of the legacy code (cannot_decompress, warning) are the outgoing calls ugrep.cannot_decompress and
// ugrep.warning, recorded here because the driver provides them. The stream is destroyed at the end of the case
// (which calls close() once more) and the FILE is closed by the driver.
// Usage: zstream_driver <cases.jsonl>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <map>
#include <string>
#include <vector>

#include "../../../rust/src_leaves/cpp2rust_tap.hpp"
#include "../../../src/zstream.hpp"

void cannot_decompress(const char *pathname, const char *message)
{
#ifndef CPP_RUST_IMPL_ZSTREAM
  TAP_OUT("ugrep.cannot_decompress", message != NULL ? message : "");
#endif
  (void)pathname;
}

void warning(const char *message, const char *arg)
{
#ifndef CPP_RUST_IMPL_ZSTREAM
  TAP_OUT("ugrep.warning", message != NULL ? message : "");
#endif
  (void)arg;
}

static std::string parse_string(const std::string& s, size_t& i)
{
  std::string out;
  ++i;
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
        default: out += s[i]; break;
      }
    }
    else
    {
      out += s[i];
    }
    ++i;
  }
  ++i;
  return out;
}

// values: strings, numbers, and arrays of strings (stored as one string per line)
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
    else if (s[i] == '[')
    {
      std::string all;
      ++i;
      while (i < s.size() && s[i] != ']')
      {
        if (s[i] == '"')
          all += parse_string(s, i) + "\n";
        else
          ++i;
      }
      ++i;
      m[key] = all;
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

static std::string from_hex(const std::string& h)
{
  std::string out;
  for (size_t i = 0; i + 1 < h.size(); i += 2)
    out += static_cast<char>(std::strtol(h.substr(i, 2).c_str(), nullptr, 16));
  return out;
}

static std::vector<std::string> split(const std::string& s, char sep)
{
  std::vector<std::string> v;
  size_t p = 0;
  for (;;)
  {
    size_t q = s.find(sep, p);
    v.push_back(s.substr(p, q == std::string::npos ? std::string::npos : q - p));
    if (q == std::string::npos)
      break;
    p = q + 1;
  }
  return v;
}

static void drain(zstreambuf& z, size_t len)
{
  std::vector<unsigned char> buf(len + 1);
  for (int n = 0; n < 100000; ++n)
    if (z.decompress(buf.data(), len) <= 0)
      break;
}

int main(int argc, char **argv)
{
  if (argc != 2)
  {
    std::fprintf(stderr, "usage: zstream_driver cases.jsonl\n");
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
    std::string input = from_hex(c["input_hex"]);
    std::string path = c["path"];
    FILE *file = std::fopen("zstream_case.tmp", "w+b");
    if (!file)
    {
      std::fprintf(stderr, "cannot create zstream_case.tmp\n");
      return 2;
    }
    if (!input.empty())
      std::fwrite(input.data(), 1, input.size(), file);
    std::fflush(file);
    std::fseek(file, 0, SEEK_SET);
    {
      zstreambuf z(path.c_str(), file);
      for (const std::string& op : split(c["ops"], '\n'))
      {
        if (op.empty())
          continue;
        std::vector<std::string> a = split(op, ':');
        if (a[0] == "is")
        {
          const char *p = op.c_str() + 3;
          zstreambuf::is_bz(p);
          zstreambuf::is_xz(p);
          zstreambuf::is_lz4(p);
          zstreambuf::is_zstd(p);
          zstreambuf::is_br(p);
          zstreambuf::is_bz3(p);
          zstreambuf::is_Z(p);
          zstreambuf::is_zip(p);
          zstreambuf::is_7z(p);
          zstreambuf::is_rar(p);
        }
        else if (a[0] == "n")
        {
          size_t len = static_cast<size_t>(std::atoi(a[1].c_str()));
          std::vector<unsigned char> buf(len + 1);
          z.decompress(buf.data(), len);
        }
        else if (a[0] == "na")
        {
          size_t len = static_cast<size_t>(std::atoi(a[1].c_str()));
          std::vector<unsigned char> buf(len + 1);
          for (int n = std::atoi(a[2].c_str()); n > 0; --n)
            if (z.decompress(buf.data(), len) <= 0)
              break;
        }
        else if (a[0] == "z")
        {
          z.zipinfo();
        }
        else if (a[0] == "p")
        {
          z.zippeek();
        }
        else if (a[0] == "Z")
        {
          size_t len = static_cast<size_t>(std::atoi(a[1].c_str()));
          for (int n = 0; n < 1000 && z.zipinfo() != NULL; ++n)
            drain(z, len);
        }
        else if (a[0] == "d")
        {
          z.decompressing();
        }
        else if (a[0] == "c")
        {
          z.close();
        }
        else if (a[0] == "o")
        {
          std::fseek(file, 0, SEEK_SET);
          z.open(path.c_str(), file);
        }
        else if (a[0] == "sg")
        {
          std::vector<char> buf(static_cast<size_t>(std::atoi(a[1].c_str())) + 1);
          z.sgetn(buf.data(), std::atoi(a[1].c_str()));
        }
      }
    }
    std::fclose(file);
  }
  return 0;
}
