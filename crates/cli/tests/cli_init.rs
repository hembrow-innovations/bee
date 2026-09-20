use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use predicates::prelude::*;
use tempfile::tempdir;

fn bee() -> assert_cmd::Command {
    let mut p = std::env::current_exe().unwrap();
    p.pop();
    p.pop();
    p.push("bee");
    assert_cmd::Command::new(p)
}

fn init_ws(root: &Path) {
    fs::create_dir_all(root).unwrap();
    bee()
        .current_dir(root)
        .arg("init")
        .assert()
        .success();
}

fn catalog(root: &Path) -> PathBuf {
    root.join(".hivemind/workbench.yaml")
}

fn pin_file(root: &Path) -> PathBuf {
    root.join(".hivemind/workbench.lock.yaml")
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

#[test]
fn help_works() {
    bee().arg("--help").assert().success();
}

#[test]
fn init_writes_hivemind_catalog() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    fs::create_dir_all(&root).unwrap();

    bee()
        .current_dir(&root)
        .arg("init")
        .assert()
        .success();

    assert!(catalog(&root).is_file());
}

#[test]
fn init_refuses_second() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws2");
    fs::create_dir_all(&root).unwrap();

    bee()
        .current_dir(&root)
        .arg("init")
        .assert()
        .success();
    assert!(catalog(&root).is_file());

    bee()
        .current_dir(&root)
        .arg("init")
        .assert()
        .failure();
}

#[test]
fn status_and_doctor_smoke() {
    let dir = tempdir().unwrap();
    bee()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .success();

    bee()
        .current_dir(dir.path())
        .arg("status")
        .assert()
        .success();

    bee()
        .current_dir(dir.path())
        .arg("doctor")
        .assert()
        .success();
}

#[test]
fn project_add_sync_pin_flow() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    init_ws(&root);

    let bare = bare_with_main(&root, "alpha");
    let url = bare.to_str().unwrap().to_string();

    bee()
        .current_dir(&root)
        .args([
            "project",
            "add",
            "alpha",
            "--path",
            "projects/alpha",
            "--url",
            &url,
            "--branch",
            "main",
        ])
        .assert()
        .success();

    assert!(root.join("projects/alpha/README").is_file());
    assert!(pin_file(&root).is_file());

    bee()
        .current_dir(&root)
        .arg("sync")
        .assert()
        .success();

    bee()
        .current_dir(&root)
        .args(["pin", "apply"])
        .assert()
        .success();
}

#[test]
fn clap_unknown_command_fails() {
    bee()
        .arg("notacommand")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("unrecognized subcommand")
                .or(predicate::str::contains("notacommand")),
        );
}
