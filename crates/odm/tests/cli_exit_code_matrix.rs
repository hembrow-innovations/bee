//! Table-driven CLI exit-code matrix (issues-140).
//! Locks exit codes 1–4 (+ action passthrough) and JSON error envelope across primary failure modes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use serde_json::Value;
use tempfile::{tempdir, TempDir};

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

fn init_ws(root: &Path) {
    odm()
        .args(["init", root.to_str().unwrap(), "--no-git"])
        .assert()
        .success();
}

fn write_config(root: &Path, yaml: &str) {
    fs::create_dir_all(root.join(".odm")).unwrap();
    fs::write(root.join(".odm/odm.config.yaml"), yaml).unwrap();
}

/// How to interpret stdout when `--json` is injected.
#[derive(Clone, Copy)]
enum JsonExpect {
    /// `{ ok: false, error: { code } }`
    ErrorEnvelope(&'static str),
    /// Action run result `{ exitCode: N }` (not error envelope).
    RunExitCode(i64),
    /// Skip JSON assertion for this row.
    None,
}

struct Case {
    name: &'static str,
    exit: i32,
    json: JsonExpect,
    /// Build fixture; return (keep-alive temp, argv without `--json`).
    build: fn() -> (TempDir, Vec<String>),
}

fn with_root(root: &Path, rest: &[&str]) -> Vec<String> {
    let mut v = vec!["--root".into(), root.to_str().unwrap().into()];
    v.extend(rest.iter().map(|s| (*s).to_string()));
    v
}

fn setup_empty_dir() -> (TempDir, Vec<String>) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("empty");
    fs::create_dir_all(&root).unwrap();
    (dir, with_root(&root, &["status"]))
}

fn setup_invalid_yaml() -> (TempDir, Vec<String>) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    write_config(&root, "projects: [\n");
    (dir, with_root(&root, &["status"]))
}

fn setup_unknown_project() -> (TempDir, Vec<String>) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    init_ws(&root);
    (dir, with_root(&root, &["project", "info", "nope"]))
}

fn setup_unknown_action() -> (TempDir, Vec<String>) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("core-desk");
    copy_dir(&core_desk_example(), &root);
    (dir, with_root(&root, &["run", "nope"]))
}

fn setup_generate_nonempty() -> (TempDir, Vec<String>) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    write_config(
        &root,
        "name: gen\ngenerators:\n  core: generators/core.yaml\n",
    );
    fs::create_dir_all(root.join("generators")).unwrap();
    fs::write(
        root.join("generators/core.yaml"),
        "hello:\n  template: templates/hello\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("templates/hello")).unwrap();
    fs::write(root.join("templates/hello/f.txt"), "x\n").unwrap();
    fs::create_dir_all(root.join("out/x")).unwrap();
    fs::write(root.join("out/x/existing.txt"), "keep\n").unwrap();
    (
        dir,
        with_root(&root, &["generate", "hello", "--dest", "out/x"]),
    )
}

fn setup_run_fail() -> (TempDir, Vec<String>) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("core-desk");
    copy_dir(&core_desk_example(), &root);
    (dir, with_root(&root, &["run", "fail"]))
}

fn setup_prune_partial() -> (TempDir, Vec<String>) {
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
        .success();
    let full = root.join("worktrees/alpha/full-orphan");
    fs::create_dir_all(&full).unwrap();
    fs::write(full.join("leftover.txt"), "x").unwrap();
    (
        dir,
        with_root(&root, &["project", "worktree", "prune", "alpha"]),
    )
}

fn setup_unknown_note() -> (TempDir, Vec<String>) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    init_ws(&root);
    odm()
        .args([
            "--root",
            root.to_str().unwrap(),
            "progen",
            "add",
            "desk",
            "--path",
            "vaults/desk",
        ])
        .assert()
        .success();
    odm()
        .args(["--root", root.to_str().unwrap(), "progen", "reindex"])
        .assert()
        .success();
    (dir, with_root(&root, &["context", "nope-missing-id"]))
}

fn setup_missing_wt() -> (TempDir, Vec<String>) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("ws");
    init_ws(&root);
    fs::create_dir_all(root.join("projects/alpha")).unwrap();
    fs::write(root.join("projects/alpha/marker"), "x\n").unwrap();
    fs::create_dir_all(root.join("actions")).unwrap();
    fs::write(
        root.join("actions/core.yaml"),
        "pwdhere:\n  tasks:\n    - run: cat marker\n",
    )
    .unwrap();
    write_config(
        &root,
        "name: cwd-ws\nprojects:\n  alpha:\n    path: projects/alpha\nactions:\n  core: actions/core.yaml\n",
    );
    (
        dir,
        with_root(
            &root,
            &["run", "pwdhere", "--project", "alpha", "--wt", "missing"],
        ),
    )
}

