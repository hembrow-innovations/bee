//! Integration tests for `odm project worktree` and `project git --wt`.
//! Requires real `git` on PATH (same as `cli_init` clone/sync flow).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use predicates::prelude::*;
use tempfile::tempdir;

fn odm() -> assert_cmd::Command {
    assert_cmd::Command::new(cargo_bin("odm"))
}

fn git_user(repo: &Path) {
    assert!(Command::new("git")
        .args(["-C", repo.to_str().unwrap(), "config", "user.email", "t@est"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["-C", repo.to_str().unwrap(), "config", "user.name", "t"])
        .status()
        .unwrap()
        .success());
}

fn bare_with_main(root: &Path, name: &str) -> PathBuf {
    let bare = root.join(format!("{name}.git"));
    assert!(Command::new("git")
        .args(["init", "--bare", bare.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    let seed = root.join(format!("{name}-seed"));
    assert!(Command::new("git")
        .args(["clone", bare.to_str().unwrap(), seed.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    git_user(&seed);
    fs::write(seed.join("README"), name).unwrap();
    assert!(Command::new("git")
        .args(["-C", seed.to_str().unwrap(), "add", "README"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["-C", seed.to_str().unwrap(), "commit", "-m", "init"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["-C", seed.to_str().unwrap(), "branch", "-M", "main"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["-C", seed.to_str().unwrap(), "push", "-u", "origin", "main"])
        .status()
        .unwrap()
        .success());
    bare
}

/// init workspace + project add with bare remote so primary is a real git checkout.
fn workspace_with_git_project(root: &Path) {
    odm()
        .args(["init", root.to_str().unwrap()])
        .assert()
        .success();

    let bare = bare_with_main(root, "alpha");

    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "add",
            "alpha",
            "--path",
            "projects/alpha",
            "--url",
            bare.to_str().unwrap(),
            "--branch",
            "main",
        ])
        .assert()
        .success();
}

#[test]
fn worktree_add_list_git_wt_rm_roundtrip() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    workspace_with_git_project(&root);

    let slot_path = root.join("worktrees/alpha/slot1");

    // add with --branch so primary's main is not double-checked-out
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "worktree",
            "add",
            "alpha",
            "slot1",
            "--branch",
            "slot1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("slot1"));

    assert!(slot_path.is_dir());
    assert!(Command::new("git")
        .args([
            "-C",
            slot_path.to_str().unwrap(),
            "rev-parse",
            "--is-inside-work-tree",
        ])
        .status()
        .unwrap()
        .success());

    // list human
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "worktree",
            "list",
            "alpha",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("slot1"));

    // list json
    let list_out = odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "--json",
            "project",
            "worktree",
            "list",
            "alpha",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let list_json: serde_json::Value = serde_json::from_slice(&list_out).unwrap();
    assert_eq!(list_json["project"], "alpha");
    assert_eq!(list_json["slots"][0]["name"], "slot1");
    assert_eq!(list_json["slots"][0]["path"], "worktrees/alpha/slot1");

    // project git --wt slot1 resolves to slot path
    let toplevel = odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "git",
            "alpha",
            "--wt",
            "slot1",
            "--",
            "rev-parse",
            "--show-toplevel",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let toplevel = String::from_utf8_lossy(&toplevel).trim().to_string();
    let expected = slot_path
        .canonicalize()
        .unwrap_or_else(|_| slot_path.clone());
    let got = PathBuf::from(&toplevel)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&toplevel));
    assert_eq!(got, expected, "git --wt toplevel should be slot path");

    // rm
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "worktree",
            "rm",
            "alpha",
            "slot1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("slot1"));

    assert!(!slot_path.exists());

    let list_after = odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "--json",
            "project",
            "worktree",
            "list",
            "alpha",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let list_after: serde_json::Value = serde_json::from_slice(&list_after).unwrap();
    assert_eq!(list_after["project"], "alpha");
    let slots = list_after["slots"].as_array().unwrap();
    assert!(slots.is_empty(), "list empty after rm: {list_after}");
}

#[test]
fn project_info_includes_registered_worktree_slots() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    workspace_with_git_project(&root);

    // empty before add
    let empty_out = odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "--json",
            "project",
            "info",
            "alpha",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let empty: serde_json::Value = serde_json::from_slice(&empty_out).unwrap();
    assert_eq!(empty["worktree_slots"], serde_json::json!([]));

    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "worktree",
            "add",
            "alpha",
            "slot1",
            "--branch",
            "slot1",
        ])
        .assert()
        .success();

    let info_out = odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "--json",
            "project",
            "info",
            "alpha",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let info: serde_json::Value = serde_json::from_slice(&info_out).unwrap();
    assert_eq!(info["worktree_slots"][0]["name"], "slot1");
    assert_eq!(info["worktree_slots"][0]["path"], "worktrees/alpha/slot1");

    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "info",
            "alpha",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("worktrees: slot1"));
}

