// ugrep-indexer: the whole program is Rust (rust/src_indexer); only the C++ entry point stays.

extern "C" int ugrep_indexer_main(int argc, const char **argv);

int main(int argc, const char **argv)
{
  return ugrep_indexer_main(argc, argv);
}
