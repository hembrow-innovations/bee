//! CLI integration: `find --progen-group` scope.

use std::fs;

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

/// Two progens + group `only-a` → [a]; shared FTS token in both.
fn multi_progen_with_group() -> (tempfile::TempDir, std::path::PathBuf) {
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

    let cfg = root.join(".odm/odm.config.yaml");
    let mut yaml = fs::read_to_string(&cfg).unwrap();
    yaml.push_str("progen_groups:\n  only-a:\n    - a\n");
    fs::write(&cfg, yaml).unwrap();

    fs::write(
        root.join("va/only-a.md"),
        "---\nid: onlya\n---\nGroupToken alphaonly\n",
    )
    .unwrap();
    fs::write(
        root.join("vb/only-b.md"),
        "---\nid: onlyb\n---\nGroupToken betaonly\n",
    )
    .unwrap();

    odm()
        .current_dir(&root)
        .args(["progen", "reindex"])
        .assert()
        .success();

    (dir, root)
}

#[test]
fn find_progen_group_narrows_hits() {
    let (_dir, root) = multi_progen_with_group();

    let all = json_stdout(
        odm()
            .current_dir(&root)
            .args(["find", "GroupToken", "--json"]),
    );
    assert_eq!(all["hits"].as_array().unwrap().len(), 2);

    let scoped = json_stdout(
        odm()
            .current_dir(&root)
            .args(["find", "GroupToken", "--progen-group", "only-a", "--json"]),
    );
    let hits = scoped["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["progen"], "a");
    assert_eq!(hits[0]["id"], "onlya");
}

#[test]
fn find_unknown_progen_group_exits_usage() {
    let (_dir, root) = multi_progen_with_group();

    odm()
        .current_dir(&root)
        .args(["find", "GroupToken", "--progen-group", "no-such-group"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown progen group"));
}

#[test]
fn find_progen_union_progen_group() {
    let (_dir, root) = multi_progen_with_group();

    // --progen b + group only-a → union {a, b} → both hits
    let union = json_stdout(
        odm().current_dir(&root).args([
            "find",
            "GroupToken",
            "--progen",
            "b",
            "--progen-group",
            "only-a",
            "--json",
        ]),
    );
    let hits = union["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 2);
    let names: Vec<_> = hits
        .iter()
        .map(|h| h["progen"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"a"));
    assert!(names.contains(&"b"));
}
