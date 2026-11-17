/******************************************************************************\
* Copyright (c) 2023, Robert van Engelen, Genivia Inc. All rights reserved.    *
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
@file      ugrep-indexer.cpp
@brief     file system indexer for the ugrep search utility
@author    Robert van Engelen - engelen@genivia.com
@copyright (c) 2023-2026 Robert van Engelen, Genivia Inc. All rights reserved.
@copyright (c) BSD-3 License - see LICENSE.txt
*/

// DO NOT ALTER THIS LINE: updated by makemake.sh and we need it physically here for MSVC++ build from source
#define UGREP_VERSION "7.8.5"

// use a task-parallel thread to decompress the stream into a pipe to search, also handles nested archives
#define WITH_DECOMPRESSION_THREAD

// ignore hidden files and directories in archives, but ugrep will never find them anymore when searching hidden!
#define WITH_SKIP_HIDDEN_ARCHIVES

// check if we are compiling for a windows OS, but not Cygwin
#if (defined(__WIN32__) || defined(_WIN32) || defined(WIN32) || defined(_WIN64) || defined(__BORLANDC__)) && !defined(__CYGWIN__)
# if defined(__MINGW32__) || defined(__MINGW64__)
#  define OS_WIN_MINGW
# else
#  define OS_WIN
# endif
# define OS_WIN_OR_MINGW
#endif

// 64 bits off_t and fseeko
#define _FILE_OFFSET_BITS 64

// PRId64 PRIu64
#define __STDC_FORMAT_MACROS

#ifdef OS_WIN_OR_MINGW // compiling for a windows OS, but not Cygwin

// disable legacy min/max macros so we can use std::min and std::max
#ifndef NOMINMAX
# define NOMINMAX
#endif

#include <windows.h>
#include <tchar.h>
#include <stdio.h>
#include <io.h>
#include <strsafe.h>
#include <string.h>
#include <fcntl.h>
#include <direct.h>
#include <errno.h>
#include <time.h>
#include <cstdint>
#include <string>

#ifdef HAVE_CONFIG_H
# include "config.h"
#endif

// 64 bits off_t and fseeko
#define off_t int64_t
#define fseeko _fseeki64
#define ftruncate _chsize_s

#define STDIN_FILENO  0
#define STDOUT_FILENO 1
#define STDERR_FILENO 2

#ifdef OS_WIN
# define PATHSEPCHR '\\'
# define PATHSEPSTR "\\"
#else
# define PATHSEPCHR '/'
# define PATHSEPSTR "/"
#endif

#ifdef OS_WIN
# define O_RDONLY _O_RDONLY
# define O_WRONLY _O_WRONLY
#endif

// convert a UTF-8 string to a wide Unicode String
std::wstring utf8_decode(const std::string &str)
{
  if (str.empty())
    return std::wstring();
  int size = MultiByteToWideChar(CP_UTF8, 0, &str[0], static_cast<int>(str.size()), NULL, 0);
  std::wstring wstr(size, 0);
  MultiByteToWideChar(CP_UTF8, 0, &str[0], static_cast<int>(str.size()), &wstr[0], size);
  return wstr;
}

