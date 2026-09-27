use std::fs;
use std::path::Path;

use regex::{Regex, RegexBuilder};

use crate::{hive_root, lookup_notes};

pub fn lookup(start: &Path, query: &[String], scope: &str, regex: bool) -> Result<String, String> {
    let needle = query.join(" ");
    let notes = lookup_notes(start)?;
    let root = hive_root(start)?;
    let re = compile_needle(&needle, regex)?;
    let mut hits = Vec::new();
    match scope {
        "planning" => walk(&notes.planning, &root, &re, &mut hits),
        "archive" => {
            let archive = notes
                .archive
                .ok_or_else(|| "Missing notes.archive".to_string())?;
            walk(&archive, &root, &re, &mut hits);
        }
        "all" => {
            walk(&notes.planning, &root, &re, &mut hits);
            if let Some(archive) = notes.archive.as_ref() {
                walk(archive, &root, &re, &mut hits);
            }
        }
        _ => return Err(format!("unknown --scope {scope}")),
    }
    Ok(hits.join("\n"))
}

fn compile_needle(needle: &str, is_regex: bool) -> Result<Regex, String> {
    let pattern = if is_regex {
        needle.to_string()
    } else {
        regex::escape(needle)
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(true)
        .build()
        .map_err(|_| "bad --regex".to_string())
}

fn walk(abs_dir: &Path, root: &Path, re: &Regex, hits: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(abs_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk(&path, root, re, hits);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let base = entry.file_name().to_string_lossy().into_owned();
        if !base.ends_with(".md") {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| base.clone());
        search_file(&rel, &base, &raw, re, hits);
    }
}

fn search_file(rel: &str, base: &str, raw: &str, re: &Regex, hits: &mut Vec<String>) {
    let mut body = false;
    for (i, line) in raw
        .split('\n')
        .map(|l| l.trim_end_matches('\r'))
        .enumerate()
    {
        if re.is_match(line) {
            hits.push(format!("{}:{}:{}", rel, i + 1, line));
            body = true;
        }
    }
    if body {
        return;
    }
    let stem = base.strip_suffix(".md").unwrap_or(base);
    if re.is_match(stem) {
        hits.push(format!("{rel}:0:{stem}"));
    }
}
