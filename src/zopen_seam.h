// Declarations of the Rust implementation of zopen (rust/src_leaves, C ABI). The public z_* functions
// of zopen.h are defined by src/zopen_facade.cpp and forward to these.
#ifndef ZOPEN_SEAM_H
#define ZOPEN_SEAM_H

#include <stdio.h>

#ifdef __cplusplus
extern "C" {
#endif

void *ugrep_rust_z_open(FILE *, const char *, int, int);
int ugrep_rust_z_close(void *);
int ugrep_rust_z_read(void *, unsigned char *, int);
int ugrep_rust_z_write(void *, const unsigned char *, int);

#ifdef __cplusplus
}
#endif

#endif