#[test]
fn project_info_includes_worktree_orphans() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    workspace_with_git_project(&root);

    // empty orphans before any dirs
    let empty_out = odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "--json",
            "project",
            "info",
            "alpha",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let empty: serde_json::Value = serde_json::from_slice(&empty_out).unwrap();
    assert_eq!(empty["worktree_orphans"], serde_json::json!([]));

    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "worktree",
            "add",
            "alpha",
            "slot1",
            "--branch",
            "slot1",
        ])
        .assert()
        .success();

    // orphan dir not registered as a git worktree
    fs::create_dir_all(root.join("worktrees/alpha/stale")).unwrap();

    let info_out = odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "--json",
            "project",
            "info",
            "alpha",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let info: serde_json::Value = serde_json::from_slice(&info_out).unwrap();
    assert_eq!(info["worktree_slots"][0]["name"], "slot1");
    assert_eq!(
        info["worktree_orphans"],
        serde_json::json!([{ "name": "stale", "path": "worktrees/alpha/stale" }])
    );
    assert!(info["worktree_orphans"][0].get("dirty").is_none());
    // registered slot must not appear under orphans
    let orphan_names: Vec<&str> = info["worktree_orphans"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    assert!(!orphan_names.contains(&"slot1"));

    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "info",
            "alpha",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("orphans: stale"));
}

#[test]
fn project_git_wt_missing_slot_fails_without_creating_path() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    workspace_with_git_project(&root);

    let missing = root.join("worktrees/alpha/missing");

    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "git",
            "alpha",
            "--wt",
            "missing",
            "--",
            "rev-parse",
            "HEAD",
        ])
        .assert()
        .failure()
        .code(4)
        .stderr(predicate::str::contains("worktree slot not found"));

    assert!(
        !missing.exists(),
        "missing --wt must not create worktrees/.../missing"
    );
}

#[test]
fn project_git_dual_wt_conflict_usage() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    workspace_with_git_project(&root);

    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "--wt",
            "slot-a",
            "project",
            "git",
            "alpha",
            "--wt",
            "slot-b",
            "--",
            "status",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("conflicting --wt"));

    // equal values OK (still missing → 4)
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "--wt",
            "same",
            "project",
            "git",
            "alpha",
            "--wt",
            "same",
            "--",
            "status",
        ])
        .assert()
        .failure()
        .code(4);
}

#[test]
fn worktree_add_non_git_project_fails() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");

    odm()
        .args(["init", root.to_str().unwrap(), "--no-git"])
        .assert()
        .success();

    // path-only project: mkdir checkout, no git init
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "add",
            "alpha",
            "--path",
            "projects/alpha",
        ])
        .assert()
        .success();

    fs::create_dir_all(root.join("projects/alpha")).unwrap();
    fs::write(root.join("projects/alpha/README"), "not a git repo").unwrap();

    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "worktree",
            "add",
            "alpha",
            "slot1",
            "--branch",
            "slot1",
        ])
        .assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains("not a git repo"));
}

#[test]
fn worktree_prune_empty_orphan_and_force_and_registered_safe() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    workspace_with_git_project(&root);

    // registered slot
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "worktree",
            "add",
            "alpha",
            "live",
            "--branch",
            "live",
        ])
        .assert()
        .success();
    let live = root.join("worktrees/alpha/live");
    assert!(live.is_dir());

    // empty orphan
    let empty = root.join("worktrees/alpha/empty-orphan");
    fs::create_dir_all(&empty).unwrap();

    // non-empty orphan
    let full = root.join("worktrees/alpha/full-orphan");
    fs::create_dir_all(&full).unwrap();
    fs::write(full.join("leftover.txt"), "x").unwrap();

    // default prune: empty gone, full remains, exit 3; JSON still printed
    let partial = odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "--json",
            "project",
            "worktree",
            "prune",
            "alpha",
        ])
        .assert()
        .failure()
        .code(3)
        .get_output()
        .stdout
        .clone();
    let partial: serde_json::Value = serde_json::from_slice(&partial).unwrap();
    assert_eq!(partial["project"], "alpha");
    let pruned = partial["pruned"].as_array().unwrap();
    assert_eq!(pruned.len(), 1);
    assert_eq!(pruned[0]["name"], "empty-orphan");
    assert_eq!(pruned[0]["path"], "worktrees/alpha/empty-orphan");
    assert!(pruned[0].get("dirty").is_none());
    let skipped = partial["skipped_nonempty"].as_array().unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["name"], "full-orphan");
    assert_eq!(skipped[0]["path"], "worktrees/alpha/full-orphan");
    assert!(skipped[0].get("dirty").is_none());
    assert!(!empty.exists());
    assert!(full.is_dir());
    assert!(live.is_dir());

    // force removes non-empty orphan; registered untouched
    let forced = odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "--json",
            "project",
            "worktree",
            "prune",
            "alpha",
            "--force",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let forced: serde_json::Value = serde_json::from_slice(&forced).unwrap();
    assert_eq!(forced["pruned"][0]["name"], "full-orphan");
    assert!(!full.exists());
    assert!(live.is_dir());
    assert!(Command::new("git")
        .args([
            "-C",
            live.to_str().unwrap(),
            "rev-parse",
            "--is-inside-work-tree",
        ])
        .status()
        .unwrap()
        .success());

    // no orphans left
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "worktree",
            "prune",
            "alpha",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("pruned 0 orphan worktree dirs"));
}

