use std::env;
use std::path::{Path, PathBuf};

use crate::config::config_path;
use crate::error::OdmError;

/// Resolve Workspace root from optional `--root` or walk-up from `start` (usually cwd).
pub fn discover_root(root_flag: Option<&Path>, start: &Path) -> Result<PathBuf, OdmError> {
    if let Some(root) = root_flag {
        let root = normalize_start(root)?;
        let cfg = config_path(&root);
        if !cfg.is_file() {
            return Err(OdmError::workspace(format!(
                "not a Workspace: --root {} missing {}",
                root.display(),
                cfg.display()
            )));
        }
        return Ok(root);
    }

    let mut dir = normalize_start(start)?;

    // If cwd is inside a `.odm/` directory, start from its parent.
    if dir.file_name().and_then(|s| s.to_str()) == Some(".odm") {
        if let Some(parent) = dir.parent() {
            dir = parent.to_path_buf();
        }
    } else if let Some(odm_ancestor) = find_odm_ancestor(&dir) {
        // only adjust when we're *inside* .odm tree, not when .odm is a sibling candidate
        if is_under_odm(&dir, &odm_ancestor) {
            if let Some(parent) = odm_ancestor.parent() {
                dir = parent.to_path_buf();
            }
        }
    }

    let stop = stop_boundary();
    let mut cur = Some(dir.as_path());
    while let Some(d) = cur {
        if config_path(d).is_file() {
            return Ok(d.to_path_buf());
        }
        if stop.as_ref().is_some_and(|s| d == s.as_path()) {
            break;
        }
        if d.parent().is_none() {
            break;
        }
        // also stop at filesystem root after checking it
        let parent = d.parent();
        if parent == Some(Path::new("")) || parent == Some(Path::new("/")) {
            if let Some(p) = parent {
                if config_path(p).is_file() {
                    return Ok(p.to_path_buf());
                }
            }
            break;
        }
        cur = parent;
    }

    Err(OdmError::workspace(format!(
        "not a Workspace: no .odm/odm.config.yaml found from {}",
        start.display()
    )))
}

fn normalize_start(path: &Path) -> Result<PathBuf, OdmError> {
    if path.as_os_str().is_empty() {
        return Err(OdmError::workspace("empty path"));
    }
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map_err(|e| OdmError::operation(format!("failed to get cwd: {e}")))?
            .join(path)
    };
    // Prefer canonical when exists
    Ok(abs.canonicalize().unwrap_or(abs))
}

fn stop_boundary() -> Option<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .and_then(|h| h.canonicalize().ok().or(Some(h)))
}

fn find_odm_ancestor(dir: &Path) -> Option<PathBuf> {
    let mut cur = Some(dir);
    while let Some(d) = cur {
        if d.file_name().and_then(|s| s.to_str()) == Some(".odm") {
            return Some(d.to_path_buf());
        }
        cur = d.parent();
    }
    None
}

fn is_under_odm(dir: &Path, odm: &Path) -> bool {
    dir.starts_with(odm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_ws(root: &Path) {
        fs::create_dir_all(root.join(".odm")).unwrap();
        fs::write(root.join(".odm/odm.config.yaml"), "{}\n").unwrap();
    }

    #[test]
    fn root_flag_requires_config() {
        let dir = tempdir().unwrap();
        let err = discover_root(Some(dir.path()), dir.path()).unwrap_err();
        assert!(matches!(err, OdmError::Workspace(_)));
        write_ws(dir.path());
        let found = discover_root(Some(dir.path()), Path::new("/tmp")).unwrap();
        assert_eq!(
            found.canonicalize().unwrap(),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn walk_up_finds_config() {
        let dir = tempdir().unwrap();
        write_ws(dir.path());
        let nested = dir.path().join("a/b/c");
        fs::create_dir_all(&nested).unwrap();
        let found = discover_root(None, &nested).unwrap();
        assert_eq!(
            found.canonicalize().unwrap(),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn inside_odm_starts_at_parent() {
        let dir = tempdir().unwrap();
        write_ws(dir.path());
        let odm = dir.path().join(".odm");
        let found = discover_root(None, &odm).unwrap();
        assert_eq!(
            found.canonicalize().unwrap(),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn empty_odm_without_config_not_workspace() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".odm")).unwrap();
        let nested = dir.path().join("x");
        fs::create_dir_all(&nested).unwrap();
        let err = discover_root(None, &nested).unwrap_err();
        assert!(matches!(err, OdmError::Workspace(_)));
    }

    #[test]
    fn root_flag_no_walk() {
        let outer = tempdir().unwrap();
        write_ws(outer.path());
        let inner = tempdir().unwrap();
        // --root points at empty dir even if cwd has a workspace up-tree
        let err = discover_root(Some(inner.path()), outer.path()).unwrap_err();
        assert!(err.to_string().contains("--root"));
    }
}
