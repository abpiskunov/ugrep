// Seam declarations of the src_indexer component (src/ugrep-indexer.cpp). See rust/src_indexer/SEAM.md.
//
// The four functions below were the entry points of the units indexer_core (index), indexer_walk (indexer, deleter)
// and indexer_cli (options). Their legacy bodies keep their code and are renamed `<name>_cpp` in
// src/ugrep-indexer.cpp. The facade in src/ugrep-indexer_facade.cpp defines the original names and selects, at build
// time, the legacy body or (CPP_RUST_IMPL_SRC_INDEXER) the Rust implementation of rust/src_indexer.
//
// Everything else (main, load_config, Stream, the flag_* globals, warnings, arg_path) stays owned by C++.
#ifndef UGREP_INDEXER_SEAM_H
#define UGREP_INDEXER_SEAM_H

#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

typedef std::vector<std::string> StrVec;

// input stream of the file being indexed, defined in src/ugrep-indexer.cpp (opaque to the facade)
struct Stream;

// state owned by C++ in src/ugrep-indexer.cpp
extern const char *arg_path;
extern int    flag_accuracy;
extern bool   flag_check;
extern bool   flag_decompress;
extern bool   flag_delete;
extern bool   flag_dereference_files;
extern bool   flag_force;
extern bool   flag_hidden;
extern bool   flag_ignore_binary;
extern bool   flag_no_messages;
extern bool   flag_quiet;
extern bool   flag_usage_warnings;
extern bool   flag_verbose;
extern size_t flag_zmax;
extern StrVec flag_ignore_files;
extern size_t warnings;

// the handle slot a Rust-owned stream state is kept in (CPP_RUST_IMPL_SRC_INDEXER), 0 when none
uint64_t& stream_rust_handle(Stream& stream);

// the archive part name of a stream, written back by the facade after a Rust index() call
std::string& stream_partname(Stream& stream);

// releases the Rust stream state of a stream still holding a handle (called by ~Stream), defined by the facade
void stream_release(Stream& stream);

// legacy bodies, defined in src/ugrep-indexer.cpp
bool index_cpp(Stream& stream, const char *pathname, uint8_t *hashes, size_t& hashes_size, float& noise, bool& compressed, bool& archive, bool& binary, uint64_t& size);
void indexer_cpp(const char *path);
void deleter_cpp(const char *pathname);
void options_cpp(int argc, const char **argv);

// facade, defined in src/ugrep-indexer_facade.cpp: the names the rest of the program calls
bool index(Stream& stream, const char *pathname, uint8_t *hashes, size_t& hashes_size, float& noise, bool& compressed, bool& archive, bool& binary, uint64_t& size);
void indexer(const char *path);
void deleter(const char *pathname);
void options(int argc, const char **argv);

#endif
