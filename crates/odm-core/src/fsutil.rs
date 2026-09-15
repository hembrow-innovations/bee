//! Shared filesystem primitives for generate.

use std::fs;
use std::io;
use std::path::Path;

use crate::error::OdmError;

/// `true` when `path` is a directory with no entries.
pub fn is_dir_empty(path: &Path) -> Result<bool, OdmError> {
    let mut entries = fs::read_dir(path).map_err(|e| {
        OdmError::operation(format!("failed to read {}: {e}", path.display()))
    })?;
    Ok(entries.next().is_none())
}

/// Create `path` and parents as directories.
pub fn ensure_dir(path: &Path) -> Result<(), OdmError> {
    fs::create_dir_all(path).map_err(|e| {
        OdmError::operation(format!("failed to create {}: {e}", path.display()))
    })
}

/// Create parent directories of `path` when a parent exists.
pub fn ensure_parent(path: &Path) -> Result<(), OdmError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            OdmError::operation(format!("failed to create {}: {e}", parent.display()))
        })?;
    }
    Ok(())
}

/// Remove a file, symlink, or directory tree at `path`.
pub fn remove_path(path: &Path) -> Result<(), OdmError> {
    let meta = fs::symlink_metadata(path).map_err(|e| {
        OdmError::operation(format!("failed to stat {}: {e}", path.display()))
    })?;
    if meta.file_type().is_symlink() || meta.is_file() {
        fs::remove_file(path).map_err(|e| {
            OdmError::operation(format!("failed to remove {}: {e}", path.display()))
        })
    } else if meta.is_dir() {
        fs::remove_dir_all(path).map_err(|e| {
            OdmError::operation(format!("failed to remove {}: {e}", path.display()))
        })
    } else {
        fs::remove_file(path).map_err(|e| {
            OdmError::operation(format!("failed to remove {}: {e}", path.display()))
        })
    }
}

/// If `to` exists with a type that blocks writing `want_dir`, remove it (file or tree).
pub fn remove_type_conflict(to: &Path, want_dir: bool) -> Result<(), OdmError> {
    let meta = match fs::symlink_metadata(to) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(OdmError::operation(format!(
                "failed to stat {}: {e}",
                to.display()
            )));
        }
    };
    let is_dir = meta.file_type().is_dir();
    if want_dir == is_dir {
        return Ok(());
    }
    remove_path(to)
}

/// Count files and symlinks under `src` the same way [`copy_tree`] would write them.
/// Directories are recursed into and not counted.
pub fn count_tree(src: &Path) -> Result<u32, OdmError> {
    let mut count = 0u32;
    let entries = fs::read_dir(src).map_err(|e| {
        OdmError::operation(format!("failed to read {}: {e}", src.display()))
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| {
            OdmError::operation(format!("failed to read entry in {}: {e}", src.display()))
        })?;
        let from = entry.path();
        let ft = entry.file_type().map_err(|e| {
            OdmError::operation(format!("failed to stat {}: {e}", from.display()))
        })?;
        if ft.is_dir() {
            count += count_tree(&from)?;
        } else {
            count += 1;
        }
    }
    Ok(count)
}

/// How [`copy_tree`] treats an existing destination path that blocks the write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    /// Remove file↔dir type conflicts before write (generate `--force` in-place).
    ResolveTypeConflicts,
}

/// Recursively copy `src` directory contents into `dst`. Counts files and symlinks.
pub fn copy_tree(
    src: &Path,
    dst: &Path,
    policy: ConflictPolicy,
) -> Result<u32, OdmError> {
    let mut copied = 0u32;
    let entries = fs::read_dir(src).map_err(|e| {
        OdmError::operation(format!("failed to read {}: {e}", src.display()))
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| {
            OdmError::operation(format!("failed to read entry in {}: {e}", src.display()))
        })?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let ft = entry.file_type().map_err(|e| {
            OdmError::operation(format!("failed to stat {}: {e}", from.display()))
        })?;

        if ft.is_dir() {
            if policy == ConflictPolicy::ResolveTypeConflicts {
                remove_type_conflict(&to, true)?;
            }
            ensure_dir(&to)?;
            copied += copy_tree(&from, &to, policy)?;
        } else if ft.is_symlink() {
            copy_symlink(&from, &to)?;
            copied += 1;
        } else {
            ensure_parent(&to)?;
            if policy == ConflictPolicy::ResolveTypeConflicts {
                remove_type_conflict(&to, false)?;
            }
            fs::copy(&from, &to).map_err(|e| {
                OdmError::operation(format!(
                    "failed to copy {} -> {}: {e}",
                    from.display(),
                    to.display()
                ))
            })?;
            copied += 1;
        }
    }
    Ok(copied)
}

/// Read symlink at `from` and create the same link at `to`, replacing any existing path.
pub fn copy_symlink(from: &Path, to: &Path) -> Result<(), OdmError> {
    let target = fs::read_link(from).map_err(|e| {
        OdmError::operation(format!("failed to read symlink {}: {e}", from.display()))
    })?;
    if to.symlink_metadata().is_ok() {
        remove_path(to)?;
    }
    create_symlink(&target, to)
}

/// Create a symlink at `link` pointing to `target`.
pub fn create_symlink(target: &Path, link: &Path) -> Result<(), OdmError> {
    symlink_at(target, link).map_err(|e| {
        OdmError::operation(format!(
            "failed to create symlink {} -> {}: {e}",
            link.display(),
            target.display()
        ))
    })
}

#[cfg(unix)]
fn symlink_at(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn symlink_at(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
        .or_else(|_| std::os::windows::fs::symlink_dir(target, link))
}
