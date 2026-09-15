//! Integration harness: drive `odm` against a temp copy of examples/core-desk.
//! Offline only; requires `git` on PATH (skip otherwise).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn skip_without_git() -> bool {
    if git_available() {
        return false;
    }
    eprintln!("skipping: git not found on PATH");
    true
}

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

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let ty = entry.file_type().unwrap();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), to).unwrap();
        }
    }
}

fn core_desk_example() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/core-desk")
        .canonicalize()
        .expect("examples/core-desk")
}

fn setup_temp_core_desk() -> (tempfile::TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("core-desk");
    copy_dir(&core_desk_example(), &root);
    assert!(Command::new("git")
        .args(["init", root.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    git_user(&root);
    (dir, root)
}

fn json_stdout(cmd: &mut assert_cmd::Command) -> Value {
    let out = cmd.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&out).expect("stdout JSON")
}

#[test]
fn init_empty_dir_json() {
    if skip_without_git() {
        return;
    }
    let dir = tempdir().unwrap();
    let root = dir.path().join("fresh");

    let v = json_stdout(odm().args(["--json", "init", root.to_str().unwrap()]));
    assert_eq!(v["root"].as_str().unwrap(), root.to_str().unwrap());
    assert_eq!(v["git"].as_bool(), Some(true));
    assert!(root.join(".odm/odm.config.yaml").is_file());
}

#[test]
fn init_empty_dir_no_git() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("fresh-nogit");

    let v = json_stdout(odm().args([
        "--json",
        "init",
        root.to_str().unwrap(),
        "--no-git",
    ]));
    assert!(v.get("root").and_then(|r| r.as_str()).is_some());
    assert_eq!(v["git"].as_bool(), Some(false));
    assert!(root.join(".odm/odm.config.yaml").is_file());
}

#[test]
fn core_desk_full_gate() {
    if skip_without_git() {
        return;
    }
    let (_dir, root) = setup_temp_core_desk();
    let root_s = root.to_str().unwrap();

    // sync → clones alpha/beta from bare fixtures
    odm()
        .args(["--root", root_s, "sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha"))
        .stdout(predicate::str::contains("beta"));
    assert!(root.join("projects/alpha").is_dir());
    assert!(root.join("projects/beta").is_dir());
    assert!(root.join(".odm/odm.lock.yaml").is_file());

    // pin status
    let pin = json_stdout(odm().args(["--root", root_s, "--json", "pin", "status"]));
    assert_eq!(pin["present"].as_bool(), Some(true));
    let entries = pin["entries"].as_array().expect("entries");
    assert!(entries.len() >= 2);
    assert!(entries.iter().any(|e| e["state"] == "in_sync"));

    // pin apply
    odm()
        .args(["--root", root_s, "pin", "apply"])
        .assert()
        .success()
        .stdout(predicate::str::contains("applied"));

    // status --json → on_disk / pin_state populated
    let st = json_stdout(odm().args(["--root", root_s, "--json", "status"]));
    let projects = st["projects"].as_array().expect("projects");
    assert_eq!(projects.len(), 2);
    for p in projects {
        assert_eq!(p["on_disk"].as_bool(), Some(true));
        assert!(p.get("pin_state").is_some());
        assert!(p["is_git"].as_bool().unwrap_or(false));
    }

    // doctor → exit 0 (or only warns; ok flag true means no fails)
    let doc = json_stdout(odm().args(["--root", root_s, "--json", "doctor"]));
    assert_eq!(doc["ok"].as_bool(), Some(true));

    // doctor --fix repairs layout/gitignore if needed
    odm()
        .args(["--root", root_s, "doctor", "--fix"])
        .assert()
        .success()
        .stdout(predicate::str::contains("doctor: ok"));
    assert!(root.join(".odm/cache").is_dir());
}

#[test]
fn core_desk_worktree_status_find_gate() {
    if skip_without_git() {
        return;
    }
    let (_dir, root) = setup_temp_core_desk();
    let root_s = root.to_str().unwrap();

    odm()
        .args(["--root", root_s, "sync"])
        .assert()
        .success();
    assert!(root.join("projects/alpha").is_dir());

    // worktree add with --branch (primary already has default branch checked out)
    odm()
        .args([
            "--root",
            root_s,
            "project",
            "worktree",
            "add",
            "alpha",
            "dogfood",
            "--branch",
            "odm-dogfood",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("dogfood"));
    assert!(root.join("worktrees/alpha/dogfood").is_dir());

    // status --json → alpha has registered worktree_slots
    let st = json_stdout(odm().args(["--root", root_s, "--json", "status"]));
    let projects = st["projects"].as_array().expect("projects");
    let alpha = projects
        .iter()
        .find(|p| p["name"] == "alpha")
        .expect("alpha project");
    let slots = alpha["worktree_slots"].as_array().expect("worktree_slots");
    let dogfood = slots
        .iter()
        .find(|s| s["name"] == "dogfood")
        .expect("dogfood slot");
    assert_eq!(dogfood["path"], "worktrees/alpha/dogfood");

    // find --limit against core-desk progen token
    odm()
        .args(["--root", root_s, "progen", "reindex"])
        .assert()
        .success();
    let found = json_stdout(odm().args([
        "--root",
        root_s,
        "find",
        "DeskUniqueToken",
        "--limit",
        "1",
        "--json",
    ]));
    let hits = found["hits"].as_array().expect("hits");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["id"], "welcome");

    // tidy temp dir
    odm()
        .args([
            "--root",
            root_s,
            "project",
            "worktree",
            "rm",
            "alpha",
            "dogfood",
        ])
        .assert()
        .success();
}

#[test]
fn core_desk_prune_dirty_doctor_gate() {
    if skip_without_git() {
        return;
    }
    let (_dir, root) = setup_temp_core_desk();
    let root_s = root.to_str().unwrap();

    odm()
        .args(["--root", root_s, "sync"])
        .assert()
        .success();
    assert!(root.join("projects/alpha").is_dir());

    odm()
        .args([
            "--root",
            root_s,
            "project",
            "worktree",
            "add",
            "alpha",
            "dogfood",
            "--branch",
            "odm-dogfood",
        ])
        .assert()
        .success();
    assert!(root.join("worktrees/alpha/dogfood").is_dir());

    // empty orphan under worktrees/alpha/
    let orphan = root.join("worktrees/alpha/stale-orphan");
    fs::create_dir_all(&orphan).unwrap();

    let doc = json_stdout(odm().args(["--root", root_s, "--json", "doctor"]));
    assert_eq!(doc["ok"].as_bool(), Some(true));
    let checks = doc["checks"].as_array().expect("checks");
    let orphan_check = checks
        .iter()
        .find(|c| c["id"] == "worktree_orphan:alpha:stale-orphan")
        .expect("worktree_orphan:alpha:stale-orphan");
    assert_eq!(orphan_check["status"], "warn");
    assert_eq!(orphan_check["fixable"].as_bool(), Some(false));

    // dirty registered slot
    fs::write(root.join("worktrees/alpha/dogfood/dirty.txt"), "x").unwrap();

    let doc = json_stdout(odm().args(["--root", root_s, "--json", "doctor"]));
    assert_eq!(doc["ok"].as_bool(), Some(true));
    let checks = doc["checks"].as_array().expect("checks");
    assert!(
        checks
            .iter()
            .any(|c| c["id"] == "worktree_dirty:alpha:dogfood"),
        "expected worktree_dirty:alpha:dogfood in {:?}",
        checks
            .iter()
            .map(|c| c["id"].as_str())
            .collect::<Vec<_>>()
    );

    // prune removes empty orphan; registered dogfood remains
    let pruned = json_stdout(odm().args([
        "--root",
        root_s,
        "--json",
        "project",
        "worktree",
        "prune",
        "alpha",
    ]));
    assert_eq!(pruned["project"], "alpha");
    let entries = pruned["pruned"].as_array().expect("pruned");
    assert!(
        entries.iter().any(|e| {
            e["name"] == "stale-orphan" && e["path"] == "worktrees/alpha/stale-orphan"
        }),
        "expected stale-orphan in pruned: {entries:?}"
    );
    assert!(!orphan.exists());
    assert!(root.join("worktrees/alpha/dogfood").is_dir());

    // optional cleanup (--force: slot still dirty from dirty.txt)
    odm()
        .args([
            "--root",
            root_s,
            "project",
            "worktree",
            "rm",
            "alpha",
            "dogfood",
            "--force",
        ])
        .assert()
        .success();
}

#[test]
fn core_desk_prune_all_and_slot_dirty_gate() {
    if skip_without_git() {
        return;
    }
    let (_dir, root) = setup_temp_core_desk();
    let root_s = root.to_str().unwrap();

    odm()
        .args(["--root", root_s, "sync"])
        .assert()
        .success();
    assert!(root.join("projects/alpha").is_dir());

    // empty orphan under worktrees/alpha/
    let orphan = root.join("worktrees/alpha/stale-orphan");
    fs::create_dir_all(&orphan).unwrap();

    // prune --all removes empty orphan workspace-wide
    let pruned = json_stdout(odm().args([
        "--root",
        root_s,
        "--json",
        "project",
        "worktree",
        "prune",
        "--all",
    ]));
    assert_eq!(pruned["all"].as_bool(), Some(true));
    let entries = pruned["pruned"].as_array().expect("pruned");
    assert!(
        entries.iter().any(|e| {
            e["project"] == "alpha"
                && e["name"] == "stale-orphan"
                && e["path"] == "worktrees/alpha/stale-orphan"
        }),
        "expected alpha/stale-orphan in pruned: {entries:?}"
    );
    assert!(
        pruned["skipped_nonempty"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(false),
        "expected empty skipped_nonempty: {:?}",
        pruned["skipped_nonempty"]
    );
    assert!(!orphan.exists());

    // registered slot + dirty file → status/list dirty true
    odm()
        .args([
            "--root",
            root_s,
            "project",
            "worktree",
            "add",
            "alpha",
            "dogfood",
            "--branch",
            "odm-dogfood",
        ])
        .assert()
        .success();
    assert!(root.join("worktrees/alpha/dogfood").is_dir());
    fs::write(root.join("worktrees/alpha/dogfood/dirty.txt"), "x").unwrap();

    let st = json_stdout(odm().args(["--root", root_s, "--json", "status"]));
    let projects = st["projects"].as_array().expect("projects");
    let alpha = projects
        .iter()
        .find(|p| p["name"] == "alpha")
        .expect("alpha project");
    let slots = alpha["worktree_slots"].as_array().expect("worktree_slots");
    let dogfood = slots
        .iter()
        .find(|s| s["name"] == "dogfood")
        .expect("dogfood slot");
    assert_eq!(
        dogfood["dirty"].as_bool(),
        Some(true),
        "expected dirty true on status slot: {dogfood}"
    );

    let listed = json_stdout(odm().args([
        "--root",
        root_s,
        "--json",
        "project",
        "worktree",
        "list",
        "alpha",
    ]));
    let list_slots = listed["slots"].as_array().expect("slots");
    let list_dogfood = list_slots
        .iter()
        .find(|s| s["name"] == "dogfood")
        .expect("dogfood in list");
    assert_eq!(
        list_dogfood["dirty"].as_bool(),
        Some(true),
        "expected dirty true on list slot: {list_dogfood}"
    );

    // cleanup (--force: slot still dirty)
    odm()
        .args([
            "--root",
            root_s,
            "project",
            "worktree",
            "rm",
            "alpha",
            "dogfood",
            "--force",
        ])
        .assert()
        .success();
}

#[test]
fn core_desk_status_orphans_gate() {
    if skip_without_git() {
        return;
    }
    let (_dir, root) = setup_temp_core_desk();
    let root_s = root.to_str().unwrap();

    odm()
        .args(["--root", root_s, "sync"])
        .assert()
        .success();
    assert!(root.join("projects/alpha").is_dir());

    // empty orphan under worktrees/alpha/ (not a registered git worktree)
    let orphan = root.join("worktrees/alpha/stale-orphan");
    fs::create_dir_all(&orphan).unwrap();

    let st = json_stdout(odm().args(["--root", root_s, "--json", "status"]));
    let projects = st["projects"].as_array().expect("projects");
    let alpha = projects
        .iter()
        .find(|p| p["name"] == "alpha")
        .expect("alpha project");
    let orphans = alpha["worktree_orphans"]
        .as_array()
        .expect("worktree_orphans");
    let stale = orphans
        .iter()
        .find(|o| o["name"] == "stale-orphan")
        .expect("stale-orphan in status worktree_orphans");
    assert_eq!(stale["path"], "worktrees/alpha/stale-orphan");
    assert!(
        stale.get("dirty").is_none(),
        "orphans must not carry dirty: {stale}"
    );

    // optional: project info same shape
    let info = json_stdout(odm().args([
        "--root",
        root_s,
        "--json",
        "project",
        "info",
        "alpha",
    ]));
    let info_orphans = info["worktree_orphans"]
        .as_array()
        .expect("info worktree_orphans");
    assert!(
        info_orphans.iter().any(|o| {
            o["name"] == "stale-orphan" && o["path"] == "worktrees/alpha/stale-orphan"
        }),
        "expected stale-orphan on project info: {info_orphans:?}"
    );
    assert!(info_orphans[0].get("dirty").is_none());

    // per-project prune clears empty orphan (no doctor --fix)
    odm()
        .args([
            "--root",
            root_s,
            "project",
            "worktree",
            "prune",
            "alpha",
        ])
        .assert()
        .success();
    assert!(!orphan.exists(), "prune should remove empty orphan on disk");

    let st = json_stdout(odm().args(["--root", root_s, "--json", "status"]));
    let projects = st["projects"].as_array().expect("projects");
    let alpha = projects
        .iter()
        .find(|p| p["name"] == "alpha")
        .expect("alpha project");
    let orphans = alpha["worktree_orphans"]
        .as_array()
        .expect("worktree_orphans after prune");
    assert!(
        orphans.iter().all(|o| o["name"] != "stale-orphan"),
        "stale-orphan should be gone after prune: {orphans:?}"
    );
}

#[test]
fn core_desk_unknown_project_exit_1() {
    if skip_without_git() {
        return;
    }
    let (_dir, root) = setup_temp_core_desk();
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "info",
            "nope",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown project"));
}

#[test]
fn core_desk_generate_dry_run_gate() {
    if skip_without_git() {
        return;
    }
    let (_dir, root) = setup_temp_core_desk();
    let root_s = root.to_str().unwrap();
    let dest = root.join("out/hello");

    let v = json_stdout(odm().args([
        "--root",
        root_s,
        "--json",
        "generate",
        "hello",
        "--dest",
        "out/hello",
        "--dry-run",
    ]));
    assert_eq!(v["generator"], "hello");
    assert_eq!(v["dest"], "out/hello");
    assert_eq!(v["dry_run"], true);
    assert!(v["copied"].as_u64().unwrap() >= 1);
    assert!(!dest.exists());

    odm()
        .args([
            "--root",
            root_s,
            "generate",
            "hello",
            "--dest",
            "out/hello",
        ])
        .assert()
        .success();
    assert!(dest.join("hello.txt").is_file());
}
