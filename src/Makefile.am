# Rust umbrella static library (rust/rust_umbrella), built by cargo; the cxx bridge headers it generates are included
# by the facades. The codecs feature (configure) makes the Rust zstream call the codec libraries on the link line.
RUST_CARGO_DIR = $(abs_top_builddir)/build/cargo
RUST_UMBRELLA = $(RUST_CARGO_DIR)/@RUST_TRIPLE@/release/librust_umbrella.a
RUST_INCLUDES = -I$(RUST_CARGO_DIR)/@RUST_TRIPLE@/cxxbridge
BUILT_SOURCES = rust-umbrella
.PHONY: rust-umbrella
rust-umbrella:
	cd $(abs_top_srcdir)/rust && CPP2RUST_CODECS_FROM_LINKER=1 CPP2RUST_CODEC_LIBS=@RUST_CODEC_LIBS@ $(CARGO) build --release --locked -p rust_umbrella @RUST_FEATURES@ @RUST_TARGET@ --target-dir $(RUST_CARGO_DIR)
$(RUST_UMBRELLA): rust-umbrella
clean-local:
	rm -rf $(RUST_CARGO_DIR)
bin_PROGRAMS = ugrep ugrep-indexer
ugrep_CPPFLAGS = \
	-I$(top_srcdir)/include \
	$(RUST_INCLUDES) \
	$(EXTRA_CFLAGS) \
	$(SIMD_FLAGS) \
	$(PTHREAD_CFLAGS) \
	-DPLATFORM=\"$(PLATFORM)\" \
	-DGREP_PATH=\"$(GREP_PATH)\" \
	-DWITH_NO_INDENT
ugrep_SOURCES = \
	$(top_srcdir)/include/reflex/absmatcher.h \
	$(top_srcdir)/include/reflex/bits.h \
	$(top_srcdir)/include/reflex/convert.h \
	$(top_srcdir)/include/reflex/debug.h \
	$(top_srcdir)/include/reflex/error.h \
	$(top_srcdir)/include/reflex/fuzzymatcher.h \
	$(top_srcdir)/include/reflex/input.h \
	$(top_srcdir)/include/reflex/linematcher.h \
	$(top_srcdir)/include/reflex/matcher.h \
	$(top_srcdir)/include/reflex/pattern.h \
	$(top_srcdir)/include/reflex/pcre2matcher.h \
	$(top_srcdir)/include/reflex/posix.h \
	$(top_srcdir)/include/reflex/ranges.h \
	$(top_srcdir)/include/reflex/setop.h \
	$(top_srcdir)/include/reflex/simd.h \
	$(top_srcdir)/include/reflex/timer.h \
	$(top_srcdir)/include/reflex/traits.h \
	$(top_srcdir)/include/reflex/unicode.h \
	$(top_srcdir)/include/reflex/utf8.h \
	ugrep.cpp \
	cnf.hpp \
	core_rust.hpp \
	cnf.cpp \
	flag.hpp \
	glob.hpp \
	glob_rust.hpp \
	glob_facade.cpp \
	mmap.hpp \
	output.hpp \
	output.cpp \
	query.hpp \
	query.cpp \
	screen.hpp \
	screen.cpp \
	settings.cpp \
	stats.hpp \
	stats.cpp \
	tui_entry.hpp \
	vkey.hpp \
	vkey.cpp \
	ugrep.hpp \
	zstream.hpp \
	zstream_seam.h \
	zthread.hpp \
	zopen.h \
	zopen_facade.cpp \
	zopen_seam.h
ugrep_LDADD = \
	$(top_builddir)/lib/libreflex.a \
	$(RUST_UMBRELLA) \
	$(PTHREAD_LIBS) -ldl -lm
ugrep_DEPENDENCIES = $(top_builddir)/lib/libreflex.a $(RUST_UMBRELLA)
ugrep_indexer_CPPFLAGS = \
	-I$(top_srcdir)/include \
	$(RUST_INCLUDES) \
	$(EXTRA_CFLAGS) \
	$(PTHREAD_CFLAGS) \
	-DPLATFORM=\"$(PLATFORM)\"
ugrep_indexer_SOURCES  = \
	$(top_srcdir)/include/reflex/input.h \
	$(top_srcdir)/include/reflex/utf8.h \
	$(top_srcdir)/lib/input.cpp \
	ugrep-indexer.cpp \
	ugrep-indexer_facade.cpp \
	ugrep-indexer_seam.h \
	indexer_rust.hpp \
	glob.hpp \
	glob_rust.hpp \
	glob_facade.cpp \
	zopen.h \
	zopen_facade.cpp \
	zopen_seam.h \
	zstream.hpp \
	zstream_seam.h \
	zthread.hpp
ugrep_indexer_LDADD = \
	$(RUST_UMBRELLA) \
	$(PTHREAD_LIBS) -ldl -lm
ugrep_indexer_DEPENDENCIES = $(RUST_UMBRELLA)

# zstream.cpp is included by zstream.hpp, not compiled on its own
EXTRA_DIST = zstream.cpp
