use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use odm_git::{Git, GitError};
use tempfile::TempDir;

fn abs(dir: &TempDir, name: &str) -> PathBuf {
    dir.path().join(name)
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

fn commit_file(repo: &Path, name: &str, body: &str) {
    fs::write(repo.join(name), body).unwrap();
    assert!(Command::new("git")
        .args(["-C", repo.to_str().unwrap(), "add", name])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["-C", repo.to_str().unwrap(), "commit", "-m", name])
        .status()
        .unwrap()
        .success());
}

fn bare_with_commit(root: &TempDir) -> PathBuf {
    let bare = abs(root, "remote.git");
    assert!(Command::new("git")
        .args(["init", "--bare", bare.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    let seed = abs(root, "seed");
    assert!(Command::new("git")
        .args(["clone", bare.to_str().unwrap(), seed.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    git_user(&seed);
    commit_file(&seed, "README", "hi");
    assert!(Command::new("git")
        .args(["-C", seed.to_str().unwrap(), "push", "origin", "HEAD"])
        .status()
        .unwrap()
        .success());
    bare
}

#[test]
fn rejects_relative_path() {
    let g = Git::new();
    let err = g.is_repo(Path::new("relative")).unwrap_err();
    assert!(matches!(err, GitError::NotAbsolute(_)));
}

#[test]
fn is_repo_false_when_missing() {
    let t = TempDir::new().unwrap();
    let g = Git::new();
    assert!(!g.is_repo(&abs(&t, "nope")).unwrap());
}

#[test]
fn init_and_is_repo() {
    let t = TempDir::new().unwrap();
    let path = abs(&t, "ws");
    fs::create_dir(&path).unwrap();
    let g = Git::new();
    g.init(&path).unwrap();
    assert!(g.is_repo(&path).unwrap());
    assert!(g.is_repo_root(&path).unwrap());
}

#[test]
fn is_repo_root_false_for_nested_path_without_own_git() {
    let t = TempDir::new().unwrap();
    let root = abs(&t, "ws");
    fs::create_dir(&root).unwrap();
    let g = Git::new();
    g.init(&root).unwrap();
    let nested = root.join("vault");
    fs::create_dir(&nested).unwrap();
    assert!(g.is_repo(&nested).unwrap(), "nested path is inside work tree");
    assert!(
        !g.is_repo_root(&nested).unwrap(),
        "nested path is not its own repo root"
    );
}

#[test]
fn clone_fetch_head_origin_clean() {
    let t = TempDir::new().unwrap();
    let bare = bare_with_commit(&t);
    let dest = abs(&t, "proj");
    let g = Git::new();

    g.clone(bare.to_str().unwrap(), &dest, None).unwrap();
    assert!(g.is_repo(&dest).unwrap());

    let sha = g.head_sha(&dest).unwrap();
    assert_eq!(sha.len(), 40);
    assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));

    let url = g.origin_url(&dest).unwrap();
    assert!(url.contains("remote.git") || url.ends_with("remote.git"));

    assert!(g.is_clean(&dest).unwrap());
    g.fetch(&dest).unwrap();
}

#[test]
fn clone_with_branch() {
    let t = TempDir::new().unwrap();
    let bare = bare_with_commit(&t);
    let seed = abs(&t, "seed");
    // seed already exists from bare_with_commit — use fresh work for branch
    let work = abs(&t, "branch-work");
    assert!(Command::new("git")
        .args(["clone", bare.to_str().unwrap(), work.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    git_user(&work);
    assert!(Command::new("git")
        .args(["-C", work.to_str().unwrap(), "checkout", "-b", "feature"])
        .status()
        .unwrap()
        .success());
    commit_file(&work, "f.txt", "x");
    assert!(Command::new("git")
        .args(["-C", work.to_str().unwrap(), "push", "-u", "origin", "feature"])
        .status()
        .unwrap()
        .success());

    let dest = abs(&t, "on-feature");
    let g = Git::new();
    g.clone(bare.to_str().unwrap(), &dest, Some("feature"))
        .unwrap();
    let head = Command::new("git")
        .args(["-C", dest.to_str().unwrap(), "branch", "--show-current"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&head.stdout).trim(), "feature");
    let _ = seed; // silence if unused on some paths
}

#[test]
fn is_clean_false_with_untracked() {
    let t = TempDir::new().unwrap();
    let path = abs(&t, "r");
    fs::create_dir(&path).unwrap();
    let g = Git::new();
    g.init(&path).unwrap();
    git_user(&path);
    commit_file(&path, "a", "1");
    assert!(g.is_clean(&path).unwrap());
    fs::write(path.join("dirt"), "x").unwrap();
    assert!(!g.is_clean(&path).unwrap());
}

#[test]
fn checkout_detached_at_sha() {
    let t = TempDir::new().unwrap();
    let bare = bare_with_commit(&t);
    let dest = abs(&t, "det");
    let g = Git::new();
    g.clone(bare.to_str().unwrap(), &dest, None).unwrap();
    let sha = g.head_sha(&dest).unwrap();
    // second commit
    git_user(&dest);
    commit_file(&dest, "b", "2");
    let tip = g.head_sha(&dest).unwrap();
    assert_ne!(sha, tip);
    g.checkout_detached(&dest, &sha).unwrap();
    assert_eq!(g.head_sha(&dest).unwrap(), sha);
}

#[test]
fn origin_missing_on_repo_without_remote() {
    let t = TempDir::new().unwrap();
    let path = abs(&t, "local");
    fs::create_dir(&path).unwrap();
    let g = Git::new();
    g.init(&path).unwrap();
    let err = g.origin_url(&path).unwrap_err();
    assert!(matches!(err, GitError::OriginMissing { .. }));
}

#[test]
fn run_empty_args_errors() {
    let t = TempDir::new().unwrap();
    let path = abs(&t, "r");
    fs::create_dir(&path).unwrap();
    let g = Git::new();
    g.init(&path).unwrap();
    let err = g.run(&path, &[] as &[&str]).unwrap_err();
    assert!(matches!(err, GitError::EmptyArgs));
}

#[test]
fn run_passthrough_status() {
    let t = TempDir::new().unwrap();
    let path = abs(&t, "r");
    fs::create_dir(&path).unwrap();
    let g = Git::new();
    g.init(&path).unwrap();
    let st = g.run(&path, &["rev-parse", "--is-inside-work-tree"]).unwrap();
    assert!(st.success());
}

#[test]
fn clone_into_empty_dir() {
    let t = TempDir::new().unwrap();
    let bare = bare_with_commit(&t);
    let dest = abs(&t, "empty");
    fs::create_dir(&dest).unwrap();
    let g = Git::new();
    g.clone(bare.to_str().unwrap(), &dest, None).unwrap();
    assert!(g.is_repo(&dest).unwrap());
}

#[test]
fn failed_clone_attaches_stderr() {
    let t = TempDir::new().unwrap();
    let dest = abs(&t, "bad");
    let g = Git::new();
    let err = g
        .clone("file:///nonexistent-odm-git-remote-xyz", &dest, None)
        .unwrap_err();
    match err {
        GitError::Failed {
            operation,
            stderr,
            code,
            ..
        } => {
            assert_eq!(operation, "clone");
            assert!(code.is_some_and(|c| c != 0));
            assert!(!stderr.is_empty());
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn worktree_add_list_remove_round_trip() {
    let t = TempDir::new().unwrap();
    let primary = abs(&t, "primary");
    fs::create_dir(&primary).unwrap();
    let g = Git::new();
    g.init(&primary).unwrap();
    git_user(&primary);
    commit_file(&primary, "README", "hi");

    let slot = abs(&t, "slot");
    g.worktree_add(&primary, &slot, Some("wt-branch")).unwrap();
    assert!(slot.is_dir());
    let slot_canon = fs::canonicalize(&slot).unwrap();

    let entries = g.worktree_list(&primary).unwrap();
    assert!(
        entries
            .iter()
            .any(|e| fs::canonicalize(&e.path).ok().as_ref() == Some(&slot_canon)),
        "list should contain slot path; got {entries:?}"
    );

    g.worktree_remove(&primary, &slot, false).unwrap();
    assert!(!slot.exists());
    let after = g.worktree_list(&primary).unwrap();
    assert!(
        after
            .iter()
            .all(|e| fs::canonicalize(&e.path).ok().as_ref() != Some(&slot_canon)),
        "list should not contain removed slot; got {after:?}"
    );
}
