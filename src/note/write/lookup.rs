use std::fs;
use std::path::Path;

use crate::{hive_root, lookup_notes};

pub fn lookup(start: &Path, query: &[String]) -> Result<String, String> {
    let needle = query.join(" ");
    let notes = lookup_notes(start)?;
    let root = hive_root(start)?;
    let mut hits = Vec::new();
    walk(&notes.planning, &root, &needle, &mut hits);
    Ok(hits.join("\n"))
}

fn walk(abs_dir: &Path, root: &Path, needle: &str, hits: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(abs_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk(&path, root, needle, hits);
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
        search_file(&rel, &base, &raw, needle, hits);
    }
}

fn search_file(rel: &str, base: &str, raw: &str, needle: &str, hits: &mut Vec<String>) {
    let folded = needle.to_lowercase();
    let mut body = false;
    for (i, line) in raw.split('\n').map(|l| l.trim_end_matches('\r')).enumerate() {
        if line.to_lowercase().contains(&folded) {
            hits.push(format!("{}:{}:{}", rel, i + 1, line));
            body = true;
        }
    }
    if body {
        return;
    }
    let stem = base.strip_suffix(".md").unwrap_or(base);
    if stem.to_lowercase().contains(&folded) {
        hits.push(format!("{rel}:0:{stem}"));
    }
}
