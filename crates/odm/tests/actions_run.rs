//! Integration harness: `odm run` against a temp copy of examples/core-desk.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

fn odm() -> assert_cmd::Command {
    assert_cmd::Command::new(cargo_bin("odm"))
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
    (dir, root)
}

fn json_stdout(cmd: &mut assert_cmd::Command) -> Value {
    let out = cmd.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&out).expect("stdout JSON")
}

#[test]
fn run_lists_hello() {
    let (_dir, root) = setup_temp_core_desk();
    odm()
        .args(["--root", root.to_str().unwrap(), "run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hello"))
        .stdout(predicate::str::contains("fail"))
        .stdout(predicate::str::contains("chain"));
}

#[test]
fn run_hello_success() {
    let (_dir, root) = setup_temp_core_desk();
    odm()
        .args(["--root", root.to_str().unwrap(), "run", "hello"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hello-desk"));
}

#[test]
fn run_fail_exit_7() {
    let (_dir, root) = setup_temp_core_desk();
    odm()
        .args(["--root", root.to_str().unwrap(), "run", "fail"])
        .assert()
        .failure()
        .code(7);
}

#[test]
fn run_unknown_exit_1() {
    let (_dir, root) = setup_temp_core_desk();
    odm()
        .args(["--root", root.to_str().unwrap(), "run", "nope"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown action"));
}

#[test]
fn run_json_hello() {
    let (_dir, root) = setup_temp_core_desk();
    let out = odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "--json",
            "run",
            "hello",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&out).expect("stdout is sole JSON object");
    assert_eq!(v["action"].as_str(), Some("hello"));
    assert_eq!(v["exitCode"].as_i64(), Some(0));
    let stdout = v["stdout"].as_str().expect("stdout field");
    assert!(stdout.contains("hello-desk"), "got: {stdout}");
    assert_eq!(v["stderr"].as_str(), Some(""));
}

#[test]
fn run_json_fail() {
    let (_dir, root) = setup_temp_core_desk();
    let out = odm()
        .args(["--root", root.to_str().unwrap(), "--json", "run", "fail"])
        .assert()
        .failure()
        .code(7)
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&out).expect("stdout is sole JSON object");
    assert_eq!(v["action"].as_str(), Some("fail"));
    assert_eq!(v["exitCode"].as_i64(), Some(7));
    assert!(v["stdout"].as_str().is_some());
    assert!(v["stderr"].as_str().is_some());
}

#[test]
fn run_json_chain_concatenates_stdout() {
    let (_dir, root) = setup_temp_core_desk();
    let v = json_stdout(odm().args([
        "--root",
        root.to_str().unwrap(),
        "--json",
        "run",
        "chain",
    ]));
    assert_eq!(v["exitCode"].as_i64(), Some(0));
    let stdout = v["stdout"].as_str().expect("stdout");
    assert!(stdout.contains("step1"), "got: {stdout}");
    assert!(stdout.contains("step2"), "got: {stdout}");
    let i1 = stdout.find("step1").unwrap();
    let i2 = stdout.find("step2").unwrap();
    assert!(i1 < i2, "tasks concatenated in order");
}

#[test]
fn run_json_list() {
    let (_dir, root) = setup_temp_core_desk();
    let v = json_stdout(odm().args(["--root", root.to_str().unwrap(), "--json", "run"]));
    let actions = v["actions"].as_array().expect("actions");
    assert!(actions.len() >= 3);
    let names: Vec<_> = actions
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"hello"));
    let hello = actions.iter().find(|a| a["name"] == "hello").unwrap();
    let tasks = hello["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0]["run"].as_str().unwrap().contains("hello-desk"));
}

#[test]
fn run_chain_success() {
    let (_dir, root) = setup_temp_core_desk();
    odm()
        .args(["--root", root.to_str().unwrap(), "run", "chain"])
        .assert()
        .success()
        .stdout(predicate::str::contains("step1"))
        .stdout(predicate::str::contains("step2"));
}

#[test]
fn run_no_actions_message() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("empty-ws");
    assert!(Command::new(cargo_bin("odm"))
        .args(["init", root.to_str().unwrap(), "--no-git"])
        .status()
        .unwrap()
        .success());
    odm()
        .args(["--root", root.to_str().unwrap(), "run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(no actions)"));
}

