// search_tap.hpp - tap macros for the Grep::search dispatch functions of src/ugrep.cpp (Prepare seam: Grep search).
// Compiled out by default: without the tap build flag every TAP_* macro is a no-op, so the product is unchanged and
// nothing outside the repository is needed. The sink, the macros and their conventions are those of
// rust/src_core/cpp2rust_tap.hpp (one `call`/`ret` pair per dispatch function, neutral encoders for the arguments).
#ifndef SEARCH_TAP_HPP
#define SEARCH_TAP_HPP

#ifdef CPP2RUST_TAP
#include "../rust/src_core/cpp2rust_tap.hpp"
#else
#include <optional>
#include <string>
#define TAP_CALL(fn, ...) ((void)0)
#define TAP_STATE(v) ((void)0)
#define TAP_RET(v) ((void)0)
#define TAP_ERR(v) ((void)0)
#define TAP_OUT(callee, ...) ((void)0)
#define TAP_OUT_RET(callee, v) ((void)0)
#define TAP_OUT_ERR(callee, v) ((void)0)
#define TAP_FAULT(callee) (std::optional<std::string>())
#define TAP_SET_CASE(id) ((void)0)
#define TAP_OBJ(ptr) (std::string())
#endif

#endif
