use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

pub fn allow_file_protocol() {
    static LOCK: Mutex<bool> = Mutex::new(false);
    let mut set = LOCK.lock().unwrap();
    if !*set {
        std::env::set_var("GIT_CONFIG_COUNT", "1");
        std::env::set_var("GIT_CONFIG_KEY_0", "protocol.file.allow");
        std::env::set_var("GIT_CONFIG_VALUE_0", "always");
        *set = true;
    }
}

pub fn git_init_commit(root: &Path) {
    allow_file_protocol();
    assert!(Command::new("git")
        .args(["init", root.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    git_user(root);
    fs::write(root.join("README"), "ws").unwrap();
    assert!(Command::new("git")
        .args(["-C", root.to_str().unwrap(), "add", "README"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["-C", root.to_str().unwrap(), "commit", "-m", "init"])
        .status()
        .unwrap()
        .success());
}

pub fn git_user(repo: &Path) {
    allow_file_protocol();
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

pub fn head_sha(repo: &Path) -> String {
    let out = Command::new("git")
        .args(["-C", repo.to_str().unwrap(), "rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

pub fn bare_with_main(root: &Path, name: &str) -> PathBuf {
    allow_file_protocol();
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
