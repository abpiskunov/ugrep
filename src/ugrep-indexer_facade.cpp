// Seam facade of the src_indexer component (src/ugrep-indexer.cpp): defines index(), indexer(), deleter() and
// options(), the entry functions of the units indexer_core, indexer_walk and indexer_cli. index(), indexer() and
// deleter() call the Rust implementation (rust/src_indexer, cxx); options() runs the retained C++ options_cpp.
// Each facade function hands the flag_* values over as one by-value snapshot and stores the result back.
// Seam notes: rust/src_indexer/SEAM.md.
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <string>

#include "ugrep-indexer_seam.h"
#include "indexer_rust.hpp"

// size of the table buffer of the caller (`uint8_t hashes[65536]`)
static const size_t INDEXER_HASHES_MAX = 65536;

// called by ~Stream: releases the Rust stream state when one is still held
void stream_release(Stream& stream)
{
  uint64_t& handle = stream_rust_handle(stream);
  if (handle != 0)
  {
    ugrep_rust::indexer_stream_drop(handle);
    handle = 0;
  }
}

// index() stays ONE coarse call across the seam: opening, reading, hashing and compressing the table of one file
// (or one archive part) all happen inside it.
bool index(Stream& stream, const char *pathname, uint8_t *hashes, size_t& hashes_size, float& noise, bool& compressed, bool& archive, bool& binary, uint64_t& size)
{
  uint64_t& handle = stream_rust_handle(stream);
  ugrep_rust::IndexerIndexed r = ugrep_rust::indexer_index(indexer_rust::flags(), handle, indexer_rust::to_slice(pathname), archive);
  if (r.hashes.size() > INDEXER_HASHES_MAX)
    abort();
  hashes_size = r.hashes.size();
  for (size_t i = 0; i < hashes_size; ++i)
    hashes[i] = r.hashes[i];
  noise = r.noise;
  compressed = r.compressed;
  archive = r.archive;
  binary = r.binary;
  size = r.size;
  stream_partname(stream) = indexer_rust::to_string(r.partname);
  handle = r.handle;
  return r.ok;
}

// recursively index files; --check ends the process inside the call (the C++ trace then has no return event)
void indexer(const char *path)
{
  ugrep_rust::IndexerExit r = ugrep_rust::indexer_run(indexer_rust::flags(), path != NULL ? indexer_rust::to_slice(path) : indexer_rust::to_slice(""), path != NULL, warnings);
  warnings = r.warnings;
  if (r.exited)
    exit(r.exit_code);
}

// recursively delete index files
void deleter(const char *pathname)
{
  ugrep_rust::IndexerExit r = ugrep_rust::indexer_delete(indexer_rust::flags(), pathname != NULL ? indexer_rust::to_slice(pathname) : indexer_rust::to_slice(""), pathname != NULL, warnings);
  warnings = r.warnings;
  if (r.exited)
    exit(r.exit_code);
}

// parse the command-line options; --help, --version and usage errors end the process inside the call
void options(int argc, const char **argv)
{
  // indexer_cli stays retained C++ (Q-indexer-6): it writes the C++-owned flag_* globals
  options_cpp(argc, argv);
}
