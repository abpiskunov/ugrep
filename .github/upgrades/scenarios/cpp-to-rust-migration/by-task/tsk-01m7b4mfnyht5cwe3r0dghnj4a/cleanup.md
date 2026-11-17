# Cleanup report
Per Q-cleanup-1. Deleted: src/glob.cpp, src/zopen.c, core_tap.hpp, indexer_tap.hpp, `_cpp` bodies in cnf/stats/zstream/ugrep-indexer, CPP_RUST_IMPL_* defines, CppRustTap. Kept: facades, retained ugrep/output remainder, indexer_cli, TUI shell, RE-flex, third-party libs.
Gates: switch PASS src_leaves (28), src_core (1579), src_indexer (115); cleanup-form linked-check PASS; policy-scan clean; codec proof 197/0 divergent after deletion (build with /p:RustCargoFeatures=codecs as one property).
Caveats: src_tui switch linked fails by design (retained C++); baseline/unit gates and rust/*-harness replay need the tap and no longer apply; src/Makefile.in needs automake regeneration; rust/*/cpp2rust_tap.hpp and tap Cargo features remain (protected harness).
