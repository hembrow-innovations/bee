use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use ignore::gitignore::GitignoreBuilder;
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};

use crate::error::{OnicError, OnicResult};
use crate::ids::as_rel_path;
use crate::types::ScannedFile;

const DEFAULT_IGNORE: &[&str] = &[
    ".git/",
    "node_modules/",
    ".onic/",
    "dist/",
    "coverage/",
    ".pstack/verify-artifacts/",
];

pub fn find_project_root(start: &Path) -> PathBuf {
    let start = absolute(start);
    let mut dir = start.clone();
    loop {
        if is_git_root(&dir) || has_graph_db(&dir) {
            return dir;
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent.to_path_buf(),
            _ => return start,
        }
    }
}

pub fn resolve_db(start: &Path, db: Option<&Path>, root: Option<&Path>) -> PathBuf {
    if let Some(db) = db {
        return if db.is_absolute() {
            db.to_path_buf()
        } else {
            start.join(db)
        };
    }
    let base = root.unwrap_or(start);
    base.join(".onic").join("graph.db")
}

pub fn walk_project(root: &Path) -> OnicResult<Vec<ScannedFile>> {
    let mut builder = GitignoreBuilder::new(root);
    for pattern in DEFAULT_IGNORE {
        builder
            .add_line(None, pattern)
            .map_err(|err| OnicError::msg(err.to_string()))?;
    }
    add_ignore_file(
        &mut builder,
        &root.join(".git").join("info").join("exclude"),
    )?;
    add_ignore_file(&mut builder, &root.join(".gitignore"))?;
    add_ignore_file(&mut builder, &root.join(".onicignore"))?;
    let ignore = builder
        .build()
        .map_err(|err| OnicError::msg(err.to_string()))?;
    let mut files = Vec::new();
    visit(root, root, &ignore, &mut files)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

pub fn lang_for_path(rel: &str) -> Option<&'static str> {
    let dot = rel.rfind('.')?;
    match rel[dot..].to_ascii_lowercase().as_str() {
        ".ts" | ".tsx" | ".mts" | ".cts" => Some("ts"),
        ".js" | ".jsx" | ".mjs" | ".cjs" => Some("js"),
        ".py" => Some("py"),
        ".rs" => Some("rust"),
        ".go" => Some("go"),
        ".drac" => Some("drac"),
        ".md" | ".mdx" => Some("md"),
        _ => None,
    }
}

pub(crate) fn absolute(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let mut out = PathBuf::new();
    for comp in abs.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

pub(crate) fn is_default_ignored(rel: &str) -> bool {
    let normalized = rel.replace('\\', "/");
    for entry in DEFAULT_IGNORE {
        let dir = entry.trim_end_matches('/');
        if normalized == dir || normalized.starts_with(&format!("{dir}/")) {
            return true;
        }
    }
    false
}

fn is_git_root(dir: &Path) -> bool {
    let git = dir.join(".git");
    match fs::metadata(&git) {
        Ok(meta) if meta.is_file() => true,
        Ok(meta) if meta.is_dir() => git.join("HEAD").exists(),
        _ => false,
    }
}

fn has_graph_db(dir: &Path) -> bool {
    let path = dir.join(".onic").join("graph.db");
    let Ok(conn) = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return false;
    };
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'nodes'",
        [],
        |_| Ok(()),
    )
    .is_ok()
}

fn add_ignore_file(builder: &mut GitignoreBuilder, path: &Path) -> OnicResult<()> {
    match fs::metadata(path) {
        Ok(meta) if meta.is_file() => {
            let _ = builder.add(path);
            Ok(())
        }
        Ok(_) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn visit(
    root: &Path,
    dir: &Path,
    ignore: &ignore::gitignore::Gitignore,
    files: &mut Vec<ScannedFile>,
) -> OnicResult<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let abs = entry.path();
        let rel = match abs.strip_prefix(root) {
            Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        let meta = match fs::symlink_metadata(&abs) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        let is_dir = meta.is_dir();
        if !is_dir && !meta.is_file() {
            continue;
        }
        if ignore.matched(&rel, is_dir).is_ignore() {
            continue;
        }
        if is_dir {
            visit(root, &abs, ignore, files)?;
            continue;
        }
        let Some(lang) = lang_for_path(&rel) else {
            continue;
        };
        let bytes = match fs::read(&abs) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let path = as_rel_path(&rel)?;
        files.push(ScannedFile {
            path,
            abs_path: abs,
            lang: lang.to_string(),
            hash: format!("{:x}", Sha256::digest(&bytes)),
            mtime: mtime_ms(&meta),
            size: meta.len(),
            text: String::from_utf8_lossy(&bytes).into_owned(),
        });
    }
    Ok(())
}

fn mtime_ms(meta: &fs::Metadata) -> i64 {
    let Ok(modified) = meta.modified() else {
        return 0;
    };
    match modified.duration_since(UNIX_EPOCH) {
        Ok(dur) => dur.as_millis() as i64,
        Err(err) => -(err.duration().as_millis() as i64),
    }
}
