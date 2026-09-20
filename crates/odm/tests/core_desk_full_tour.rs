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
    let mut cmd = assert_cmd::Command::new(cargo_bin("odm"));
    cmd.env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "protocol.file.allow")
        .env("GIT_CONFIG_VALUE_0", "always");
    cmd
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

fn setup_temp_core_desk() -> (tempfile::TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    fs::create_dir_all(&root).unwrap();
    odm()
        .current_dir(&root)
        .arg("init")
        .assert()
        .success();
    git_user(&root);
    let alpha = bare_with_main(dir.path(), "alpha");
    fs::create_dir_all(root.join("progens/notes/.obsidian")).unwrap();
    fs::write(root.join("progens/notes/.obsidian/app.json"), "{}\n").unwrap();
    fs::write(
        root.join("progens/notes/Welcome.md"),
        "---\nid: welcome\n---\nDeskUniqueToken\n",
    )
    .unwrap();
    fs::write(
        root.join("progens/notes/README.md"),
        "---\nid: readme\n---\nSee [[Welcome]].\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("progens/ops")).unwrap();
    fs::write(
        root.join("progens/ops/ops-note.md"),
        "---\nid: ops-note\n---\nOpsUniqueToken\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("actions")).unwrap();
    fs::write(
        root.join("actions/core.yaml"),
        "in-alpha:\n  tasks:\n    - run: true\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("templates/hello")).unwrap();
    fs::write(
        root.join("templates/hello/hello.txt"),
        "hello from core-desk generator\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("generators")).unwrap();
    fs::write(
        root.join("generators/core.yaml"),
        "hello:\n  template: templates/hello\n",
    )
    .unwrap();
    fs::write(
        root.join(".hivemind/workbench.yaml"),
        format!(
            "\
name: desk
projects:
  alpha:
    path: projects/alpha
    url: \"{}\"
    branch: main
progens:
  notes:
    path: progens/notes
  ops:
    path: progens/ops
progen_groups:
  default:
    - notes
  all-docs:
    - notes
    - ops
actions:
  core: actions/core.yaml
generators:
  core: generators/core.yaml
",
            alpha.display()
        ),
    )
    .unwrap();
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
