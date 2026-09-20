use std::fs;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

use crate::cli::DocsCmd;
use crate::docs::resolve_note;

pub fn run(vault: &Path, cmd: &DocsCmd) -> Result<String, String> {
    match cmd {
        DocsCmd::Write {
            path,
            content,
            content_file,
            if_absent,
        } => {
            let body = take_content(content.as_deref(), content_file.as_deref())?;
            mutate(write_note(vault, path, &body, *if_absent))
        }
        DocsCmd::Append {
            path,
            content,
            content_file,
            if_missing,
        } => {
            let body = take_content(content.as_deref(), content_file.as_deref())?;
            mutate(append_note(vault, path, &body, *if_missing))
        }
        DocsCmd::Patch {
            path,
            target_type,
            target,
            op,
            content,
            content_file,
        } => patch(
            vault,
            path,
            target_type.as_deref(),
            target,
            op.as_deref(),
            content.as_deref(),
            content_file.as_deref(),
        ),
        DocsCmd::Rm { path, permanent } => mutate(rm_note(vault, path, *permanent)),
        DocsCmd::Mv {
            from,
            to,
            overwrite,
        } => mutate(mv_note(vault, from, to, *overwrite)),
        _ => Err("internal".into()),
    }
}

fn mutate(rel: Result<String, String>) -> Result<String, String> {
    Ok(format!("{}\n", rel?))
}

fn take_content(content: Option<&str>, content_file: Option<&str>) -> Result<String, String> {
    match (content, content_file) {
        (Some(_), Some(_)) => Err("use --content or --content-file, not both".into()),
        (Some(c), None) => Ok(c.to_string()),
        (None, Some(path)) => {
            if !Path::new(path).is_file() {
                return Err(format!("content file not found: {path}"));
            }
            fs::read_to_string(path).map_err(|_| format!("content file not found: {path}"))
        }
        (None, None) => read_stdin(),
    }
}

fn read_stdin() -> Result<String, String> {
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err("missing --content".into());
    }
    let mut s = String::new();
    stdin.read_to_string(&mut s).map_err(|e| e.to_string())?;
    Ok(s)
}

fn write_note(vault: &Path, path: &str, content: &str, if_absent: bool) -> Result<String, String> {
    let (abs, rel) = resolve_note(vault, path)?;
    if if_absent && abs.exists() {
        return Ok(rel);
    }
    atomic_write(&abs, content)?;
    Ok(rel)
}

fn append_note(
    vault: &Path,
    path: &str,
    content: &str,
    if_missing: bool,
) -> Result<String, String> {
    let (abs, rel) = resolve_note(vault, path)?;
    if !abs.exists() {
        let body = if content.ends_with('\n') {
            content.to_string()
        } else {
            format!("{content}\n")
        };
        atomic_write(&abs, &body)?;
        return Ok(rel);
    }
    let prev = fs::read_to_string(&abs).map_err(|e| e.to_string())?;
    if !if_missing && prev.ends_with(content) {
        return Ok(rel);
    }
    let body = if prev.ends_with('\n') || prev.is_empty() {
        prev
    } else {
        format!("{prev}\n")
    };
    let next = format!(
        "{body}{content}{}",
        if content.ends_with('\n') { "" } else { "\n" }
    );
    atomic_write(&abs, &next)?;
    Ok(rel)
}

fn patch(
    vault: &Path,
    path: &str,
    target_type: Option<&str>,
    target: &[String],
    op: Option<&str>,
    content: Option<&str>,
    content_file: Option<&str>,
) -> Result<String, String> {
    let target_type = target_type.unwrap_or("");
    if target_type != "heading" && target_type != "frontmatter" && target_type != "block" {
        return Err("missing --target-type heading|frontmatter|block".into());
    }
    let joined = target.join("::");
    if joined.is_empty() {
        return Err("missing --target".into());
    }
    let op = op.unwrap_or("append");
    if op != "append" && op != "prepend" && op != "replace" && op != "delete" {
        return Err(format!("bad --op: {op}"));
    }
    let needs_content = target_type == "heading" && op != "delete";
    let body = if needs_content {
        take_content(content, content_file)?
    } else {
        String::new()
    };
    mutate(patch_note(vault, path, target_type, &joined, op, &body))
}

