use std::path::{Path, PathBuf};

use odm_core::OdmError;
use regex::Regex;
use serde_json::Value;

/// Stable note identity within one store.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NoteId(pub String);

impl NoteId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One Markdown note in a vault.
#[derive(Debug, Clone)]
pub struct NoteDoc {
    pub id: NoteId,
    pub path: PathBuf,
    /// Path relative to vault root, forward slashes.
    pub rel_path: String,
    pub title: Option<String>,
    pub body: String,
    pub frontmatter: Option<Value>,
    pub wikilinks: Vec<String>,
}

/// Parse YAML frontmatter + body from Markdown text.
/// Invalid YAML between `---` markers is a hard error (includes `rel_path`).
pub fn parse_markdown(rel_path: &str, abs: &Path, text: &str) -> Result<NoteDoc, OdmError> {
    let (fm_raw, body) = split_frontmatter(text);
    let frontmatter = match fm_raw {
        None => None,
        Some(y) => match serde_yaml::from_str::<Value>(y) {
            Ok(v) => Some(v),
            Err(e) => {
                return Err(OdmError::operation(format!(
                    "invalid frontmatter in {rel_path}: {e}"
                )));
            }
        },
    };
    let id = id_from_frontmatter(&frontmatter).unwrap_or_else(|| path_id(rel_path));
    let title = title_from(&frontmatter, &body, rel_path);
    let wikilinks = parse_wikilinks(&body);
    Ok(NoteDoc {
        id: NoteId(id),
        path: abs.to_path_buf(),
        rel_path: rel_path.to_string(),
        title,
        body,
        frontmatter,
        wikilinks,
    })
}

/// Extract `[[target]]` and `[[target|alias]]` targets (Obsidian-style).
/// Fenced code blocks and inline `` `spans` `` are skipped.
pub fn parse_wikilinks(body: &str) -> Vec<String> {
    let scan = strip_code_for_wikilinks(body);
    let re = Regex::new(r"\[\[([^\]|#]+)(?:[|#][^\]]*)?\]\]").expect("wikilink regex");
    let mut out = Vec::new();
    for cap in re.captures_iter(&scan) {
        let t = cap[1].trim();
        if !t.is_empty() && !out.iter().any(|x: &String| x == t) {
            out.push(t.to_string());
        }
    }
    out
}

/// Drop fenced ```/~~~ blocks and inline `code` so they cannot yield wikilinks.
fn strip_code_for_wikilinks(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut in_fence = false;
    for line in body.split_inclusive('\n') {
        let core = line.trim_end_matches(['\n', '\r']);
        let trimmed = core.trim_start();
        if !in_fence {
            if is_fence_line(trimmed) {
                in_fence = true;
                continue;
            }
            out.push_str(&strip_inline_code(core));
            if line.ends_with('\n') {
                out.push('\n');
            }
        } else if is_fence_line(trimmed) {
            in_fence = false;
        }
    }
    out
}

fn is_fence_line(trimmed: &str) -> bool {
    let b = trimmed.as_bytes();
    if b.len() < 3 {
        return false;
    }
    let ch = b[0];
    if ch != b'`' && ch != b'~' {
        return false;
    }
    let n = b.iter().take_while(|&&c| c == ch).count();
    n >= 3
}

fn strip_inline_code(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find('`') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let open = rest.chars().take_while(|c| *c == '`').count();
        rest = &rest[open..];
        let needle = "`".repeat(open);
        if let Some(end) = rest.find(&needle) {
            rest = &rest[end + open..];
            out.push(' ');
        } else {
            break;
        }
    }
    out.push_str(rest);
    out
}

fn split_frontmatter(text: &str) -> (Option<&str>, String) {
    let t = text;
    if !t.starts_with("---") {
        return (None, t.to_string());
    }
    let rest = &t[3..];
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    if let Some(end) = rest.find("\n---") {
        let yaml = &rest[..end];
        let after = &rest[end + 4..];
        let body = after.strip_prefix('\n').unwrap_or(after).to_string();
        return (Some(yaml), body);
    }
    (None, t.to_string())
}

