//! Unit tests for worktree ops via injectable CommandRunner (no real git).

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::{Arc, Mutex};

use odm_git::{CommandOutput, CommandRunner, Git, GitError, WorktreeEntry};

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

#[cfg(not(unix))]
use std::process::Command;

fn exit_ok() -> ExitStatus {
    #[cfg(unix)]
    {
        ExitStatus::from_raw(0)
    }
    #[cfg(not(unix))]
    {
        Command::new("true").status().unwrap()
    }
}

fn exit_fail(code: i32) -> ExitStatus {
    #[cfg(unix)]
    {
        ExitStatus::from_raw(code << 8)
    }
    #[cfg(not(unix))]
    {
        let _ = code;
        Command::new("false").status().unwrap()
    }
}

struct RecordingRunner {
    calls: Arc<Mutex<Vec<Vec<OsString>>>>,
    next: Mutex<Option<io::Result<CommandOutput>>>,
}

impl RecordingRunner {
    fn new(out: CommandOutput) -> (Self, Arc<Mutex<Vec<Vec<OsString>>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                calls: Arc::clone(&calls),
                next: Mutex::new(Some(Ok(out))),
            },
            calls,
        )
    }

    fn ok() -> (Self, Arc<Mutex<Vec<Vec<OsString>>>>) {
        Self::new(CommandOutput {
            status: exit_ok(),
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }

    fn fail(operation_stderr: &str) -> (Self, Arc<Mutex<Vec<Vec<OsString>>>>) {
        Self::new(CommandOutput {
            status: exit_fail(1),
            stdout: Vec::new(),
            stderr: operation_stderr.as_bytes().to_vec(),
        })
    }

    fn porcelain(stdout: &str) -> (Self, Arc<Mutex<Vec<Vec<OsString>>>>) {
        Self::new(CommandOutput {
            status: exit_ok(),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        })
    }
}

impl CommandRunner for RecordingRunner {
    fn output(&self, _program: &OsStr, args: &[OsString]) -> io::Result<CommandOutput> {
        self.calls.lock().unwrap().push(args.to_vec());
        self.next.lock().unwrap().take().unwrap_or_else(|| {
            Ok(CommandOutput {
                status: exit_ok(),
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        })
    }

    fn status(&self, _program: &OsStr, args: &[OsString]) -> io::Result<ExitStatus> {
        self.calls.lock().unwrap().push(args.to_vec());
        Ok(exit_ok())
    }
}

fn args_as_strings(args: &[OsString]) -> Vec<String> {
    args.iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn worktree_add_rejects_relative_paths_without_runner_call() {
    let (runner, calls) = RecordingRunner::ok();
    let g = Git::with_runner(runner);
    let err = g
        .worktree_add(Path::new("rel"), Path::new("/abs/slot"), None)
        .unwrap_err();
    assert!(matches!(err, GitError::NotAbsolute(_)));
    assert!(calls.lock().unwrap().is_empty());

    let (runner, calls) = RecordingRunner::ok();
    let g = Git::with_runner(runner);
    let err = g
        .worktree_add(Path::new("/abs/primary"), Path::new("rel-slot"), None)
        .unwrap_err();
    assert!(matches!(err, GitError::NotAbsolute(_)));
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn worktree_list_rejects_relative_primary_without_runner_call() {
    let (runner, calls) = RecordingRunner::ok();
    let g = Git::with_runner(runner);
    let err = g.worktree_list(Path::new("rel")).unwrap_err();
    assert!(matches!(err, GitError::NotAbsolute(_)));
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn worktree_remove_rejects_relative_paths_without_runner_call() {
    let (runner, calls) = RecordingRunner::ok();
    let g = Git::with_runner(runner);
    let err = g
        .worktree_remove(Path::new("/p"), Path::new("slot"), false)
        .unwrap_err();
    assert!(matches!(err, GitError::NotAbsolute(_)));
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn worktree_add_argv_without_branch() {
    let (runner, calls) = RecordingRunner::ok();
    let g = Git::with_runner(runner);
    g.worktree_add(Path::new("/repo"), Path::new("/repo/wt/slot"), None)
        .unwrap();
    let got = calls.lock().unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(
        args_as_strings(&got[0]),
        vec!["-C", "/repo", "worktree", "add", "--", "/repo/wt/slot"]
    );
}

#[test]
fn worktree_add_argv_with_branch() {
    let (runner, calls) = RecordingRunner::ok();
    let g = Git::with_runner(runner);
    g.worktree_add(
        Path::new("/repo"),
        Path::new("/repo/wt/slot"),
        Some("feature/x"),
    )
    .unwrap();
    let got = calls.lock().unwrap();
    assert_eq!(
        args_as_strings(&got[0]),
        vec![
            "-C",
            "/repo",
            "worktree",
            "add",
            "-b",
            "feature/x",
            "--",
            "/repo/wt/slot"
        ]
    );
}

#[test]
fn worktree_remove_argv_without_force() {
    let (runner, calls) = RecordingRunner::ok();
    let g = Git::with_runner(runner);
    g.worktree_remove(Path::new("/repo"), Path::new("/repo/wt/slot"), false)
        .unwrap();
    assert_eq!(
        args_as_strings(&calls.lock().unwrap()[0]),
        vec![
            "-C",
            "/repo",
            "worktree",
            "remove",
            "--",
            "/repo/wt/slot"
        ]
    );
}

#[test]
fn worktree_remove_argv_with_force() {
    let (runner, calls) = RecordingRunner::ok();
    let g = Git::with_runner(runner);
    g.worktree_remove(Path::new("/repo"), Path::new("/repo/wt/slot"), true)
        .unwrap();
    assert_eq!(
        args_as_strings(&calls.lock().unwrap()[0]),
        vec![
            "-C",
            "/repo",
            "worktree",
            "remove",
            "--force",
            "--",
            "/repo/wt/slot"
        ]
    );
}

#[test]
fn worktree_list_argv_porcelain() {
    let porcelain = "\
worktree /repo
HEAD abcdef0123456789abcdef0123456789abcdef01
branch refs/heads/main

worktree /repo/wt/slot
HEAD fedcba9876543210fedcba9876543210fedcba98
branch refs/heads/feature/x

";
    let (runner, calls) = RecordingRunner::porcelain(porcelain);
    let g = Git::with_runner(runner);
    let entries = g.worktree_list(Path::new("/repo")).unwrap();
    assert_eq!(
        args_as_strings(&calls.lock().unwrap()[0]),
        vec!["-C", "/repo", "worktree", "list", "--porcelain"]
    );
    assert_eq!(
        entries,
        vec![
            WorktreeEntry {
                path: PathBuf::from("/repo"),
                head: Some("abcdef0123456789abcdef0123456789abcdef01".into()),
                branch: Some("refs/heads/main".into()),
            },
            WorktreeEntry {
                path: PathBuf::from("/repo/wt/slot"),
                head: Some("fedcba9876543210fedcba9876543210fedcba98".into()),
                branch: Some("refs/heads/feature/x".into()),
            },
        ]
    );
}

#[test]
fn worktree_list_includes_detached_entry() {
    let porcelain = "\
worktree /repo
HEAD abcdef0123456789abcdef0123456789abcdef01
branch refs/heads/main

worktree /repo/wt/det
HEAD fedcba9876543210fedcba9876543210fedcba98
detached

";
    let (runner, _) = RecordingRunner::porcelain(porcelain);
    let g = Git::with_runner(runner);
    let entries = g.worktree_list(Path::new("/repo")).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].path, PathBuf::from("/repo/wt/det"));
    assert_eq!(
        entries[1].head.as_deref(),
        Some("fedcba9876543210fedcba9876543210fedcba98")
    );
    assert_eq!(entries[1].branch, None);
}

#[test]
fn worktree_list_parse_error_on_record_without_worktree_path() {
    let porcelain = "\
HEAD abcdef0123456789abcdef0123456789abcdef01
branch refs/heads/main

";
    let (runner, _) = RecordingRunner::porcelain(porcelain);
    let g = Git::with_runner(runner);
    let err = g.worktree_list(Path::new("/repo")).unwrap_err();
    match err {
        GitError::Parse { operation, .. } => assert_eq!(operation, "worktree_list"),
        other => panic!("expected Parse, got {other:?}"),
    }
}

#[test]
fn worktree_add_failed_status() {
    let (runner, _) = RecordingRunner::fail("fatal: already exists");
    let g = Git::with_runner(runner);
    let err = g
        .worktree_add(Path::new("/repo"), Path::new("/repo/wt/s"), None)
        .unwrap_err();
    match err {
        GitError::Failed {
            operation, stderr, ..
        } => {
            assert_eq!(operation, "worktree_add");
            assert!(stderr.contains("already exists"));
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn worktree_list_failed_status() {
    let (runner, _) = RecordingRunner::fail("fatal: not a git repository");
    let g = Git::with_runner(runner);
    let err = g.worktree_list(Path::new("/repo")).unwrap_err();
    match err {
        GitError::Failed { operation, .. } => assert_eq!(operation, "worktree_list"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn worktree_remove_failed_status() {
    let (runner, _) = RecordingRunner::fail("fatal: dirty");
    let g = Git::with_runner(runner);
    let err = g
        .worktree_remove(Path::new("/repo"), Path::new("/repo/wt/s"), false)
        .unwrap_err();
    match err {
        GitError::Failed { operation, .. } => assert_eq!(operation, "worktree_remove"),
        other => panic!("expected Failed, got {other:?}"),
    }
}