/// Minimal workspace with project path + optional worktree slot + action bundle.
fn setup_cwd_workspace() -> (tempfile::TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    assert!(Command::new(cargo_bin("odm"))
        .args(["init", root.to_str().unwrap(), "--no-git"])
        .status()
        .unwrap()
        .success());
    fs::create_dir_all(root.join("projects/alpha")).unwrap();
    fs::write(root.join("projects/alpha/marker"), "from-project\n").unwrap();
    fs::create_dir_all(root.join("worktrees/alpha/slot1")).unwrap();
    fs::write(root.join("worktrees/alpha/slot1/marker"), "from-wt\n").unwrap();
    fs::create_dir_all(root.join("actions")).unwrap();
    fs::write(
        root.join("actions/core.yaml"),
        "\
pwdhere:
  tasks:
    - run: cat marker
echoargs:
  tasks:
    - run: printf '%s\\n'
",
    )
    .unwrap();
    fs::write(
        root.join(".odm/odm.config.yaml"),
        "\
name: cwd-ws
projects:
  alpha:
    path: projects/alpha
actions:
  core: actions/core.yaml
",
    )
    .unwrap();
    (dir, root)
}

#[test]
fn run_missing_bundle_exit_2() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    assert!(Command::new(cargo_bin("odm"))
        .args(["init", root.to_str().unwrap(), "--no-git"])
        .status()
        .unwrap()
        .success());
    fs::write(
        root.join(".odm/odm.config.yaml"),
        "name: t\nactions:\n  core: actions/missing.yaml\n",
    )
    .unwrap();
    odm()
        .args(["--root", root.to_str().unwrap(), "run"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("path does not exist"));
}

#[test]
fn run_project_cwd() {
    let (_dir, root) = setup_cwd_workspace();
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "run",
            "pwdhere",
            "--project",
            "alpha",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("from-project"));
}

#[test]
fn run_global_project_cwd() {
    let (_dir, root) = setup_cwd_workspace();
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "--project",
            "alpha",
            "run",
            "pwdhere",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("from-project"));
}

#[test]
fn run_wt_cwd() {
    let (_dir, root) = setup_cwd_workspace();
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "run",
            "pwdhere",
            "--project",
            "alpha",
            "--wt",
            "slot1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("from-wt"));
}

#[test]
fn run_wt_requires_project_exit_1() {
    let (_dir, root) = setup_cwd_workspace();
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "run",
            "pwdhere",
            "--wt",
            "slot1",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("--wt requires --project"));
}

#[test]
fn run_missing_wt_slot_exit_4() {
    let (_dir, root) = setup_cwd_workspace();
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "run",
            "pwdhere",
            "--project",
            "alpha",
            "--wt",
            "missing",
        ])
        .assert()
        .failure()
        .code(4)
        .stderr(predicate::str::contains("worktree slot not found"));
}

#[test]
fn run_missing_project_path_exit_4() {
    let (_dir, root) = setup_cwd_workspace();
    fs::remove_dir_all(root.join("projects/alpha")).unwrap();
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "run",
            "pwdhere",
            "--project",
            "alpha",
        ])
        .assert()
        .failure()
        .code(4)
        .stderr(predicate::str::contains("project path missing"));
}

#[test]
fn run_unknown_project_exit_1() {
    let (_dir, root) = setup_cwd_workspace();
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "run",
            "pwdhere",
            "--project",
            "nope",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown project"));
}

#[test]
fn run_extra_args_via_cli() {
    let (_dir, root) = setup_cwd_workspace();
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "run",
            "echoargs",
            "--",
            "one",
            "two",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("one\ntwo\n"));
}
