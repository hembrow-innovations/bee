use std::path::Path;

use odm_core::{project_add, CheckoutMode, OdmError, ProgenEntry, ProjectEntry};
use odm_git::Git;
use odm_progen::add_progen;

use crate::load_workbench;

pub fn add_project(
    root: &Path,
    name: &str,
    path: String,
    url: Option<String>,
    branch: Option<String>,
) -> Result<(), OdmError> {
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
            checkout: CheckoutMode::Clone,
        },
        false,
    )?;
    Ok(())
}

pub fn add_progen_checkout(
    root: &Path,
    name: &str,
    path: String,
    url: Option<String>,
    branch: Option<String>,
) -> Result<(), OdmError> {
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
            checkout: CheckoutMode::Clone,
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
        add_progen_checkout(root, "desk", "generated/desk".into(), None, None).unwrap();
        assert!(root.join("generated/desk").is_dir());
        let yaml = fs::read_to_string(workbench_path(root)).unwrap();
        assert!(yaml.contains("desk"));
        assert!(yaml.contains("generated/desk"));
        assert!(!yaml.contains("gitlink"));
    }
}
