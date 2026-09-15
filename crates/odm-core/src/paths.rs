//! Workspace layout path policy — single owner for checkout, worktree, and index paths.

use std::path::{Component, Path, PathBuf};

use crate::error::OdmError;

/// Why a path failed workspace-root resolution policy.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathResolveError {
    #[error("path must be relative, got '{path}'")]
    Absolute { path: String },
    #[error("path escapes Workspace root: '{path}'")]
    Escape { path: String },
}

impl PathResolveError {
    pub fn path(&self) -> &str {
        match self {
            Self::Absolute { path } | Self::Escape { path } => path,
        }
    }
}

impl From<PathResolveError> for OdmError {
    fn from(err: PathResolveError) -> Self {
        OdmError::workspace(err.to_string())
    }
}

/// ODM state directory: `<root>/.odm`.
pub fn odm_dir(root: &Path) -> PathBuf {
    root.join(".odm")
}

/// Workspace config path: `<root>/.odm/odm.config.yaml`.
pub fn config_path(root: &Path) -> PathBuf {
    odm_dir(root).join("odm.config.yaml")
}

/// Pin file path: `<root>/.odm/odm.lock.yaml`.
pub fn pin_path(root: &Path) -> PathBuf {
    odm_dir(root).join("odm.lock.yaml")
}

/// Resolve config-relative `rel` under Workspace `root`.
/// Err if absolute or escapes via `..`.
pub fn resolve_under_root(root: &Path, rel: &str) -> Result<PathBuf, PathResolveError> {
    let p = Path::new(rel);
    if p.is_absolute() {
        return Err(PathResolveError::Absolute {
            path: rel.to_string(),
        });
    }
    let mut out = root.to_path_buf();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::Normal(s) => out.push(s),
            Component::ParentDir => {
                if !out.pop() || out.as_os_str().is_empty() {
                    return Err(PathResolveError::Escape {
                        path: rel.to_string(),
                    });
                }
                if !out.starts_with(root) {
                    return Err(PathResolveError::Escape {
                        path: rel.to_string(),
                    });
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(PathResolveError::Absolute {
                    path: rel.to_string(),
                });
            }
        }
    }
    if !out.starts_with(root) {
        return Err(PathResolveError::Escape {
            path: rel.to_string(),
        });
    }
    Ok(out)
}

/// Primary checkout / Progen store absolute path (escape-safe).
pub fn abs_checkout(root: &Path, rel: &str) -> Result<PathBuf, OdmError> {
    Ok(resolve_under_root(root, rel)?)
}

/// Single path component used as a name (entity, worktree slot, …).
///
/// Non-empty after trim; no `/` `\` NUL; not `.` or `..`; no Windows drive prefix.
/// Returns the trimmed token, or a short reason string on failure.
pub fn parse_path_token(name: &str) -> Result<&str, &'static str> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("empty");
    }
    if trimmed == "." || trimmed == ".." {
        return Err("dot");
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains('\0') {
        return Err("separator");
    }
    if trimmed.starts_with('/') {
        return Err("absolute");
    }
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err("drive");
    }
    Ok(trimmed)
}

/// Worktree slot working tree: `<root>/worktrees/<project>/<slot>`.
pub fn worktree_slot_path(root: &Path, project_name: &str, slot_name: &str) -> PathBuf {
    root.join("worktrees").join(project_name).join(slot_name)
}

/// ODM-side Progen index dir: `<root>/.odm/progen/<name>`.
pub fn progen_index_dir(root: &Path, progen_name: &str) -> PathBuf {
    odm_dir(root).join("progen").join(progen_name)
}

/// Relative path string helper for CLI (rejects absolute; normalizes `\` → `/`).
pub fn path_buf_to_rel(path: &Path) -> Result<String, OdmError> {
    let s = path.to_string_lossy();
    if path.is_absolute() {
        return Err(OdmError::usage(format!(
            "path must be relative, got '{s}'"
        )));
    }
    Ok(s.replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_under_root_happy_path() {
        let root = Path::new("/ws");
        let got = resolve_under_root(root, "projects/a").unwrap();
        assert_eq!(got, PathBuf::from("/ws/projects/a"));
    }

    #[test]
    fn resolve_under_root_rejects_escape() {
        let root = Path::new("/ws");
        assert!(matches!(
            resolve_under_root(root, "../outside"),
            Err(PathResolveError::Escape { path }) if path == "../outside"
        ));
        assert!(matches!(
            resolve_under_root(root, "a/../../outside"),
            Err(PathResolveError::Escape { path }) if path == "a/../../outside"
        ));
    }

    #[test]
    fn resolve_under_root_rejects_absolute() {
        let root = Path::new("/ws");
        assert!(matches!(
            resolve_under_root(root, "/abs"),
            Err(PathResolveError::Absolute { path }) if path == "/abs"
        ));
    }

    #[test]
    fn path_resolve_error_into_workspace_odm() {
        let abs: OdmError = PathResolveError::Absolute {
            path: "/x".into(),
        }
        .into();
        assert_eq!(abs.code(), "workspace");
        assert!(abs.message().contains("relative"));
        let esc: OdmError = PathResolveError::Escape {
            path: "../y".into(),
        }
        .into();
        assert_eq!(esc.code(), "workspace");
        assert!(esc.message().contains("escape"));
    }

    #[test]
    fn abs_checkout_matches_resolve() {
        let root = Path::new("/ws");
        assert_eq!(
            abs_checkout(root, "projects/a").unwrap(),
            resolve_under_root(root, "projects/a").unwrap()
        );
        assert!(abs_checkout(root, "../outside").is_err());
    }

    #[test]
    fn worktree_slot_path_shape() {
        let root = Path::new("/ws");
        assert_eq!(
            worktree_slot_path(root, "alpha", "slot1"),
            PathBuf::from("/ws/worktrees/alpha/slot1")
        );
    }

    #[test]
    fn progen_index_dir_shape() {
        let root = Path::new("/ws");
        assert_eq!(
            progen_index_dir(root, "desk"),
            PathBuf::from("/ws/.odm/progen/desk")
        );
    }

    #[test]
    fn layout_helpers() {
        let root = Path::new("/ws");
        assert_eq!(odm_dir(root), PathBuf::from("/ws/.odm"));
        assert_eq!(config_path(root), PathBuf::from("/ws/.odm/odm.config.yaml"));
        assert_eq!(pin_path(root), PathBuf::from("/ws/.odm/odm.lock.yaml"));
    }

    #[test]
    fn parse_path_token_rules() {
        assert_eq!(parse_path_token("  ok  ").unwrap(), "ok");
        assert!(parse_path_token("").is_err());
        assert!(parse_path_token("a/b").is_err());
        assert!(parse_path_token("..").is_err());
        assert!(parse_path_token(".").is_err());
    }

    #[test]
    fn path_buf_to_rel_rejects_absolute() {
        let err = path_buf_to_rel(Path::new("/abs")).unwrap_err();
        assert!(err.to_string().contains("relative"));
        assert_eq!(
            path_buf_to_rel(Path::new("vaults/desk")).unwrap(),
            "vaults/desk"
        );
    }
}
