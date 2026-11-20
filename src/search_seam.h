// Seam declarations for the Grep search seam (component src_core, src/ugrep.cpp), C linkage.
// The Rust side is rust/src_core/src/ffi/search_host.rs (the table, mirrored field by field) and rust/src_core/src/lib.rs
// (the matcher handle and the branch functions over cxx); the notes are in rust/src_core/SEAM.md ("Grep seam").
// The enums and the order of the table are pinned by tests in search_host.rs and search.rs: change both sides together.
//
// Ownership: the SearchContext is owned by Rust (search_context_new); ugrep_core_search_set_host copies the table, whose
// ctx (the C++ facade of the Grep object) must outlive the context. Matchers stay owned by C++; a handle only borrows:
// Rust calls them through a ugrep_core_matcher_table that the facade fills (one per matcher, copied into the handle).
// Handler lifetime: the handler object registered through set_shift_handler or set_rust_handler is owned by the facade;
// it is destroyed by the next set_shift_handler/set_rust_handler, by reset and with the facade, and the facade never
// leaves the matcher with a pointer to a destroyed handler.
// Threads: every callback is called on the thread that runs Grep::search, never from another thread. Results by pointer
// stay valid until the next callback. Positions are byte offsets from the start of the matcher buffer.
#ifndef SEARCH_SEAM_H
#define SEARCH_SEAM_H

#include <stddef.h>

// C++11 or later (static_assert); this header is included from C++ only.
#define UGREP_CORE_ASSERT(cond) static_assert(cond, #cond)

