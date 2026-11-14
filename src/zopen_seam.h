// Seam declarations for zopen (component src_leaves): the legacy C bodies in src/zopen.c and the
// Rust implementation, both with C linkage. The public z_* functions of zopen.h are defined by the
// facade in src/zopen_facade.cpp, which selects one of them at build time (CPP_RUST_IMPL_ZOPEN).
#ifndef ZOPEN_SEAM_H
#define ZOPEN_SEAM_H

#include <stdio.h>

#ifdef __cplusplus
extern "C" {
#endif

void *z_open_cpp(FILE *, const char *, int, int);
int z_close_cpp(void *);
int z_read_cpp(void *, unsigned char *, int);
int z_write_cpp(void *, const unsigned char *, int);

// defined by rust/src_leaves (C ABI), only referenced when CPP_RUST_IMPL_ZOPEN is defined
void *ugrep_rust_z_open(FILE *, const char *, int, int);
int ugrep_rust_z_close(void *);
int ugrep_rust_z_read(void *, unsigned char *, int);
int ugrep_rust_z_write(void *, const unsigned char *, int);

#ifdef __cplusplus
}
#endif

#endif
