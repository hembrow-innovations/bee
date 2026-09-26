use std::path::Path;

use hive_core::HiveError;
use hive_git::Git;

use crate::load_workbench;

pub fn list_worktrees(root: &Path, project: &str) -> Result<(), HiveError> {
    let text = render_list(root, project)?;
    print!("{text}");
    Ok(())
}

fn render_list(root: &Path, project: &str) -> Result<String, HiveError> {
    let wb = load_workbench(root)?;
    let git = Git::new();
    let out = hive_core::worktree_list(&git, &wb, project)?;
    let mut text = String::new();
    for slot in &out.slots {
        text.push_str(&slot.name);
        text.push(' ');
        text.push_str(&slot.path);
        text.push('\n');
    }
    Ok(text)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::git_fixture::{git_init_commit, head_sha};
    use crate::init_workbench;
    use crate::workbench_path;
    use clap::Parser;
    use std::fs;
    use std::process::Command;
    use tempfile::tempdir;

    fn register_project(root: &Path, name: &str, rel: &str) {
        fs::write(
            workbench_path(root),
            format!("projects:\n  {name}:\n    path: {rel}\n"),
        )
        .unwrap();
    }

    #[test]
    pub(crate) fn wt_list_prints_slot_path() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        init_workbench(&root).unwrap();
        let rel = "projects/alpha";
        let primary = root.join(rel);
        fs::create_dir_all(&primary).unwrap();
        git_init_commit(&primary);
        register_project(&root, "alpha", rel);
        let before = head_sha(&primary);

        let empty = render_list(&root, "alpha").unwrap();
        assert!(empty.is_empty(), "{empty}");
        assert!(!root.join("worktrees").exists());
        assert_eq!(head_sha(&primary), before);

        let slot = root.join("worktrees/alpha/agent");
        fs::create_dir_all(slot.parent().unwrap()).unwrap();
        assert!(Command::new("git")
            .args([
                "-C",
                primary.to_str().unwrap(),
                "worktree",
                "add",
                "-b",
                "agent",
                slot.to_str().unwrap(),
            ])
            .status()
            .unwrap()
            .success());

        let text = render_list(&root, "alpha").unwrap();
        assert_eq!(text, "agent worktrees/alpha/agent\n");
        assert_eq!(head_sha(&primary), before);

        let cli = crate::Cli::try_parse_from([
            "bee", "--wt", "ignored", "project", "worktree", "list", "alpha",
        ])
        .unwrap();
        assert_eq!(crate::execute(cli, &root), 0);
        println!("odm.wt:path");
    }

    #[test]
    pub(crate) fn wt_list_non_git_exits_3() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        init_workbench(&root).unwrap();
        fs::create_dir_all(root.join("projects/plain")).unwrap();
        fs::write(root.join("projects/plain/README"), "x").unwrap();
        register_project(&root, "plain", "projects/plain");
        let err = render_list(&root, "plain").unwrap_err();
        assert_eq!(hive_core::exit_code(&err), 3);
        let cli =
            crate::Cli::try_parse_from(["bee", "project", "worktree", "list", "plain"]).unwrap();
        assert_eq!(crate::execute(cli, &root), 3);
        println!("odm.wt:git-project");
    }

    #[test]
    fn wt_list_unknown_project_exits_1() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        init_workbench(&root).unwrap();
        let err = render_list(&root, "missing").unwrap_err();
        assert_eq!(hive_core::exit_code(&err), 1);
        let cli =
            crate::Cli::try_parse_from(["bee", "project", "worktree", "list", "missing"]).unwrap();
        assert_eq!(crate::execute(cli, &root), 1);
    }
}
