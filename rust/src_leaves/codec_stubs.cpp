// Link-time stand-ins for the retained codec libraries, compiled only when build.rs finds no codec libraries (the
// product build links none: D-zopen-switch). Each function reports failure. They are unreachable in that build because
// the Rust zstream is handed a Config without codecs.
#include <cstddef>
#include <cstdint>

extern "C" {
int inflateInit2_(void*, int, const char*, int) { return -6; }
int inflate(void*, int) { return -2; }
int inflateEnd(void*) { return -2; }
int inflateReset(void*) { return -2; }

int BZ2_bzDecompressInit(void*, int, int) { return -2; }
int BZ2_bzDecompress(void*) { return -2; }
int BZ2_bzDecompressEnd(void*) { return -2; }

int lzma_auto_decoder(void*, uint64_t, uint32_t) { return 11; }
int lzma_code(void*, int) { return 11; }
void lzma_end(void*) {}

void* LZ4_createStreamDecode() { return nullptr; }
int LZ4_freeStreamDecode(void*) { return 0; }
int LZ4_decompress_safe_continue(void*, const char*, char*, int, int) { return -1; }

void* ZSTD_createDStream() { return nullptr; }
size_t ZSTD_freeDStream(void*) { return 0; }
size_t ZSTD_initDStream(void*) { return 0; }
size_t ZSTD_decompressStream(void*, void*, void*) { return static_cast<size_t>(-1); }
unsigned ZSTD_isError(size_t code) { return code > static_cast<size_t>(-120) ? 1u : 0u; }
size_t ZSTD_DStreamInSize() { return 0; }

void* viinew(void*) { return nullptr; }
void viifree(void*) {}
int viiget(void*, char*, size_t, int64_t*, uint64_t*) { return -1; }
int viidec(void*, uint8_t*, size_t) { return -1; }
}
