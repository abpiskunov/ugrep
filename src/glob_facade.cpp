// glob_match of glob.hpp: gitignore-style pathname globbing, implemented in Rust (rust/src_leaves).
// The C++ implementation (src/glob.cpp, Robert van Engelen, Genivia Inc., BSD-3) was removed after the port.
#include "glob.hpp"
#include "glob_rust.hpp"

bool glob_match(const char *pathname, const char *basename, const char *glob, bool icase, bool lead, bool path)
{
  return rust_glob_match(pathname, basename, glob, icase, lead, path);
}
