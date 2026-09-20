use std::path::Path;

use hive_core::{sync_managed, HiveError};
use hive_git::Git;

use crate::load_workbench;

pub fn sync_workbench(root: &Path, names: &[String]) -> Result<(), HiveError> {
    let wb = load_workbench(root)?;
    let git = Git::new();
    sync_managed(&git, &wb.root, &wb.config, names)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_fixture::{bare_with_main, git_user, head_sha};
    use crate::init_workbench;
    use crate::workbench_path;
    use std::fs;
    use std::process::Command;
    use tempfile::tempdir;

    #[test]
    fn sync_fetches_without_moving_head() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        init_workbench(root).unwrap();
        let bare = bare_with_main(dir.path(), "alpha");
        fs::write(
            workbench_path(root),
            format!(
                "projects:\n  alpha:\n    path: projects/alpha\n    url: {}\n    branch: main\n",
                bare.display()
            ),
        )
        .unwrap();
        sync_workbench(root, &[]).unwrap();
        let primary = root.join("projects/alpha");
        let before = head_sha(&primary);
        git_user(&primary);
        fs::write(primary.join("next"), "2").unwrap();
        assert!(Command::new("git")
            .args(["-C", primary.to_str().unwrap(), "add", "next"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["-C", primary.to_str().unwrap(), "commit", "-m", "two"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["-C", primary.to_str().unwrap(), "push", "origin", "main"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["-C", primary.to_str().unwrap(), "reset", "--hard", &before])
            .status()
            .unwrap()
            .success());
        sync_workbench(root, &[]).unwrap();
        assert_eq!(head_sha(&primary), before);
        let fetched = Command::new("git")
            .args([
                "-C",
                primary.to_str().unwrap(),
                "rev-parse",
                "origin/main",
            ])
            .output()
            .unwrap();
        assert!(fetched.status.success());
        let remote = String::from_utf8(fetched.stdout).unwrap().trim().to_string();
        assert_ne!(remote, before);
    }
}
