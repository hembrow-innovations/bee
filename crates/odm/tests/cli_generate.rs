//! Integration tests for `odm generate` (local template v1).

use std::fs;
use std::path::Path;

use assert_cmd::cargo::cargo_bin;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

fn odm() -> assert_cmd::Command {
    assert_cmd::Command::new(cargo_bin("odm"))
}

/// Minimal workspace: local `hello` template + url-only `remote`.
fn setup_gen_ws() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("gen-ws");
    fs::create_dir_all(root.join(".odm")).unwrap();
    fs::write(
        root.join(".odm/odm.config.yaml"),
        "name: gen-ws\ngenerators:\n  core: generators/core.yaml\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("generators")).unwrap();
    fs::write(
        root.join("generators/core.yaml"),
        "hello:\n  template: templates/hello\nremote:\n  url: https://example.com/gen.git\n",
    )
    .unwrap();
    let nested = root.join("templates/hello/nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("hello.txt"), "hello-from-template\n").unwrap();
    (dir, root)
}

fn json_stdout(cmd: &mut assert_cmd::Command) -> Value {
    let out = cmd.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&out).expect("stdout JSON")
}

fn assert_file_eq(path: &Path, want: &str) {
    let got = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert_eq!(got, want, "contents of {}", path.display());
}

#[test]
fn generate_lists_names() {
    let (_dir, root) = setup_gen_ws();
    let root_s = root.to_str().unwrap();

    odm()
        .args(["--root", root_s, "generate"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hello"))
        .stdout(predicate::str::contains("remote"));

    let v = json_stdout(odm().args(["--root", root_s, "--json", "generate"]));
    let gens = v["generators"].as_array().expect("generators");
    let names: Vec<_> = gens
        .iter()
        .map(|g| g["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"hello"));
    assert!(names.contains(&"remote"));
    let hello = gens.iter().find(|g| g["name"] == "hello").unwrap();
    assert_eq!(hello["template"], "templates/hello");
    assert!(hello["url"].is_null());
    let remote = gens.iter().find(|g| g["name"] == "remote").unwrap();
    assert!(remote["template"].is_null());
    assert_eq!(remote["url"], "https://example.com/gen.git");
}

#[test]
fn generate_materializes_nested_template() {
    let (_dir, root) = setup_gen_ws();
    let root_s = root.to_str().unwrap();

    odm()
        .args([
            "--root",
            root_s,
            "generate",
            "hello",
            "--dest",
            "out/x",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("generated hello -> out/x"));

    assert_file_eq(
        &root.join("out/x/nested/hello.txt"),
        "hello-from-template\n",
    );

    let v = json_stdout(odm().args([
        "--root",
        root_s,
        "--json",
        "generate",
        "hello",
        "--dest",
        "out/json",
    ]));
    assert_eq!(v["generator"], "hello");
    assert_eq!(v["dest"], "out/json");
    assert_eq!(v["copied"].as_u64(), Some(1));
    assert_eq!(v["dry_run"], false);
    assert_file_eq(
        &root.join("out/json/nested/hello.txt"),
        "hello-from-template\n",
    );
}

#[test]
fn generate_dry_run_json_no_write() {
    let (_dir, root) = setup_gen_ws();
    let root_s = root.to_str().unwrap();
    let dest = root.join("out/dry");

    let v = json_stdout(odm().args([
        "--root",
        root_s,
        "--json",
        "generate",
        "hello",
        "--dest",
        "out/dry",
        "--dry-run",
    ]));
    assert_eq!(v["generator"], "hello");
    assert_eq!(v["dest"], "out/dry");
    assert_eq!(v["dry_run"], true);
    assert!(v["copied"].as_u64().unwrap() > 0);
    assert!(!dest.exists(), "dry-run must not create dest");
}

#[test]
fn generate_dry_run_human_would_generate() {
    let (_dir, root) = setup_gen_ws();
    let root_s = root.to_str().unwrap();

    odm()
        .args([
            "--root",
            root_s,
            "generate",
            "hello",
            "--dest",
            "out/dry-h",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("would generate hello -> out/dry-h"));

    assert!(!root.join("out/dry-h").exists());
}

#[test]
fn generate_dry_run_nonempty_dest_requires_force() {
    let (_dir, root) = setup_gen_ws();
    let root_s = root.to_str().unwrap();

    odm()
        .args([
            "--root",
            root_s,
            "generate",
            "hello",
            "--dest",
            "out/x",
        ])
        .assert()
        .success();

    odm()
        .args([
            "--root",
            root_s,
            "generate",
            "hello",
            "--dest",
            "out/x",
            "--dry-run",
        ])
        .assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains("not empty"))
        .stderr(predicate::str::contains("force"));
}

#[test]
fn generate_url_only_dry_run_exit_1() {
    let (_dir, root) = setup_gen_ws();
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "generate",
            "remote",
            "--dest",
            "out/remote",
            "--dry-run",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no local template"))
        .stderr(predicate::str::contains("remote"));
}

#[test]
fn generate_force_required_when_dest_nonempty() {
    let (_dir, root) = setup_gen_ws();
    let root_s = root.to_str().unwrap();

    odm()
        .args([
            "--root",
            root_s,
            "generate",
            "hello",
            "--dest",
            "out/x",
        ])
        .assert()
        .success();

    odm()
        .args([
            "--root",
            root_s,
            "generate",
            "hello",
            "--dest",
            "out/x",
        ])
        .assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains("not empty"))
        .stderr(predicate::str::contains("force"));

    // mutate dest then force-overwrite
    fs::write(root.join("out/x/nested/hello.txt"), "stale\n").unwrap();

    odm()
        .args([
            "--root",
            root_s,
            "generate",
            "hello",
            "--dest",
            "out/x",
            "--force",
        ])
        .assert()
        .success();

    assert_file_eq(
        &root.join("out/x/nested/hello.txt"),
        "hello-from-template\n",
    );
}

#[test]
fn generate_unknown_name_exit_1() {
    let (_dir, root) = setup_gen_ws();
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "generate",
            "nope",
            "--dest",
            "out",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown generator"));
}

#[test]
fn generate_url_only_exit_1() {
    let (_dir, root) = setup_gen_ws();
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "generate",
            "remote",
            "--dest",
            "out/remote",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no local template"))
        .stderr(predicate::str::contains("remote"));
}
