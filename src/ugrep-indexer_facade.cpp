// Seam facade of the src_indexer component (src/ugrep-indexer.cpp): defines index(), indexer(), deleter() and
// options(), the entry functions of the units indexer_core, indexer_walk and indexer_cli, and selects the
// implementation at build time. CPP_RUST_IMPL_SRC_INDEXER calls the Rust implementation (rust/src_indexer, cxx);
// otherwise the legacy bodies `<name>_cpp` of src/ugrep-indexer.cpp run.
// Behavior is the same as before the seam: each facade function only forwards its arguments, handing the
// flag_* values over as one by-value snapshot and storing the result back.
// Switch: CPP_RUST_IMPL_SRC_INDEXER (msvc/rust.props). Seam notes: rust/src_indexer/SEAM.md.
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <string>

#include "ugrep-indexer_seam.h"
#include "indexer_tap.hpp"
#include "indexer_rust.hpp"

// index() stays ONE coarse call across the seam: opening, reading, hashing and compressing the table of one file
// (or one archive part) all happen inside it.
bool index(Stream& stream, const char *pathname, uint8_t *hashes, size_t& hashes_size, float& noise, bool& compressed, bool& archive, bool& binary, uint64_t& size)
{
  TAP_CALL("src_indexer.index", indexer_tap::flags(), pathname, archive);
#ifdef CPP_RUST_IMPL_SRC_INDEXER
  uint64_t& handle = stream_rust_handle(stream);
  ugrep_rust::IndexerIndexed r = ugrep_rust::indexer_index(indexer_rust::flags(), handle, indexer_rust::to_slice(pathname), archive);
  hashes_size = r.hashes.size();
  for (size_t i = 0; i < hashes_size; ++i)
    hashes[i] = r.hashes[i];
  noise = r.noise;
  compressed = r.compressed;
  archive = r.archive;
  binary = r.binary;
  size = r.size;
  handle = r.handle;
  bool result = r.ok;
#else
  bool result = index_cpp(stream, pathname, hashes, hashes_size, noise, compressed, archive, binary, size);
#endif
  TAP_STATE(indexer_tap::indexed(hashes, hashes_size, noise, compressed, archive, binary, size));
  TAP_RET(result);
  return result;
}

// recursively index files; --check ends the process inside the call (the C++ trace then has no return event)
void indexer(const char *path)
{
  TAP_CALL("src_indexer.indexer", indexer_tap::flags(), path, warnings);
#ifdef CPP_RUST_IMPL_SRC_INDEXER
  ugrep_rust::IndexerExit r = ugrep_rust::indexer_run(indexer_rust::flags(), path != NULL ? indexer_rust::to_slice(path) : indexer_rust::to_slice(""), path != NULL, warnings);
  warnings = r.warnings;
  if (r.exited)
    exit(r.exit_code);
#else
  indexer_cpp(path);
#endif
  TAP_STATE(warnings);
  TAP_RET(indexer_tap::none());
}

// recursively delete index files
void deleter(const char *pathname)
{
  TAP_CALL("src_indexer.deleter", indexer_tap::flags(), pathname, warnings);
#ifdef CPP_RUST_IMPL_SRC_INDEXER
  ugrep_rust::IndexerExit r = ugrep_rust::indexer_delete(indexer_rust::flags(), pathname != NULL ? indexer_rust::to_slice(pathname) : indexer_rust::to_slice(""), pathname != NULL, warnings);
  warnings = r.warnings;
  if (r.exited)
    exit(r.exit_code);
#else
  deleter_cpp(pathname);
#endif
  TAP_STATE(warnings);
  TAP_RET(indexer_tap::none());
}

// parse the command-line options; --help, --version and usage errors end the process inside the call
void options(int argc, const char **argv)
{
  TAP_CALL("src_indexer.options", indexer_tap::flags(), argc, indexer_tap::args(argc, argv), arg_path, warnings);
#ifdef CPP_RUST_IMPL_SRC_INDEXER
  rust::Vec<ugrep_rust::IndexerString> args;
  for (int i = 0; i < argc; ++i)
    args.push_back(ugrep_rust::IndexerString{indexer_rust::to_vec(argv[i] != NULL ? argv[i] : "")});
  ugrep_rust::IndexerOptions r = ugrep_rust::indexer_options(indexer_rust::flags(), std::move(args), arg_path != NULL ? indexer_rust::to_slice(arg_path) : indexer_rust::to_slice(""), arg_path != NULL, warnings);
  indexer_rust::apply(r.flags);
  if (r.arg_path_index >= 0 && r.arg_path_index < argc)
    arg_path = argv[r.arg_path_index];
  warnings = r.exit.warnings;
  if (r.exit.exited)
    exit(r.exit.exit_code);
#else
  options_cpp(argc, argv);
#endif
  TAP_STATE(indexer_tap::flags());
  TAP_RET(indexer_tap::none());
}
