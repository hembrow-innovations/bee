use std::path::Path;

use hive_actions::{list_actions, run_action, CwdTarget, RunOptions, StdioMode};
use hive_core::{
    build_status, format_status_human, generate_local, run_doctor, CheckStatus, HiveError,
};
use hive_git::Git;
use hive_store::{context_notes, find_notes, format_context_human, format_find_human};

use crate::load_workbench;

pub fn doctor(root: &Path) -> Result<(), HiveError> {
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
        Err(HiveError::operation("doctor failed"))
    }
}

pub fn doctor_report(
    root: &Path,
) -> Result<hive_core::DoctorReport, HiveError> {
    let wb = load_workbench(root)?;
    let git = Git::new();
    run_doctor(&git, &wb, false)
}

pub fn status(root: &Path) -> Result<String, HiveError> {
    let wb = load_workbench(root)?;
    let git = Git::new();
    let snap = build_status(&git, &wb)?;
    Ok(format_status_human(&snap))
}

pub fn find(root: &Path, query: Option<String>, limit: usize) -> Result<String, HiveError> {
    let wb = load_workbench(root)?;
    let q = query.unwrap_or_default();
    let hits = find_notes(&wb, &q, &[], &[], limit)?;
    Ok(format_find_human(&hits))
}

pub fn context(root: &Path, id: &str) -> Result<String, HiveError> {
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
) -> Result<i32, HiveError> {
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
) -> Result<(), HiveError> {
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
                HiveError::usage("generate requires --dest <path> when a name is given")
            })?;
            generate_local(&wb, &name, &dest, force, dry_run)?;
            Ok(())
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::git_fixture::bare_with_main;
    use crate::init_workbench;
    use crate::project::add_project;
    use crate::workbench_path;
    use clap::Parser;
    use hive_actions::CwdTarget;
    use hive_store::ensure_vault;
    use std::fs;
    use tempfile::tempdir;

    fn hive_with_neighborhood() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let root = dir.path();
        init_workbench(root).unwrap();
        let vault = root.join("mem");
        ensure_vault(&vault).unwrap();
        fs::write(
            vault.join("alpha.md"),
            "---\nid: a1\ntitle: Alpha\n---\nSee [[Beta]].\n",
        )
        .unwrap();
        fs::write(
            vault.join("beta.md"),
            "---\nid: b1\ntitle: Beta\n---\nOther.\n",
        )
        .unwrap();
        fs::write(workbench_path(root), "progens:\n  mem:\n    path: mem\n").unwrap();
        dir
    }

    fn write_rel(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn hive_with_pack_fixture() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let root = dir.path();
        init_workbench(root).unwrap();
        write_rel(
            root,
            "docs/guides/guides-agent-gotchas.md",
            "# Agent gotchas\n",
        );
        write_rel(
            root,
            "docs/specs/bee/scan/purpose.md",
            "# Scan purpose\n",
        );
        for name in ["alpha", "bravo", "charlie", "delta", "echo"] {
            write_rel(
                root,
                &format!("docs/specs/bee/scan/{name}/contract.md"),
                &format!("# {name} contract\n"),
            );
        }
        for i in 1..=12 {
            write_rel(
                root,
                &format!("docs/specs/bee/scan/extra-{i:02}.md"),
                &format!("# extra {i}\n"),
            );
        }
        dir
    }

    fn markdown_section<'a>(text: &'a str, heading: &str) -> &'a str {
        let mut start = None;
        let mut end = text.len();
        let mut off = 0;
        for line in text.lines() {
            let name = line.trim_start_matches('#').trim();
            let is_heading = line.trim().starts_with('#') && !name.is_empty();
            if is_heading && name == heading && start.is_none() {
                start = Some(off + line.len());
            } else if is_heading && start.is_some() {
                end = off;
                break;
            }
            off += line.len() + 1;
        }
        match start {
            Some(s) => {
                let s = if s < text.len() && text.as_bytes().get(s) == Some(&b'\n') {
                    s + 1
                } else {
                    s
                };
                &text[s.min(text.len())..end.min(text.len())]
            }
            None => "",
        }
    }

    fn pack_stdout(root: &Path) -> String {
        let cli = crate::Cli::try_parse_from(["bee", "context", "--area", "scan"]).unwrap();
        assert_eq!(crate::execute(cli, root), 0);
        crate::pack::build_pack(
            root,
            crate::pack::PackSelectors {
                area: Some("scan".into()),
                query: None,
                unit: None,
                domain: None,
                k: 10,
            },
        )
        .unwrap()
    }

    #[test]
    pub(crate) fn doctor_warns_without_fixing_orphan_slot() {
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
        println!("odm.wt:doctor-warn");
    }

    #[test]
    pub(crate) fn run_wt_missing_slot_exits_4() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        init_workbench(root).unwrap();
        fs::create_dir_all(root.join("projects/alpha")).unwrap();
        fs::create_dir_all(root.join("actions")).unwrap();
        fs::write(
            root.join("actions/core.yaml"),
            "pwdhere:\n  tasks:\n    - run: echo hi\n",
        )
        .unwrap();
        fs::write(
            workbench_path(root),
            "projects:\n  alpha:\n    path: projects/alpha\nactions:\n  core: actions/core.yaml\n",
        )
        .unwrap();

        let missing = crate::Cli::try_parse_from([
            "bee",
            "run",
            "--project",
            "alpha",
            "--wt",
            "missing",
            "pwdhere",
        ])
        .unwrap();
        assert_eq!(crate::execute(missing, root), 4);
        assert!(!root.join("worktrees/alpha/missing").exists());

        let bare_wt =
            crate::Cli::try_parse_from(["bee", "run", "--wt", "missing", "pwdhere"]).unwrap();
        assert_eq!(crate::execute(bare_wt, root), 1);
        assert!(!root.join("worktrees").exists());
        println!("odm.wt:no-auto-create");
    }

    #[test]
    pub(crate) fn context_id_prints_neighborhood() {
        let dir = hive_with_neighborhood();
        let root = dir.path();
        let cli = crate::Cli::try_parse_from(["bee", "context", "a1"]).unwrap();
        assert_eq!(crate::execute(cli, root), 0);
        let text = context(root, "a1").unwrap();
        assert!(text.starts_with("# context a1\n"), "{text}");
        assert!(text.contains("## outgoing\n- b1 (Beta)\n"), "{text}");
        assert!(text.contains("## incoming\n"), "{text}");
        println!("bee.context:neighborhood");
    }

    #[test]
    pub(crate) fn context_id_and_selector_exits_usage() {
        let dir = hive_with_neighborhood();
        let root = dir.path();
        for flag in ["--area", "--query", "--unit", "--domain"] {
            let parsed = crate::Cli::try_parse_from(["bee", "context", "a1", flag, "bee"]);
            assert!(parsed.is_ok(), "id plus {flag} must parse then usage-exit");
            assert_eq!(crate::execute(parsed.unwrap(), root), 1, "{flag}");
        }
        match crate::Cli::try_parse_from(["bee", "context"]) {
            Ok(cli) => assert_eq!(crate::execute(cli, root), 1),
            Err(_) => {}
        }
        println!("bee.context:usage");
    }

    #[test]
    pub(crate) fn context_pack_prints_markdown() {
        let dir = hive_with_pack_fixture();
        let text = pack_stdout(dir.path());
        for heading in [
            "Query",
            "Area",
            "Must read",
            "Related",
            "Excluded",
            "Next",
        ] {
            assert!(
                text.lines()
                    .any(|l| l.trim_start_matches('#').trim() == heading),
                "missing heading {heading} in {text:?}"
            );
        }
        let must = markdown_section(&text, "Must read");
        assert!(
            must.contains("guides-agent-gotchas.md"),
            "must read missing always-on guide: {must:?}"
        );
        assert!(
            must.contains("purpose.md"),
            "must read missing area purpose: {must:?}"
        );
        let contracts = must.matches("contract.md").count();
        assert!(
            contracts <= 4,
            "must read has {contracts} contracts: {must:?}"
        );
        let related = markdown_section(&text, "Related");
        let related_hits = related.matches(".md").count();
        assert!(
            related_hits <= 10,
            "related has {related_hits} hits: {related:?}"
        );
        println!("bee.context:pack-md");
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
