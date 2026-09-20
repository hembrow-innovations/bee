use std::path::Path;

use hive_core::{project_add, project_rm, CheckoutMode, HiveError, ProgenEntry, ProjectEntry};
use hive_git::Git;
use hive_store::add_progen;

use crate::load_workbench;

fn checkout_mode(gitlink: bool) -> CheckoutMode {
    if gitlink {
        CheckoutMode::Gitlink
    } else {
        CheckoutMode::Clone
    }
}

pub fn add_project(
    root: &Path,
    name: &str,
    path: String,
    url: Option<String>,
    branch: Option<String>,
    gitlink: bool,
) -> Result<(), HiveError> {
    let mut wb = load_workbench(root)?;
    let git = Git::new();
    project_add(
        &git,
        root,
        &mut wb.config,
        name,
        ProjectEntry {
            path,
            url,
            branch,
            type_: None,
            checkout: checkout_mode(gitlink),
        },
        false,
    )?;
    Ok(())
}

pub fn rm_project(root: &Path, name: &str) -> Result<(), HiveError> {
    let mut wb = load_workbench(root)?;
    let git = Git::new();
    project_rm(&git, root, &mut wb.config, name, false, false)?;
    Ok(())
}

pub fn add_progen_checkout(
    root: &Path,
    name: &str,
    path: String,
    url: Option<String>,
    branch: Option<String>,
    gitlink: bool,
) -> Result<(), HiveError> {
    let mut wb = load_workbench(root)?;
    let git = Git::new();
    add_progen(
        &git,
        root,
        &mut wb.config,
        name,
        ProgenEntry {
            path,
            url,
            branch,
            checkout: checkout_mode(gitlink),
        },
        false,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_fixture::bare_with_main;
    use crate::init_workbench;
    use crate::workbench_path;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn project_add_clones_default_membership() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        init_workbench(root).unwrap();
        let bare = bare_with_main(root, "alpha");
        add_project(
            root,
            "alpha",
            "projects/alpha".into(),
            Some(bare.to_string_lossy().into()),
            Some("main".into()),
            false,
        )
        .unwrap();
        assert!(root.join("projects/alpha/.git").exists());
        let yaml = fs::read_to_string(workbench_path(root)).unwrap();
        assert!(yaml.contains("alpha"));
        assert!(yaml.contains("projects/alpha"));
        assert!(!yaml.contains("gitlink"));
    }

    #[test]
    fn progen_add_nested_path_only() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        init_workbench(root).unwrap();
        add_progen_checkout(root, "desk", "generated/desk".into(), None, None, false).unwrap();
        assert!(root.join("generated/desk").is_dir());
        let yaml = fs::read_to_string(workbench_path(root)).unwrap();
        assert!(yaml.contains("desk"));
        assert!(yaml.contains("generated/desk"));
        assert!(!yaml.contains("gitlink"));
    }

    #[test]
    fn project_and_progen_add_gitlink_opt_in() {
        crate::git_fixture::allow_file_protocol();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        crate::init_workbench(root).unwrap();
        crate::git_fixture::git_init_commit(root);
        let bare_p = crate::git_fixture::bare_with_main(root, "plink");
        add_project(
            root,
            "plink",
            "vendor/plink".into(),
            Some(bare_p.to_string_lossy().into()),
            Some("main".into()),
            true,
        )
        .unwrap();
        let bare_g = crate::git_fixture::bare_with_main(root, "glink");
        add_progen_checkout(
            root,
            "glink",
            "vendor/glink".into(),
            Some(bare_g.to_string_lossy().into()),
            Some("main".into()),
            true,
        )
        .unwrap();
        let yaml = std::fs::read_to_string(crate::workbench_path(root)).unwrap();
        assert!(yaml.contains("checkout: gitlink"), "{yaml}");
        assert!(root.join("vendor/plink").exists());
        assert!(root.join("vendor/glink").exists());
        let lock = std::fs::read_to_string(crate::pin_path(root)).unwrap_or_default();
        assert!(!lock.contains("plink"));
        assert!(!lock.contains("glink"));
    }
}
