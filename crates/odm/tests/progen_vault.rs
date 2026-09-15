//! Progen / Obsidian-vault integration tests.

use std::fs;
use std::path::PathBuf;

use assert_cmd::cargo::cargo_bin;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

fn odm() -> assert_cmd::Command {
    assert_cmd::Command::new(cargo_bin("odm"))
}

fn json_stdout(cmd: &mut assert_cmd::Command) -> Value {
    let out = cmd.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&out).expect("json stdout")
}

#[test]
fn progen_add_find_context_flow() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    fs::create_dir_all(&root).unwrap();

    odm()
        .current_dir(&root)
        .args(["init", "--no-git", "."])
        .assert()
        .success();

    odm()
        .current_dir(&root)
        .args(["progen", "add", "desk", "--path", "vaults/desk"])
        .assert()
        .success()
        .stdout(predicate::str::contains("vault ready"));

    let vault = root.join("vaults/desk");
    assert!(vault.join("README.md").is_file());
    assert!(vault.join(".obsidian/app.json").is_file());

    fs::write(
        vault.join("alpha.md"),
        "---\nid: a1\ntitle: Alpha\n---\n# Alpha\n\nUniqueZebra and [[Beta]].\n",
    )
    .unwrap();
    fs::write(
        vault.join("beta.md"),
        "---\nid: b1\ntitle: Beta\n---\n# Beta\n\nLinked from alpha.\n",
    )
    .unwrap();

    odm()
        .current_dir(&root)
        .args(["progen", "reindex"])
        .assert()
        .success()
        .stdout(predicate::str::contains("desk"));

    assert!(root.join(".odm/progen/desk/index.db").is_file());
    // Engine index must not live inside the vault
    assert!(!vault.join(".index").exists());

    let v = json_stdout(
        odm()
            .current_dir(&root)
            .args(["find", "UniqueZebra", "--json"]),
    );
    let hits = v["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["progen"], "desk");
    assert_eq!(hits[0]["id"], "a1");

    let ctx = json_stdout(
        odm()
            .current_dir(&root)
            .args(["context", "a1", "--json"]),
    );
    assert_eq!(ctx["anchor"]["id"], "a1");
    let out = ctx["outgoing"].as_array().unwrap();
    assert!(out.iter().any(|h| h["id"] == "b1" || h["title"] == "Beta"));

    let get = json_stdout(
        odm()
            .current_dir(&root)
            .args(["progen", "get", "a1", "--json"]),
    );
    assert_eq!(get["id"], "a1");
    assert!(get["body"].as_str().unwrap().contains("UniqueZebra"));

    odm()
        .current_dir(&root)
        .args(["progen", "body", "a1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("UniqueZebra"));

    let tree = json_stdout(
        odm()
            .current_dir(&root)
            .args(["progen", "tree", "--json"]),
    );
    let paths = tree["paths"].as_array().unwrap();
    assert!(paths.iter().any(|p| p.as_str() == Some("alpha.md")));

    let bl = json_stdout(
        odm()
            .current_dir(&root)
            .args(["progen", "backlinks", "b1", "--json"]),
    );
    let links = bl["backlinks"].as_array().unwrap();
    assert!(links.iter().any(|h| h["id"] == "a1"));

    odm()
        .current_dir(&root)
        .args(["progen", "get", "nope-missing"])
        .assert()
        .failure()
        .code(4);

    odm()
        .current_dir(&root)
        .args(["progen", "doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ok"));

    odm()
        .current_dir(&root)
        .args(["progen", "info", "desk", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("has_obsidian"));
}

#[test]
fn generate_lists_empty_and_requires_dest() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    fs::create_dir_all(&root).unwrap();
    odm()
        .current_dir(&root)
        .args(["init", "--no-git", "."])
        .assert()
        .success();

    odm()
        .current_dir(&root)
        .args(["generate"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(no generators)"));

    // name without --dest → usage (not not_implemented)
    odm()
        .current_dir(&root)
        .args(["generate", "foo"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("--dest"));
}

#[test]
fn list_includes_disk_summary() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    fs::create_dir_all(&root).unwrap();
    odm()
        .current_dir(&root)
        .args(["init", "--no-git", "."])
        .assert()
        .success();
    fs::create_dir_all(root.join("projects/alpha")).unwrap();
    fs::write(
        root.join(".odm/odm.config.yaml"),
        "\
name: t
projects:
  alpha:
    path: projects/alpha
progens:
  notes:
    path: progens/notes
",
    )
    .unwrap();
    fs::create_dir_all(root.join("progens/notes")).unwrap();

    let pj = json_stdout(
        odm()
            .current_dir(&root)
            .args(["project", "list", "--json"]),
    );
    let projects = pj["projects"].as_array().unwrap();
    assert_eq!(projects[0]["name"], "alpha");
    assert_eq!(projects[0]["on_disk"].as_bool(), Some(true));
    assert!(projects[0].get("is_git").is_some());
    assert!(projects[0].get("pin_state").is_some());

    let pg = json_stdout(
        odm()
            .current_dir(&root)
            .args(["progen", "list", "--json"]),
    );
    let progens = pg["progens"].as_array().unwrap();
    assert_eq!(progens[0]["name"], "notes");
    assert_eq!(progens[0]["on_disk"].as_bool(), Some(true));
}

#[test]
fn find_requires_progen_configured() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    fs::create_dir_all(&root).unwrap();
    odm()
        .current_dir(&root)
        .args(["init", "--no-git", "."])
        .assert()
        .success();
    odm()
        .current_dir(&root)
        .args(["find", "x"])
        .assert()
        .failure()
        .code(1);
}

#[test]
fn find_limit_zero_is_usage() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    fs::create_dir_all(&root).unwrap();
    odm()
        .current_dir(&root)
        .args(["init", "--no-git", "."])
        .assert()
        .success();
    odm()
        .current_dir(&root)
        .args(["progen", "add", "desk", "--path", "vaults/desk"])
        .assert()
        .success();
    odm()
        .current_dir(&root)
        .args(["find", "x", "--limit", "0"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("limit"));
}

#[test]
fn find_limit_caps_hits() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    fs::create_dir_all(&root).unwrap();
    odm()
        .current_dir(&root)
        .args(["init", "--no-git", "."])
        .assert()
        .success();
    odm()
        .current_dir(&root)
        .args(["progen", "add", "desk", "--path", "vaults/desk"])
        .assert()
        .success();

    let vault = root.join("vaults/desk");
    for (name, id) in [("one.md", "n1"), ("two.md", "n2"), ("three.md", "n3")] {
        fs::write(
            vault.join(name),
            format!("---\nid: {id}\n---\nLimitToken shared body.\n"),
        )
        .unwrap();
    }

    odm()
        .current_dir(&root)
        .args(["progen", "reindex"])
        .assert()
        .success();

    let v = json_stdout(
        odm()
            .current_dir(&root)
            .args(["find", "LimitToken", "--limit", "1", "--json"]),
    );
    assert_eq!(v["hits"].as_array().unwrap().len(), 1);
}

#[test]
fn multi_progen_scope() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    fs::create_dir_all(&root).unwrap();
    odm()
        .current_dir(&root)
        .args(["init", "--no-git", "."])
        .assert()
        .success();
    odm()
        .current_dir(&root)
        .args(["progen", "add", "a", "--path", "va"])
        .assert()
        .success();
    odm()
        .current_dir(&root)
        .args(["progen", "add", "b", "--path", "vb"])
        .assert()
        .success();

    fs::write(
        root.join("va/only-a.md"),
        "---\nid: onlya\n---\nsharedtoken alphaonly\n",
    )
    .unwrap();
    fs::write(
        root.join("vb/only-b.md"),
        "---\nid: onlyb\n---\nsharedtoken betaonly\n",
    )
    .unwrap();

    odm()
        .current_dir(&root)
        .args(["progen", "reindex"])
        .assert()
        .success();

    let all = json_stdout(
        odm()
            .current_dir(&root)
            .args(["find", "sharedtoken", "--json"]),
    );
    assert_eq!(all["hits"].as_array().unwrap().len(), 2);

    let only_a = json_stdout(
        odm()
            .current_dir(&root)
            .args(["find", "sharedtoken", "--progen", "a", "--json"]),
    );
    let hits = only_a["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["progen"], "a");

    // multi-progen single-root read: neutral message (no "write")
    odm()
        .current_dir(&root)
        .args(["progen", "ls"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("requires --progen"))
        .stderr(predicate::str::contains("write").not());

    // name:id vs conflicting --progen → usage
    odm()
        .current_dir(&root)
        .args(["--progen", "b", "progen", "get", "a:onlya"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("conflicts"));

    // matching name:id + --progen OK
    odm()
        .current_dir(&root)
        .args(["--progen", "a", "progen", "get", "a:onlya", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("onlya"));
}

#[test]
fn core_desk_seeded_progen_find() {
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/core-desk")
        .canonicalize()
        .unwrap();
    let dir = tempdir().unwrap();
    let root = dir.path().join("core-desk");
    copy_dir(&example, &root);

    assert!(root.join("progens/notes/Welcome.md").is_file());
    assert!(root.join("progens/notes/.obsidian/app.json").is_file());

    odm()
        .current_dir(&root)
        .args(["progen", "reindex"])
        .assert()
        .success();

    let v = json_stdout(
        odm()
            .current_dir(&root)
            .args(["find", "DeskUniqueToken", "--json"]),
    );
    let hits = v["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["id"], "welcome");
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
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
