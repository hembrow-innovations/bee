use odm_git::GitError;

/// Typed library errors. Process exit mapping lives in the `odm` bin.
#[derive(Debug, thiserror::Error)]
pub enum OdmError {
    #[error("{0}")]
    Usage(String),

    #[error("{0}")]
    Workspace(String),

    #[error("{0}")]
    Operation(String),

    #[error("{0}")]
    NotFound(String),
}

impl OdmError {
    pub fn usage(msg: impl Into<String>) -> Self {
        Self::Usage(msg.into())
    }

    pub fn workspace(msg: impl Into<String>) -> Self {
        Self::Workspace(msg.into())
    }

    pub fn operation(msg: impl Into<String>) -> Self {
        Self::Operation(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub fn not_implemented(verb: &str) -> Self {
        Self::Usage(format!("not implemented: {verb}"))
    }

    /// Machine-stable error code for `--json` envelopes.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Usage(_) => "usage",
            Self::Workspace(_) => "workspace",
            Self::Operation(_) => "operation",
            Self::NotFound(_) => "not_found",
        }
    }

    pub fn message(&self) -> String {
        self.to_string()
    }

    /// Optional detail (e.g. git stderr) for JSON `detail` field.
    pub fn detail(&self) -> Option<String> {
        match self {
            Self::Operation(msg) if msg.contains('\n') => {
                let mut lines = msg.lines();
                let _first = lines.next();
                let rest: Vec<_> = lines.collect();
                if rest.is_empty() {
                    None
                } else {
                    Some(rest.join("\n"))
                }
            }
            _ => None,
        }
    }
}

impl From<GitError> for OdmError {
    fn from(err: GitError) -> Self {
        match &err {
            GitError::NotAbsolute(p) => {
                Self::Operation(format!("path is not absolute: {}", p.display()))
            }
            GitError::GitNotFound(_) => Self::Operation("git executable not found on PATH".into()),
            GitError::NotARepo { path } => {
                Self::Operation(format!("not a git work tree: {}", path.display()))
            }
            GitError::OriginMissing { path } => Self::Operation(format!(
                "remote 'origin' is not configured: {}",
                path.display()
            )),
            GitError::Failed {
                operation,
                path,
                code,
                stderr,
            } => {
                let mut msg = format!("git {operation} failed");
                if let Some(p) = path {
                    msg.push_str(&format!(" in {}", p.display()));
                }
                if let Some(c) = code {
                    msg.push_str(&format!(" (exit {c})"));
                }
                if !stderr.is_empty() {
                    msg.push('\n');
                    msg.push_str(stderr);
                }
                Self::Operation(msg)
            }
            GitError::Parse {
                operation,
                detail,
                ..
            } => Self::Operation(format!("unexpected git output for {operation}: {detail}")),
            GitError::EmptyArgs => {
                Self::Usage("git passthrough requires at least one argument".into())
            }
        }
    }
}

impl From<std::io::Error> for OdmError {
    fn from(err: std::io::Error) -> Self {
        Self::Operation(err.to_string())
    }
}

/// Exit code for the bin (0 success; 1–4 error kinds).
pub fn exit_code(err: &OdmError) -> i32 {
    match err {
        OdmError::Usage(_) => 1,
        OdmError::Workspace(_) => 2,
        OdmError::Operation(_) => 3,
        OdmError::NotFound(_) => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn exit_code_matrix() {
        let cases: &[(OdmError, i32)] = &[
            (OdmError::usage("bad flag"), 1),
            (OdmError::workspace("no root"), 2),
            (OdmError::operation("git failed"), 3),
            (OdmError::not_found("missing"), 4),
        ];
        for (err, want) in cases {
            assert_eq!(exit_code(err), *want, "exit_code for {:?}", err.code());
        }
    }

    #[test]
    fn code_stable_strings() {
        let cases: &[(OdmError, &str)] = &[
            (OdmError::usage("x"), "usage"),
            (OdmError::workspace("x"), "workspace"),
            (OdmError::operation("x"), "operation"),
            (OdmError::not_found("x"), "not_found"),
        ];
        for (err, want) in cases {
            assert_eq!(err.code(), *want);
        }
    }

    #[test]
    fn detail_multiline_operation_only() {
        let cases: &[(OdmError, Option<&str>)] = &[
            (OdmError::operation("single line"), None),
            (OdmError::operation("first\n"), None),
            (
                OdmError::operation("git fetch failed (exit 1)\nfatal: remote gone\nretry later"),
                Some("fatal: remote gone\nretry later"),
            ),
            (OdmError::usage("a\nb"), None),
            (OdmError::workspace("a\nb"), None),
            (OdmError::not_found("a\nb"), None),
        ];
        for (err, want) in cases {
            assert_eq!(err.detail().as_deref(), *want, "detail for {}", err.message());
        }
    }

    #[test]
    fn message_matches_display() {
        let err = OdmError::not_found("nope");
        assert_eq!(err.message(), err.to_string());
        assert_eq!(err.message(), "nope");
    }

    #[test]
    fn not_implemented_is_usage() {
        let err = OdmError::not_implemented("frobnicate");
        assert_eq!(err.code(), "usage");
        assert_eq!(exit_code(&err), 1);
        assert!(err.message().contains("frobnicate"));
    }

    #[test]
    fn from_git_error_failed_carries_multiline_detail() {
        let ge = GitError::Failed {
            operation: "fetch",
            path: Some(PathBuf::from("/ws/p")),
            code: Some(128),
            stderr: "fatal: remote error\nplease check".into(),
        };
        let err: OdmError = ge.into();
        assert_eq!(err.code(), "operation");
        assert_eq!(exit_code(&err), 3);
        assert_eq!(err.detail().as_deref(), Some("fatal: remote error\nplease check"));
        assert!(err.message().contains("git fetch failed"));
    }

    #[test]
    fn from_git_error_empty_args_is_usage() {
        let err: OdmError = GitError::EmptyArgs.into();
        assert_eq!(err.code(), "usage");
        assert_eq!(exit_code(&err), 1);
    }

    #[test]
    fn from_io_error_is_operation() {
        let err: OdmError = std::io::Error::other("disk full").into();
        assert_eq!(err.code(), "operation");
        assert_eq!(exit_code(&err), 3);
        assert!(err.message().contains("disk full"));
    }
}
