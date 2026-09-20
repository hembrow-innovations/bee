//! Progen membership composition — core membership + vault scaffold.

use std::path::Path;

use hive_core::{
    abs_checkout, progen_add as core_progen_add, progen_rm as core_progen_rm, MaterializeOutcome,
    HiveError, ProgenEntry, HiveConfig,
};
use hive_git::Git;

use crate::vault::ensure_vault;

/// Add a Progen: core membership + Obsidian vault scaffold with prior timing rules.
///
/// - path-only: always vault (creates path)
/// - managed after successful materialize: vault if path exists
/// - `--no-clone` managed: skip vault when no materialize outcome
pub fn add_progen<R: hive_git::CommandRunner>(
    git: &Git<R>,
    root: &Path,
    config: &mut HiveConfig,
    name: &str,
    entry: ProgenEntry,
    no_clone: bool,
) -> Result<Option<MaterializeOutcome>, HiveError> {
    let rel = entry.path.clone();
    let managed = entry.url.is_some();
    let outcome = core_progen_add(git, root, config, name, entry, no_clone)?;

    // Same vault timing as the former CLI-injected ensure_vault callback.
    if !managed || outcome.is_some() {
        let abs = abs_checkout(root, &rel)?;
        if !managed || abs.exists() {
            ensure_vault(&abs)?;
        }
    }

    Ok(outcome)
}

/// Remove a Progen (delegates to core membership; groups + index handled there).
pub fn rm_progen<R: hive_git::CommandRunner>(
    git: &Git<R>,
    root: &Path,
    config: &mut HiveConfig,
    name: &str,
    delete: bool,
    force: bool,
) -> Result<(), HiveError> {
    core_progen_rm(git, root, config, name, delete, force)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hive_core::{init_hive, InitOptions, HiveConfig};
    use tempfile::tempdir;

    #[test]
    fn path_only_add_scaffolds_vault_without_bin_closure() {
        let dir = tempdir().unwrap();
        let res = init_hive(InitOptions {
            path: dir.path().to_path_buf(),
            no_git: true,
            name: None,
        })
        .unwrap();
        let root = res.root;
        let g = Git::new();
        let mut cfg = HiveConfig::default();
        let outcome = add_progen(
            &g,
            &root,
            &mut cfg,
            "desk",
            ProgenEntry {
                path: "vaults/desk".into(),
                url: None,
                branch: None,
                ..Default::default()
            },
            false,
        )
        .unwrap();
        assert!(outcome.is_none());
        assert!(cfg.progens.contains_key("desk"));
        let vault = root.join("vaults/desk");
        assert!(vault.join("README.md").is_file());
        assert!(vault.join(".obsidian/app.json").is_file());
    }

    #[test]
    fn managed_no_clone_skips_vault() {
        let dir = tempdir().unwrap();
        let res = init_hive(InitOptions {
            path: dir.path().to_path_buf(),
            no_git: true,
            name: None,
        })
        .unwrap();
        let root = res.root;
        let g = Git::new();
        let mut cfg = HiveConfig::default();
        let outcome = add_progen(
            &g,
            &root,
            &mut cfg,
            "remote",
            ProgenEntry {
                path: "vaults/remote".into(),
                url: Some("https://example.com/remote.git".into()),
                branch: Some("main".into()),
                ..Default::default()
            },
            true, // no_clone
        )
        .unwrap();
        assert!(outcome.is_none());
        assert!(cfg.progens.contains_key("remote"));
        assert!(!root.join("vaults/remote").exists());
    }
}
