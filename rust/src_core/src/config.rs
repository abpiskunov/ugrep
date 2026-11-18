//! `load_config` and `save_config` of src/ugrep.cpp on the S1 settings struct. Everything that touches the
//! operating system or the C++-owned containers (opening and reading files, the home directory, `warning`,
//! `error`, the `std::string` and `std::set` options) is behind [`ConfigEnv`]; the parsing and writing logic
//! is here. Bytes are written exactly as the C++ `fprintf` calls did.

use crate::options::{List, Z};
use crate::settings::Settings;
use std::os::raw::c_char;

/// Build-dependent values of the C++ side.
#[derive(Clone, Copy)]
pub struct ConfigConsts {
    /// `DEFAULT_QUERY_DELAY`
    pub default_query_delay: usize,
    /// `PATHSEPCHR`
    pub path_sep: u8,
    /// `WITH_XDG_CONFIG_HOME` and not `OS_WIN`
    pub xdg: bool,
}

/// Where a message goes: `std::cerr` or `Static::errout`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sink {
    Cerr,
    Errout,
}

/// What `load_config` and `save_config` need from the retained C++ side, on top of the options [`Env`].
pub trait ConfigEnv {
    type File: Copy;

    /// `usage(message, arg, NULL)`; returns when `flag_usage_warnings` is set.
    fn usage(&mut self, message: &str, arg: Option<&[u8]>, valid: Option<&str>);
    fn list_clear(&mut self, list: List);
    fn list_empty(&mut self, list: List) -> bool;
    fn filter_clear(&mut self);
    /// The bytes of a C string.
    fn c_string(&mut self, p: *const c_char) -> Vec<u8>;

