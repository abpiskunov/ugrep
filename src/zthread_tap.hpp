// C++ tap of the Zthread seam (src/ugrep.cpp, struct Zthread facade): TAP_CALL / TAP_RET macros from
// rust/src_core/cpp2rust_tap.hpp and the neutral encoder of a FILE*. Everything compiles to nothing unless CPP2RUST_TAP
// is defined (msvc/rust.targets, /p:CppRustTap=true). The Rust tap, the capture script and the replay harness belong
// to the Behavior task.
#ifndef ZTHREAD_TAP_HPP
#define ZTHREAD_TAP_HPP

#ifdef CPP2RUST_TAP
#include "../rust/src_core/cpp2rust_tap.hpp"   // needs C++17 (msvc/rust.targets sets it for the tap build)
#else
#define TAP_CALL(fn, ...) ((void)0)
#define TAP_RET(v) ((void)0)
#endif

#include <cstdio>
#include <string>

namespace zthread_tap {

// a FILE* is never recorded by address: first-seen object ids per case ("#1", "#2", ...), null for NULL
struct FileRef { const FILE *p; };

inline FileRef file(const FILE *p) { return FileRef{p}; }

#ifdef CPP2RUST_TAP
inline std::string to_neutral(const FileRef& f)
{
  return f.p != NULL ? ::cpp2rust::tap::Sink::get().obj_id(f.p) : std::string("null");
}
#endif

}

#endif