fn id_from_frontmatter(fm: &Option<Value>) -> Option<String> {
    let obj = fm.as_ref()?.as_object()?;
    if let Some(v) = obj.get("id") {
        if let Some(s) = v.as_str() {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn title_from(fm: &Option<Value>, body: &str, rel: &str) -> Option<String> {
    if let Some(obj) = fm.as_ref().and_then(|v| v.as_object()) {
        if let Some(s) = obj.get("title").and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    for line in body.lines() {
        let t = line.trim();
        if let Some(h) = t.strip_prefix("# ") {
            let h = h.trim();
            if !h.is_empty() {
                return Some(h.to_string());
            }
        }
    }
    Path::new(rel)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
}

fn path_id(rel: &str) -> String {
    let no_ext = rel.strip_suffix(".md").unwrap_or(rel);
    no_ext.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn frontmatter_id_and_wikilinks() {
        let text = r#"---
id: n1
title: Hello
---
See [[Other Note]] and [[foo|bar]].
"#;
        let n = parse_markdown("a/b.md", &PathBuf::from("/x/a/b.md"), text).unwrap();
        assert_eq!(n.id.as_str(), "n1");
        assert_eq!(n.title.as_deref(), Some("Hello"));
        assert_eq!(n.wikilinks, vec!["Other Note", "foo"]);
    }

    #[test]
    fn path_id_fallback() {
        let n = parse_markdown("notes/hi.md", &PathBuf::from("/v/notes/hi.md"), "hi\n").unwrap();
        assert_eq!(n.id.as_str(), "notes/hi");
    }

    #[test]
    fn wikilinks_skip_fenced_code() {
        let body = r#"Real [[KeepMe]].

```rust
let x = [[NotALink]];
```

After [[AlsoKeep]].
"#;
        assert_eq!(
            parse_wikilinks(body),
            vec!["KeepMe".to_string(), "AlsoKeep".to_string()]
        );
    }

    #[test]
    fn wikilinks_skip_tilde_fences_and_inline() {
        let body = "Before [[A]]\n~~~\n[[FenceOnly]]\n~~~\nAnd `[[InlineOnly]]` plus [[B]].\n";
        assert_eq!(parse_wikilinks(body), vec!["A".to_string(), "B".to_string()]);
    }

    #[test]
    fn invalid_frontmatter_errors_with_path() {
        let text = r#"---
id: [broken
title: x
---
Body [[Link]].
"#;
        let err = parse_markdown("notes/bad.md", &PathBuf::from("/v/notes/bad.md"), text)
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("notes/bad.md") && msg.to_lowercase().contains("frontmatter"),
            "expected path + frontmatter in error, got: {msg}"
        );
    }

    #[test]
    fn happy_frontmatter_and_body_link() {
        let text = r#"---
id: ok1
---
See [[Target]].

```
[[No]]
```
"#;
        let n = parse_markdown("ok.md", &PathBuf::from("/v/ok.md"), text).unwrap();
        assert_eq!(n.id.as_str(), "ok1");
        assert_eq!(n.wikilinks, vec!["Target"]);
    }

    #[test]
    fn empty_frontmatter_ok() {
        let text = "---\n---\nBody [[X]].\n";
        let n = parse_markdown("e.md", &PathBuf::from("/v/e.md"), text).unwrap();
        assert_eq!(n.wikilinks, vec!["X"]);
    }

    #[test]
    fn title_from_heading_fallback() {
        let text = "Intro\n\n# Heading Title\n\nMore.\n";
        let n = parse_markdown("notes/x.md", &PathBuf::from("/v/notes/x.md"), text).unwrap();
        assert_eq!(n.title.as_deref(), Some("Heading Title"));
        assert_eq!(n.id.as_str(), "notes/x");
    }

    #[test]
    fn title_from_path_stem_when_no_heading() {
        let n = parse_markdown(
            "folder/My Note.md",
            &PathBuf::from("/v/folder/My Note.md"),
            "plain body no heading\n",
        )
        .unwrap();
        assert_eq!(n.title.as_deref(), Some("My Note"));
    }

    #[test]
    fn wikilink_strips_header_and_alias() {
        let body = "See [[Target#Section]] and [[Other|display name]] and [[Target]].\n";
        assert_eq!(
            parse_wikilinks(body),
            vec!["Target".to_string(), "Other".to_string()]
        );
    }

    #[test]
    fn empty_id_in_frontmatter_falls_back_to_path() {
        let text = "---\nid: \"\"\ntitle: T\n---\nBody\n";
        let n = parse_markdown("p/q.md", &PathBuf::from("/v/p/q.md"), text).unwrap();
        assert_eq!(n.id.as_str(), "p/q");
        assert_eq!(n.title.as_deref(), Some("T"));
    }
}
