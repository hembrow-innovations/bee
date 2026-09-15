//! Composition gate: one tour of core-desk surfaces that focused gates miss.
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
fn core_desk_full_tour() {
    if skip_without_git() {
        return;
    }
    let (_dir, root) = setup_temp_core_desk();
    let root_s = root.to_str().unwrap();

    // 1. sync + reindex
    odm()
        .args(["--root", root_s, "sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha"));
    assert!(root.join("projects/alpha").is_dir());

    odm()
        .args(["--root", root_s, "progen", "reindex"])
        .assert()
        .success();

    // 2. find token in notes; find --progen-group narrows
    let found = json_stdout(odm().args([
        "--root",
        root_s,
        "find",
        "DeskUniqueToken",
        "--json",
    ]));
    let hits = found["hits"].as_array().expect("hits");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["id"], "welcome");
    assert_eq!(hits[0]["progen"], "notes");

    let scoped = json_stdout(odm().args([
        "--root",
        root_s,
        "find",
        "OpsUniqueToken",
        "--progen-group",
        "default",
        "--json",
    ]));
    assert_eq!(
        scoped["hits"].as_array().expect("scoped hits").len(),
        0,
        "default group is notes-only; OpsUniqueToken lives in ops"
    );

    let ops = json_stdout(odm().args([
        "--root",
        root_s,
        "find",
        "OpsUniqueToken",
        "--progen-group",
        "all-docs",
        "--json",
    ]));
    let ops_hits = ops["hits"].as_array().expect("ops hits");
    assert_eq!(ops_hits.len(), 1);
    assert_eq!(ops_hits[0]["id"], "ops-note");
    assert_eq!(ops_hits[0]["progen"], "ops");

    // 3. context welcome JSON anchor id
    let ctx = json_stdout(odm().args([
        "--root",
        root_s,
        "--json",
        "context",
        "welcome",
        "--progen",
        "notes",
    ]));
    assert_eq!(ctx["anchor"]["id"], "welcome");
    assert!(ctx.get("outgoing").and_then(|v| v.as_array()).is_some());
    assert!(ctx.get("incoming").and_then(|v| v.as_array()).is_some());

    // 4. progen get / body / tree / ls / backlinks on seeded ids
    let get = json_stdout(odm().args([
        "--root",
        root_s,
        "--json",
        "progen",
        "get",
        "welcome",
        "--progen",
        "notes",
    ]));
    assert_eq!(get["id"], "welcome");
    assert!(get["body"]
        .as_str()
        .unwrap()
        .contains("DeskUniqueToken"));

    odm()
        .args([
            "--root",
            root_s,
            "progen",
            "body",
            "welcome",
            "--progen",
            "notes",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("DeskUniqueToken"));

    let tree = json_stdout(odm().args([
        "--root",
        root_s,
        "--json",
        "progen",
        "tree",
        "--progen",
        "notes",
    ]));
    let paths = tree["paths"].as_array().expect("paths");
    assert!(
        paths.iter().any(|p| p.as_str() == Some("Welcome.md")),
        "expected Welcome.md in tree: {paths:?}"
    );

    let ls = json_stdout(odm().args([
        "--root",
        root_s,
        "--json",
        "progen",
        "ls",
        "--progen",
        "notes",
    ]));
    let notes = ls["notes"].as_array().expect("notes");
    assert!(
        notes.iter().any(|n| n["id"] == "welcome"),
        "expected welcome in ls: {notes:?}"
    );

    let bl = json_stdout(odm().args([
        "--root",
        root_s,
        "--json",
        "progen",
        "backlinks",
        "welcome",
        "--progen",
        "notes",
    ]));
    let links = bl["backlinks"].as_array().expect("backlinks");
    assert!(
        links.iter().any(|h| h["id"] == "readme"),
        "README wikilinks Welcome: {links:?}"
    );

    // 5. project git alpha -- rev-parse HEAD
    odm()
        .args([
            "--root",
            root_s,
            "project",
            "git",
            "alpha",
            "--",
            "rev-parse",
            "HEAD",
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^[0-9a-f]{40}\n?$").unwrap());

    // 6. worktree add + run in-alpha --project alpha (and --wt)
    odm()
        .args([
            "--root",
            root_s,
            "project",
            "worktree",
            "add",
            "alpha",
            "tour",
            "--branch",
            "odm-tour",
        ])
        .assert()
        .success();
    assert!(root.join("worktrees/alpha/tour").is_dir());

    odm()
        .args([
            "--root",
            root_s,
            "run",
            "in-alpha",
            "--project",
            "alpha",
        ])
        .assert()
        .success();

    odm()
        .args([
            "--root",
            root_s,
            "run",
            "in-alpha",
            "--project",
            "alpha",
            "--wt",
            "tour",
        ])
        .assert()
        .success();

    // 7. generate --force after first materialize
    let dest = root.join("out/hello");
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

    fs::write(dest.join("hello.txt"), "stale\n").unwrap();
    odm()
        .args([
            "--root",
            root_s,
            "generate",
            "hello",
            "--dest",
            "out/hello",
            "--force",
        ])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(dest.join("hello.txt")).unwrap(),
        "hello from core-desk generator\n"
    );
}
