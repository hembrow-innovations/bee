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

#[test]
fn help_works() {
    odm()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Orchestrated Development Management"));
}

#[test]
fn init_and_project_list() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");

    odm()
        .args(["init", root.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized Workspace"));

    assert!(root.join(".odm/odm.config.yaml").is_file());

    fs::write(
        root.join(".odm/odm.config.yaml"),
        "projects:\n  alpha:\n    path: projects/alpha\n    url: ./fixtures/alpha.git\n",
    )
    .unwrap();

    odm()
        .args(["--root", root.to_str().unwrap(), "project", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha"));

    odm()
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
        .stdout(predicate::str::contains("\"name\": \"alpha\""));
}

#[test]
fn init_json_and_refuse_second() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws2");

    odm()
        .args(["--json", "init", root.to_str().unwrap(), "--no-git"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"git\": false"));

    odm()
        .args(["init", root.to_str().unwrap(), "--no-git"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("already a Workspace"));
}

#[test]
fn status_and_doctor_smoke() {
    let dir = tempdir().unwrap();
    odm()
        .current_dir(dir.path())
        .args(["init", ".", "--no-git"])
        .assert()
        .success();

    odm()
        .current_dir(dir.path())
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Workspace:"));

    odm()
        .current_dir(dir.path())
        .args(["--json", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"projects\""))
        .stdout(predicate::str::contains("\"progens\""));

    odm()
        .current_dir(dir.path())
        .args(["doctor", "--fix"])
        .assert()
        .success()
        .stdout(predicate::str::contains("doctor: ok"));

    assert!(dir.path().join(".odm/cache").is_dir());
}

#[test]
fn discover_walk_up() {
    let dir = tempdir().unwrap();
    odm()
        .args(["init", dir.path().to_str().unwrap(), "--no-git"])
        .assert()
        .success();
    let nested = dir.path().join("a/b");
    fs::create_dir_all(&nested).unwrap();
    odm()
        .current_dir(&nested)
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(no projects)"));
}

#[test]
fn unknown_project_usage() {
    let dir = tempdir().unwrap();
    odm()
        .args(["init", dir.path().to_str().unwrap(), "--no-git"])
        .assert()
        .success();
    odm()
        .args([
            "--root",
            dir.path().to_str().unwrap(),
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
fn project_add_sync_pin_flow() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    odm()
        .args(["init", root.to_str().unwrap()])
        .assert()
        .success();

    let bare = bare_with_main(&root, "alpha");

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
        .success()
        .stdout(predicate::str::contains("cloned"));

    assert!(root.join("projects/alpha/README").is_file());
    assert!(root.join(".odm/odm.lock.yaml").is_file());

    odm()
        .args(["--root", root.to_str().unwrap(), "sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha"));

    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "--json",
            "pin",
            "status",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("in_sync"));

    odm()
        .args(["--root", root.to_str().unwrap(), "pin", "apply"])
        .assert()
        .success()
        .stdout(predicate::str::contains("applied"));

    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "project",
            "git",
            "alpha",
            "--",
            "rev-parse",
            "HEAD",
        ])
        .assert()
        .success();
}

#[test]
fn clap_unknown_command_exit_1() {
    odm()
        .arg("notacommand")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unrecognized subcommand").or(
            predicate::str::contains("notacommand"),
        ));
}

#[test]
fn clap_parse_error_json_envelope() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    odm()
        .args(["init", root.to_str().unwrap(), "--no-git"])
        .assert()
        .success();

    let stdout = String::from_utf8(
        odm()
            .args([
                "--json",
                "--root",
                root.to_str().unwrap(),
                "project",
                "worktree",
                "prune",
            ])
            .assert()
            .failure()
            .code(1)
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["code"], "usage");
    assert!(!v["error"]["message"].as_str().unwrap().is_empty());
}


