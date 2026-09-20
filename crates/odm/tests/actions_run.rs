//! Integration harness: `run` against a temp workbench.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin;
use predicates::prelude::*;
use tempfile::tempdir;

fn odm() -> assert_cmd::Command {
    assert_cmd::Command::new(cargo_bin("odm"))
}

fn init_ws(root: &Path) {
    fs::create_dir_all(root).unwrap();
    odm()
        .current_dir(root)
        .arg("init")
        .assert()
        .success();
}

fn write_catalog(root: &Path, yaml: &str) {
    fs::create_dir_all(root.join(".hivemind")).unwrap();
    fs::write(root.join(".hivemind/workbench.yaml"), yaml).unwrap();
}

const DESK_ACTIONS: &str = "\
hello:
  tasks:
    - run: echo hello-desk
fail:
  tasks:
    - run: exit 7
chain:
  tasks:
    - run: echo step1
    - run: echo step2
";

fn setup_temp_desk() -> (tempfile::TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    init_ws(&root);
    fs::create_dir_all(root.join("actions")).unwrap();
    fs::write(root.join("actions/core.yaml"), DESK_ACTIONS).unwrap();
    write_catalog(
        &root,
        "name: desk\nactions:\n  core: actions/core.yaml\n",
    );
    (dir, root)
}

#[test]
fn run_lists_hello() {
    let (_dir, root) = setup_temp_desk();
    odm()
        .current_dir(&root)
        .arg("run")
        .assert()
        .success()
        .stdout(predicate::str::contains("hello"))
        .stdout(predicate::str::contains("fail"))
        .stdout(predicate::str::contains("chain"));
}

#[test]
fn run_hello_success() {
    let (_dir, root) = setup_temp_desk();
    odm()
        .current_dir(&root)
        .args(["run", "hello"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hello-desk"));
}

#[test]
fn run_fail_exit_7() {
    let (_dir, root) = setup_temp_desk();
    odm()
        .current_dir(&root)
        .args(["run", "fail"])
        .assert()
        .failure()
        .code(7);
}

#[test]
fn run_unknown_exit_1() {
    let (_dir, root) = setup_temp_desk();
    odm()
        .current_dir(&root)
        .args(["run", "nope"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown action"));
}

#[test]
fn run_json_hello() {
    let (_dir, root) = setup_temp_desk();
    odm()
        .current_dir(&root)
        .args(["run", "hello"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hello-desk"));
}

#[test]
fn run_json_fail() {
    let (_dir, root) = setup_temp_desk();
    odm()
        .current_dir(&root)
        .args(["run", "fail"])
        .assert()
        .failure()
        .code(7);
}

#[test]
fn run_json_chain_concatenates_stdout() {
    let (_dir, root) = setup_temp_desk();
    odm()
        .current_dir(&root)
        .args(["run", "chain"])
        .assert()
        .success()
        .stdout(predicate::str::contains("step1"))
        .stdout(predicate::str::contains("step2"));
}

#[test]
fn run_json_list() {
    let (_dir, root) = setup_temp_desk();
    odm()
        .current_dir(&root)
        .arg("run")
        .assert()
        .success()
        .stdout(predicate::str::contains("hello"));
}

#[test]
fn run_chain_success() {
    let (_dir, root) = setup_temp_desk();
    odm()
        .current_dir(&root)
        .args(["run", "chain"])
        .assert()
        .success()
        .stdout(predicate::str::contains("step1"))
        .stdout(predicate::str::contains("step2"));
}

#[test]
fn run_no_actions_message() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("empty-ws");
    init_ws(&root);
    odm().current_dir(&root).arg("run").assert().success();
}

/// Minimal workspace with project path + optional worktree slot + action bundle.
fn setup_cwd_workspace() -> (tempfile::TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    init_ws(&root);
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
    write_catalog(
        &root,
        "\
name: cwd-ws
projects:
  alpha:
    path: projects/alpha
actions:
  core: actions/core.yaml
",
    );
    (dir, root)
}

#[test]
fn run_missing_bundle_exit_2() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    init_ws(&root);
    write_catalog(
        &root,
        "name: t\nactions:\n  core: actions/missing.yaml\n",
    );
    odm()
        .current_dir(&root)
        .arg("run")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("path does not exist"));
}

#[test]
fn run_project_cwd() {
    let (_dir, root) = setup_cwd_workspace();
    odm()
        .current_dir(&root)
        .args(["run", "pwdhere", "--project", "alpha"])
        .assert()
        .success()
        .stdout(predicate::str::contains("from-project"));
}

#[test]
fn run_global_project_cwd() {
    let (_dir, root) = setup_cwd_workspace();
    odm()
        .current_dir(&root)
        .args(["--project", "alpha", "run", "pwdhere"])
        .assert()
        .success()
        .stdout(predicate::str::contains("from-project"));
}

#[test]
fn run_wt_cwd() {
    let (_dir, root) = setup_cwd_workspace();
    odm()
        .current_dir(&root)
        .args(["run", "pwdhere", "--project", "alpha", "--wt", "slot1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("from-wt"));
}

#[test]
fn run_wt_requires_project_exit_1() {
    let (_dir, root) = setup_cwd_workspace();
    odm()
        .current_dir(&root)
        .args(["run", "pwdhere", "--wt", "slot1"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("--wt requires --project"));
}

#[test]
fn run_missing_wt_slot_exit_4() {
    let (_dir, root) = setup_cwd_workspace();
    odm()
        .current_dir(&root)
        .args(["run", "pwdhere", "--project", "alpha", "--wt", "missing"])
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
        .current_dir(&root)
        .args(["run", "pwdhere", "--project", "alpha"])
        .assert()
        .failure()
        .code(4)
        .stderr(predicate::str::contains("project path missing"));
}

#[test]
fn run_unknown_project_exit_1() {
    let (_dir, root) = setup_cwd_workspace();
    odm()
        .current_dir(&root)
        .args(["run", "pwdhere", "--project", "nope"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown project"));
}

#[test]
fn run_extra_args_via_cli() {
    let (_dir, root) = setup_cwd_workspace();
    odm()
        .current_dir(&root)
        .args(["run", "echoargs", "--", "one", "two"])
        .assert()
        .success()
        .stdout(predicate::str::contains("one\ntwo\n"));
}