    /// `fopen_smart(&file, path, "r")` (and the owner check of the working directory file when `check_owner`).
    fn open(&mut self, path: Z<'_>, check_owner: bool) -> Option<Self::File>;
    /// Read and trim the next line of a config file; false at end of file.
    fn next_line(&mut self, file: Self::File) -> bool;
    /// The trimmed line, without NUL.
    fn line_text(&mut self, file: Self::File) -> Vec<u8>;
    /// Insert `--` in front of the line and return its C string; the memory lives until `close`.
    fn prefix_line(&mut self, file: Self::File) -> *const c_char;
    fn file_error(&mut self, file: Self::File) -> bool;
    /// `fclose` unless standard input
    fn close(&mut self, file: Self::File);

    fn files_contains(&mut self, path: Z<'_>) -> bool;
    fn files_insert(&mut self, path: Z<'_>);
    fn files_len(&mut self) -> usize;
    fn files_get(&mut self, index: usize) -> Vec<u8>;

    fn home_dir(&mut self) -> Option<Vec<u8>>;

    fn warnings_reset(&mut self);
    fn warnings(&mut self) -> usize;
    fn write(&mut self, sink: Sink, bytes: &[u8]);
    fn error(&mut self, message: &str, arg: Z<'_>);
    fn warning(&mut self, message: &str, arg: Z<'_>);
    /// `exit(EXIT_ERROR)`
    fn exit_error(&mut self) -> !;

    /// `options(pattern_args, 2, {NULL, arg})`
    fn parse_option(&mut self, settings: &mut Settings, arg: *const c_char);

    fn regexp_clear(&mut self);
    fn filter(&mut self) -> Vec<u8>;
    fn list_len(&mut self, list: List) -> usize;
    fn list_get(&mut self, list: List, index: usize) -> Vec<u8>;

    /// `std::rename(path, old)`; on success sets `errno = EEXIST` and warns "saved old configuration file to".
    fn rename_old(&mut self, path: Z<'_>, old: Z<'_>) -> bool;
    /// `fopen_smart(&file, path, "w")`
    fn save_open(&mut self, path: Z<'_>) -> Option<Self::File>;
    fn save_write(&mut self, file: Self::File, bytes: &[u8]);
    fn save_error(&mut self, file: Self::File) -> bool;
    /// `fclose` unless standard output
    fn save_close(&mut self, file: Self::File);
}

fn nz(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

fn text(env: &mut impl ConfigEnv, p: *const c_char) -> Option<Vec<u8>> {
    if p.is_null() {
        None
    } else {
        Some(env.c_string(p))
    }
}

/// `load_config(pattern_args, recurse)`
pub fn load_config<E: ConfigEnv>(s: &mut Settings, env: &mut E, k: ConfigConsts, recurse: bool) {
    let flag_config = text(env, s.flag_config).unwrap_or_default();
    let flag_config = if flag_config.is_empty() {
        s.flag_config = c".ugrep".as_ptr();
        b".ugrep".to_vec()
    } else {
        flag_config
    };

    // if the config file was parsed before, then only try parsing the home dir config file
    let home = env.files_contains(&nz(&flag_config));
    let mut wdir = !home;

    let mut config_file = flag_config.clone();
    let mut file: Option<E::File> = None;

    if wdir {
        file = env.open(&nz(&flag_config), true);
    }

    if file.is_none() {
        wdir = false;

        if let Some(home_dir) = env.home_dir() {
            if flag_config[0] != b'~' && flag_config[0] != k.path_sep {
                config_file = home_dir;
                config_file.push(k.path_sep);
                config_file.extend_from_slice(&flag_config);
                file = if env.files_contains(&nz(&config_file)) { None } else { env.open(&nz(&config_file), false) };
            }
        }

        if k.xdg && file.is_none() && flag_config == b".ugrep" {
            let mut dir = std::env::var_os("XDG_CONFIG_HOME").map(|v| v.into_encoded_bytes()).unwrap_or_default();
            if dir.is_empty() {
                dir = b".config".to_vec();
            }
            config_file = dir;
            for part in [&[k.path_sep][..], b"ugrep", &[k.path_sep][..], b"config"] {
                config_file.extend_from_slice(part);
            }
            file = if env.files_contains(&nz(&config_file)) { None } else { env.open(&nz(&config_file), false) };
        }
    }

    if let Some(file) = file {
        env.files_insert(&nz(&config_file));

        let mut lineno: usize = 1;
        let mut errors = false;

        while env.next_line(file) {
            let line = env.line_text(file);

            // parse option or skip empty lines and comments
            if !line.is_empty() && line[0] != b'#' {
                let arg = env.prefix_line(file);
                let line = env.c_string(arg);

                env.warnings_reset();

                // warn about invalid options but do not exit
                s.flag_usage_warnings = true;

                env.parse_option(s, arg);

                if env.warnings() > 0
                    || s.flag_bool
                    || s.flag_fuzzy != 0
                    || s.flag_mode != 0
                    || s.flag_any_line
                    || s.flag_count
                    || s.flag_files_with_matches
                    || s.flag_files_without_match
                    || s.flag_invert_match
                    || s.flag_line_regexp
                    || s.flag_only_line_number
                    || s.flag_only_matching
                    || !s.flag_replace.is_null()
                    || s.flag_text == 1
                    || s.flag_word_regexp
                {
                    // reject invalid options and options that don't make sense for config
                    let mut msg = b"ugrep: error in config file ".to_vec();
                    msg.extend_from_slice(&config_file);
                    msg.extend_from_slice(format!(" at line {lineno}\n").as_bytes());
                    env.write(Sink::Cerr, &msg);
                    errors = true;
                } else if line.starts_with(b"--config") {
                    // parse a config file, but do not recurse more than one level deep
                    if recurse {
                        let mut msg = b"ugrep: error in config file ".to_vec();
                        msg.extend_from_slice(&config_file);
                        msg.extend_from_slice(format!(" recursion at line {lineno}\n").as_bytes());
                        env.write(Sink::Cerr, &msg);
                        errors = true;
                    } else {
                        let this_config = s.flag_config;
                        let this_file = std::mem::take(&mut config_file);

                        s.flag_config = if line.len() == 8 { std::ptr::null() } else { arg.wrapping_add(9) };
                        load_config(s, env, k, true);

                        config_file = this_file;
                        s.flag_config = this_config;
                    }
                }
            }

            lineno += 1;
        }

        // silently ignore some options that don't make sense for config, clashing and confusing to allow
        env.list_clear(List::File);
        env.list_clear(List::From);
        s.flag_match = false;
        s.flag_max_count = 0;
        s.flag_max_files = 0;
        s.flag_max_line = 0;
        s.flag_max_size = 0;
        s.flag_min_line = 0;
        s.flag_min_size = 0;
        s.flag_pager = std::ptr::null();
        s.flag_query = false;
        s.flag_quiet = false;
        env.regexp_clear();
        s.flag_save_config = std::ptr::null();
        if wdir {
            s.flag_view = std::ptr::null();
            env.filter_clear();
            env.list_clear(List::FilterMagicLabel);
        }

        if env.file_error(file) {
            env.error("error while reading", &nz(&config_file));
        }

        env.close(file);

        if errors {
            env.exit_error();
        }
    } else if flag_config != b".ugrep" {
        env.error("option --config: cannot read", &nz(&flag_config));
    }

    s.flag_usage_warnings = false;
}

/// `fprintf(file, "...%s...")` of an optional C string
fn cs(env: &mut impl ConfigEnv, p: *const c_char) -> Vec<u8> {
    text(env, p).unwrap_or_default()
}

fn put(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
}

fn put_b(out: &mut Vec<u8>, s: &[u8]) {
    out.extend_from_slice(s);
}

/// `fprintf(file, "<comment>\n%s<name>\n\n", flag.is_undefined() ? "# " : flag ? "" : "no-")`
fn tri_prefix(v: i32) -> &'static str {
    if v < 0 {
        "# "
    } else if v == 1 {
        ""
    } else {
        "no-"
    }
}

/// A list option as a section: a comment line, `name=value` lines and an empty line.
fn put_list<E: ConfigEnv>(env: &mut E, out: &mut Vec<u8>, list: List, comment: &str, name: &str) {
    let n = env.list_len(list);
    if n == 0 {
        return;
    }
    put(out, comment);
    for i in 0..n {
        let v = env.list_get(list, i);
        put(out, name);
        put(out, "=");
        put_b(out, &v);
        put(out, "\n");
    }
    put(out, "\n");
}

const HEADER: &str = "\
#\n\
# A long option is defined per line with an optional `=' and its argument,\n\
# when applicable.  Options that change patterns and search modes are ignored.\n\
# Empty lines and lines starting with a `#' are ignored.\n\
#\n\
# Try `ug --help' or `ug --help WHAT' for help with options.\n\n";

const COLORS_HELP: &str = "\
# The argument is a colon-separated list of one or more parameters `sl='\n\
# (selected line), `cx=' (context line), `mt=' (matched text), `ms=' (match\n\
# selected), `mc=' (match context), `fn=' (file name), `ln=' (line number),\n\
# `cn=' (column number), `bn=' (byte offset), `se=' (separator), `qp=' (TUI\n\
# prompt), `qe=' (TUI errors), `qr=' (TUI regex), `qm=' (TUI regex meta\n\
# characters), `ql=' (TUI regex lists and literals), `qb=' (TUI regex braces).\n\
# Parameter values are ANSI SGR color codes or `k' (black), `r' (red), `g'\n\
# (green), `y' (yellow), `b' (blue), `m' (magenta), `c' (cyan), `w' (white), or\n\
# leave empty for no color.\n\
# Upper case specifies background colors.\n\
# A `+' qualifies a color as bright.\n\
# A foreground and a background color may be combined with font properties `n'\n\
# (normal), `f' (faint), `h' (highlight), `i' (invert), `u' (underline).\n\
# Parameter `hl' enables file name hyperlinks (same as --hyperlink).\n\
# Parameter `rv' reverses the `sl=' and `cx=' parameters when option -v is\n\
# used.\n\
#\n\
# The ugrep default color scheme:\n\
#   colors=cx=33:mt=1;31:fn=1;35:ln=1;32:cn=1;32:bn=1;32:se=36:qp=1;32:qe=1;37;41:qm=1;32:ql=36:qb=1;35\n\
# The GNU grep and ripgrep default color scheme:\n\
#   colors=sl=37:cx=33:mt=1;31:fn=35:ln=32:cn=32:bn=32:se=36:qp=1;32:qe=1;37;41:qm=1;32:ql=36:qb=1;35\n\
# The silver searcher default color scheme:\n\
#   colors=mt=30;43:fn=1;32:ln=1;33:cn=1;33:bn=1;33:qp=1;32:qe=1;37;41:qm=1;32:ql=36:qb=1;35\n\
# Underlined bright green matches with shaded background on bright selected lines:\n\
#   colors=sl=1:cx=33:ms=1;4;32;100:mc=1;4;32:fn=1;32;100:ln=1;32:cn=1;32:bn=1;32:se=36:qp=1;32:qe=1;37;41:qm=1;32:ql=36:qb=1;35\n\
# Inverted bright yellow matches and TUI regex syntax highlighting with background colors:\n\
#   colors=cx=hb:ms=hiy:mc=hic:fn=hi+y+K:ln=hg:cn=hg:bn=hg:se=c:gp=hg:qr=hwB:qm=hwG:ql=hwC:qb=hwM\n\
# Only change the TUI regex syntax highlighting to use background colors:\n\
#   colors=gp=hg:qr=hwB:qm=hwG:ql=hwC:qb=hwM\n\n";

/// The text of the configuration file that `save_config` writes.
fn render<E: ConfigEnv>(s: &Settings, env: &mut E, k: ConfigConsts, save_config: &[u8]) -> Vec<u8> {
    let mut f: Vec<u8> = Vec::with_capacity(8192);
    let out = &mut f;
    let pretty = !s.flag_pretty.is_null();

    if save_config == b".ugrep" {
        put(out, "# ugrep configuration used by ug and ugrep --config.\n");
    } else if save_config == b"-" {
        put(out, "# ugrep configuration\n");
    } else {
        put(out, "# ugrep configuration used with --config=");
        put_b(out, save_config);
        put(out, " or ---");
        put_b(out, save_config);
        put(out, ".\n");
    }

    put(out, HEADER);
    put(out, "### TERMINAL DISPLAY ###\n\n");
    put(out, "# Custom color scheme, overrides default GREP_COLORS parameters\n");
    if let Some(colors) = text(env, s.flag_colors) {
        put(out, "colors=");
        put_b(out, &colors);
        put(out, "\n");
    } else {
        put(out, "# colors=\n");
    }
    put(out, COLORS_HELP);

    put(out, "# Enable color output to the terminal\n");
    put(out, if s.flag_color.is_null() { "no-color" } else { "color" });
    put(out, "\n\n");
    if let Some(hyperlink) = text(env, s.flag_hyperlink) {
        put(out, "# Enable hyperlinks in color output\n");
        if hyperlink.is_empty() {
            put(out, "hyperlink\n\n");
        } else {
            put(out, "hyperlink=");
            put_b(out, &hyperlink);
            put(out, "\n\n");
        }
    }
    put(out, "# Enable query TUI confirmation prompts, default: confirm\n");
    put(out, if s.flag_confirm { "# " } else { "" });
    put(out, "no-confirm\n\n");
    put(out, "# Split query TUI screen on startup, default: no-split\n");
    put(out, if s.flag_split { "" } else { "# " });
    put(out, "split\n\n");
    put(out, "# Default query TUI response delay in units of 100ms, default: delay=4\n");
    if s.flag_delay == k.default_query_delay {
        put(out, "# delay=4\n\n");
    } else {
        put(out, &format!("delay={}\n\n", s.flag_delay));
    }

    put(out, "# Enable query TUI file viewing command with CTRL-Y or F2, default: view\n");
    match text(env, s.flag_view) {
        Some(view) if view.is_empty() => put(out, "# view=less\n\n"),
        Some(view) => {
            put(out, "view=");
            put_b(out, &view);
            put(out, "\n\n");
        }
        None => put(out, "no-view\n\n"),
    }

    put(out, "# Enable pretty output to the terminal, default: pretty\n");
    put(out, if pretty { "pretty" } else { "no-pretty" });
    put(out, "\n\n");

    put(out, "# Enable directory tree output to the terminal for -l (--files-with-matches) and -c (--count)\n");
    put(out, if s.flag_tree == 1 { "tree" } else { "no-tree" });
    put(out, "\n\n");

    let differs = |v: i32| v >= 0 && (v == 1) != pretty;
    if differs(s.flag_heading) {
        put(out, "# Enable headings (enabled with --pretty)\n");
        put(out, if s.flag_heading == 1 { "heading" } else { "no-heading" });
        put(out, "\n\n");
    }
    if differs(s.flag_break) {
        put(out, "# Enable break after matching files (enabled with --pretty)\n");
        put(out, if s.flag_break == 1 { "break" } else { "no-break" });
        put(out, "\n\n");
    }
    if differs(s.flag_initial_tab) {
        put(out, "# Enable initial tab (enabled with --pretty)\n");
        put(out, if s.flag_initial_tab == 1 { "initial-tab" } else { "no-initial-tab" });
        put(out, "\n\n");
    }
    if differs(s.flag_line_number) {
        put(out, "# Enable line numbers (enabled with --pretty)\n");
        put(out, if s.flag_line_number == 1 { "line-number" } else { "no-line-number" });
        put(out, "\n\n");
    }
    if s.flag_column_number >= 0 {
        put(out, "# Enable column numbers\n");
        put(out, if s.flag_column_number == 1 { "column-number" } else { "no-column-number" });
        put(out, "\n\n");
    }
    if s.flag_byte_offset >= 0 {
        put(out, "# Enable byte offsets\n");
        put(out, if s.flag_byte_offset == 1 { "byte-offset" } else { "no-byte-offset" });
        put(out, "\n\n");
    }

    let binary_files = cs(env, s.flag_binary_files);
    if binary_files == b"hex" {
        put(out, "# Hex output\nhex\n\n");
    } else if binary_files == b"with-hex" {
        put(out, "# Output with hex for binary matches\nwith-hex\n\n");
    }

    if let Some(hexdump) = text(env, s.flag_hexdump) {
        put(out, "# Hex dump (columns, no space breaks, no character column, no hex spacing)\nhexdump=");
        put_b(out, &hexdump);
        put(out, "\n\n");
    }

    match text(env, s.flag_group_separator) {
        None => put(out, "# Disable group separator for contexts\nno-group-separator\n\n"),
        Some(sep) if sep != b"--" => {
            put(out, "# Group separator for contexts\ngroup-separator=");
            put_b(out, &sep);
            put(out, "\n\n");
        }
        Some(_) => {}
    }

    put(out, "### SEARCH PATTERNS ###\n\n");

    put(out, "# Enable case-insensitive search, default: no-ignore-case\n");
    put(out, tri_prefix(s.flag_ignore_case));
    put(out, "ignore-case\n\n");
    put(out, "# Enable smart case, default: no-smart-case\n");
    put(out, tri_prefix(s.flag_smart_case));
    put(out, "smart-case\n\n");
    put(out, "# Enable empty pattern matches, default: no-empty\n");
    put(out, tri_prefix(s.flag_empty));
    put(out, "empty\n\n");
    put(out, "# Force option -c (--count) to return nonzero matches with --min-count=1, default: --min-count=0\n");
    if s.flag_min_count == 0 {
        put(out, "# min-count=1\n\n");
    } else {
        put(out, &format!("min-count={}\n\n", s.flag_min_count));
    }

    put(out, "### SEARCH TARGETS ###\n\n");

    put(out, "# Case-insensitive glob matching, default: no-glob-ignore-case\n");
    put(out, if s.flag_glob_ignore_case { "" } else { "# " });
    put(out, "glob-ignore-case\n\n");
    put(out, "# Search hidden files and directories, default: no-hidden\n");
    put(out, if s.flag_hidden { "" } else { "# " });
    put(out, "hidden\n\n");
    put(out, "# Ignore binary files, default: no-ignore-binary\n");
    put(out, if binary_files == b"without-match" { "" } else { "# " });
    put(out, "ignore-binary\n\n");

    put_list(env, out, List::IncludeFs, "# Include specific file systems only\n", "include-fs");
    put_list(env, out, List::ExcludeFs, "# Exclude specific file systems\n", "exclude-fs");
    put_list(env, out, List::IncludeDir, "# Include specific directories only\n", "include-dir");
    put_list(env, out, List::ExcludeDir, "# Exclude specific directories\n", "exclude-dir");
    put_list(env, out, List::Include, "# Include specific files only\n", "include");
    // the C++ writes `exclude-dir=` for the excluded files (kept)
    put_list(env, out, List::Exclude, "# Exclude specific files\n", "exclude-dir");

    put(out, "# Enable decompression and archive search, default: no-decompress\n");
    put(out, if s.flag_decompress { "" } else { "# " });
    put(out, "decompress\n\n");
    put(out, "# Maximum decompression and de-archiving nesting levels, default: zmax=1\n");
    put(out, &format!("zmax={}\n\n", s.flag_zmax));
    if s.flag_dereference {
        put(out, "# Dereference symlinks, default: no-dereference\ndereference\n\n");
    } else if s.flag_dereference_files {
        put(out, "# Dereference symlinks to files, not directories, default: no-dereference-files\ndereference-files\n\n");
    }
    put(out, "# Search devices, default: devices=skip\n");
    match text(env, s.flag_devices) {
        None => put(out, "# devices=skip\n\n"),
        Some(devices) => {
            put(out, "devices=");
            put_b(out, &devices);
            put(out, "\n\n");
        }
    }
    let directories = text(env, s.flag_directories);
    if directories.as_deref().is_none_or(|d| d == b"read") {
        put(out, "# Warn when searching directories specified on the command line (like grep) with directories=read\n");
        put(out, if directories.is_none() { "# " } else { "" });
        put(out, "directories=read\n\n");
    }
    if s.flag_max_depth > 0 {
        put(
            out,
            &format!(
                "# Recursively search directories up to {0} levels deep\nmax-depth={0}\n\n",
                s.flag_max_depth
            ),
        );
    }
    put(out, "# Ignore files and directories specified in .gitignore, default: no-ignore-files\n");
    let n = env.list_len(List::IgnoreFiles);
    if n == 0 {
        put(out, "# ignore-files\n\n");
    } else {
        for i in 0..n {
            let v = env.list_get(List::IgnoreFiles, i);
            put(out, "ignore-files=");
            put_b(out, &v);
            put(out, "\n");
        }
        put(out, "\n");
    }

    let filter = env.filter();
    if !filter.is_empty() {
        put(out, "# Filter search with file format conversion tools\nfilter=");
        put_b(out, &filter);
        put(out, "\n\n");
        let n = env.list_len(List::FilterMagicLabel);
        if n > 0 {
            put(out, "# Filter by file signature magic bytes\n");
            for i in 0..n {
                let v = env.list_get(List::FilterMagicLabel, i);
                put(out, "filter-magic-label=");
                put_b(out, &v);
                put(out, "\n");
            }
            put(out, "# Warning: filter-magic-label significantly reduces performance!\n\n");
        }
    }

    put(out, "### OUTPUT ###\n\n");

    if let Some(separator) = text(env, s.flag_separator) {
        put(out, "# Separator, default: none specified to output a `:'\nseparator=");
        put_b(out, &separator);
        put(out, "\n\n");
    }

    put(out, "# Sort the list of files and directories searched and matched, default: sort\n");
    if let Some(sort) = text(env, s.flag_sort) {
        put(out, "sort=");
        put_b(out, &sort);
        put(out, "\n\n");
    } else {
        put(out, "# sort\n\n");
    }

    f
}

/// `save_config()`
pub fn save_config<E: ConfigEnv>(s: &mut Settings, env: &mut E, k: ConfigConsts) {
    let save = cs(env, s.flag_save_config);
    let mut exists = false;

    // rename old config file to .old when present and not standard input
    if save != b"-" {
        let mut old = save.clone();
        old.extend_from_slice(b".old");
        if env.rename_old(&nz(&save), &nz(&old)) {
            exists = true;
        }
    }

    // if not saved to standard output ("-"), then inform user
    if !s.flag_no_messages && save != b"-" {
        let mut msg = format!("ugrep: {} configuration file ", if exists { "updating" } else { "saving" }).into_bytes();
        msg.extend_from_slice(&save);
        let n = env.files_len();
        if n > 0 {
            put(&mut msg, " with options based on");
            for i in 0..n {
                msg.push(b' ');
                let f = env.files_get(i);
                msg.extend_from_slice(&f);
            }
        }
        msg.push(b'\n');
        env.write(Sink::Errout, &msg);
    }

    let Some(file) = env.save_open(&nz(&save)) else {
        env.usage("cannot save configuration file ", Some(&save), None);
        return;
    };

    let content = render(s, env, k, &save);
    env.save_write(file, &content);

    if env.save_error(file) {
        env.error("cannot save", &nz(&save));
    }

    env.save_close(file);

    let unsaved = [List::FileType, List::FileExtension, List::FileMagic, List::Glob, List::Iglob];
    if unsaved.iter().any(|&l| !env.list_empty(l)) {
        env.warning("options --file-type, --file-extension, --file-magic, --glob and --iglob are not saved to", &nz(&save));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENOENT: i32 = 2;

    /// Mimics `fopen_smart`: a `~` path without a home directory fails with errno = ENOENT.
    #[derive(Default)]
    struct Mock {
        home: Option<Vec<u8>>,
        strings: Vec<(*const c_char, Vec<u8>)>,
        errno: i32,
        opened: Vec<Vec<u8>>,
        errors: Vec<(String, i32)>,
        usages: Vec<(String, i32)>,
    }

    impl Mock {
        fn fopen(&mut self, path: Z<'_>) -> Option<()> {
            self.opened.push(path.to_vec());
            if path.first() == Some(&b'~') && self.home.is_none() {
                self.errno = ENOENT;
                return None;
            }
            Some(())
        }
    }

    impl ConfigEnv for Mock {
        type File = ();

        fn usage(&mut self, message: &str, _arg: Option<&[u8]>, _valid: Option<&str>) {
            self.usages.push((message.to_string(), self.errno));
        }
        fn list_clear(&mut self, _list: List) {}
        fn list_empty(&mut self, _list: List) -> bool {
            true
        }
        fn filter_clear(&mut self) {}
        fn c_string(&mut self, p: *const c_char) -> Vec<u8> {
            self.strings.iter().find(|(q, _)| *q == p).map(|(_, v)| v.clone()).unwrap_or_default()
        }
        fn open(&mut self, path: Z<'_>, _check_owner: bool) -> Option<()> {
            self.fopen(path)
        }
        fn next_line(&mut self, _file: ()) -> bool {
            false
        }
        fn line_text(&mut self, _file: ()) -> Vec<u8> {
            Vec::new()
        }
        fn prefix_line(&mut self, _file: ()) -> *const c_char {
            std::ptr::null()
        }
        fn file_error(&mut self, _file: ()) -> bool {
            false
        }
        fn close(&mut self, _file: ()) {}
        fn files_contains(&mut self, _path: Z<'_>) -> bool {
            false
        }
        fn files_insert(&mut self, _path: Z<'_>) {}
        fn files_len(&mut self) -> usize {
            0
        }
        fn files_get(&mut self, _index: usize) -> Vec<u8> {
            Vec::new()
        }
        fn home_dir(&mut self) -> Option<Vec<u8>> {
            self.home.clone()
        }
        fn warnings_reset(&mut self) {}
        fn warnings(&mut self) -> usize {
            0
        }
        fn write(&mut self, _sink: Sink, _bytes: &[u8]) {}
        fn error(&mut self, message: &str, _arg: Z<'_>) {
            self.errors.push((message.to_string(), self.errno));
        }
        fn warning(&mut self, _message: &str, _arg: Z<'_>) {}
        fn exit_error(&mut self) -> ! {
            panic!("exit(EXIT_ERROR)")
        }
        fn parse_option(&mut self, _settings: &mut Settings, _arg: *const c_char) {}
        fn regexp_clear(&mut self) {}
        fn filter(&mut self) -> Vec<u8> {
            Vec::new()
        }
        fn list_len(&mut self, _list: List) -> usize {
            0
        }
        fn list_get(&mut self, _list: List, _index: usize) -> Vec<u8> {
            Vec::new()
        }
        fn rename_old(&mut self, _path: Z<'_>, _old: Z<'_>) -> bool {
            false
        }
        fn save_open(&mut self, path: Z<'_>) -> Option<()> {
            self.fopen(path)
        }
        fn save_write(&mut self, _file: (), _bytes: &[u8]) {}
        fn save_error(&mut self, _file: ()) -> bool {
            false
        }
        fn save_close(&mut self, _file: ()) {}
    }

    const K: ConfigConsts = ConfigConsts { default_query_delay: 3, path_sep: b'\\', xdg: false };

    #[test]
    fn load_config_tilde_path_without_home_cannot_read() {
        let mut s = Settings::DEFAULT;
        s.flag_config = c"~\\h.cfg".as_ptr();
        let mut env = Mock::default();
        env.strings.push((s.flag_config, b"~\\h.cfg".to_vec()));
        load_config(&mut s, &mut env, K, false);
        assert_eq!(env.opened, vec![b"~\\h.cfg\0".to_vec()]);
        assert_eq!(env.errors, vec![("option --config: cannot read".to_string(), ENOENT)]);
    }

    #[test]
    fn save_config_tilde_path_without_home_cannot_save() {
        let mut s = Settings::DEFAULT;
        s.flag_save_config = c"~\\x".as_ptr();
        let mut env = Mock::default();
        env.strings.push((s.flag_save_config, b"~\\x".to_vec()));
        save_config(&mut s, &mut env, K);
        assert_eq!(env.opened, vec![b"~\\x\0".to_vec()]);
        assert_eq!(env.usages, vec![("cannot save configuration file ".to_string(), ENOENT)]);
    }
}