// open UTF-8 encoded Unicode filename
int fopenw_s(FILE **file, const char *filename, const char *mode)
{
  *file = NULL;
  std::wstring wfilename = utf8_decode(filename);
  HANDLE hFile;
  if (strchr(mode, 'a') == NULL && strchr(mode, 'w') == NULL)
    hFile = CreateFileW(wfilename.c_str(), (strchr(mode, '+') == NULL ? GENERIC_READ : GENERIC_READ | GENERIC_WRITE), FILE_SHARE_READ, NULL, OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN, NULL);
  else if (strchr(mode, 'a') == NULL)
    hFile = CreateFileW(wfilename.c_str(), (strchr(mode, '+') == NULL ? GENERIC_WRITE : GENERIC_READ | GENERIC_WRITE), FILE_SHARE_READ, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
  else
    hFile = CreateFileW(wfilename.c_str(), FILE_APPEND_DATA, FILE_SHARE_READ, NULL, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
  if (hFile == INVALID_HANDLE_VALUE)
    return errno = (GetLastError() == ERROR_ACCESS_DENIED ? EACCES : ENOENT);
  int fd = _open_osfhandle(reinterpret_cast<intptr_t>(hFile), O_RDONLY);
  if (fd == -1)
  {
    CloseHandle(hFile);
    return errno = EINVAL;
  }
  *file = _fdopen(fd, mode);
  if (*file == NULL)
  {
    _close(fd);
    return errno ? errno : (errno = EINVAL);
  }
  return 0;
}

#else // not compiling for a windows OS

#include <signal.h>
#include <dirent.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>
#include <cerrno>
#include <cstdlib>
#include <cstdio>
#include <cstdint>
#include <cstring>
#include <ctime>
#include <string>

#ifdef HAVE_CONFIG_H
# include "config.h"
#endif

#define PATHSEPCHR '/'
#define PATHSEPSTR "/"

// open UTF-8 encoded Unicode filename
int fopenw_s(FILE **file, const char *filename, const char *mode)
{
  *file = NULL;
#if defined(HAVE_F_RDAHEAD)
  if (strchr(mode, 'a') == NULL && strchr(mode, 'w') == NULL)
  {
#if defined(O_NOCTTY)
    int fd = open(filename, O_RDONLY | O_NOCTTY);
#else
    int fd = open(filename, O_RDONLY);
#endif
    if (fd < 0)
      return errno;
    fcntl(fd, F_RDAHEAD, 1);
    return (*file = fdopen(fd, mode)) == NULL ? errno ? errno : (errno = EINVAL) : 0;
  }
#endif
  return (*file = fopen(filename, mode)) == NULL ? errno ? errno : (errno = EINVAL) : 0;
}

#endif

// platform -- see configure.ac
#if !defined(PLATFORM)
# if defined(OS_WIN)
#  if defined(_WIN64)
#   define PLATFORM "WIN64"
#  elif defined(_WIN32)
#   define PLATFORM "WIN32"
#  else
#   define PLATFORM "WIN"
#  endif
# else
#  define PLATFORM ""
# endif
#endif

// use zlib, libbz2, liblzma for option -z
#ifdef HAVE_LIBZ
# include "zstream.hpp"
#ifdef WITH_DECOMPRESSION_THREAD
# include "zthread.hpp"
#endif
#endif

#include "reflex/input.h"
#include "glob.hpp"
#include <cctype>
#include <cinttypes>
#include <iostream>
#include <memory>
#include <vector>

// seam of the src_indexer component: index(), indexer() and deleter() are implemented in Rust (rust/src_indexer) and
// reached through the facade in src/ugrep-indexer_facade.cpp
#include "ugrep-indexer_seam.h"

// default --ignore-files=FILE argument
#define DEFAULT_IGNORE_FILE ".gitignore"

typedef std::vector<std::string> StrVec;

// fixed constant strings
static const char ugrep_index_filename[] = "._UG#_Store";
static const char ugrep_index_file_magic[5] = "UG#\x03";
static const char ugrep_indexer_config_filename[] = ".ugrep-indexer";

// command-line optional PATH argument
const char *arg_path = NULL;

// command-line options
int    flag_accuracy          = 4;     // -0 ... -9 (--accuracy) default is -4
bool   flag_check             = false; // -c (--check)
bool   flag_decompress        = false; // -z (--decompress)
bool   flag_delete            = false; // -d (--delete)
bool   flag_dereference_files = false; // -S (--dereference-files)
bool   flag_force             = false; // -f (--force)
bool   flag_hidden            = false; // -. (--hidden)
bool   flag_ignore_binary     = false; // -I (--ignore-binary)
bool   flag_no_messages       = false; // -s (--no-messages)
bool   flag_quiet             = false; // -q (--quiet)
bool   flag_usage_warnings    = false; // internal flag
bool   flag_verbose           = false; // -v (--verbose)
size_t flag_zmax              = 1;     // --zmax
StrVec flag_ignore_files;              // -X (--ignore-files)

// count warnings
size_t warnings = 0;

// Input stream to index
struct Stream {

  Stream()
    :
      file(NULL)
#ifdef HAVE_LIBZ
#ifdef WITH_DECOMPRESSION_THREAD
    , zthread(false, partname)
#else
    , is_compressed(false)
#endif
#endif
  { }

  ~Stream()
  {
    close();
    stream_release(*this);
  }

  bool open(const char *pathname)
  {
    if (file != NULL)
      fclose(file);
    return fopenw_s(&file, pathname, "rb") == 0;
  }

  void close()
  {
    // close input separately when not associated with the file
    if (input.file() != file && input.file() != NULL)
      fclose(input.file());
    input.clear();
    // close the file
    if (file != NULL)
      fclose(file);
    file = NULL;
  }

#ifdef HAVE_LIBZ

  bool read_file(const char *pathname, bool& archive)
  {
#ifdef WITH_DECOMPRESSION_THREAD

    // we don't know yet if this is an archive until we first read some data
    archive = false;

    if (flag_decompress)
    {
      // close the underlying pipe previously created with pipe() and fdopen()
      if (input.file() != NULL)
      {
        // close and unassign input
        fclose(input.file());
        input.clear();
      }

      partname.clear();

      // start decompression thread if not running, get pipe with decompressed input
      FILE *pipe_in = zthread.start(flag_zmax, pathname, file);
      if (pipe_in == NULL)
      {
        close();
        return false;
      }

      // read archive/compressed/plain data from the decompression thread chain pipe
      input = pipe_in;
    }
    else
    {
      input = file;
    }

    return true;

#else

    // non-threaded decompression, one level only and no tar/pax/cpio and no compressed utf16/32 decoding
    if (flag_decompress)
    {
      // create or open a new zstreambuf
      if (!zstream)
        zstream = std::unique_ptr<zstreambuf>(new zstreambuf(pathname, file));
      else if (!archive)
        zstream->open(pathname, file);

      is_compressed = zstream->decompressing();

      // get zip/7z info, if indexing a zip/7z archive
      const zstreambuf::ZipInfo *zipinfo = zstream->zipinfo();

      // set archive flag when indexing an archive to get the next parts
      archive = zipinfo != NULL;
      if (archive)
        partname = zipinfo->name;
      else
        partname.clear();

      // new stream with zstreambuf, delete the old
      istream = std::unique_ptr<std::istream>(new std::istream(zstream.get()));

      // utf16/32 is no longer auto-converted like FILE*, so this does not index utf16/32 files
      input = istream.get();
    }
    else
    {
      archive = false;
      input = file;
    }

    return true;

#endif
  }

  bool read_next_file(const char *pathname, bool& archive)
  {
#ifdef WITH_DECOMPRESSION_THREAD

    // -z: open next archived file if any or close the compressed file/archive
    if (flag_decompress)
    {
      // close the pipe previously created with pipe() and fdopen()
      if (input.file() != NULL)
      {
        // close and unassign input
        fclose(input.file());
        input.clear();
      }

      partname.clear();

      // open pipe to the next file or part in an archive if there is a next file
      FILE *pipe_in = zthread.open_next(pathname);
      if (pipe_in != NULL)
      {
        // assign the next extracted file as input to search
        input = pipe_in;

        // start searching the next file in the archive
        return true;
      }

      // if not extracting an archive, then read the next file
      if (!archive)
        return read_file(pathname, archive);

      // no more archive parts to extract
      return false;
    }

    archive = false;
    input = file;

    return true;

#else

    // -z: open next archived file if any or close the compressed file/archive
    if (flag_decompress)
    {
      partname.clear();

      if (archive)
      {
        const zstreambuf::ZipInfo *zipinfo = zstream->zipinfo();
        // if no more archive parts to extract, then return false
        if (zipinfo == NULL)
          return false;
      }
    }

    return read_file(pathname, archive);

#endif
  }

  // return true if decompressing a file in any of the decompression chain stages
  bool decompressing()
  {
#ifdef WITH_DECOMPRESSION_THREAD
    return zthread.decompressing();
#else
    return is_compressed;
#endif
  }

#endif

  FILE *file;
  reflex::Input input;
  std::string partname;

  // handle of the Rust-owned stream state, 0 when none
  uint64_t rust_handle = 0;

#ifdef HAVE_LIBZ
#ifdef WITH_DECOMPRESSION_THREAD
  Zthread zthread;
#else
  bool is_compressed;
  std::unique_ptr<zstreambuf> zstream;
  std::unique_ptr<std::istream> istream;
#endif
#endif
};

// the Rust handle slot of a stream, used by the facade in src/ugrep-indexer_facade.cpp
uint64_t& stream_rust_handle(Stream& stream)
{
  return stream.rust_handle;
}

// the partname slot of a stream, used by the facade
std::string& stream_partname(Stream& stream)
{
  return stream.partname;
}

// display the version info and exit
void version()
{
  std::cout << "ugrep-indexer " UGREP_VERSION " " PLATFORM "\n"
    "License: BSD-3-Clause; ugrep user manual: <https://ugrep.com>\n"
    "Written by Robert van Engelen and others: <https://github.com/Genivia/ugrep>\n"
    "Ugrep utilizes the RE/flex regex library: <https://github.com/Genivia/RE-flex>" << std::endl;
  exit(EXIT_SUCCESS);
}

// display a help message and exit
void help()
{
  std::cout << "\nUsage:\n\nugrep-indexer [-0|...|-9] [-.] [-c|-d|-f] [-I] [-q] [-S] [-s] [-X] [-z] [PATH]\n\n\
    Updates indexes incrementally unless option -f or --force is specified.\n\
    \n\
    When option -I or --ignore-binary is specified, binary files are ignored\n\
    and not indexed.  Searching with ugrep --index still searches binary files\n\
    unless ugrep option -I or --ignore-binary is specified also.\n\
    \n\
    Archives and compressed files are indexed when option -z or --decompress is\n\
    specified.  Otherwise, archives and compressed files are indexed as binary\n\
    files, or are ignored with option -I or --ignore-binary.\n\
    \n\
    To save a log file, specify option -v or --verbose and redirect standard\n\
    output to a log file.  All messages are sent to standard output.\n\
    \n\
    A .ugrep-indexer configuration file with configuration options is loaded\n\
    when present in the working directory or in the home directory.  A\n\
    configuration option consists of the name of a long option and its argument\n\
    when applicable.\n\
    \n\
    The following options are available:\n\
    \n\
    PATH    Optional pathname to the root of the directory tree to index.  The\n\
            default is to recursively index the working directory tree.\n\n\
    -0, -1, -2, -3, ..., -9, --accuracy=DIGIT\n\
            Specifies indexing accuracy.  A low accuracy reduces the indexing\n\
            storage overhead at the cost of a higher rate of false positive\n\
            pattern matches (more noise).  A high accuracy reduces the rate of\n\
            false positive regex pattern matches (less noise) at the cost of an\n\
            increased indexing storage overhead.  An accuracy between 2 and 7\n\
            is recommended.  The default accuracy is 4.\n\
    -., --hidden\n\
            Index hidden files and directories.\n\
    -?, --help\n\
            Display a help message and exit.\n\
    -c, --check\n\
            Recursively check and report indexes without reindexing files.\n\
    -d, --delete\n\
            Recursively remove index files.\n\
    -f, --force\n\
            Force reindexing of files, even those that are already indexed.\n\
    -I, --ignore-binary\n\
            Do not index binary files.\n\
    -q, --quiet, --silent\n\
            Quiet mode: do not display indexing statistics.\n\
    -S, --dereference-files\n\
            Follow symbolic links to files.  Symbolic links to directories are\n\
            never followed.\n\
    -s, --no-messages\n\
            Silent mode: nonexistent and unreadable files are ignored, i.e.\n\
            their error messages and warnings are suppressed.\n\
    -V, --version\n\
            Display version and exit.\n\
    -v, --verbose\n\
            Produce verbose output.  Files are marked A for archive, C for\n\
            compressed, and B for binary or I for ignored binary.  Deletions\n\
            are marked D.\n\
    -X, --ignore-files, --ignore-files=FILE\n\
            Do not index files and directories matching the globs in FILE\n\
            encountered during indexing.  The default FILE is `" DEFAULT_IGNORE_FILE "'.\n\
            This option may be repeated to specify additional files.\n\
    -z, --decompress\n\
            Index the contents of compressed files and archives.  Hidden files\n\
            in archives are ignored unless option -. or --hidden is specified.\n\
            Option -I or --ignore-binary ignores compressed binary files.  When\n\
            used with option --zmax=NUM, indexes the contents of compressed\n\
            files and archives stored within archives up to NUM levels deep.\n"
#ifndef HAVE_LIBZ
            "\
            This option is not available in this build of ugrep-indexer.\n"
#else
            "\
            Supported compression formats: gzip (.gz), compress (.Z), zip"
#ifndef WITH_NO_7ZIP
            ", 7z"
#endif
#ifdef HAVE_LIBBZ2
            ",\n\
            bzip2 (requires suffix .bz, .bz2, .bzip2, .tbz, .tbz2, .tb2, .tz2)"
#endif
#ifdef HAVE_LIBLZMA
            ",\n\
            lzma and xz (requires suffix .lzma, .tlz, .xz, .txz)"
#endif
#ifdef HAVE_LIBLZ4
            ",\n\
            lz4 (requires suffix .lz4)"
#endif
#ifdef HAVE_LIBZSTD
            ",\n\
            zstd (requires suffix .zst, .zstd, .tzst)"
#endif
#ifdef HAVE_LIBBROTLI
            ",\n\
            brotli (requires suffix .br)"
#endif
#ifdef HAVE_LIBBZIP3
            ",\n\
            bzip3 (requires suffix .bz3)"
#endif
            ".\n"
#endif
            "\
    --zmax=NUM\n\
            When used with option -z (--decompress), indexes the contents of\n\
            compressed files and archives stored within archives by up to NUM\n\
            expansion levels deep.  The default --zmax=1 only permits indexing\n\
            uncompressed files stored in cpio, pax, tar, zip and 7z archives;\n\
            compressed files and archives are detected as binary files and are\n\
            effectively ignored.  Specify --zmax=2 to index compressed files\n\
            and archives stored in cpio, pax, tar, zip and 7z archives.  NUM\n\
            may range from 1 to 99 for up to 99 decompression and de-archiving\n\
            steps.  Increasing NUM values gradually degrades performance.\n"
#ifndef WITH_DECOMPRESSION_THREAD
            "\
            This option is not available in this build configuration of ugrep.\n"
#endif
"\n\
    The ugrep-indexer utility exits with one of the following values:\n\
    0      Indexes are up to date.\n\
    1      Indexing check -c detected missing and outdated index files.\n\
\n";

  exit(EXIT_SUCCESS);
}

// display usage information and exit
void usage(const char *message, const char *arg = NULL)
{
  if (!flag_usage_warnings)
  {
    std::cerr << "ugrep-indexer: " << message << (arg != NULL ? arg : "") << '\n';
    help();
  }
  else
  {
    ++warnings;
    std::cerr << "ugrep-indexer: " << ugrep_indexer_config_filename << " configuration file: " << message << (arg != NULL ? arg : "") << '\n';
  }
}

#ifdef HAVE_LIBZ

// decompression diagnostics reported by the zstream adapter (src/zstream.cpp)
void cannot_decompress(const char *pathname, const char *message)
{
  ++warnings;
  if (flag_no_messages)
    return;
  printf("ugrep-indexer: warning: cannot decompress %s: %s\n", pathname, message != NULL ? message : "");
  fflush(stdout);
}

void warning(const char *message, const char *arg)
{
  ++warnings;
  if (flag_no_messages)
    return;
  printf("ugrep-indexer: warning: %s%s%s\n", message, arg != NULL ? " " : "", arg != NULL ? arg : "");
  fflush(stdout);
}

#endif

// convert unsigned decimal to non-negative size_t, produce error when conversion fails
size_t strtonum(const char *string, const char *message)
{
  char *rest = NULL;
  size_t size = static_cast<size_t>(strtoull(string, &rest, 10));
  if (rest == NULL || *rest != '\0')
    usage(message, string);
  return size;
}

// convert unsigned decimal to positive size_t, produce error when conversion fails or when the value is zero
size_t strtopos(const char *string, const char *message)
{
  size_t size = strtonum(string, message);
  if (size == 0)
    usage(message, string);
  return size;
}

// trim white space from either end of the line
inline void trim(std::string& line)
{
  size_t len = line.length();
  size_t pos;

  for (pos = 0; pos < len && isspace(line.at(pos)); ++pos)
    continue;

  if (pos > 0)
    line.erase(0, pos);

  len -= pos;

  for (pos = len; pos > 0 && isspace(line.at(pos - 1)); --pos)
    continue;

  if (len > pos)
    line.erase(pos, len - pos);
}

// read a line from buffered input, returns true when eof
inline bool getline(reflex::BufferedInput& input, std::string& line)
{
  int ch;

  line.erase();

  while ((ch = input.get()) != EOF && ch != '\n')
    line.push_back(ch);

  if (!line.empty() && line.back() == '\r')
    line.pop_back();

  return ch == EOF && line.empty();
}

// parse the command-line options
void options_cpp(int argc, const char **argv)
{
  bool options = true;

  for (int i = 1; i < argc; ++i)
  {
    const char *arg = argv[i];

    if ((*arg == '-'
#ifdef OS_WIN
         || *arg == '/'
#endif
        ) && arg[1] != '\0' && options)
    {
      bool is_grouped = true;

      // parse a ugrep command-line option
      while (is_grouped && *++arg != '\0')
      {
        switch (*arg)
        {
          case '-':
            is_grouped = false;
            if (*++arg == '\0')
            {
              options = false;
              continue;
            }

            if (strncmp(arg, "accuracy=", 9) == 0 && isdigit(arg[9]))
              flag_accuracy = arg[9] - '0';
            else if (strcmp(arg, "check") == 0)
              flag_check = true;
            else if (strcmp(arg, "decompress") == 0)
              flag_decompress = true;
            else if (strcmp(arg, "delete") == 0)
              flag_delete = true;
            else if (strcmp(arg, "dereference-files") == 0)
              flag_dereference_files = true;
            else if (strcmp(arg, "force") == 0)
              flag_force = true;
            else if (strcmp(arg, "help") == 0)
              help();
            else if (strcmp(arg, "hidden") == 0)
              flag_hidden = true;
            else if (strcmp(arg, "ignore-binary") == 0)
              flag_ignore_binary = true;
            else if (strcmp(arg, "ignore-files") == 0)
              flag_ignore_files.emplace_back(DEFAULT_IGNORE_FILE);
            else if (strncmp(arg, "ignore-files=", 13) == 0)
              flag_ignore_files.emplace_back(arg + 13);
            else if (strcmp(arg, "no-messages") == 0)
              flag_no_messages = true;
            else if (strcmp(arg, "quiet") == 0)
              flag_quiet = flag_no_messages = true;
            else if (strcmp(arg, "silent") == 0)
              flag_quiet = flag_no_messages = true;
            else if (strcmp(arg, "verbose") == 0)
              flag_verbose = true;
            else if (strcmp(arg, "version") == 0)
              version();
            else if (strncmp(arg, "zmax=", 5) == 0)
              flag_zmax = strtopos(arg + 5, "invalid argument --zmax=");
            else
              usage("invalid option --", arg);

            break;

          case 'c':
            flag_check = true;
            break;

          case 'd':
            flag_delete = true;
            break;

          case 'f':
            flag_force = true;
            break;

          case 'I':
            flag_ignore_binary = true;
            break;

          case 'q':
            flag_quiet = flag_no_messages = true;
            break;

          case 'S':
            flag_dereference_files = true;
            break;

          case 's':
            flag_no_messages = true;
            break;

          case 'V':
            version();
            break;

          case 'v':
            flag_verbose = true;
            break;

          case 'z':
            flag_decompress = true;
            break;

          case '.':
            flag_hidden = true;
            break;

          case 'X':
            flag_ignore_files.emplace_back(DEFAULT_IGNORE_FILE);
            break;

          case '?':
            help();
            break;

          default:
            if (isdigit(*arg))
              flag_accuracy = *arg - '0';
            else
              usage("invalid option -", arg);
        }
      }
    }
    else if (arg_path == NULL)
    {
      arg_path = arg;
    }
    else
    {
      usage("argument PATH already specified as ", arg_path);
    }
  }

  // -q overrides -v
  if (flag_quiet)
    flag_verbose = false;

  // -c silently overrides -d and -f
  if (flag_check)
    flag_delete = flag_force = false;

  // -d silently overrides -f
  if (flag_delete)
    flag_force = false;

#ifndef HAVE_LIBZ
  if (flag_decompress)
    usage("Option -z (--decompress) is not available");
#endif

#ifdef WITH_DECOMPRESSION_THREAD
  // --zmax: NUM argument exceeds limit?
  if (flag_zmax > 99)
    usage("option --zmax argument exceeds upper limit");
#else
  if (flag_zmax > 1)
    usage("Option --zmax is not available");
#endif
}

// load .ugrep-indexer config file when present in the working or home directory
void load_config(const char *config_filename)
{
  // open a local config file or in the home directory
  FILE *file = NULL;
  if (fopenw_s(&file, config_filename, "r") != 0)
  {
#ifdef OS_WIN
    const char *home_dir = getenv("USERPROFILE");
#else
    const char *home_dir = getenv("HOME");
#endif
    file = NULL;
    if (home_dir != NULL)
    {
      // open a config file in the home directory
      std::string config_filepath;
      config_filepath.assign(home_dir).append(PATHSEPSTR).append(config_filename);
      if (fopenw_s(&file, config_filepath.c_str(), "r") != 0)
        file = NULL;
    }
  }

  // parse config file when present
  if (file != NULL)
  {
    reflex::BufferedInput input(file);
    std::string line;

    // warn about invalid options but do not exit
    flag_usage_warnings = true;

    while (true)
    {
      // read the next line
      if (getline(input, line))
        break;

      trim(line);

      // parse option or skip empty lines and comments
      if (!line.empty() && line.front() != '#')
      {
        // construct an option argument to parse as argv[]
        line.insert(0, "--");
        const char *arg = line.c_str();
        const char *args[2] = { NULL, arg };

        options(2, args);
      }
    }

    // bail out when config file has errors
    if (warnings > 0)
      exit(EXIT_FAILURE);

    // reset flag
    flag_usage_warnings = false;

    fclose(file);
  }
}

// where the magic happens
int main(int argc, const char **argv)
{
#if !defined(OS_WIN_OR_MINGW) && defined(HAVE_LIBZ) && defined(WITH_DECOMPRESSION_THREAD)
  // ignore SIGPIPE
  signal(SIGPIPE, SIG_IGN);
#endif

  load_config(ugrep_indexer_config_filename);

  options(argc, argv);

  if (flag_delete)
    deleter(arg_path);
  else
    indexer(arg_path);

  return EXIT_SUCCESS;
}
