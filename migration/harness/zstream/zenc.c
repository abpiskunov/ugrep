/* Encoder helper of the zstream corpus generator: reads stdin, writes an lz4 frame (arg "lz4") or a zstd frame
   (arg "zstd") to stdout. Built by build-zenc.ps1 against the pinned codec libraries. Not part of the product. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <io.h>
#include <fcntl.h>
#include <lz4frame.h>
#include <zstd.h>

int main(int argc, char **argv)
{
  _setmode(0, _O_BINARY);
  _setmode(1, _O_BINARY);
  size_t cap = 1 << 20, n = 0, r;
  unsigned char *in = (unsigned char *)malloc(cap);
  while ((r = fread(in + n, 1, cap - n, stdin)) > 0)
  {
    n += r;
    if (n == cap)
      in = (unsigned char *)realloc(in, cap *= 2);
  }
  if (argc > 1 && strcmp(argv[1], "lz4") == 0)
  {
    size_t bound = LZ4F_compressFrameBound(n, NULL);
    unsigned char *out = (unsigned char *)malloc(bound);
    size_t m = LZ4F_compressFrame(out, bound, in, n, NULL);
    if (LZ4F_isError(m))
      return 1;
    fwrite(out, 1, m, stdout);
    return 0;
  }
  if (argc > 1 && strcmp(argv[1], "zstd") == 0)
  {
    size_t bound = ZSTD_compressBound(n);
    unsigned char *out = (unsigned char *)malloc(bound);
    size_t m = ZSTD_compress(out, bound, in, n, 3);
    if (ZSTD_isError(m))
      return 1;
    fwrite(out, 1, m, stdout);
    return 0;
  }
  return 2;
}
