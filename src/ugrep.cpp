/******************************************************************************\
* Copyright (c) 2019, Robert van Engelen, Genivia Inc. All rights reserved.    *
*                                                                              *
* Redistribution and use in source and binary forms, with or without           *
* modification, are permitted provided that the following conditions are met:  *
*                                                                              *
*   (1) Redistributions of source code must retain the above copyright notice, *
*       this list of conditions and the following disclaimer.                  *
*                                                                              *
*   (2) Redistributions in binary form must reproduce the above copyright      *
*       notice, this list of conditions and the following disclaimer in the    *
*       documentation and/or other materials provided with the distribution.   *
*                                                                              *
*   (3) The name of the author may not be used to endorse or promote products  *
*       derived from this software without specific prior written permission.  *
*                                                                              *
* THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR IMPLIED *
* WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF         *
* MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO   *
* EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,       *
* SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, *
* PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;  *
* OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,     *
* WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR      *
* OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF       *
* ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.                                   *
\******************************************************************************/

/**
@file      ugrep.cpp
@brief     file pattern searcher
@author    Robert van Engelen - engelen@genivia.com
@copyright (c) 2019-2026, Robert van Engelen, Genivia Inc. All rights reserved.
@copyright (c) BSD-3 License - see LICENSE.txt

User manual:

  <https://ugrep.com>

Source code repository:

  <https://github.com/Genivia/ugrep>

This program uses RE/flex:

  <https://github.com/Genivia/RE-flex>

Optional libraries to support options -P and -z:

  -P: PCRE2 (or Boost.Regex)    recommended
  -z: zlib (.gz)                recommended
  -z: bzip2 (.bz, bz2, .bzip2)  recommended
  -z: lzma (.lzma, .xz)         recommended
  -z: lz4 (.lz4)                optional
  -z: zstd (.zst, .zstd)        optional
  -z: brotli (.br)              optional
  -z: bzip3 (.bz3)              optional

Build ugrep as follows:

  $ ./configure
  $ make -j

Git does not preserve time stamps so ./configure may fail, in that case do:

  $ autoreconf -fi
  $ ./configure
  $ make -j

After this, you may want to test ugrep and install it (optional):

  $ make test
  $ sudo make install

*/

#include "ugrep.hpp"
#include "glob.hpp"
#include "mmap.hpp"
#include "output.hpp"
#include "output_rust.hpp"
#include "tui_entry.hpp"
#include "stats.hpp"

// set_color() and trim() are implemented in Rust (rust/src_core), see rust/src_core/SEAM.md
#include "core_rust.hpp"
#include "zthread_seam.h"
#include "search_seam.h"
#include "zstream_seam.h"
#include <reflex/matcher.h>
#include <reflex/linematcher.h>
#include <reflex/fuzzymatcher.h>
#include <reflex/unicode.h>
#include <iomanip>
#include <cctype>
#include <limits>
#include <functional>
#include <list>
#include <deque>
#include <thread>
#include <memory>
#include <chrono>
#include <sstream>

#ifdef OS_WIN_OR_MINGW // compiling for a windows OS, but not Cygwin

// optionally enable --color=auto by default
// #define WITH_COLOR

// optionally enable PCRE2 for -P
// #define HAVE_PCRE2

// optionally enable Boost.Regex for -P
// #define HAVE_BOOST_REGEX

// optionally enable zlib for -z
// #define HAVE_LIBZ

// optionally enable libbz2 for -z
// #define HAVE_LIBBZ2

// optionally enable liblzma for -z
// #define HAVE_LIBLZMA

// optionally enable liblz4 for -z
// #define HAVE_LIBLZ4

// optionally enable libzstd for -z
// #define HAVE_LIBZSTD

// optionally enable brotli library for -z
// #define HAVE_LIBBROTLI

// optionally enable libbzip3 for -z
// #define HAVE_LIBBZIP3

#include <stringapiset.h>       // internationalization
#include <direct.h>             // directory access
#include <winsock.h>            // gethostname() for --hyperlink
#include <securitybaseapi.h>    // GetTokenInformation()
#include <aclapi.h>             // GetSecurityInfo()

#ifdef OS_WIN
#pragma comment(lib, "Ws2_32.lib")
#endif

#else // not compiling for a windows OS or for windows OS

#include <signal.h>
#include <dirent.h>

#endif

// use PCRE2 for option -P
#ifdef HAVE_PCRE2
# include <reflex/pcre2matcher.h>
#else
// use Boost.Regex for option -P
# ifdef HAVE_BOOST_REGEX
#  include <reflex/boostmatcher.h>
# endif
#endif

// optional: specify an optimal decompression block size, default is 65536, must be larger than 1024 for tar extraction
// #define Z_BUF_LEN 16384
// #define Z_BUF_LEN 32768

// use zlib, libbz2, liblzma etc for option -z
#ifdef HAVE_LIBZ
# define WITH_MAX_7ZIP_SIZE 1073741824 // 7zip expands archived files in memory, we should limit expansion up to 1GB
# include "zstream.hpp"
#endif

// ugrep exit codes
#define EXIT_OK    0 // One or more lines were selected
#define EXIT_FAIL  1 // No lines were selected
#define EXIT_ERROR 2 // An error occurred

// limit the total number of threads spawn (i.e. limit spawn overhead), because grepping is practically IO bound
#ifndef DEFAULT_MAX_JOBS
# define DEFAULT_MAX_JOBS 12
#endif

// default limit on the job queue size to wait for worker threads to finish more work
#ifndef DEFAULT_MAX_JOB_QUEUE_SIZE
# define DEFAULT_MAX_JOB_QUEUE_SIZE 8192
#endif

// a hard limit on the recursive search depth
#ifndef MAX_DEPTH
# define MAX_DEPTH 100
#endif

// --min-steal default, the minimum co-worker's queue size of pending jobs to steal a job from, smaller values result in higher job stealing rates, should not be less than 3
#ifndef MIN_STEAL
# define MIN_STEAL 3U
#endif

// use dirent d_type when available to improve performance
#ifdef HAVE_STRUCT_DIRENT_D_TYPE
# define DIRENT_TYPE_UNKNOWN DT_UNKNOWN
# define DIRENT_TYPE_LNK     DT_LNK
# define DIRENT_TYPE_DIR     DT_DIR
# define DIRENT_TYPE_REG     DT_REG
#else
# define DIRENT_TYPE_UNKNOWN 0
# define DIRENT_TYPE_LNK     1
# define DIRENT_TYPE_DIR     1
# define DIRENT_TYPE_REG     1
#endif

// ANSI SGR substrings extracted from GREP_COLORS






#ifndef OS_WIN_OR_MINGW

// output file stat() result is available and S_ISREG (regular file)
bool Static::output_stat_result  = false;
bool Static::output_stat_regular = false;
struct stat Static::output_stat;

// container of inodes to detect directory cycles when symlinks are traversed with --dereference
std::set<ino_t> visited;

// use statvfs() for --include-fs and --exclude-fs
#if defined(HAVE_STATVFS)

// containers of file system ids to exclude from recursive searches or include in recursive searches
std::set<uint64_t> exclude_fs_ids, include_fs_ids;

// the type of statvfs buffer to use
typedef struct statvfs stat_fs_t;

// wrapper to call statvfs()
inline int stat_fs(const char *path, stat_fs_t *buf)
{
  return statvfs(path, buf);
}

#if defined(_AIX) || defined(__TOS_AIX__)
// NetBSD compatible struct fsid_t with unsigned integers f_fsid.val[2]
inline uint64_t fsid_to_uint64(fsid_t& fsid)
{
  return static_cast<uint64_t>(fsid.val[0]) | (static_cast<uint64_t>(fsid.val[1]) << 32);
}
#else
// POSIX compliant unsigned integer f_fsid
template<typename T>
inline uint64_t fsid_to_uint64(T& fsid)
{
  return static_cast<uint64_t>(fsid);
}
#endif

// if statvfs() is not available then use statfs() for --include-fs and --exclude-fs
#elif defined(HAVE_STATFS)

// containers of file system ids to exclude from recursive searches or include in recursive searches
std::set<uint64_t> exclude_fs_ids, include_fs_ids;

// the type of statfs buffer to use
typedef struct statfs stat_fs_t;

// wrapper to call statfs()
inline int stat_fs(const char *path, stat_fs_t *buf)
{
  return statfs(path, buf);
}

inline uint64_t fsid_to_uint64(fsid_t& fsid)
{
  return static_cast<uint64_t>(fsid.val[0]) | (static_cast<uint64_t>(fsid.val[1]) << 32);
}

#endif

#endif

// ugrep command-line options
// The option state (flag_*, color_*, match_*) is owned by Rust (rust/src_core/src/settings.rs, see flag.hpp);
// only the defaults that depend on the build configuration are assigned here, before main().
static const struct SettingsDefaults {
  SettingsDefaults()
  {
    flag_confirm = DEFAULT_CONFIRM;
    flag_hidden = DEFAULT_HIDDEN;
    flag_delay = DEFAULT_QUERY_DELAY;
    flag_max_mmap = DEFAULT_MAX_MMAP_SIZE;
    flag_max_queue = DEFAULT_MAX_JOB_QUEUE_SIZE;
    flag_min_steal = MIN_STEAL;
    flag_tabs = DEFAULT_TABS;
    flag_color = DEFAULT_COLOR;
    flag_label = Static::LABEL_STANDARD_INPUT;
    flag_pretty = DEFAULT_PRETTY;
  }
} settings_defaults;

// store string arguments and the UTF-8 arguments decoded from wargv[] in strings to re-populate argv[] with pointers
std::list<std::string> arg_strings;

// helper function protos
void options(std::list<std::pair<CNF::PATTERN,const char*>>& pattern_args, int argc, const char **argv);
void init(int argc, const char **argv);
void set_color(const char *colors, const char *parameter, char color[COLORLEN]);
void trim(std::string& line);
void trim_pathname_arg(const char *arg);
bool is_output(ino_t inode);
void import_globs(FILE *file, std::vector<std::string>& files, std::vector<std::string>& dirs, bool gitignore = false, const char *pathname = NULL);
void usage(const char *message, const char *arg = NULL, const char *valid = NULL);
static int core_main(int argc, const char **argv);
void help(const char *what = NULL);
void version();
void is_directory(const char *pathname);
void cannot_decompress(const char *pathname, const char *message);
void open_pager();
void close_pager();

#ifdef OS_WIN_OR_MINGW

// CTRL-C handler
BOOL WINAPI sigint(DWORD signal)
{
  if (signal == CTRL_C_EVENT || signal == CTRL_BREAK_EVENT)
  {
    // be nice, reset colors on interrupt when sending output to a color TTY
    if (flag_color_term)
      flag_color_term = write(1, "\033[m", 3) > 0; // appease -Wunused-result

    close_pager();
  }

  // return FALSE to invoke the next handler (when applicable) or just exit
  return FALSE;
}

#else

// SIGINT and SIGTERM handler
static void sigint(int sig)
{
  // reset to the default handler
  signal(sig, SIG_DFL);

  // be nice, reset colors on interrupt when sending output to a color TTY
  if (flag_color_term)
    flag_color_term = write(1, "\033[m", 3) > 0; // appease -Wunused-result

  close_pager();

  // signal again
  kill(getpid(), sig);
}

#endif

// set this thread's affinity and priority, if supported by the OS, ignore errors to leave scheduling to the OS
static void set_this_thread_affinity_and_priority(size_t cpu)
{
  // set affinity

#if defined(OS_WIN_OR_MINGW) || defined(__CYGWIN__)

  (void)SetThreadAffinityMask(GetCurrentThread(), DWORD_PTR(1) << cpu);

#elif defined(__APPLE__) && defined(HAVE_PTHREAD_SET_QOS_CLASS_SELF_NP)

  (void)pthread_set_qos_class_self_np(QOS_CLASS_USER_INITIATED, 0);

#elif defined(__FreeBSD__) && defined(HAVE_CPUSET_SETAFFINITY)

  cpuset_t cpuset;
  CPU_ZERO(&cpuset);
  CPU_SET(cpu, &cpuset);
  (void)cpuset_setaffinity(CPU_LEVEL_WHICH, CPU_WHICH_TID, -1, sizeof(cpuset), &cpuset);

#elif defined(__DragonFly__) && defined(HAVE_PTHREAD_SETAFFINITY_NP)

  cpuset_t cpuset;
  CPU_ZERO(&cpuset);
  CPU_SET(cpu, &cpuset);
  (void)pthread_setaffinity_np(pthread_self(), sizeof(cpuset), &cpuset);

#elif defined(__NetBSD__) && defined(HAVE_PTHREAD_SETAFFINITY_NP)

  cpuset_t *cpuset = cpuset_create();
  cpuset_set(cpu, cpuset);
  (void)pthread_setaffinity_np(pthread_self(), cpuset_size(cpuset), cpuset);
  cpuset_destroy(cpuset);

#elif defined(HAVE_SCHED_SETAFFINITY)

  cpu_set_t cpuset;
  CPU_ZERO(&cpuset);
  CPU_SET(cpu, &cpuset);
  (void)sched_setaffinity(0, sizeof(cpuset), &cpuset);

#elif defined(HAVE_CPUSET_SETAFFINITY)

  cpu_set_t cpuset;
  CPU_ZERO(&cpuset);
  CPU_SET(cpu, &cpuset);
  (void)cpuset_setaffinity(CPU_LEVEL_WHICH, CPU_WHICH_TID, -1, sizeof(cpuset), &cpuset);

#elif defined(HAVE_PTHREAD_SETAFFINITY_NP)

  cpu_set_t cpuset;
  CPU_ZERO(&cpuset);
  CPU_SET(cpu, &cpuset);
  (void)pthread_setaffinity_np(pthread_self(), sizeof(cpuset), &cpuset);

#endif

  // set priority

#if defined(OS_WIN_OR_MINGW) || defined(__CYGWIN__)

  (void)SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);

#elif defined(__APPLE__)

  (void)setpriority(PRIO_DARWIN_THREAD, 0, -20);

#elif defined(HAVE_PTHREAD_SETSCHEDPRIO)

  (void)pthread_setschedprio(pthread_self(), -20);

#elif defined(HAVE_SETPRIORITY)

  (void)setpriority(PRIO_PROCESS, 0, -20);

#endif

  (void)cpu;
}

// open a pager if output is to a TTY then page through the results
void open_pager()
{
  if (flag_pager != NULL)
  {
    // --pager without argument uses $PAGER or DEFAULT_PAGER_COMMAND
    if (*flag_pager == '\0')
    {
      const char *pager = getenv("PAGER");

      if (pager != NULL && *pager != '\0')
        flag_pager = pager;
      else
        flag_pager = DEFAULT_PAGER_COMMAND;
    }

    // pager 'less' requires -R
    if (strcmp(flag_pager, "less") == 0)
      flag_pager = "less -R";

    // open a pipe to a forked pager
#ifdef OS_WIN
    Static::output = popen(flag_pager, "wb");
#else
    Static::output = popen(flag_pager, "w");
#endif
    if (Static::output == NULL)
      error("cannot open pipe to pager", flag_pager);

    Static::errout = Static::output;

    // enable --heading if not explicitly disabled (enables --break later)
    if (flag_heading.is_undefined())
      flag_heading = true;

    // enable --line-buffered to flush output to the pager immediately
    flag_line_buffered = true;
  }
}

// close the pipe to the forked pager
void close_pager()
{
  if (flag_pager != NULL && Static::output != NULL && Static::output != stdout)
    pclose(Static::output);
}

// open a file where - means stdin (mode "r") or stdout (mode "a" or "w") and an initial ~ expands to home directory
int fopen_smart(FILE **file, const char *filename, const char *mode)
{
  *file = NULL;

  if (filename == NULL || *filename == '\0')
    return errno = ENOENT;

  if (strcmp(filename, "-") == 0)
  {
    *file = strchr(mode, 'r') != NULL ? stdin : stdout;
    return 0;
  }

  if (*filename == '~')
  {
    if (Static::home_dir == NULL)
      return errno = ENOENT;
    return fopenw_s(file, std::string(Static::home_dir).append(filename + 1).c_str(), mode);
  }

  return fopenw_s(file, filename, mode);
}

// read a line from buffered input, returns true when eof
inline bool getline(reflex::BufferedInput& input, std::string& line)
{
  int ch;

  line.erase();

  while ((ch = input.get()) != EOF && ch != '\n' && ch != '\0')
    line.push_back(ch);

  if (!line.empty() && line.back() == '\r')
    line.pop_back();

  return ch == EOF && line.empty();
}

// return true if s[0..n-1] contains a \0 (NUL) or a non-displayable invalid UTF-8 encoding
inline bool is_binary(const char *s, size_t n)
{
  // not --null-data or --encoding=null-data that permit NUL in the input and non-UTF-8 like GNU grep
  if (flag_encoding_type == reflex::Input::file_encoding::null_data)
    return false;

  // not -a and -U or -W: file is binary if it has a \0 (NUL) or an invalid UTF-8 encoding when --encoding is unset
  if (!flag_text && (!flag_binary || flag_with_hex) && flag_encoding_type == reflex::Input::file_encoding::plain)
    return !reflex::isutf8(s, s + n);

  // otherwise, file is binary if it contains a \0 (NUL) which is what GNU grep checks
  return memchr(s, '\0', n) != NULL;
}

// return true if -X or if -W and match s[0..n-1] is binary
inline bool check_binary(const char *s, size_t n)
{
  return (flag_hex || (flag_with_hex && is_binary(s, n)));
}

// check if a file's inode is the current output file, to avoid searching the file we write to
inline bool is_output(ino_t inode)
{
#ifdef OS_WIN_OR_MINGW
  (void)inode;  // appease -Wunused
  return false; // TODO check that two FILE* on Windows are the same, is this possible?
#else
  return Static::output_stat_regular && inode == Static::output_stat.st_ino;
#endif
}

// specify a line of input for the matcher to read, matcher must not use text() or rest() to keep the line contents unmodified
inline void read_line(reflex::AbstractMatcher *matcher, const char *line, size_t size)
{
  // safe cast: buffer() is read-only if no matcher.text() and matcher.rest() are used, size + 1 to include final \0
  matcher->buffer(const_cast<char*>(line), size + 1);
}

// specify a line of input for the matcher to read, matcher must not use text() or rest() to keep the line contents unmodified
inline void read_line(reflex::AbstractMatcher *matcher, const std::string& line)
{
  // safe cast: buffer() is read-only if no matcher.text() and matcher.rest() are used, size + 1 to include final \0
  matcher->buffer(const_cast<char*>(line.c_str()), line.size() + 1);
}

// copy color buffers
inline void copy_color(char to[COLORLEN], const char from[COLORLEN])
{
  size_t len = std::min<size_t>(strlen(from), COLORLEN - 1);

  memcpy(to, from, len);
  to[len] = '\0';

  char *comma = strchr(to, ',');
  if (comma != NULL)
    *comma = '\0';
}

#ifdef HAVE_LIBZ
#ifdef WITH_DECOMPRESSION_THREAD

// the Zthread facade of the Rust decompression thread of rust/src_core (see rust/src_core/SEAM.md), used by Grep
struct Zthread {

  Zthread(bool is_chained, std::string& partname) :
      partnameref(partname),
      handle(NULL)
  {
    ugrep_core_zthread_env env;
    memset(&env, 0, sizeof(env));
    env.ctx = this;
    env.set_partname = &Zthread::set_partname;
    env.warning = &Zthread::warn;
    env.cannot_decompress = &Zthread::cannot_decompress_cb;
    env.archive_found = &Zthread::archive_found;
    env.select_by_flags = &Zthread::select_by_flags;
    // same build configuration as zs_config() in zstream.cpp
    env.zs_codecs = ZS_CODEC_ZLIB;
#ifdef HAVE_LIBBZ2
    env.zs_codecs |= ZS_CODEC_BZ2;
#endif
#ifdef HAVE_LIBLZMA
    env.zs_codecs |= ZS_CODEC_LZMA;
#endif
#ifdef HAVE_LIBLZ4
    env.zs_codecs |= ZS_CODEC_LZ4;
#endif
#ifdef HAVE_LIBZSTD
    env.zs_codecs |= ZS_CODEC_ZSTD;
#endif
#ifdef HAVE_LIBBROTLI
    env.zs_codecs |= ZS_CODEC_BROTLI;
#endif
#ifdef HAVE_LIBBZIP3
    env.zs_codecs |= ZS_CODEC_BZIP3;
#endif
#ifndef WITH_NO_7ZIP
    env.zs_codecs |= ZS_CODEC_7ZIP;
#endif
#ifdef WITH_ZIP_CRC32
    env.zs_zip_crc32 = 1;
#endif
#ifdef WITH_MAX_7ZIP_SIZE
    env.zs_max_7zip_size = WITH_MAX_7ZIP_SIZE;
#endif
    handle = ugrep_core_zthread_new(&env, is_chained ? 1 : 0);
  }

  Zthread(const Zthread&) = delete;
  Zthread& operator=(const Zthread&) = delete;

  ~Zthread()
  {
    close();
  }

  // start decompression thread if not running, open new pipe, returns pipe or NULL on failure
  FILE *start(size_t ztstage, const char *pathname, FILE *file_in, const char *find = NULL)
  {
    FILE *file = NULL;
    int status = ugrep_core_zthread_start(handle, ztstage, pathname, file_in, find, &file);
    return status == ZTHREAD_OK ? file : NULL;
  }

  // open pipe to the next file or part in the archive or return NULL
  FILE *open_next(const char *pathname)
  {
    FILE *file = NULL;
    int status = ugrep_core_zthread_open_next(handle, pathname, &file);
    return status == ZTHREAD_OK ? file : NULL;
  }

  // cancel decompression gracefully
  void cancel()
  {
    ugrep_core_zthread_cancel(handle);
  }

  // join the decompression threads and release their resources, safe to call again (the destructor calls it)
  void close()
  {
    ugrep_core_zthread_close(handle);
    handle = NULL;
  }

private:

  static void set_partname(void *ctx, const char *name, size_t len)
  {
    static_cast<Zthread*>(ctx)->partnameref.assign(name, len);
  }

  static void warn(void *, const char *message, const char *arg)
  {
    warning(message, arg);
  }

  static void cannot_decompress_cb(void *, const char *pathname, const char *message)
  {
    cannot_decompress(pathname, message);
  }

  static void archive_found(void *)
  {
    // produce headers with archive part pathnames (Grep::partname)
    if (!flag_no_filename)
      flag_no_header = false;
  }

  // the flags part of select_matching(): hidden files, --exclude/--include globs and -M magic bytes
  static int select_by_flags(void *, const char *path, const unsigned char *buf, size_t len)
  {
    bool is_selected = true;

    // extract the basename from the path
    const char *basename = strrchr(path, '/');
    if (basename == NULL)
      basename = path;
    else
      ++basename;

    if (*basename == '.' && !flag_hidden)
      return 0;

    // exclude files whose basename matches any one of the --exclude globs
    for (const auto& glob : flag_all_exclude)
    {
      bool ignore_case = &glob < &flag_all_exclude.front() + flag_exclude_iglob_size;
      if (glob_match(path, basename, glob.c_str(), ignore_case))
        return 0;
    }

    // include files whose basename matches any one of the --include globs
    for (const auto& glob : flag_all_include)
    {
      bool ignore_case = &glob < &flag_all_include.front() + flag_include_iglob_size;
      if ((is_selected = glob_match(path, basename, glob.c_str(), ignore_case)))
        break;
    }

    // -M: check magic bytes, buf[len] is readable
    if (buf != NULL && !flag_file_magic.empty() && (flag_all_include.empty() || !is_selected))
    {
      // create a matcher to match the magic pattern, we cannot use magic_matcher because it is not thread safe
      reflex::Matcher magic(Static::magic_pattern);
      magic.buffer(const_cast<char*>(reinterpret_cast<const char*>(buf)), len + 1);
      size_t match = magic.scan();
      is_selected = match == flag_not_magic || match >= flag_min_magic;
    }

    return is_selected ? 1 : 0;
  }

  std::string&  partnameref;   // Grep::partname, assigned by Rust through set_partname()
  void         *handle;        // the Rust decompression thread

};

#endif
#endif

// grep manages output, matcher, input, and decompression
struct Grep {

  // exit search exception
  struct EXIT_SEARCH : public std::exception { };

  // entry type
  enum class Type { SKIP, DIRECTORY, OTHER };

  // entry data extracted from directory contents, moves pathname to this entry
  struct Entry {

    static const uint16_t MIN_COST       = 0;
    static const uint16_t UNDEFINED_COST = 65534;
    static const uint16_t MAX_COST       = 65535;

    Entry(std::string& pathname, ino_t inode, uint64_t info)
      :
        pathname(std::move(pathname)),
        inode(inode),
        info(info),
        cost(UNDEFINED_COST)
    { }

    std::string pathname;
    ino_t       inode;
    uint64_t    info;
    uint16_t    cost;

#ifdef OS_WIN_OR_MINGW

    // get modification time (micro seconds) from directory entry
    static uint64_t modified_time(const WIN32_FIND_DATAW& ffd)
    {
      const struct _FILETIME& time = ffd.ftLastWriteTime;
      return static_cast<uint64_t>(time.dwLowDateTime) | (static_cast<uint64_t>(time.dwHighDateTime) << 32);
    }

    // get modification time (micro seconds) from file handle
    static int64_t modified_time(const HANDLE& hFile)
    {
      struct _FILETIME time;
      GetFileTime(hFile, NULL, NULL, &time);
      return static_cast<uint64_t>(time.dwLowDateTime) | (static_cast<uint64_t>(time.dwHighDateTime) << 32);
    }

#else

    // get sortable info from stat buf
    static uint64_t sort_info(const struct stat& buf)
    {
#if defined(HAVE_STAT_ST_ATIM) && defined(HAVE_STAT_ST_MTIM) && defined(HAVE_STAT_ST_CTIM)
      // tv_sec may be 64 bit, but value is small enough to multiply by 1000000 to fit in 64 bits
      return static_cast<uint64_t>(flag_sort_key == Sort::SIZE ? buf.st_size : flag_sort_key == Sort::USED ? static_cast<uint64_t>(buf.st_atim.tv_sec) * 1000000 + buf.st_atim.tv_nsec / 1000 : flag_sort_key == Sort::CHANGED ? static_cast<uint64_t>(buf.st_mtim.tv_sec) * 1000000 + buf.st_mtim.tv_nsec / 1000 : flag_sort_key == Sort::CREATED ? static_cast<uint64_t>(buf.st_ctim.tv_sec) * 1000000 + buf.st_ctim.tv_nsec / 1000 : 0);
#elif defined(HAVE_STAT_ST_ATIMESPEC) && defined(HAVE_STAT_ST_MTIMESPEC) && defined(HAVE_STAT_ST_CTIMESPEC)
      // tv_sec may be 64 bit, but value is small enough to multiply by 1000000 to fit in 64 bits
      return static_cast<uint64_t>(flag_sort_key == Sort::SIZE ? buf.st_size : flag_sort_key == Sort::USED ? static_cast<uint64_t>(buf.st_atimespec.tv_sec) * 1000000 + buf.st_atimespec.tv_nsec / 1000 : flag_sort_key == Sort::CHANGED ? static_cast<uint64_t>(buf.st_mtimespec.tv_sec) * 1000000 + buf.st_mtimespec.tv_nsec / 1000 : flag_sort_key == Sort::CREATED ? static_cast<uint64_t>(buf.st_ctimespec.tv_sec) * 1000000 + buf.st_ctimespec.tv_nsec / 1000 : 0);
#else
      return static_cast<uint64_t>(flag_sort_key == Sort::SIZE ? buf.st_size : flag_sort_key == Sort::USED ? buf.st_atime : flag_sort_key == Sort::CHANGED ? buf.st_mtime : flag_sort_key == Sort::CREATED ? buf.st_ctime : 0);
#endif
    }

    // get modification time (micro seconds) from stat buf
    static uint64_t modified_time(const struct stat& buf)
    {
#if defined(HAVE_STAT_ST_ATIM) && defined(HAVE_STAT_ST_MTIM) && defined(HAVE_STAT_ST_CTIM)
      // tv_sec may be 64 bit, but value is small enough to multiply by 1000000 to fit in 64 bits
      return static_cast<uint64_t>(static_cast<uint64_t>(buf.st_mtim.tv_sec) * 1000000 + buf.st_mtim.tv_nsec / 1000);
#elif defined(HAVE_STAT_ST_ATIMESPEC) && defined(HAVE_STAT_ST_MTIMESPEC) && defined(HAVE_STAT_ST_CTIMESPEC)
      // tv_sec may be 64 bit, but value is small enough to multiply by 1000000 to fit in 64 bits
      return static_cast<uint64_t>(static_cast<uint64_t>(buf.st_mtimespec.tv_sec) * 1000000 + buf.st_mtimespec.tv_nsec / 1000);
#else
      return static_cast<uint64_t>(buf.st_mtime);
#endif
    }

#endif

    // compare two entries by pathname
    static bool comp_by_path(const Entry& a, const Entry& b)
    {
      return a.pathname < b.pathname;
    }

    // compare two entries by size or time (atime, mtime, or ctime), if equal compare by pathname
    static bool comp_by_info(const Entry& a, const Entry& b)
    {
      return a.info < b.info || (a.info == b.info && a.pathname < b.pathname);
    }

    // compare two entries by edit distance cost
    static bool comp_by_best(const Entry& a, const Entry& b)
    {
      return a.cost < b.cost || (a.cost == b.cost && a.pathname < b.pathname);
    }

    // reverse compare two entries by pathname
    static bool rev_comp_by_path(const Entry& a, const Entry& b)
    {
      return a.pathname > b.pathname;
    }

    // reverse compare two entries by size or time (atime, mtime, or ctime), if equal reverse compare by pathname
    static bool rev_comp_by_info(const Entry& a, const Entry& b)
    {
      return a.info > b.info || (a.info == b.info && a.pathname > b.pathname);
    }

    // reverse compare two entries by edit distance cost
    static bool rev_comp_by_best(const Entry& a, const Entry& b)
    {
      return a.cost > b.cost || (a.cost == b.cost && a.pathname > b.pathname);
    }

  };

  // a job in the job queue
  struct Job {

    // sentinel job NONE
    static const size_t NONE = UNDEFINED_SIZE;

    Job()
      :
        pathname(),
        cost(Entry::UNDEFINED_COST),
        slot(NONE)
    { }

    Job(const char *pathname, uint16_t cost, size_t slot)
      :
        pathname(pathname != Static::LABEL_STANDARD_INPUT ? pathname : ""), // empty pathname means stdin
        cost(cost),
        slot(slot)
    { }

    bool none()
    {
      return slot == NONE;
    }

    std::string pathname;
    uint16_t    cost;
    size_t      slot;

  };

#ifdef WITH_LOCK_FREE_JOB_QUEUE

  // a lock-free job queue for one producer and one consumer with a bounded circular buffer
  struct JobQueue {

    JobQueue()
      :
        head(ring),
        tail(ring),
        todo(0)
    { }

    bool empty() const
    {
      return head.load() == tail.load();
    }

    // add a sentinel NONE job to the queue
    void enqueue()
    {
      enqueue("", Entry::UNDEFINED_COST, Job::NONE);
    }

    // add a job to the queue
    void enqueue(const char *pathname, uint16_t cost, size_t slot)
    {
      Job *job = tail.load();
      Job *next = job + 1;
      if (next == &ring[DEFAULT_MAX_JOB_QUEUE_SIZE])
        next = ring;

      while (next == head.load())
      {
        // we must lock and wait until the buffer is not full
        std::unique_lock<std::mutex> lock(queue_mutex);
        queue_full.wait(lock);
        lock.unlock();
      }

      job->pathname.assign(pathname);
      job->cost = cost;
      job->slot = slot;
      tail.store(next);
      ++todo;
      queue_data.notify_one();
    }

    // try to add a job to the queue if the queue is not too large
    bool try_enqueue(const char *pathname, uint16_t cost, size_t slot)
    {
      Job *job = tail.load();
      Job *next = job + 1;
      if (next == &ring[DEFAULT_MAX_JOB_QUEUE_SIZE])
        next = ring;

      if (next == head.load())
        return false;

      job->pathname.assign(pathname);
      job->cost = cost;
      job->slot = slot;
      tail.store(next);
      ++todo;
      queue_data.notify_one();

      return true;
    }

    // pop a job
    void dequeue(Job& job)
    {
      while (empty())
      {
        // we must lock and wait until the buffer is not empty
        std::unique_lock<std::mutex> lock(queue_mutex);
        queue_data.wait(lock);
        lock.unlock();
      }

      Job *next = head.load() + 1;
      if (next == &ring[DEFAULT_MAX_JOB_QUEUE_SIZE])
        next = ring;

      job = *head.load();
      head.store(next);
      --todo;
      queue_full.notify_one();
    }

    Job                     ring[DEFAULT_MAX_JOB_QUEUE_SIZE];
    std::atomic<Job*>       head;
    std::atomic<Job*>       tail;
    std::mutex              queue_mutex; // job queue mutex used when queue is empty or full
    std::condition_variable queue_data;  // cv to control the job queue
    std::condition_variable queue_full;  // cv to control the job queue
    std::atomic_size_t      todo;        // number of jobs in the queue

  };

#else

  // a job queue
  struct JobQueue : public std::deque<Job> {

    JobQueue()
      :
        todo(0)
    { }

    // add a sentinel NONE job to the queue
    void enqueue()
    {
      std::unique_lock<std::mutex> lock(queue_mutex);
      emplace_back();
      ++todo;
      lock.unlock();

      queue_work.notify_one();
    }

    // add a job to the queue
    void enqueue(const char *pathname, uint16_t cost, size_t slot)
    {
      std::unique_lock<std::mutex> lock(queue_mutex);
      emplace_back(pathname, cost, slot);
      ++todo;
      lock.unlock();

      queue_work.notify_one();
    }

    // pop a job
    void dequeue(Job& job)
    {
      std::unique_lock<std::mutex> lock(queue_mutex);

      while (empty())
        queue_work.wait(lock);

      job = front();
      pop_front();
      --todo;

      // if we popped a Job::NONE sentinel but the queue has some jobs, then move the sentinel to the back of the queue
      if (job.none() && !empty())
      {
        emplace_back();
        job = front();
        pop_front();
      }

      lock.unlock();
    }

    // steal a job from this worker, if at least --min-steal jobs to do, returns true if successful
    bool steal_job(Job& job)
    {
      std::unique_lock<std::mutex> lock(queue_mutex);

      if (empty())
        return false;

      job = front();

      // we cannot steal a Job::NONE sentinel
      if (job.none())
        return false;

      pop_front();
      --todo;

      lock.unlock();

      return true;
    }

    // move a stolen job to this worker, maintaining job slot order
    void move_job(Job& job)
    {
      bool inserted = false;

      std::unique_lock<std::mutex> lock(queue_mutex);

      // insert job in the small queue to maintain job order
      const auto e = end();
      for (auto j = begin(); j != e; ++j)
      {
        if (j->slot > job.slot)
        {
          insert(j, std::move(job));
          inserted = true;
          break;
        }
      }

      if (!inserted)
        emplace_back(std::move(job));

      ++todo;

      lock.unlock();

      queue_work.notify_one();
    }

    std::mutex              queue_mutex; // job queue mutex
    std::condition_variable queue_work;  // cv to control the job queue
    std::atomic_size_t      todo;        // number of jobs in the queue, atomic for job stealing

  };

#endif

  // extend the reflex::Input::Handler to handle detection of binary input like GNU grep (check for NULs)
  struct BinaryHandler : public reflex::Input::Handler {

    BinaryHandler(Grep& grep)
      :
        grep(grep)
    { }

    Grep& grep;

    size_t operator()(FILE*, char *buf, size_t len)
    {
      // simpler form of binary file detection without Unicode UTF-8 validation, check for NULs like GNU grep
      grep.binfile = grep.binfile || (memchr(buf, 0, len) != NULL);

      return len;
    }

  };

#ifndef OS_WIN

  // extend the reflex::Input::Handler to handle non-blocking input from a character device (TTY) or from a pipe
  struct StdInHandler : public reflex::Input::Handler {

    StdInHandler(Grep& grep)
      :
        grep(grep)
    { }

    Grep& grep;

    size_t operator()(FILE*, char *buf, size_t len)
    {
      if (len == 0)
      {
        // could not read input data, but we aren't at EOF yet (i.e. non-blocking read returned zero)

        // invoke buffer shift GrepHandler when set, to output the rest of a matching line and its context
        grep.matcher->handle();

        // flush output
        grep.out.try_flush();
      }
      else if (!flag_text && !flag_hex && !flag_with_hex && flag_encoding_type != reflex::Input::file_encoding::null_data && !grep.binfile)
      {
        // simpler form of binary file detection without Unicode UTF-8 validation, check for NULs like GNU grep
        grep.binfile = (memchr(buf, 0, len) != NULL);
      }

      return len;
    }

  };

#endif

  // extend the reflex::AbstractMatcher::Handler with a grep object reference and references to some of the grep::search locals
  struct GrepHandler : public reflex::AbstractMatcher::Handler {

    GrepHandler(Grep& grep, const char *& pathname)
      :
        grep(grep),
        pathname(pathname)
    { }

    Grep&         grep;     // grep object
    const char *& pathname; // grep::search() argument pathname reference (pointer may be changed)

    // define a reflex::AbstractMatcher functor to handle buffer shifts
    virtual void operator()(reflex::AbstractMatcher&, const char*, size_t, size_t) override
    {
      grep.output_or_save_restline(false, flag_hex_after > 0);
    }

    // get the start of the after/before context lines, if present
    void begin_before(reflex::AbstractMatcher& matcher, const char *buf, size_t len, size_t num, const char *& ptr, size_t& size, size_t& offset)
    {
      ptr = NULL;
      size = 0;
      offset = 0;

      // if no gap to shift then return (this also happens when the buffer grows)
      if (len == 0)
        return;

      size_t current = matcher.lineno();
      size_t between = current - grep.lineno;

      if (between > 1)
      {
        const char *s = buf + len;
        const char *e = s;

        if (buf[len - 1] != '\n')
          --between;

        while (--s >= buf)
        {
          if (*s == '\n')
          {
            if (--between == 0)
              break;;
            e = s + 1;
          }
        }

        ptr = ++s;
        size = e - s;
        offset = s - buf + num;

        ++grep.lineno;
      }
    }

    // advance to the next before context line, if present
    void next_before(const char *buf, size_t len, size_t num, const char *& ptr, size_t& size, size_t& offset)
    {
      if (ptr == NULL)
        return;

      const char *pos = ptr + size;
      const char *end = buf + len;

      if (pos >= end)
      {
        ptr = NULL;
      }
      else
      {
        const char *eol = static_cast<const char*>(memchr(pos, '\n', end - pos));

        if (eol == NULL)
          eol = buf + len;
        else
          ++eol;

        ptr = pos;
        size = eol - pos;
        offset = pos - buf + num;

        ++grep.lineno;
      }
    }

  };

  // extend event GrepHandler for --only-matching with context
  struct OnlyMatchingContextGrepHandler : public GrepHandler {

   OnlyMatchingContextGrepHandler(Grep& grep, const char *& pathname)
      :
        GrepHandler(grep, pathname)
    { }

    // define a reflex::AbstractMatcher functor to handle buffer shifts by saving the restline used for context
    virtual void operator()(reflex::AbstractMatcher&, const char*, size_t, size_t) override
    {
      grep.save_restline();
    }

  };

  // extend event GrepHandler to output invert match lines for -v
  struct InvertMatchGrepHandler : public GrepHandler {

    InvertMatchGrepHandler(Grep& grep, const char *& pathname)
      :
        GrepHandler(grep, pathname)
    { }

    // functor invoked by the reflex::AbstractMatcher when the buffer contents are shifted out, also called explicitly in grep::search
    virtual void operator()(reflex::AbstractMatcher& matcher, const char *buf, size_t len, size_t num) override
    {
      const char *ptr;
      size_t size;
      size_t offset;

      // if no gap to shift buffer, then let gap span the buffer until the begin of the matching line (bol)
      if (len == 0)
        len = matcher.before().len;

      begin_before(matcher, buf, len, num, ptr, size, offset);

      while (ptr != NULL)
      {
        // --range: max line exceeded?
        if (flag_max_line > 0 && grep.lineno > flag_max_line)
          break;

        // --max-files: max reached?
        if (grep.matches == 0 && !Stats::found_part())
        {
          grep.stop = true;
          break;
        }

        // --max-count: max number of matches reached?
        if (flag_max_count > 0 && grep.matches >= flag_max_count)
          break;

        // output blocked?
        if (grep.out.eof)
          break;

        ++grep.matches;

        if (flag_with_hex)
          grep.binary = false;

        grep.binary = grep.binary || check_binary(ptr, size);

        if (grep.binfile || (grep.binary && !flag_hex && !flag_with_hex))
          break;

        if (grep.hex && !grep.binary)
          grep.out.dump.done();

        if (!flag_no_header)
          grep.out.header(pathname, grep.partname, grep.heading, grep.lineno, NULL, offset, flag_separator, grep.binary);

        grep.hex = grep.binary;

        if (grep.binary)
        {
          grep.out.dump.hex(Output::Dump::HEX_LINE, offset, ptr, size);
        }
        else
        {
          bool lf_only = false;
          if (size > 0)
          {
            lf_only = ptr[size - 1] == '\n';
            size_t sizen = size - lf_only;
            if (sizen > 0)
            {
              grep.out.str(color_sl);
              grep.out.str(ptr, sizen);
              grep.out.str(color_off);
            }
          }
          grep.out.nl(lf_only);
        }

        next_before(buf, len, num, ptr, size, offset);
      }
    }

  };

  // extend event GrepHandler to output formatted invert match lines for --format -v
  struct FormatInvertMatchGrepHandler : public GrepHandler {

    FormatInvertMatchGrepHandler(Grep& grep, const char *& pathname)
      :
        GrepHandler(grep, pathname)
    { }

    // functor invoked by the reflex::AbstractMatcher when the buffer contents are shifted out, also called explicitly in grep::search
    virtual void operator()(reflex::AbstractMatcher& matcher, const char *buf, size_t len, size_t num) override
    {
      const char *ptr;
      size_t size;
      size_t offset;

      // if no gap to shift buffer, then let gap span the buffer until the begin of the matching line (bol)
      if (len == 0)
        len = matcher.before().len;

      begin_before(matcher, buf, len, num, ptr, size, offset);

      while (ptr != NULL)
      {
        // --range: max line exceeded?
        if (flag_max_line > 0 && grep.lineno > flag_max_line)
          break;

        // output --format-open
        if (grep.matches == 0)
        {
          // --format-open or --format-close: we must out.acquire() lock before Stats::found_part()
          if (flag_format_open != NULL || flag_format_close != NULL)
            grep.out.acquire();

          // --max-files: max reached?
          if (!Stats::found_part())
          {
            grep.stop = true;
            break;
          }

          if (flag_format_open != NULL)
            grep.out.format(flag_format_open, pathname, grep.partname, Stats::found_parts(), NULL, &matcher, grep.heading, false, Stats::found_parts() > 1);
        }

        // --max-count: max number of matches reached?
        if (flag_max_count > 0 && grep.matches >= flag_max_count)
          break;

        // output blocked?
        if (grep.out.eof)
          break;

        ++grep.matches;

        // output --format
        grep.out.format_invert(flag_format, pathname, grep.partname, grep.matches, grep.lineno, offset, ptr, size - (size > 0 && ptr[size - 1] == '\n'), grep.heading, grep.matches > 1);

        next_before(buf, len, num, ptr, size, offset);
      }
    }

  };

  // extend event GrepHandler to output any context lines for -y
  struct AnyLineGrepHandler : public GrepHandler {

    AnyLineGrepHandler(Grep& grep, const char *& pathname)
      :
        GrepHandler(grep, pathname)
    { }

    // functor invoked by the reflex::AbstractMatcher when the buffer contents are shifted out, also called explicitly in grep::search
    virtual void operator()(reflex::AbstractMatcher& matcher, const char *buf, size_t len, size_t num) override
    {
      const char *ptr;
      size_t size;
      size_t offset;

      // if no gap to shift buffer, then let gap span the buffer until the begin of the matching line (bol)
      if (len == 0)
        len = matcher.before().len;

      begin_before(matcher, buf, len, num, ptr, size, offset);

      grep.output_or_save_restline(flag_invert_match);

      // context colors with or without -v
      short v_hex_context_line = flag_invert_match ? Output::Dump::HEX_LINE : Output::Dump::HEX_CONTEXT_LINE;
      const char *v_color_cx = flag_invert_match ? color_sl : color_cx;
      const char *separator = flag_invert_match ? flag_separator : flag_separator_dash;

      while (ptr != NULL)
      {
        // --range: max line exceeded?
        if (flag_max_line > 0 && grep.lineno > flag_max_line)
          break;

        if (grep.matches == 0 && flag_invert_match)
        {
          // --max-files: max reached?
          if (!Stats::found_part())
          {
            grep.stop = true;
            break;
          }
        }

        // --max-count: max number of matches reached?
        if (flag_invert_match && flag_max_count > 0 && grep.matches >= flag_max_count)
        {
          grep.stop = true;
          break;
        }

        // output blocked?
        if (grep.out.eof)
          break;

        if (flag_with_hex)
          grep.binary = false;

        if (flag_invert_match)
          ++grep.matches;

        grep.binary = grep.binary || check_binary(ptr, size);

        if (grep.binfile || (grep.binary && !flag_hex && !flag_with_hex))
          break;

        if (grep.hex && !grep.binary)
          grep.out.dump.done();

        if (!flag_no_header)
          grep.out.header(pathname, grep.partname, grep.heading, grep.lineno, NULL, offset, separator, grep.binary);

        grep.hex = grep.binary;

        if (grep.binary)
        {
          grep.out.dump.hex(v_hex_context_line, offset, ptr, size);
        }
        else
        {
          bool lf_only = false;
          if (size > 0)
          {
            lf_only = ptr[size - 1] == '\n';
            size_t sizen = size - lf_only;
            if (sizen > 0)
            {
              grep.out.str(v_color_cx);
              grep.out.str(ptr, sizen);
              grep.out.str(color_off);
            }
          }
          grep.out.nl(lf_only);
        }

        next_before(buf, len, num, ptr, size, offset);
      }
    }

  };

  // extend event AnyLineGrepHandler to output specific context lines for -ABC without -v
  struct ContextGrepHandler : public AnyLineGrepHandler {

    // context state to track context lines before and after a match
    struct ContextState {

      struct Line {

        Line() :
          binary(false),
          offset(0),
          ptr(NULL),
          size(0)
        { }

        bool        binary; // before context binary line
        size_t      offset; // before context offset of line
        const char* ptr;    // before context pointer to line
        size_t      size;   // before context length of the line
        std::string line;   // before context line data saved from ptr with size bytes

      };

      typedef std::vector<Line> Lines;

      ContextState()
        :
          before_index(0),
          before_length(0),
          after_lineno(0),
          after_length(flag_after_context)
      {
        before_lines.resize(flag_before_context);
      }

      size_t before_index;  // before context rotation index
      size_t before_length; // accumulated length of the before context
      Lines  before_lines;  // before context line data
      size_t after_lineno;  // after context line number
      size_t after_length;  // accumulated length of the after context

    };

    ContextGrepHandler(Grep& grep, const char *& pathname)
      :
        AnyLineGrepHandler(grep, pathname)
    { }

    // output the before context
    void output_before_context()
    {
      // the group separator indicates lines skipped between contexts, like GNU grep
      if (state.after_lineno > 0 && state.after_lineno + state.after_length < grep.matcher->lineno() - state.before_length)
      {
        if (grep.hex)
          grep.out.dump.done();

        if (flag_group_separator != NULL)
        {
          if (flag_query && !flag_text)
          {
            grep.out.chr('\0');
            grep.out.str(color_se);
            grep.out.chr('\0');
            grep.out.str(flag_group_separator);
            grep.out.chr('\0');
          }
          else
          {
            grep.out.str(color_se);
            grep.out.str(flag_group_separator);
          }
          grep.out.str(color_off);
          grep.out.nl();
        }
      }

      // output the before context
      if (state.before_length > 0)
      {
        // the first line number of the before context
        size_t before_lineno = grep.matcher->lineno() - state.before_length;

        for (size_t i = 0; i < state.before_length; ++i)
        {
          size_t j = (state.before_index + i) % state.before_length;
          const ContextState::Line& before_line = state.before_lines[j];

          if (grep.hex && !before_line.binary)
            grep.out.dump.done();

          if (!flag_no_header)
            grep.out.header(pathname, grep.partname, grep.heading, before_lineno + i, NULL, before_line.offset, flag_separator_dash, before_line.binary);

          grep.hex = before_line.binary;

          // lines are always saved at the moment, so ptr is NULL, but we may optimize this in the future to not always save
          const char *ptr = before_line.ptr != NULL ? before_line.ptr : before_line.line.c_str();
          size_t size = before_line.size;

          if (grep.hex)
          {
            grep.out.dump.hex(Output::Dump::HEX_CONTEXT_LINE, before_line.offset, ptr, size);
          }
          else
          {
            bool lf_only = false;
            if (size > 0)
            {
              lf_only = ptr[size - 1] == '\n';
              size -= lf_only;
              if (size > 0)
              {
                grep.out.str(color_cx);
                grep.out.str(ptr, size);
                grep.out.str(color_off);
              }
            }
            grep.out.nl(lf_only);
          }
        }
      }

      // reset the before context state
      state.before_index = 0;
      state.before_length = 0;
    }

    // set the after context to the current line + 1
    void set_after_lineno(size_t lineno)
    {
      // set the after context state with the first after context line number
      state.after_length = 0;
      state.after_lineno = lineno;
    }

    // functor invoked by the reflex::AbstractMatcher when the buffer contents are shifted out, also called explicitly in grep::search
    virtual void operator()(reflex::AbstractMatcher& matcher, const char *buf, size_t len, size_t num) override
    {
      const char *ptr;
      size_t size;
      size_t offset;

      // if no gap to shift buffer, then let gap span the buffer until the begin of the matching line (bol)
      if (len == 0)
        len = matcher.before().len;

      // if we only need the before context, then look for it right before the current lineno
      if (state.after_length >= flag_after_context)
      {
        size_t current = matcher.lineno();
        if (grep.lineno + flag_before_context + 1 < current)
          grep.lineno = current - flag_before_context - 1;
      }

      begin_before(matcher, buf, len, num, ptr, size, offset);

      grep.output_or_save_restline();

      while (ptr != NULL)
      {
        // --range: max line exceeded?
        if (flag_max_line > 0 && grep.lineno > flag_max_line)
          break;

        // output blocked?
        if (grep.out.eof)
          break;

        if (flag_with_hex)
          grep.binary = false;

        grep.binary = grep.binary || check_binary(ptr, size);

        if (grep.binfile || (grep.binary && !flag_hex && !flag_with_hex))
          break;

        if (state.after_lineno > 0 && state.after_length < flag_after_context)
        {
          ++state.after_length;

          if (grep.hex && !grep.binary)
            grep.out.dump.done();

          if (!flag_no_header)
            grep.out.header(pathname, grep.partname, grep.heading, grep.lineno, NULL, offset, flag_separator_dash, grep.binary);

          grep.hex = grep.binary;

          if (grep.binary)
          {
            grep.out.dump.hex(Output::Dump::HEX_CONTEXT_LINE, offset, ptr, size);
          }
          else
          {
            bool lf_only = false;
            if (size > 0)
            {
              lf_only = ptr[size - 1] == '\n';
              size_t sizen = size - lf_only;
              if (sizen > 0)
              {
                grep.out.str(color_cx);
                grep.out.str(ptr, sizen);
                grep.out.str(color_off);
              }
            }
            grep.out.nl(lf_only);
          }
        }
        else if (flag_before_context > 0)
        {
          if (state.before_length < flag_before_context)
            ++state.before_length;

          state.before_index %= state.before_length;
          state.before_lines[state.before_index].binary = grep.binary;
          state.before_lines[state.before_index].offset = offset;
          state.before_lines[state.before_index].ptr    = ptr;
          state.before_lines[state.before_index].size   = size;

          ++state.before_index;
        }
        else
        {
          break;
        }

        next_before(buf, len, num, ptr, size, offset);
      }

      // save the (new or additional) before lines that become inaccessible after shift or buffer growth
      for (size_t i = 0; i < state.before_length; ++i)
      {
        if (state.before_lines[i].ptr != NULL)
        {
          state.before_lines[i].line.assign(state.before_lines[i].ptr, state.before_lines[i].size);
          state.before_lines[i].ptr = NULL;
        }
      }
    }

    ContextState state;

  };

  // extend event AnyLineGrepHandler to output specific context lines for -ABC with -v
  struct InvertContextGrepHandler : public AnyLineGrepHandler {

    // context state to track matching lines before non-matching lines
    struct InvertContextState {

      struct Match {

        Match(size_t pos, size_t size, size_t offset)
          :
            pos(pos),
            size(size),
            offset(offset)
        { }

        size_t pos;    // position on the line
        size_t size;   // size of the match
        size_t offset; // byte offset of the match

      };

      typedef std::vector<Match> Matches;

      struct Line {

        Line()
          :
            binary(false),
            columno(0),
            offset(0)
        { }

        bool        binary;  // before context binary line
        size_t      columno; // before context column number of first match
        size_t      offset;  // before context offset of first match
        std::string line;    // before context line data
        Matches     matches; // before context matches per line

      };

      typedef std::vector<Line> Lines;

      InvertContextState()
        :
          before_index(0),
          before_length(0),
          after_lineno(0)
      {
        before_lines.resize(flag_before_context);
      }

      size_t before_index;  // before context rotation index
      size_t before_length; // accumulated length of the before context
      Lines  before_lines;  // before context line data
      size_t after_lineno;  // the after context line number

    };

    InvertContextGrepHandler(Grep& grep, const char *& pathname)
      :
        AnyLineGrepHandler(grep, pathname)
    { }

    // output the inverted before context
    void output_before_context()
    {
      // the group separator indicates lines skipped between contexts, like GNU grep
      if (state.after_lineno > 0 && state.after_lineno + flag_after_context + flag_before_context < grep.lineno && flag_group_separator != NULL)
      {
        if (grep.hex)
          grep.out.dump.done();

        if (flag_query && !flag_text)
        {
          grep.out.chr('\0');
          grep.out.str(color_se);
          grep.out.chr('\0');
          grep.out.str(flag_group_separator);
          grep.out.chr('\0');
        }
        else
        {
          grep.out.str(color_se);
          grep.out.str(flag_group_separator);
        }
        grep.out.str(color_off);
        grep.out.nl();
      }

      // output the inverted before context
      if (state.before_length > 0)
      {
        // the first line number of the before context
        size_t before_lineno = grep.lineno - state.before_length;

        for (size_t i = 0; i < state.before_length; ++i)
        {
          size_t j = (state.before_index + i) % state.before_length;
          const InvertContextState::Line& before_line = state.before_lines[j];
          size_t offset = before_line.matches.empty() ? before_line.offset : before_line.matches.front().offset;

          if (grep.hex && !before_line.binary)
            grep.out.dump.done();

          if (!flag_no_header)
            grep.out.header(pathname, grep.partname, grep.heading, before_lineno + i, NULL, offset, flag_separator_dash, before_line.binary);

          grep.hex = before_line.binary;

          const char *ptr = before_line.line.c_str();
          size_t size = before_line.line.size();
          size_t pos = 0;

          for (const auto& match : before_line.matches)
          {
            if (grep.hex)
            {
              grep.out.dump.hex(Output::Dump::HEX_CONTEXT_LINE, match.offset - (match.pos - pos), ptr + pos, match.pos - pos);
              grep.out.dump.hex(Output::Dump::HEX_CONTEXT_MATCH, match.offset, ptr + match.pos, match.size);
            }
            else
            {
              if (match.pos > pos)
              {
                grep.out.str(color_cx);
                grep.out.str(ptr + pos, match.pos - pos);
                grep.out.str(color_off);
              }

              if (match.size > 0)
              {
                size_t sizen = match.size - (ptr[match.pos + match.size - 1] == '\n');
                if (sizen > 0)
                {
                  grep.out.str(match_mc);
                  grep.out.str(ptr + match.pos, sizen);
                  grep.out.str(match_off);
                }
              }
            }

            pos = match.pos + match.size;
          }

          if (grep.hex)
          {
            grep.out.dump.hex(Output::Dump::HEX_CONTEXT_LINE, before_line.offset + pos, ptr + pos, size - pos);
          }
          else
          {
            bool lf_only = false;
            if (size > pos)
            {
              lf_only = ptr[size - 1] == '\n';
              size -= lf_only;
              if (size > pos)
              {
                grep.out.str(color_cx);
                grep.out.str(ptr + pos, size - pos);
                grep.out.str(color_off);
              }
            }
            grep.out.nl(lf_only);
          }
        }
      }

      // reset the context state
      state.before_index = 0;
      state.before_length = 0;
      state.after_lineno = grep.lineno + 1;
    }

    // add context line with the first match to the inverted before context
    void add_before_context_line(const char *bol, const char *eol, size_t columno, size_t offset)
    {
      if (state.before_length < flag_before_context)
        ++state.before_length;

      state.before_index %= state.before_length;

      state.before_lines[state.before_index].binary  = grep.binary;
      state.before_lines[state.before_index].columno = columno;
      state.before_lines[state.before_index].offset  = offset;
      state.before_lines[state.before_index].line.assign(bol, eol - bol);
      state.before_lines[state.before_index].matches.clear();

      ++state.before_index;
    }

    // add match fragment to the inverted before context
    void add_before_context_match(size_t pos, size_t size, size_t offset)
    {
      // only add a match if we have a before line, i.e. not an after line with a multiline match
      if (state.before_length > 0)
      {
        size_t index = (state.before_index + state.before_length - 1) % state.before_length;
        state.before_lines[index].matches.emplace_back(pos, size, offset);
      }
    }

    // functor invoked by the reflex::AbstractMatcher when the buffer contents are shifted out, also called explicitly in grep::search
    virtual void operator()(reflex::AbstractMatcher& matcher, const char *buf, size_t len, size_t num) override
    {
      const char *ptr;
      size_t size;
      size_t offset;

      // if no gap to shift buffer, then let gap span the buffer until the begin of the matching line (bol)
      if (len == 0)
        len = matcher.before().len;

      begin_before(matcher, buf, len, num, ptr, size, offset);

      grep.output_or_save_restline(true);

      if (ptr != NULL)
        output_before_context();

      while (ptr != NULL)
      {
        state.after_lineno = grep.lineno + 1;

        // --range: max line exceeded?
        if (flag_max_line > 0 && grep.lineno > flag_max_line)
          break;

        if (grep.matches == 0)
        {
          // --max-files: max reached?
          if (!Stats::found_part())
          {
            grep.stop = true;
            break;
          }
        }

        // --max-count: max number of matches reached?
        if (flag_max_count > 0 && grep.matches >= flag_max_count)
          break;

        // output blocked?
        if (grep.out.eof)
          break;

        ++grep.matches;

        if (flag_with_hex)
          grep.binary = false;

        grep.binary = grep.binary || check_binary(ptr, size);

        if (grep.binfile || (grep.binary && !flag_hex && !flag_with_hex))
          break;

        if (grep.hex && !grep.binary)
          grep.out.dump.done();

        if (!flag_no_header)
          grep.out.header(pathname, grep.partname, grep.heading, grep.lineno, NULL, offset, flag_separator, grep.binary);

        grep.hex = grep.binary;

        if (grep.binary)
        {
          grep.out.dump.hex(Output::Dump::HEX_LINE, offset, ptr, size);
        }
        else
        {
          bool lf_only = false;
          if (size > 0)
          {
            lf_only = ptr[size - 1] == '\n';
            size_t sizen = size - lf_only;
            if (sizen > 0)
            {
              grep.out.str(color_sl);
              grep.out.str(ptr, sizen);
              grep.out.str(color_off);
            }
          }
          grep.out.nl(lf_only);
        }

        next_before(buf, len, num, ptr, size, offset);
      }
    }

    InvertContextState state;

  };

  Grep(FILE *file, reflex::AbstractMatcher *matcher, Static::Matchers *matchers)
    :
      filename(NULL),
      restline_data(NULL),
      restline_size(0),
      restline_last(0),
      out(file),
      matcher(matcher),
      matchers(matchers),
      file_in(NULL),
      bin_handler(*this)
#ifndef OS_WIN_OR_MINGW
    , stdin_handler(*this)
#endif
#ifdef HAVE_LIBZ
#ifdef WITH_DECOMPRESSION_THREAD
    , zthread(false, partname)
#else
    , zstream(NULL)
    , stream(NULL)
#endif
#endif
  { }

  virtual ~Grep()
  {
#ifdef HAVE_LIBZ
#ifndef WITH_DECOMPRESSION_THREAD
    if (stream != NULL)
    {
      delete stream;
      stream = NULL;
    }

    if (zstream != NULL)
    {
      delete zstream;
      zstream = NULL;
    }
#endif
#endif
  }

  // cancel all active searches
  void cancel()
  {
    // global cancellation is forced by cancelling the shared output
    out.cancel();

#ifdef HAVE_LIBZ
#ifdef WITH_DECOMPRESSION_THREAD
    // -z: cancel decompression thread's decompression loop
    if (flag_decompress)
      zthread.cancel();
#endif
#endif
  }

  // search the specified files or standard input for pattern matches
  virtual void ugrep();

  // search file or directory for pattern matches
  Type select(size_t level, const char *pathname, const char *basename, int type, uint32_t attr, ino_t& inode, uint64_t& info, bool is_argument = false);

  // recurse a directory
  virtual void recurse(size_t level, const char *pathname);

  // -Z and --sort=best: perform a presearch to determine edit distance cost, return cost of pathname file, MAX_COST when no match is found
  uint16_t compute_cost(const char *pathname);

  // search a file or archive
  virtual void search(const char *pathname, uint16_t cost);

  // the locals of search() that the search branches share (see rust/src_core/SEAM.md, "Grep seam")
  struct SearchContext {
    SearchContext(const char *pathname, uint16_t cost) : pathname(pathname), cost(cost), matched(false) { }
    const char *pathname; // search() argument pathname (the pointer may be changed, handlers refer to it)
    uint16_t    cost;     // search() argument cost, used before the branches only
    bool        matched;  // this file or archive has a match
  };

  // how a search branch ends: CONTINUE and DONE go on to the epilogue (done_search), EXIT leaves it out (exit_search)
  enum class SearchStep { CONTINUE, DONE, EXIT };

  // one function per search branch, each replaced by Rust by one later port unit, in the order search() runs them
  void       search_setup(SearchContext& ctx);                // --files: reset the matching bitmasks
  SearchStep search_quiet_list(SearchContext& ctx);           // -q, -l, -L
  SearchStep search_count(SearchContext& ctx);                // -c
  SearchStep search_format(SearchContext& ctx);               // --format
  SearchStep search_only_matching(SearchContext& ctx);        // -o
  SearchStep search_only_matching_context(SearchContext& ctx); // -o with -A, -B or -C
  SearchStep search_plain(SearchContext& ctx);                // plain matches and --hex
  SearchStep search_invert(SearchContext& ctx);               // -v
  SearchStep search_any_line(SearchContext& ctx);             // -y
  SearchStep search_context(SearchContext& ctx);              // -A, -B, -C
  SearchStep search_context_invert(SearchContext& ctx);       // -A, -B, -C with -v
  void       search_epilogue(SearchContext& ctx);             // --files check, --break, --stats

  // search input after lineno to populate a string vector with the matching line and lines after up to max lines
  void find_text_preview(const char *filename, const char *partname, size_t from_lineno, size_t max, size_t& lineno, size_t& num, std::vector<std::string>& text);

  // extract a part from an archive and send to a stream
  void extract(const char *filename, const char *partname, FILE *output);

  // check CNF AND/OR/NOT conditions are met for the line(s) spanning bol to eol
  bool cnf_matching(const char *bol, const char *eol, bool acquire = false)
  {
    if (flag_files)
    {
      if (out.holding())
      {
        size_t k = 0;    // iterate over matching[] bitmask
        bool all = true; // all terms matched

        // for each AND term check if the AND term was matched before or has a match this time
        for (const auto& i : *matchers)
        {
          // an OR term hasn't matched before
          if (!matching[k])
          {
            auto j = i.begin();
            auto e = i.end();

            if (j != e)
            {
              // check OR terms
              if (*j && (*j)->buffer(const_cast<char*>(bol), eol - bol + 1).find() != 0)
              {
                matching[k] = true;
                ++j;
              }
              else
              {
                // check OR NOT terms
                size_t l = 0;     // iterate over notmaching[k] bitmask
                bool none = true; // all not-terms matched

                while (++j != e)
                {
                  if (*j && !notmatching[k][l])
                  {
                    if ((*j)->buffer(const_cast<char*>(bol), eol - bol + 1).find() != 0)
                      notmatching[k][l] = true;
                    else
                      all = none = false;
                  }

                  ++l;
                }

                if (none)
                {
                  // when all not-terms matched and we don't have a positive alternative then stop searching this file
                  if (!*i.begin())
                    throw EXIT_SEARCH();

                  all = false;
                }
              }
            }
          }
          ++k;
        }

        // if all terms matched globally per file then remove the hold to launch output
        if (all)
        {
          if (acquire)
            out.acquire();

          // --max-files: max reached?
          if (!Stats::found_part())
            throw EXIT_SEARCH();

          out.launch();
        }
      }
    }
    else
    {
      // for each AND term check if the line has a match
      for (const auto& i : *matchers)
      {
        auto j = i.begin();
        auto e = i.end();

        if (j != e)
        {
          // check OR terms
          if (*j && (*j)->buffer(const_cast<char*>(bol), eol - bol + 1).find() != 0)
            continue;

          // check OR NOT terms
          while (++j != e)
            if (*j && (*j)->buffer(const_cast<char*>(bol), eol - bol + 1).find() == 0)
              break;

          if (j == e)
            return false;
        }
      }
    }

    return true;
  }

  // if CNF AND/OR/NOT conditions are met globally then launch output after searching a file with --files
  bool cnf_satisfied(bool acquire = false)
  {
    if (out.holding())
    {
      size_t k = 0; // iterate over matching[] bitmask

      // for each AND term check if the term was matched before
      for (const auto& i : *matchers)
      {
        // an OR term hasn't matched
        if (!matching[k])
        {
          // return if there are no OR NOT terms to check
          if (i.size() <= 1)
            return false;

          auto j = i.begin();
          auto e = i.end();

          // check if not all of the OR NOT terms matched
          if (j != e)
          {
            size_t l = 0; // iterate over notmaching[k] bitmask
            while (++j != e)
            {
              if (*j && !notmatching[k][l])
                break;
              ++l;
            }
            // return if all OR NOT terms matched
            if (j == e)
              return false;
          }
        }
        ++k;
      }

      if (acquire)
        out.acquire();

      // --max-files: max reached?
      if (!Stats::found_part())
        throw EXIT_SEARCH();

      out.launch();
    }

    return true;
  }

  // open a file for (binary) reading and assign input, decompress the file when -z, --decompress specified, may throw bad_alloc
  bool open_file(const char *pathname, const char *find = NULL)
  {
    if (pathname == Static::LABEL_STANDARD_INPUT)
    {
      if (Static::source == NULL)
        return false;

      pathname = flag_label;
      file_in = Static::source;

#ifdef OS_WIN
      (void)_setmode(fileno(Static::source), _O_BINARY);
#endif
    }
    else if (fopenw_s(&file_in, pathname, "rb") != 0)
    {
      warning("cannot read", pathname);

      return false;
    }

    // "interactive" means possibly slow input from a TTY or pipe
    interactive = false;

    // --filter: fork process to filter file, when applicable
    if (!filter(file_in, pathname))
      return false;

#if defined(__linux__)
    if (file_in != stdin && file_in != Static::source)
    {
      // Linux has system "regular files" that can be set non-blocking to raise EAGAIN e.g. for /proc and /sys files
      if (flag_devices_action != Action::READ)
      {
        int fd = fileno(file_in);
        struct stat buf;

        if (fstat(fd, &buf) == 0 && S_ISREG(buf.st_mode))
          fcntl(fd, F_SETFL, fcntl(fd, F_GETFL) | O_NONBLOCK);
      }
    }
#endif

#ifdef HAVE_LIBZ
    if (flag_decompress)
    {
#ifdef WITH_DECOMPRESSION_THREAD

      // start decompression thread if not running, get pipe with decompressed input
      FILE *pipe_in = zthread.start(flag_zmax, pathname, file_in, find);
      if (pipe_in == NULL)
      {
        fclose(file_in);
        file_in = NULL;

        return false;
      }

      input = reflex::Input(pipe_in, flag_encoding_type);

#else
      (void)find; // appease -Wunused

      // create or open a new zstreambuf
      if (zstream == NULL)
        zstream = new zstreambuf(pathname, file_in);
      else
        zstream->open(pathname, file_in);

      if (stream != NULL)
        delete stream;

      stream = new std::istream(zstream);

      input = stream;

#endif
    }
    else
    {
      input = reflex::Input(file_in, flag_encoding_type);
    }
#else
    (void)find; // appease -Wunused
    input = reflex::Input(file_in, flag_encoding_type);
#endif

#ifndef OS_WIN_OR_MINGW
    if (!flag_decompress && !flag_quiet && !flag_files_with_matches && !flag_count)
    {
      int fd = fileno(file_in);
      struct stat buf;

      // if input is a character device (e.g. TTY) or a pipe, then make it non-blocking and register a stdin handler to continue reading and flush results to output, don't check for invalid Unicode input
      if (fstat(fd, &buf) == 0 && (S_ISCHR(buf.st_mode) || S_ISFIFO(buf.st_mode) || S_ISSOCK(buf.st_mode)))
        interactive = (fcntl(fd, F_SETFL, fcntl(fd, F_GETFL) | O_NONBLOCK) != -1);
    }
#else
    // for Windows we don't check for invalid Unicode input, no stdin handler
    if (input == stdin && !flag_quiet && !flag_files_with_matches && !flag_count)
      interactive = true;
#endif

    return true;
  }

  // --filter: return true on success, create a pipe to replace file input if filtering files in a forked process
  bool filter(FILE*& in, const char *pathname)
  {
    if (!flag_filter.empty() && in != NULL)
    {
      const char *basename = strrchr(pathname, PATHSEPCHR);
      if (basename == NULL)
        basename = pathname;
      else
        ++basename;

      // get the basenames's extension suffix
      const char *suffix = strrchr(basename, '.');

      // don't consider . at the front of basename, otherwise skip .
      if (suffix == basename)
        suffix = NULL;
      else if (suffix != NULL)
        ++suffix;

      // --filter-magic-label: if the file is seekable (to rewind later), then check for a magic pattern match
      if (!flag_filter_magic_label.empty() && fseek(in, 0, SEEK_SET) == 0)
      {
        bool is_plus = false;

        // --filter-magic-label: check for overriding +
        if (suffix != NULL)
        {
          for (const auto& i : flag_filter_magic_label)
          {
            if (i.front() == '+')
            {
              is_plus = true;

              break;
            }
          }
        }

        // --filter-magic-label: if the basename has no suffix or a +LABEL + then check magic bytes
        if (suffix == NULL || is_plus)
        {
          // create a matcher to match the magic pattern
          size_t match = reflex::Matcher(Static::filter_magic_pattern, in).scan();

          // rewind the input after scan
          rewind(in);

          if (match > 0 && match <= flag_filter_magic_label.size())
          {
            suffix = flag_filter_magic_label[match - 1].c_str();

            if (*suffix == '+')
              ++suffix;
          }
        }
      }

      // basenames without a suffix get "*" as a suffix
      if (suffix == NULL || *suffix == '\0')
        suffix = "*";

      size_t sep = strlen(suffix);

      const char *command = flag_filter.c_str();
      const char *default_command = NULL;

      // find the command corresponding to the first matching suffix specified in the filter
      while (true)
      {
        while (isspace(static_cast<unsigned char>(*command)))
          ++command;

        // wildcard *:command is considered only when no matching suffix was found
        if (*command == '*')
          default_command = strchr(command, ':');

        // match filter filename extension (case insensitive)
        bool ext_match = strncasecmp(suffix, command, sep) == 0;
        if (ext_match && (command[sep] == ':' || command[sep] == ',' || isspace(static_cast<unsigned char>(command[sep]))))
        {
          command = strchr(command, ':');
          break;
        }

        command = strchr(command, ',');
        if (command == NULL)
          break;

        ++command;
      }

      // if no matching command, use the wildcard *:command when specified
      if (command == NULL)
        command = default_command;

      // suffix has a command to execute
      if (command != NULL)
      {
        // skip over the ':'
        ++command;

        return filter_spawn(in, pathname, command);
      }
    }

    return true;
  }

  // the OS part of --filter (the retained C++ of the seam, called by filter() and by the facade): create the pipe and run
  // the command, return false when the pipe cannot be created (warned, in closed unless stdin, in set to NULL)
  bool filter_spawn(FILE*& in, const char *pathname, const char *command)
  {
    {
      {
        int fd[2];

#ifdef OS_WIN_OR_MINGW
        // Windows CreateProcess requires an "inherited" pipe handle specific to Windows
        bool ok = (pipe_inherit(fd) == 0);
#else
        bool ok = (pipe(fd) == 0);
#endif

        if (ok)
        {
#ifdef OS_WIN_OR_MINGW

          std::wstring wcommand(utf8_decode(command));
          size_t pathname_pos = 0;

          // replace all % by the pathname, except when quoted
          while (true)
          {
            size_t size = wcommand.size();

            for (; pathname_pos < size && wcommand[pathname_pos] != L'%'; ++pathname_pos)
              if (wcommand[pathname_pos] == L'"')
                while (++pathname_pos < size && wcommand[pathname_pos] != L'"')
                  continue;

            if (pathname_pos >= size)
              break;

            std::wstring wpathname(utf8_decode(in == stdin ? "-" : pathname));
            wcommand.replace(pathname_pos, 1, wpathname);
            pathname_pos += wpathname.size();
          }

          // set up inherited stdin, stdout and stderr for the child process
          STARTUPINFOW si;
          memset(&si, 0, sizeof(STARTUPINFOW));
          si.cb = sizeof(STARTUPINFOW);
          si.dwFlags = STARTF_USESTDHANDLES;
          si.hStdInput = reinterpret_cast<HANDLE>(_get_osfhandle(_fileno(in)));
          si.hStdOutput = reinterpret_cast<HANDLE>(_get_osfhandle(fd[1]));
          if (!flag_quiet && !flag_no_messages)
            si.hStdError = GetStdHandle(STD_ERROR_HANDLE);
          if (in != stdin)
            SetHandleInformation(si.hStdInput, HANDLE_FLAG_INHERIT, 1);

          PROCESS_INFORMATION pi;
          memset(&pi, 0, sizeof(PROCESS_INFORMATION));

          // use buffer to allow CreateProcessW to change the command and arguments to pass them as argc argv
          wchar_t *wbuffer = new wchar_t[wcommand.size() + 1];
          wcscpy(wbuffer, wcommand.c_str());

          if (CreateProcessW(NULL, wbuffer, NULL, NULL, TRUE, 0, NULL, NULL, &si, &pi))
          {
            CloseHandle(pi.hProcess);
            CloseHandle(pi.hThread);
          }
          else
          {
            const char *end;

            for (end = command; *end != '\0' && *end != ','; ++end)
              if (*end == '"')
                while (*++end != '\0' && *end != '"')
                  continue;

            std::string arg(command, end - command);
            errno = GetLastError();
            warning("--filter: cannot create process for command", arg.c_str());
          }

          delete[] wbuffer;

#else

          int pid;

          // fork to execute the specified --filter utility command on the input to produce filtered output
          if ((pid = fork()) == 0)
          {
            // child process

            // close the reading end of the pipe
            close(fd[0]);

            // dup the input file to stdin unless reading stdin
            if (in != stdin)
            {
              dup2(fileno(in), STDIN_FILENO);
              fclose(in);
            }

            // dup the writing end of the pipe to stdout
            dup2(fd[1], STDOUT_FILENO);
            close(fd[1]);

            // -q or -q: suppress error messages sent to stderr by the filter command by redirecting to /dev/null
            if (flag_quiet || flag_no_messages)
            {
              int dev_null = open("/dev/null", O_WRONLY);
              if (dev_null >= 0)
              {
                dup2(dev_null, STDERR_FILENO);
                close(dev_null);
              }
            }

            // populate argv[] with the command and its arguments, destroying flag_filter in the child process
            std::vector<const char*> args;

            char *arg = const_cast<char*>(command);

            while (*arg != '\0' && *arg != ',')
            {
              while (isspace(static_cast<unsigned char>(*arg)))
                ++arg;

              char *s = arg;

              if (*arg == '"')
              {
                // "quoted argument" separated by space
                ++s;

                while (*s != '\0' &&
                    (*s != '"' ||
                     (s[1] != '\0' && s[1] != ',' && !isspace(static_cast<unsigned char>(s[1])))))
                  ++s;

                if (*s == '"')
                {
                  ++arg;
                  *s++ = '\0';
                }
              }
              else
              {
                // space-separated argument
                while (*s != '\0' && *s != ',' && !isspace(static_cast<unsigned char>(*s)))
                  ++s;
              }

              if (s > arg)
              {
                if (s - arg == 1 && *arg == '%')
                  args.push_back(in == stdin ? "-" : pathname);
                else
                  args.push_back(arg);
              }

              if (*s == ',')
                *s = '\0';

              if (*s == '\0')
                break;

              *s = '\0';

              arg = s + 1;
            }

            // silently bail out if there is no command
            if (args.empty())
              exit(EXIT_SUCCESS);

            // add sentinel
            args.push_back(NULL);

            // get argv[] array data
            char * const *argv = const_cast<char * const *>(args.data());

            // execute
            execvp(argv[0], argv);

            error("--filter: cannot exec", argv[0]);
          }

#endif

          // close the writing end of the pipe
          close(fd[1]);

          // close the file and use the reading end of the pipe
          if (in != NULL && in != stdin)
            fclose(in);
          in = fdopen(fd[0], "r");
        }
        else
        {
          if (in != stdin)
            fclose(in);
          in = NULL;

          warning("--filter: cannot create pipe", flag_filter.c_str());

          return false;
        }
      }
    }

    return true;
  }

  // close the file and clear input, return true if next file is extracted from an archive to search
  bool close_file(const char *pathname)
  {
    // check if the input has an error condition, but do not check stdin which is handled differently
    if (file_in != NULL && file_in != stdin && file_in != Static::source && ferror(file_in))
      warning("cannot read", pathname);

#ifdef HAVE_LIBZ
#ifdef WITH_DECOMPRESSION_THREAD

    // -z: open next archived file if any or close the compressed file/archive
    if (flag_decompress)
    {
      // close the input FILE* and its underlying pipe previously created with pipe() and fdopen()
      if (input.file() != NULL)
      {
        // close and unassign input, i.e. input.file() == NULL, also closes pipe_fd[0] per fdopen()
        fclose(input.file());
        input.clear();
      }

      // if output is blocked, then cancel decompression threads
      if (out.eof)
        zthread.cancel();

      // open pipe to the next file or part in an archive if there is a next file to extract
      FILE *pipe_in = zthread.open_next(pathname);

      if (pipe_in != NULL)
      {
        // assign the next extracted file as input to search
        input = reflex::Input(pipe_in, flag_encoding_type);

        // loop back in search() to start searching the next file in the archive
        return true;
      }
    }

#else

    if (stream != NULL)
    {
      delete stream;
      stream = NULL;
    }

#endif
#endif

#if defined(WITH_STDIN_DRAIN) && !defined(OS_WIN_OR_MINGW)

    // drain stdin when non-seekable file such as a pipe until eof, unless option -q is used
    if (file_in == stdin && !flag_quiet && !flag_files_with_matches && flag_max_count == 0 && flag_max_line == 0 &&
        !feof(stdin) && fseek(stdin, 0, SEEK_END) < 0 && errno != EINVAL)
    {
      char buf[16384];
      while (input.get(buf, sizeof(buf)) > 0)
        continue;
    }

#endif

    // close the current input file
    if (file_in != NULL && file_in != stdin && file_in != Static::source)
    {
      fclose(file_in);
      file_in = NULL;
    }

    input.clear();

    return false;
  }

  // specify input to read for matcher, when input is a regular file then try mmap for zero copy overhead
  bool init_read()
  {
    // initialize Grep search and output variables
    binfile = false;
    lineno = flag_min_line > 0 ? flag_min_line - 1 : 0;
    heading = flag_with_filename;
    hex = false;
    binary = false;
    matches = 0;
    stop = false;

    const char *base;
    size_t size;

    // attempt to mmap the input file, if mmap is supported and enabled (disabled by default)
    if (mmap.file(input, base, size))
    {
      // matcher reads directly from protected mmap memory (cast is safe: base[0..size] is not modified!)
      matcher->buffer(const_cast<char*>(base), size + 1);
    }
    else
    {
      // assign input to the matcher to search
      matcher->input(input);

#if !defined(HAVE_PCRE2) && defined(HAVE_BOOST_REGEX)
      // buffer all input to work around Boost.Regex partial matching bug, but this may throw std::bad_alloc if the file is too large
      if (flag_mode == Mode::PERL)
      {
        matcher->buffer();
      }
      else
#endif

#ifndef OS_WIN_OR_MINGW
        // set interactive search handler e.g. for slow stdin input
        if (interactive)
          matcher->in.set_handler(&stdin_handler);
#endif
    }

    // interactive: do not check init_is_binary(), wait until handler detects binary files that have NUL bytes
    if (!interactive)
    {
      // otherwise, check if the file is a binary file
      if (flag_binary_without_match)
      {
        // -I: do not match binary files
        if (init_is_binary())
          return false;
      }
      else if (!flag_quiet && !flag_files_with_matches && !flag_count && flag_format == NULL && !flag_text && !flag_hex && !flag_with_hex && flag_encoding_type != reflex::Input::file_encoding::null_data)
      {
        // not -q, -l, -c, --format, -a, -X, -W, -00: check if initial part of the file is binary
        binfile = init_is_binary();

        // detect binary input dynamically when more input is read after checking the inital input
        matcher->in.set_handler(&bin_handler);
      }
    }

    // --range=[MIN,][MAX]: start searching at line MIN
    for (size_t i = flag_min_line; i > 1; --i)
      if (!matcher->skip('\n'))
        break;

    return true;
  }

  // initial part of a file is binary
  bool init_is_binary()
  {
    // check initial input for binary data, fill up to 131072 bytes to check (similar GNU grep, but a bit more)
    size_t avail = matcher->avail(reflex::AbstractMatcher::Const::BUFSZ);
    if (avail == 0)
      return false;

    // do not cut off the last UTF-8 sequence, ignore it, otherwise we risk failing the UTF-8 check
    const char *buf = matcher->begin();
    if ((buf[avail - 1] & 0x80) == 0x80)
    {
      size_t n = std::min<size_t>(avail, 4); // note: 1 <= n <= 4 bytes to check
      while (n > 0 && (buf[--avail] & 0xc0) == 0x80)
        --n;
      if ((buf[avail] & 0xc0) != 0xc0)
        return true;
    }

    return is_binary(matcher->begin(), avail);
  }

  // output the rest of the matching line or save it for later, uses lineno of previous match to check
  void output_or_save_restline(bool invert = false, bool hex_context = false)
  {
    if (restline_data != NULL)
    {
      if (flag_ungroup || lineno != matcher->lineno())
        output_restline(invert, hex_context);
      else
        save_restline();
    }
  }

  // output the rest of the matching line or save it for later, this is necessary when the buffer shifts
  void output_restline(bool invert = false, bool hex_context = false)
  {
    if (restline_data != NULL)
    {
      if (binary)
      {
        // --hexdump with ABC context: if binary then show hex context
        if (hex_context)
        {
          size_t right = flag_hex_after * flag_hex_columns - ((restline_last - 1) % flag_hex_columns) - 1;
          if (restline_last + right > matcher->first())
            right = matcher->first() - restline_last;
          if (right < restline_size)
            restline_size = right;
        }

        out.dump.hex(invert ? Output::Dump::HEX_CONTEXT_LINE : Output::Dump::HEX_LINE, restline_last, restline_data, restline_size);
      }
      else
      {
        bool lf_only = false;
        if (restline_size > 0)
        {
          lf_only = restline_data[restline_size - 1] == '\n';
          restline_size -= lf_only;
          if (restline_size > 0)
          {
            out.str(invert ? color_cx : color_sl);
            out.str(restline_data, restline_size);
            out.str(color_off);
          }
        }
        out.nl(lf_only);
      }

      restline_data = NULL;
    }
  }

  // save the rest of the line to a string buffer to output later, may self-assign, so swap with a temp string
  void save_restline()
  {
    std::string temp(restline_data, restline_size);
    temp.swap(restline);
    restline_data = restline.c_str();
  }

  // The facade of the Grep seam (src/search_seam.h, rust/src_core/SEAM.md "Grep file members"): fills the host table
  // ugrep_core_search_env and the matcher tables of every matcher kind for the Rust side. It only forwards to the members
  // of Grep above, which stay active: nothing in the default build creates a facade yet, the port task of the file members
  // does. One facade per Grep, used on the thread that runs Grep::search, destroyed before the matchers it handed out.
  //
  // Exception channel: no exception unwinds through a Rust frame. Every callback runs under guard(): a catch (...) stores
  // the exception as a std::exception_ptr, the callback returns a neutral value (0, NULL, false) and the callbacks that
  // follow do nothing. Rust sees error_pending, returns EXIT and the C++ caller of the bridge calls rethrow(). The one
  // exception that is part of the protocol, EXIT_SEARCH of cnf_matching and cnf_satisfied, is caught inside the callback and
  // returned as UGREP_CORE_CNF_EXIT. Every offset and index Rust passes is checked against the data it indexes; a value
  // out of range is an error of the channel (std::out_of_range), never an access out of bounds.
  struct SeamFacade {

    explicit SeamFacade(Grep& grep)
      :
        grep(grep),
        path_slot(NULL),
        context_handler(NULL),
        invert_context_handler(NULL),
        state(ugrep_rust::grep_state_new())
    { }

    SeamFacade(const SeamFacade&) = delete;
    SeamFacade& operator=(const SeamFacade&) = delete;

    ~SeamFacade()
    {
      for (auto& i : matcher_ctxs)
        if (i.handler)
          i.matcher->set_handler(NULL);
    }

    // the pathname of the search the handlers refer to by reference: they refer to the slot, which can be set (again) at
    // any time, also after the handlers were made
    void bind(const char *search_pathname)
    {
      path_slot = search_pathname;
    }

    // run a callback: a pending error skips it, an exception is stored and the neutral value R() is returned
    template<typename Fn>
    auto guard(Fn&& fn) -> decltype(fn())
    {
      typedef decltype(fn()) R;
      if (error)
        return R();
      try
      {
        return fn();
      }
      catch (...)
      {
        error = std::current_exception();
        return R();
      }
    }

    // a value from Rust is out of range: an error of the channel
    static void check(bool in_range, const char *what)
    {
      if (!in_range)
        throw std::out_of_range(what);
    }

    // a FILE* from Rust is never NULL where a stream is used: NULL is an error of the channel
    static FILE *stream(void *file, const char *what)
    {
      check(file != NULL, what);
      return static_cast<FILE*>(file);
    }

    // the C++ cancels the decompression threads when the output is blocked (out.eof) before it opens the next file; an
    // unconditional cancel would stop the threads that the next archive reuses
    void cancel_zthread_if_blocked()
    {
#if defined(HAVE_LIBZ) && defined(WITH_DECOMPRESSION_THREAD)
      if (flag_decompress && grep.out.eof)
        grep.zthread.cancel();
#endif
    }

    // no matcher of the facade keeps reading a stream that is about to be closed: its input is cleared
    void detach_matchers(FILE *file)
    {
      for (auto& i : matcher_ctxs)
        if (i.matcher->in.file() == file)
          i.matcher->in.clear();
    }

    // the C++ caller of the bridge calls this after every call: rethrow the stored exception, once
    void rethrow()
    {
      if (error)
      {
        std::exception_ptr e;
        std::swap(e, error);
        std::rethrow_exception(e);
      }
    }

    // read access to the protected buffer state of a matcher: the buffer base buf_ and the end of its data end_
    struct Peek : public reflex::AbstractMatcher {
      static const char *base(reflex::AbstractMatcher& matcher) { return matcher.*(&Peek::buf_); }
      static size_t end(reflex::AbstractMatcher& matcher) { return matcher.*(&Peek::end_); }
    };

    // a shift handler that calls the Rust shift function
    struct RustHandler : public reflex::AbstractMatcher::Handler {
      RustHandler(void *user, ugrep_core_shift_fn shift) : user(user), shift(shift) { }
      virtual void operator()(reflex::AbstractMatcher&, const char *buf, size_t len, size_t num) override
      {
        shift(user, len, num, reinterpret_cast<const unsigned char*>(buf));
      }
      void             *user;
      ugrep_core_shift_fn shift;
    };

    // the matcher table ctx: one per matcher handed out, owns its shift handler
    struct MatcherCtx {
      MatcherCtx(SeamFacade *facade, reflex::AbstractMatcher *matcher) : facade(facade), matcher(matcher) { }
      SeamFacade                                        *facade;
      reflex::AbstractMatcher                           *matcher;
      std::unique_ptr<reflex::AbstractMatcher::Handler>  handler;
    };

    // the context of a matcher, created on first use, the address stays valid
    MatcherCtx *matcher_ctx(reflex::AbstractMatcher *matcher)
    {
      for (auto& i : matcher_ctxs)
        if (i.matcher == matcher)
          return &i;
      matcher_ctxs.emplace_back(this, matcher);
      return &matcher_ctxs.back();
    }

    template<typename M>
    static bool is_kind(reflex::AbstractMatcher *matcher)
    {
      return dynamic_cast<M*>(matcher) != NULL;
    }

    // UGREP_CORE_MATCHER_* of a matcher, 0 for none of the kinds ugrep creates; FuzzyMatcher derives from Matcher: test it first
    static size_t matcher_kind(reflex::AbstractMatcher *matcher)
    {
      if (is_kind<reflex::FuzzyMatcher>(matcher))
        return UGREP_CORE_MATCHER_FUZZY;
      if (is_kind<reflex::LineMatcher>(matcher))
        return UGREP_CORE_MATCHER_LINE;
#ifdef HAVE_PCRE2
      if (is_kind<reflex::PCRE2Matcher>(matcher))
        return UGREP_CORE_MATCHER_PCRE2;
#elif defined(HAVE_BOOST_REGEX)
      if (is_kind<reflex::BoostPerlMatcher>(matcher))
        return UGREP_CORE_MATCHER_BOOST;
#endif
      if (is_kind<reflex::Matcher>(matcher))
        return UGREP_CORE_MATCHER_REFLEX;
      return 0;
    }

    static MatcherCtx& mctx(void *ctx) { return *static_cast<MatcherCtx*>(ctx); }
    static reflex::AbstractMatcher& am(void *ctx) { return *mctx(ctx).matcher; }
    static size_t offset(void *ctx, const char *ptr) { return ptr - Peek::base(am(ctx)); }

    // guard a callback of the facade (env ctx) or of a matcher table (MatcherCtx)
    template<typename Fn>
    static auto G(void *ctx, Fn&& fn) -> decltype(fn()) { return F(ctx).guard(fn); }
    template<typename Fn>
    static auto GM(void *ctx, Fn&& fn) -> decltype(fn()) { return mctx(ctx).facade->guard(fn); }

    // destroy the shift handler of a matcher, the matcher forgets it first; the context handler pointers are only
    // reset when they are this matcher's handler
    void clear_handler(MatcherCtx& mc)
    {
      mc.matcher->set_handler(NULL);
      if (mc.handler.get() == context_handler)
        context_handler = NULL;
      if (mc.handler.get() == invert_context_handler)
        invert_context_handler = NULL;
      mc.handler.reset();
    }

    // construct and register the UGREP_CORE_HANDLER_* class on the matcher, NONE only unregisters
    void set_shift_handler(MatcherCtx& mc, int kind)
    {
      clear_handler(mc);
      const char *& path = path_slot;
      switch (kind)
      {
        case UGREP_CORE_HANDLER_GREP:
          mc.handler.reset(new GrepHandler(grep, path));
          break;
        case UGREP_CORE_HANDLER_ONLY_MATCHING_CONTEXT:
          mc.handler.reset(new OnlyMatchingContextGrepHandler(grep, path));
          break;
        case UGREP_CORE_HANDLER_INVERT_MATCH:
          mc.handler.reset(new InvertMatchGrepHandler(grep, path));
          break;
        case UGREP_CORE_HANDLER_FORMAT_INVERT_MATCH:
          mc.handler.reset(new FormatInvertMatchGrepHandler(grep, path));
          break;
        case UGREP_CORE_HANDLER_ANY_LINE:
          mc.handler.reset(new AnyLineGrepHandler(grep, path));
          break;
        case UGREP_CORE_HANDLER_CONTEXT:
          context_handler = new ContextGrepHandler(grep, path);
          mc.handler.reset(context_handler);
          break;
        case UGREP_CORE_HANDLER_INVERT_CONTEXT:
          invert_context_handler = new InvertContextGrepHandler(grep, path);
          mc.handler.reset(invert_context_handler);
          break;
        default:
          break;
      }
      if (mc.handler)
        mc.matcher->set_handler(mc.handler.get());
    }

    // fill the table of a matcher of any kind, returns false for a NULL matcher or a kind that ugrep does not create
    bool fill_matcher_table(reflex::AbstractMatcher *matcher, ugrep_core_matcher_table *table)
    {
      if (matcher == NULL)
        return false;
      size_t kind = matcher_kind(matcher);
      if (kind == 0)
        return false;

      memset(table, 0, sizeof(*table));
      table->ctx = matcher_ctx(matcher);
      table->kind = kind;
      table->find = [](void *c) -> int { return GM(c, [&]() -> int { return am(c).find() != 0; }); };
      table->next_match = [](void *c, int skip_line) -> int
      {
        return GM(c, [&]() -> int { return (skip_line == 0 || am(c).skip('\n')) && am(c).find() != 0; });
      };
      table->bol = [](void *c) -> size_t { return GM(c, [&]() -> size_t { return offset(c, am(c).bol()); }); };
      table->eol = [](void *c, int inclusive) -> size_t
      {
        return GM(c, [&]() -> size_t { return offset(c, am(c).eol(inclusive != 0)); });
      };
      table->begin = [](void *c) -> size_t { return GM(c, [&]() -> size_t { return offset(c, am(c).begin()); }); };
      table->end = [](void *c) -> size_t { return GM(c, [&]() -> size_t { return offset(c, am(c).end()); }); };
      table->text_bounds = [](void *c, ugrep_core_text_bounds *out)
      {
        memset(out, 0, sizeof(*out));
        GM(c, [&]()
        {
          reflex::AbstractMatcher& m = am(c);
          out->eol = offset(c, m.eol()); // eol() before bol(), begin() and end()
          out->bol = offset(c, m.bol());
          out->begin = offset(c, m.begin());
          out->end = offset(c, m.end());
          out->first = m.first();
          out->last = m.last();
          out->size = m.size();
          out->lineno = m.lineno();
          out->lines = m.lines();
        });
      };
      table->first = [](void *c) -> size_t { return GM(c, [&]() -> size_t { return am(c).first(); }); };
      table->last = [](void *c) -> size_t { return GM(c, [&]() -> size_t { return am(c).last(); }); };
      table->size = [](void *c) -> size_t { return GM(c, [&]() -> size_t { return am(c).size(); }); };
      table->lineno = [](void *c) -> size_t { return GM(c, [&]() -> size_t { return am(c).lineno(); }); };
      table->lines = [](void *c) -> size_t { return GM(c, [&]() -> size_t { return am(c).lines(); }); };
      table->columno = [](void *c) -> size_t { return GM(c, [&]() -> size_t { return am(c).columno(); }); };
      table->at_bol = [](void *c) -> int { return GM(c, [&]() -> int { return am(c).at_bol(); }); };
      table->hit_end = [](void *c) -> int { return GM(c, [&]() -> int { return am(c).hit_end(); }); };
      table->skip = [](void *c, int ch) -> int
      {
        return GM(c, [&]() -> int { return am(c).skip(static_cast<char>(ch)); });
      };
      table->border = [](void *c) -> size_t { return GM(c, [&]() -> size_t { return am(c).border(); }); };
      table->before = [](void *c, ugrep_core_buf_context *out)
      {
        memset(out, 0, sizeof(*out));
        GM(c, [&]()
        {
          reflex::AbstractMatcher::Context context = am(c).before();
          out->offset = offset(c, context.buf);
          out->len = context.len;
          out->num = context.num;
        });
      };
      table->after = [](void *c, ugrep_core_buf_context *out)
      {
        memset(out, 0, sizeof(*out));
        GM(c, [&]()
        {
          reflex::AbstractMatcher::Context context = am(c).after();
          out->offset = offset(c, context.buf);
          out->len = context.len;
          out->num = context.num;
        });
      };
      table->bef = [](void *c, size_t len) -> size_t { return GM(c, [&]() -> size_t { return offset(c, am(c).bef(len)); }); };
      table->aft = [](void *c, size_t len) -> size_t { return GM(c, [&]() -> size_t { return offset(c, am(c).aft(len)); }); };
      table->bytes = [](void *c, size_t off, size_t len) -> const unsigned char*
      {
        return GM(c, [&]() -> const unsigned char*
        {
          reflex::AbstractMatcher& m = am(c);
          size_t end = Peek::end(m);
          if (off > end || len > end - off)
            return NULL;
          return reinterpret_cast<const unsigned char*>(Peek::base(m)) + off;
        });
      };
      table->set_reserve = [](void *c, size_t size) { GM(c, [&]() { am(c).set_reserve(size); }); };
      table->fuzzy_distance = [](void *c, unsigned short max)
      {
        GM(c, [&]()
        {
          reflex::FuzzyMatcher *fuzzy = dynamic_cast<reflex::FuzzyMatcher*>(&am(c));
          if (fuzzy != NULL)
            fuzzy->distance(max);
        });
      };
      table->fuzzy_edits = [](void *c) -> unsigned short
      {
        return GM(c, [&]() -> unsigned short
        {
          reflex::FuzzyMatcher *fuzzy = dynamic_cast<reflex::FuzzyMatcher*>(&am(c));
          return fuzzy != NULL ? fuzzy->edits() : 0;
        });
      };
      table->lineno_skip = [](void *c, int on) { GM(c, [&]() { am(c).lineno_skip(on != 0); }); };
      table->set_shift_handler = [](void *c, int kind)
      {
        GM(c, [&]() { mctx(c).facade->set_shift_handler(mctx(c), kind); });
      };
      table->set_rust_handler = [](void *c, void *user, ugrep_core_shift_fn shift)
      {
        GM(c, [&]()
        {
          MatcherCtx& mc = mctx(c);
          mc.facade->clear_handler(mc);
          mc.handler.reset(new RustHandler(user, shift));
          mc.matcher->set_handler(mc.handler.get());
        });
      };
      table->invoke_handler = [](void *c, const ugrep_core_buf_context *context)
      {
        GM(c, [&]()
        {
          MatcherCtx& mc = mctx(c);
          size_t end = Peek::end(*mc.matcher);
          check(context->offset <= end && context->len <= end - context->offset, "invoke_handler: outside the buffer");
          if (mc.handler)
            (*mc.handler)(*mc.matcher, Peek::base(*mc.matcher) + context->offset, context->len, context->num);
        });
      };
      table->reset = [](void *c)
      {
        GM(c, [&]()
        {
          MatcherCtx& mc = mctx(c);
          mc.facade->clear_handler(mc);
          mc.matcher->reset();
        });
      };
      table->input = [](void *c, const void *host_input)
      {
        GM(c, [&]() { am(c).input(*static_cast<const reflex::Input*>(host_input)); });
      };
      table->buffer = [](void *c, const unsigned char *base, size_t size)
      {
        GM(c, [&]() { am(c).buffer(const_cast<char*>(reinterpret_cast<const char*>(base)), size); });
      };
      table->buffer_all = [](void *c) -> int { return GM(c, [&]() -> int { return am(c).buffer(); }); };
      table->avail = [](void *c, size_t len) -> size_t { return GM(c, [&]() -> size_t { return am(c).avail(len); }); };
      table->begin_ptr = [](void *c, size_t len) -> const unsigned char*
      {
        return GM(c, [&]() -> const unsigned char*
        {
          reflex::AbstractMatcher& m = am(c);
          const char *ptr = m.begin();
          size_t end = Peek::end(m);
          size_t off = ptr - Peek::base(m);
          if (off > end || len > end - off)
            return NULL;
          return reinterpret_cast<const unsigned char*>(ptr);
        });
      };
      table->set_input_handler = [](void *c, int kind)
      {
        GM(c, [&]()
        {
          SeamFacade& f = *mctx(c).facade;
          reflex::AbstractMatcher& m = am(c);
          switch (kind)
          {
            case UGREP_CORE_INPUT_HANDLER_BINARY:
              m.in.set_handler(&f.grep.bin_handler);
              break;
#ifndef OS_WIN_OR_MINGW
            case UGREP_CORE_INPUT_HANDLER_STDIN:
              m.in.set_handler(&f.grep.stdin_handler);
              break;
#endif
            default:
              m.in.set_handler(NULL);
              break;
          }
        });
      };
      // matcher->buffer(src_bol, src_eol - src_bol + 1).find() on the buffer of another matcher, for cnf_matching
      table->find_in_line = [](void *c, void *src_c, size_t bol, size_t eol) -> int
      {
        return GM(c, [&]() -> int
        {
          reflex::AbstractMatcher& src = am(src_c);
          check(bol <= eol && eol <= Peek::end(src), "find_in_line: outside the buffer of the source matcher");
          return am(c).buffer(const_cast<char*>(Peek::base(src)) + bol, eol - bol + 1).find() != 0;
        });
      };
      return true;
    }

    // the matcher at (term, alt) of grep.matchers, NULL when there is none
    reflex::AbstractMatcher *cnf_matcher(size_t term, size_t alt) const
    {
      if (grep.matchers == NULL || term >= grep.matchers->size())
        return NULL;
      auto i = grep.matchers->begin();
      std::advance(i, term);
      if (alt >= i->size())
        return NULL;
      auto j = i->begin();
      std::advance(j, alt);
      return j->get();
    }

    // the buffer position of an offset in the buffer of Grep::matcher, for cnf_matching
    const char *position(size_t off) const
    {
      return Peek::base(*grep.matcher) + off;
    }

    // the data restline_data points into: the saved string restline or the data of the buffer of Grep::matcher
    void restline_region(const char *& lo, const char *& hi)
    {
      uintptr_t data = reinterpret_cast<uintptr_t>(grep.restline_data);
      uintptr_t saved = reinterpret_cast<uintptr_t>(grep.restline.c_str());
      if (data >= saved && data <= saved + grep.restline.size())
      {
        lo = grep.restline.c_str();
        hi = lo + grep.restline.size();
        return;
      }
      lo = Peek::base(*grep.matcher);
      hi = lo + Peek::end(*grep.matcher);
      check(data >= reinterpret_cast<uintptr_t>(lo) && data <= reinterpret_cast<uintptr_t>(hi), "restline_data: outside its buffer");
    }

    // restline_data[0..restline_size] must lie inside the data it points into before the members of Grep read it
    void check_restline()
    {
      if (grep.restline_data != NULL)
      {
        const char *lo, *hi;
        restline_region(lo, hi);
        check(grep.restline_size <= static_cast<size_t>(hi - grep.restline_data), "restline_size: past the end of the data");
      }
    }

    // fill the host table: every callback forwards to Grep, ctx is this facade
    void fill_env(ugrep_core_search_env *env)
    {
      memset(env, 0, sizeof(*env));
      env->ctx = this;
      env->member_get = [](void *c, ugrep_core_member member) -> size_t
      {
        return G(c, [&]() -> size_t
        {
          Grep& g = F(c).grep;
          switch (member)
          {
            case UGREP_CORE_MEMBER_MATCHES: return g.matches;
            case UGREP_CORE_MEMBER_LINENO: return g.lineno;
            case UGREP_CORE_MEMBER_HEADING: return g.heading;
            case UGREP_CORE_MEMBER_STOP: return g.stop;
            case UGREP_CORE_MEMBER_BINFILE: return g.binfile;
            case UGREP_CORE_MEMBER_HEX: return g.hex;
            case UGREP_CORE_MEMBER_BINARY: return g.binary;
            case UGREP_CORE_MEMBER_RESTLINE_SIZE: return g.restline_size;
            case UGREP_CORE_MEMBER_RESTLINE_LAST: return g.restline_last;
            case UGREP_CORE_MEMBER_INTERACTIVE: return g.interactive;
          }
          return 0;
        });
      };
      // restline_size is checked against the data when the data is used (restline_bytes, the restline output and save)
      env->member_set = [](void *c, ugrep_core_member member, size_t value)
      {
        G(c, [&]()
        {
          Grep& g = F(c).grep;
          switch (member)
          {
            case UGREP_CORE_MEMBER_MATCHES: g.matches = value; break;
            case UGREP_CORE_MEMBER_LINENO: g.lineno = value; break;
            case UGREP_CORE_MEMBER_HEADING: g.heading = value != 0; break;
            case UGREP_CORE_MEMBER_STOP: g.stop = value != 0; break;
            case UGREP_CORE_MEMBER_BINFILE: g.binfile = value != 0; break;
            case UGREP_CORE_MEMBER_HEX: g.hex = value != 0; break;
            case UGREP_CORE_MEMBER_BINARY: g.binary = value != 0; break;
            case UGREP_CORE_MEMBER_RESTLINE_SIZE: g.restline_size = value; break;
            case UGREP_CORE_MEMBER_RESTLINE_LAST: g.restline_last = value; break;
            case UGREP_CORE_MEMBER_INTERACTIVE: g.interactive = value != 0; break;
          }
        });
      };
      env->partname = [](void *c, size_t *len) -> const unsigned char*
      {
        *len = 0;
        return G(c, [&]() -> const unsigned char*
        {
          Grep& g = F(c).grep;
          *len = g.partname.size();
          return reinterpret_cast<const unsigned char*>(g.partname.c_str());
        });
      };
      env->restline_output = [](void *c, int kind, int invert, int hex_context)
      {
        G(c, [&]()
        {
          SeamFacade& f = F(c);
          f.check_restline();
          if (kind == 0)
            f.grep.output_restline(invert != 0, hex_context != 0);
          else
            f.grep.output_or_save_restline(invert != 0, hex_context != 0);
        });
      };
      env->restline_save = [](void *c)
      {
        G(c, [&]()
        {
          SeamFacade& f = F(c);
          check(f.grep.restline_data != NULL, "save_restline: no restline_data");
          f.check_restline();
          f.grep.save_restline();
        });
      };
      env->restline_data_get = [](void *c, size_t *off) -> int
      {
        *off = 0;
        return G(c, [&]() -> int
        {
          SeamFacade& f = F(c);
          if (f.grep.restline_data == NULL)
            return UGREP_CORE_RESTLINE_NONE;
          const char *lo, *hi;
          f.restline_region(lo, hi);
          *off = f.grep.restline_data - lo;
          return lo == f.grep.restline.c_str() ? UGREP_CORE_RESTLINE_SAVED : UGREP_CORE_RESTLINE_BUFFER;
        });
      };
      env->restline_data_set = [](void *c, int kind, size_t off)
      {
        G(c, [&]()
        {
          Grep& g = F(c).grep;
          if (kind == UGREP_CORE_RESTLINE_BUFFER)
          {
            check(off <= Peek::end(*g.matcher), "restline_data_set: offset outside the buffer");
            g.restline_data = Peek::base(*g.matcher) + off;
          }
          else if (kind == UGREP_CORE_RESTLINE_SAVED)
          {
            check(off <= g.restline.size(), "restline_data_set: offset outside restline");
            g.restline_data = g.restline.c_str() + off;
          }
          else
          {
            g.restline_data = NULL;
          }
        });
      };
      env->restline_advance = [](void *c, size_t n)
      {
        G(c, [&]()
        {
          SeamFacade& f = F(c);
          if (f.grep.restline_data == NULL)
          {
            check(n == 0, "restline_advance: no restline_data");
            return;
          }
          const char *lo, *hi;
          f.restline_region(lo, hi);
          check(n <= static_cast<size_t>(hi - f.grep.restline_data), "restline_advance: past the end of the data");
          f.grep.restline_data += n;
        });
      };
      env->restline_bytes = [](void *c, size_t *len) -> const unsigned char*
      {
        *len = 0;
        return G(c, [&]() -> const unsigned char*
        {
          SeamFacade& f = F(c);
          if (f.grep.restline_data == NULL)
            return NULL;
          f.check_restline();
          *len = f.grep.restline_size;
          return reinterpret_cast<const unsigned char*>(f.grep.restline_data);
        });
      };
      env->restline_set = [](void *c, const unsigned char *s, size_t len)
      {
        G(c, [&]()
        {
          Grep& g = F(c).grep;
          std::string temp(reinterpret_cast<const char*>(s), len);
          temp.swap(g.restline);
          g.restline_data = g.restline.c_str();
        });
      };
      env->handler_op = [](void *c, ugrep_core_handler_op op, size_t a, size_t b, size_t cc, size_t d)
      {
        G(c, [&]()
        {
          SeamFacade& f = F(c);
          const char *base = Peek::base(*f.grep.matcher);
          switch (op)
          {
            case UGREP_CORE_HANDLER_OP_OUTPUT_BEFORE_CONTEXT:
              if (f.context_handler != NULL)
                f.context_handler->output_before_context();
              else if (f.invert_context_handler != NULL)
                f.invert_context_handler->output_before_context();
              break;
            case UGREP_CORE_HANDLER_OP_SET_AFTER_LINENO:
              if (f.context_handler != NULL)
                f.context_handler->set_after_lineno(a);
              break;
            case UGREP_CORE_HANDLER_OP_ADD_BEFORE_CONTEXT_LINE:
              check(a <= b && b <= Peek::end(*f.grep.matcher), "add_before_context_line: outside the buffer");
              if (f.invert_context_handler != NULL)
                f.invert_context_handler->add_before_context_line(base + a, base + b, cc, d);
              break;
            case UGREP_CORE_HANDLER_OP_ADD_BEFORE_CONTEXT_MATCH:
              if (f.invert_context_handler != NULL)
              {
                // output_before_context reads ptr + pos for size bytes and computes pos - the end of the previous match: the
                // match must start at or after the previous match and end inside the before line it is added to
                auto& state = f.invert_context_handler->state;
                if (state.before_length > 0)
                {
                  const auto& before_line = state.before_lines[(state.before_index + state.before_length - 1) % state.before_length];
                  size_t prev_end = before_line.matches.empty() ? 0 : before_line.matches.back().pos + before_line.matches.back().size;
                  size_t line_size = before_line.line.size();
                  check(a >= prev_end && b <= line_size && a <= line_size - b, "add_before_context_match: outside the before line");
                }
                f.invert_context_handler->add_before_context_match(a, b, cc);
              }
              break;
          }
        });
      };
      env->cnf_matching = [](void *c, size_t bol, size_t eol, int acquire) -> int
      {
        return G(c, [&]() -> int
        {
          SeamFacade& f = F(c);
          check(bol <= eol && eol <= Peek::end(*f.grep.matcher), "cnf_matching: bol or eol outside the buffer");
          try
          {
            return f.grep.cnf_matching(f.position(bol), f.position(eol), acquire != 0) ? UGREP_CORE_CNF_YES : UGREP_CORE_CNF_NO;
          }
          catch (EXIT_SEARCH&)
          {
            return UGREP_CORE_CNF_EXIT;
          }
        });
      };
      env->cnf_satisfied = [](void *c, int acquire) -> int
      {
        return G(c, [&]() -> int
        {
          try
          {
            return F(c).grep.cnf_satisfied(acquire != 0) ? UGREP_CORE_CNF_YES : UGREP_CORE_CNF_NO;
          }
          catch (EXIT_SEARCH&)
          {
            return UGREP_CORE_CNF_EXIT;
          }
        });
      };
      env->matchers_len = [](void *c) -> size_t
      {
        return G(c, [&]() -> size_t
        {
          Grep& g = F(c).grep;
          return g.matchers != NULL ? g.matchers->size() : static_cast<size_t>(-1);
        });
      };
      // the first matcher of AND term index
      env->matchers_matcher = [](void *c, size_t index, ugrep_core_matcher_table *table) -> int
      {
        return G(c, [&]() -> int
        {
          SeamFacade& f = F(c);
          return f.fill_matcher_table(f.cnf_matcher(index, 0), table);
        });
      };
      env->matchers_reset = [](void *c)
      {
        G(c, [&]()
        {
          Grep& g = F(c).grep;
          if (g.matchers == NULL)
            return;
          size_t n = g.matchers->size();
          g.matching.resize(0);
          g.matching.resize(n);
          g.notmatching.resize(n);
          size_t j = 0;
          for (auto& i : *g.matchers)
          {
            g.notmatching[j].resize(0);
            g.notmatching[j].resize(i.size() > 0 ? i.size() - 1 : 0);
            ++j;
          }
        });
      };
      env->out_eof = [](void *c) -> int { return G(c, [&]() -> int { return F(c).grep.out.eof; }); };
      env->out_holding = [](void *c) -> int { return G(c, [&]() -> int { return F(c).grep.out.holding(); }); };
      env->out_op = [](void *c, ugrep_core_out_op op)
      {
        G(c, [&]()
        {
          SeamFacade& f = F(c);
          Output& out = f.grep.out;
          switch (op)
          {
            case UGREP_CORE_OUT_HOLD: out.hold(); break;
            case UGREP_CORE_OUT_ACQUIRE: out.acquire(); break;
            case UGREP_CORE_OUT_RELEASE: out.release(); break;
            case UGREP_CORE_OUT_CHECK_FLUSH: out.check_flush(); break;
            case UGREP_CORE_OUT_NL: out.nl(); break;
            case UGREP_CORE_OUT_NL_LF: out.nl(true); break;
            case UGREP_CORE_OUT_HEADER_FILE: out.header(f.current_pathname(), f.grep.partname); break;
            case UGREP_CORE_OUT_BINARY_FILE_MATCHES: out.binary_file_matches(f.current_pathname(), f.grep.partname); break;
            case UGREP_CORE_OUT_DUMP_DONE: out.dump.done(); break;
            case UGREP_CORE_OUT_LAUNCH: out.launch(); break;
          }
        });
      };
      env->out_chr = [](void *c, unsigned char ch) { G(c, [&]() { F(c).grep.out.chr(ch); }); };
      env->out_str = [](void *c, const unsigned char *s, size_t len)
      {
        G(c, [&]() { F(c).grep.out.str(reinterpret_cast<const char*>(s), len); });
      };
      env->out_utf8strn = [](void *c, const unsigned char *s, size_t len, size_t k)
      {
        G(c, [&]() { F(c).grep.out.utf8strn(reinterpret_cast<const char*>(s), len, k); });
      };
      env->out_num = [](void *c, size_t i, int width) { G(c, [&]() { F(c).grep.out.num(i, width); }); };
      env->out_header = [](void *c, size_t lineno, size_t byte_offset, const unsigned char *sep, size_t sep_len, int newline)
      {
        G(c, [&]()
        {
          SeamFacade& f = F(c);
          std::string separator(reinterpret_cast<const char*>(sep), sep_len);
          f.grep.out.header(f.current_pathname(), f.grep.partname, f.grep.heading, lineno, f.grep.matcher, byte_offset, separator.c_str(), newline != 0);
        });
      };
      env->out_format_count = [](void *c, const unsigned char *format, size_t len, size_t matches)
      {
        G(c, [&]()
        {
          std::string text(reinterpret_cast<const char*>(format), len);
          F(c).grep.out.format(text.c_str(), matches);
        });
      };
      env->out_format = [](void *c, const unsigned char *format, size_t len, size_t matches, size_t *matching, int body, int next) -> int
      {
        return G(c, [&]() -> int
        {
          SeamFacade& f = F(c);
          std::string text(reinterpret_cast<const char*>(format), len);
          return f.grep.out.format(text.c_str(), f.current_pathname(), f.grep.partname, matches, matching, f.grep.matcher, f.grep.heading, body != 0, next != 0);
        });
      };
      env->out_dump_hex = [](void *c, short mode, size_t byte_offset, const unsigned char *data, size_t len)
      {
        G(c, [&]() { F(c).grep.out.dump.hex(mode, byte_offset, reinterpret_cast<const char*>(data), len); });
      };
      env->out_dump_next = [](void *c, size_t byte_offset) { G(c, [&]() { F(c).grep.out.dump.next(byte_offset); }); };
      env->stats_found = [](void *c, int what) -> int
      {
        return G(c, [&]() -> int
        {
          switch (what)
          {
            case 0: return Stats::found_file();
            case 1: return Stats::found_part();
            default: Stats::undo_found_part(); return 0;
          }
        });
      };
      env->stats_found_parts = [](void *c) -> size_t { return G(c, [&]() -> size_t { return Stats::found_parts(); }); };
      env->stats_score_matches = [](void *c, size_t matches, size_t lines) { G(c, [&]() { Stats::score_matches(matches, lines); }); };

      // the file members: filename, file_in, input, Static::source, filter magic, warning, zthread (no mmap: it is Rust)
      env->filename = [](void *c, size_t *len) -> const unsigned char*
      {
        *len = 0;
        return G(c, [&]() -> const unsigned char*
        {
          Grep& g = F(c).grep;
          *len = g.filename != NULL ? strlen(g.filename) : 0;
          return reinterpret_cast<const unsigned char*>(g.filename);
        });
      };
      env->file_in = [](void *c) -> void* { return G(c, [&]() -> void* { return F(c).grep.file_in; }); };
      env->set_file_in = [](void *c, void *file) { G(c, [&]() { F(c).grep.file_in = static_cast<FILE*>(file); }); };
      env->input = [](void *c) -> void* { return G(c, [&]() -> void* { return &F(c).grep.input; }); };
      env->input_file = [](void *c) -> void* { return G(c, [&]() -> void* { return F(c).grep.input.file(); }); };
      env->input_set_file = [](void *c, void *file, int encoding)
      {
        G(c, [&]()
        {
          F(c).grep.input = reflex::Input(stream(file, "input_set_file: NULL file"), static_cast<reflex::Input::file_encoding_type>(encoding));
        });
      };
      env->input_clear = [](void *c) { G(c, [&]() { F(c).grep.input.clear(); }); };
      env->input_get = [](void *c, unsigned char *buf, size_t len) -> size_t
      {
        return G(c, [&]() -> size_t { return F(c).grep.input.get(reinterpret_cast<char*>(buf), len); });
      };
      env->source = [](void *c) -> void* { return G(c, [&]() -> void* { return Static::source; }); };
      env->stdin_file = [](void *c) -> void* { return G(c, [&]() -> void* { return stdin; }); };
      // scan the magic pattern on the file and rewind it, as Grep::filter does
      env->filter_magic_scan = [](void *c, void *file) -> size_t
      {
        return G(c, [&]() -> size_t
        {
          FILE *in = stream(file, "filter_magic_scan: NULL file");
          size_t match = reflex::Matcher(Static::filter_magic_pattern, in).scan();
          rewind(in);
          return match;
        });
      };
      env->warning = [](void *c, const char *message, const char *arg) { G(c, [&]() { warning(message, arg); }); };
      // the slots are never NULL: without the decompression thread they are stubs (the Rust side only calls them when
      // flag_decompress is set, which needs HAVE_LIBZ)
#if defined(HAVE_LIBZ) && defined(WITH_DECOMPRESSION_THREAD)
      env->zthread_start = [](void *c, size_t ztstage, const char *path, const char *find) -> void*
      {
        return G(c, [&]() -> void* { return F(c).grep.zthread.start(ztstage, path, stream(F(c).grep.file_in, "zthread_start: NULL file_in"), find); });
      };
      env->zthread_open_next = [](void *c, const char *path) -> void*
      {
        return G(c, [&]() -> void* { return F(c).grep.zthread.open_next(path); });
      };
      env->zthread_cancel = [](void *c) { G(c, [&]() { F(c).grep.zthread.cancel(); }); };
#else
      env->zthread_start = [](void *, size_t, const char *, const char *) -> void* { return NULL; };
      env->zthread_open_next = [](void *, const char *) -> void* { return NULL; };
      env->zthread_cancel = [](void *) { };
#endif
      env->cnf_alts = [](void *c, size_t term) -> size_t
      {
        return G(c, [&]() -> size_t
        {
          Grep& g = F(c).grep;
          if (g.matchers == NULL || term >= g.matchers->size())
            return 0;
          auto i = g.matchers->begin();
          std::advance(i, term);
          return i->size();
        });
      };
      env->cnf_alt_matcher = [](void *c, size_t term, size_t alt, ugrep_core_matcher_table *table) -> int
      {
        return G(c, [&]() -> int
        {
          SeamFacade& f = F(c);
          return f.fill_matcher_table(f.cnf_matcher(term, alt), table);
        });
      };
      env->cnf_bit_get = [](void *c, int which, size_t k, size_t l) -> int
      {
        return G(c, [&]() -> int
        {
          Grep& g = F(c).grep;
          if (which == UGREP_CORE_CNF_MATCHING)
            return k < g.matching.size() ? g.matching[k] : 0;
          return k < g.notmatching.size() && l < g.notmatching[k].size() ? g.notmatching[k][l] : 0;
        });
      };
      env->cnf_bit_set = [](void *c, int which, size_t k, size_t l)
      {
        G(c, [&]()
        {
          Grep& g = F(c).grep;
          if (which == UGREP_CORE_CNF_MATCHING)
          {
            check(k < g.matching.size(), "cnf_bit_set: term out of range");
            g.matching[k] = true;
          }
          else
          {
            check(k < g.notmatching.size() && l < g.notmatching[k].size(), "cnf_bit_set: term or alternative out of range");
            g.notmatching[k][l] = true;
          }
        });
      };
      // not guarded: it reports the channel
      env->error_pending = [](void *c) -> int { return F(c).error != nullptr; };
      env->input_encoding = [](void *c) -> int { return G(c, [&]() -> int { return F(c).grep.input.file_encoding(); }); };
      env->encoding_null_data = [](void *c) -> int
      {
        return G(c, [&]() -> int { return static_cast<int>(reflex::Input::file_encoding::null_data); });
      };
      env->flag_filter = [](void *c, size_t *len) -> const unsigned char*
      {
        *len = 0;
        return G(c, [&]() -> const unsigned char*
        {
          *len = flag_filter.size();
          return reinterpret_cast<const unsigned char*>(flag_filter.c_str());
        });
      };
      env->flag_filter_magic_label = [](void *c, size_t index, size_t *len) -> const unsigned char*
      {
        *len = 0;
        return G(c, [&]() -> const unsigned char*
        {
          if (index >= flag_filter_magic_label.size())
            return NULL;
          *len = flag_filter_magic_label[index].size();
          return reinterpret_cast<const unsigned char*>(flag_filter_magic_label[index].c_str());
        });
      };
      env->file_open = [](void *c, const char *path) -> void*
      {
        return G(c, [&]() -> void*
        {
          FILE *file = NULL;
          if (fopenw_s(&file, path, "rb") != 0)
          {
            warning("cannot read", path);
            return NULL;
          }
          return file;
        });
      };
      env->file_close = [](void *c, void *file) { G(c, [&]() { fclose(stream(file, "file_close: NULL file")); }); };
      env->file_read_error = [](void *c, void *file, const char *path) -> int
      {
        return G(c, [&]() -> int
        {
          if (!ferror(stream(file, "file_read_error: NULL file")))
            return 0;
          warning("cannot read", path);
          return 1;
        });
      };
      env->file_set_binary = [](void *c, void *file)
      {
        G(c, [&]()
        {
#ifdef OS_WIN
          (void)_setmode(fileno(stream(file, "file_set_binary: NULL file")), _O_BINARY);
#else
          (void)stream(file, "file_set_binary: NULL file");
#endif
        });
      };
      env->file_nonblock_regular = [](void *c, void *file)
      {
        G(c, [&]()
        {
#if defined(__linux__)
          int fd = fileno(stream(file, "file_nonblock_regular: NULL file"));
          struct stat buf;

          if (fstat(fd, &buf) == 0 && S_ISREG(buf.st_mode))
            fcntl(fd, F_SETFL, fcntl(fd, F_GETFL) | O_NONBLOCK);
#else
          (void)stream(file, "file_nonblock_regular: NULL file");
#endif
        });
      };
      env->file_probe_interactive = [](void *c, void *file) -> int
      {
        return G(c, [&]() -> int
        {
#ifndef OS_WIN_OR_MINGW
          int fd = fileno(stream(file, "file_probe_interactive: NULL file"));
          struct stat buf;

          if (fstat(fd, &buf) == 0 && (S_ISCHR(buf.st_mode) || S_ISFIFO(buf.st_mode) || S_ISSOCK(buf.st_mode)))
            return fcntl(fd, F_SETFL, fcntl(fd, F_GETFL) | O_NONBLOCK) != -1;
#else
          (void)stream(file, "file_probe_interactive: NULL file");
#endif
          return 0;
        });
      };
      env->file_seek_set = [](void *c, void *file) -> int
      {
        return G(c, [&]() -> int { return fseek(stream(file, "file_seek_set: NULL file"), 0, SEEK_SET) == 0; });
      };
      env->stdin_drain_check = [](void *c) -> int
      {
        return G(c, [&]() -> int
        {
#if defined(WITH_STDIN_DRAIN) && !defined(OS_WIN_OR_MINGW)
          return !feof(stdin) && fseek(stdin, 0, SEEK_END) < 0 && errno != EINVAL;
#else
          return 0;
#endif
        });
      };
      env->filter_spawn = [](void *c, void *file, const char *path, const char *command, void **out) -> int
      {
        *out = NULL;
        return G(c, [&]() -> int
        {
          FILE *in = stream(file, "filter_spawn: NULL file");
          bool ok = F(c).grep.filter_spawn(in, path, command);
          *out = in;
          return ok;
        });
      };
      env->is_binary = [](void *c, const unsigned char *bytes, size_t len) -> int
      {
        return G(c, [&]() -> int { return is_binary(reinterpret_cast<const char*>(bytes), len); });
      };
      // the close of close_file(): the stream stays in C++, Rust never holds it
      env->close_input_file = [](void *c)
      {
        G(c, [&]()
        {
          SeamFacade& f = F(c);
          FILE *file = f.grep.input.file();
          if (file != NULL)
          {
            f.detach_matchers(file);
            fclose(file);
          }
          f.grep.input.clear();
          f.cancel_zthread_if_blocked();
        });
      };
      env->close_file_in = [](void *c)
      {
        G(c, [&]()
        {
          SeamFacade& f = F(c);
          FILE *file = f.grep.file_in;
          if (file != NULL)
          {
            f.cancel_zthread_if_blocked();
            f.detach_matchers(file);
            if (file != stdin && file != Static::source)
              fclose(file);
            f.grep.file_in = NULL;
          }
          f.grep.input.clear();
        });
      };
    }

    static SeamFacade& F(void *ctx) { return *static_cast<SeamFacade*>(ctx); }

    // the pathname of the search being bound, NULL when none
    const char *current_pathname() const { return path_slot; }

    Grep&                  grep;                   // the Grep object
    const char            *path_slot;              // the pathname of the search in progress, see bind(); handlers refer to it
    std::exception_ptr     error;                  // the pending exception of the exception channel, see guard()
    ContextGrepHandler    *context_handler;        // the registered handler when of this class, owned by its MatcherCtx
    InvertContextGrepHandler *invert_context_handler; // the registered handler when of this class
    std::deque<MatcherCtx> matcher_ctxs;           // one per matcher handed out
    rust::Box<ugrep_rust::GrepState> state;        // the Rust state of the Grep lifetime (the mmap mapping), freed with the facade
  };

  const char                    *filename;      // the name of the file being searched
  std::string                    partname;      // the name of an extracted file from an archive
  std::string                    restline;      // a buffer to store the rest of a line to search
  const char                    *restline_data; // rest of the line data pointer or NULL
  size_t                         restline_size; // rest of the line size
  size_t                         restline_last; // rest of the line last byte offset
  Output                         out;           // asynchronous output
  reflex::AbstractMatcher       *matcher;       // the pattern matcher we're using, never NULL
  Static::Matchers              *matchers;      // the CNF of AND/OR/NOT matchers or NULL
  std::vector<bool>              matching;      // bitmap to keep track of globally matching CNF terms
  std::vector<std::vector<bool>> notmatching;   // bitmap to keep track of globally matching OR NOT CNF terms
  MMap                           mmap;          // mmap state
  reflex::Input                  input;         // input to the matcher
  FILE                          *file_in;       // the current input file
  bool                           interactive;   // the file being searched is a TTY or pipe
  bool                           binfile;       // the file being searched is a binary file
  size_t                         lineno;        // the line number of the last match
  bool                           heading;       // heading on/off, initially set to --with-filename
  bool                           hex;           // the match is output in hex
  bool                           binary;        // the match is binary as per option -X or -W
  size_t                         matches;       // number of matches
  bool                           stop;          // stop searching when --max-files max reached
  BinaryHandler                  bin_handler;   // a handler to detect binary input like GNU grep
#ifndef OS_WIN_OR_MINGW
  StdInHandler                   stdin_handler; // a handler to handle non-blocking input from a TTY or a slow pipe
#endif
#ifdef HAVE_LIBZ
#ifdef WITH_DECOMPRESSION_THREAD
  Zthread                        zthread;
#else
  zstreambuf                    *zstream;       // the decompressed stream buffer from compressed input
  std::istream                  *stream;        // input stream is the decompressed zstream
#endif
#endif

};

struct GrepWorker;

// master submits jobs to workers and implements operations to support job stealing
struct GrepMaster : public Grep {

  GrepMaster(FILE *file, reflex::AbstractMatcher *matcher, Static::Matchers *matchers)
    :
      Grep(file, matcher, matchers),
      sync(flag_sort_key == Sort::NA ? Output::Sync::Mode::UNORDERED : Output::Sync::Mode::ORDERED)
  {
    // master and workers synchronize their output
    out.sync_on(&sync);

    // set global handle to be able to call cancel_ugrep()
    Static::set_grep_handle(this);

    if (Static::cores >= 8)
      set_this_thread_affinity_and_priority(Static::cores - 1);

    start_workers();

    iworker = workers.begin();
  }

  virtual ~GrepMaster()
  {
    stop_workers();
    Static::clear_grep_handle();
  }

  // clone the pattern matcher - the caller is responsible to deallocate the returned matcher
  reflex::AbstractMatcher *matcher_clone() const
  {
    return matcher->clone();
  }

  // clone the CNF of AND/OR/NOT matchers - the caller is responsible to deallocate the returned list of matchers if not NULL
  Static::Matchers *matchers_clone() const
  {
    if (matchers == NULL)
      return NULL;

    Static::Matchers *new_matchers = new Static::Matchers;

    for (const auto& i : *matchers)
    {
      new_matchers->emplace_back();

      auto& last = new_matchers->back();

      for (const auto& j : i)
      {
        if (j)
          last.emplace_back(j->clone());
        else
          last.emplace_back();
      }
    }

    return new_matchers;
  }

  // search a file by submitting it as a job to a worker
  void search(const char *pathname, uint16_t cost) override
  {
    submit(pathname, cost);
  }

  // start worker threads
  void start_workers();

  // stop all workers
  void stop_workers();

  // submit a job with a pathname to a worker, workers are visited round-robin
  void submit(const char *pathname, uint16_t cost);

  // job stealing on behalf of a worker from a co-worker with at least --min-steal jobs still to do
  bool steal(GrepWorker *worker);

  std::list<GrepWorker>           workers; // workers running threads
  std::list<GrepWorker>::iterator iworker; // the next worker to submit a job to
  Output::Sync                    sync;    // sync output of workers

};

// worker runs a thread to execute jobs submitted by the master
struct GrepWorker : public Grep {

  GrepWorker(FILE *file, size_t id, GrepMaster *master)
    :
      Grep(file, master->matcher_clone(), master->matchers_clone()),
      id(id),
      master(master)
  {
    // all workers synchronize their output on the master's sync object
    out.sync_on(&master->sync);

    // run worker thread executing jobs assigned to its queue
    thread = std::thread(&GrepWorker::execute, this);
  }

  virtual ~GrepWorker()
  {
    // delete the cloned matcher
    delete matcher;

    // delete the cloned matchers, if any
    if (matchers != NULL)
      delete matchers;
  }

  // worker thread execution
  void execute();

  // submit Job::NONE sentinel to this worker
  void submit_job()
  {
    jobs.enqueue();
  }

  // submit a job to this worker
  void submit_job(const char *pathname, uint16_t cost, size_t slot)
  {
    jobs.enqueue(pathname, cost, slot);
  }

  // receive a job for this worker, wait until one arrives
  void next_job(Job& job)
  {
    jobs.dequeue(job);
  }

  // submit Job::NONE sentinel to stop this worker
  void stop()
  {
    submit_job();
  }

  std::thread             thread;      // thread of this worker, spawns GrepWorker::execute()
  const size_t            id;          // worker number 0 and up
  GrepMaster             *master;      // the master of this worker
  JobQueue                jobs;        // queue of pending jobs submitted to this worker
};

// start worker threads
void GrepMaster::start_workers()
{
  size_t num;

  // create worker threads
  try
  {
    for (num = 0; num < Static::threads; ++num)
      workers.emplace(workers.end(), out.file, num, this);
  }

  // if sufficient resources are not available then reduce the number of threads to the number of active workers created
  catch (std::system_error& error)
  {
    if (error.code() != std::errc::resource_unavailable_try_again)
      throw;

    Static::threads = num;
  }
}

// stop all workers
void GrepMaster::stop_workers()
{
  // submit Job::NONE sentinel to workers
  for (auto& worker : workers)
    worker.stop();

  // wait for workers to join
  for (auto& worker : workers)
    worker.thread.join();
}

// submit a job with a pathname to a worker
void GrepMaster::submit(const char *pathname, uint16_t cost)
{
  while (true)
  {
    size_t min_todo = iworker->jobs.todo;

    // find a worker with the minimum number of jobs
    if (min_todo > 0)
    {
      auto min_worker = iworker;

      for (size_t num = 1; num < Static::threads; ++num)
      {
        ++iworker;
        if (iworker == workers.end())
          iworker = workers.begin();

        if (iworker->jobs.todo < min_todo)
        {
          min_todo = iworker->jobs.todo;
          if (min_todo == 0)
            break;

          min_worker = iworker;
        }
      }

      iworker = min_worker;
    }

#ifdef WITH_LOCK_FREE_JOB_QUEUE

    if (iworker->try_submit_job(pathname, cost, sync.next) || out.eof || out.cancelled())
      break;

    // give the worker threads some slack to make progress, then try again
    std::this_thread::sleep_for(std::chrono::milliseconds(1));

#else

    // give the worker threads some slack to make progress when their queues reached a soft max size
    if (min_todo > flag_max_queue && flag_max_queue > 0)
      std::this_thread::sleep_for(std::chrono::milliseconds(1));

    // submit job to worker, do not loop
    iworker->submit_job(pathname, cost, sync.next);
    break;

#endif
  }

  ++sync.next;

  // around we go
  ++iworker;
  if (iworker == workers.end())
    iworker = workers.begin();
}

#ifndef WITH_LOCK_FREE_JOB_QUEUE

// job stealing on behalf of a worker from a co-worker with at least --min-steal jobs still to do
bool GrepMaster::steal(GrepWorker *worker)
{
  // try to steal a job from a co-worker with the most jobs
  auto coworker = workers.begin();
  auto max_worker = coworker;
  size_t max_todo = 0;

  for (size_t i = 0; i < Static::threads; ++i)
  {
    if (&*coworker != worker && coworker->jobs.todo > max_todo)
    {
      max_todo = coworker->jobs.todo;
      max_worker = coworker;
    }

    // around we go
    ++coworker;
    if (coworker == workers.end())
      coworker = workers.begin();
  }

  // not enough jobs in the co-worker's queue to steal from
  if (max_todo < flag_min_steal)
    return false;

  coworker = max_worker;

  Job job;

  // steal a job from the co-worker for this worker
  if (coworker->jobs.steal_job(job))
  {
    worker->jobs.move_job(job);

    return true;
  }

  // couldn't steal any job
  return false;
}

#endif

// execute worker thread
void GrepWorker::execute()
{
  if (Static::cores >= 3)
    set_this_thread_affinity_and_priority(id);

  Job job;

  while (!out.eof && !out.cancelled())
  {
    // wait for next job
    next_job(job);

    // worker should stop?
    if (job.none())
      break;

    // start synchronizing output for this job slot in ORDERED mode (--sort)
    out.begin(job.slot);

    // search the file for this job, an empty pathname means stdin
    search(job.pathname.empty() ? Static::LABEL_STANDARD_INPUT : job.pathname.c_str(), job.cost);

    // end output in ORDERED mode (--sort) for this job slot
    out.end();

#ifndef WITH_LOCK_FREE_JOB_QUEUE
    // if only one job is left to do or nothing to do, then try stealing another job from a co-worker
    if (jobs.todo <= 1)
      master->steal(this);
#endif
  }
}

// the CNF of Boolean search queries and patterns
CNF Static::bcnf;

// unique address to identify standard input path
const char *Static::LABEL_STANDARD_INPUT = "(standard input)";

// unique address to identify color and pretty WHEN arguments
const char *Static::NEVER = "never";
const char *Static::ALWAYS = "always";
const char *Static::AUTO = "auto";

// pointer to the --index pattern DFA with HFA constructed before threads start
const reflex::Pattern *Static::index_pattern = NULL; // concurrent access is thread safe

// the -M MAGIC pattern DFA constructed before threads start, read-only afterwards
reflex::Pattern Static::magic_pattern; // concurrent access is thread safe
reflex::Matcher Static::magic_matcher; // concurrent access is not thread safe

// the --filter-magic-label pattern DFA
reflex::Pattern Static::filter_magic_pattern; // concurrent access is thread safe

// ugrep command-line arguments pointing to argv[]
const char *Static::arg_pattern = NULL;
std::vector<const char*> Static::arg_files;

// number of cores
size_t Static::cores;

// number of concurrent threads for workers
size_t Static::threads;

// number of warnings given
std::atomic_size_t Static::warnings;

// redirectable source is standard input by default or a pipe
FILE *Static::source = stdin;

// redirectable output destination is standard output by default or a pipe
FILE *Static::output = stdout;

// redirectable error output destination is standard error by default or a pipe
FILE *Static::errout = stderr;

// full home directory path or NULL to expand ~ in options with path arguments
const char *Static::home_dir = NULL;

// Grep object handle, to cancel the search with cancel_ugrep()
struct Grep *Static::grep_handle = NULL;
std::mutex Static::grep_handle_mutex;

// patterns
reflex::Pattern Static::reflex_pattern;
std::string Static::string_pattern;
std::list<reflex::Pattern> Static::reflex_patterns;
std::list<std::string> Static::string_patterns;

// the LineMatcher, PCRE2Matcher, FuzzyMatcher or Matcher, concurrent access is not thread safe
std::unique_ptr<reflex::AbstractMatcher> Static::matcher;

// the CNF of AND/OR/NOT matchers or NULL, concurrent access is not thread safe
Static::Matchers Static::matchers;

// table of RE/flex file encodings for option --encoding (may be specified in any case)
const Encoding encoding_table[] = {
  { "binary",      reflex::Input::file_encoding::plain      },
  { "ASCII",       reflex::Input::file_encoding::utf8       },
  { "UTF-8",       reflex::Input::file_encoding::utf8       },
  { "UTF-16",      reflex::Input::file_encoding::utf16be    },
  { "UTF-16BE",    reflex::Input::file_encoding::utf16be    },
  { "UTF-16LE",    reflex::Input::file_encoding::utf16le    },
  { "UTF-32",      reflex::Input::file_encoding::utf32be    },
  { "UTF-32BE",    reflex::Input::file_encoding::utf32be    },
  { "UTF-32LE",    reflex::Input::file_encoding::utf32le    },
  { "LATIN1",      reflex::Input::file_encoding::latin      },
  { "ISO-8859-1",  reflex::Input::file_encoding::latin      },
  { "ISO-8859-2",  reflex::Input::file_encoding::iso8859_2  },
  { "ISO-8859-3",  reflex::Input::file_encoding::iso8859_3  },
  { "ISO-8859-4",  reflex::Input::file_encoding::iso8859_4  },
  { "ISO-8859-5",  reflex::Input::file_encoding::iso8859_5  },
  { "ISO-8859-6",  reflex::Input::file_encoding::iso8859_6  },
  { "ISO-8859-7",  reflex::Input::file_encoding::iso8859_7  },
  { "ISO-8859-8",  reflex::Input::file_encoding::iso8859_8  },
  { "ISO-8859-9",  reflex::Input::file_encoding::iso8859_9  },
  { "ISO-8859-10", reflex::Input::file_encoding::iso8859_10 },
  { "ISO-8859-11", reflex::Input::file_encoding::iso8859_11 },
  { "ISO-8859-13", reflex::Input::file_encoding::iso8859_13 },
  { "ISO-8859-14", reflex::Input::file_encoding::iso8859_14 },
  { "ISO-8859-15", reflex::Input::file_encoding::iso8859_15 },
  { "ISO-8859-16", reflex::Input::file_encoding::iso8859_16 },
  { "MAC",         reflex::Input::file_encoding::macroman   },
  { "MACROMAN",    reflex::Input::file_encoding::macroman   },
  { "EBCDIC",      reflex::Input::file_encoding::ebcdic     },
  { "CP437",       reflex::Input::file_encoding::cp437      },
  { "CP850",       reflex::Input::file_encoding::cp850      },
  { "CP858",       reflex::Input::file_encoding::cp858      },
  { "CP1250",      reflex::Input::file_encoding::cp1250     },
  { "CP1251",      reflex::Input::file_encoding::cp1251     },
  { "CP1252",      reflex::Input::file_encoding::cp1252     },
  { "CP1253",      reflex::Input::file_encoding::cp1253     },
  { "CP1254",      reflex::Input::file_encoding::cp1254     },
  { "CP1255",      reflex::Input::file_encoding::cp1255     },
  { "CP1256",      reflex::Input::file_encoding::cp1256     },
  { "CP1257",      reflex::Input::file_encoding::cp1257     },
  { "CP1258",      reflex::Input::file_encoding::cp1258     },
  { "KOI8-R",      reflex::Input::file_encoding::koi8_r     },
  { "KOI8-U",      reflex::Input::file_encoding::koi8_u     },
  { "KOI8-RU",     reflex::Input::file_encoding::koi8_ru    },
  { "null-data",   reflex::Input::file_encoding::null_data  },
  { NULL, 0 }
};

// table of file types for option -t, --file-type
const Type type_table[] = {
  { "actionscript", "as,mxml", NULL,                                                  NULL },
  { "ada",          "ada,adb,ads", NULL,                                              NULL },
  { "adoc",         "adoc", NULL,                                                     NULL },
  { "asm",          "asm,s,S", NULL,                                                  NULL },
  { "asp",          "asp", NULL,                                                      NULL },
  { "aspx",         "master,ascx,asmx,aspx,svc", NULL,                                NULL },
  { "autoconf",     "ac,in", NULL,                                                    NULL },
  { "automake",     "am,in", NULL,                                                    NULL },
  { "awk",          "awk", NULL,                                                      NULL },
  { "Awk",          "awk", NULL,                                                      "#!\\h*/.*\\Wg?awk(\\W.*)?\\n" },
  { "basic",        "bas,BAS,cls,frm,ctl,vb,resx", NULL,                              NULL },
  { "batch",        "bat,BAT,cmd,CMD", NULL,                                          NULL },
  { "bison",        "y,yy,ymm,ypp,yxx", NULL,                                         NULL },
  { "c",            "c,h,H,hdl,xs", NULL,                                             NULL },
  { "c++",          "cpp,CPP,cc,cxx,CXX,h,hh,H,hpp,hxx,Hxx,HXX", NULL,                NULL },
  { "clojure",      "clj", NULL,                                                      NULL },
  { "cpp",          "cpp,CPP,cc,cxx,CXX,h,hh,H,hpp,hxx,Hxx,HXX", NULL,                NULL },
  { "csharp",       "cs", NULL,                                                       NULL },
  { "css",          "css", NULL,                                                      NULL },
  { "csv",          "csv", NULL,                                                      NULL },
  { "dart",         "dart", NULL,                                                     NULL },
  { "Dart",         "dart", NULL,                                                     "#!\\h*/.*\\Wdart(\\W.*)?\\n" },
  { "delphi",       "pas,int,dfm,nfm,dof,dpk,dproj,groupproj,bdsgroup,bdsproj", NULL, NULL },
  { "elisp",        "el", NULL,                                                       NULL },
  { "elixir",       "ex,exs", NULL,                                                   NULL },
  { "erlang",       "erl,hrl", NULL,                                                  NULL },
  { "fortran",      "for,ftn,fpp,f,F,f77,F77,f90,F90,f95,F95,f03,F03", NULL,          NULL },
  { "gif",          "gif", NULL,                                                      NULL },
  { "Gif",          "gif", NULL,                                                      "GIF87a|GIF89a" },
  { "go",           "go", NULL,                                                       NULL },
  { "groovy",       "groovy,gtmpl,gpp,grunit,gradle", NULL,                           NULL },
  { "gsp",          "gsp", NULL,                                                      NULL },
  { "haskell",      "hs,lhs", NULL,                                                   NULL },
  { "html",         "htm,html,xhtml", NULL,                                           NULL },
  { "jade",         "jade", NULL,                                                     NULL },
  { "java",         "java,properties", NULL,                                          NULL },
  { "jpeg",         "jpg,jpeg", NULL,                                                 NULL },
  { "Jpeg",         "jpg,jpeg", NULL,                                                 "\\xff\\xd8\\xff[\\xdb\\xe0\\xe1\\xee]" },
  { "js",           "js", NULL,                                                       NULL },
  { "json",         "json", NULL,                                                     NULL },
  { "jsp",          "jsp,jspx,jthm,jhtml", NULL,                                      NULL },
  { "julia",        "jl", NULL,                                                       NULL },
  { "kotlin",       "kt,kts", NULL,                                                   NULL },
  { "less",         "less", NULL,                                                     NULL },
  { "lex",          "l,ll,lmm,lpp,lxx", NULL,                                         NULL },
  { "lisp",         "lisp,lsp", NULL,                                                 NULL },
  { "lua",          "lua", NULL,                                                      NULL },
  { "m4",           "m4", NULL,                                                       NULL },
  { "make",         "mk,mak", "makefile,Makefile,Makefile.Debug,Makefile.Release",    NULL },
  { "markdown",     "md", NULL,                                                       NULL },
  { "matlab",       "m", NULL,                                                        NULL },
  { "node",         "js", NULL,                                                       NULL },
  { "Node",         "js", NULL,                                                       "#!\\h*/.*\\Wnode(\\W.*)?\\n" },
  { "objc",         "m,h", NULL,                                                      NULL },
  { "objc++",       "mm,h", NULL,                                                     NULL },
  { "ocaml",        "ml,mli,mll,mly", NULL,                                           NULL },
  { "parrot",       "pir,pasm,pmc,ops,pod,pg,tg", NULL,                               NULL },
  { "pascal",       "pas,pp", NULL,                                                   NULL },
  { "pdf",          "pdf", NULL,                                                      NULL },
  { "Pdf",          "pdf", NULL,                                                      "\\x25\\x50\\x44\\x46\\x2d" },
  { "perl",         "pl,PL,pm,pod,t,psgi", NULL,                                      NULL },
  { "Perl",         "pl,PL,pm,pod,t,psgi", NULL,                                      "#!\\h*/.*\\Wperl(\\W.*)?\\n" },
  { "php",          "php,php3,php4,phtml", NULL,                                      NULL },
  { "Php",          "php,php3,php4,phtml", NULL,                                      "#!\\h*/.*\\Wphp(\\W.*)?\\n" },
  { "png",          "png", NULL,                                                      NULL },
  { "Png",          "png", NULL,                                                      "\\x89PNG\\x0d\\x0a\\x1a\\x0a" },
  { "prolog",       "pl,pro", NULL,                                                   NULL },
  { "python",       "py", NULL,                                                       NULL },
  { "Python",       "py", NULL,                                                       "#!\\h*/.*\\Wpython[23]?(\\W.*)?\\n" },
  { "r",            "R", NULL,                                                        NULL },
  { "rpm",          "rpm", NULL,                                                      NULL },
  { "Rpm",          "rpm", NULL,                                                      "\\xed\\xab\\xee\\xdb" },
  { "rst",          "rst", NULL,                                                      NULL },
  { "rtf",          "rtf", NULL,                                                      NULL },
  { "Rtf",          "rtf", NULL,                                                      "\\{\\rtf1" },
  { "ruby",         "rb,rhtml,rjs,rxml,erb,rake,spec", "Rakefile",                    NULL },
  { "Ruby",         "rb,rhtml,rjs,rxml,erb,rake,spec", "Rakefile",                    "#!\\h*/.*\\Wruby(\\W.*)?\\n" },
  { "rust",         "rs", NULL,                                                       NULL },
  { "scala",        "scala", NULL,                                                    NULL },
  { "scheme",       "scm,ss", NULL,                                                   NULL },
  { "seed7",        "sd7,s7i", NULL,                                                  NULL },
  { "shell",        "sh,bash,dash,csh,tcsh,ksh,zsh,fish", NULL,                       NULL },
  { "Shell",        "sh,bash,dash,csh,tcsh,ksh,zsh,fish", NULL,                       "#!\\h*/.*\\W(ba|da|t?c|k|z|fi)?sh(\\W.*)?\\n" },
  { "smalltalk",    "st", NULL,                                                       NULL },
  { "sql",          "sql,ctl", NULL,                                                  NULL },
  { "svg",          "svg", NULL,                                                      NULL },
  { "swift",        "swift", NULL,                                                    NULL },
  { "tcl",          "tcl,itcl,itk", NULL,                                             NULL },
  { "tex",          "tex,cls,sty,bib", NULL,                                          NULL },
  { "text",         "text,txt,TXT,md,rst,adoc", NULL,                                 NULL },
  { "tiff",         "tif,tiff", NULL,                                                 NULL },
  { "Tiff",         "tif,tiff", NULL,                                                 "\\x49\\x49\\x2a\\x00|\\x4d\\x4d\\x00\\x2a" },
  { "tt",           "tt,tt2,ttml", NULL,                                              NULL },
  { "typescript",   "ts,tsx", NULL,                                                   NULL },
  { "verilog",      "v,vh,sv", NULL,                                                  NULL },
  { "vhdl",         "vhd,vhdl", NULL,                                                 NULL },
  { "vim",          "vim", NULL,                                                      NULL },
  { "xml",          "xml,xsd,xsl,xslt,wsdl,rss,svg,ent,plist", NULL,                  NULL },
  { "Xml",          "xml,xsd,xsl,xslt,wsdl,rss,svg,ent,plist", NULL,                  "<\\?xml " },
  { "yacc",         "y", NULL,                                                        NULL },
  { "yaml",         "yaml,yml", NULL,                                                 NULL },
  { "zig",          "zig,zon", NULL,                                                  NULL },
  { NULL,           NULL, NULL,                                                       NULL }
};

#ifdef OS_WIN_OR_MINGW
// ugrep main() for windows to support wide string arguments and globbing
#ifdef OS_WIN
// ugrep wmain() for VS
int wmain(int argc, const wchar_t **wargv)
#else
// ugrep main() for MinGW, pick up argc and wargv from CommandLineToArgvW()
int main()
#endif
#else
// ugrep main() for non-windows
int main(int argc, const char **argv)
#endif
{
#ifdef OS_WIN_OR_MINGW

#ifdef OS_WIN_MINGW
  int argc = 0;
  wchar_t **wargv = CommandLineToArgvW(GetCommandLineW(), &argc);
#endif

  // store UTF-8 arguments for the duration of main() and convert Unicode command line arguments wargv[] to UTF-8 arguments argv[]
  const char **argv = new const char *[argc];
  for (int i = 0; i < argc; ++i)
  {
    arg_strings.emplace_back(utf8_encode(wargv[i]));
    argv[i] = arg_strings.back().c_str();
  }

  // handle CTRL-C
  SetConsoleCtrlHandler(&sigint, TRUE);

#else

  // ignore SIGPIPE
  signal(SIGPIPE, SIG_IGN);

  // reset color on SIGINT and SIGTERM
  signal(SIGINT, sigint);
  signal(SIGTERM, sigint);

#endif

  int status = core_main(argc, argv);

#ifdef OS_WIN_OR_MINGW

  delete[] argv;

#endif

  return status;
}

// the callback table of the Rust options parser (ffi/options.rs), the order of the list numbers mirrors Rust's List
struct OptionsEnv
{
  void *ctx;
  void (*usage)(void *, const char *, const char *, const char *);
  void (*help)(void *, const char *);
  void (*version)(void *);
  const char *(*strarg)(void *, const char *);
  void (*pattern_push)(void *, int, const char *);
  int (*patterns_empty)(void *);
  void (*list_push)(void *, int, const char *);
  void (*list_clear)(void *, int);
  int (*list_empty)(void *, int);
  void (*filter_append)(void *, const char *);
  void (*filter_clear)(void *);
  int (*arg_pattern_is_null)(void *);
  void (*set_arg_pattern)(void *, const char *);
  void (*push_arg_file)(void *, const char *);
  size_t (*screen_width)(void *);
  const char *auto_;
  const char *never;
  size_t max_mmap_size;
  size_t default_tabs;
  const char *default_tag;
  const char *default_ignore_file;
  int os_win;
};

extern "C" void ugrep_core_options(const OptionsEnv *env, int argc, const char **argv);

namespace
{

typedef std::list<std::pair<CNF::PATTERN,const char*>> PatternArgs;

std::vector<std::string> *options_list(int list)
{
  switch (list)
  {
    case 0: return &flag_file;
    case 1: return &flag_file_type;
    case 2: return &flag_file_extension;
    case 3: return &flag_file_magic;
    case 4: return &flag_filter_magic_label;
    case 5: return &flag_from;
    case 6: return &flag_glob;
    case 7: return &flag_iglob;
    case 8: return &flag_include;
    case 9: return &flag_include_dir;
    case 10: return &flag_include_from;
    case 11: return &flag_include_fs;
    case 12: return &flag_exclude;
    case 13: return &flag_exclude_dir;
    case 14: return &flag_exclude_from;
    case 15: return &flag_exclude_fs;
    default: return NULL;
  }
}

void options_usage(void *, const char *message, const char *arg, const char *valid)
{
  usage(message, arg, valid);
}

void options_help(void *, const char *what)
{
  help(what);
}

void options_version(void *)
{
  version();
}

const char *options_strarg(void *, const char *string)
{
  arg_strings.emplace_back(string);
  return arg_strings.back().c_str();
}

void options_pattern_push(void *ctx, int mask, const char *arg)
{
  static_cast<PatternArgs*>(ctx)->emplace_back(static_cast<CNF::PATTERN>(mask), arg);
}

int options_patterns_empty(void *ctx)
{
  return static_cast<PatternArgs*>(ctx)->empty();
}

void options_list_push(void *, int list, const char *value)
{
  if (list == 16)
    flag_ignore_files.insert(value);
  else
    options_list(list)->emplace_back(value);
}

void options_list_clear(void *, int list)
{
  if (list == 16)
    flag_ignore_files.clear();
  else
    options_list(list)->clear();
}

int options_list_empty(void *, int list)
{
  if (list == 16)
    return flag_ignore_files.empty();
  return options_list(list)->empty();
}

void options_filter_append(void *, const char *value)
{
  flag_filter.append(flag_filter.empty() ? "" : ",").append(value);
}

void options_filter_clear(void *)
{
  flag_filter.clear();
}

int options_arg_pattern_is_null(void *)
{
  return Static::arg_pattern == NULL;
}

void options_set_arg_pattern(void *, const char *arg)
{
  Static::arg_pattern = arg;
}

void options_push_arg_file(void *, const char *arg)
{
  Static::arg_files.emplace_back(arg);
}

size_t options_screen_width(void *)
{
  return tui_screen_getsize();
}

} // namespace

// fill the callback table of the Rust options parser, pattern_args is the context
static void config_options_env(OptionsEnv& env, PatternArgs& pattern_args)
{
  env.ctx = &pattern_args;
  env.usage = options_usage;
  env.help = options_help;
  env.version = options_version;
  env.strarg = options_strarg;
  env.pattern_push = options_pattern_push;
  env.patterns_empty = options_patterns_empty;
  env.list_push = options_list_push;
  env.list_clear = options_list_clear;
  env.list_empty = options_list_empty;
  env.filter_append = options_filter_append;
  env.filter_clear = options_filter_clear;
  env.arg_pattern_is_null = options_arg_pattern_is_null;
  env.set_arg_pattern = options_set_arg_pattern;
  env.push_arg_file = options_push_arg_file;
  env.screen_width = options_screen_width;
  env.auto_ = Static::AUTO;
  env.never = Static::NEVER;
  env.max_mmap_size = MAX_MMAP_SIZE;
  env.default_tabs = DEFAULT_TABS;
  env.default_tag = DEFAULT_TAG;
  env.default_ignore_file = DEFAULT_IGNORE_FILE;
#ifdef OS_WIN
  env.os_win = 1;
#else
  env.os_win = 0;
#endif
}

// parse the command-line options, implemented in Rust on the settings struct
void options(std::list<std::pair<CNF::PATTERN,const char*>>& pattern_args, int argc, const char **argv)
{
  OptionsEnv env;
  config_options_env(env, pattern_args);
  ugrep_core_options(&env, argc, argv);
}

// the callback table of the Rust load_config/save_config (ffi/config.rs)
struct ConfigEnv
{
  const OptionsEnv *opts;
  void *(*open)(const char *, int);
  int (*next_line)(void *);
  const char *(*line_text)(void *);
  const char *(*prefix_line)(void *);
  int (*file_error)(void *);
  void (*close)(void *);
  int (*files_contains)(const char *);
  void (*files_insert)(const char *);
  size_t (*files_len)();
  const char *(*files_get)(size_t);
  const char *(*home_dir)();
  void (*warnings_reset)();
  size_t (*warnings)();
  void (*write)(int, const char *, size_t);
  void (*error)(const char *, const char *);
  void (*warning)(const char *, const char *);
  void (*exit_error)();
  void (*regexp_clear)();
  const char *(*filter)();
  size_t (*list_len)(int);
  const char *(*list_get)(int, size_t);
  int (*rename_old)(const char *, const char *);
  void *(*save_open)(const char *);
  void (*save_write)(void *, const char *, size_t);
  int (*save_error)(void *);
  void (*save_close)(void *);
  size_t default_query_delay;
  int path_sep;
  int xdg;
};

extern "C" void ugrep_core_load_config(const ConfigEnv *env, int recurse);
extern "C" void ugrep_core_save_config(const ConfigEnv *env);

namespace
{

// an open config file with its buffered input and the current line
struct ConfigFile
{
  ConfigFile(FILE *f) : file(f), input(f) { }
  FILE *file;
  reflex::BufferedInput input;
  std::string line;
};

void *config_open(const char *filename, int check_owner)
{
  FILE *file = NULL;

  if (fopen_smart(&file, filename, "r") != 0)
    return NULL;

  if (file != NULL && file != stdin && check_owner)
  {
    // check if we own this config file located in the working directory
#ifdef OS_WIN_OR_MINGW
    HANDLE hFile = reinterpret_cast<HANDLE>(_get_osfhandle(_fileno(file)));
    PSID pSidOwner = NULL;
    PSECURITY_DESCRIPTOR pSD = NULL;
    if (GetSecurityInfo(hFile, SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION, &pSidOwner, NULL, NULL, NULL, &pSD) == ERROR_SUCCESS)
    {
      HANDLE hToken = NULL;
      if (OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &hToken))
      {
        DWORD dwBufferSize = 0;
        GetTokenInformation(hToken, TokenUser, NULL, 0, &dwBufferSize);
        PTOKEN_USER pTokenUser = reinterpret_cast<PTOKEN_USER>(HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, dwBufferSize));
        if (pTokenUser != NULL)
        {
          if (!GetTokenInformation(hToken, TokenUser, pTokenUser, dwBufferSize, &dwBufferSize) ||
              !EqualSid(pSidOwner, pTokenUser->User.Sid))
          {
            fclose(file);
            file = NULL;
          }
          HeapFree(GetProcessHeap(), 0, pTokenUser);
        }
        CloseHandle(hToken);
      }
      if (pSD)
        LocalFree(pSD);
    }
#else
    struct stat buf;
    if (fstat(fileno(file), &buf) == 0 && buf.st_uid != getuid())
    {
      fclose(file);
      file = NULL;
    }
#endif
  }

  if (file == NULL)
    return NULL;

  return new ConfigFile(file);
}

int config_next_line(void *handle)
{
  ConfigFile *cf = static_cast<ConfigFile*>(handle);

  // read the next line
  if (getline(cf->input, cf->line))
    return 0;

  trim(cf->line);

  return 1;
}

const char *config_line_text(void *handle)
{
  return static_cast<ConfigFile*>(handle)->line.c_str();
}

// construct an option argument to parse as argv[]
const char *config_prefix_line(void *handle)
{
  ConfigFile *cf = static_cast<ConfigFile*>(handle);

  cf->line.insert(0, "--");

  return cf->line.c_str();
}

int config_file_error(void *handle)
{
  return ferror(static_cast<ConfigFile*>(handle)->file);
}

void config_close(void *handle)
{
  ConfigFile *cf = static_cast<ConfigFile*>(handle);

  if (cf->file != stdin)
    fclose(cf->file);

  delete cf;
}

int config_files_contains(const char *path)
{
  return flag_config_files.find(path) != flag_config_files.end();
}

void config_files_insert(const char *path)
{
  flag_config_files.insert(path);
}

size_t config_files_len()
{
  return flag_config_files.size();
}

const char *config_files_get(size_t index)
{
  auto i = flag_config_files.begin();
  std::advance(i, index);
  return i->c_str();
}

const char *config_home_dir()
{
  return Static::home_dir;
}

void config_warnings_reset()
{
  Static::warnings = 0;
}

size_t config_warnings()
{
  return Static::warnings;
}

void config_write(int sink, const char *data, size_t len)
{
  if (sink == 0)
    std::cerr.write(data, len);
  else
    fwrite(data, 1, len, Static::errout);
}

void config_error(const char *message, const char *arg)
{
  error(message, arg);
}

void config_warning(const char *message, const char *arg)
{
  warning(message, arg);
}

void config_exit_error()
{
  exit(EXIT_ERROR);
}

void config_regexp_clear()
{
  flag_regexp.clear();
}

const char *config_filter()
{
  return flag_filter.c_str();
}

const std::string& config_list_item(const std::vector<std::string>& v, size_t index)
{
  return v[index];
}

size_t config_list_len(int list)
{
  if (list == 16)
    return flag_ignore_files.size();
  return options_list(list)->size();
}

const char *config_list_get(int list, size_t index)
{
  if (list == 16)
  {
    auto i = flag_ignore_files.begin();
    std::advance(i, index);
    return i->c_str();
  }
  return config_list_item(*options_list(list), index).c_str();
}

int config_rename_old(const char *path, const char *config_old)
{
  if (std::rename(path, config_old) != 0)
    return 0;

  errno = EEXIST;
  warning("saved old configuration file to", config_old);

  return 1;
}

void *config_save_open(const char *filename)
{
  FILE *file = NULL;

  if (fopen_smart(&file, filename, "w") != 0)
    return NULL;

  return file;
}

void config_save_write(void *file, const char *data, size_t len)
{
  fwrite(data, 1, len, static_cast<FILE*>(file));
}

int config_save_error(void *file)
{
  return ferror(static_cast<FILE*>(file));
}

void config_save_close(void *file)
{
  if (file != stdout)
    fclose(static_cast<FILE*>(file));
}

void config_env_init(ConfigEnv& cenv, const OptionsEnv& env)
{
  cenv.opts = &env;
  cenv.open = config_open;
  cenv.next_line = config_next_line;
  cenv.line_text = config_line_text;
  cenv.prefix_line = config_prefix_line;
  cenv.file_error = config_file_error;
  cenv.close = config_close;
  cenv.files_contains = config_files_contains;
  cenv.files_insert = config_files_insert;
  cenv.files_len = config_files_len;
  cenv.files_get = config_files_get;
  cenv.home_dir = config_home_dir;
  cenv.warnings_reset = config_warnings_reset;
  cenv.warnings = config_warnings;
  cenv.write = config_write;
  cenv.error = config_error;
  cenv.warning = config_warning;
  cenv.exit_error = config_exit_error;
  cenv.regexp_clear = config_regexp_clear;
  cenv.filter = config_filter;
  cenv.list_len = config_list_len;
  cenv.list_get = config_list_get;
  cenv.rename_old = config_rename_old;
  cenv.save_open = config_save_open;
  cenv.save_write = config_save_write;
  cenv.save_error = config_save_error;
  cenv.save_close = config_save_close;
  cenv.default_query_delay = DEFAULT_QUERY_DELAY;
  cenv.path_sep = PATHSEPCHR;
#if defined(WITH_XDG_CONFIG_HOME) && !defined(OS_WIN)
  cenv.xdg = 1;
#else
  cenv.xdg = 0;
#endif
}

} // namespace

// load config file specified or the default .ugrep, located in the working directory or home directory
static void load_config(std::list<std::pair<CNF::PATTERN,const char*>>& pattern_args, bool recurse = false)
{
  OptionsEnv env;
  config_options_env(env, pattern_args);
  ConfigEnv cenv;
  config_env_init(cenv, env);
  ugrep_core_load_config(&cenv, recurse);
}

// save a configuration file after loading it first when present
static void save_config()
{
  PatternArgs pattern_args;
  OptionsEnv env;
  config_options_env(env, pattern_args);
  ConfigEnv cenv;
  config_env_init(cenv, env);
  ugrep_core_save_config(&cenv);
}

// the callback table of the Rust init (ffi/init.rs), the field order mirrors Rust's InitEnv, the list numbers mirror Rust's List
struct InitEnv
{
  void *ctx;
  void (*set_home_dir)(void *, const char *);
  void (*cerr)(void *, const char *, size_t);
  void (*exit)(void *, int);
  void (*usage)(void *, const char *);
  void (*warning)(void *, const char *, const char *);
  void (*error)(void *, const char *, const char *);
  void (*abort)(void *, const char *);
  void (*abort_what)(void *, const char *, const char *);
  void (*load_config)(void *);
  void (*save_config)(void *);
  void (*warnings_reset)(void *);
  size_t (*warnings)(void *);
  void (*options)(void *);
  void (*terminal)(void *);
  size_t (*patterns_len)(void *);
  void (*pattern_get)(void *, size_t, int *, const char **);
  void (*bcnf_new_pattern)(void *, int, const char *);
  void (*bcnf_new_pattern_regexp)(void *);
  void (*bcnf_new_term)(void *);
  void (*regexp_append)(void *, const char *);
  size_t (*list_len)(void *, int);
  const char *(*list_get)(void *, int, size_t);
  void (*list_push)(void *, int, const char *);
  void (*list_truncate)(void *, int, size_t, size_t);
  void (*ignore_file_erase)(void *, const char *);
  const char *(*arg_pattern)(void *);
  void (*set_arg_pattern)(void *, const char *);
  size_t (*arg_files_len)(void *);
  const char *(*arg_file)(void *, size_t);
  void (*arg_files_erase)(void *, size_t);
  void (*arg_files_insert_front)(void *, const char *);
  void (*expand_arg_files)(void *);
  int (*check_arg_file)(void *, const char *, int);
  void (*trim_pathname_arg)(void *, const char *);
  int (*file_open)(void *, const char *);
  int (*file_getline)(void *, size_t *);
  void (*arg_files_push_line)(void *);
  void (*file_close)(void *);
  void (*import_globs)(void *, int, int);
  int (*fs_probe)(void *, const char *, uint64_t *);
  void (*fs_insert)(void *, int, uint64_t);
  int (*type_entry)(void *, size_t, const char **, const char **, const char **, const char **);
  int (*encoding_entry)(void *, size_t, const char **, uint16_t *);
  const char *(*magic_pattern_assign)(void *, const char *);
  const char *(*filter_magic_assign)(void *, const char *);
  int (*filter_empty)(void *);
  int (*isatty_stdin)(void *);
  void (*set_binary_stdout)(void *);
  const char *home_var;
  const char *auto_;
  int pathsep;
  int os_win;
  int os_win_or_mingw;
  int have_libz;
  int have_regex_perl;
  int have_stat_fs;
  int cygwin;
  size_t max_hex_columns;
  uint16_t encoding_null_data;
};

extern "C" void ugrep_core_init(const InitEnv *env, int argc, const char **argv);

namespace
{

// the state behind the init callbacks
struct InitCtx
{
  int argc;
  const char **argv;
  PatternArgs pattern_args; // collected -e, -N, --and, --andnot, --not and those of the config files
  FILE *file;               // the file opened by init_file_open()
  std::unique_ptr<reflex::BufferedInput> input;
  std::string line;
  std::string what;         // the text of a regex_error that is handed to Rust
};

InitCtx *init_ctx(void *ctx)
{
  return static_cast<InitCtx*>(ctx);
}

void init_set_home_dir(void *, const char *home)
{
  Static::home_dir = home;
}

void init_cerr(void *, const char *text, size_t len)
{
  std::cerr.write(text, len);
}

void init_exit(void *, int code)
{
  exit(code);
}

void init_usage(void *, const char *message)
{
  usage(message);
}

void init_warning(void *, const char *message, const char *arg)
{
  warning(message, arg);
}

void init_error(void *, const char *message, const char *arg)
{
  error(message, arg);
}

void init_abort(void *, const char *message)
{
  abort(message);
}

void init_abort_what(void *, const char *message, const char *what)
{
  abort(message, std::string(what));
}

void init_load_config(void *ctx)
{
  load_config(init_ctx(ctx)->pattern_args);
}

void init_save_config(void *)
{
  save_config();
}

void init_warnings_reset(void *)
{
  Static::warnings = 0;
}

size_t init_warnings(void *)
{
  return Static::warnings;
}

void init_options(void *ctx)
{
  options(init_ctx(ctx)->pattern_args, init_ctx(ctx)->argc, init_ctx(ctx)->argv);
}

void init_terminal(void *)
{
  terminal();
}

size_t init_patterns_len(void *ctx)
{
  return init_ctx(ctx)->pattern_args.size();
}

void init_pattern_get(void *ctx, size_t i, int *mask, const char **arg)
{
  auto it = init_ctx(ctx)->pattern_args.begin();
  std::advance(it, i);
  *mask = static_cast<int>(it->first);
  *arg = it->second;
}

void init_bcnf_new_pattern(void *, int mask, const char *arg)
{
  Static::bcnf.new_pattern(static_cast<CNF::PATTERN>(mask), arg);
}

void init_bcnf_new_pattern_regexp(void *)
{
  Static::bcnf.new_pattern(CNF::PATTERN::ALT, flag_regexp.c_str());
}

void init_bcnf_new_term(void *)
{
  Static::bcnf.new_term();
}

void init_regexp_append(void *, const char *text)
{
  flag_regexp.append(text);
}

size_t init_list_len(void *, int list)
{
  if (list == 16)
    return flag_ignore_files.size();
  return options_list(list)->size();
}

const char *init_list_get(void *, int list, size_t i)
{
  if (list == 16)
  {
    auto it = flag_ignore_files.begin();
    std::advance(it, i);
    return it->c_str();
  }
  return (*options_list(list))[i].c_str();
}

void init_list_push(void *, int list, const char *value)
{
  if (list == 16)
    flag_ignore_files.insert(value);
  else
    options_list(list)->emplace_back(value);
}

void init_list_truncate(void *, int list, size_t i, size_t len)
{
  (*options_list(list))[i].resize(len);
}

void init_ignore_file_erase(void *, const char *name)
{
  flag_ignore_files.erase(name);
}

const char *init_arg_pattern(void *)
{
  return Static::arg_pattern;
}

void init_set_arg_pattern(void *, const char *arg)
{
  Static::arg_pattern = arg;
}

size_t init_arg_files_len(void *)
{
  return Static::arg_files.size();
}

const char *init_arg_file(void *, size_t i)
{
  return Static::arg_files[i];
}

void init_arg_files_erase(void *, size_t i)
{
  Static::arg_files.erase(Static::arg_files.begin() + i);
}

void init_arg_files_insert_front(void *, const char *arg)
{
  Static::arg_files.insert(Static::arg_files.begin(), arg);
}

void init_expand_arg_files(void *)
{
#ifdef OS_WIN

  // Windows shell does not expand wildcards in arguments, do that now (basename part only)
  if (!Static::arg_files.empty())
  {
    std::vector<const char*> expanded_arg_files;

    for (const auto& arg_file : Static::arg_files)
    {
      std::wstring filename(utf8_decode(arg_file));
      bool has_wildcard_char = false;

      size_t basename_pos;
      for (basename_pos = filename.size(); basename_pos > 0; --basename_pos)
      {
        wchar_t ch = filename[basename_pos - 1];

        if (ch == L'*' || ch == L'?')
          has_wildcard_char = true;
        else if (ch == L'\\' || ch == L'/' || ch == L':')
          break;
      }

      if (!has_wildcard_char)
      {
        // no wildcard chars, use argument as-is
        expanded_arg_files.push_back(arg_file);
        continue;
      }

      WIN32_FIND_DATAW find_data;

      HANDLE hFile = FindFirstFileExW(filename.c_str(), FindExInfoBasic, &find_data, FindExSearchNameMatch, NULL, 0);
      if (hFile == INVALID_HANDLE_VALUE)
      {
        // glob pattern didn't match any files, use argument as-is which will trigger a warning later
        expanded_arg_files.push_back(arg_file);
        continue;
      }

      bool glob_starts_with_dot = filename[basename_pos] == L'.';

      do
      {
        if (find_data.cFileName[0] == L'.')
        {
          // don't expand directories "." or ".."
          if (find_data.cFileName[1] == 0 ||
              (find_data.cFileName[1] == L'.' && find_data.cFileName[2] == 0))
            continue;

          // don't expand hidden files unless --hidden or the pattern started with '.'
          if (!flag_hidden && !glob_starts_with_dot)
            continue;
        }

        // replace glob pattern with matching filename converted to UTF-8, then add to expanded filename list
        filename.erase(basename_pos);
        filename += find_data.cFileName;
        arg_strings.emplace_back(utf8_encode(filename));
        expanded_arg_files.push_back(arg_strings.back().c_str());
      } while (FindNextFileW(hFile, &find_data));

      FindClose(hFile);
    }

    // replace the original filenames list with the expanded list
    Static::arg_files.swap(expanded_arg_files);
  }

#endif
}

// check a FILE argument: 0 does not exist (errno is set), 1 skip the symbolic link, 2 not readable (errno is set), 3 directory, 4 other
int init_check_arg_file(void *, const char *arg_file, int no_dereference)
{
#ifdef OS_WIN_OR_MINGW

  (void)no_dereference;

  DWORD attr = GetFileAttributesW(utf8_decode(arg_file).c_str());

  if (attr == INVALID_FILE_ATTRIBUTES)
  {
    // argument FILE does not exist
    errno = ENOENT;
    return 0;
  }

  return (attr & FILE_ATTRIBUTE_DIRECTORY) != 0 ? 3 : 4;

#else

  struct stat buf;
  int ret;

  if (no_dereference)
    ret = lstat(arg_file, &buf);
  else
    ret = stat(arg_file, &buf);

  if (ret != 0)
    return 0;

  if (no_dereference && S_ISLNK(buf.st_mode))
    return 1;

#ifdef WITH_WARN_UNREADABLE_FILE_ARG
  if ((buf.st_mode & S_IRUSR) == 0)
  {
    // the specified argument FILE or directory is not readable, even when executing as su
    errno = EACCES;
    return 2;
  }
#endif

  return S_ISDIR(buf.st_mode) ? 3 : 4;

#endif
}

void init_trim_pathname_arg(void *, const char *arg)
{
  trim_pathname_arg(arg);
}

// open the file with fopen_smart(), leaves errno from a failure for the error message
int init_file_open(void *ctx, const char *name)
{
  FILE *file = NULL;

  if (fopen_smart(&file, name, "r") != 0)
    return 0;

  init_ctx(ctx)->file = file;
  return 1;
}

int init_file_getline(void *ctx, size_t *len)
{
  // the input is created on demand: import_globs() reads the same FILE with its own BufferedInput
  if (!init_ctx(ctx)->input)
    init_ctx(ctx)->input.reset(new reflex::BufferedInput(init_ctx(ctx)->file));
  bool eof = getline(*init_ctx(ctx)->input, init_ctx(ctx)->line);
  *len = init_ctx(ctx)->line.size();
  return eof;
}

void init_arg_files_push_line(void *ctx)
{
  // push file pathnames imported from the file to the back of the arg files vector, saving std::string first in a list
  arg_strings.emplace_back();
  arg_strings.back().swap(init_ctx(ctx)->line);
  Static::arg_files.push_back(arg_strings.back().c_str());
}

void init_file_close(void *ctx)
{
  init_ctx(ctx)->input.reset();

  if (init_ctx(ctx)->file != stdin)
    fclose(init_ctx(ctx)->file);

  init_ctx(ctx)->file = NULL;
}

void init_import_globs(void *ctx, int include, int gitignore)
{
  // push globs imported from the file to the back of the vectors
  if (include)
    import_globs(init_ctx(ctx)->file, flag_include, flag_include_dir);
  else
    import_globs(init_ctx(ctx)->file, flag_exclude, flag_exclude_dir, gitignore != 0);
}

int init_fs_probe(void *, const char *path, uint64_t *id)
{
#if defined(HAVE_STATVFS) || defined(HAVE_STATFS)
  stat_fs_t buf;

  if (stat_fs(path, &buf) == 0)
  {
    *id = fsid_to_uint64(buf.f_fsid);
    return 1;
  }
#else
  (void)path;
  (void)id;
#endif
  return 0;
}

void init_fs_insert(void *, int include, uint64_t id)
{
#if defined(HAVE_STATVFS) || defined(HAVE_STATFS)
  if (include)
    include_fs_ids.insert(id);
  else
    exclude_fs_ids.insert(id);
#else
  (void)include;
  (void)id;
#endif
}

int init_type_entry(void *, size_t i, const char **type, const char **extensions, const char **filenames, const char **magic)
{
  if (i >= sizeof(type_table) / sizeof(type_table[0]) || type_table[i].type == NULL)
    return 0;

  *type = type_table[i].type;
  *extensions = type_table[i].extensions;
  *filenames = type_table[i].filenames;
  *magic = type_table[i].magic;
  return 1;
}

int init_encoding_entry(void *, size_t i, const char **format, uint16_t *encoding)
{
  if (i >= sizeof(encoding_table) / sizeof(encoding_table[0]) || encoding_table[i].format == NULL)
    return 0;

  *format = encoding_table[i].format;
  *encoding = static_cast<uint16_t>(encoding_table[i].encoding);
  return 1;
}

// -M: create a magic matcher for the MAGIC regex to match file with magic.scan(), returns NULL or the error text
const char *init_magic_pattern_assign(void *ctx, const char *regex)
{
  try
  {
    // construct magic_pattern DFA for -M !MAGIC and -M MAGIC
    if (regex != NULL)
      Static::magic_pattern.assign(regex, "r");
    Static::magic_matcher.pattern(Static::magic_pattern);
  }

  catch (reflex::regex_error& error)
  {
    init_ctx(ctx)->what = error.what();
    return init_ctx(ctx)->what.c_str();
  }

  return NULL;
}

// --filter-magic-label: construct Static::filter_magic_pattern DFA, returns NULL or the error text
const char *init_filter_magic_assign(void *ctx, const char *regex)
{
  try
  {
    Static::filter_magic_pattern.assign(regex, "r");
  }

  catch (reflex::regex_error& error)
  {
    init_ctx(ctx)->what = error.what();
    return init_ctx(ctx)->what.c_str();
  }

  return NULL;
}

int init_filter_empty(void *)
{
  return flag_filter.empty();
}

int init_isatty_stdin(void *)
{
  return isatty(STDIN_FILENO);
}

void init_set_binary_stdout(void *)
{
#ifdef OS_WIN
  // save_config() and help() assume text mode, so switch to
  // binary after we're no longer going to call them.
  (void)_setmode(fileno(stdout), _O_BINARY);
#endif
}

} // namespace

// parse the command-line options and initialize, implemented in Rust on the settings struct
void init(int argc, const char **argv)
{
  InitCtx ctx;
  ctx.argc = argc;
  ctx.argv = argv;
  ctx.file = NULL;

  InitEnv env;
  env.ctx = &ctx;
  env.set_home_dir = init_set_home_dir;
  env.cerr = init_cerr;
  env.exit = init_exit;
  env.usage = init_usage;
  env.warning = init_warning;
  env.error = init_error;
  env.abort = init_abort;
  env.abort_what = init_abort_what;
  env.load_config = init_load_config;
  env.save_config = init_save_config;
  env.warnings_reset = init_warnings_reset;
  env.warnings = init_warnings;
  env.options = init_options;
  env.terminal = init_terminal;
  env.patterns_len = init_patterns_len;
  env.pattern_get = init_pattern_get;
  env.bcnf_new_pattern = init_bcnf_new_pattern;
  env.bcnf_new_pattern_regexp = init_bcnf_new_pattern_regexp;
  env.bcnf_new_term = init_bcnf_new_term;
  env.regexp_append = init_regexp_append;
  env.list_len = init_list_len;
  env.list_get = init_list_get;
  env.list_push = init_list_push;
  env.list_truncate = init_list_truncate;
  env.ignore_file_erase = init_ignore_file_erase;
  env.arg_pattern = init_arg_pattern;
  env.set_arg_pattern = init_set_arg_pattern;
  env.arg_files_len = init_arg_files_len;
  env.arg_file = init_arg_file;
  env.arg_files_erase = init_arg_files_erase;
  env.arg_files_insert_front = init_arg_files_insert_front;
  env.expand_arg_files = init_expand_arg_files;
  env.check_arg_file = init_check_arg_file;
  env.trim_pathname_arg = init_trim_pathname_arg;
  env.file_open = init_file_open;
  env.file_getline = init_file_getline;
  env.arg_files_push_line = init_arg_files_push_line;
  env.file_close = init_file_close;
  env.import_globs = init_import_globs;
  env.fs_probe = init_fs_probe;
  env.fs_insert = init_fs_insert;
  env.type_entry = init_type_entry;
  env.encoding_entry = init_encoding_entry;
  env.magic_pattern_assign = init_magic_pattern_assign;
  env.filter_magic_assign = init_filter_magic_assign;
  env.filter_empty = init_filter_empty;
  env.isatty_stdin = init_isatty_stdin;
  env.set_binary_stdout = init_set_binary_stdout;
#ifdef OS_WIN
  env.home_var = "USERPROFILE";
#else
  env.home_var = "HOME";
#endif
  env.auto_ = Static::AUTO;
  env.pathsep = PATHSEPCHR;
#ifdef OS_WIN
  env.os_win = 1;
#else
  env.os_win = 0;
#endif
#ifdef OS_WIN_OR_MINGW
  env.os_win_or_mingw = 1;
#else
  env.os_win_or_mingw = 0;
#endif
#ifdef HAVE_LIBZ
  env.have_libz = 1;
#else
  env.have_libz = 0;
#endif
#if defined(HAVE_PCRE2) || defined(HAVE_BOOST_REGEX)
  env.have_regex_perl = 1;
#else
  env.have_regex_perl = 0;
#endif
#if defined(HAVE_STATVFS) || defined(HAVE_STATFS)
  env.have_stat_fs = 1;
#else
  env.have_stat_fs = 0;
#endif
#if defined(__CYGWIN__)
  env.cygwin = 1;
#else
  env.cygwin = 0;
#endif
  env.max_hex_columns = MAX_HEX_COLUMNS;
  env.encoding_null_data = static_cast<uint16_t>(reflex::Input::file_encoding::null_data);

  ugrep_core_init(&env, argc, argv);
}

// search the specified files, directories, and/or standard input for pattern matches, may throw an exception
void ugrep()
{
  // reset stats
  Stats::reset();

  // --tree: reset directory tree output
  Output::Tree::reset();

  // clear internal combined all-include and all-exclude globs
  flag_all_exclude.clear();
  flag_all_include.clear();
  flag_all_exclude_dir.clear();
  flag_all_include_dir.clear();

  // --iglob: add case-insensitive globs to all-include-iglob/all-exclude-iglob if not --glob-ignore-case
  for (const auto& globs : flag_iglob)
  {
    size_t from = 0;

    while (true)
    {
      size_t to;

      for (to = from; to < globs.size() && globs[to] != ','; ++to)
      {
        if (globs[to] == '[')
          while (++to < globs.size() && globs[to] != ']')
            to += globs[to] == '\\';
        else if (globs[to] == '\\')
          ++to;
      }

      size_t size = to - from;

      if (size > 0)
      {
        bool negate = size > 1 && (globs[from] == '!' || globs[from] == '^');

        if (negate)
        {
          ++from;
          --size;
          if (globs[from + size - 1] == '/')
            flag_all_exclude_dir.emplace_back(globs, from, size - 1);
          else
            flag_all_exclude.emplace_back(globs, from, size);
        }
        else
        {
          if (globs[from + size - 1] == '/')
            flag_all_include_dir.emplace_back(globs, from, size - 1);
          else
            flag_all_include.emplace_back(globs, from, size);
        }
      }

      if (to >= globs.size())
        break;

      from = to + 1;
    }
  }

  // all --iglob globs are case-sensitive
  flag_include_iglob_size = flag_all_include.size();
  flag_exclude_iglob_size = flag_all_exclude.size();
  flag_include_iglob_dir_size = flag_all_include_dir.size();
  flag_exclude_iglob_dir_size = flag_all_exclude_dir.size();

  // -g, --glob: add globs to all-include/all-exclude
  for (const auto& globs : flag_glob)
  {
    size_t from = 0;

    while (true)
    {
      size_t to;

      for (to = from; to < globs.size() && globs[to] != ','; ++to)
      {
        if (globs[to] == '[')
          while (++to < globs.size() && globs[to] != ']')
            to += globs[to] == '\\';
        else if (globs[to] == '\\')
          ++to;
      }

      size_t size = to - from;

      if (size > 0)
      {
        bool negate = size > 1 && (globs[from] == '!' || globs[from] == '^');

        if (negate)
        {
          ++from;
          --size;
          if (globs[from + size - 1] == '/')
            flag_all_exclude_dir.emplace_back(globs, from, size - 1);
          else
            flag_all_exclude.emplace_back(globs, from, size);
        }
        else
        {
          if (globs[from + size - 1] == '/')
            flag_all_include_dir.emplace_back(globs, from, size - 1);
          else
            flag_all_include.emplace_back(globs, from, size);
        }
      }

      if (to >= globs.size())
        break;

      from = to + 1;
    }
  }

  // populate the internal combined all-include and all-exclude globs by appending --include and --exclude
  flag_all_exclude.insert(flag_all_exclude.end(), flag_exclude.begin(), flag_exclude.end());
  flag_all_include.insert(flag_all_include.end(), flag_include.begin(), flag_include.end());
  // same for dirs
  flag_all_exclude_dir.insert(flag_all_exclude_dir.end(), flag_exclude_dir.begin(), flag_exclude_dir.end());
  flag_all_include_dir.insert(flag_all_include_dir.end(), flag_include_dir.begin(), flag_include_dir.end());

  // all excluded files: normalize by moving directory globs (globs ending in a path separator /) to --exclude-dir and remove consecutive duplicates
  std::string *last = NULL;
  auto it = flag_all_exclude.begin();
  while (it != flag_all_exclude.end())
  {
    // remove empty and duplicate --exclude globs
    if (it->empty() || (last != NULL && it->compare(*last) == 0))
    {
      it = flag_all_exclude.erase(it);
      if (static_cast<size_t>(std::distance(flag_all_exclude.begin(), it)) < flag_exclude_iglob_size)
        --flag_exclude_iglob_size;
    }
    else if (it->back() == '/')
    {
      // move directory glob (ending in a /) from --exclude to --exclude-dir (these are all case sensitive)
      flag_all_exclude_dir.emplace_back(*it);
      it = flag_all_exclude.erase(it);
    }
    else
    {
      last = &*it++;
    }
  }

  // if all --include files have paths, then add them to --include-dir to speed up searching
  std::vector<std::string> all_include_dir_from_iglob;
  std::vector<std::string> all_include_dir_from_glob;

  // all included files: normalize by moving or adding directory globs (globs ending in a path separator /) to --include-dir and remove consecutive duplicates
  bool add_wdir = false;
  last = NULL;
  it = flag_all_include.begin();
  while (it != flag_all_include.end())
  {
    // remove empty and duplicate --include globs
    if (it->empty() || (last != NULL && it->compare(*last) == 0))
    {
      it = flag_all_include.erase(it);
      if (static_cast<size_t>(std::distance(flag_all_include.begin(), it)) < flag_include_iglob_size)
        --flag_include_iglob_size;
    }
    else
    {
      if (it->back() == '/')
      {
        // move directory glob (ending in a /) from --include to --include-dir (these are all case sensitive)
        flag_all_include_dir.emplace_back(*it);
        it = flag_all_include.erase(it);
      }
      else
      {
        size_t last_sep = it->find_last_of('/');

        // if include file has a path, then add the path part to the include-dirs, respect case (in)sensitivity
        if (last_sep != std::string::npos)
        {
          size_t first_sep = it->find_first_of('/');

          if (first_sep == last_sep && (first_sep == 0 || (first_sep == 1 && it->front() == '.')))
          {
            add_wdir = true;
          }
          else if (static_cast<size_t>(std::distance(flag_all_include.begin(), it)) < flag_include_iglob_size)
          {
            all_include_dir_from_iglob.emplace_back(*it, 0, last_sep);
            if (first_sep == last_sep)
              all_include_dir_from_iglob.back().insert(0, "./");
          }
          else
          {
            all_include_dir_from_glob.emplace_back(*it, 0, last_sep);
            if (first_sep == last_sep)
              all_include_dir_from_glob.back().insert(0, "./");
          }
        }

        // if an include file glob starts with a dot, then enable --hidden
        if (last_sep != std::string::npos ? it->at(last_sep + 1) == '.' : it->front() == '.')
          flag_hidden = true;

        last = &*it++;
      }
    }
  }

  // update include-dir to search case insensitive directories collected from --iglob
  flag_include_iglob_dir_size += all_include_dir_from_iglob.size();
  flag_all_include_dir.insert(flag_all_include_dir.begin(), all_include_dir_from_iglob.begin(), all_include_dir_from_iglob.end());

  // search the working dir?
  if (add_wdir)
    flag_all_include_dir.emplace_back("./");

  // update include-dir to search case sensitive directories collected from --include, --glob
  flag_all_include_dir.insert(flag_all_include_dir.end(), all_include_dir_from_glob.begin(), all_include_dir_from_glob.end());

  // all excluded-dir: normalize by removing trailing path separators and removing consecutive duplicates
  last = NULL;
  it = flag_all_exclude_dir.begin();
  while (it != flag_all_exclude_dir.end())
  {
    while ((it->size() > 2 || (it->size() == 2 && it->front() != '.')) && it->back() == '/')
      it->pop_back();
    if (last != NULL && it->compare(*last) == 0)
      it = flag_all_exclude_dir.erase(it);
    else
      last = &*it++;
  }

  // all included-dir: normalize by removing trailing path separators and removing consecutive duplicates
  last = NULL;
  it = flag_all_include_dir.begin();
  while (it != flag_all_include_dir.end())
  {
    while ((it->size() > 2 || (it->size() == 2 && it->front() != '.')) && it->back() == '/')
      it->pop_back();
    if (last != NULL && it->compare(*last) == 0)
      it = flag_all_include_dir.erase(it);
    else
      last = &*it++;
  }

  // if an include (sub)dir glob starts with a dot, then enable --hidden
  if (!flag_hidden)
  {
    for (const auto& dir : flag_all_include_dir)
    {
      if ((dir.front() == '.' && dir.compare(0, 2, "./") != 0) || dir.find("/.") != std::string::npos)
      {
        flag_hidden = true;
        break;
      }
    }
  }

  // if --include-dir is nonempty, and -d unspecified, then we should recurse into directories
  if (!flag_all_include_dir.empty() && flag_directories_action == Action::UNSP)
    flag_directories_action = Action::RECURSE;

#ifdef HAVE_LIBZ
#ifdef WITH_DECOMPRESSION_THREAD
  // -z with -M or -O/-t/-g/--include: add globs to also search archive contents for matching files
  if (flag_decompress && (!flag_file_magic.empty() || !flag_all_include.empty()))
  {
    flag_all_include.emplace_back("*.cpio");
    flag_all_include.emplace_back("*.pax");
    flag_all_include.emplace_back("*.tar");
    flag_all_include.emplace_back("*.zip");
    flag_all_include.emplace_back("*.zipx");
    flag_all_include.emplace_back("*.ZIP");

#ifndef WITH_NO_7ZIP
    flag_all_include.emplace_back("*.7z");
    flag_all_include.emplace_back("*.7Z");
#endif

    flag_all_include.emplace_back("*.cpio.gz");
    flag_all_include.emplace_back("*.pax.gz");
    flag_all_include.emplace_back("*.tar.gz");
    flag_all_include.emplace_back("*.taz");
    flag_all_include.emplace_back("*.tgz");
    flag_all_include.emplace_back("*.tpz");

    flag_all_include.emplace_back("*.cpio.Z");
    flag_all_include.emplace_back("*.pax.Z");
    flag_all_include.emplace_back("*.tar.Z");

    flag_all_include.emplace_back("*.cpio.zip");
    flag_all_include.emplace_back("*.pax.zip");
    flag_all_include.emplace_back("*.tar.zip");

#ifdef HAVE_LIBBZ2
    flag_all_include.emplace_back("*.cpio.bz");
    flag_all_include.emplace_back("*.pax.bz");
    flag_all_include.emplace_back("*.tar.bz");
    flag_all_include.emplace_back("*.cpio.bz2");
    flag_all_include.emplace_back("*.pax.bz2");
    flag_all_include.emplace_back("*.tar.bz2");
    flag_all_include.emplace_back("*.cpio.bzip2");
    flag_all_include.emplace_back("*.pax.bzip2");
    flag_all_include.emplace_back("*.tar.bzip2");
    flag_all_include.emplace_back("*.tb2");
    flag_all_include.emplace_back("*.tbz");
    flag_all_include.emplace_back("*.tbz2");
    flag_all_include.emplace_back("*.tz2");
#endif

#ifdef HAVE_LIBLZMA
    flag_all_include.emplace_back("*.cpio.lzma");
    flag_all_include.emplace_back("*.pax.lzma");
    flag_all_include.emplace_back("*.tar.lzma");
    flag_all_include.emplace_back("*.cpio.xz");
    flag_all_include.emplace_back("*.pax.xz");
    flag_all_include.emplace_back("*.tar.xz");
    flag_all_include.emplace_back("*.tlz");
    flag_all_include.emplace_back("*.txz");
#endif

#ifdef HAVE_LIBLZ4
    flag_all_include.emplace_back("*.cpio.lz4");
    flag_all_include.emplace_back("*.pax.lz4");
    flag_all_include.emplace_back("*.tar.lz4");
#endif

#ifdef HAVE_LIBZSTD
    flag_all_include.emplace_back("*.cpio.zst");
    flag_all_include.emplace_back("*.pax.zst");
    flag_all_include.emplace_back("*.tar.zst");
    flag_all_include.emplace_back("*.cpio.zstd");
    flag_all_include.emplace_back("*.pax.zstd");
    flag_all_include.emplace_back("*.tar.zstd");
    flag_all_include.emplace_back("*.tzst");
#endif

#ifdef HAVE_LIBBROTLI
    flag_all_include.emplace_back("*.cpio.br");
    flag_all_include.emplace_back("*.pax.br");
    flag_all_include.emplace_back("*.tar.br");
#endif

#ifdef HAVE_LIBBZIP3
    flag_all_include.emplace_back("*.cpio.bz3");
    flag_all_include.emplace_back("*.pax.bz3");
    flag_all_include.emplace_back("*.tar.bz3");
#endif
  }
#endif
#endif

  // keep track which globs are case-insensitive when --glob-ignore-case is specified, i.e. not --ignore-file globs
  if (flag_glob_ignore_case)
  {
    flag_exclude_iglob_size = flag_all_exclude.size();
    flag_exclude_iglob_dir_size = flag_all_exclude_dir.size();
    flag_include_iglob_size = flag_all_include.size();
    flag_include_iglob_dir_size = flag_all_include_dir.size();
  }

  // --sort: check sort KEY and set flags
  if (flag_sort != NULL)
  {
    flag_sort_rev = *flag_sort == 'r' || *flag_sort == '^';

    const char *sort_by = flag_sort + flag_sort_rev;

    if (strcmp(sort_by, "name") == 0)
      flag_sort_key = Sort::NAME;
    else if (strcmp(sort_by, "best") == 0)
      flag_sort_key = Sort::BEST;
    else if (strcmp(sort_by, "size") == 0)
      flag_sort_key = Sort::SIZE;
    else if (strcmp(sort_by, "used") == 0 || strcmp(sort_by, "atime") == 0)
      flag_sort_key = Sort::USED;
    else if (strcmp(sort_by, "changed") == 0 || strcmp(sort_by, "mtime") == 0)
      flag_sort_key = Sort::CHANGED;
    else if (strcmp(sort_by, "created") == 0 || strcmp(sort_by, "ctime") == 0)
      flag_sort_key = Sort::CREATED;
    else if (strcmp(sort_by, "list") == 0)
      flag_sort_key = Sort::LIST;
    else
      usage("invalid argument --sort=KEY, valid arguments are 'name', 'best', 'size', 'used' ('atime'), 'changed' ('mtime'), 'created' ('ctime'), 'list', 'rname', 'rbest', 'rsize', 'rused' ('ratime'), 'rchanged' ('rmtime'), 'rcreated' ('rctime') and 'rlist'");
  }

  // add PATTERN to the CNF
  if (Static::arg_pattern != NULL)
    Static::bcnf.new_pattern(CNF::PATTERN::ALT, Static::arg_pattern);

  // the regex compiled from PATTERN, -e PATTERN, -N PATTERN, and -f FILE
  std::string regex;

  if (Static::bcnf.defined())
  {
    // prune empty terms from the CNF that match anything
    Static::bcnf.prune();

    // split the patterns at newlines, standard grep behavior
    Static::bcnf.split();

    if (flag_file.empty())
    {
      // the CNF patterns to search, this matches more than necessary to support multiline matching and to highlight all matches in color
      regex.assign(Static::bcnf.adjoin());

      // an empty pattern specified matches every line, including empty lines
      if (regex.empty())
      {
        if (Static::bcnf.empty())
        {
          if (!flag_hex)
            flag_only_matching = false;
        }
        else if (!flag_quiet && !flag_files_with_matches && !flag_files_without_match)
        {
          if (flag_hex)
            regex.assign("(?-u)[^\\n]*\\n?"); // include trailing \n of a line when outputting hex per line
          else
            regex.assign("(?-u)[^\\n]*");
        }

        // an empty pattern matches every line, including empty lines
        flag_empty = true;
        flag_dotall = false;
      }

      // CNF is empty if all patterns are empty, i.e. match anything unless -f FILE specified
      if (Static::bcnf.empty())
      {
        flag_match = true;
        flag_dotall = false;
      }
    }
    else
    {
      // -f FILE is combined with -e, --and, --andnot, --not

      if (Static::bcnf.first_empty())
      {
        if (!flag_quiet && !flag_files_with_matches && !flag_files_without_match)
        {
          if (flag_hex)
            regex.assign("(?-u)[^\\n]*\\n?"); // include trailing \n of a line when outputting hex per line
          else
            regex.assign("(?-u)[^\\n]*");
        }

        // an empty pattern specified with -e '' matches every line, including empty lines
        flag_empty = true;
      }
      else
      {
        // for efficiency, take only the first CNF OR-list terms to search in combination with -f FILE patterns
        regex.assign(Static::bcnf.first());
      }
    }
  }

  // -f: get patterns from file
  if (!flag_file.empty())
  {
    // -F: make newline-separated lines in regex literal with \Q and \E
    bool fixed_strings = flag_mode == Mode::FIXED;
    const char *bar = flag_mode == Mode::BRE ? "\\|" : "|";

    // PATTERN or -e PATTERN: add an ending '|' (or BRE '\|') to the regex to concatenate sub-expressions
    if (!regex.empty())
    {
      // -F does not apply to patterns in -f FILE when PATTERN or -e PATTERN is specified
      fixed_strings = false;
      regex.append(bar);
    }

    // -f: read patterns from the specified file or files
    for (const auto& filename : flag_file)
    {
      FILE *file = NULL;

      if (fopen_smart(&file, filename.c_str(), "r") != 0)
      {
        // could not open, try GREP_PATH environment variable
        char *env_grep_path = NULL;
        dupenv_s(&env_grep_path, "GREP_PATH");

        if (env_grep_path != NULL)
        {
          if (fopen_smart(&file, std::string(env_grep_path).append(PATHSEPSTR).append(filename).c_str(), "r") != 0)
            file = NULL;

          free(env_grep_path);
        }
      }

#ifdef GREP_PATH
      if (file == NULL)
      {
        if (fopen_smart(&file, std::string(GREP_PATH).append(PATHSEPSTR).append(filename).c_str(), "r") != 0)
          file = NULL;
      }
#endif

      if (file == NULL)
        throw std::runtime_error(std::string("option -f: cannot read ").append(filename)); // to catch in query UI

      reflex::BufferedInput input(file);
      std::string line;

      while (true)
      {
        // read the next line
        if (getline(input, line))
          break;

        // add line to the regex if not empty
        if (!line.empty())
        {
          if (fixed_strings)
            CNF::quote(line);
          regex.append(line).append(bar);
        }
      }

      if (file != stdin)
        fclose(file);
    }

    if (regex.empty())
    {
      if (Static::bcnf.empty())
      {
        if (!flag_hex)
          flag_only_matching = false;
      }
      else if (!flag_quiet && !flag_files_with_matches && !flag_files_without_match)
      {
        if (flag_hex)
          regex.assign("(?-u)[^\\n]*\\n?"); // include trailing \n of a line when outputting hex per line
        else
          regex.assign("(?-u)[^\\n]*");
      }

      // an empty pattern matches every line, including empty lines
      flag_empty = true;
      flag_dotall = false;

      // CNF is empty if all patterns are empty
      if (Static::bcnf.empty())
      {
        flag_match = true;
        flag_word_regexp = false;
      }
    }
    else
    {
      // pop unused ending '|' (or BRE '\|') from the |-concatenated regexes in the regex string
      regex.pop_back();
      if (flag_mode == Mode::BRE)
        regex.pop_back();
    }

    // -x or -w: add line or word anchors
    CNF::anchor(regex);
  }

  // patterns ^, $ and ^$ are special cases to optimize for search
  if (flag_mode != Mode::FIXED && Static::bcnf.singleton_or_undefined())
  {
    if (regex == "^$")
    {
      flag_match = true;
      flag_line_regexp = true;
      regex.clear();
    }
    else if (regex == "^" || regex == "$")
    {
      flag_match = true;
      regex.clear();
    }
  }

  // -x or --match: enable -Y and disable --dotall and -w
  if (flag_line_regexp || flag_match)
  {
    flag_empty = true;
    flag_dotall = false;
    flag_word_regexp = false;
  }

  // --match: adjust color highlighting to show matches as selected lines without color
  if (flag_match)
  {
    copy_color(match_ms, color_sl);
    copy_color(match_mc, color_cx);
    copy_color(match_off, color_off);
  }
  else
  {
    // --tag: output tagged matches instead of colors
    if (flag_tag != NULL)
    {
      const char *s1 = strchr(flag_tag, ',');
      const char *s2 = s1 != NULL ? strchr(s1 + 1, ',') : NULL;

      copy_color(match_ms, flag_tag);

      if (s1 == NULL)
      {
        copy_color(match_mc, flag_tag);
        copy_color(match_off, flag_tag);
      }
      else
      {
        copy_color(match_off, s1 + 1);

        if (s2 == NULL)
          copy_color(match_mc, match_ms);
        else
          copy_color(match_mc, s2 + 1);
      }
    }
    else
    {
      copy_color(match_ms, color_ms);
      copy_color(match_mc, color_mc);
      copy_color(match_off, color_off);
    }
  }

  // patterns .* and .+ are common easy special cases to optimize for search
  if (flag_mode != Mode::FIXED && !flag_dotall && Static::bcnf.singleton_or_undefined())
  {
    if (regex == ".*")
    {
      flag_match = true;
      flag_empty = true;
      regex.clear();
    }
    else if (regex == ".+")
    {
      flag_match = true;
      regex.clear();
    }
  }

  // -i: disable -j (unconditional overrides conditional)
  if (flag_ignore_case)
    flag_smart_case = false;

  // -j: smart case insensitive search if regex does not contain a Unicode upper case letter
  if (flag_smart_case)
  {
    flag_ignore_case = true;

    for (const char *s = regex.c_str(); *s != '\0'; ++s)
    {
      if (*s == '\\')
      {
        ++s;
      }
      else if (*s == '{')
      {
        while (*++s != '\0' && *s != '}')
          continue;
      }
      else
      {
        int c = reflex::utf8(s, &s);
        if (reflex::Unicode::tolower(c) != c)
        {
          flag_ignore_case = false;
          break;
        }
        --s;
      }
    }
  }

  // -v or -y: disable -o and -u
  if (flag_invert_match || flag_any_line)
    flag_only_matching = flag_ungroup = false;

  // --match: when matching everything disable -ABC unless -o or -x
  if (flag_match && !flag_only_matching && !flag_line_regexp)
    flag_after_context = flag_before_context = 0;

  // -y: disable -ABC
  if (flag_any_line)
    flag_after_context = flag_before_context = 0;

  // -o and --replace: disable -ABC
  if (flag_only_matching && flag_replace != NULL)
    flag_after_context = flag_before_context = 0;

  // --depth: if -R or -r is not specified then enable -r
  if (flag_min_depth > 0 || flag_max_depth > 0)
    flag_directories_action = Action::RECURSE;

  // --index: enable -r if -r or -R is not enabled, reject -dskip
  if (flag_index != NULL)
  {
    if (flag_directories_action == Action::SKIP)
      usage("option --index searches recursively, but option -dskip is specified");
    flag_directories_action = Action::RECURSE;
  }

  // -p (--no-dereference) and -S (--dereference-files): -p takes priority over -S and -R
  if (flag_no_dereference)
    flag_dereference = flag_dereference_files = false;

  // output file name if more than one input file is specified or options -R, -r, and option -h --no-filename is not specified
  if (!flag_no_filename && (flag_all_threads || flag_directories_action == Action::RECURSE || Static::arg_files.size() > 1 || (flag_stdin && !Static::arg_files.empty())))
    flag_with_filename = true;

  // if no output options -H, -n, -k, -b are set, enable --no-header to suppress headers for speed
  if (!flag_with_filename && !flag_line_number && !flag_column_number && !flag_byte_offset)
    flag_no_header = true;

  // -q: we only need to find one matching file and we're done
  if (flag_quiet)
  {
    flag_max_files = 1;

    // -q overrides -l and -L
    flag_files_with_matches = false;
    flag_files_without_match = false;

    // disable --format options
    flag_format_begin = NULL;
    flag_format_open = NULL;
    flag_format = NULL;
    flag_format_close = NULL;
    flag_format_end = NULL;

    // disable sort, unnecessary
    flag_sort_key = Sort::NA;
  }

  // -o and --format: enable -u to ungroup matches
  if (flag_only_matching && flag_format != NULL)
    flag_ungroup = true;

  // -L: enable -l and flip -v i.e. -L=-lv and -l=-Lv
  if (flag_files_without_match)
  {
    flag_files_with_matches = true;
    flag_invert_match = !flag_invert_match;
  }

  // -l or -L: enable -H, disable -c
  if (flag_files_with_matches)
  {
    flag_with_filename = true;
    flag_count = false;
  }

  // --heading: enable --break when filenames are shown
  if (flag_heading && flag_with_filename && flag_break.is_undefined())
    flag_break = true;

  // get number of CPU cores, returns 0 if unknown
#if defined(__APPLE__) && defined(HAVE_NEON)
  // apple silicon with 8 or more cores should be reduced to 4 P cores
  Static::cores = 4;
#else
  Static::cores = std::thread::hardware_concurrency();
#endif

  // -J: the default is the number of cores, minus one for >= 8 cores, limited to DEFAULT_MAX_JOBS
  if (flag_jobs == 0)
  {
    flag_jobs = Static::cores - (Static::cores >= 8);

    // we want at least 2 threads to be available for workers if needed, but not more than DEFAULT_MAX_JOBS
    if (flag_jobs < 2)
      flag_jobs = 2;
    else if (flag_jobs > DEFAULT_MAX_JOBS)
      flag_jobs = DEFAULT_MAX_JOBS;
  }

  // --sort and --max-filea and not -l or -L or -c: limit number of threads to --max-files to prevent unordered results, this is a special case
  if (flag_sort_key != Sort::NA && flag_max_files > 0 && !flag_files_with_matches && !flag_count)
    flag_jobs = std::min(flag_jobs, flag_max_files);

  // set the number of threads to the number of files or when recursing to the value of -J, --jobs
  if (flag_all_threads || flag_directories_action == Action::RECURSE)
    Static::threads = flag_jobs;
  else
    Static::threads = std::min(Static::arg_files.size() + flag_stdin, flag_jobs);

  // inverted character classes and \s do not match newlines, e.g. [^x] matches anything except x and \n
  reflex::convert_flag_type convert_flags = reflex::convert_flag::notnewline;

  // not -U: convert regex to Unicode
  if (!flag_binary)
    convert_flags |= reflex::convert_flag::unicode;

  // -G: convert basic regex (BRE) to extended regex (ERE)
  if (flag_mode == Mode::BRE)
    convert_flags |= reflex::convert_flag::basic;

  // set reflex::Pattern options to enable multiline mode
  std::string pattern_options("(?m");

  // -i: case insensitive reflex::Pattern option
  if (flag_ignore_case)
    pattern_options.push_back('i');

  // --dotall and not --match (or empty pattern): dot matches newline
  if (flag_dotall)
    pattern_options.push_back('s');

  // --free-space: convert_flags is needed to check free-space conformance by the converter
  if (flag_free_space)
  {
    convert_flags |= reflex::convert_flag::freespace;
    pattern_options.push_back('x');
  }

  // prepend the pattern options (?m...) to the regex
  pattern_options.push_back(')');
  regex.insert(0, pattern_options);

  // reflex::Matcher options
  std::string matcher_options;

  // grep compatibility mode: literally match closing ) like GNU grep when not paired with ( unless -F or -P
  if (flag_grep && flag_mode != Mode::FIXED && flag_mode != Mode::PERL)
    convert_flags |= reflex::convert_flag::closing;

  // -Y or --empty: permit empty pattern matches
  if (flag_empty)
    matcher_options.push_back('N');

  // -w: match whole words
  if (flag_word_regexp)
    matcher_options.push_back('W');

  // --tabs: set reflex::Matcher option T to NUM (1, 2, 4, or 8) tab size
  if (flag_tabs)
    matcher_options.append("T=").push_back(static_cast<char>(flag_tabs) + '0');

  // --format-begin
  if (flag_format_begin != NULL)
    Output(Static::output).format(flag_format_begin, 0);

  // --index: search is only possible with compatible options
  if (flag_index != NULL)
  {
    if (flag_mode == Mode::PERL)
      usage("options --index and -P (--perl-regexp) are not compatible");
    if (!flag_filter.empty())
      usage("options --index and --filter are not compatible");
    if (flag_fuzzy > 0)
      usage("options --index and -Z (--fuzzy) are not compatible");
    if (flag_invert_match)
      usage("options --index and -v (--invert-match) are not compatible");
    if (flag_encoding != NULL)
      usage("options --index and --encoding are not compatible");

    // -c and --index: force --min-count larger than 0, because indexed search skips non-matching files
    if (flag_count && flag_min_count == 0)
      flag_min_count = 1;
  }

  if (flag_match)
  {
    // --match: match lines

    if (flag_line_regexp)
    {
      // -xv and --match: don't match empty lines (remove LineMatcher option N)
      if (flag_invert_match)
        matcher_options.clear();
      else // -x and --match: only match empty lines (set LineMatcher option W)
        matcher_options.push_back('X');

      // do not invert when searching
      flag_invert_match = false;
    }

    // --match and -v matches nothing, but -y and also -c should still list files when --min-count > 0
    if (!flag_invert_match || flag_any_line || (flag_count && flag_min_count == 0))
    {
      // -X (--hex) and --match: also output terminating \n in hexdumps (set LineMatcher option A)
      if (flag_hex)
        matcher_options.push_back('A');

      // --match: match lines with the RE/flex Line Matcher
      Static::matcher = std::unique_ptr<reflex::AbstractMatcher>(new reflex::LineMatcher(reflex::Input(), matcher_options.c_str()));

      // --pager
      open_pager();

      if (Static::threads > 1)
      {
        GrepMaster grep(Static::output, Static::matcher.get(), NULL);
        grep.ugrep();
      }
      else
      {
        Grep grep(Static::output, Static::matcher.get(), NULL);
        Static::set_grep_handle(&grep);
        grep.ugrep();
        Static::clear_grep_handle();
      }
    }
  }
  else if (flag_mode == Mode::PERL)
  {
    // -P: Perl matching with PCRE2 or Boost.Regex
#if defined(HAVE_PCRE2)
    // construct the PCRE2 JIT-optimized Perl pattern matcher
    uint32_t options = flag_binary ? (PCRE2_NEVER_UTF | PCRE2_NEVER_UCP) : (PCRE2_UTF | PCRE2_UCP);
    if (!Static::bcnf.singleton_or_undefined())
      options |= PCRE2_DUPNAMES;
    Static::string_pattern.assign(flag_binary ? reflex::PCRE2Matcher::convert(regex, convert_flags, &flag_multiline) : reflex::PCRE2UTFMatcher::convert(regex, convert_flags, &flag_multiline));
    Static::matcher = std::unique_ptr<reflex::AbstractMatcher>(new reflex::PCRE2Matcher(Static::string_pattern, reflex::Input(), matcher_options.c_str(), options));
    Static::matchers.clear();

    if (!Static::bcnf.singleton_or_undefined())
    {
      std::string subregex;

      for (const auto& i : Static::bcnf.lists())
      {
        Static::matchers.emplace_back();

        auto& submatchers = Static::matchers.back();

        for (const auto& j : i)
        {
          if (j)
          {
            subregex.assign(pattern_options).append(*j);
            submatchers.emplace_back(new reflex::PCRE2Matcher((flag_binary ? reflex::PCRE2Matcher::convert(subregex, convert_flags) : reflex::PCRE2UTFMatcher::convert(subregex, convert_flags)), reflex::Input(), matcher_options.c_str(), options));
          }
          else
          {
            submatchers.emplace_back();
          }
        }
      }
    }

    // --pager
    open_pager();

    if (Static::threads > 1)
    {
      GrepMaster grep(Static::output, Static::matcher.get(), Static::bcnf.singleton_or_undefined() ? NULL : &Static::matchers);
      grep.ugrep();
    }
    else
    {
      Grep grep(Static::output, Static::matcher.get(), Static::bcnf.singleton_or_undefined() ? NULL : &Static::matchers);
      Static::set_grep_handle(&grep);
      grep.ugrep();
      Static::clear_grep_handle();
    }
#elif defined(HAVE_BOOST_REGEX)
    try
    {
      // construct the Boost.Regex Perl pattern matcher
      Static::string_pattern.assign(reflex::BoostPerlMatcher::convert(regex, convert_flags, &flag_multiline));
      Static::matcher = std::unique_ptr<reflex::AbstractMatcher>(new reflex::BoostPerlMatcher(Static::string_pattern, reflex::Input(), matcher_options.c_str()));
      Static::matchers.clear();

      if (!Static::bcnf.singleton_or_undefined())
      {
        std::string subregex;

        for (const auto& i : Static::bcnf.lists())
        {
          Static::matchers.emplace_back();

          auto& submatchers = Static::matchers.back();

          for (const auto& j : i)
          {
            if (j)
            {
              subregex.assign(pattern_options).append(*j);
              submatchers.emplace_back(new reflex::BoostPerlMatcher(reflex::BoostPerlMatcher::convert(subregex, convert_flags), reflex::Input(), matcher_options.c_str()));
            }
            else
            {
              submatchers.emplace_back();
            }
          }
        }
      }

      // --pager
      open_pager();

      if (Static::threads > 1)
      {
        GrepMaster grep(Static::output, Static::matcher.get(), Static::bcnf.singleton_or_undefined() ? NULL : &Static::matchers);
        grep.ugrep();
      }
      else
      {
        Grep grep(Static::output, Static::matcher.get(), Static::bcnf.singleton_or_undefined() ? NULL : &Static::matchers);
        Static::set_grep_handle(&grep);
        grep.ugrep();
        Static::clear_grep_handle();
      }
    }

    catch (boost::regex_error& error)
    {
      reflex::regex_error_type code;

      switch (error.code())
      {
        case boost::regex_constants::error_collate:
          code = reflex::regex_error::invalid_collating;
          break;
        case boost::regex_constants::error_ctype:
          code = reflex::regex_error::invalid_class;
          break;
        case boost::regex_constants::error_escape:
          code = reflex::regex_error::invalid_escape;
          break;
        case boost::regex_constants::error_backref:
          code = reflex::regex_error::invalid_backreference;
          break;
        case boost::regex_constants::error_brack:
          code = reflex::regex_error::invalid_class;
          break;
        case boost::regex_constants::error_paren:
          code = reflex::regex_error::mismatched_parens;
          break;
        case boost::regex_constants::error_brace:
          code = reflex::regex_error::mismatched_braces;
          break;
        case boost::regex_constants::error_badbrace:
          code = reflex::regex_error::invalid_repeat;
          break;
        case boost::regex_constants::error_range:
          code = reflex::regex_error::invalid_class_range;
          break;
        case boost::regex_constants::error_space:
          code = reflex::regex_error::exceeds_limits;
          break;
        case boost::regex_constants::error_badrepeat:
          code = reflex::regex_error::invalid_repeat;
          break;
        case boost::regex_constants::error_complexity:
          code = reflex::regex_error::exceeds_limits;
          break;
        case boost::regex_constants::error_stack:
          code = reflex::regex_error::exceeds_limits;
          break;
        default:
          code = reflex::regex_error::invalid_syntax;
      }

      throw reflex::regex_error(code, Static::string_pattern, error.position() + 1);
    }
#endif
  }
  else
  {
    // construct the RE/flex DFA-based pattern matcher and start matching files
    Static::reflex_pattern.assign(reflex::Matcher::convert(regex, convert_flags, &flag_multiline), (flag_index != NULL ? "hr" : "r"));
    Static::matchers.clear();

    if (flag_fuzzy > 0)
    {
      // -U or --binary: disable fuzzy Unicode matching, ASCII/binary matching with -Z MAX edit distance
      uint16_t max = static_cast<uint16_t>(flag_fuzzy) | (flag_binary ? reflex::FuzzyMatcher::BIN : 0);
      Static::matcher = std::unique_ptr<reflex::AbstractMatcher>(new reflex::FuzzyMatcher(Static::reflex_pattern, max, reflex::Input(), matcher_options.c_str()));

      if (!Static::bcnf.singleton_or_undefined())
      {
        std::string subregex;

        for (const auto& i : Static::bcnf.lists())
        {
          Static::matchers.emplace_back();

          auto& submatchers = Static::matchers.back();

          for (const auto& j : i)
          {
            if (j)
            {
              subregex.assign(pattern_options).append(*j);
              Static::reflex_patterns.emplace_back(reflex::FuzzyMatcher::convert(subregex, convert_flags), "r");
              submatchers.emplace_back(new reflex::FuzzyMatcher(Static::reflex_patterns.back(), reflex::Input(), matcher_options.c_str()));
            }
            else
            {
              submatchers.emplace_back();
            }
          }
        }
      }

      // --pager
      open_pager();

      if (Static::threads > 1)
      {
        GrepMaster grep(Static::output, Static::matcher.get(), Static::bcnf.singleton_or_undefined() ? NULL : &Static::matchers);
        grep.ugrep();
      }
      else
      {
        Grep grep(Static::output, Static::matcher.get(), Static::bcnf.singleton_or_undefined() ? NULL : &Static::matchers);
        Static::set_grep_handle(&grep);
        grep.ugrep();
        Static::clear_grep_handle();
      }
    }
    else
    {
      Static::matcher = std::unique_ptr<reflex::AbstractMatcher>(new reflex::Matcher(Static::reflex_pattern, reflex::Input(), matcher_options.c_str()));

      if (!Static::bcnf.singleton_or_undefined())
      {
        std::string subregex;

        for (const auto& i : Static::bcnf.lists())
        {
          Static::matchers.emplace_back();

          auto& submatchers = Static::matchers.back();

          for (const auto& j : i)
          {
            if (j)
            {
              subregex.assign(pattern_options).append(*j);
              Static::reflex_patterns.emplace_back(reflex::Matcher::convert(subregex, convert_flags), "r");
              submatchers.emplace_back(new reflex::Matcher(Static::reflex_patterns.back(), reflex::Input(), matcher_options.c_str()));
            }
            else
            {
              submatchers.emplace_back();
            }
          }
        }
      }

      // --index: perform indexed search using the pattern indexing hash finite state automaton (HFA)
      if (flag_index != NULL && Static::reflex_pattern.has_hfa())
        Static::index_pattern = &Static::reflex_pattern;

      // --pager
      open_pager();

      if (Static::threads > 1)
      {
        GrepMaster grep(Static::output, Static::matcher.get(), Static::bcnf.singleton_or_undefined() ? NULL : &Static::matchers);
        grep.ugrep();
      }
      else
      {
        Grep grep(Static::output, Static::matcher.get(), Static::bcnf.singleton_or_undefined() ? NULL : &Static::matchers);
        Static::set_grep_handle(&grep);
        grep.ugrep();
        Static::clear_grep_handle();
      }

      // pattern is out of scope and implicitly deleted, invalidating this pointer
      Static::index_pattern = NULL;
    }
  }

  // --tree with -l or -c but not --format: finish tree output
  if (flag_tree && (flag_files_with_matches || flag_count) && flag_format == NULL)
  {
    Output out(Static::output);
    for (int i = 1; i < Output::Tree::depth(); ++i)
      out.str(Output::Tree::end());
    out.nl();
  }

  // --format-end
  if (flag_format_end != NULL)
    Output(Static::output).format(flag_format_end, Stats::found_parts());

  // --stats: output stats when we're done
  if (flag_stats != NULL)
  {
    Stats::report(Static::output);

    Static::bcnf.report(Static::output);

    if (strcmp(flag_stats, "vm") == 0)
    {
      size_t nodes = Static::reflex_pattern.nodes();
      size_t edges = Static::reflex_pattern.edges();
      size_t words = Static::reflex_pattern.words();
      size_t hashes = Static::reflex_pattern.hashes();

      size_t nodes_time = static_cast<size_t>(Static::reflex_pattern.nodes_time());
      size_t edges_time = static_cast<size_t>(Static::reflex_pattern.parse_time() + Static::reflex_pattern.edges_time());
      size_t words_time = static_cast<size_t>(Static::reflex_pattern.words_time());
      size_t study_time = static_cast<size_t>(Static::reflex_pattern.analysis_time());

      fprintf(Static::output, "VM: %zu nodes (%zums) %zu edges (%zums) %zu opcode words (%zums)", nodes, nodes_time, edges, edges_time, words, words_time);
      if (hashes > 0)
        fprintf(Static::output, " %zu hash tables (%zums)", hashes, study_time);
      fprintf(Static::output, NEWLINESTR);
    }
  }

  // --pager
  close_pager();
}

// perform a limited ugrep search on a single file with optional archive part and store up to num results in a vector, may throw an exception, used by the TUI
void ugrep_find_text_preview(const char *filename, const char *partname, size_t from_lineno, size_t max, size_t& lineno, size_t& num, std::vector<std::string>& text)
{
  // we can only return a text preview when the main search engine ugrep() already started and executes, or after it ends
  if (!Static::matcher)
    return;

  // clone matcher, since it is not thread safe to use when the main search engine still executes
  reflex::AbstractMatcher *matcher = Static::matcher->clone();

  // clone the CNF matchers if any are used
  Static::Matchers *matchers = Static::bcnf.singleton_or_undefined() ? NULL : Static::matchers_clone();

  Grep grep(NULL, matcher, matchers);
  grep.find_text_preview(filename, partname, from_lineno, max, lineno, num, text);

  // delete the cloned matchers, if any
  if (matchers != NULL)
    delete matchers;

  // delete the cloned matcher
  delete matcher;
}

// extract a part from an archive and send to a stream, used by the TUI
void ugrep_extract(const char *filename, const char *partname, FILE *output)
{
  Grep grep(NULL, NULL, NULL);
  grep.extract(filename, partname, output);
}

// set the handle to be able to use cancel_ugrep()
void Static::set_grep_handle(Grep *grep)
{
  std::unique_lock<std::mutex> lock(Static::grep_handle_mutex);
  Static::grep_handle = grep;
  lock.unlock();
}

// reset the grep handle
void Static::clear_grep_handle()
{
  std::unique_lock<std::mutex> lock(Static::grep_handle_mutex);
  Static::grep_handle = NULL;
  lock.unlock();
}

// cancel the search
void Static::cancel_ugrep()
{
  std::unique_lock<std::mutex> lock(Static::grep_handle_mutex);
  if (Static::grep_handle != NULL)
    Static::grep_handle->cancel();
  lock.unlock();
}

// search the specified files or standard input for pattern matches
void Grep::ugrep()
{
  // read each input file to find pattern matches
  if (flag_stdin)
  {
    Stats::score_file();

    // search standard input
    search(Static::LABEL_STANDARD_INPUT, static_cast<uint16_t>(flag_fuzzy));
  }

  if (Static::arg_files.empty())
  {
    if (flag_directories_action == Action::RECURSE)
      recurse(1, ".");
  }
  else
  {
#ifdef OS_WIN_OR_MINGW
    const DWORD attr = INVALID_FILE_ATTRIBUTES;
#else
    const uint32_t attr = 0;
    std::pair<std::set<ino_t>::iterator,bool> vino;
#endif

    for (const auto pathname : Static::arg_files)
    {
      // stop after finding max-files matching files
      if (flag_max_files > 0 && Stats::found_parts() >= flag_max_files)
        break;

      // stop when output is blocked or search cancelled
      if (out.eof || out.cancelled())
        break;

      // search file or directory, get the basename from the file argument first
      const char *basename = strrchr(pathname, PATHSEPCHR);
      if (basename != NULL)
        ++basename;
      else
        basename = pathname;

      ino_t inode = 0;
      uint64_t info;

      // search file or recursively search directory based on selection criteria
      switch (select(1, pathname, basename, DIRENT_TYPE_UNKNOWN, attr, inode, info, true))
      {
        case Type::DIRECTORY:
          if (flag_directories_action != Action::SKIP)
          {
#ifndef OS_WIN_OR_MINGW
            if (flag_dereference)
              vino = visited.insert(inode);
#endif

            recurse(1, pathname);

#ifndef OS_WIN_OR_MINGW
            if (flag_dereference)
              visited.erase(vino.first);
#endif
          }
          break;

        case Type::OTHER:
          search(pathname, Entry::UNDEFINED_COST);
          break;

        case Type::SKIP:
          break;
      }
    }
  }
}

// select file or directory to search for pattern matches, return SKIP, DIRECTORY or OTHER
Grep::Type Grep::select(size_t level, const char *pathname, const char *basename, int type, uint32_t attr, ino_t& inode, uint64_t& info, bool is_argument)
{
  if (*basename == '.' && !flag_hidden && !is_argument)
    return Type::SKIP;

#ifdef OS_WIN_OR_MINGW

  (void)type; (void)inode; (void)info; // appease -Wunused

  // use file attributes from FindFirstFileW() WIN32_FIND_DATAW dwFileAttributes otherwise attr == INVALID_FILE_ATTRIBUTES
  if (attr == INVALID_FILE_ATTRIBUTES)
    attr = GetFileAttributesW(utf8_decode(pathname).c_str());

  if (attr == INVALID_FILE_ATTRIBUTES)
  {
    errno = ENOENT;
    warning("cannot read", pathname);
    return Type::SKIP;
  }

  // skip hidden and system files unless explicitly searched with --hidden
  if (!flag_hidden && !is_argument && ((attr & FILE_ATTRIBUTE_HIDDEN) || (attr & FILE_ATTRIBUTE_SYSTEM)))
    return Type::SKIP;

  if ((attr & FILE_ATTRIBUTE_DIRECTORY) != 0)
  {
    if (flag_directories_action == Action::READ)
    {
      // directories cannot be read actually, so grep produces a warning message (errno is not set)
      is_directory(pathname);
      return Type::SKIP;
    }

    if (is_argument || flag_directories_action == Action::RECURSE)
    {
      // --depth: recursion level exceeds max depth?
      if (flag_max_depth > 0 && level > flag_max_depth)
        return Type::SKIP;

      // hard maximum recursion depth reached?
      if (level > MAX_DEPTH)
      {
        if (!flag_no_messages)
          fprintf(Static::errout, "%sugrep: %s%s%s recursion depth hit hard limit of %d\n", color_off, color_high, pathname, color_off, MAX_DEPTH);
        return Type::SKIP;
      }

      // check for --exclude-dir and --include-dir constraints if pathname != "."
      if (strcmp(pathname, ".") != 0)
      {
        if (!flag_all_exclude_dir.empty())
        {
          // exclude directories whose pathname matches any one of the --exclude-dir globs unless negated with !
          bool ok = true;
          for (const auto& glob : flag_all_exclude_dir)
          {
            bool ignore_case = &glob < &flag_all_exclude_dir.front() + flag_exclude_iglob_size;
            if (glob.front() == '!')
            {
              if (!ok && glob_match(pathname, basename, glob.c_str() + 1, ignore_case))
                ok = true;
            }
            else if (ok && glob_match(pathname, basename, glob.c_str(), ignore_case))
            {
              ok = false;
            }
          }
          if (!ok)
            return Type::SKIP;
        }

        if (!flag_all_include_dir.empty())
        {
          // include directories whose pathname matches any one of the --include-dir globs unless negated with !
          bool ok = false;
          for (const auto& glob : flag_all_include_dir)
          {
            bool ignore_case = &glob < &flag_all_include_dir.front() + flag_include_iglob_dir_size;
            if (glob.front() == '!')
            {
              if (ok && glob_match(pathname, basename, glob.c_str() + 1, ignore_case))
                ok = false;
            }
            else if (!ok && glob_match(pathname, basename, glob.c_str(), ignore_case, true))
            {
              // the pathname matches the leading --include-dir glob, OK to descent into directory
              ok = true;
            }
          }
          if (!ok)
            return Type::SKIP;
        }
      }

      return Type::DIRECTORY;
    }
  }
  else if ((attr & FILE_ATTRIBUTE_DEVICE) == 0 || (is_argument && flag_devices_action == Action::UNSP) || flag_devices_action == Action::READ)
  {
    // --depth: recursion level not deep enough?
    if (flag_min_depth > 0 && level <= flag_min_depth)
      return Type::SKIP;

    bool ok;

    if (!flag_all_include_dir.empty())
    {
      // only include paths to the file to search that match an --include-dir path glob
      ok = false;
      for (const auto& glob : flag_all_include_dir)
      {
        bool ignore_case = &glob < &flag_all_include_dir.front() + flag_include_iglob_dir_size;
        if (glob.front() != '!' && glob_match(pathname, basename, glob.c_str(), ignore_case, false, true))
        {
          // pathname matches the --include-dir path glob
          ok = true;
          break;
        }
      }
      if (!ok)
        return Type::SKIP;
    }

    ok = true;

    if (!flag_all_exclude.empty())
    {
      // exclude files whose pathname matches any one of the --exclude globs unless negated with !
      for (const auto& glob : flag_all_exclude)
      {
        bool ignore_case = &glob < &flag_all_exclude.front() + flag_exclude_iglob_size;
        if (glob.front() == '!')
        {
          if (!ok && glob_match(pathname, basename, glob.c_str() + 1, ignore_case))
            ok = true;
        }
        else if (ok && glob_match(pathname, basename, glob.c_str(), ignore_case))
        {
          ok = false;
        }
      }
      if (!ok)
        return Type::SKIP;
    }

    if (!flag_all_include.empty())
    {
      // include files whose pathname matches any one of the --include globs unless negated with !
      ok = false;
      for (const auto& glob : flag_all_include)
      {
        bool ignore_case = &glob < &flag_all_include.front() + flag_include_iglob_size;
        if (glob.front() == '!')
        {
          if (ok && glob_match(pathname, basename, glob.c_str() + 1), ignore_case)
            ok = false;
        }
        else if (!ok && glob_match(pathname, basename, glob.c_str(), ignore_case))
        {
          ok = true;
        }
      }
      if (!ok && flag_file_magic.empty())
        return Type::SKIP;
    }

    // check magic pattern against the file signature, when --file-magic=MAGIC is specified
    if (!flag_file_magic.empty() && (flag_all_include.empty() || !ok))
    {
      FILE *file;

      if (fopenw_s(&file, pathname, "rb") != 0)
      {
        warning("cannot read", pathname);
        return Type::SKIP;
      }

#ifdef HAVE_LIBZ
      if (flag_decompress)
      {
        zstreambuf streambuf(pathname, file);
        std::istream stream(&streambuf);

        // file has the magic bytes we're looking for: search the file
        size_t match = Static::magic_matcher.input(&stream).scan();
        if (match == flag_not_magic || match >= flag_min_magic)
        {
          fclose(file);

          Stats::score_file();

          return Type::OTHER;
        }
      }
      else
#endif
      {
        size_t match = Static::magic_matcher.input(reflex::Input(file, flag_encoding_type)).scan();
        if (match == flag_not_magic || match >= flag_min_magic)
        {
          fclose(file);

          Stats::score_file();

          return Type::OTHER;
        }
      }

      fclose(file);

      return Type::SKIP;
    }

    Stats::score_file();

    return Type::OTHER;
  }

#else

  (void)attr;

  struct stat buf;

  // -R or command line argument FILE to search: follow symlinks to directories
  bool follow = flag_dereference || is_argument;

  // if dir entry is unknown and not following, then use lstat() to check if pathname is a symlink
  if (type != DIRENT_TYPE_UNKNOWN || follow || lstat(pathname, &buf) == 0)
  {
    // is it a symlink? If dir entry unknown and following then set to symlink = true to call stat() below
    bool symlink = type != DIRENT_TYPE_UNKNOWN ? type == DIRENT_TYPE_LNK : follow ? true : S_ISLNK(buf.st_mode);
    // if we got a symlink, use stat() to check if pathname is a directory or a regular file, we also stat when following and when sorting by stat info such as modification time
    if (( ( (type != DIRENT_TYPE_UNKNOWN && type != DIRENT_TYPE_LNK) ||    // type is known and not symlink
            (!follow && !symlink)                                          // or not following and not symlink
          ) &&
          (flag_sort_key == Sort::NA || flag_sort_key == Sort::NAME)       // and we're not sorting or by name
        ) ||
        stat(pathname, &buf) == 0)                                      // otherwise, do a stat
    {
      // check if directory
      if (type == DIRENT_TYPE_DIR || ((type == DIRENT_TYPE_UNKNOWN || type == DIRENT_TYPE_LNK) && S_ISDIR(buf.st_mode)))
      {
        // if symlinked directory, then follow only if -R is specified or if FILE is a command line argument
        if (!symlink || follow)
        {
          if (flag_directories_action == Action::READ)
          {
            // directories cannot be read actually, so grep produces a warning message (errno is not set)
            is_directory(pathname);
            return Type::SKIP;
          }

          if (is_argument || flag_directories_action == Action::RECURSE)
          {
            // --depth: recursion level exceeds max depth?
            if (flag_max_depth > 0 && level > flag_max_depth)
              return Type::SKIP;

            // hard maximum recursion depth reached?
            if (level > MAX_DEPTH)
            {
              if (!flag_no_messages)
                fprintf(Static::errout, "%sugrep: %s%s%s recursion depth hit hard limit of %d\n", color_off, color_high, pathname, color_off, MAX_DEPTH);
              return Type::SKIP;
            }

            // check for --exclude-dir and --include-dir constraints if pathname != "."
            if (strcmp(pathname, ".") != 0)
            {
              if (!flag_all_exclude_dir.empty())
              {
                // exclude directories whose pathname matches any one of the --exclude-dir globs unless negated with !
                bool ok = true;
                for (const auto& glob : flag_all_exclude_dir)
                {
                  bool ignore_case = &glob < &flag_all_exclude_dir.front() + flag_exclude_iglob_dir_size;
                  if (glob.front() == '!')
                  {
                    if (!ok && glob_match(pathname, basename, glob.c_str() + 1, ignore_case))
                      ok = true;
                  }
                  else if (ok && glob_match(pathname, basename, glob.c_str(), ignore_case))
                  {
                    ok = false;
                  }
                }
                if (!ok)
                  return Type::SKIP;
              }

              if (!flag_all_include_dir.empty())
              {
                // include directories whose name matches any one of the --include-dir globs unless negated with !
                bool ok = false;
                for (const auto& glob : flag_all_include_dir)
                {
                  bool ignore_case = &glob < &flag_all_include_dir.front() + flag_include_iglob_dir_size;
                  if (glob.front() == '!')
                  {
                    if (ok && glob_match(pathname, basename, glob.c_str() + 1, ignore_case))
                      ok = false;
                  }
                  else if (!ok && glob_match(pathname, basename, glob.c_str(), ignore_case, true))
                  {
                    // the pathname matches the leading --include-dir glob, OK to descent into directory
                    ok = true;
                  }
                }
                if (!ok)
                  return Type::SKIP;
              }
            }

            if (type != DIRENT_TYPE_DIR)
              inode = buf.st_ino;

            info = Entry::sort_info(buf);

            return Type::DIRECTORY;
          }
        }
      }
      else if (type == DIRENT_TYPE_REG ? !is_output(inode) : (type == DIRENT_TYPE_UNKNOWN || type == DIRENT_TYPE_LNK) && S_ISREG(buf.st_mode) ? !is_output(buf.st_ino) : (is_argument && flag_devices_action == Action::UNSP) || flag_devices_action == Action::READ)
      {
        // if symlinked files, then follow only if -R or -S is specified or if FILE is a command line argument
        if (!symlink || follow || flag_dereference_files)
        {
          // --depth: recursion level not deep enough?
          if (flag_min_depth > 0 && level <= flag_min_depth)
            return Type::SKIP;

          bool ok;

          if (!flag_all_include_dir.empty())
          {
            // only include paths to the file to search that match an --include-dir path glob
            ok = false;
            for (const auto& glob : flag_all_include_dir)
            {
              bool ignore_case = &glob < &flag_all_include_dir.front() + flag_include_iglob_dir_size;
              if (glob.front() != '!' && glob_match(pathname, basename, glob.c_str(), ignore_case, false, true))
              {
                // pathname matches the --include-dir path glob
                ok = true;
                break;
              }
            }
            if (!ok)
              return Type::SKIP;
          }

          ok = true;

          if (!flag_all_exclude.empty())
          {
            // exclude files whose pathname matches any one of the --exclude globs unless negated with !
            for (const auto& glob : flag_all_exclude)
            {
              bool ignore_case = &glob < &flag_all_exclude.front() + flag_exclude_iglob_size;
              if (glob.front() == '!')
              {
                if (!ok && glob_match(pathname, basename, glob.c_str() + 1, ignore_case))
                  ok = true;
              }
              else if (ok && glob_match(pathname, basename, glob.c_str(), ignore_case))
              {
                ok = false;
              }
            }
            if (!ok)
              return Type::SKIP;
          }

          if (!flag_all_include.empty())
          {
            // include files whose basename matches any one of the --include globs if not negated with !
            ok = false;
            for (const auto& glob : flag_all_include)
            {
              bool ignore_case = &glob < &flag_all_include.front() + flag_include_iglob_size;
              if (glob.front() == '!')
              {
                if (ok && glob_match(pathname, basename, glob.c_str() + 1, ignore_case))
                  ok = false;
              }
              else if (!ok && glob_match(pathname, basename, glob.c_str(), ignore_case))
              {
                ok = true;
              }
            }
            if (!ok && flag_file_magic.empty())
              return Type::SKIP;
          }

          // --min-size, --max-size: skip regular files that are too small or too large
          if (flag_min_size != 0 || flag_max_size != 0)
          {
            // if stat not done then do it to get size
            if (type == DIRENT_TYPE_REG && (flag_sort_key == Sort::NA || flag_sort_key == Sort::NAME))
              stat(pathname, &buf);

            if (buf.st_size < static_cast<off_t>(flag_min_size) || (flag_max_size > 0 && buf.st_size > static_cast<off_t>(flag_max_size)))
              return Type::SKIP;
          }

          // check magic pattern against the file signature, when --file-magic=MAGIC is specified
          if (!flag_file_magic.empty() && (flag_all_include.empty() || !ok))
          {
            FILE *file;

            if (fopenw_s(&file, pathname, "rb") != 0)
            {
              warning("cannot read", pathname);
              return Type::SKIP;
            }

#ifdef HAVE_LIBZ
            if (flag_decompress)
            {
              zstreambuf streambuf(pathname, file);
              std::istream stream(&streambuf);

              // file has the magic bytes we're looking for: search the file
              size_t match = Static::magic_matcher.input(&stream).scan();
              if (match == flag_not_magic || match >= flag_min_magic)
              {
                fclose(file);

                Stats::score_file();

                info = Entry::sort_info(buf);

                return Type::OTHER;
              }
            }
            else
#endif
            {
              // if file has the magic bytes we're looking for: search the file
              size_t match = Static::magic_matcher.input(reflex::Input(file, flag_encoding_type)).scan();
              if (match == flag_not_magic || match >= flag_min_magic)
              {
                fclose(file);

                Stats::score_file();

                info = Entry::sort_info(buf);

                return Type::OTHER;
              }
            }

            fclose(file);

            return Type::SKIP;
          }

          Stats::score_file();

          info = Entry::sort_info(buf);

          return Type::OTHER;
        }
      }
    }
  }
  else
  {
    warning("lstat", pathname);
  }

#endif

  return Type::SKIP;
}

// recurse over directory, searching for pattern matches in files and subdirectories
void Grep::recurse(size_t level, const char *pathname)
{
  // output is closed or cancelled?
  if (out.eof || out.cancelled())
    return;

#ifdef OS_WIN_OR_MINGW

  WIN32_FIND_DATAW ffd;

  std::string glob;

  if (strcmp(pathname, ".") != 0)
    glob.assign(pathname).append("/*");
  else
    glob.assign("*");

  std::wstring wglob(utf8_decode(glob));
  HANDLE hFind = FindFirstFileW(wglob.c_str(), &ffd);

  if (hFind == INVALID_HANDLE_VALUE)
  {
    if (GetLastError() != ERROR_FILE_NOT_FOUND)
      warning("cannot open directory", pathname);
    return;
  }

#else

#if defined(HAVE_STATVFS) || defined(HAVE_STATFS)

  if (!exclude_fs_ids.empty() || !include_fs_ids.empty())
  {
    stat_fs_t buf;

    if (stat_fs(pathname, &buf) == 0)
    {
      if (exclude_fs_ids.find(fsid_to_uint64(buf.f_fsid)) != exclude_fs_ids.end())
        return;

      if (!include_fs_ids.empty() && include_fs_ids.find(fsid_to_uint64(buf.f_fsid)) == include_fs_ids.end())
        return;
    }
  }

#endif

  DIR *dir = opendir(pathname);

  if (dir == NULL)
  {
    warning("cannot open directory", pathname);
    return;
  }

#endif

  // --index: check index file, but only when necessary on demand to store indexed file entries in a map
  bool index_demand = Static::index_pattern != NULL;
  std::map<std::string,bool> indexed;

  // the indexing file stored per indexed directory and index file identifying magic bytes
  static const char ugrep_index_filename[] = "._UG#_Store";
  static const char ugrep_index_file_magic[5] = "UG#\x03";

  // --ignore-files: check if one or more are present to read and extend the file and dir exclusions
  size_t saved_all_exclude_size = 0;
  size_t saved_all_exclude_dir_size = 0;
  bool saved = false;

  if (!flag_ignore_files.empty())
  {
    std::string ignore_filename;

    for (const auto& ignore_file : flag_ignore_files)
    {
      ignore_filename.assign(pathname).append(PATHSEPSTR).append(ignore_file);

      FILE *file = NULL;

      if (fopenw_s(&file, ignore_filename.c_str(), "r") == 0)
      {
        // save current exclusions vector depths to restore afterwards
        if (!saved)
        {
          saved_all_exclude_size = flag_all_exclude.size();
          saved_all_exclude_dir_size = flag_all_exclude_dir.size();
          saved = true;
        }

        Stats::ignore_file(ignore_filename);

        // push globs imported from the ignore file to the back of the vectors
        import_globs(file, flag_all_exclude, flag_all_exclude_dir, true, pathname);

        fclose(file);
      }
    }
  }

  Stats::score_dir();

  std::vector<Entry> file_entries;
  std::vector<Entry> dir_entries;
  std::string entry_pathname;

#ifdef OS_WIN_OR_MINGW

  std::string cFileName;
  uint64_t list = 0;
  uint64_t index_time = 0;

  do
  {
    cFileName.assign(utf8_encode(ffd.cFileName));

    // search directory entries that aren't . or .. or hidden
    if (cFileName[0] != '.' || (flag_hidden && cFileName[1] != '\0' && cFileName[1] != '.'))
    {
      // --index: do not search index files themselves, even when --hidden is specified
      if (flag_index != NULL && cFileName == ugrep_index_filename)
        continue;

      // --min-size, --max-size: skip regular files that are too small or too large
      if (flag_min_size != 0 || flag_max_size != 0)
      {
        uint64_t size = static_cast<uint64_t>(ffd.nFileSizeLow) | (static_cast<uint64_t>(ffd.nFileSizeHigh) << 32);
        if (size < flag_min_size || (flag_max_size != 0 && size > flag_max_size))
          continue;
      }

      size_t len = strlen(pathname);

      if (len == 1 && pathname[0] == '.')
        entry_pathname.assign(cFileName);
      else if (len > 0 && pathname[len - 1] == PATHSEPCHR)
        entry_pathname.assign(pathname).append(cFileName);
      else
        entry_pathname.assign(pathname).append(PATHSEPSTR).append(cFileName);

      uint64_t info = 0;

      // --sort: get file info
      if (flag_sort_key != Sort::NA && flag_sort_key != Sort::NAME)
      {
        if (flag_sort_key == Sort::SIZE)
        {
          info = static_cast<uint64_t>(ffd.nFileSizeLow) | (static_cast<uint64_t>(ffd.nFileSizeHigh) << 32);
        }
        else if (flag_sort_key == Sort::LIST)
        {
          info = list++;
        }
        else
        {
          struct _FILETIME& time = flag_sort_key == Sort::USED ? ffd.ftLastAccessTime : flag_sort_key == Sort::CHANGED ? ffd.ftLastWriteTime : ffd.ftCreationTime;
          info = static_cast<uint64_t>(time.dwLowDateTime) | (static_cast<uint64_t>(time.dwHighDateTime) << 32);
        }
      }

      ino_t inode = 0;
      Type type = select(level + 1, entry_pathname.c_str(), cFileName.c_str(), DIRENT_TYPE_UNKNOWN, ffd.dwFileAttributes, inode, info);

      // check if we're recursing on a symlink to skip them when -p or -r is specified
      if (!flag_dereference && (ffd.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0 && ffd.dwReserved0 == IO_REPARSE_TAG_SYMLINK)
      {
        if (type == Type::DIRECTORY)
          type = Type::SKIP;
        else if (type == Type::OTHER && !flag_dereference_files)
          type = Type::SKIP;
      }

      // --index: search indexed files quickly, but only when necessary on demand
      if (type == Type::OTHER && Static::index_pattern != NULL)
      {
        // if we did not check for an index file yet, then check it now on demand and read it when found
        if (index_demand)
        {
          // check no more than once at most
          index_demand = false;

          // check if an index file is present and use it
          std::string index_filename(pathname);
          index_filename.append(PATHSEPSTR).append(ugrep_index_filename);
          std::wstring windex_filename(utf8_decode(index_filename));
          HANDLE hFile = CreateFileW(windex_filename.c_str(), GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN, NULL);

          if (hFile != INVALID_HANDLE_VALUE)
          {
            char check_magic[sizeof(ugrep_index_file_magic)];
            DWORD numread = 0;

            // if an index file is present in the directory pathname, then read and stat it
            if (ReadFile(hFile, check_magic, sizeof(ugrep_index_file_magic), &numread, NULL) &&
                numread == sizeof(ugrep_index_file_magic) &&
                memcmp(check_magic, ugrep_index_file_magic, sizeof(ugrep_index_file_magic)) == 0)
            {
              // time of indexing to check which files were modified after indexing
              index_time = Entry::modified_time(hFile);

              // allocate a buffer, not on the stack, because we are in a deeply recursive function
              char *buffer = new char[65536]; // size must be 65536
              uint8_t header[4];
              std::map<std::string,bool>::iterator skip = indexed.end();

              // populate indexed map
              while (true)
              {
                if (!ReadFile(hFile, header, sizeof(header), &numread, NULL) || numread != sizeof(header))
                  break;

                uint16_t basename_size = header[2] | (header[3] << 8);

                if (!ReadFile(hFile, buffer, basename_size, &numread, NULL) || numread != basename_size)
                  break;

                // make basename in buffer 0-terminated
                buffer[basename_size] = '\0';

                // hashes table size, zero to skip empty files and binary files when -I is specified
                uint32_t hashes_size = 0;
                uint8_t logsize = header[1] & 0x1f;
                if (logsize > 0)
                  for (hashes_size = 1; logsize > 0; --logsize)
                    hashes_size <<= 1;

                // sanity check
                if (hashes_size > 65536)
                  break;

                // archives have multiple entries in the index file, we keep the skip iterator to compare entries
                if ((header[1] & 0x40) == 0)
                  skip = indexed.end();

                // we're still looking in the same archive?
                bool same_archive = skip != indexed.end() && skip->first == buffer;

                // if not and archive or not the same archive entry, then add basename to the indexed map
                if (!same_archive)
                {
                  Stats::score_indexed();
                  skip = indexed.emplace(buffer, true).first;
                }

                // now read the hashes into the buffer to check for a possible match
                if (hashes_size > 0 && (!ReadFile(hFile, buffer, hashes_size, &numread, NULL) || numread != hashes_size))
                  break;

                if ((header[1] & 0x80) != 0 && flag_binary_without_match)
                {
                  // -I: if the indexed file to search is a binary file, then skip it
                }
                else if ((header[1] & 0x60) != 0 && !flag_decompress)
                {
                  // not -z: if the indexed file to search is an archive or is a compressed file, then skip it
                }
                else if (hashes_size > 0)
                {
                  // check if the hashed pattern has a potential match with the file's index hash
                  if (Static::index_pattern->match_hfa(reinterpret_cast<const uint8_t*>(buffer), hashes_size))
                  {
                    skip->second = false;
                    if (*flag_index == 'l')
                      fprintf(Static::errout, "INDEX LOG: %s" PATHSEPSTR "%s\n", pathname, skip->first.c_str());
                  }
                  else
                  {
                    // skip indexed file, does not match
                  }
                }
                else if ((header[1] & 0x80) == 0)
                {
                  // skip indexed file
                }
                else
                {
                  // not -I: do not skip non-indexed binary files (marked empty in index) that we need to search
                  skip->second = false;
                  if (*flag_index == 'l')
                    fprintf(Static::errout, "INDEX LOG: %s" PATHSEPSTR "%s (not indexed binary)\n", pathname, skip->first.c_str());
                }
              }

              delete[] buffer;
            }

            CloseHandle(hFile);
          }
        }

        // check if the file to search was indexed and did not match the specified pattern
        const std::map<std::string,bool>::const_iterator skip = indexed.find(cFileName);
        if (skip == indexed.end())
        {
          // a new file that was not indexed
          Stats::score_added();
          if (*flag_index == 'l')
            fprintf(Static::errout, "INDEX LOG: %s (not indexed)\n", entry_pathname.c_str());
        }
        else if (skip->second)
        {
          bool is_changed = Entry::modified_time(ffd) > index_time;

          if (is_changed)
          {
            // search the file that was changed after indexing
            Stats::score_changed();
            if (*flag_index == 'l')
              fprintf(Static::errout, "INDEX LOG: %s (changed)\n", entry_pathname.c_str());
          }
          else
          {
            // skip this indexed file that does not match the specified pattern
            Stats::score_skipped();
            type = Type::SKIP;
          }
        }
      }

      // search entry_pathname, unless searchable directory into which we should recurse
      switch (type)
      {
        case Type::DIRECTORY:
          dir_entries.emplace_back(entry_pathname, 0, info);
          break;

        case Type::OTHER:
          if (flag_sort_key == Sort::NA)
            search(entry_pathname.c_str(), Entry::UNDEFINED_COST);
          else
            file_entries.emplace_back(entry_pathname, 0, info);
          break;

        case Type::SKIP:
          break;
      }

      // stop after finding max-files matching files
      if (flag_max_files > 0 && Stats::found_parts() >= flag_max_files)
        break;

      // stop when output is blocked or search cancelled
      if (out.eof || out.cancelled())
        break;
    }
  } while (FindNextFileW(hFind, &ffd) != 0);

  FindClose(hFind);

#else

  struct dirent *dirent = NULL;
  uint64_t list = 0;

  while ((dirent = readdir(dir)) != NULL)
  {
    // search directory entries that aren't . or .. or hidden
    if (dirent->d_name[0] != '.' || (flag_hidden && dirent->d_name[1] != '\0' && dirent->d_name[1] != '.'))
    {
      // --index: do not search index files themselves, even when --hidden is specified
      if (flag_index != NULL && strcmp(dirent->d_name, ugrep_index_filename) == 0)
        continue;

      size_t len = strlen(pathname);

      if (len == 1 && pathname[0] == '.')
        entry_pathname.assign(dirent->d_name);
      else if (len > 0 && pathname[len - 1] == PATHSEPCHR)
        entry_pathname.assign(pathname).append(dirent->d_name);
      else
        entry_pathname.assign(pathname).append(PATHSEPSTR).append(dirent->d_name);

      ino_t inode;
      uint64_t info;
      Type type;

      // search entry_pathname, unless searchable directory into which we should recurse
#if defined(HAVE_STRUCT_DIRENT_D_TYPE) && defined(HAVE_STRUCT_DIRENT_D_INO)
      inode = dirent->d_ino;
      type = select(level + 1, entry_pathname.c_str(), dirent->d_name, dirent->d_type, 0, inode, info);
#else
      inode = 0;
      type = select(level + 1, entry_pathname.c_str(), dirent->d_name, DIRENT_TYPE_UNKNOWN, 0, inode, info);
#endif

      if (flag_sort_key == Sort::LIST)
        info = list++;

      // --index: search indexed files quickly, but only when necessary on demand
      if (type == Type::OTHER && Static::index_pattern != NULL)
      {
        // if we did not check for an index file yet, then check it now on demand and read it when found
        if (index_demand)
        {
          // check no more than once at most
          index_demand = false;

          // check if an index file is present and use it
          FILE *index_file = NULL;
          std::string index_filename(pathname);
          index_filename.append(PATHSEPSTR).append(ugrep_index_filename);

          if (fopenw_s(&index_file, index_filename.c_str(), "rb") == 0)
          {
            char check_magic[sizeof(ugrep_index_file_magic)];

            // if an index file is present in the directory pathname, then read and stat it
            if (fread(check_magic, sizeof(ugrep_index_file_magic), 1, index_file) != 0 &&
                memcmp(check_magic, ugrep_index_file_magic, sizeof(ugrep_index_file_magic)) == 0)
            {
              struct stat buf;

              if (fstat(fileno(index_file), &buf) == 0)
              {
                // time of indexing to check which files were modified after indexing
                uint64_t index_time = Entry::modified_time(buf);

                // allocate a buffer, not on the stack, because we are in a deeply recursive function
                char *buffer = new char[65536]; // size must be 65536
                uint8_t header[4];
                std::string index_pathname;
                bool is_changed = false;
                std::map<std::string,bool>::iterator skip = indexed.end();

                // populate indexed map
                while (true)
                {
                  if (fread(header, sizeof(header), 1, index_file) == 0)
                    break;

                  uint16_t basename_size = header[2] | (header[3] << 8);

                  if (fread(buffer, 1, basename_size, index_file) < basename_size)
                    break;

                  // make basename in buffer 0-terminated
                  buffer[basename_size] = '\0';

                  // hashes table size, zero to skip empty files and binary files when -I is specified
                  uint32_t hashes_size = 0;
                  uint8_t logsize = header[1] & 0x1f;
                  if (logsize > 0)
                    for (hashes_size = 1; logsize > 0; --logsize)
                      hashes_size <<= 1;

                  // sanity check
                  if (hashes_size > 65536)
                    break;

                  // archives have multiple entries in the index file, we keep the skip iterator to compare entries
                  if ((header[1] & 0x40) == 0)
                    skip = indexed.end();

                  // we're still looking in the same archive?
                  bool same_archive = skip != indexed.end() && skip->first == buffer;

                  // if not and archive or not the same archive entry, then add basename to the indexed map
                  if (!same_archive)
                  {
                    Stats::score_indexed();

                    skip = indexed.emplace(buffer, true).first;

                    // not --index=fast: check if the file to search was not modified after indexing
                    if (*flag_index != 'f')
                    {
                      if (len == 1 && pathname[0] == '.')
                        index_pathname.assign(buffer, basename_size);
                      else if (len > 0 && pathname[len - 1] == PATHSEPCHR)
                        index_pathname.assign(pathname).append(buffer, basename_size);
                      else
                        index_pathname.assign(pathname).append(PATHSEPSTR).append(buffer, basename_size);

                      // does the file exist and was changed after indexing?
                      is_changed = stat(index_pathname.c_str(), &buf) == 0 && Entry::modified_time(buf) > index_time;

                      if (is_changed)
                      {
                        // search the file that was changed after indexing
                        skip->second = false;
                        Stats::score_changed();
                        if (*flag_index == 'l')
                          fprintf(Static::errout, "INDEX LOG: %s (changed)\n", index_pathname.c_str());
                      }
                    }
                  }

                  // now read the hashes into the buffer to check for a possible match
                  if (hashes_size > 0 && fread(buffer, hashes_size, 1, index_file) == 0)
                    break;

                  if (!is_changed)
                  {
                    if ((header[1] & 0x80) != 0 && flag_binary_without_match)
                    {
                      // -I: if the indexed file to search is a binary file, then skip it
                    }
                    else if ((header[1] & 0x60) != 0 && !flag_decompress)
                    {
                      // not -z: if the indexed file to search is an archive or is a compressed file, then skip it
                    }
                    else if (hashes_size > 0)
                    {
                      // check if the hashed pattern has a potential match with the file's index hash
                      if (Static::index_pattern->match_hfa(reinterpret_cast<const uint8_t*>(buffer), hashes_size))
                      {
                        skip->second = false;
                        if (*flag_index == 'l')
                          fprintf(Static::errout, "INDEX LOG: %s\n", index_pathname.c_str());
                      }
                      else
                      {
                        // skip indexed file, not changed and does not match
                      }
                    }
                    else if ((header[1] & 0x80) == 0)
                    {
                      // skip indexed file, not changed and is empty
                    }
                    else
                    {
                      // not -I: do not skip non-indexed binary files (marked empty in index) that we need to search
                      skip->second = false;
                      if (*flag_index == 'l')
                        fprintf(Static::errout, "INDEX LOG: %s (not indexed binary)\n", index_pathname.c_str());
                    }
                  }
                }

                delete[] buffer;
              }
            }
          }

          if (index_file != NULL)
            fclose(index_file);
        }

        // check if the file to search was indexed and did not match the specified pattern
        const std::map<std::string,bool>::const_iterator skip = indexed.find(dirent->d_name);
        if (skip == indexed.end())
        {
          // a new file that was not indexed
          Stats::score_added();
          if (*flag_index == 'l')
            fprintf(Static::errout, "INDEX LOG: %s (not indexed)\n", entry_pathname.c_str());
        }
        else if (skip->second)
        {
          // skip this indexed file that does not match the specified pattern
          Stats::score_skipped();
          type = Type::SKIP;
        }
      }

      switch (type)
      {
        case Type::DIRECTORY:
          dir_entries.emplace_back(entry_pathname, inode, info);
          break;

        case Type::OTHER:
          if (flag_sort_key == Sort::NA)
            search(entry_pathname.c_str(), Entry::UNDEFINED_COST);
          else
            file_entries.emplace_back(entry_pathname, inode, info);
          break;

        case Type::SKIP:
          break;
      }

      // stop after finding max-files matching files
      if (flag_max_files > 0 && Stats::found_parts() >= flag_max_files)
        break;

      // stop when output is blocked or search cancelled
      if (out.eof || out.cancelled())
        break;
    }
  }

  closedir(dir);
  dir = NULL;

#endif

  // -Z and --sort=best without --match: presearch the selected files to determine edit distance cost
  if (flag_fuzzy > 0 && flag_sort_key == Sort::BEST && !flag_match)
  {
    auto entry = file_entries.begin();
    while (entry != file_entries.end())
    {
      entry->cost = compute_cost(entry->pathname.c_str());

      // if a file cannot be opened, then remove it
      if (entry->cost == Entry::UNDEFINED_COST)
        entry = file_entries.erase(entry);
      else
        ++entry;
    }
  }

  // --sort: sort the selected non-directory entries and search them
  if (flag_sort_key != Sort::NA)
  {
    if (flag_sort_key == Sort::NAME)
    {
      if (flag_sort_rev)
        std::sort(file_entries.begin(), file_entries.end(), Entry::rev_comp_by_path);
      else
        std::sort(file_entries.begin(), file_entries.end(), Entry::comp_by_path);
    }
    else if (flag_sort_key == Sort::BEST)
    {
      if (flag_sort_rev)
        std::sort(file_entries.begin(), file_entries.end(), Entry::rev_comp_by_best);
      else
        std::sort(file_entries.begin(), file_entries.end(), Entry::comp_by_best);
    }
    else
    {
      if (flag_sort_rev)
        std::sort(file_entries.begin(), file_entries.end(), Entry::rev_comp_by_info);
      else
        std::sort(file_entries.begin(), file_entries.end(), Entry::comp_by_info);
    }

    // search the select sorted non-directory entries
    for (const auto& entry : file_entries)
    {
      search(entry.pathname.c_str(), entry.cost);

      // stop after finding max-files matching files
      if (flag_max_files > 0 && Stats::found_parts() >= flag_max_files)
        break;

      // stop when output is blocked or search cancelled
      if (out.eof || out.cancelled())
        break;
    }
  }

  // --sort: sort the selected subdirectory entries
  if (flag_sort_key != Sort::NA)
  {
    if (flag_sort_key == Sort::NAME || flag_sort_key == Sort::BEST)
    {
      if (flag_sort_rev)
        std::sort(dir_entries.begin(), dir_entries.end(), Entry::rev_comp_by_path);
      else
        std::sort(dir_entries.begin(), dir_entries.end(), Entry::comp_by_path);
    }
    else
    {
      if (flag_sort_rev)
        std::sort(dir_entries.begin(), dir_entries.end(), Entry::rev_comp_by_info);
      else
        std::sort(dir_entries.begin(), dir_entries.end(), Entry::comp_by_info);
    }
  }

  // recurse into the selected subdirectories
  for (const auto& entry : dir_entries)
  {
    // stop after finding max-files matching files
    if (flag_max_files > 0 && Stats::found_parts() >= flag_max_files)
      break;

    // stop when output is blocked or search cancelled
    if (out.eof || out.cancelled())
      break;

#ifndef OS_WIN_OR_MINGW
    // -R: check if this directory was visited before
    std::pair<std::set<ino_t>::iterator,bool> vino;

    if (flag_dereference)
    {
      vino = visited.insert(entry.inode);

      // if visited before, then do not recurse on this directory again
      if (!vino.second)
        continue;
    }
#endif

    recurse(level + 1, entry.pathname.c_str());

#ifndef OS_WIN_OR_MINGW
    if (flag_dereference)
      visited.erase(vino.first);
#endif
  }

  // --ignore-files: restore all exclusions when saved by popping off all additions
  if (saved)
  {
    flag_all_exclude.resize(saved_all_exclude_size);
    flag_all_exclude_dir.resize(saved_all_exclude_dir_size);
  }
}

// -Z and --sort=best: perform a presearch to determine edit distance cost, returns MAX_COST when no match is found
uint16_t Grep::compute_cost(const char *pathname)
{
  // default cost is undefined, which erases pathname from the sorted list
  uint16_t cost = Entry::UNDEFINED_COST;

  // stop when output is blocked
  if (out.eof)
    return cost;

  try
  {
    // open (archive or compressed) file (pathname is NULL to read stdin), return on failure
    if (!open_file(pathname))
      return cost;
  }

  catch (...)
  {
    // this should never happen
    warning("exception while opening", pathname);

    return cost;
  }

  cost = Entry::MAX_COST;

  // -Z: matcher is a FuzzyMatcher for sure
  reflex::FuzzyMatcher *fuzzy_matcher = dynamic_cast<reflex::FuzzyMatcher*>(matcher);
  fuzzy_matcher->distance(static_cast<uint16_t>(flag_fuzzy));

  // search file to compute minimum cost
  do
  {
    try
    {
      if (init_read())
      {
        while (fuzzy_matcher->find())
        {
          if (fuzzy_matcher->edits() < cost)
            cost = fuzzy_matcher->edits();

          // exact match?
          if (cost == 0)
            break;
        }
      }
    }

    catch (...)
    {
      // this should never happen
      warning("exception while searching", pathname);
    }

#ifdef HAVE_LIBZ
#ifdef WITH_DECOMPRESSION_THREAD
    if (flag_decompress && cost == 0)
      zthread.cancel();
#endif
#endif

    // close file or -z: loop over next extracted archive parts, when applicable
  } while (close_file(pathname));

  return cost;
}

// search input to output the pattern matches
void Grep::search(const char *pathname, uint16_t cost)
{
  // -Zbest (or --best-match) without --match: compute cost if not yet computed by --sort=best
  if (flag_best_match && flag_fuzzy > 0 && !flag_match && !flag_quiet && (!flag_files_with_matches || flag_format != NULL) && matchers == NULL && pathname != Static::LABEL_STANDARD_INPUT)
  {
    // -Z: matcher is a FuzzyMatcher for sure
    reflex::FuzzyMatcher *fuzzy_matcher = dynamic_cast<reflex::FuzzyMatcher*>(matcher);
    fuzzy_matcher->distance(static_cast<uint16_t>(flag_fuzzy));

    // compute distance cost if not yet computed by --sort=best
    if (cost == Entry::UNDEFINED_COST)
    {
      cost = compute_cost(pathname);

      // if no match, then stop searching this file
      if (cost == Entry::UNDEFINED_COST)
        return;
    }

    // no match found?
    if (cost == Entry::MAX_COST)
    {
      if (!flag_invert_match)
        return;

      // -v: invert match when no match was found, zero cost since we don't expect any matches
      cost = 0;
    }

    // combine max distance cost (lower byte) with INS, DEL, SUB and BIN fuzzy flags (upper byte)
    fuzzy_matcher->distance((cost & 0xff) | (flag_fuzzy & 0xff00));
  }

  // stop when output is blocked
  if (out.eof)
    return;

  try
  {
    // open (archive or compressed) file (pathname is NULL to read stdin), return on failure
    if (!open_file(pathname))
      return;
  }

  catch (...)
  {
    // this should never happen
    warning("exception while opening", pathname != NULL && *pathname != '\0' ? pathname : flag_label);

    return;
  }

  // pathname is NULL when stdin is searched
  if (pathname == Static::LABEL_STANDARD_INPUT)
    pathname = flag_label;

  // the locals shared by the search branches, passed by reference to each branch function
  SearchContext ctx(pathname, cost);

  // -z: loop over extracted archive parts, when applicable
  do
  {
    if (!init_read())
      goto exit_search;

    try
    {
      // --files: reset the matching[] bitmask used in cnf_matching() for each matcher in matchers
      search_setup(ctx);

      SearchStep step;

      // dispatch to the branch for the output mode, each branch function ends as its goto labels did
      if (flag_quiet || flag_files_with_matches)
        step = search_quiet_list(ctx);
      else if (flag_count)
        step = search_count(ctx);
      else if (flag_format != NULL)
        step = search_format(ctx);
      else if (flag_only_matching && flag_before_context == 0 && flag_after_context == 0)
        step = search_only_matching(ctx);
      else if (flag_only_matching)
        step = search_only_matching_context(ctx);
      else if (flag_before_context == 0 && flag_after_context == 0 && !flag_any_line && !flag_invert_match)
        step = search_plain(ctx);
      else if (flag_before_context == 0 && flag_after_context == 0 && !flag_any_line)
        step = search_invert(ctx);
      else if (flag_any_line)
        step = search_any_line(ctx);
      else if (!flag_invert_match)
        step = search_context(ctx);
      else
        step = search_context_invert(ctx);

      // exit_search: the branch left out the epilogue
      if (step == SearchStep::EXIT)
        goto exit_search;

      // done_search: the epilogue
      search_epilogue(ctx);
    }

    catch (EXIT_SEARCH&)
    {
      // --files: cnf_matching() rejected a file, no need to search this file any further
    }

    catch (...)
    {
      // this should never happen
      warning("exception while searching", ctx.pathname);
    }

exit_search:

    // flush and release output to allow other workers to output results
    out.release();

    // close file or -z: loop over next extracted archive parts, when applicable
  } while (close_file(ctx.pathname));

  // this file or archive has a match
  if (ctx.matched)
    Stats::found_file();
}

// search branch setup: --files resets the matching bitmasks, called before the branch is chosen
void Grep::search_setup(SearchContext& ctx)
{
  // --files: reset the matching[] bitmask used in cnf_matching() for each matcher in matchers
  if (flag_files && matchers != NULL)
  {
    // hold the output
    out.hold();

    // reset the bit corresponding to each matcher in matchers
    size_t n = matchers->size();
    matching.resize(0);
    matching.resize(n);

    // reset the bit corresponding to the OR NOT terms of each matcher in matchers
    notmatching.resize(n);
    size_t j = 0;
    for (auto& i : *matchers)
    {
      notmatching[j].resize(0);
      notmatching[j].resize(i.size() > 0 ? i.size() - 1 : 0);
      ++j;
    }
  }
}

// search branch -q, -l, -L (the branch bodies keep the indentation they had inside search(), so the diff reads as a move)
Grep::SearchStep Grep::search_quiet_list(SearchContext& ctx)
{
  const char *& pathname = ctx.pathname;

      {
        // option -q, -l, or -L

        // --format with open/close or --max-files: we must out.acquire() lock before Stats::found_part()
        bool acquire = (flag_format != NULL && (flag_format_open != NULL || flag_format_close != NULL)) || (!flag_quiet && flag_max_files > 0);

        // skip accurate line number counting, only count matching lines
        if (!flag_multiline && flag_max_line == 0 && flag_stats == NULL)
          matcher->lineno_skip(true);

        while (matcher->find())
        {
          size_t current_lineno = matcher->lineno();

          // --range: max line exceeded?
          if (flag_max_line > 0 && current_lineno > flag_max_line)
            break;

          if (matchers != NULL)
          {
            const char *eol = matcher->eol(true); // warning: call eol() before bol() and end()
            const char *bol = matcher->bol();

            // check CNF AND/OR/NOT matching
            if (!cnf_matching(bol, eol, acquire) || out.holding())
              continue;
          }

          if (flag_min_count == 0 || flag_ungroup || flag_only_matching || lineno != current_lineno)
            ++matches;

          // --min-count: require at least min-count matches, otherwise stop searching
          if (flag_min_count == 0 || matches >= flag_min_count)
            break;

          lineno = current_lineno;
        }

        // --min-count: require at least min-count matches
        if (flag_min_count > 0 && matches < flag_min_count)
          matches = 0;

        // --files: if we are still holding the output and CNF is finally satisfyable then a match was made
        if (flag_files && matchers != NULL)
        {
          if (!cnf_satisfied(acquire))
            goto exit_search;

          matches = 1;
        }

        // -v: invert
        if (flag_invert_match)
          matches = (matches == 0);

        if (matches > 0)
        {
          // --format with open/close or --max-files: we must out.acquire() lock before Stats::found_part()
          if (acquire)
            out.acquire();

          if (!flag_files || matchers == NULL)
          {
            // --max-files: max reached?
            if (!Stats::found_part())
              goto exit_search;
          }

          // -l or -L
          if (flag_files_with_matches)
          {
            if (flag_format != NULL)
            {
              if (flag_format_open != NULL)
                out.format(flag_format_open, pathname, partname, Stats::found_parts(), NULL, matcher, heading, false, Stats::found_parts() > 1);
              out.format(flag_format, pathname, partname, 1, NULL, matcher, heading, false, false);
              if (flag_format_close != NULL)
                out.format(flag_format_close, pathname, partname, Stats::found_parts(), NULL, matcher, heading, false, Stats::found_parts() > 1);
            }
            else
            {
              out.header(pathname, partname);

              if (flag_null)
                out.chr('\0');
              else
                out.nl();
            }
          }
        }
      }

  return SearchStep::DONE;

exit_search:
  return SearchStep::EXIT;
}

// search branch -c
Grep::SearchStep Grep::search_count(SearchContext& ctx)
{
  const char *& pathname = ctx.pathname;

      {
        // option -c

        // --format with open/close or --max-files: we must out.acquire() lock before Stats::found_part()
        bool acquire = (flag_format != NULL && (flag_format_open != NULL || flag_format_close != NULL)) || flag_max_files > 0;

        // not --match to match all or not -v
        if (!flag_match || !flag_invert_match)
        {
          if (flag_ungroup || flag_only_matching)
          {
            // -co or -cu: count the number of patterns matched

            // skip accurate line number counting, only count matching lines
            if (!flag_multiline && flag_max_line == 0 && flag_stats == NULL)
              matcher->lineno_skip(true);

            while (matcher->find())
            {
              // --range: max line exceeded?
              if (flag_max_line > 0 && matcher->lineno() > flag_max_line)
                break;

              if (matchers != NULL)
              {
                const char *eol = matcher->eol(true); // warning: call eol() before bol() and end()
                const char *bol = matcher->bol();

                // check CNF AND/OR/NOT matching, with --files acquire lock before Stats::found_part()
                if (!cnf_matching(bol, eol, acquire))
                  continue;
              }

              ++matches;

              // -m: max number of matches reached?
              if (flag_max_count > 0 && matches >= flag_max_count)
                break;
            }
          }
          else if (!flag_multiline && !flag_invert_match && flag_max_line == 0 && matchers == NULL && flag_stats == NULL)
          {
            // -c without -o/-u/-v/-K/-%: count the number of matching lines

            // skip accurate line number counting, only count matching lines
            matcher->lineno_skip(true);

            while (matcher->find())
            {
              ++matches;

              // -m: max number of matches reached?
              if (flag_max_count > 0 && matches >= flag_max_count)
                break;

              // if the match does not span more than one line, then skip to end of the line (we count matching lines)
              if (!matcher->at_bol())
                matcher->skip('\n');
            }
          }
          else
          {
            // -c without -o/-u: count the number of matching lines

            while (matcher->find())
            {
              size_t current_lineno = matcher->lineno();

              if (lineno != current_lineno)
              {
                // --range: max line exceeded?
                if (flag_max_line > 0 && current_lineno > flag_max_line)
                  break;

                if (matchers != NULL)
                {
                  const char *eol = matcher->eol(true); // warning: call eol() before bol() and end()
                  const char *bol = matcher->bol();

                  // check CNF AND/OR/NOT matching, with --files acquire lock before Stats::found_part()
                  if (!cnf_matching(bol, eol, acquire))
                    continue;
                }

                ++matches;

                // -m: max number of matches reached?
                if (flag_max_count > 0 && matches >= flag_max_count)
                  break;

                lineno = current_lineno;

                // if the match does not span more than one line, then skip to end of the line (we count matching lines)
                if (!flag_multiline && !matcher->at_bol())
                  matcher->skip('\n');
              }
            }

            // -c with -v: count non-matching lines
            if (flag_invert_match)
            {
              matches = matcher->lineno() - matches;
              if (matches > 0)
                --matches;
            }
          }
        }

        // --min-count: require at least min-count matches
        if (flag_min_count > 0 && matches < flag_min_count)
          goto exit_search;

        // --files: if we are still holding the output and CNF is not satisfyable then no global matches were made
        if (flag_files && matchers != NULL)
        {
          if (!cnf_satisfied(acquire))
            goto exit_search; // we cannot report 0 matches and ensure accurate output
        }
        else
        {
          // --format with open/close or --max-files: we must out.acquire() lock before Stats::found_part()
          if (acquire)
            out.acquire();

          // --max-files: max reached?
          // unfortunately, allowing 'acquire' below produces "x matching + y in archives"
          // but without this we cannot produce correct format-open and format-close outputs
          if (matches > 0 || acquire)
            if (!Stats::found_part())
              goto exit_search;
        }

        if (flag_format != NULL)
        {
          if (flag_format_open != NULL)
            out.format(flag_format_open, pathname, partname, Stats::found_parts(), NULL, matcher, heading, false, Stats::found_parts() > 1);
          out.format(flag_format, pathname, partname, matches, NULL, matcher, heading, false, false);
          if (flag_format_close != NULL)
            out.format(flag_format_close, pathname, partname, Stats::found_parts(), NULL, matcher, heading, false, Stats::found_parts() > 1);
        }
        else
        {
          if (flag_with_filename || !partname.empty())
          {
            out.header(pathname, partname);

            if (flag_null)
            {
              out.chr('\0');
            }
            else
            {
              out.str(color_se);
              out.str(flag_separator);
              out.str(color_off);
            }
          }
          out.num(matches);
          out.nl();
        }
      }

  return SearchStep::DONE;

exit_search:
  return SearchStep::EXIT;
}

// search branch --format
Grep::SearchStep Grep::search_format(SearchContext& ctx)
{
  const char *& pathname = ctx.pathname;

      {
        // option --format

        // --format with open/close: we must out.acquire() lock before Stats::found_part()
        bool acquire = flag_format_open != NULL || flag_format_close != NULL;

        if (flag_invert_match)
        {
          // construct event handler functor with captured *this and some of the locals
          FormatInvertMatchGrepHandler invert_match_handler(*this, pathname);

          // register the event handler to output non-matching lines, matcher resets to NULL when a new input source is assigned
          matcher->set_handler(&invert_match_handler);

          // to get the context from the invert_match handler explicitly
          reflex::AbstractMatcher::Context context;

          while (matcher->find())
          {
            size_t current_lineno = matcher->lineno();

            if (lineno != current_lineno)
            {
              if (matchers != NULL)
              {
                const char *eol = matcher->eol(true); // warning: call eol() before bol() and end()
                const char *bol = matcher->bol();

                // check CNF AND/OR/NOT matching
                if (!cnf_matching(bol, eol))
                  continue;
              }

              // get the lines before the matched line
              context = matcher->before();

              // output non-matching lines up to this line
              if (context.len > 0)
                invert_match_handler(*matcher, context.buf, context.len, context.num);

              // --range: max line exceeded?
              if (flag_max_line > 0 && current_lineno > flag_max_line)
                goto done_search;

              // --max-files: max reached?
              if (stop)
                goto exit_search;

              // -m: max number of matches reached?
              if (flag_max_count > 0 && matches >= flag_max_count)
                goto done_search;

              // output blocked?
              if (out.eof)
                goto exit_search;
            }

            lineno = current_lineno + matcher->lines() - 1;
          }

          // get the remaining context after the last match to output
          context = matcher->after();

          if (context.len > 0)
            invert_match_handler(*matcher, context.buf, context.len, context.num);
        }
        else
        {
          // counter for format field %m matches
          size_t format_matching = 0;

          while (matcher->find())
          {
            size_t current_lineno = matcher->lineno();

            // --range: max line exceeded?
            if (flag_max_line > 0 && current_lineno > flag_max_line)
              break;

            if (matchers != NULL)
            {
              const char *eol = matcher->eol(true); // warning: call eol() before bol() and end()
              const char *bol = matcher->bol();

              // check CNF AND/OR/NOT matching
              if (!cnf_matching(bol, eol, acquire))
                continue;
            }

            if (current_lineno != lineno || flag_ungroup)
            {
              ++matches;

              // --min-count: require at least min-count matches
              if (flag_min_count > 0 && matches < flag_min_count)
              {
                lineno = current_lineno;
                continue;
              }

              // output --format-open=FORMAT for the first (or --min-count) match found
              if (matches == flag_min_count + (flag_min_count == 0))
              {
                if (flag_files && matchers != NULL)
                {
                  // --format-open: we must acquire lock early before Stats::found_part()
                  if (acquire && out.holding())
                  {
                    out.acquire();

                    // --max-files: max reached?
                    if (!Stats::found_part())
                      goto exit_search;
                  }
                }
                else
                {
                  // --format-open: we must acquire lock early before Stats::found_part()
                  if (acquire)
                    out.acquire();

                  // --max-files: max reached?
                  if (!Stats::found_part())
                    goto exit_search;
                }

                if (flag_format_open != NULL)
                {
                  // output --format-open=FORMAT
                  out.format(flag_format_open, pathname, partname, Stats::found_parts(), NULL, matcher, heading, false, Stats::found_parts() > 1);

                  // --files: undo files count
                  if (flag_files && matchers != NULL && out.holding())
                    Stats::undo_found_part();
                }
              }
            }

            // output --format=FORMAT
            // if the match does not span more than one line, then skip to end of the line
            if (!out.format(flag_format, pathname, partname, matches, &format_matching, matcher, heading, matches > 1 || current_lineno == lineno, matches > 1))
              if (!flag_multiline && !matcher->at_bol())
                if (!matcher->skip('\n'))
                  break;

            // --max-count: max number of matches reached?
            if (flag_max_count > 0 && matches >= flag_max_count)
              break;

            out.check_flush();

            lineno = current_lineno;
          }
        }

        // --min-count: require at least min-count matches
        if (flag_min_count > 0 && matches < flag_min_count)
          goto exit_search;

        // --files: if we are still holding the output and CNF is not satisfyable then no global matches were made
        if (flag_files && matchers != NULL)
          if (!cnf_satisfied(true))
            goto exit_search;

        // output --format-close=FORMAT
        if (matches > 0 && flag_format_close != NULL)
          out.format(flag_format_close, pathname, partname, Stats::found_parts(), NULL, matcher, heading, false, Stats::found_parts() > 1);
      }

done_search:
  return SearchStep::DONE;

exit_search:
  return SearchStep::EXIT;
}

// search branch -o without -ABC
Grep::SearchStep Grep::search_only_matching(SearchContext& ctx)
{
  const char *& pathname = ctx.pathname;

      {
        // option -o without -ABC

        // counter for replace field %m matches
        size_t replace_matching = 0;
        bool nl = false;

        // skip accurate line number counting, only count matching lines
        if (!flag_line_number && !flag_multiline && flag_max_line == 0 && flag_stats == NULL)
          matcher->lineno_skip(true);

        while (matcher->find())
        {
          const char *begin = matcher->begin();
          size_t size = matcher->size();

          binary = check_binary(begin, size);

          if (hex && !binary)
          {
            out.dump.done();
          }
          else if (!hex && binary && nl)
          {
            out.nl();
            nl = false;
          }

          if (nl)
          {
            out.nl();
            nl = false;
          }

          size_t current_lineno = matcher->lineno();

          // --range: max line exceeded?
          if (flag_max_line > 0 && current_lineno > flag_max_line)
            break;

          // output blocked?
          if (out.eof)
            goto exit_search;

          if (matchers != NULL)
          {
            const char *eol = matcher->eol(true); // warning: call eol() before bol() and end()
            const char *bol = matcher->bol();

            // check CNF AND/OR/NOT matching
            if (!cnf_matching(bol, eol))
              continue;
          }

          if (lineno != current_lineno || flag_ungroup)
          {
            lineno = current_lineno;

            ++matches;

            // --min-count: require at least min-count matches
            if (flag_min_count > 0 && matches < flag_min_count)
              continue;

            // --max-count: max number of matches reached?
            if (flag_max_count > 0 && matches > flag_max_count)
              break;

            if (matches == flag_min_count + (flag_min_count == 0) && (!flag_files || matchers == NULL))
            {
              // --max-files: max reached?
              if (!Stats::found_part())
                goto exit_search;
            }
          }

          if (binfile || (binary && !flag_hex && !flag_with_hex))
          {
            if (flag_binary_without_match)
            {
              matches = 0;
            }
            else
            {
              out.binary_file_matches(pathname, partname);
              matches = 1;
            }

            if (flag_files && matchers != NULL && out.holding())
              continue;

            goto done_search;
          }

          if (!flag_no_header)
            out.header(pathname, partname, heading, current_lineno, matcher, matcher->first(), flag_separator, binary);

          // --min-count: require at least min-count matches
          if (flag_min_count > 0 && matches < flag_min_count)
            continue;

          hex = binary;

          if (binary)
          {
            if (flag_hex || flag_with_hex)
            {
              out.dump.next(matcher->first());
              out.dump.hex(Output::Dump::HEX_MATCH, matcher->first(), begin, size);
            }
            else
            {
              if (flag_binary_without_match)
              {
                matches = 0;
              }
              else
              {
                out.binary_file_matches(pathname, partname);
                matches = 1;
              }

              if (flag_files && matchers != NULL && out.holding())
                continue;

              goto done_search;
            }

            lineno += matcher->lines() - 1;
          }
          else if (flag_replace != NULL)
          {
            // output --replace=FORMAT
            out.str(match_ms);
            out.format(flag_replace, pathname, partname, matches, &replace_matching, matcher, heading, matches > 1, matches > 1);
            out.str(match_off);
          }
          else
          {
            if (flag_multiline)
            {
              // echo multi-line matches line-by-line
              const char *from = begin;
              const char *to;

              while ((to = static_cast<const char*>(memchr(from, '\n', size - (from - begin)))) != NULL)
              {
                out.str(match_ms);
                out.str(from, to - from);
                out.str(match_off);
                out.chr('\n');

                out.header(pathname, partname, heading, ++lineno, NULL, matcher->first() + (to - begin) + 1, flag_separator_bar, false);

                from = to + 1;
              }

              size -= from - begin;
              begin = from;
            }

            if (size > 0)
            {
              bool lf_only = begin[size - 1] == '\n';
              size -= lf_only;
              if (size > 0)
              {
                out.str(match_ms);
                out.str(begin, size);
                out.str(match_off);
              }
              out.nl(lf_only);
            }
            else
            {
              nl = true;
            }
          }
        }

        // --min-count: require at least min-count matches
        if (flag_min_count > 0 && matches < flag_min_count)
          goto exit_search;

        if (nl)
          out.nl();

        if (hex)
          out.dump.done();
      }

done_search:
  return SearchStep::DONE;

exit_search:
  return SearchStep::EXIT;
}

// search branch -o with -ABC
Grep::SearchStep Grep::search_only_matching_context(SearchContext& ctx)
{
  const char *& pathname = ctx.pathname;

      {
        // option -o with -ABC specified

        size_t count = 0;
        size_t width = flag_before_context + flag_after_context;
        bool nl = false;

        // skip accurate line number counting, only count matching lines
        if (!flag_line_number && !flag_multiline && flag_max_line == 0 && flag_stats == NULL)
          matcher->lineno_skip(true);

        // construct event handler functor with captured *this and some of the locals to save the restline when the buffer shifts
        OnlyMatchingContextGrepHandler handler(*this, pathname);

        // register the event handler to update restline on buffer shift, matcher resets to NULL when a new input source is assigned
        matcher->set_handler(&handler);

        // the rest of the matching line
        restline_data = NULL;
        restline_size = 0;
        restline_last = 0;

        while (matcher->find())
        {
          const char *begin = matcher->begin();
          size_t size = matcher->size();

          binary = check_binary(begin, size);

          if (hex && !binary)
          {
            out.dump.done();
          }
          else if (!hex && binary && nl)
          {
            out.nl();
            nl = false;
          }

          if (nl)
          {
            if (restline_size > 0)
            {
              out.str(color_cx);
              if (utf8nlen(restline_data, restline_size) > width + 3)
              {
                out.utf8strn(restline_data, restline_size, width);
                out.str(color_off);
                out.str(color_se);
                out.str("...", 3);
              }
              else
              {
                out.str(restline_data, restline_size);
              }
              out.str(color_off);
              restline_size = 0;
            }

            if (count > 0)
            {
              out.str(color_se);
              out.str("...", 3);
              out.str(color_off);
              out.str(color_cn);
              out.str("[+", 2);
              out.num(count);
              out.str(" more]", 6);
              out.str(color_off);
            }

            out.nl();

            count = 0;
            width = flag_before_context + flag_after_context;
            nl = false;
          }

          size_t current_lineno = matcher->lineno();

          // --range: max line exceeded?
          if (flag_max_line > 0 && current_lineno > flag_max_line)
            break;

          // output blocked?
          if (out.eof)
            goto exit_search;

          if (matchers != NULL)
          {
            const char *eol = matcher->eol(true); // warning: call eol() before bol() and end()
            const char *bol = matcher->bol();

            // check CNF AND/OR/NOT matching
            if (!cnf_matching(bol, eol))
              continue;
          }

          if (lineno != current_lineno || flag_ungroup)
          {
            lineno = current_lineno;

            ++matches;

            // --min-count: require at least min-count matches
            if (flag_min_count > 0 && matches < flag_min_count)
              continue;

            // --max-count: max number of matches reached?
            if (flag_max_count > 0 && matches > flag_max_count)
              break;

            if (matches == flag_min_count + (flag_min_count == 0) && (!flag_files || matchers == NULL))
            {
              // --max-files: max reached?
              if (!Stats::found_part())
                goto exit_search;
            }
          }

          if (binfile || (binary && !flag_hex && !flag_with_hex))
          {
            if (flag_binary_without_match)
            {
              matches = 0;
            }
            else
            {
              out.binary_file_matches(pathname, partname);
              matches = 1;
            }

            if (flag_files && matchers != NULL && out.holding())
              continue;

            goto done_search;
          }

          size_t first = matcher->first();
          size_t last = matcher->last();

          if (!flag_no_header)
            out.header(pathname, partname, heading, current_lineno, matcher, first, flag_separator, binary);

          // --min-count: require at least min-count matches
          if (flag_min_count > 0 && matches < flag_min_count)
            continue;

          hex = binary;

          if (binary)
          {
            if (flag_hex || flag_with_hex)
            {
              out.dump.next(first);
              out.dump.hex(Output::Dump::HEX_MATCH, first, begin, size);
            }
            else
            {
              if (flag_binary_without_match)
              {
                matches = 0;
              }
              else
              {
                out.binary_file_matches(pathname, partname);
                matches = 1;
              }

              if (flag_files && matchers != NULL && out.holding())
                continue;

              goto done_search;
            }

            lineno += matcher->lines() - 1;
          }
          else
          {
            if (restline_size > 0)
            {
              if (first - restline_last < restline_size)
                restline_size = first - restline_last;

              size_t after = utf8nlen(restline_data, restline_size);

              if (after > width)
                after = width;

              width -= after;

              if (after > 0)
              {
                out.str(color_cx);
                out.utf8strn(restline_data, restline_size, after);
                out.str(color_off);
              }

              restline_size = 0;
            }

            if (width == 0)
            {
              ++count;
            }
            else
            {
              while (true)
              {
                // check for additional lines in the match output
                size_t rest = 0;
                const char *to = static_cast<const char*>(memchr(begin, '\n', size));
                if (to != NULL)
                {
                  rest = size;
                  size = to - begin;
                  rest -= size;
                }

                // length of the match is the number of UTF-8-encoded Unicode characters
                size_t length = utf8nlen(begin, size);
                size_t fit_length = length;

                if (fit_length > width)
                {
                  if (fit_length > width + 4)
                    fit_length = width;
                  width = 0;
                }
                else
                {
                  width -= fit_length;
                }

                if (!nl)
                {
                  const char *bol = matcher->bol();
                  size_t border = matcher->border();
                  size_t margin = utf8nlen(bol, border);
                  size_t before = flag_before_context * fit_length / (flag_before_context + flag_after_context);

                  if (before < flag_before_context)
                    before = flag_before_context - before;
                  else
                    before = 0;

                  if (margin > before)
                  {
                    out.str(color_se);
                    out.str("...", 3);
                    out.str(color_off);
                    out.str(color_cx);
                    out.utf8strn(utf8skipn(bol, border, margin - before), border, before);
                    out.str(color_off);
                    width -= before;
                  }
                  else
                  {
                    if (border > 0)
                    {
                      out.str(color_cx);
                      out.str(bol, border);
                      out.str(color_off);
                    }
                    if (margin >= 3)
                      width -= margin - 3;
                    else
                      width += 3 - margin;
                  }
                }

                out.str(match_ms);
                if (fit_length == length)
                {
                  out.str(begin, size);
                }
                else
                {
                  out.utf8strn(begin, size, fit_length);
                  out.str("[+", 2);
                  out.num(length - fit_length);
                  out.chr(']');
                }
                out.str(match_off);

                // no more additional lines to output?
                if (to == NULL)
                  break;

                out.nl();

                // output header and repeat to output a multiline match after previous newline
                if (!flag_no_header)
                  out.header(pathname, partname, heading, ++lineno, matcher, first + (to - begin), flag_separator_bar, false);

                begin = to + 1;
                size = rest - 1;
                width = flag_before_context + flag_after_context;

                nl = true;
              }

              const char *eol = matcher->eol(); // warning: call eol() before bol() and end()
              const char *end = matcher->end();

              restline_data = end;
              restline_size = eol - end;
              restline_last = last;
            }

            nl = true;
          }
        }

        // --min-count: require at least min-count matches
        if (flag_min_count > 0 && matches < flag_min_count)
          goto exit_search;

        if (nl)
        {
          if (restline_size > 0)
          {
            out.str(color_cx);
            if (utf8nlen(restline_data, restline_size) > width + 3)
            {
              out.utf8strn(restline_data, restline_size, width);
              out.str(color_off);
              out.str(color_se);
              out.str("...", 3);
            }
            else
            {
              out.str(restline_data, restline_size);
            }
            out.str(color_off);
            restline_size = 0;
          }

          if (count > 0)
          {
            out.str(color_se);
            out.str("...", 3);
            out.str(color_off);
            out.str(color_cn);
            out.str("[+", 2);
            out.num(count);
            out.str(" more]", 6);
            out.str(color_off);
          }

          out.nl();
        }

        if (hex)
          out.dump.done();
      }

done_search:
  return SearchStep::DONE;

exit_search:
  return SearchStep::EXIT;
}

// search branch plain matches and --hex
Grep::SearchStep Grep::search_plain(SearchContext& ctx)
{
  const char *& pathname = ctx.pathname;

      {
        // options -ABC, -y, -v are not specified, --hexdump context is supported (including with options -ABC)

        // counter for replace field %m matches
        size_t replace_matching = 0;
        bool colorize = flag_color != NULL || flag_replace != NULL || flag_tag != NULL;

        // skip accurate line number counting, only count matching lines
        if (!flag_line_number && !flag_multiline && flag_max_line == 0 && flag_stats == NULL)
          matcher->lineno_skip(true);

        // construct event handler functor with captured *this and some of the locals to output or save restline when the buffer shifts
        GrepHandler handler(*this, pathname);

        // register the event handler to update restline on buffer shift, matcher resets to NULL when a new input source is assigned
        matcher->set_handler(&handler);

        // --hexdump: keep -B NUM+1 before context times hex column bytes in the buffer when shifting
        if (flag_hex_before > 0)
          matcher->set_reserve(flag_hex_before * flag_hex_columns);

        // the rest of the matching line
        restline_data = NULL;
        restline_size = 0;
        restline_last = 0;

        while (matcher->find())
        {
          size_t current_lineno = matcher->lineno();

          if (lineno != current_lineno || flag_ungroup)
          {
            output_restline(false, flag_hex_after > 0);

            // --range: max line exceeded?
            if (flag_max_line > 0 && current_lineno > flag_max_line)
              break;

            // --max-count: max number of matches reached?
            if (flag_max_count > 0 && matches >= flag_max_count)
              break;

            // output blocked?
            if (out.eof)
              goto exit_search;

            const char *eol = matcher->eol(true); // warning: call eol() before bol() and end()
            const char *bol = matcher->bol();

            // check CNF AND/OR/NOT matching
            if (matchers != NULL && !cnf_matching(bol, eol))
              continue;

            ++matches;

            // --min-count: require at least min-count matches
            if (flag_min_count > 0 && matches < flag_min_count)
            {
              lineno = current_lineno;
              continue;
            }

            if (matches == flag_min_count + (flag_min_count == 0) && (!flag_files || matchers == NULL))
            {
              // --max-files: max reached?
              if (!Stats::found_part())
                goto exit_search;
            }

            binary = check_binary(bol, eol - bol);

            // --hexdump with ABC context: if binary then show hex context
            if (binary && (flag_hex_after > 0 || flag_hex_before > 0))
            {
              const char *aft = matcher->aft((flag_hex_after + flag_hex_before) * flag_hex_columns);
              if (aft > eol)
              {
                eol = aft;
                bol = matcher->bol(); // warning: bol() position may have shifted with aft()
              }
            }

            if (binfile || (binary && !flag_hex && !flag_with_hex))
            {
              if (flag_binary_without_match)
              {
                matches = 0;
              }
              else
              {
                out.binary_file_matches(pathname, partname);
                matches = 1;
              }

              if (flag_files && matchers != NULL && out.holding())
                continue;

              goto done_search;
            }

            size_t border = matcher->border();
            size_t first = matcher->first();
            size_t last = matcher->last();
            const char *begin = matcher->begin();
            const char *end = matcher->end();
            size_t size = matcher->size();

            if (hex && !binary)
              out.dump.done();

            if (!flag_no_header)
              out.header(pathname, partname, heading, current_lineno, matcher, first, flag_separator, binary);

            hex = binary;

            lineno = current_lineno;

            if (binary)
            {
              // --hexdump with ABC context: if binary then show hex context
              if (flag_hex_before > 0)
              {
                const char *bef = matcher->bef(flag_hex_before * flag_hex_columns);
                if (bef < bol)
                {
                  bol =  bef;
                  border = begin - bol;
                }

                size_t left = 0;
                if (restline_last + restline_size < first)
                {
                  left = flag_hex_before * flag_hex_columns + (first % flag_hex_columns) - flag_hex_columns;
                  if (restline_last + restline_size + left > first)
                    left = first - (restline_last + restline_size);
                }

                if (begin > bol + left)
                {
                  bol = begin - left;
                  border = left;
                }
              }

              out.dump.hex(Output::Dump::HEX_LINE, first - border, bol, border);
              out.dump.hex(Output::Dump::HEX_MATCH, first, begin, size);

              if (flag_ungroup)
              {
                // --hexdump with ABC context: if binary then show hex context
                if (flag_hex_after > 0)
                {
                  size_t right = flag_hex_after * flag_hex_columns - ((last - 1) % flag_hex_columns) - 1;
                  if (end + right < eol)
                    eol = end + right;
                }

                out.dump.hex(Output::Dump::HEX_LINE, last, end, eol - end);
                out.dump.done();
              }
              else
              {
                restline_data = end;
                restline_size = eol - end;
                restline_last = last;
              }

              lineno += matcher->lines() - 1;
            }
            else
            {
              if (border > 0)
              {
                out.str(color_sl);
                out.str(bol, border);
                out.str(color_off);
              }

              if (flag_replace != NULL)
              {
                // output --replace=FORMAT
                out.str(match_ms);
                out.format(flag_replace, pathname, partname, matches, &replace_matching, matcher, heading, matches > 1, matches > 1);
                out.str(match_off);

                lineno += matcher->lines() - 1;
              }
              else
              {
                if (flag_multiline)
                {
                  // echo multi-line matches line-by-line
                  const char *from = begin;
                  const char *to;

                  while ((to = static_cast<const char*>(memchr(from, '\n', size - (from - begin)))) != NULL)
                  {
                    out.str(match_ms);
                    out.str(from, to - from);
                    out.str(match_off);
                    out.chr('\n');

                    out.header(pathname, partname, heading, ++lineno, NULL, first + (to - begin) + 1, flag_separator_bar, false);

                    from = to + 1;
                  }

                  size -= from - begin;
                  begin = from;
                }

                out.str(match_ms);
                out.str(begin, size);
                out.str(match_off);
              }

              if (flag_ungroup)
              {
                if (eol > end)
                {
                  bool lf_only = end[eol - end - 1] == '\n';
                  eol -= lf_only;
                  if (eol > end)
                  {
                    out.str(color_sl);
                    out.str(end, eol - end);
                    out.str(color_off);
                  }
                  out.nl(lf_only);
                }
                else if (matcher->hit_end())
                {
                  out.nl();
                }
                else
                {
                  out.check_flush();
                }
              }
              else
              {
                restline_data = end;
                restline_size = eol - end;
                restline_last = last;
              }
            }

            // no -u, no --hexdump and no colors: if the match does not span more than one line, then skip to end of the line
            if (!flag_ungroup && !colorize && !(binary && (flag_hex_after > 0 || flag_hex_before > 0)))
              if (!flag_multiline && !matcher->at_bol())
                if (!matcher->skip('\n'))
                  break;
          }
          else
          {
            // --min-count: require at least min-count matches
            if (flag_min_count > 0 && matches < flag_min_count)
              continue;

            size_t size = matcher->size();

            if (size > 0)
            {
              size_t lines = matcher->lines();

              if (lines > 1 || colorize)
              {
                size_t first = matcher->first();
                size_t last = matcher->last();
                const char *begin = matcher->begin();

                if (binary)
                {
                  if (restline_data != NULL)
                  {
                    if (restline_last + restline_size > first)
                      restline_size = first - restline_last;

                    // --hexdump with ABC context: if binary then show hex context
                    if (flag_hex_after > 0)
                    {
                      size_t right = flag_hex_after * flag_hex_columns - ((restline_last - 1) % flag_hex_columns) - 1;
                      if (right < restline_size)
                      {
                        out.dump.hex(Output::Dump::HEX_LINE, restline_last, restline_data, right);

                        restline_data += right;
                        restline_size -= right;
                        restline_last += right;
                      }
                      else
                      {
                        out.dump.hex(Output::Dump::HEX_LINE, restline_last, restline_data, restline_size);

                        restline_data = NULL;
                      }
                    }

                    if (restline_data != NULL)
                    {
                      // --hexdump with ABC context: if binary then show hex context
                      if (flag_hex_before > 0)
                      {
                        size_t left = flag_hex_before * flag_hex_columns + (first % flag_hex_columns) - flag_hex_columns;
                        if (left < restline_size)
                        {
                          if (!flag_no_header)
                            out.header(pathname, partname, heading, current_lineno, matcher, first, flag_separator_bar, binary);

                          restline_data += restline_size - left;
                          restline_last += restline_size - left;
                          restline_size = left;
                        }
                      }

                      out.dump.hex(Output::Dump::HEX_LINE, restline_last, restline_data, restline_size);
                    }

                    restline_data = NULL;
                  }

                  out.dump.hex(Output::Dump::HEX_MATCH, first, begin, size);
                }
                else
                {
                  if (restline_data != NULL)
                  {
                    if (restline_last + restline_size > first)
                      restline_size = first - restline_last;
                    out.str(color_sl);
                    out.str(restline_data, restline_size);
                    out.str(color_off);
                  }

                  if (flag_replace != NULL)
                  {
                    // output --replace=FORMAT
                    out.str(match_ms);
                    out.format(flag_replace, pathname, partname, matches, &replace_matching, matcher, heading, matches > 1, matches > 1);
                    out.str(match_off);
                  }
                  else
                  {
                    if (lines > 1)
                    {
                      // echo multi-line matches line-by-line

                      const char *from = begin;
                      const char *to;
                      size_t num = 1;

                      while ((to = static_cast<const char*>(memchr(from, '\n', size - (from - begin)))) != NULL)
                      {
                        out.str(match_ms);
                        out.str(from, to - from);
                        out.str(match_off);
                        out.chr('\n');

                        out.header(pathname, partname, heading, lineno + num, NULL, first + (to - begin) + 1, flag_separator_bar, false);

                        from = to + 1;
                        ++num;
                      }

                      size -= from - begin;
                      begin = from;
                    }

                    out.str(match_ms);
                    out.str(begin, size);
                    out.str(match_off);
                  }
                }

                const char *eol = matcher->eol(true); // warning: call eol() before end()
                const char *end = matcher->end();

                if (lines == 1)
                {
                  // --hexdump with ABC context: if binary then show hex context
                  if (binary && (flag_hex_after > 0 || flag_hex_before > 0))
                  {
                    const char *aft = matcher->aft((flag_hex_after + flag_hex_before) * flag_hex_columns);
                    if (aft > eol)
                    {
                      eol = aft;
                      end = matcher->end(); // warning: end() position may have shifted with aft()
                    }
                  }

                  restline_data = end;
                  restline_size = eol - end;
                  restline_last = last;
                }
                else
                {
                  binary = check_binary(end, eol - end);

                  // --hexdump with ABC context: if binary then show hex context
                  if (binary && (flag_hex_after > 0 || flag_hex_before > 0))
                  {
                    const char *aft = matcher->aft((flag_hex_after + flag_hex_before) * flag_hex_columns);
                    if (aft > eol)
                    {
                      eol = aft;
                      end = matcher->end(); // warning: end() position may have shifted with aft()
                    }
                  }

                  if (hex && !binary)
                    out.dump.done();
                  else if (!hex && binary)
                    out.nl();

                  if (hex != binary && !flag_no_header)
                    out.header(pathname, partname, heading, lineno + lines - 1, matcher, last, flag_separator, binary);

                  hex = binary;

                  if (flag_ungroup)
                  {
                    if (binary)
                    {
                      // --hexdump with ABC context: if binary then show hex context
                      if (flag_hex_after > 0)
                      {
                        size_t right = flag_hex_after * flag_hex_columns - ((last - 1) % flag_hex_columns) - 1;
                        if (end + right < eol)
                          eol = end + right;
                      }

                      out.dump.hex(Output::Dump::HEX_LINE, last, end, eol - end);
                      out.dump.done();
                    }
                    else
                    {
                      if (eol > end)
                      {
                        bool lf_only = end[eol - end - 1] == '\n';
                        eol -= lf_only;
                        if (eol > end)
                        {
                          out.str(color_sl);
                          out.str(end, eol - end);
                          out.str(color_off);
                        }
                        out.nl(lf_only);
                      }
                      else if (matcher->hit_end())
                      {
                        out.nl();
                      }
                      else
                      {
                        out.check_flush();
                      }
                    }
                  }
                  else
                  {
                    restline_data = end;
                    restline_size = eol - end;
                    restline_last = last;
                  }

                  lineno += lines - 1;
                }
              }
            }
          }
        }

        // --min-count: require at least min-count matches
        if (flag_min_count > 0 && matches < flag_min_count)
          goto exit_search;

        output_restline(false, flag_hex_after > 0);

        if (binary)
          out.dump.done();
      }

done_search:
  return SearchStep::DONE;

exit_search:
  return SearchStep::EXIT;
}

// search branch -v
Grep::SearchStep Grep::search_invert(SearchContext& ctx)
{
  const char *& pathname = ctx.pathname;

      {
        // option -v without -ABC, -y

        // construct event handler functor with captured *this and some of the locals to output or save restline when the buffer shifts
        InvertMatchGrepHandler invert_match_handler(*this, pathname);

        // register the event handler to output non-matching lines, matcher resets to NULL when a new input source is assigned
        matcher->set_handler(&invert_match_handler);

        // to get the context from the invert_match handler explicitly
        reflex::AbstractMatcher::Context context;

        while (matcher->find())
        {
          size_t current_lineno = matcher->lineno();

          if (lineno != current_lineno)
          {
            if (matchers != NULL)
            {
              const char *eol = matcher->eol(true); // warning: call eol() before bol() and end()
              const char *bol = matcher->bol();

              // check CNF AND/OR/NOT matching
              if (!cnf_matching(bol, eol))
                continue;
            }

            // get the lines before the matched line
            context = matcher->before();

            // output non-matching lines up to this line
            if (context.len > 0)
              invert_match_handler(*matcher, context.buf, context.len, context.num);

            if (matches > 0 && (binfile || (binary && !flag_hex && !flag_with_hex)))
              break;

            if (binary)
              out.dump.done();

            // --range: max line exceeded?
            if (flag_max_line > 0 && current_lineno > flag_max_line)
              goto done_search;

            // --max-files: max reached?
            if (stop)
              goto exit_search;

            // --max-count: max number of matches reached?
            if (flag_max_count > 0 && matches >= flag_max_count)
              goto done_search;

            // output blocked?
            if (out.eof)
              goto exit_search;
          }

          lineno = current_lineno + matcher->lines() - 1;
        }

        // get the remaining context after the last match to output
        context = matcher->after();

        if (context.len > 0)
          invert_match_handler(*matcher, context.buf, context.len, context.num);

        if (matches > 0 && (binfile || (binary && !flag_hex && !flag_with_hex)))
        {
          if (flag_binary_without_match)
            matches = 0;
          else
            out.binary_file_matches(pathname, partname);
        }

        if (binary)
          out.dump.done();
      }

done_search:
  return SearchStep::DONE;

exit_search:
  return SearchStep::EXIT;
}

// search branch -y
Grep::SearchStep Grep::search_any_line(SearchContext& ctx)
{
  const char *& pathname = ctx.pathname;

      {
        // option -y

        // counter for replace field %m matches
        size_t replace_matching = 0;
        bool colorize = flag_color != NULL || flag_replace != NULL || flag_tag != NULL;

        // to output the rest of the matching line
        restline_data = NULL;
        restline_size = 0;
        restline_last = 0;

        // construct event handler functor with captured *this and some of the locals to output or save restline when the buffer shifts
        AnyLineGrepHandler any_line_handler(*this, pathname);

        // register the event handler functor to output non-matching lines, matcher resets to NULL when a new input source is assigned
        matcher->set_handler(&any_line_handler);

        // to output colors with or without -v
        short v_hex_line = flag_invert_match ? Output::Dump::HEX_CONTEXT_LINE : Output::Dump::HEX_LINE;
        short v_hex_match = flag_invert_match ? Output::Dump::HEX_CONTEXT_MATCH : Output::Dump::HEX_MATCH;
        const char *v_color_sl = flag_invert_match ? color_cx : color_sl;
        const char *v_match_ms = flag_invert_match ? match_mc : match_ms;
        const char *separator = flag_invert_match ? flag_separator_dash : flag_separator;

        // to get the context from the any_line handler explicitly
        reflex::AbstractMatcher::Context context;

        while (matcher->find())
        {
          size_t current_lineno = matcher->lineno();

          if (lineno != current_lineno) /* || flag_ungroup) // logically OK but dead code because -y */
          {
            output_restline(flag_invert_match);

            const char *eol = matcher->eol(true); // warning: call eol() before bol() and end()
            const char *bol = matcher->bol();

            // check CNF AND/OR/NOT matching
            if (matchers != NULL && !cnf_matching(bol, eol))
              continue;

            if (!flag_invert_match)
            {
              ++matches;

              // --min-count: require at least min-count matches
              if (flag_min_count > 0 && matches < flag_min_count)
              {
                lineno = current_lineno;
                continue;
              }

              if (matches == flag_min_count + (flag_min_count == 0) && (!flag_files || matchers == NULL))
              {
                // --max-files: max reached?
                if (!Stats::found_part())
                  goto exit_search;
              }
            }

            // --range: max line exceeded?
            if (flag_max_line > 0 && current_lineno > flag_max_line)
              break;

            // --max-files: max reached?
            if (stop)
              goto exit_search;

            // get the lines before the matched line
            context = matcher->before();

            if (context.len > 0)
            {
              any_line_handler(*matcher, context.buf, context.len, context.num);

              if (matches > 0 && (binfile || (binary && !flag_hex && !flag_with_hex)))
              {
                if (flag_binary_without_match)
                {
                  matches = 0;
                }
                else
                {
                  out.binary_file_matches(pathname, partname);
                  matches = 1;
                }

                if (flag_files && matchers != NULL && out.holding())
                  continue;

                goto done_search;
              }
            }

            // output blocked?
            if (out.eof)
              goto exit_search;

            binary = check_binary(bol, eol - bol);

            if (binfile || (binary && !flag_hex && !flag_with_hex))
            {
              if (flag_binary_without_match)
              {
                matches = 0;
              }
              else if (flag_invert_match)
              {
                lineno = current_lineno + matcher->lines() - 1;
                continue;
              }
              else
              {
                out.binary_file_matches(pathname, partname);
                matches = 1;
              }

              if (flag_files && matchers != NULL && out.holding())
                continue;

              goto done_search;
            }

            size_t border = matcher->border();
            size_t first = matcher->first();
            size_t last = matcher->last();
            const char *begin = matcher->begin();
            const char *end = matcher->end();
            size_t size = matcher->size();

            if (hex && !binary)
              out.dump.done();

            if (!flag_no_header)
              out.header(pathname, partname, heading, current_lineno, matcher, first, separator, binary);

            hex = binary;

            lineno = current_lineno;

            if (binary)
            {
              out.dump.hex(v_hex_line, first - border, bol, border);
              out.dump.hex(v_hex_match, first, begin, size);

              if (false) /* flag_ungroup) // logically OK but dead code because -y */
              {
                out.dump.hex(v_hex_line, last, end, eol - end);
                out.dump.done();
              }
              else
              {
                restline_data = end;
                restline_size = eol - end;
                restline_last = last;
              }

              lineno += matcher->lines() - 1;
            }
            else
            {
              if (border > 0)
              {
                out.str(v_color_sl);
                out.str(bol, border);
                out.str(color_off);
              }

              if (flag_replace != NULL)
              {
                // output --replace=FORMAT
                out.str(v_match_ms);
                out.format(flag_replace, pathname, partname, matches, &replace_matching, matcher, heading, matches > 1, matches > 1);
                out.str(match_off);

                lineno += matcher->lines() - 1;
              }
              else
              {
                if (flag_multiline)
                {
                  // echo multi-line matches line-by-line
                  const char *from = begin;
                  const char *to;

                  while ((to = static_cast<const char*>(memchr(from, '\n', size - (from - begin)))) != NULL)
                  {
                    out.str(v_match_ms);
                    out.str(from, to - from);
                    out.str(match_off);
                    out.chr('\n');

                    out.header(pathname, partname, heading, ++lineno, NULL, first + (to - begin) + 1, flag_separator_bar, false);

                    from = to + 1;
                  }

                  size -= from - begin;
                  begin = from;
                }

                out.str(v_match_ms);
                out.str(begin, size);
                out.str(match_off);
              }

              if (false) /* flag_ungroup) // logically OK but dead code because -y */
              {
                if (eol > end)
                {
                  bool lf_only = end[eol - end - 1] == '\n';
                  eol -= end[eol - end - 1] == '\n';
                  if (eol > end)
                  {
                    out.str(v_color_sl);
                    out.str(end, eol - end);
                    out.str(color_off);
                  }
                  out.nl(lf_only);
                }
                else if (matcher->hit_end())
                {
                  out.nl();
                }
                else
                {
                  out.check_flush();
                }
              }
              else
              {
                restline_data = end;
                restline_size = eol - end;
                restline_last = last;
              }
            }

            // --max-count: max number of matches reached?
            if (flag_max_count > 0 && matches >= flag_max_count)
              break;

            // no -u and no colors: if the match does not span more than one line, then skip to end of the line
            if (!flag_ungroup && !colorize)
              if (!flag_multiline && !matcher->at_bol())
                if (!matcher->skip('\n'))
                  break;
          }
          else if (!binfile && (!binary || flag_hex || flag_with_hex))
          {
            // --min-count: require at least min-count matches
            if (flag_min_count > 0 && matches < flag_min_count)
              continue;

            size_t size = matcher->size();

            if (size > 0)
            {
              size_t lines = matcher->lines();

              if (lines > 1 || colorize)
              {
                size_t first = matcher->first();
                size_t last = matcher->last();
                const char *begin = matcher->begin();

                if (binary)
                {
                  if (restline_data != NULL)
                  {
                    if (restline_last + restline_size > first)
                      restline_size = first - restline_last;
                    out.dump.hex(v_hex_line, restline_last, restline_data, restline_size);
                  }

                  out.dump.hex(v_hex_match, first, begin, size);
                }
                else
                {
                  if (restline_data != NULL)
                  {
                    if (restline_last + restline_size > first)
                      restline_size = first - restline_last;
                    out.str(v_color_sl);
                    out.str(restline_data, restline_size);
                    out.str(color_off);
                  }

                  if (flag_replace != NULL)
                  {
                    // output --replace=FORMAT
                    out.str(v_match_ms);
                    out.format(flag_replace, pathname, partname, matches, &replace_matching, matcher, heading, matches > 1, matches > 1);
                    out.str(match_off);
                  }
                  else
                  {
                    if (lines > 1)
                    {
                      // echo multi-line matches line-by-line

                      const char *from = begin;
                      const char *to;
                      size_t num = 1;

                      while ((to = static_cast<const char*>(memchr(from, '\n', size - (from - begin)))) != NULL)
                      {
                        out.str(v_match_ms);
                        out.str(from, to - from);
                        out.str(match_off);
                        out.chr('\n');

                        out.header(pathname, partname, heading, lineno + num, NULL, first + (to - begin) + 1, flag_separator_bar, false);

                        from = to + 1;
                        ++num;
                      }

                      size -= from - begin;
                      begin = from;
                    }

                    out.str(v_match_ms);
                    out.str(begin, size);
                    out.str(match_off);
                  }
                }

                const char *eol = matcher->eol(true); // warning: call eol() before end()
                const char *end = matcher->end();

                if (lines == 1)
                {
                  restline_data = end;
                  restline_size = eol - end;
                  restline_last = last;
                }
                else
                {
                  binary = check_binary(end, eol - end);

                  if (hex && !binary)
                    out.dump.done();
                  else if (!hex && binary)
                    out.nl();

                  if (hex != binary && !flag_no_header)
                    out.header(pathname, partname, heading, lineno + lines - 1, matcher, last, flag_separator, binary);

                  hex = binary;

                  if (false) /* flag_ungroup) // logically OK but dead code because -y */
                  {
                    if (binary)
                    {
                      out.dump.hex(v_hex_line, last, end, eol - end);
                      out.dump.done();
                    }
                    else
                    {
                      if (eol > end)
                      {
                        bool lf_only = end[eol - end - 1] == '\n';
                        eol -= lf_only;
                        if (eol > end)
                        {
                          out.str(v_color_sl);
                          out.str(end, eol - end);
                          out.str(color_off);
                        }
                        out.nl(lf_only);
                      }
                      else if (matcher->hit_end())
                      {
                        out.nl();
                      }
                      else
                      {
                        out.check_flush();
                      }
                    }
                  }
                  else
                  {
                    restline_data = end;
                    restline_size = eol - end;
                    restline_last = last;
                  }

                  lineno += lines - 1;
                }
              }
            }
          }
        }

        output_restline(flag_invert_match);

        if (flag_max_count == 0 || matches < flag_max_count)
        {
          // get the remaining context after the last match to output
          context = matcher->after();

          if (context.len > 0)
            any_line_handler(*matcher, context.buf, context.len, context.num);
        }

        // --min-count: require at least min-count matches
        if (flag_min_count > 0 && matches < flag_min_count)
          matches = 0;

        if (matches > 0 && (binfile || (binary && !flag_hex && !flag_with_hex)))
        {
          if (flag_binary_without_match)
            matches = 0;
          else
            out.binary_file_matches(pathname, partname);
        }

        if (binary)
          out.dump.done();
      }

done_search:
  return SearchStep::DONE;

exit_search:
  return SearchStep::EXIT;
}

// search branch -A, -B, -C
Grep::SearchStep Grep::search_context(SearchContext& ctx)
{
  const char *& pathname = ctx.pathname;

      {
        // options -ABC without -v

        // counter for replace field %m matches
        size_t replace_matching = 0;
        bool colorize = flag_color != NULL || flag_replace != NULL || flag_tag != NULL;

        // to output the rest of the matching line
        restline_data = NULL;
        restline_size = 0;
        restline_last = 0;

        // construct event handler functor with captured *this and some of the locals to output or save restline when the buffer shifts
        ContextGrepHandler context_handler(*this, pathname);

        // register the event handler functor to output non-matching lines, matcher resets to NULL when a new input source is assigned
        matcher->set_handler(&context_handler);

        // to get the context from the context handler explicitly
        reflex::AbstractMatcher::Context context;

        while (matcher->find())
        {
          size_t current_lineno = matcher->lineno();

          if (lineno != current_lineno || flag_ungroup)
          {
            output_restline();

            const char *eol = matcher->eol(true); // warning: call eol() before bol() and end()
            const char *bol = matcher->bol();

            // check CNF AND/OR/NOT matching
            if (matchers != NULL && !cnf_matching(bol, eol))
              continue;

            ++matches;

            // --min-count: require at least min-count matches
            if (flag_min_count > 0 && matches < flag_min_count)
            {
              lineno = current_lineno;
              continue;
            }

            // --range: max line exceeded?
            if (flag_max_line > 0 && current_lineno > flag_max_line)
              break;

            // --max-files: max reached?
            if (stop)
              goto exit_search;

            if (matches == 1 && (!flag_files || matchers == NULL))
            {
              // --max-files: max reached?
              if (!Stats::found_part())
                goto exit_search;
            }

            // output blocked?
            if (out.eof)
              goto exit_search;

            // get the lines before the matched line
            context = matcher->before();

            if (context.len > 0)
              context_handler(*matcher, context.buf, context.len, context.num);

            // --max-count: max number of matches reached?
            if (flag_max_count > 0 && matches > flag_max_count)
              break;

            context_handler.output_before_context();

            binary = check_binary(bol, eol - bol);

            if (binfile || (binary && !flag_hex && !flag_with_hex))
            {
              if (flag_binary_without_match)
              {
                matches = 0;
              }
              else
              {
                out.binary_file_matches(pathname, partname);
                matches = 1;
              }

              if (flag_files && matchers != NULL && out.holding())
                continue;

              goto done_search;
            }

            size_t border = matcher->border();
            size_t first = matcher->first();
            size_t last = matcher->last();
            const char *begin = matcher->begin();
            const char *end = matcher->end();
            size_t size = matcher->size();

            if (hex && !binary)
              out.dump.done();

            if (!flag_no_header)
              out.header(pathname, partname, heading, current_lineno, matcher, first, flag_separator, binary);

            hex = binary;

            lineno = current_lineno;

            if (binary)
            {
              out.dump.hex(Output::Dump::HEX_LINE, first - border, bol, border);
              out.dump.hex(Output::Dump::HEX_MATCH, first, begin, size);

              if (flag_ungroup)
              {
                out.dump.hex(Output::Dump::HEX_LINE, last, end, eol - end);
                out.dump.done();
              }
              else
              {
                restline_data = end;
                restline_size = eol - end;
                restline_last = last;
              }

              lineno += matcher->lines() - 1;
            }
            else
            {
              if (border > 0)
              {
                out.str(color_sl);
                out.str(bol, border);
                out.str(color_off);
              }

              if (flag_replace != NULL)
              {
                // output --replace=FORMAT
                out.str(match_ms);
                out.format(flag_replace, pathname, partname, matches, &replace_matching, matcher, heading, matches > 1, matches > 1);
                out.str(match_off);

                lineno += matcher->lines() - 1;
              }
              else
              {
                if (flag_multiline)
                {
                  // echo multi-line matches line-by-line
                  const char *from = begin;
                  const char *to;

                  while ((to = static_cast<const char*>(memchr(from, '\n', size - (from - begin)))) != NULL)
                  {
                    out.str(match_ms);
                    out.str(from, to - from);
                    out.str(match_off);
                    out.chr('\n');

                    out.header(pathname, partname, heading, ++lineno, NULL, first + (to - begin) + 1, flag_separator_bar, false);

                    from = to + 1;
                  }

                  size -= from - begin;
                  begin = from;
                }

                out.str(match_ms);
                out.str(begin, size);
                out.str(match_off);
              }

              if (flag_ungroup)
              {
                if (eol > end)
                {
                  bool lf_only = end[eol - end - 1] == '\n';
                  eol -= lf_only;
                  if (eol > end)
                  {
                    out.str(color_sl);
                    out.str(end, eol - end);
                    out.str(color_off);
                  }
                  out.nl(lf_only);
                }
                else if (matcher->hit_end())
                {
                  out.nl();
                }
                else
                {
                  out.check_flush();
                }
              }
              else
              {
                restline_data = end;
                restline_size = eol - end;
                restline_last = last;
              }
            }

            // no -u and no colors: if the match does not span more than one line, then skip to end of the line
            if (!flag_ungroup && !colorize)
              if (!flag_multiline && !matcher->at_bol())
                if (!matcher->skip('\n'))
                  break;
          }
          else
          {
            // --min-count: require at least min-count matches
            if (flag_min_count > 0 && matches < flag_min_count)
              continue;

            size_t size = matcher->size();

            if (size > 0)
            {
              size_t lines = matcher->lines();

              if (lines > 1 || colorize)
              {
                size_t first = matcher->first();
                size_t last = matcher->last();
                const char *begin = matcher->begin();

                if (binary)
                {
                  if (restline_data != NULL)
                  {
                    if (restline_last + restline_size > first)
                      restline_size = first - restline_last;
                    out.dump.hex(Output::Dump::HEX_LINE, restline_last, restline_data, restline_size);
                  }

                  out.dump.hex(Output::Dump::HEX_MATCH, first, begin, size);
                }
                else
                {
                  if (restline_data != NULL)
                  {
                    if (restline_last + restline_size > first)
                      restline_size = first - restline_last;
                    out.str(color_sl);
                    out.str(restline_data, restline_size);
                    out.str(color_off);
                  }

                  if (flag_replace != NULL)
                  {
                    // output --replace=FORMAT
                    out.str(match_ms);
                    out.format(flag_replace, pathname, partname, matches, &replace_matching, matcher, heading, matches > 1, matches > 1);
                    out.str(match_off);
                  }
                  else
                  {
                    if (lines > 1)
                    {
                      // echo multi-line matches line-by-line

                      const char *from = begin;
                      const char *to;
                      size_t num = 1;

                      while ((to = static_cast<const char*>(memchr(from, '\n', size - (from - begin)))) != NULL)
                      {
                        out.str(match_ms);
                        out.str(from, to - from);
                        out.str(match_off);
                        out.chr('\n');

                        out.header(pathname, partname, heading, lineno + num, NULL, first + (to - begin) + 1, flag_separator_bar, false);

                        from = to + 1;
                        ++num;
                      }

                      size -= from - begin;
                      begin = from;
                    }

                    out.str(match_ms);
                    out.str(begin, size);
                    out.str(match_off);
                  }
                }

                const char *eol = matcher->eol(true); // warning: call eol() before end()
                const char *end = matcher->end();

                if (lines == 1)
                {
                  restline_data = end;
                  restline_size = eol - end;
                  restline_last = last;
                }
                else
                {
                  binary = check_binary(end, eol - end);

                  if (hex && !binary)
                    out.dump.done();
                  else if (!hex && binary)
                    out.nl();

                  if (hex != binary && !flag_no_header)
                    out.header(pathname, partname, heading, lineno + lines - 1, matcher, last, flag_separator, binary);

                  hex = binary;

                  if (flag_ungroup)
                  {
                    if (binary)
                    {
                      out.dump.hex(Output::Dump::HEX_LINE, last, end, eol - end);
                      out.dump.done();
                    }
                    else
                    {
                      if (eol > end)
                      {
                        bool lf_only = end[eol - end - 1] == '\n';
                        eol -= lf_only;
                        if (eol > end)
                        {
                          out.str(color_sl);
                          out.str(end, eol - end);
                          out.str(color_off);
                        }
                        out.nl(lf_only);
                      }
                      else if (matcher->hit_end())
                      {
                        out.nl();
                      }
                      else
                      {
                        out.check_flush();
                      }
                    }
                  }
                  else
                  {
                    restline_data = end;
                    restline_size = eol - end;
                    restline_last = last;
                  }

                  lineno += lines - 1;
                }
              }
            }
          }

          context_handler.set_after_lineno(lineno + 1);
        }

        output_restline();

        // get the remaining context after the last match to output
        context = matcher->after();

        if (context.len > 0)
          context_handler(*matcher, context.buf, context.len, context.num);

        if (binfile || (binary && !flag_hex && !flag_with_hex))
        {
          if (flag_binary_without_match)
            matches = 0;
          else if (matches > 0)
            out.binary_file_matches(pathname, partname);
        }

        if (binary)
          out.dump.done();
      }

done_search:
  return SearchStep::DONE;

exit_search:
  return SearchStep::EXIT;
}

// search branch -A, -B, -C with -v
Grep::SearchStep Grep::search_context_invert(SearchContext& ctx)
{
  const char *& pathname = ctx.pathname;

      {
        // options -ABC with -v

        size_t after = flag_after_context;
        size_t last_lineno = lineno;
        size_t next_lineno = lineno + 1;
        // counter for replace field %m matches
        size_t replace_matching = 0;
        bool colorize = flag_color != NULL || flag_replace != NULL || flag_tag != NULL;

        // to output the rest of the matching line
        restline_data = NULL;
        restline_size = 0;
        restline_last = 0;

        // construct event handler functor with captured *this and some of the locals to output or save restline when the buffer shifts
        InvertContextGrepHandler invert_context_handler(*this, pathname);

        // register the event handler functor to output non-matching lines, matcher resets to NULL when a new input source is assigned
        matcher->set_handler(&invert_context_handler);

        // to get the context from the context handler explicitly
        reflex::AbstractMatcher::Context context;

        while (matcher->find())
        {
          size_t current_lineno = matcher->lineno();
          size_t lines = matcher->lines();

          // adjust the number of after lines output so far upward if uninterrupted, or reset to zero
          if (last_lineno != current_lineno)
          {
            if (next_lineno >= current_lineno)
              after += lines;
            else
              after = 0;
          }

          if (lineno != current_lineno)
          {
            output_restline(true);

            const char *eol = matcher->eol(true); // warning: call eol() before bol() and end()
            const char *bol = matcher->bol();

            // check CNF AND/OR/NOT matching
            if (matchers != NULL && !cnf_matching(bol, eol))
              continue;

            // get the lines before the matched line
            context = matcher->before();

            if (context.len > 0)
            {
              invert_context_handler(*matcher, context.buf, context.len, context.num);

              if (matches > 0 && (binfile || (binary && !flag_hex && !flag_with_hex)))
              {
                if (flag_binary_without_match)
                {
                  matches = 0;
                }
                else
                {
                  out.binary_file_matches(pathname, partname);
                  matches = 1;
                }

                goto done_search;
              }
            }

            lineno = current_lineno;

            // --range: max line exceeded?
            if (flag_max_line > 0 && lineno > flag_max_line)
              break;

            // --max-files: max reached?
            if (stop)
              goto exit_search;

            /* logically OK but dead code because -v
            if (matches == 0 && !flag_invert_match && (!flag_files || matchers == NULL))
            {
              // --max-files: max reached?
              if (!Stats::found_part())
                goto exit_search;
            }
            */

            // --max-count: max number of matches reached?
            if (flag_max_count > 0 && matches >= flag_max_count)
              break;

            // output blocked?
            if (out.eof)
              goto exit_search;

            if (after < flag_after_context)
            {
              binary = check_binary(bol, eol - bol);

              if (binfile || (binary && !flag_hex && !flag_with_hex))
              {
                if (flag_binary_without_match)
                {
                  matches = 0;
                }
                else // if (flag_invert_match) is true
                {
                  last_lineno = current_lineno;
                  next_lineno = current_lineno + lines;
                  lineno = next_lineno - 1;
                  continue;
                }
                /* logically OK but dead code because -v
                else
                {
                  out.binary_file_matches(pathname, partname);
                  matches = 1;
                }
                */

                goto done_search;
              }

              size_t border = matcher->border();
              size_t first = matcher->first();
              size_t last = matcher->last();
              const char *begin = matcher->begin();
              const char *end = matcher->end();
              size_t size = matcher->size();

              if (hex && !binary)
                out.dump.done();

              if (!flag_no_header)
                out.header(pathname, partname, heading, lineno, matcher, first, flag_separator_dash, binary);

              hex = binary;

              if (binary)
              {
                if (flag_hex || flag_with_hex)
                {
                  out.dump.hex(Output::Dump::HEX_CONTEXT_LINE, first - border, bol, border);
                  out.dump.hex(Output::Dump::HEX_CONTEXT_MATCH, first, begin, size);

                  restline_data = end;
                  restline_size = eol - end;
                  restline_last = last;
                }
              }
              else
              {
                if (border > 0)
                {
                  out.str(color_cx);
                  out.str(bol, border);
                  out.str(color_off);
                }

                if (flag_replace != NULL)
                {
                  // output --replace=FORMAT
                  out.str(match_mc);
                  out.format(flag_replace, pathname, partname, matches, &replace_matching, matcher, heading, matches > 1, matches > 1);
                  out.str(match_off);
                }
                else
                {
                  if (lines > 1)
                  {
                    // echo multi-line matches line-by-line

                    const char *from = begin;
                    const char *to;
                    size_t num = 1;

                    while ((to = static_cast<const char*>(memchr(from, '\n', size - (from - begin)))) != NULL)
                    {
                      out.str(match_mc);
                      out.str(from, to - from);
                      out.str(match_off);
                      out.chr('\n');

                      out.header(pathname, partname, heading, lineno + num, NULL, first + (to - begin) + 1, flag_separator_dash, false);

                      from = to + 1;
                      ++num;
                    }

                    size -= from - begin;
                    begin = from;
                  }

                  out.str(match_mc);
                  out.str(begin, size);
                  out.str(match_off);
                }

                restline_data = end;
                restline_size = eol - end;
                restline_last = last;
              }
            }
            else if (flag_before_context > 0)
            {
              binary = check_binary(bol, eol - bol);

              if (binfile || (binary && !flag_hex && !flag_with_hex))
              {
                if (flag_binary_without_match)
                {
                  matches = 0;
                }
                else // if (flag_invert_match) is true
                {
                  last_lineno = current_lineno;
                  next_lineno = current_lineno + lines;
                  lineno = next_lineno - 1;
                  continue;
                }
                /* logically OK but dead code because -v
                else
                {
                  out.binary_file_matches(pathname, partname);
                  matches = 1;
                }
                */

                goto done_search;
              }

              if (hex && !binary)
                out.dump.done();
              hex = binary;

              const char *begin = matcher->begin();
              size_t size = matcher->size();
              size_t offset = matcher->first();

              if (lines == 1)
              {
                invert_context_handler.add_before_context_line(bol, eol, matcher->columno(), offset - (begin - bol));
                invert_context_handler.add_before_context_match(begin - bol, size, offset);
              }
              else
              {
                // add lines to the before context

                const char *from = begin;
                const char *to;

                while ((to = static_cast<const char*>(memchr(from, '\n', eol - from))) != NULL)
                {
                  if (from == begin)
                  {
                    invert_context_handler.add_before_context_line(bol, to + 1, matcher->columno(), offset - (begin - bol));
                    invert_context_handler.add_before_context_match(begin - bol, to - from + 1, offset);
                  }
                  else
                  {
                    invert_context_handler.add_before_context_line(from, to + 1, 1, offset);
                    invert_context_handler.add_before_context_match(0, to + 1 < from + size ? to - from + 1 : size, offset);
                  }

                  size -= to - from + 1;
                  offset += to - from + 1;
                  from = to + 1;
                }
              }
            }
          }
          else if (after < flag_after_context)
          {
            size_t size = matcher->size();

            if (size > 0)
            {
              if (lines > 1 || colorize)
              {
                size_t first = matcher->first();
                size_t last = matcher->last();
                const char *begin = matcher->begin();

                if (binary)
                {
                  if (restline_data != NULL)
                    out.dump.hex(Output::Dump::HEX_CONTEXT_LINE, restline_last, restline_data, first - restline_last);

                  out.dump.hex(Output::Dump::HEX_CONTEXT_MATCH, first, begin, size);
                }
                else
                {
                  if (restline_data != NULL)
                  {
                    out.str(color_cx);
                    out.str(restline_data, first - restline_last);
                    out.str(color_off);
                  }

                  if (flag_replace != NULL)
                  {
                    // output --replace=FORMAT
                    out.str(match_mc);
                    out.format(flag_replace, pathname, partname, matches, &replace_matching, matcher, heading, matches > 1, matches > 1);
                    out.str(match_off);
                  }
                  else
                  {
                    if (lines > 1)
                    {
                      // echo multi-line matches line-by-line

                      const char *from = begin;
                      const char *to;
                      size_t num = 1;

                      while ((to = static_cast<const char*>(memchr(from, '\n', size - (from - begin)))) != NULL)
                      {
                        out.str(match_mc);
                        out.str(from, to - from);
                        out.str(match_off);
                        out.chr('\n');

                        out.header(pathname, partname, heading, lineno + num, NULL, first + (to - begin) + 1, flag_separator_dash, false);

                        from = to + 1;
                        ++num;
                      }

                      size -= from - begin;
                      begin = from;
                    }

                    out.str(match_mc);
                    out.str(begin, size);
                    out.str(match_off);
                  }
                }

                if (lines == 1)
                {
                  restline_data += last - restline_last;
                  restline_size -= last - restline_last;
                  restline_last = last;
                }
                else
                {
                  const char *eol = matcher->eol(true); // warning: call eol() before end()
                  const char *end = matcher->end();

                  binary = check_binary(end, eol - end);

                  if (hex && !binary)
                    out.dump.done();
                  else if (!hex && binary)
                    out.nl();

                  if (hex != binary && !flag_no_header)
                    out.header(pathname, partname, heading, lineno + lines - 1, matcher, last, flag_separator_dash, binary);

                  hex = binary;

                  restline_data = end;
                  restline_size = eol - end;
                  restline_last = last;
                }
              }
            }
          }
          else
          {
            output_restline(true);

            if (flag_before_context > 0)
            {
              const char *eol = matcher->eol(true); // warning: call eol() before bol()
              const char *bol = matcher->bol();
              const char *begin = matcher->begin();
              size_t size = matcher->size();
              size_t offset = matcher->first();

              if (lines == 1)
              {
                invert_context_handler.add_before_context_match(begin - bol, size, offset);
              }
              else
              {
                // add lines to the before context

                const char *end = matcher->end();

                binary = check_binary(end, eol - end);

                if (binfile || (binary && !flag_hex && !flag_with_hex))
                {
                  if (flag_binary_without_match)
                  {
                    matches = 0;
                  }
                  else // if (flag_invert_match) is true
                  {
                    last_lineno = current_lineno;
                    next_lineno = current_lineno + lines;
                    lineno = next_lineno - 1;
                    continue;
                  }
                  /* logically OK but dead code because -v
                  else
                  {
                    out.binary_file_matches(pathname, partname);
                    matches = 1;
                  }
                  */

                  goto done_search;
                }

                if (hex && !binary)
                  out.dump.done();
                hex = binary;

                const char *from = begin;
                const char *to;

                while ((to = static_cast<const char*>(memchr(from, '\n', eol - from))) != NULL)
                {
                  if (from == begin)
                  {
                    invert_context_handler.add_before_context_match(begin - bol, to - from + 1, offset);
                  }
                  else
                  {
                    invert_context_handler.add_before_context_line(from, to + 1, 1, offset);
                    invert_context_handler.add_before_context_match(0, to + 1 < from + size ? to - from + 1 : size, offset);
                  }

                  size -= to - from + 1;
                  offset += to - from + 1;
                  from = to + 1;
                }
              }
            }
          }

          last_lineno = current_lineno;
          next_lineno = current_lineno + lines;
          lineno = next_lineno - 1;
        }

        output_restline(true);

        // get the remaining context after the last match to output
        context = matcher->after();

        if (context.len > 0)
          invert_context_handler(*matcher, context.buf, context.len, context.num);

        if (matches > 0 && (binfile || (binary && !flag_hex && !flag_with_hex)))
        {
          if (flag_binary_without_match)
            matches = 0;
          else
            out.binary_file_matches(pathname, partname);
        }

        if (binary)
          out.dump.done();
      }

done_search:
  return SearchStep::DONE;

exit_search:
  return SearchStep::EXIT;
}

// search epilogue: --files check, --break and --stats, run when the branch did not leave through exit_search
void Grep::search_epilogue(SearchContext& ctx)
{
  // --files: check if all CNF conditions are met globally to launch output or reset matches
  if (flag_files && matchers != NULL)
    if (!cnf_satisfied())
      matches = 0;

  // any matches in this file or archive?
  if (matches > 0)
    ctx.matched = true;

  // --break: add a line break when applicable
  if (flag_break && (matches > 0 || flag_any_line) && !flag_quiet && !flag_files_with_matches && !flag_count && flag_format == NULL)
    out.nl();

  if (flag_stats != NULL)
    Stats::score_matches(matches, matcher->lineno() > 0 ? matcher->lineno() - 1 : 0);
}

// search input after lineno to populate a string vector with the matching line and lines after up to max lines. used by the TUI
void Grep::find_text_preview(const char *filename, const char *findpart, size_t from_lineno, size_t max, size_t& lineno, size_t& num, std::vector<std::string>& text)
{
  // no results yet
  lineno = from_lineno;
  num = 0;

  try
  {
    // open (archive or compressed) file
    if (!open_file(filename, findpart))
      return;
  }

  catch (...)
  {
    // this should never happen
    return;
  }

  matcher->input(input);

#if !defined(HAVE_PCRE2) && defined(HAVE_BOOST_REGEX)
  // buffer all input to work around Boost.Regex partial matching bug, but this may throw std::bad_alloc if the file is too large
  if (flag_mode == Mode::PERL)
    matcher->buffer();
#endif

  bool eof = true;

  for (size_t i = from_lineno; i > 1; --i)
    if (!matcher->skip('\n'))
      break;

  if (flag_invert_match)
  {
    lineno = matcher->lineno();
    eof = false;
  }
  else
  {
    while (matcher->find())
    {
      const char *eol = matcher->eol(true); // warning: call eol() before bol() and end()
      const char *bol = matcher->bol();

      // check CNF AND/OR/NOT matching
      if (matchers != NULL && !flag_files && !cnf_matching(bol, eol))
        continue;

      lineno = matcher->lineno();

      if (text.empty())
        text.emplace_back(bol, eol - bol);
      else
        text[0].assign(bol, eol - bol);

      num = 1;
      eof = !matcher->skip('\n');

      break;
    }
  }

  while (!eof && num < max)
  {
    const char *eol = matcher->eol(true); // warning: call eol() before bol() and end()
    const char *bol = matcher->bol();

    if (text.size() <= num)
      text.emplace_back(bol, eol - bol);
    else
      text[num].assign(bol, eol - bol);

    ++num;

    eof = !matcher->skip('\n');
  }

  // close file or -z: loop over next extracted archive parts, when applicable
  while (close_file(filename))
    continue;
}

// extract a part from an archive and send to a stream, used by the TUI
void Grep::extract(const char *filename, const char *findpart, FILE *output)
{
  try
  {
    // open (archive or compressed) file
    if (!open_file(filename, findpart))
      return;
  }

  catch (...)
  {
    // this should never happen
    return;
  }

  // copy input to output
  char buffer[65536];
  while (true)
  {
    size_t len = input.get(buffer, sizeof(buffer));
    if (len == 0 || fwrite(buffer, 1, len, output) < len)
      break;
  }

  // close file or -z: loop over next extracted archive parts, when applicable
  while (close_file(filename))
    continue;
}

// read globs from a file and split them into files or dirs to include or exclude by pushing them onto the vectors, gitignore compatible (true), use local pathname as root dir / in globs (when non-NULL)
void import_globs(FILE *file, std::vector<std::string>& files, std::vector<std::string>& dirs, bool gitignore, const char *pathname)
{
  // read globs from the specified file or files
  reflex::BufferedInput input(file);
  std::string pathname_glob;
  std::string line;

  while (true)
  {
    // read the next line
    if (getline(input, line))
      break;

    // trim white space from either end
    trim(line);

    // add glob to files or dirs using gitignore glob pattern rules
    if (!line.empty() && line.front() != '#')
    {
      if (line.size() > 1 || line.front() != '!') // negated matches start with ! (but not ^)
      {
        // if pathname is specified, then expand the local pathname to replace the root dir / in globs
        if (pathname != NULL && *pathname != '\0')
        {
          size_t pos = line.front() == '!'; // negated matches start with ! (but not ^)

          if (line.size() > pos && line.at(pos) == '/')
          {
            // create pathname glob on demand by escaping *, ?, [, and \ in the pathname to match literally
            if (pathname_glob.empty())
            {
              pathname_glob.assign(pathname);

              for (size_t i = 0; i < pathname_glob.size(); ++i)
              {
                int ch = pathname_glob.at(i);
                if (ch == '*' || ch == '?' || ch == '[' || ch == '\\')
                  pathname_glob.insert(i++, "\\", 1);
              }
            }

            line.insert(pos, pathname_glob);
          }
        }

        if (line.back() == '/')
        {
          if (line.size() > 1)
            line.pop_back();
          dirs.emplace_back(line);
        }
        else
        {
          files.emplace_back(line);
          if (gitignore)
            dirs.emplace_back(line);
        }
      }
    }
  }
}

// trim path separators from an argv[] argument - important: modifies the argv[] string passed as arg
void trim_pathname_arg(const char *arg)
{
  // remove trailing path separators after the drive prefix and path, if any - note: this truncates argv[] strings
  const char *path = strchr(arg, ':');

  if (path != NULL)
    ++path;
  else
    path = arg;

  size_t len = strlen(path);

  while (len > 1 && path[--len] == PATHSEPCHR)
    const_cast<char*>(path)[len] = '\0';
}

// usage(), help(), version(), is_directory(), cannot_decompress(), warning(), error(), abort() and the body of main()
// are Rust (rust/src_core/src/help_main.rs, ffi/help_main.rs): the C++ keeps the streams, errno, exit() and the
// build configuration, and hands them over in a callback table

// the callback table and build configuration of the Rust messages, the field order mirrors Rust's DiagEnv
struct CoreDiagEnv
{
  void *ctx;
  void (*cerr_write)(void *, const char *, size_t);
  void (*cout_write)(void *, const char *, size_t);
  void (*errout_write)(void *, const char *, size_t);
  void (*strerror_text)(void *, int, char *, size_t);
  const char *version;
  const char *platform;
  const char *simd;
  const char *regex;
  int perl;
  int zlib;
  int no_7zip;
  int bzip2;
  int lzma;
  int zstd;
  int lz4;
  int brotli;
  int bzip3;
  int decompression_thread;
  int os_win;
  int os_win_or_mingw;
  int statfs;
  const char *grep_path;
  const char *default_pager;
  const char *default_ignore_file;
  void (*past_end_text)(void *, size_t, size_t, char *, size_t);
};

// the callback table of the Rust main, the field order mirrors Rust's MainEnv
struct CoreMainEnv
{
  void *ctx;
  const CoreDiagEnv *diag;
  const char *(*init)(void *, int, const char **);
  void (*tui_query)(void *);
  const char *(*ugrep)(void *);
  size_t (*warnings)(void *);
  int (*found_any)(void *);
};

extern "C" int ugrep_core_usage(const CoreDiagEnv *env, const char *message, const char *arg, const char *valid);
extern "C" int ugrep_core_help(const CoreDiagEnv *env, const char *what);
extern "C" int ugrep_core_version(const CoreDiagEnv *env);
extern "C" void ugrep_core_is_directory(const CoreDiagEnv *env, const char *pathname);
extern "C" void ugrep_core_cannot_decompress(const CoreDiagEnv *env, const char *pathname, const char *message);
extern "C" void ugrep_core_warning(const CoreDiagEnv *env, const char *message, const char *arg, int errnum);
extern "C" int ugrep_core_error(const CoreDiagEnv *env, const char *message, const char *arg, int errnum);
extern "C" int ugrep_core_abort(const CoreDiagEnv *env, const char *message, const char *what);
extern "C" int ugrep_core_main(const CoreMainEnv *env, int argc, const char **argv);

namespace
{

void diag_cerr(void *, const char *data, size_t len)
{
  std::cerr.write(data, len);
  std::cerr.flush();
}

void diag_cout(void *, const char *data, size_t len)
{
  std::cout.write(data, len);
}

void diag_errout(void *, const char *data, size_t len)
{
  fwrite(data, 1, len, Static::errout);
}

// use safe strerror_s() instead of strerror() when available
void diag_strerror(void *, int errnum, char *buf, size_t size)
{
#if defined(__STDC_LIB_EXT1__) || defined(OS_WIN_OR_MINGW)
  strerror_s(buf, size, errnum);
#else
  const char *errmsg = strerror(errnum);
  strncpy(buf, errmsg, size - 1);
  buf[size - 1] = '\0';
#endif
}

// the text of the std::out_of_range exception of std::string::at(), as produced by the C++ standard library
void diag_past_end(void *, size_t n, size_t size, char *buf, size_t bufsize)
{
  try
  {
    std::string(size, ' ').at(n);
  }

  catch (std::out_of_range& e)
  {
    strncpy(buf, e.what(), bufsize - 1);
    buf[bufsize - 1] = '\0';
    return;
  }

  buf[0] = '\0';
}

CoreDiagEnv diag_env()
{
  CoreDiagEnv env;
  env.ctx = NULL;
  env.cerr_write = diag_cerr;
  env.cout_write = diag_cout;
  env.errout_write = diag_errout;
  env.strerror_text = diag_strerror;
  env.version = UGREP_VERSION;
  env.platform = PLATFORM;
  env.simd =
#if defined(HAVE_AVX512BW)
    (reflex::have_HW_AVX512BW() ? " +avx512" : (reflex::have_HW_AVX2() ? " +avx2" : reflex::have_HW_SSE2() ?  " +sse2" : " (no sse2!)"));
#elif defined(HAVE_AVX2)
    (reflex::have_HW_AVX2() ? " +avx2" : reflex::have_HW_SSE2() ?  " +sse2" : " (no sse2!)");
#elif defined(HAVE_SSE2)
    (reflex::have_HW_SSE2() ?  " +sse2" : " (no sse2!)");
#elif defined(HAVE_NEON)
    " +neon/AArch64";
#else
    "";
#endif
#if defined(HAVE_PCRE2)
  uint32_t tmp = 0;
  env.regex = pcre2_config(PCRE2_CONFIG_JIT, &tmp) >= 0 && tmp != 0 ? "; -P:pcre2jit" : "; -P:pcre2";
  env.perl = 1;
#elif defined(HAVE_BOOST_REGEX)
  env.regex = "; -P:boost_regex";
  env.perl = 2;
#else
  env.regex = "";
  env.perl = 0;
#endif
#ifdef HAVE_LIBZ
  env.zlib = 1;
#else
  env.zlib = 0;
#endif
#ifdef WITH_NO_7ZIP
  env.no_7zip = 1;
#else
  env.no_7zip = 0;
#endif
#ifdef HAVE_LIBBZ2
  env.bzip2 = 1;
#else
  env.bzip2 = 0;
#endif
#ifdef HAVE_LIBLZMA
  env.lzma = 1;
#else
  env.lzma = 0;
#endif
#ifdef HAVE_LIBZSTD
  env.zstd = 1;
#else
  env.zstd = 0;
#endif
#ifdef HAVE_LIBLZ4
  env.lz4 = 1;
#else
  env.lz4 = 0;
#endif
#ifdef HAVE_LIBBROTLI
  env.brotli = 1;
#else
  env.brotli = 0;
#endif
#ifdef HAVE_LIBBZIP3
  env.bzip3 = 1;
#else
  env.bzip3 = 0;
#endif
#ifdef WITH_DECOMPRESSION_THREAD
  env.decompression_thread = 1;
#else
  env.decompression_thread = 0;
#endif
#ifdef OS_WIN
  env.os_win = 1;
#else
  env.os_win = 0;
#endif
#ifdef OS_WIN_OR_MINGW
  env.os_win_or_mingw = 1;
#else
  env.os_win_or_mingw = 0;
#endif
#if defined(HAVE_STATVFS) || defined(HAVE_STATFS)
  env.statfs = 1;
#else
  env.statfs = 0;
#endif
#ifdef GREP_PATH
  env.grep_path = GREP_PATH;
#else
  env.grep_path = NULL;
#endif
  env.default_pager = DEFAULT_PAGER_COMMAND;
  env.default_ignore_file = DEFAULT_IGNORE_FILE;
  env.past_end_text = diag_past_end;
  return env;
}

// the exception text handed to Rust must outlive the callback
std::string core_main_what;

const char *core_main_init(void *, int argc, const char **argv)
{
  try
  {
    init(argc, argv);
  }

  catch (std::exception& error)
  {
    core_main_what = error.what();
    return core_main_what.c_str();
  }

  return NULL;
}

void core_main_tui_query(void *)
{
  tui_query();
}

const char *core_main_ugrep(void *)
{
  try
  {
    ugrep();
  }

  catch (reflex::regex_error& error)
  {
    core_main_what = error.what();
    return core_main_what.c_str();
  }

  catch (std::exception& error)
  {
    core_main_what = error.what();
    return core_main_what.c_str();
  }

  return NULL;
}

size_t core_main_warnings(void *)
{
  return Static::warnings;
}

int core_main_found_any(void *)
{
  return Stats::found_any_file() ? 1 : 0;
}

} // namespace

// the body of main() after the arguments and signal handlers are set up
static int core_main(int argc, const char **argv)
{
  CoreDiagEnv diag = diag_env();
  CoreMainEnv env;
  env.ctx = NULL;
  env.diag = &diag;
  env.init = core_main_init;
  env.tui_query = core_main_tui_query;
  env.ugrep = core_main_ugrep;
  env.warnings = core_main_warnings;
  env.found_any = core_main_found_any;
  return ugrep_core_main(&env, argc, argv);
}

// print a diagnostic message
void usage(const char *message, const char *arg, const char *valid)
{
  CoreDiagEnv env = diag_env();
  int status = ugrep_core_usage(&env, message, arg, valid);

  // do not exit when reading a config file
  if (status >= 0)
    exit(status);

  ++Static::warnings;
}

// print a helpful information for WHAT, if specified, and exit
void help(const char *what)
{
  CoreDiagEnv env = diag_env();
  exit(ugrep_core_help(&env, what));
}

// print the version info
void version()
{
  CoreDiagEnv env = diag_env();
  exit(ugrep_core_version(&env));
}

// print to standard error: ... is a directory if -s and -q not specified
void is_directory(const char *pathname)
{
  CoreDiagEnv env = diag_env();
  ugrep_core_is_directory(&env, pathname);
}

#ifdef HAVE_LIBZ
// print to standard error: cannot decompress message if -s and -q not specified
void cannot_decompress(const char *pathname, const char *message)
{
  CoreDiagEnv env = diag_env();
  ugrep_core_cannot_decompress(&env, pathname, message);
}
#endif

// print to standard error: warning message if -s and -q not specified, display error if errno is set, like perror()
void warning(const char *message, const char *arg)
{
  int errnum = errno;
  CoreDiagEnv env = diag_env();
  ugrep_core_warning(&env, message, arg, errnum);
  ++Static::warnings;
}

// print to standard error: error message, assumes errno is set, like perror(), then exit
void error(const char *message, const char *arg)
{
  int errnum = errno;
  CoreDiagEnv env = diag_env();
  exit(ugrep_core_error(&env, message, arg, errnum));
}

// print to standard error: abort message with exception details, then exit
void abort(const char *message)
{
  CoreDiagEnv env = diag_env();
  exit(ugrep_core_abort(&env, message, NULL));
}

// print to standard error: abort message with exception details, then exit
void abort(const char *message, const std::string& what)
{
  CoreDiagEnv env = diag_env();
  exit(ugrep_core_abort(&env, message, what.c_str()));
}

// src_core seam facades (rust/src_core/SEAM.md): init(), terminal(), set_terminal_hyperlink(), set_color() and trim() are Rust

// the callback table of the Rust terminal code (rust/src_core/src/ffi/terminal.rs)
struct TerminalEnv
{
  void *ctx;
  void (*usage)(void *, const char *);
  int (*probe_output)(void *);
  void (*tree_utf8)(void *);
  void (*hyperlink_set)(void *, const char *, const char *, const char *);
  const char *never;
  const char *always;
  const char *auto_;
  const char *default_grep_colors;
  int os_win;
  int pathsep;
};

extern "C" void ugrep_core_terminal(const TerminalEnv *env);
extern "C" void ugrep_core_set_terminal_hyperlink(const TerminalEnv *env);

namespace
{

void terminal_usage(void *, const char *message)
{
  usage(message, NULL, NULL);
}

// fstat() standard output into Static::output_stat*, return non-zero when it is /dev/null
int terminal_probe_output(void *)
{
#ifndef OS_WIN_OR_MINGW

  Static::output_stat_result = fstat(STDOUT_FILENO, &Static::output_stat) == 0;
  Static::output_stat_regular = Static::output_stat_result && S_ISREG(Static::output_stat.st_mode);

  struct stat dev_null_stat;
  return Static::output_stat_result &&
    S_ISCHR(Static::output_stat.st_mode) &&
    stat("/dev/null", &dev_null_stat) == 0 &&
    Static::output_stat.st_dev == dev_null_stat.st_dev &&
    Static::output_stat.st_ino == dev_null_stat.st_ino;

#else

  return 0;

#endif
}

void terminal_tree_utf8(void *)
{
  Output::Tree::glyphs("│ ", "╰╴", "▔ ");
}

void terminal_hyperlink_set(void *, const char *host, const char *path, const char *prefix)
{
  flag_hyperlink_host.assign(host);
  flag_hyperlink_path.append(path);
  flag_hyperlink_prefix.assign(prefix);
}

void terminal_env(TerminalEnv& env)
{
  env.ctx = NULL;
  env.usage = terminal_usage;
  env.probe_output = terminal_probe_output;
  env.tree_utf8 = terminal_tree_utf8;
  env.hyperlink_set = terminal_hyperlink_set;
  env.never = Static::NEVER;
  env.always = Static::ALWAYS;
  env.auto_ = Static::AUTO;
  env.default_grep_colors = DEFAULT_GREP_COLORS;
#ifdef OS_WIN_OR_MINGW
  env.os_win = 1;
#else
  env.os_win = 0;
#endif
  env.pathsep = PATHSEPCHR;
}

} // namespace

// check TTY info and set colors
void terminal()
{
  TerminalEnv env;
  terminal_env(env);
  ugrep_core_terminal(&env);
}

// --hyperlink parsing
void set_terminal_hyperlink()
{
  TerminalEnv env;
  terminal_env(env);
  ugrep_core_set_terminal_hyperlink(&env);
}

// convert GREP_COLORS and set the color substring to the ANSI SGR codes
void set_color(const char *colors, const char *parameter, char color[COLORLEN])
{
  ugrep_rust::core_set_color(core_rust::to_slice(colors), core_rust::to_slice(parameter), rust::Slice<uint8_t>(reinterpret_cast<uint8_t*>(color), COLORLEN));
}

// trim white space from either end of the line
void trim(std::string& line)
{
  ugrep_rust::TrimRange r = ugrep_rust::core_trim(core_rust::to_slice(line));
  line.erase(r.end);
  line.erase(0, r.begin);
}
