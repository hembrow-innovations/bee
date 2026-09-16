use std::path::Path;

use odm_actions::{list_actions, run_action, CwdTarget, RunOptions, StdioMode};
use odm_core::{
    build_status, format_status_human, generate_local, run_doctor, CheckStatus, OdmError,
};
use odm_git::Git;
use odm_progen::{context_notes, find_notes, format_context_human, format_find_human};

use crate::load_workbench;

pub fn doctor(root: &Path) -> Result<(), OdmError> {
    let wb = load_workbench(root)?;
    let git = Git::new();
    let report = run_doctor(&git, &wb, false)?;
    for c in &report.checks {
        if c.status != CheckStatus::Pass {
            eprintln!("{}\t{:?}\t{}", c.id, c.status, c.message);
        }
    }
    if report.ok {
        Ok(())
    } else {
        Err(OdmError::operation("doctor failed"))
    }
}

pub fn doctor_report(
    root: &Path,
) -> Result<odm_core::DoctorReport, OdmError> {
    let wb = load_workbench(root)?;
    let git = Git::new();
    run_doctor(&git, &wb, false)
}

pub fn status(root: &Path) -> Result<String, OdmError> {
    let wb = load_workbench(root)?;
    let git = Git::new();
    let snap = build_status(&git, &wb)?;
    Ok(format_status_human(&snap))
}

pub fn find(root: &Path, query: Option<String>, limit: usize) -> Result<String, OdmError> {
    let wb = load_workbench(root)?;
    let q = query.unwrap_or_default();
    let hits = find_notes(&wb, &q, &[], &[], limit)?;
    Ok(format_find_human(&hits))
}

pub fn context(root: &Path, id: &str) -> Result<String, OdmError> {
    let wb = load_workbench(root)?;
    let hit = context_notes(&wb, id, None)?;
    Ok(format_context_human(&hit))
}

pub fn run(
    root: &Path,
    action: Option<String>,
    extra: &[String],
    project: Option<&str>,
    wt: Option<&str>,
) -> Result<i32, OdmError> {
    let wb = load_workbench(root)?;
    let Some(name) = action else {
        let listed = list_actions(&wb);
        for (n, _) in listed {
            println!("{n}");
        }
        return Ok(0);
    };
    let cwd = CwdTarget::from_flags(project, wt)?;
    let result = run_action(
        &wb,
        &name,
        RunOptions {
            cwd,
            extra_args: extra,
            stdio: StdioMode::Inherit,
        },
    )?;
    Ok(result.exit_code)
}

pub fn generate(
    root: &Path,
    name: Option<String>,
    dest: Option<String>,
    force: bool,
    dry_run: bool,
) -> Result<(), OdmError> {
    let wb = load_workbench(root)?;
    match name {
        None => {
            for n in wb.generators.keys() {
                println!("{n}");
            }
            Ok(())
        }
        Some(name) => {
            let dest = dest.ok_or_else(|| {
                OdmError::usage("generate requires --dest <path> when a name is given")
            })?;
            generate_local(&wb, &name, &dest, force, dry_run)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_fixture::bare_with_main;
    use crate::init_workbench;
    use crate::project::add_project;
    use crate::workbench_path;
    use odm_actions::CwdTarget;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn doctor_warns_without_fixing_orphan_slot() {
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
        let orphan = root.join("worktrees/alpha/slot1");
        fs::create_dir_all(&orphan).unwrap();
        let report = doctor_report(root).unwrap();
        assert!(report
            .checks
            .iter()
            .any(|c| c.id.contains("worktree_orphan") && c.status == CheckStatus::Warn));
        assert!(!report
            .checks
            .iter()
            .any(|c| c.id.contains("worktree_orphan") && c.fixable));
        assert!(orphan.is_dir());
    }

    #[test]
    fn status_runs_on_workbench() {
        let dir = tempdir().unwrap();
        init_workbench(dir.path()).unwrap();
        let text = status(dir.path()).unwrap();
        assert!(text.contains(dir.path().to_str().unwrap()) || text.contains("no projects"));
    }

    #[test]
    fn run_binds_project_and_wt() {
        assert!(CwdTarget::from_flags(None, Some("feat")).is_err());
        assert!(matches!(
            CwdTarget::from_flags(Some("alpha"), Some("feat")).unwrap(),
            CwdTarget::Worktree {
                project: "alpha",
                slot: "feat"
            }
        ));
        assert!(matches!(
            CwdTarget::from_flags(Some("alpha"), None).unwrap(),
            CwdTarget::Project { name: "alpha" }
        ));
    }

    #[test]
    fn generate_writes_template() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        init_workbench(root).unwrap();
        fs::create_dir_all(root.join("tmpl")).unwrap();
        fs::write(root.join("tmpl/hi.txt"), "hi").unwrap();
        fs::write(
            workbench_path(root),
            "generators:\n  core: g.yaml\n",
        )
        .unwrap();
        fs::write(root.join("g.yaml"), "pkg:\n  template: tmpl\n").unwrap();
        generate(root, Some("pkg".into()), Some("out".into()), false, false).unwrap();
        assert_eq!(fs::read_to_string(root.join("out/hi.txt")).unwrap(), "hi");
    }
}