fn patch_note(
    vault: &Path,
    path: &str,
    target_type: &str,
    target: &str,
    op: &str,
    content: &str,
) -> Result<String, String> {
    let (abs, rel) = resolve_note(vault, path)?;
    if !abs.is_file() {
        return Err(format!("note not found: {rel}"));
    }
    if target_type == "block" {
        return Err("UNSUPPORTED: block patch".into());
    }
    let prev = fs::read_to_string(&abs).map_err(|e| e.to_string())?;
    let next = if target_type == "frontmatter" {
        patch_frontmatter(&prev, target, op)
    } else {
        patch_heading(&prev, target, op, content)
    };
    let Some(next) = next else {
        return Err(format!("target not found: {target}"));
    };
    if next != prev {
        atomic_write(&abs, &next)?;
    }
    Ok(rel)
}

fn rm_note(vault: &Path, path: &str, permanent: bool) -> Result<String, String> {
    let (abs, rel) = resolve_note(vault, path)?;
    if !abs.exists() {
        return Ok(rel);
    }
    if permanent {
        fs::remove_file(&abs).map_err(|e| e.to_string())?;
        return Ok(rel);
    }
    let trash = vault.join(".trash").join(&rel);
    if let Some(parent) = trash.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if trash.exists() {
        if trash.is_dir() {
            fs::remove_dir_all(&trash).map_err(|e| e.to_string())?;
        } else {
            fs::remove_file(&trash).map_err(|e| e.to_string())?;
        }
    }
    fs::rename(&abs, &trash).map_err(|e| e.to_string())?;
    Ok(rel)
}

