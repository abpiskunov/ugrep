//! Port of src/stats.cpp `Stats::report`: builds the report text the C++ facade writes to the `FILE*`.

use super::cfmt::{fmt_g, percent};
use super::{Action, Bytes, Mode, StatsInput};

pub fn report(s: &StatsInput) -> Bytes {
    let nl: &[u8] = if s.crlf { b"\r\n" } else { b"\n" };
    let mut o: Bytes = Vec::new();
    let put = |o: &mut Bytes, t: &str| o.extend_from_slice(t.as_bytes());
    let line = |o: &mut Bytes, t: &str| {
        o.extend_from_slice(t.as_bytes());
        o.extend_from_slice(nl);
    };
    let plural = |n: usize, one: &'static str, many: &'static str| if n == 1 { one } else { many };

    let (sf, sd, si, ss, sc, sa, sl) =
        (s.searched_files, s.searched_dirs, s.searched_indexed, s.searched_skipped, s.searched_changed, s.searched_added, s.searched_lines);
    let (ff, fp, fm, ws) = (s.found_files, s.found_parts, s.found_matches, s.warnings);

    o.extend_from_slice(nl);
    put(&mut o, &format!("Searched {} file{}", sf, plural(sf, "", "s")));
    if sd > 0 {
        put(&mut o, &format!(" in {} director{}", sd, plural(sd, "y", "ies")));
    }
    if !s.query && !s.pager {
        put(&mut o, &format!(" in {} seconds", fmt_g(0.001 * s.elapsed_ms, 3)));
    }
    if s.threads > 1 {
        put(&mut o, &format!(" with {} threads", s.threads));
    }
    put(&mut o, &format!(": {} matching ({}%)", ff, fmt_g(percent(ff, sf), 4)));
    if fp > ff {
        put(&mut o, &format!(" + {} in archives", fp.wrapping_sub(ff)));
    }
    o.extend_from_slice(nl);

    if fm > 0 && !s.quiet && !s.files_with_matches && !s.files_without_match {
        if s.ungroup || (s.count && s.only_matching) || (!s.count && s.format) {
            line(&mut o, &format!("Searched {} line{}: found {} match{} (ungrouped)", sl, plural(sl, "", "s"), fm, plural(fm, "", "es")));
        } else {
            line(&mut o, &format!("Searched {} line{}: {} matching ({}%)", sl, plural(sl, "", "s"), fm, fmt_g(percent(fm, sl), 4)));
        }
    }

    if s.index && si > 0 {
        let rest = sf.wrapping_sub(ff);
        put(
            &mut o,
            &format!("Skipped {} file{} of {} ({}%) not matching {} indexes\n", ss, plural(ss, "", "s"), rest, fmt_g(percent(ss, rest), 4), si),
        );
        if sc > 0 || sa > 0 {
            put(&mut o, "Detected outdated or missing index files, run ugrep-indexer to re-index:\n");
            if sc > 1 {
                put(&mut o, &format!("  searched {} changed files\n", sc));
            } else if sc == 1 {
                put(&mut o, "  searched 1 changed file\n");
            }
            if sa > 1 {
                put(&mut o, &format!("  searched {} new files\n", sa));
            } else if sa == 1 {
                put(&mut o, "  searched 1 new file\n");
            }
        }
    }

    if ws > 0 {
        line(&mut o, &format!("Received {} warning{}", ws, plural(ws, "", "s")));
    }

    line(&mut o, "The following pathname selections and search constraints were applied:");
    let mode = Mode(s.mode);
    let bytes_line = |o: &mut Bytes, pre: &str, b: &[u8], post: &str| {
        o.extend_from_slice(pre.as_bytes());
        o.extend_from_slice(b);
        o.extend_from_slice(post.as_bytes());
        o.extend_from_slice(nl);
    };

    if let Some(config) = &s.config {
        bytes_line(&mut o, "  --config=", config, "");
        for i in &s.config_files {
            bytes_line(&mut o, "    using ", i, "");
        }
    }
    if s.fuzzy > 0 {
        line(&mut o, "  --fuzzy");
    }
    if s.boolean {
        line(&mut o, if s.files { "  --bool --files" } else { "  --bool --lines" });
    }
    if mode == Mode::BRE {
        line(&mut o, "  --basic-regexp");
    } else if mode == Mode::FIXED {
        line(&mut o, "  --fixed-strings");
    } else if mode == Mode::PERL {
        line(&mut o, "  --perl-regexp");
    } else {
        line(&mut o, "  --extended-regexp");
    }
    if s.decompress {
        line(&mut o, &format!("  --decompress --zmax={}", s.zmax));
    }
    if s.min_depth > 0 && s.max_depth > 0 {
        line(&mut o, &format!("  --depth={},{}", s.min_depth, s.max_depth));
    } else if s.min_depth > 0 {
        line(&mut o, &format!("  --depth={},", s.min_depth));
    } else if s.max_depth > 0 {
        line(&mut o, &format!("  --depth={}", s.max_depth));
    }
    if s.dereference {
        line(&mut o, "  --dereference");
    } else if s.no_dereference {
        line(&mut o, "  --no-dereference");
    }
    match Action(s.devices_action) {
        Action::SKIP => line(&mut o, "  --devices=skip"),
        Action::READ => line(&mut o, "  --devices=read"),
        _ => {}
    }
    match Action(s.directories_action) {
        Action::SKIP => line(&mut o, "  --directories=skip"),
        Action::READ => line(&mut o, "  --directories=read"),
        Action::RECURSE => line(&mut o, "  --directories=recurse"),
        _ => {}
    }
    if s.glob_ignore_case {
        line(&mut o, "  --glob-ignore-case");
    }
    if s.hidden_default {
        line(&mut o, if s.hidden { "  --hidden (default)" } else { "  --no-hidden" });
    } else {
        line(&mut o, if s.hidden { "  --hidden" } else { "  --no-hidden (default)" });
    }
    for i in &s.ignore_files {
        bytes_line(&mut o, "  --ignore-files=\"", i, "\"");
    }
    if s.index {
        line(&mut o, "  --index");
    }
    if s.min_count > 0 {
        line(&mut o, &format!("  --min-count={}", s.min_count));
    }
    if s.max_count > 0 {
        line(&mut o, &format!("  --max-count={}", s.max_count));
    }
    if s.max_files > 0 {
        line(&mut o, &format!("  --max-files={}", s.max_files));
    }
    if s.min_line > 0 {
        line(&mut o, &format!("  --min-line={}", s.min_line));
    }
    if s.max_line > 0 {
        line(&mut o, &format!("  --max-line={}", s.max_line));
    }
    if let Some(sort) = &s.sort {
        bytes_line(&mut o, "  --sort=", sort, "");
    }
    if s.min_size > 0 {
        line(&mut o, &format!("  --min-size={}", s.min_size));
    }
    if s.max_size > 0 {
        line(&mut o, &format!("  --max-size={}", s.max_size));
    }
    for i in &s.ignore {
        // substr(0, find_last_of(PATHSEPCHR)): the whole string when there is no separator
        let dir = match i.iter().rposition(|&c| c == s.pathsep) {
            Some(p) => &i[..p],
            None => &i[..],
        };
        o.extend_from_slice(b"    ");
        o.extend_from_slice(i);
        o.extend_from_slice(b" exclusions were applied to ");
        o.extend_from_slice(dir);
        o.extend_from_slice(b" and its subdirectories");
        o.extend_from_slice(nl);
    }
    for i in &s.file_magic {
        if matches!(i.first(), Some(b'!') | Some(b'^')) {
            bytes_line(&mut o, "  --file-magic=\"!", &i[1..], "\" (negated)");
        } else {
            bytes_line(&mut o, "  --file-magic=\"", i, "\"");
        }
    }
    for i in &s.include_fs {
        bytes_line(&mut o, "  --include-fs=\"", i, "\"");
    }
    for i in &s.exclude_fs {
        bytes_line(&mut o, "  --exclude-fs=\"", i, "\"");
    }
    let globs: [(&str, &Vec<Bytes>, usize); 4] = [
        ("  --include=\"", &s.all_include, s.include_iglob_size),
        ("  --exclude=\"", &s.all_exclude, s.exclude_iglob_size),
        ("  --include-dir=\"", &s.all_include_dir, s.include_iglob_dir_size),
        ("  --exclude-dir=\"", &s.all_exclude_dir, s.exclude_iglob_dir_size),
    ];
    for (name, list, iglob) in globs {
        for (n, i) in list.iter().enumerate() {
            let neg = if i.first() == Some(&b'!') { " (negated)" } else { "" };
            let ic = if n < iglob { " (ignore case)" } else { "" };
            bytes_line(&mut o, name, i, &format!("\"{neg}{ic}"));
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warnings_and_default_constraints() {
        let s = StatsInput { searched_files: 2, found_files: 1, warnings: 1, query: true, mode: Mode::ERE.0, crlf: false, ..Default::default() };
        let out = String::from_utf8(report(&s)).unwrap();
        assert!(out.starts_with("\nSearched 2 files: 1 matching (50%)\nReceived 1 warning\n"));
        assert!(out.ends_with("  --extended-regexp\n  --no-hidden (default)\n"));
    }
}
