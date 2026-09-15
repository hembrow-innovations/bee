use std::ffi::{OsStr, OsString};
use std::io;
use std::process::{Command, ExitStatus, Output, Stdio};

/// Result of one process invocation.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl CommandOutput {
    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// Injectable process runner (real git or test double).
pub trait CommandRunner {
    fn output(
        &self,
        program: &OsStr,
        args: &[OsString],
    ) -> io::Result<CommandOutput>;

    /// Inherit stdio (for `project git` passthrough).
    fn status(&self, program: &OsStr, args: &[OsString]) -> io::Result<ExitStatus>;
}

/// Shells out via `std::process::Command`.
///
/// Lifecycle ops ([`CommandRunner::output`]) set `GIT_TERMINAL_PROMPT=0` so
/// clone/fetch/etc. fail fast instead of hanging on interactive auth.
/// [`CommandRunner::status`] (`Git::run` passthrough) inherits stdio and does
/// not force non-interactive env — that path is user-facing.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessRunner;

/// Build a `Command` for ODM lifecycle git ops (captured stdio, non-interactive).
pub(crate) fn lifecycle_command(program: &OsStr, args: &[OsString]) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd
}

impl CommandRunner for ProcessRunner {
    fn output(&self, program: &OsStr, args: &[OsString]) -> io::Result<CommandOutput> {
        let out: Output = lifecycle_command(program, args).output()?;
        Ok(CommandOutput {
            status: out.status,
            stdout: out.stdout,
            stderr: out.stderr,
        })
    }

    fn status(&self, program: &OsStr, args: &[OsString]) -> io::Result<ExitStatus> {
        Command::new(program)
            .args(args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::ffi::OsString;

    #[test]
    fn lifecycle_command_sets_git_terminal_prompt_zero() {
        let args = [OsString::from("status")];
        let cmd = lifecycle_command(OsStr::new("git"), &args);
        let envs: HashMap<_, _> = cmd.get_envs().collect();
        assert_eq!(
            envs.get(OsStr::new("GIT_TERMINAL_PROMPT")).copied().flatten(),
            Some(OsStr::new("0"))
        );
    }
}