fn setup_clap_bad_flags() -> (TempDir, Vec<String>) {
    let dir = tempdir().unwrap();
    // no workspace needed for unknown subcommand
    (dir, vec!["notacommand".into()])
}

const CASES: &[Case] = &[
    Case {
        name: "not_a_workspace",
        exit: 2,
        json: JsonExpect::ErrorEnvelope("workspace"),
        build: setup_empty_dir,
    },
    Case {
        name: "invalid_config_yaml",
        exit: 2,
        json: JsonExpect::ErrorEnvelope("workspace"),
        build: setup_invalid_yaml,
    },
    Case {
        name: "unknown_project_name",
        exit: 1,
        json: JsonExpect::ErrorEnvelope("usage"),
        build: setup_unknown_project,
    },
    Case {
        name: "unknown_action",
        exit: 1,
        json: JsonExpect::ErrorEnvelope("usage"),
        build: setup_unknown_action,
    },
    Case {
        name: "generate_nonempty_without_force",
        exit: 3,
        json: JsonExpect::ErrorEnvelope("operation"),
        build: setup_generate_nonempty,
    },
    Case {
        name: "run_action_fail_passthrough",
        exit: 7,
        json: JsonExpect::RunExitCode(7),
        build: setup_run_fail,
    },
    Case {
        name: "worktree_prune_partial_nonempty",
        exit: 3,
        json: JsonExpect::None, // prune still prints success-shaped JSON body on partial
        build: setup_prune_partial,
    },
    Case {
        name: "unknown_note_id_context",
        exit: 4,
        json: JsonExpect::ErrorEnvelope("not_found"),
        build: setup_unknown_note,
    },
    Case {
        name: "missing_wt_slot_on_run",
        exit: 4,
        json: JsonExpect::ErrorEnvelope("not_found"),
        build: setup_missing_wt,
    },
    Case {
        name: "clap_bad_flags",
        exit: 1,
        json: JsonExpect::ErrorEnvelope("usage"),
        build: setup_clap_bad_flags,
    },
];

fn insert_json(args: &[String]) -> Vec<String> {
    let mut v = vec!["--json".to_string()];
    v.extend(args.iter().cloned());
    v
}

fn assert_error_envelope(stdout: &[u8], want_code: &str, case: &str) {
    let v: Value = serde_json::from_slice(stdout).unwrap_or_else(|e| {
        panic!(
            "{case}: stdout not JSON: {e}; got {}",
            String::from_utf8_lossy(stdout)
        )
    });
    assert_eq!(v["ok"], false, "{case}: ok");
    assert_eq!(
        v["error"]["code"].as_str(),
        Some(want_code),
        "{case}: error.code; body={v}"
    );
    assert!(
        v["error"]["message"].as_str().map(|m| !m.is_empty()) == Some(true),
        "{case}: error.message non-empty"
    );
}

#[test]
fn cli_exit_code_matrix() {
    assert!(
        CASES.len() >= 10,
        "need ≥10 distinct failure rows, got {}",
        CASES.len()
    );
    let mut json_assertions = 0usize;

    for case in CASES {
        let (_keep, args) = (case.build)();
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

        odm()
            .args(&arg_refs)
            .assert()
            .failure()
            .code(case.exit);

        match case.json {
            JsonExpect::None => {}
            JsonExpect::ErrorEnvelope(code) => {
                let jargs = insert_json(&args);
                let jrefs: Vec<&str> = jargs.iter().map(|s| s.as_str()).collect();
                let out = odm()
                    .args(&jrefs)
                    .assert()
                    .failure()
                    .code(case.exit)
                    .get_output()
                    .stdout
                    .clone();
                assert_error_envelope(&out, code, case.name);
                json_assertions += 1;
            }
            JsonExpect::RunExitCode(n) => {
                let jargs = insert_json(&args);
                let jrefs: Vec<&str> = jargs.iter().map(|s| s.as_str()).collect();
                let out = odm()
                    .args(&jrefs)
                    .assert()
                    .failure()
                    .code(case.exit)
                    .get_output()
                    .stdout
                    .clone();
                let v: Value = serde_json::from_slice(&out).expect("run json");
                assert_eq!(
                    v["exitCode"].as_i64(),
                    Some(n),
                    "{}: exitCode; body={v}",
                    case.name
                );
                json_assertions += 1;
            }
        }
    }

    assert!(
        json_assertions >= 4,
        "need ≥4 JSON assertions, got {json_assertions}"
    );
}
