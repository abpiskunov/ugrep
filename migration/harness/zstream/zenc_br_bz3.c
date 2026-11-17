/* Encoder helper of the .br/.bz3 zstream corpus generator (D-S6-1): reads stdin, writes a brotli stream (arg "br")
   or a bzip3 file    (arg "bz3", optional block size in bytes, default 131072; bzip3's minimum is 65 KiB) to stdout. The bzip3 file format is that of the bzip3 tool: "BZ3v1", u32 block size, then per
   block u32 compressed size, u32 original size, data (little endian). Built by build-zenc-br-bz3.ps1 against the
   pinned codec libraries. Not part of the product. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <io.h>
#include <fcntl.h>
#include <brotli/encode.h>
#include <libbz3.h>

static void put32(unsigned char *p, unsigned v)
{
  p[0] = v & 255;
  p[1] = (v >> 8) & 255;
  p[2] = (v >> 16) & 255;
  p[3] = (v >> 24) & 255;
}

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
  if (argc > 1 && strcmp(argv[1], "br") == 0)
  {
    size_t bound = BrotliEncoderMaxCompressedSize(n);
    size_t m = bound ? bound : n + 1024;
    unsigned char *out = (unsigned char *)malloc(m);
    if (!BrotliEncoderCompress(5, 22, BROTLI_MODE_GENERIC, n, in, &m, out))
      return 1;
    fwrite(out, 1, m, stdout);
    return 0;
  }
  if (argc > 1 && strcmp(argv[1], "bz3") == 0)
  {
    unsigned bs = argc > 2 ? (unsigned)atoi(argv[2]) : 131072;
    struct bz3_state *st = bz3_new(bs);
    if (!st)
      return 1;
    unsigned char *buf = (unsigned char *)malloc(bz3_bound(bs));
    unsigned char hdr[9] = { 'B', 'Z', '3', 'v', '1' };
    put32(hdr + 5, bs);
    fwrite(hdr, 1, 9, stdout);
    for (size_t off = 0; off < n; off += bs)
    {
      size_t len = n - off < bs ? n - off : bs;
      memcpy(buf, in + off, len);
      int c = bz3_encode_block(st, buf, (int32_t)len);
      if (c < 0)
        return 1;
      unsigned char bh[8];
      put32(bh, (unsigned)c);
      put32(bh + 4, (unsigned)len);
      fwrite(bh, 1, 8, stdout);
      fwrite(buf, 1, (size_t)c, stdout);
    }
    bz3_free(st);
    return 0;
  }
  return 2;
}