/// init + two git projects (alpha, beta).
fn workspace_with_two_git_projects(root: &Path) {
    odm()
        .args(["init", root.to_str().unwrap()])
        .assert()
        .success();

    for name in ["alpha", "beta"] {
        let bare = bare_with_main(root, name);
        odm()
            .args([
                "--root",
                root.to_str().unwrap(),
                "project",
                "add",
                name,
                "--path",
                &format!("projects/{name}"),
                "--url",
                bare.to_str().unwrap(),
                "--branch",
                "main",
            ])
            .assert()
            .success();
    }
}

#[test]
fn worktree_prune_all_two_empty_orphans() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    workspace_with_two_git_projects(&root);

    let a = root.join("worktrees/alpha/stale");
    let b = root.join("worktrees/beta/stale");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();

    let out = odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "--json",
            "project",
            "worktree",
            "prune",
            "--all",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["all"], true);
    let pruned = v["pruned"].as_array().unwrap();
    assert_eq!(pruned.len(), 2);
    assert_eq!(pruned[0]["project"], "alpha");
    assert_eq!(pruned[0]["name"], "stale");
    assert_eq!(pruned[0]["path"], "worktrees/alpha/stale");
    assert_eq!(pruned[1]["project"], "beta");
    assert_eq!(pruned[1]["path"], "worktrees/beta/stale");
    assert_eq!(v["skipped_nonempty"].as_array().unwrap().len(), 0);
    assert!(!a.exists());
    assert!(!b.exists());
}

#[test]
fn worktree_prune_all_partial_nonempty_exit_3() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    workspace_with_two_git_projects(&root);

    let empty = root.join("worktrees/alpha/empty");
    let full = root.join("worktrees/beta/full");
    fs::create_dir_all(&empty).unwrap();
    fs::create_dir_all(&full).unwrap();
    fs::write(full.join("leftover.txt"), "x").unwrap();

    let out = odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "--json",
            "project",
            "worktree",
            "prune",
            "--all",
        ])
        .assert()
        .failure()
        .code(3)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["all"], true);
    assert_eq!(v["pruned"].as_array().unwrap().len(), 1);
    assert_eq!(v["pruned"][0]["project"], "alpha");
    assert_eq!(v["skipped_nonempty"].as_array().unwrap().len(), 1);
    assert_eq!(v["skipped_nonempty"][0]["project"], "beta");
    assert_eq!(v["skipped_nonempty"][0]["name"], "full");
    assert!(!empty.exists());
    assert!(full.is_dir());

    // force clears remaining
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "worktree",
            "prune",
            "--all",
            "--force",
        ])
        .assert()
        .success();
    assert!(!full.exists());
}

#[test]
fn worktree_prune_all_mutual_exclusion_and_bare() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    workspace_with_git_project(&root);

    // clap usage → exit 1 (same as library OdmError::Usage)
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "worktree",
            "prune",
        ])
        .assert()
        .failure()
        .code(1);

    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "worktree",
            "prune",
            "--all",
            "alpha",
        ])
        .assert()
        .failure()
        .code(1);
}

#[test]
fn worktree_prune_all_skips_non_git_in_mix() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    workspace_with_git_project(&root);

    // non-git project in config
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "add",
            "docs",
            "--path",
            "projects/docs",
        ])
        .assert()
        .success();
    fs::create_dir_all(root.join("projects/docs")).unwrap();
    fs::write(root.join("projects/docs/README"), "not git").unwrap();

    let orphan = root.join("worktrees/alpha/stale");
    fs::create_dir_all(&orphan).unwrap();

    let out = odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "--json",
            "project",
            "worktree",
            "prune",
            "--all",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["pruned"].as_array().unwrap().len(), 1);
    assert_eq!(v["pruned"][0]["project"], "alpha");
    assert!(!orphan.exists());
}

#[test]
fn worktree_prune_unknown_project_usage() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    workspace_with_git_project(&root);

    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "worktree",
            "prune",
            "missing",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown project"));
}

#[test]
fn worktree_prune_non_git_project_fails() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");

    odm()
        .args(["init", root.to_str().unwrap(), "--no-git"])
        .assert()
        .success();

    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "add",
            "alpha",
            "--path",
            "projects/alpha",
        ])
        .assert()
        .success();

    fs::create_dir_all(root.join("projects/alpha")).unwrap();
    fs::write(root.join("projects/alpha/README"), "not a git repo").unwrap();

    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "worktree",
            "prune",
            "alpha",
        ])
        .assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains("not a git repo"));
}
