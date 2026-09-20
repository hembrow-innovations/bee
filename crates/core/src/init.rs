use std::fs;
use std::path::{Path, PathBuf};

use hive_git::Git;

use crate::config::{config_path, odm_dir, save_config, HiveConfig};
use crate::error::HiveError;
use crate::gitignore::apply_managed_gitignore;

#[derive(Debug, Clone)]
pub struct InitOptions {
    /// Target Hive root (absolute or relative).
    pub path: PathBuf,
    /// Skip `git init` at Hive root.
    pub no_git: bool,
    /// Optional Hive name written to config.
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitResult {
    pub root: PathBuf,
    pub git: bool,
}

/// Bootstrap a new Hive at `opts.path`.
pub fn init_hive(opts: InitOptions) -> Result<InitResult, HiveError> {
    let root = absolutize(&opts.path)?;
    fs::create_dir_all(&root).map_err(|e| {
        HiveError::operation(format!("failed to create {}: {e}", root.display()))
    })?;

    let cfg = config_path(&root);
    if cfg.is_file() {
        return Err(HiveError::hive(format!(
            "already a Hive: {}",
            cfg.display()
        )));
    }

    fs::create_dir_all(odm_dir(&root)).map_err(|e| {
        HiveError::operation(format!("failed to create .odm: {e}"))
    })?;

    let mut config = HiveConfig::minimal();
    config.name = opts.name;

    let git = if opts.no_git {
        false
    } else {
        let g = Git::new();
        // init even if already a repo is ok for git; we still report git=true if repo after
        if !g.is_repo(&root)? {
            g.init(&root)?;
        }
        g.is_repo(&root)?
    };

    save_config(&root, &config)?;

    if git && config.manage_gitignore() {
        apply_managed_gitignore(&root, &config)?;
    }

    Ok(InitResult { root, git })
}

fn absolutize(path: &Path) -> Result<PathBuf, HiveError> {
    if path.as_os_str().is_empty() {
        return Err(HiveError::usage("init path must not be empty"));
    }
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| HiveError::operation(format!("failed to get cwd: {e}")))?
            .join(path)
    };
    Ok(abs.canonicalize().unwrap_or(abs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn init_creates_config_and_git() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("ws");
        let res = init_hive(InitOptions {
            path: target.clone(),
            no_git: false,
            name: Some("demo".into()),
        })
        .unwrap();
        assert!(res.git);
        assert!(config_path(&res.root).is_file());
        let text = fs::read_to_string(config_path(&res.root)).unwrap();
        assert!(text.contains("name: demo"));
        assert!(res.root.join(".gitignore").is_file());
        let gi = fs::read_to_string(res.root.join(".gitignore")).unwrap();
        assert!(gi.contains(".odm/cache/"));
    }

    #[test]
    fn init_no_git() {
        let dir = tempdir().unwrap();
        let res = init_hive(InitOptions {
            path: dir.path().to_path_buf(),
            no_git: true,
            name: None,
        })
        .unwrap();
        assert!(!res.git);
        assert!(!res.root.join(".gitignore").exists());
    }

    #[test]
    fn refuse_existing() {
        let dir = tempdir().unwrap();
        init_hive(InitOptions {
            path: dir.path().to_path_buf(),
            no_git: true,
            name: None,
        })
        .unwrap();
        let err = init_hive(InitOptions {
            path: dir.path().to_path_buf(),
            no_git: true,
            name: None,
        })
        .unwrap_err();
        assert!(err.to_string().contains("already a Hive"));
    }
}
