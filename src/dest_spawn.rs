use std::path::Path;
use std::process::{Command, Stdio};

use crate::dest_config::CmdSpec;

pub enum Interpolate {
    Ok(String),
    Skip,
}

pub fn interpolate(template: &str, cwd: &str, lane: &str, run_id: &str, path: &str) -> Interpolate {
    let mut skip = false;
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        rest = &rest[start + 2..];
        let Some(end) = rest.find("}}") else {
            return Interpolate::Skip;
        };
        let name = &rest[..end];
        rest = &rest[end + 2..];
        if skip {
            continue;
        }
        match resolve(name, cwd, lane, run_id, path) {
            Interpolate::Skip => skip = true,
            Interpolate::Ok(v) => out.push_str(&v),
        }
    }
    if skip {
        return Interpolate::Skip;
    }
    out.push_str(rest);
    if out.contains("{{") {
        return Interpolate::Skip;
    }
    Interpolate::Ok(out)
}

fn resolve(name: &str, cwd: &str, lane: &str, run_id: &str, path: &str) -> Interpolate {
    if let Some(key) = name.strip_prefix("env.") {
        if key.is_empty() {
            return Interpolate::Skip;
        }
        match std::env::var(key) {
            Ok(v) if !v.is_empty() => Interpolate::Ok(v),
            _ => Interpolate::Skip,
        }
    } else if name == "cwd" {
        Interpolate::Ok(cwd.into())
    } else if name == "lane" {
        Interpolate::Ok(lane.into())
    } else if name == "run-id" {
        if run_id.is_empty() {
            Interpolate::Skip
        } else {
            Interpolate::Ok(run_id.into())
        }
    } else if name == "path" {
        if path.is_empty() {
            Interpolate::Skip
        } else {
            Interpolate::Ok(path.into())
        }
    } else {
        Interpolate::Skip
    }
}

pub fn tokenize(cmd: &str) -> Option<Vec<String>> {
    let mut argv = Vec::new();
    let chars: Vec<char> = cmd.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && is_space(chars[i]) {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let quote = chars[i];
        if quote == '"' || quote == '\'' {
            i += 1;
            let mut token = String::new();
            let mut closed = false;
            while i < chars.len() {
                if chars[i] == quote {
                    closed = true;
                    i += 1;
                    break;
                }
                token.push(chars[i]);
                i += 1;
            }
            if !closed {
                return None;
            }
            argv.push(token);
            continue;
        }
        let mut token = String::new();
        while i < chars.len() && !is_space(chars[i]) {
            token.push(chars[i]);
            i += 1;
        }
        argv.push(token);
    }
    Some(argv)
}

fn is_space(ch: char) -> bool {
    ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r'
}

pub fn render_argv(
    spec: &CmdSpec,
    cwd: &str,
    lane: &str,
    run_id: &str,
    path: &str,
) -> Option<Vec<String>> {
    match spec {
        CmdSpec::String(s) => {
            let Interpolate::Ok(v) = interpolate(s, cwd, lane, run_id, path) else {
                return None;
            };
            tokenize(&v)
        }
        CmdSpec::List(parts) => {
            let mut argv = Vec::new();
            for part in parts {
                let Interpolate::Ok(v) = interpolate(part, cwd, lane, run_id, path) else {
                    return None;
                };
                argv.push(v);
            }
            Some(argv)
        }
    }
}

pub fn spawn_argv(cwd: &Path, argv: &[String]) -> Result<i32, String> {
    if argv.is_empty() {
        return Err("empty argv".into());
    }
    let status = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| e.to_string())?;
    Ok(status.code().unwrap_or(1))
}

pub fn spawn_cmds(
    cwd: &Path,
    lane: &str,
    run_id: &str,
    path: &str,
    cmds: &[CmdSpec],
) -> Result<i32, String> {
    let cwd_s = cwd.to_string_lossy();
    for cmd in cmds {
        let Some(argv) = render_argv(cmd, &cwd_s, lane, run_id, path) else {
            continue;
        };
        let code = spawn_argv(cwd, &argv)?;
        if code != 0 {
            return Ok(code);
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_string_and_quotes() {
        assert_eq!(
            tokenize("echo hi").unwrap(),
            vec!["echo".to_string(), "hi".into()]
        );
        assert_eq!(tokenize("echo 'a b'").unwrap(), vec!["echo".to_string(), "a b".into()]);
        assert!(tokenize("echo 'oops").is_none());
    }

    #[test]
    fn interpolate_skips_empty_env_and_leftover() {
        assert!(matches!(
            interpolate("{{env.BEE_MISSING}}", "/", "l", "r", "p"),
            Interpolate::Skip
        ));
        assert!(matches!(interpolate("hello {{", "/", "l", "r", "p"), Interpolate::Skip));
    }

    #[test]
    fn list_cmd_keeps_spaces() {
        let spec = CmdSpec::List(vec!["true".into(), "a b".into()]);
        let argv = render_argv(&spec, "/", "l", "r", "p").unwrap();
        assert_eq!(argv, vec!["true", "a b"]);
    }

    #[test]
    fn no_shell_on_metacharacters() {
        let spec = CmdSpec::List(vec!["true".into(), "x; rm -rf /".into()]);
        let argv = render_argv(&spec, "/", "l", "r", "p").unwrap();
        assert_eq!(argv[1], "x; rm -rf /");
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(spawn_argv(dir.path(), &argv).unwrap(), 0);
    }
}
