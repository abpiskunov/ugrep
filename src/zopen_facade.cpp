// Seam facade of the zopen unit (component src_leaves): defines the public z_* functions of
// zopen.h and selects the implementation at build time. CPP_RUST_IMPL_ZOPEN calls the Rust
// implementation (rust/src_leaves, C ABI); otherwise the legacy C bodies of src/zopen.c run.
// Behavior is the same as before the seam: each facade function only forwards its arguments.
#include <stdio.h>
#include <string>

#include "zopen.h"
#include "zopen_seam.h"

#ifdef CPP2RUST_TAP
#include "../rust/src_leaves/cpp2rust_tap.hpp"

// object identity without addresses: "#n" in first-seen order per case (TAP_OBJ without its quotes)
static std::string zopen_tap_obj(const void *p)
{
  if (p == nullptr)
    return "null";
  std::string s = TAP_OBJ(p);
  return s.substr(1, s.size() - 2);
}

static std::string zopen_tap_bytes(const unsigned char *buf, int n)
{
  return n > 0 && buf != nullptr ? std::string(reinterpret_cast<const char*>(buf), static_cast<size_t>(n)) : std::string();
}
#else
#define TAP_CALL(fn, ...) ((void)0)
#define TAP_STATE(v) ((void)0)
#define TAP_RET(v) ((void)0)
#endif

extern "C" {

void *z_open(FILE *file, const char *mode, int bits, int plain)
{
  TAP_CALL("src_leaves.z_open", zopen_tap_obj(file), mode, bits, plain);
#ifdef CPP_RUST_IMPL_ZOPEN
  void *handle = ugrep_rust_z_open(file, mode, bits, plain);
#else
  void *handle = z_open_cpp(file, mode, bits, plain);
#endif
  TAP_RET(zopen_tap_obj(handle));
  return handle;
}

int z_close(void *handle)
{
  TAP_CALL("src_leaves.z_close", zopen_tap_obj(handle));
#ifdef CPP_RUST_IMPL_ZOPEN
  int result = ugrep_rust_z_close(handle);
#else
  int result = z_close_cpp(handle);
#endif
  TAP_RET(result);
  return result;
}

int z_read(void *handle, unsigned char *buf, int len)
{
  TAP_CALL("src_leaves.z_read", zopen_tap_obj(handle), len);
#ifdef CPP_RUST_IMPL_ZOPEN
  int result = ugrep_rust_z_read(handle, buf, len);
#else
  int result = z_read_cpp(handle, buf, len);
#endif
  TAP_STATE(zopen_tap_bytes(buf, result));
  TAP_RET(result);
  return result;
}

int z_write(void *handle, const unsigned char *buf, int len)
{
  TAP_CALL("src_leaves.z_write", zopen_tap_obj(handle), zopen_tap_bytes(buf, len), len);
#ifdef CPP_RUST_IMPL_ZOPEN
  int result = ugrep_rust_z_write(handle, buf, len);
#else
  int result = z_write_cpp(handle, buf, len);
#endif
  TAP_RET(result);
  return result;
}

} // extern "C"
