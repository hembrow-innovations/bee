use std::path::Path;

use odm_core::{
    abs_checkout, pin_apply, resolve_managed, save_pin, CheckoutMode, OdmError, PinEntry, PinFile,
};
use odm_git::Git;

use crate::load_workbench;

pub fn pin_record_primary(root: &Path, names: &[String]) -> Result<(), OdmError> {
    let wb = load_workbench(root)?;
    let git = Git::new();
    let entities = resolve_managed(&wb.config, names)?;
    let mut pin = odm_core::load_pin(root)?.unwrap_or_else(PinFile::new_v1);
    for entity in entities {
        if entity.checkout != CheckoutMode::Clone {
            continue;
        }
        let path = abs_checkout(root, &entity.path)?;
        if !path.exists() || !git.is_repo(&path)? {
            return Err(OdmError::not_found(format!(
                "path is not a git repo for '{}': {}",
                entity.name, entity.path
            )));
        }
        let rev = git.head_sha(&path)?;
        pin.pins.insert(
            entity.name,
            PinEntry {
                rev,
                url: entity.url,
                branch: entity.branch,
            },
        );
    }
    save_pin(root, &pin)
}

pub fn pin_apply_primary(root: &Path, names: &[String], force: bool) -> Result<(), OdmError> {
    let wb = load_workbench(root)?;
    let git = Git::new();
    pin_apply(&git, root, &wb.config, names, force)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_fixture::{bare_with_main, git_user, head_sha};
    use crate::init_workbench;
    use crate::pin_path;
    use crate::sync::sync_workbench;
    use crate::workbench_path;
    use std::fs;
    use std::process::Command;
    use tempfile::tempdir;

    fn seeded(root: &std::path::Path) {
        init_workbench(root).unwrap();
        let bare = bare_with_main(root, "alpha");
        fs::write(
            workbench_path(root),
            format!(
                "projects:\n  alpha:\n    path: projects/alpha\n    url: {}\n    branch: main\n",
                bare.display()
            ),
        )
        .unwrap();
        sync_workbench(root, &[]).unwrap();
    }

    #[test]
    fn pin_record_writes_workbench_lock() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seeded(root);
        pin_record_primary(root, &[]).unwrap();
        let lock = pin_path(root);
        assert!(lock.is_file());
        let text = fs::read_to_string(&lock).unwrap();
        assert!(text.contains("alpha"));
        assert!(!root.join(".odm/odm.lock.yaml").exists());
    }

    #[test]
    fn pin_apply_restores_primary_sha() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seeded(root);
        pin_record_primary(root, &[]).unwrap();
        let primary = root.join("projects/alpha");
        let pinned = head_sha(&primary);
        git_user(&primary);
        fs::write(primary.join("extra"), "x").unwrap();
        assert!(Command::new("git")
            .args(["-C", primary.to_str().unwrap(), "add", "extra"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["-C", primary.to_str().unwrap(), "commit", "-m", "move"])
            .status()
            .unwrap()
            .success());
        assert_ne!(head_sha(&primary), pinned);
        pin_apply_primary(root, &[], false).unwrap();
        assert_eq!(head_sha(&primary), pinned);
        let det = Command::new("git")
            .args(["-C", primary.to_str().unwrap(), "symbolic-ref", "-q", "HEAD"])
            .status()
            .unwrap();
        assert!(!det.success());
    }
}
