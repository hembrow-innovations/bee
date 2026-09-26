use std::path::Path;

use hive_core::{
    project_add, project_git, project_rm, CheckoutMode, HiveError, ProgenEntry, ProjectEntry,
};
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

pub fn git_project(
    root: &Path,
    name: &str,
    git_args: &[String],
    wt: Option<&str>,
) -> Result<std::process::ExitStatus, HiveError> {
    let wb = load_workbench(root)?;
    let git = Git::new();
    project_git(&git, &wb, name, git_args, wt)
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
pub(crate) mod tests {
    use super::*;
    use crate::git_fixture::bare_with_main;
    use crate::init_workbench;
    use crate::workbench_path;
    use clap::Parser;
    use std::fs;
    use std::path::Path;
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

    fn register_project(root: &Path, name: &str, rel: &str) {
        fs::write(
            workbench_path(root),
            format!(
                "projects:\n  {name}:\n    path: {rel}\n    url: https://example.com/{name}.git\n    branch: main\n"
            ),
        )
        .unwrap();
    }

    fn current_branch(repo: &Path) -> String {
        let out = std::process::Command::new("git")
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

    fn git_cli(wt: Option<&str>, project: &str, git_args: &[&str]) -> crate::Cli {
        let mut args = vec!["bee".to_string()];
        if let Some(slot) = wt {
            args.push("--wt".into());
            args.push(slot.into());
        }
        args.extend(["project".into(), "git".into(), project.into(), "--".into()]);
        args.extend(git_args.iter().map(|s| (*s).to_string()));
        let argv: Vec<&str> = args.iter().map(String::as_str).collect();
        crate::Cli::try_parse_from(argv).unwrap()
    }

    #[test]
    pub(crate) fn project_git_wt_missing_slot() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        init_workbench(&root).unwrap();
        let rel = "projects/alpha";
        fs::create_dir_all(root.join(rel)).unwrap();
        crate::git_fixture::git_init_commit(&root.join(rel));
        register_project(&root, "alpha", rel);

        let cli = git_cli(Some("missing"), "alpha", &["status"]);
        match &cli.command {
            crate::Commands::Project {
                cmd: crate::ProjectCmd::Git { name, git_args },
            } => {
                assert_eq!(name, "alpha");
                assert_eq!(git_args, &vec!["status".to_string()]);
            }
            other => panic!("expected project git, got {other:?}"),
        }
        assert_eq!(cli.wt, vec!["missing".to_string()]);
        assert_eq!(crate::execute(cli, &root), 4);
        assert!(!root.join("worktrees/alpha/missing").exists());
        assert!(!root.join("worktrees").exists());
        println!("odm.wt:no-auto-create");
    }

    #[test]
    pub(crate) fn project_git_wt_skips_pin() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        init_workbench(&root).unwrap();
        crate::git_fixture::git_init_commit(&root);
        let rel = "projects/alpha";
        let primary = root.join(rel);
        fs::create_dir_all(&primary).unwrap();
        crate::git_fixture::git_init_commit(&primary);
        register_project(&root, "alpha", rel);

        let add = crate::Cli::try_parse_from([
            "bee", "project", "worktree", "add", "alpha", "agent", "--branch", "topic",
        ])
        .unwrap();
        assert_eq!(crate::execute(add, &root), 0);
        let slot = root.join("worktrees/alpha/agent");
        assert!(slot.is_dir());
        assert_eq!(current_branch(&slot), "topic");
        assert_ne!(current_branch(&primary), "topic");

        let pin = crate::pin_path(&root);
        assert_eq!(
            pin.strip_prefix(&root).unwrap(),
            Path::new(".hivemind/workbench.lock.yaml")
        );
        let before_pin = fs::read(&pin).unwrap();
        let before_primary = crate::git_fixture::head_sha(&primary);
        let before_slot = crate::git_fixture::head_sha(&slot);

        let cli = crate::Cli::try_parse_from([
            "bee",
            "--project",
            "decoy",
            "--wt",
            "agent",
            "project",
            "git",
            "alpha",
            "--",
            "commit",
            "--allow-empty",
            "-m",
            "slot-review",
        ])
        .unwrap();
        assert_eq!(cli.project.as_deref(), Some("decoy"));
        assert_eq!(crate::execute(cli, &root), 0);

        assert_eq!(fs::read(&pin).unwrap(), before_pin);
        assert_eq!(crate::git_fixture::head_sha(&primary), before_primary);
        assert_ne!(crate::git_fixture::head_sha(&slot), before_slot);
        assert_eq!(current_branch(&slot), "topic");
        assert_ne!(current_branch(&primary), "topic");
        println!("odm.wt:flag-agree");
    }

    #[test]
    fn project_git_empty_args_exits_1() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        init_workbench(&root).unwrap();
        register_project(&root, "alpha", "projects/alpha");
        fs::create_dir_all(root.join("projects/alpha")).unwrap();

        let bare = crate::Cli::try_parse_from(["bee", "project", "git", "alpha"]).unwrap();
        assert_eq!(crate::execute(bare, &root), 1);
        let dashed = git_cli(None, "alpha", &[]);
        assert_eq!(crate::execute(dashed, &root), 1);
        assert!(crate::Cli::try_parse_from(["bee", "pr"]).is_err());
        assert!(crate::Cli::try_parse_from(["bee", "project", "pr"]).is_err());

        let clash = crate::Cli::try_parse_from([
            "bee", "--wt", "a", "--wt", "b", "project", "git", "alpha", "--", "status",
        ])
        .unwrap();
        assert_eq!(crate::execute(clash, &root), 1);
        assert!(!root.join("worktrees").exists());
    }
}