fn mv_note(vault: &Path, from: &str, to: &str, overwrite: bool) -> Result<String, String> {
    let (src, src_rel) = resolve_note(vault, from)?;
    let (dest, rel) = resolve_note(vault, to)?;
    if src == dest {
        return Ok(rel);
    }
    if !src.exists() {
        return Err(format!("note not found: {src_rel}"));
    }
    if dest.exists() && !overwrite {
        return Err(format!("file exists: {rel}"));
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::rename(&src, &dest).map_err(|e| e.to_string())?;
    Ok(rel)
}

fn atomic_write(abs: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = PathBuf::from(format!("{}.tmp-{}", abs.display(), std::process::id()));
    fs::write(&tmp, content).map_err(|e| e.to_string())?;
    fs::rename(&tmp, abs).map_err(|e| e.to_string())
}

fn patch_frontmatter(text: &str, target: &str, op: &str) -> Option<String> {
    if !text.starts_with("---\n") {
        return None;
    }
    let end = text[4..].find("\n---").map(|i| i + 4)?;
    if op == "delete" {
        let key = target.split('=').next().unwrap_or(target);
        let yaml = text[4..end]
            .split('\n')
            .filter(|line| !line.starts_with(&format!("{key}:")))
            .collect::<Vec<_>>()
            .join("\n");
        return Some(format!("---\n{}\n---{}", yaml.trim_end(), &text[end + 4..]));
    }
    let eq = target.find('=')?;
    let key = &target[..eq];
    let value = &target[eq + 1..];
    let rhs = if needs_quote(value) {
        json_string(value)
    } else {
        value.to_string()
    };
    set_fields(text, key, &rhs)
}

fn needs_quote(value: &str) -> bool {
    value.is_empty()
        || value.chars().any(|c| {
            matches!(
                c,
                ':' | '#'
                    | '{'
                    | '}'
                    | '['
                    | ']'
                    | ','
                    | '&'
                    | '*'
                    | '!'
                    | '|'
                    | '>'
                    | '\''
                    | '"'
                    | '%'
                    | '@'
                    | '`'
            ) || c.is_whitespace()
        })
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn set_fields(text: &str, key: &str, value: &str) -> Option<String> {
    if !text.starts_with("---\n") {
        return None;
    }
    let end = text[4..].find("\n---").map(|i| i + 4)?;
    let yaml = &text[4..end];
    let rest = &text[end + 4..];
    let line = format!("{key}: {value}");
    let mut replaced = false;
    let mut out = String::new();
    for existing in yaml.split('\n') {
        if existing.starts_with(&format!("{key}:")) && !replaced {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&line);
            replaced = true;
        } else {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(existing);
        }
    }
    if !replaced {
        let mut yaml = yaml.trim_end().to_string();
        if !yaml.is_empty() {
            yaml.push('\n');
        }
        yaml.push_str(&line);
        yaml.push('\n');
        return Some(format!("---\n{}\n---{rest}", yaml.trim_end()));
    }
    Some(format!("---\n{}\n---{rest}", out.trim_end()))
}

fn patch_heading(text: &str, target: &str, op: &str, content: &str) -> Option<String> {
    let path: Vec<&str> = target
        .split("::")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if path.is_empty() {
        return None;
    }
    let nl = text.ends_with('\n');
    let body = if nl { &text[..text.len() - 1] } else { text };
    let mut lines: Vec<String> = body.split('\n').map(str::to_string).collect();
    let section = find_section(&lines, &path)?;
    if op == "delete" {
        lines.drain(section.head..section.subtree_to);
        return Some(join_lines(&lines, nl));
    }
    let chunk: Vec<String> = content.split('\n').map(str::to_string).collect();
    let at = if op == "replace" {
        lines.drain(section.body_from..section.body_to);
        section.body_from
    } else if op == "prepend" {
        section.body_from
    } else {
        section.body_to
    };
    for (i, line) in chunk.into_iter().enumerate() {
        lines.insert(at + i, line);
    }
    Some(join_lines(&lines, nl))
}

struct Section {
    head: usize,
    body_from: usize,
    body_to: usize,
    subtree_to: usize,
}

fn find_section(lines: &[String], path: &[&str]) -> Option<Section> {
    let mut from = 0usize;
    let mut to = lines.len();
    let mut min_level = 0usize;
    let mut found = None;
    for name in path {
        found = None;
        let mut i = from;
        while i < to {
            let Some((level, title)) = heading(&lines[i]) else {
                i += 1;
                continue;
            };
            if level <= min_level && i > from {
                break;
            }
            if title == *name && level > min_level {
                let body_from = i + 1;
                let mut body_to = body_from;
                let mut subtree_to = to;
                for (j, line) in lines.iter().enumerate().take(to).skip(body_from) {
                    let Some((nlevel, _)) = heading(line) else {
                        continue;
                    };
                    if nlevel <= level {
                        subtree_to = j;
                        break;
                    }
                    if nlevel > level && body_to == body_from {
                        body_to = j;
                    }
                }
                if body_to == body_from {
                    body_to = subtree_to;
                }
                found = Some(Section {
                    head: i,
                    body_from,
                    body_to,
                    subtree_to,
                });
                from = body_from;
                to = subtree_to;
                min_level = level;
                break;
            }
            i += 1;
        }
        found.as_ref()?;
    }
    found
}

fn heading(line: &str) -> Option<(usize, String)> {
    let line = line.trim_end_matches('\r');
    if !line.starts_with('#') {
        return None;
    }
    let bytes = line.as_bytes();
    let mut level = 0usize;
    while level < 6 && level < bytes.len() && bytes[level] == b'#' {
        level += 1;
    }
    if level == 0 || level >= bytes.len() || bytes[level] != b' ' {
        return None;
    }
    let rest = line[level + 1..].trim_end();
    if rest.is_empty() {
        return None;
    }
    Some((level, strip_trailing_hashes(rest).to_string()))
}

fn strip_trailing_hashes(s: &str) -> &str {
    let s = s.trim_end();
    let bytes = s.as_bytes();
    let mut i = bytes.len();
    while i > 0 && bytes[i - 1] == b'#' {
        i -= 1;
    }
    if i == bytes.len() {
        return s;
    }
    let before = &s[..i];
    let trimmed = before.trim_end();
    if trimmed.len() < before.len() && !trimmed.is_empty() {
        trimmed
    } else {
        s
    }
}

fn join_lines(lines: &[String], nl: bool) -> String {
    format!("{}{}", lines.join("\n"), if nl { "\n" } else { "" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Commands};
    use crate::docs;
    use crate::execute;
    use std::fs;
    use tempfile::tempdir;

    const DOC: &str = "---\nstatus: draft\ntitle: t\n---\n\n# t\n\n## Tasks\n- old\n### Nested\nstay\n\n## Other\nx\n";

    fn write_rel(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn run(root: &Path, vault: Option<&str>, cmd: DocsCmd) -> Result<String, String> {
        docs::run_with_env(root, vault, None, &cmd)
    }

    fn write_cmd(path: &str, content: &str, if_absent: bool) -> DocsCmd {
        DocsCmd::Write {
            path: path.into(),
            content: Some(content.into()),
            content_file: None,
            if_absent,
        }
    }

    #[test]
    fn write_and_if_absent() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/.keep", "");
        let out = run(dir.path(), None, write_cmd("a.md", "one\n", false)).unwrap();
        assert_eq!(out, "a.md\n");
        assert_eq!(
            fs::read_to_string(dir.path().join("docs/a.md")).unwrap(),
            "one\n"
        );
        let skip = run(dir.path(), None, write_cmd("a.md", "two\n", true)).unwrap();
        assert_eq!(skip, "a.md\n");
        assert_eq!(
            fs::read_to_string(dir.path().join("docs/a.md")).unwrap(),
            "one\n"
        );
    }

    #[test]
    fn append_is_idempotent() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/a.md", "hi\n");
        let cmd = |c: &str| DocsCmd::Append {
            path: "a.md".into(),
            content: Some(c.into()),
            content_file: None,
            if_missing: false,
        };
        assert_eq!(run(dir.path(), None, cmd("there\n")).unwrap(), "a.md\n");
        assert_eq!(run(dir.path(), None, cmd("there\n")).unwrap(), "a.md\n");
        assert_eq!(
            fs::read_to_string(dir.path().join("docs/a.md")).unwrap(),
            "hi\nthere\n"
        );
    }

    #[test]
    fn patch_heading_append_leaves_nested() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/a.md", DOC);
        let out = run(
            dir.path(),
            None,
            DocsCmd::Patch {
                path: "a.md".into(),
                target_type: Some("heading".into()),
                target: vec!["Tasks".into()],
                op: Some("append".into()),
                content: Some("- new".into()),
                content_file: None,
            },
        )
        .unwrap();
        assert_eq!(out, "a.md\n");
        let text = fs::read_to_string(dir.path().join("docs/a.md")).unwrap();
        assert!(text.contains("- old\n- new\n### Nested"), "{text}");
        assert!(text.contains("### Nested\nstay"), "{text}");
    }

    #[test]
    fn patch_heading_child_and_delete_subtree() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/a.md", DOC);
        run(
            dir.path(),
            None,
            DocsCmd::Patch {
                path: "a.md".into(),
                target_type: Some("heading".into()),
                target: vec!["Tasks".into(), "Nested".into()],
                op: Some("replace".into()),
                content: Some("gone".into()),
                content_file: None,
            },
        )
        .unwrap();
        let text = fs::read_to_string(dir.path().join("docs/a.md")).unwrap();
        assert!(text.contains("### Nested\ngone"), "{text}");
        run(
            dir.path(),
            None,
            DocsCmd::Patch {
                path: "a.md".into(),
                target_type: Some("heading".into()),
                target: vec!["Tasks".into()],
                op: Some("delete".into()),
                content: None,
                content_file: None,
            },
        )
        .unwrap();
        let text = fs::read_to_string(dir.path().join("docs/a.md")).unwrap();
        assert!(!text.contains("## Tasks"), "{text}");
        assert!(!text.contains("### Nested"), "{text}");
        assert!(text.contains("## Other"), "{text}");
    }

    #[test]
    fn patch_frontmatter_field() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/a.md", DOC);
        run(
            dir.path(),
            None,
            DocsCmd::Patch {
                path: "a.md".into(),
                target_type: Some("frontmatter".into()),
                target: vec!["status=done".into()],
                op: Some("replace".into()),
                content: None,
                content_file: None,
            },
        )
        .unwrap();
        let text = fs::read_to_string(dir.path().join("docs/a.md")).unwrap();
        assert!(text.contains("status: done"), "{text}");
    }

    #[test]
    fn rm_trash_then_missing_is_noop() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/guides/a.md", DOC);
        assert_eq!(
            run(
                dir.path(),
                None,
                DocsCmd::Rm {
                    path: "guides/a.md".into(),
                    permanent: false,
                }
            )
            .unwrap(),
            "guides/a.md\n"
        );
        assert!(!dir.path().join("docs/guides/a.md").exists());
        assert!(dir.path().join("docs/.trash/guides/a.md").is_file());
        assert_eq!(
            run(
                dir.path(),
                None,
                DocsCmd::Rm {
                    path: "guides/a.md".into(),
                    permanent: false,
                }
            )
            .unwrap(),
            "guides/a.md\n"
        );
    }

    #[test]
    fn mv_overwrite_and_same_path() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/a.md", "a\n");
        write_rel(dir.path(), "docs/b.md", "b\n");
        let same = run(
            dir.path(),
            None,
            DocsCmd::Mv {
                from: "a.md".into(),
                to: "a.md".into(),
                overwrite: false,
            },
        )
        .unwrap();
        assert_eq!(same, "a.md\n");
        let clash = run(
            dir.path(),
            None,
            DocsCmd::Mv {
                from: "a.md".into(),
                to: "b.md".into(),
                overwrite: false,
            },
        )
        .unwrap_err();
        assert!(clash.contains("file exists: b.md"), "{clash}");
        run(
            dir.path(),
            None,
            DocsCmd::Mv {
                from: "a.md".into(),
                to: "b.md".into(),
                overwrite: true,
            },
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("docs/b.md")).unwrap(),
            "a\n"
        );
    }

    #[test]
    fn refuse_obsidian_and_outside_vault() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/.keep", "");
        let hidden = run(dir.path(), None, write_cmd(".obsidian/app.md", "x", false)).unwrap_err();
        assert!(hidden.contains("refuse .obsidian"), "{hidden}");
        let outside = run(dir.path(), None, write_cmd("../escape.md", "x", false)).unwrap_err();
        assert!(outside.contains("path outside vault"), "{outside}");
    }

    #[test]
    fn default_store_write_walk_up() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/.keep", "");
        let nested = dir.path().join("packages/app");
        fs::create_dir_all(&nested).unwrap();
        let out = docs::run_with_env(
            &nested,
            None,
            None,
            &write_cmd("guides/a.md", "hi\n", false),
        )
        .unwrap();
        assert_eq!(out, "guides/a.md\n");
        assert_eq!(
            fs::read_to_string(dir.path().join("docs/guides/a.md")).unwrap(),
            "hi\n"
        );
    }

    #[test]
    fn vault_flag_write() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/skip.md", "docs store\n");
        write_rel(dir.path(), "other/.keep", "");
        let other = dir.path().join("other");
        let out = run(
            dir.path(),
            Some(other.to_str().unwrap()),
            write_cmd("hit.md", "secret store\n", false),
        )
        .unwrap();
        assert_eq!(out, "hit.md\n");
        assert_eq!(
            fs::read_to_string(other.join("hit.md")).unwrap(),
            "secret store\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("docs/skip.md")).unwrap(),
            "docs store\n"
        );
    }

    #[test]
    fn execute_write_with_vault_flag() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/.keep", "");
        let vault = dir.path().join("docs").to_string_lossy().into_owned();
        assert_eq!(
            execute(
                Cli {
                    project: None,
                    wt: vec![],
                    command: Commands::Docs {
                        vault: Some(vault),
                        cmd: write_cmd("a.md", "ok\n", false),
                    },
                },
                dir.path()
            ),
            0
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("docs/a.md")).unwrap(),
            "ok\n"
        );
    }

    #[test]
    fn content_file_and_conflict() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/.keep", "");
        let file = dir.path().join("body.txt");
        fs::write(&file, "from file\n").unwrap();
        let out = run(
            dir.path(),
            None,
            DocsCmd::Write {
                path: "a.md".into(),
                content: None,
                content_file: Some(file.to_string_lossy().into_owned()),
                if_absent: false,
            },
        )
        .unwrap();
        assert_eq!(out, "a.md\n");
        assert_eq!(
            fs::read_to_string(dir.path().join("docs/a.md")).unwrap(),
            "from file\n"
        );
        let err = run(
            dir.path(),
            None,
            DocsCmd::Write {
                path: "a.md".into(),
                content: Some("x".into()),
                content_file: Some(file.to_string_lossy().into_owned()),
                if_absent: false,
            },
        )
        .unwrap_err();
        assert!(
            err.contains("use --content or --content-file, not both"),
            "{err}"
        );
    }

    #[test]
    fn patch_block_unsupported() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/a.md", DOC);
        let err = run(
            dir.path(),
            None,
            DocsCmd::Patch {
                path: "a.md".into(),
                target_type: Some("block".into()),
                target: vec!["x".into()],
                op: None,
                content: None,
                content_file: None,
            },
        )
        .unwrap_err();
        assert!(err.contains("UNSUPPORTED: block patch"), "{err}");
    }
}