#ifdef __cplusplus
extern "C" {
#endif

// MatcherKind of a matcher handle (every kind crosses)
#define UGREP_CORE_MATCHER_REFLEX 1 // reflex::Matcher
#define UGREP_CORE_MATCHER_PCRE2  2 // reflex::PCRE2Matcher
#define UGREP_CORE_MATCHER_LINE   3 // reflex::LineMatcher (--match)
#define UGREP_CORE_MATCHER_FUZZY  4 // reflex::FuzzyMatcher (-Z)
#define UGREP_CORE_MATCHER_BOOST  5 // reflex::BoostPerlMatcher

// SearchStep returned by a branch
#define UGREP_CORE_STEP_CONTINUE 0
#define UGREP_CORE_STEP_DONE     1
#define UGREP_CORE_STEP_EXIT     2

// HandlerKind: the buffer-shift handler class set_shift_handler registers on the matcher
#define UGREP_CORE_HANDLER_NONE                 0
#define UGREP_CORE_HANDLER_GREP                 1   // GrepHandler
#define UGREP_CORE_HANDLER_ONLY_MATCHING_CONTEXT 2  // OnlyMatchingContextGrepHandler
#define UGREP_CORE_HANDLER_INVERT_MATCH         3   // InvertMatchGrepHandler
#define UGREP_CORE_HANDLER_FORMAT_INVERT_MATCH  4   // FormatInvertMatchGrepHandler
#define UGREP_CORE_HANDLER_ANY_LINE             5   // AnyLineGrepHandler
#define UGREP_CORE_HANDLER_CONTEXT              6   // ContextGrepHandler
#define UGREP_CORE_HANDLER_INVERT_CONTEXT       7   // InvertContextGrepHandler
#define UGREP_CORE_HANDLER_RUST                 255 // trampoline into matcher_shift_event

// the hex dump modes of Output::Dump (output.hpp), the mode of out_dump_hex
#define UGREP_CORE_HEX_MATCH         0
#define UGREP_CORE_HEX_LINE          1
#define UGREP_CORE_HEX_CONTEXT_MATCH 2
#define UGREP_CORE_HEX_CONTEXT_LINE  3

// restline_data_get / restline_data_set kinds
#define UGREP_CORE_RESTLINE_NONE   0 // restline_data == NULL
#define UGREP_CORE_RESTLINE_BUFFER 1 // offset into the matcher buffer
#define UGREP_CORE_RESTLINE_SAVED  2 // restline.c_str()

// the bool and size_t members of Grep: member_get / member_set
typedef enum ugrep_core_member {
  UGREP_CORE_MEMBER_MATCHES = 0,
  UGREP_CORE_MEMBER_LINENO,
  UGREP_CORE_MEMBER_HEADING,
  UGREP_CORE_MEMBER_STOP,
  UGREP_CORE_MEMBER_BINFILE,
  UGREP_CORE_MEMBER_HEX,
  UGREP_CORE_MEMBER_BINARY,
  UGREP_CORE_MEMBER_RESTLINE_SIZE,
  UGREP_CORE_MEMBER_RESTLINE_LAST
} ugrep_core_member;

// Output operations without argument: out_op
typedef enum ugrep_core_out_op {
  UGREP_CORE_OUT_HOLD = 0,
  UGREP_CORE_OUT_ACQUIRE,
  UGREP_CORE_OUT_RELEASE,
  UGREP_CORE_OUT_CHECK_FLUSH,
  UGREP_CORE_OUT_NL,                  // out.nl()
  UGREP_CORE_OUT_NL_LF,               // out.nl(true)
  UGREP_CORE_OUT_HEADER_FILE,         // out.header(pathname, partname)
  UGREP_CORE_OUT_BINARY_FILE_MATCHES, // out.binary_file_matches(pathname, partname)
  UGREP_CORE_OUT_DUMP_DONE            // out.dump.done()
} ugrep_core_out_op;

// the methods of the registered handler (ContextGrepHandler, InvertContextGrepHandler): handler_op(ctx, op, a, b, c, d)
typedef enum ugrep_core_handler_op {
  UGREP_CORE_HANDLER_OP_OUTPUT_BEFORE_CONTEXT = 0, // output_before_context()
  UGREP_CORE_HANDLER_OP_SET_AFTER_LINENO,          // set_after_lineno(a = lineno)
  UGREP_CORE_HANDLER_OP_ADD_BEFORE_CONTEXT_LINE,   // add_before_context_line(buf + a, buf + b, c = columno, d = offset)
  UGREP_CORE_HANDLER_OP_ADD_BEFORE_CONTEXT_MATCH   // add_before_context_match(a = pos, b = size, c = offset)
} ugrep_core_handler_op;

// the positions of the current match, offsets from the start of the matcher buffer (TextBounds)
typedef struct ugrep_core_text_bounds {
  size_t begin, end, first, last, size, bol, eol, lineno, lines;
} ugrep_core_text_bounds;

// reflex::AbstractMatcher::Context as offsets into the matcher buffer (BufContext)
typedef struct ugrep_core_buf_context {
  size_t offset, len, num;
} ugrep_core_buf_context;

// the Rust shift handler (ShiftFn): called by the trampoline handler for every buffer-shift event
typedef void (*ugrep_core_shift_fn)(void *user, size_t len, size_t num, const unsigned char *buf);

// how Rust calls a C++ matcher (MatcherTable): every function takes ctx first, every field is one word
typedef struct ugrep_core_matcher_table {
  void *ctx;     // the facade's handle of the matcher
  size_t kind;   // UGREP_CORE_MATCHER_*
  int (*find)(void *ctx);
  int (*next_match)(void *ctx, int skip_line);
  size_t (*bol)(void *ctx);
  size_t (*eol)(void *ctx, int inclusive);
  size_t (*begin)(void *ctx);
  size_t (*end)(void *ctx);
  void (*text_bounds)(void *ctx, ugrep_core_text_bounds *out);
  size_t (*first)(void *ctx);
  size_t (*last)(void *ctx);
  size_t (*size)(void *ctx);
  size_t (*lineno)(void *ctx);
  size_t (*lines)(void *ctx);
  size_t (*columno)(void *ctx);
  int (*at_bol)(void *ctx);
  int (*hit_end)(void *ctx);
  int (*skip)(void *ctx, int c);
  size_t (*border)(void *ctx);
  void (*before)(void *ctx, ugrep_core_buf_context *out);
  void (*after)(void *ctx, ugrep_core_buf_context *out);
  size_t (*bef)(void *ctx, size_t len);
  size_t (*aft)(void *ctx, size_t len);
  // buf_ + offset, len readable bytes, valid until the next call into the matcher; NULL unless [offset, offset + len)
  // lies inside the buffer (Rust aborts on NULL rather than read outside it)
  const unsigned char *(*bytes)(void *ctx, size_t offset, size_t len);
  void (*set_reserve)(void *ctx, size_t size);
  void (*fuzzy_distance)(void *ctx, unsigned short max);
  unsigned short (*fuzzy_edits)(void *ctx);
  void (*lineno_skip)(void *ctx, int on);
  // destroy the previous handler, construct and register the UGREP_CORE_HANDLER_* class (NONE: only unregister)
  void (*set_shift_handler)(void *ctx, int handler_kind);
  // destroy the previous handler, register a trampoline handler that calls shift(user, len, num, buf)
  void (*set_rust_handler)(void *ctx, void *user, ugrep_core_shift_fn shift);
  // handler(*matcher, context->buf, context->len, context->num) on the registered handler
  void (*invoke_handler)(void *ctx, const ugrep_core_buf_context *context);
  // matcher->reset(): back to the start of the input, destroys the registered handler
  void (*reset)(void *ctx);
} ugrep_core_matcher_table;

// what stays in C++ and is called back by Rust; the flags are Rust-owned (ugrep_settings) and are not in the table
typedef struct ugrep_core_search_env {
  void *ctx;
  size_t (*member_get)(void *ctx, ugrep_core_member member);
  void (*member_set)(void *ctx, ugrep_core_member member, size_t value);
  // Grep::partname, length through *len
  const unsigned char *(*partname)(void *ctx, size_t *len);
  // output_restline (kind 0) or output_or_save_restline (kind 1)
  void (*restline_output)(void *ctx, int kind, int invert, int hex_context);
  void (*restline_save)(void *ctx);
  // returns UGREP_CORE_RESTLINE_*, the offset in the matcher buffer through *offset for _BUFFER
  int (*restline_data_get)(void *ctx, size_t *offset);
  void (*restline_data_set)(void *ctx, int kind, size_t offset);
  // restline_data += n
  void (*restline_advance)(void *ctx, size_t n);
  // restline_data[0..restline_size], NULL when restline_data is NULL
  const unsigned char *(*restline_bytes)(void *ctx, size_t *len);
  void (*handler_op)(void *ctx, ugrep_core_handler_op op, size_t a, size_t b, size_t c, size_t d);
  // cnf_matching(bol, eol, acquire) and cnf_satisfied(acquire)
  int (*cnf_matching)(void *ctx, size_t bol, size_t eol, int acquire);
  int (*cnf_satisfied)(void *ctx, int acquire);
  // matchers->size(), or (size_t)-1 when matchers is NULL
  size_t (*matchers_len)(void *ctx);
  // matchers->at(index): fills the matcher table, returns 0 when there is none
  int (*matchers_matcher)(void *ctx, size_t index, ugrep_core_matcher_table *table);
  // reset the matching and notmatching bitmasks (--files)
  void (*matchers_reset)(void *ctx);
  int (*out_eof)(void *ctx);
  int (*out_holding)(void *ctx);
  void (*out_op)(void *ctx, ugrep_core_out_op op);
  void (*out_chr)(void *ctx, unsigned char c);
  void (*out_str)(void *ctx, const unsigned char *s, size_t len);
  void (*out_utf8strn)(void *ctx, const unsigned char *s, size_t len, size_t k);
  void (*out_num)(void *ctx, size_t i, int width);
  // out.header(pathname, partname, heading, lineno, matcher, byte_offset, sep, newline)
  void (*out_header)(void *ctx, size_t lineno, size_t byte_offset, const unsigned char *sep, size_t sep_len, int newline);
  // out.format(format, matches)
  void (*out_format_count)(void *ctx, const unsigned char *format, size_t len, size_t matches);
  // out.format(format, pathname, partname, matches, matching, matcher, heading, body, next): *matching in/out, may be NULL
  int (*out_format)(void *ctx, const unsigned char *format, size_t len, size_t matches, size_t *matching, int body, int next);
  // out.dump.hex(mode, byte_offset, data, size)
  void (*out_dump_hex)(void *ctx, short mode, size_t byte_offset, const unsigned char *data, size_t len);
  // out.dump.next(byte_offset)
  void (*out_dump_next)(void *ctx, size_t byte_offset);
  // Stats: what 0 found_file, 1 found_part, 2 undo_found_part; the first two return the bool
  int (*stats_found)(void *ctx, int what);
  size_t (*stats_found_parts)(void *ctx);
  void (*stats_score_matches)(void *ctx, size_t matches, size_t lines);
} ugrep_core_search_env;

// layout pins: the Rust tests pin the same numbers (search.rs, ffi/search_host.rs)
UGREP_CORE_ASSERT(UGREP_CORE_MATCHER_BOOST == 5);
UGREP_CORE_ASSERT(UGREP_CORE_STEP_EXIT == 2);
UGREP_CORE_ASSERT(UGREP_CORE_HANDLER_INVERT_CONTEXT == 7);
UGREP_CORE_ASSERT(UGREP_CORE_HANDLER_RUST == 255);
UGREP_CORE_ASSERT(UGREP_CORE_HEX_CONTEXT_LINE == 3);
UGREP_CORE_ASSERT(UGREP_CORE_RESTLINE_SAVED == 2);
UGREP_CORE_ASSERT(UGREP_CORE_MEMBER_RESTLINE_LAST == 8);
UGREP_CORE_ASSERT(UGREP_CORE_OUT_DUMP_DONE == 8);
UGREP_CORE_ASSERT(UGREP_CORE_HANDLER_OP_ADD_BEFORE_CONTEXT_MATCH == 3);
UGREP_CORE_ASSERT(sizeof(ugrep_core_text_bounds) == 9 * sizeof(size_t));
UGREP_CORE_ASSERT(offsetof(ugrep_core_text_bounds, lines) == 8 * sizeof(size_t));
UGREP_CORE_ASSERT(sizeof(ugrep_core_buf_context) == 3 * sizeof(size_t));
UGREP_CORE_ASSERT(offsetof(ugrep_core_buf_context, num) == 2 * sizeof(size_t));
UGREP_CORE_ASSERT(offsetof(ugrep_core_matcher_table, kind) == 1 * sizeof(size_t));
UGREP_CORE_ASSERT(offsetof(ugrep_core_matcher_table, find) == 2 * sizeof(size_t));
UGREP_CORE_ASSERT(offsetof(ugrep_core_matcher_table, bytes) == 23 * sizeof(size_t));
UGREP_CORE_ASSERT(offsetof(ugrep_core_matcher_table, set_shift_handler) == 28 * sizeof(size_t));
UGREP_CORE_ASSERT(offsetof(ugrep_core_matcher_table, reset) == 31 * sizeof(size_t));
UGREP_CORE_ASSERT(sizeof(ugrep_core_matcher_table) == 32 * sizeof(size_t));
UGREP_CORE_ASSERT(offsetof(ugrep_core_search_env, member_get) == 1 * sizeof(size_t));
UGREP_CORE_ASSERT(offsetof(ugrep_core_search_env, handler_op) == 10 * sizeof(size_t));
UGREP_CORE_ASSERT(offsetof(ugrep_core_search_env, matchers_matcher) == 14 * sizeof(size_t));
UGREP_CORE_ASSERT(offsetof(ugrep_core_search_env, stats_score_matches) == 30 * sizeof(size_t));
UGREP_CORE_ASSERT(sizeof(ugrep_core_search_env) == 31 * sizeof(size_t));

// attach the table to the Rust SearchContext (src_core::SearchContext, passed as an opaque pointer) and read the flags
void ugrep_core_search_set_host(const void *search_context, const ugrep_core_search_env *env);

// one handle per C++ matcher: copies the table (whose ctx must outlive the handle); the result is the MatcherHandle of the
// cxx matcher functions. The free unregisters a Rust shift handler from the matcher first.
void *ugrep_core_matcher_handle_new(const ugrep_core_matcher_table *table);
void ugrep_core_matcher_handle_free(void *handle);

#ifdef __cplusplus
}
#endif

#endif
