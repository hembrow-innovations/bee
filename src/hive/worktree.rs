use std::path::Path;

use hive_core::HiveError;
use hive_git::Git;

use crate::load_workbench;

pub fn list_worktrees(root: &Path, project: &str) -> Result<(), HiveError> {
    let text = render_list(root, project)?;
    print!("{text}");
    Ok(())
}

pub fn add_worktree(
    root: &Path,
    project: &str,
    slot: &str,
    branch: Option<&str>,
) -> Result<(), HiveError> {
    let wb = load_workbench(root)?;
    let git = Git::new();
    hive_core::worktree_add(&git, &wb, project, slot, branch)?;
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

    fn current_branch(repo: &Path) -> String {
        let out = Command::new("git")
            .args(["-C", repo.to_str().unwrap(), "branch", "--show-current"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn git_project(root: &Path, name: &str) -> std::path::PathBuf {
        init_workbench(root).unwrap();
        let rel = format!("projects/{name}");
        let primary = root.join(&rel);
        fs::create_dir_all(&primary).unwrap();
        git_init_commit(&primary);
        register_project(root, name, &rel);
        primary
    }

    fn add_cli(project: &str, slot: &str, branch: Option<&str>) -> crate::Cli {
        let mut args = vec![
            "bee".to_string(),
            "project".to_string(),
            "worktree".to_string(),
            "add".to_string(),
            project.to_string(),
            slot.to_string(),
        ];
        if let Some(branch) = branch {
            args.push("--branch".into());
            args.push(branch.into());
        }
        let argv: Vec<&str> = args.iter().map(String::as_str).collect();
        crate::Cli::try_parse_from(argv).unwrap()
    }

    fn assert_add(cli: &crate::Cli, project: &str, slot: &str, branch: Option<&str>) {
        match &cli.command {
            crate::Commands::Project {
                cmd:
                    crate::ProjectCmd::Worktree {
                        cmd:
                            crate::WorktreeCmd::Add {
                                project: got_project,
                                slot: got_slot,
                                branch: got_branch,
                            },
                    },
            } => {
                assert_eq!(got_project, project);
                assert_eq!(got_slot, slot);
                assert_eq!(got_branch.as_deref(), branch);
            }
            other => panic!("expected worktree add, got {other:?}"),
        }
    }

    #[test]
    pub(crate) fn wt_add_branch_primary_unmoved() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let primary = git_project(&root, "alpha");
        let before = head_sha(&primary);

        let cli = add_cli("alpha", "agent", Some("topic"));
        assert_add(&cli, "alpha", "agent", Some("topic"));
        assert_eq!(crate::execute(cli, &root), 0);

        assert_eq!(head_sha(&primary), before);
        let slot = root.join("worktrees/alpha/agent");
        assert!(slot.is_dir());
        assert!(!slot.starts_with(root.join(".odm")));
        assert_eq!(current_branch(&slot), "topic");

        let omit = add_cli("alpha", "same", None);
        assert_add(&omit, "alpha", "same", None);
        let omit_code = crate::execute(omit, &root);
        assert_ne!(omit_code, 1, "omitted --branch must not be a bin refusal");
        assert_eq!(head_sha(&primary), before);
        println!("primary-head:unchanged");
    }

    #[test]
    pub(crate) fn wt_add_refuses_existing_path() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let primary = git_project(&root, "alpha");
        let before = head_sha(&primary);

        let first = add_cli("alpha", "agent", Some("topic"));
        assert_eq!(crate::execute(first, &root), 0);
        let slot = root.join("worktrees/alpha/agent");
        assert_eq!(
            slot.strip_prefix(&root).unwrap().to_str().unwrap(),
            "worktrees/alpha/agent"
        );
        fs::write(slot.join("marker"), "keep").unwrap();

        let err = add_worktree(&root, "alpha", "agent", Some("other")).unwrap_err();
        assert_eq!(hive_core::exit_code(&err), 3);
        assert!(err.to_string().contains("worktrees/alpha/agent"), "{err}");
        assert!(!err.to_string().contains(".odm/"));

        let second = add_cli("alpha", "agent", Some("other"));
        assert_add(&second, "alpha", "agent", Some("other"));
        assert_eq!(crate::execute(second, &root), 3);

        assert!(slot.join("marker").is_file());
        assert_eq!(fs::read_to_string(slot.join("marker")).unwrap(), "keep");
        assert_eq!(current_branch(&slot), "topic");
        assert_eq!(head_sha(&primary), before);
        println!("odm.wt:path");
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
