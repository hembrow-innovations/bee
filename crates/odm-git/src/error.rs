use std::path::PathBuf;
use std::process::ExitStatus;

/// Errors from `odm-git` public operations.
///
/// Policy (dirty/force, origin URL match) lives in core — this crate reports facts and exec failures.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("path is not absolute: {0}")]
    NotAbsolute(PathBuf),

    #[error("git executable not found on PATH")]
    GitNotFound(#[source] std::io::Error),

    #[error("not a git work tree: {}", .path.display())]
    NotARepo { path: PathBuf },

    #[error("remote 'origin' is not configured: {}", .path.display())]
    OriginMissing { path: PathBuf },

    #[error("git {operation} failed{}{}", path_suffix(.path), code_suffix(*.code))]
    Failed {
        operation: &'static str,
        path: Option<PathBuf>,
        code: Option<i32>,
        stderr: String,
    },

    #[error("unexpected git output for {operation}: {detail}")]
    Parse {
        operation: &'static str,
        stdout: String,
        detail: String,
    },

    #[error("git passthrough requires at least one argument")]
    EmptyArgs,
}

fn path_suffix(path: &Option<PathBuf>) -> String {
    match path {
        Some(p) => format!(" in {}", p.display()),
        None => String::new(),
    }
}

fn code_suffix(code: Option<i32>) -> String {
    match code {
        Some(c) => format!(" (exit {c})"),
        None => String::new(),
    }
}

impl GitError {
    pub fn failed(
        operation: &'static str,
        path: Option<PathBuf>,
        status: ExitStatus,
        stderr: impl Into<String>,
        stdout: impl Into<String>,
    ) -> Self {
        let stderr = trim_output(stderr.into());
        let stdout = trim_output(stdout.into());
        let detail = if !stderr.is_empty() {
            stderr
        } else {
            stdout
        };
        Self::Failed {
            operation,
            path,
            code: status.code(),
            stderr: detail,
        }
    }
}

pub(crate) fn trim_output(s: String) -> String {
    s.trim().to_string()
}
