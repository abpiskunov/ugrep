// The public z_* functions of zopen.h (.Z / LZW streams), implemented in Rust (rust/src_leaves, C ABI).
// The C implementation (src/zopen.c) was removed after the port.
#include <stdio.h>

#include "zopen.h"
#include "zopen_seam.h"

extern "C" {

void *z_open(FILE *file, const char *mode, int bits, int plain)
{
  return ugrep_rust_z_open(file, mode, bits, plain);
}

int z_close(void *handle)
{
  return ugrep_rust_z_close(handle);
}

int z_read(void *handle, unsigned char *buf, int len)
{
  return ugrep_rust_z_read(handle, buf, len);
}

int z_write(void *handle, const unsigned char *buf, int len)
{
  return ugrep_rust_z_write(handle, buf, len);
}

} // extern "C"
