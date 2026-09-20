//! One open-context step: root, workspace, git, resolved globals.

use std::path::{Path, PathBuf};

use hive_core::{discover_root, load_hive, HiveError, Hive};
use hive_git::Git;

/// Resolved workspace session for command handlers.
pub struct Ctx {
    pub root: PathBuf,
    pub ws: Hive,
    pub git: Git,
    pub project: Option<String>,
    pub wt: Option<String>,
    pub progen: Vec<String>,
    pub progen_group: Vec<String>,
}

impl Ctx {
    /// Discover root, load workspace, attach git + globals (`--wt` already resolved).
    pub fn open(
        root_flag: Option<&Path>,
        project: Option<String>,
        wt: Option<String>,
        progen: Vec<String>,
        progen_group: Vec<String>,
    ) -> Result<Self, HiveError> {
        let root = discover_root(root_flag, &std::env::current_dir()?)?;
        let ws = load_hive(&root)?;
        Ok(Self {
            root,
            ws,
            git: Git::new(),
            project,
            wt,
            progen,
            progen_group,
        })
    }
}
