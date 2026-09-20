use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use regex::{Regex, RegexBuilder};
use serde_yaml::Value;

use crate::cli::DocsCmd;
use crate::dest_note::{parse_front_matter, ParseFrontMatter, YamlMap};
use crate::note_write::iso_stamp;

const SKIP: &[&str] = &[".obsidian", ".trash", "99_scribble", ".git"];
const BODY_CAP: usize = 1000;
const HIT_CAP: usize = 160;

struct VaultFile {
    rel: String,
    abs: PathBuf,
    mtime: SystemTime,
    size: u64,
}

pub fn run(start: &Path, vault_arg: Option<&str>, cmd: &DocsCmd) -> Result<String, String> {
    let env = std::env::var("OBSIDIAN_VAULT").ok();
    run_with_env(start, vault_arg, env.as_deref(), cmd)
}

pub(crate) fn run_with_env(
    start: &Path,
    vault_arg: Option<&str>,
    env_vault: Option<&str>,
    cmd: &DocsCmd,
) -> Result<String, String> {
    let vault = find_vault_root(start, vault_arg, env_vault)
        .ok_or_else(|| "no docs vault found from cwd. Pass --vault.".to_string())?;
    match cmd {
        DocsCmd::Home => home(&vault),
        DocsCmd::Ls {
            dir,
            recursive,
            sort,
            limit,
            ext,
            fields,
        } => ls(
            &vault,
            dir.as_deref(),
            *recursive,
            sort,
            *limit,
            ext.as_deref(),
            fields.as_deref(),
        ),
        DocsCmd::Read {
            paths,
            full,
            metadata,
        } => read(&vault, paths, *full, *metadata),
        DocsCmd::Search {
            query,
            regex,
            case_sensitive,
            context,
            tag,
            path,
            frontmatter,
            modified_since,
            limit,
        } => search(
            &vault,
            &query.join(" "),
            *regex,
            *case_sensitive,
            *context,
            tag,
            path.as_deref(),
            frontmatter,
            modified_since.as_deref(),
            *limit,
        ),
        DocsCmd::Recent {
            days,
            limit,
            fields,
        } => recent(&vault, *days, *limit, fields.as_deref()),
        DocsCmd::Write { .. }
        | DocsCmd::Append { .. }
        | DocsCmd::Patch { .. }
        | DocsCmd::Rm { .. }
        | DocsCmd::Mv { .. } => crate::docs_write::run(&vault, cmd),
    }
}

fn find_vault_root(
    start: &Path,
    vault_arg: Option<&str>,
    env_vault: Option<&str>,
) -> Option<PathBuf> {
    if let Some(raw) = vault_arg.filter(|s| !s.is_empty()) {
        return locate(start, raw);
    }
    if let Some(raw) = env_vault.filter(|s| !s.is_empty()) {
        return locate(start, raw);
    }
    walk_named(start, "docs")
}

fn locate(start: &Path, raw: &str) -> Option<PathBuf> {
    let path = Path::new(raw);
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        start.join(path)
    };
    if abs.is_dir() {
        return Some(abs);
    }
    if path.is_absolute() {
        return None;
    }
    walk_named(start, raw)
}

fn walk_named(start: &Path, name: &str) -> Option<PathBuf> {
    let mut dir = if start.is_absolute() {
        start.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(start)
    };
    loop {
        let candidate = dir.join(name);
        if candidate.is_dir() {
            return Some(candidate);
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent.to_path_buf(),
            _ => return None,
        }
    }
}

fn list_notes(vault: &Path, ext: &str) -> Vec<VaultFile> {
    let mut out = Vec::new();
    visit(vault, vault, ext, &mut out);
    out
}

fn visit(vault: &Path, dir: &Path, ext: &str, out: &mut Vec<VaultFile>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if SKIP.contains(&name) {
            continue;
        }
        let full = entry.path();
        let Ok(st) = entry.metadata() else { continue };
        if st.is_dir() {
            visit(vault, &full, ext, out);
            continue;
        }
        if !ext.is_empty() && !name.ends_with(&format!(".{ext}")) {
            continue;
        }
        let rel = posix_rel(vault, &full);
        out.push(VaultFile {
            rel,
            abs: full,
            mtime: st.modified().unwrap_or(UNIX_EPOCH),
            size: st.len(),
        });
    }
}

fn posix_rel(vault: &Path, abs: &Path) -> String {
    abs.strip_prefix(vault)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

fn filter_dir(files: Vec<VaultFile>, dir: Option<&str>, recursive: bool) -> Vec<VaultFile> {
    let prefix = match dir {
        None | Some("") | Some(".") => String::new(),
        Some(d) => d.trim_end_matches('/').to_string(),
    };
    files
        .into_iter()
        .filter(|f| {
            if prefix.is_empty() {
                return recursive || !f.rel.contains('/');
            }
            let head = format!("{prefix}/");
            if !f.rel.starts_with(&head) {
                return false;
            }
            let rest = &f.rel[head.len()..];
            recursive || !rest.contains('/')
        })
        .collect()
}

fn sort_notes(mut files: Vec<VaultFile>, sort: &str, limit: usize) -> Vec<VaultFile> {
    match sort {
        "modified" => files.sort_by(|a, b| b.mtime.cmp(&a.mtime)),
        "size" => files.sort_by(|a, b| b.size.cmp(&a.size)),
        _ => files.sort_by(|a, b| a.rel.cmp(&b.rel)),
    }
    files.truncate(limit);
    files
}

fn ls_vault(
    vault: &Path,
    dir: Option<&str>,
    recursive: bool,
    sort: &str,
    limit: usize,
    ext: &str,
) -> Vec<VaultFile> {
    sort_notes(
        filter_dir(list_notes(vault, ext), dir, recursive),
        sort,
        limit,
    )
}

fn home(vault: &Path) -> Result<String, String> {
    let files = ls_vault(vault, None, true, "modified", 5, "md");
    let n = list_notes(vault, "md").len();
    let now = SystemTime::now();
    let mut lines = vec![format!("vault: {}", vault.display()), format!("notes: {n}")];
    lines.extend(
        files
            .iter()
            .map(|f| format!("{}\t{}", f.rel, ago(f.mtime, now))),
    );
    Ok(format!("{}\n", lines.join("\n")))
}

fn ls(
    vault: &Path,
    dir: Option<&str>,
    recursive: bool,
    sort: &str,
    limit: Option<usize>,
    ext: Option<&str>,
    fields: Option<&str>,
) -> Result<String, String> {
    if sort != "path" && sort != "modified" && sort != "size" {
        return Err(format!("bad --sort: {sort}"));
    }
    let files = ls_vault(
        vault,
        dir,
        recursive,
        sort,
        limit.unwrap_or(200),
        ext.unwrap_or("md"),
    );
    Ok(listed(&rows(&files, fields == Some("path"))))
}

fn recent(
    vault: &Path,
    days: Option<u64>,
    limit: Option<usize>,
    fields: Option<&str>,
) -> Result<String, String> {
    let now = SystemTime::now();
    let min = days.map(|d| {
        now.checked_sub(Duration::from_secs(d * 86400))
            .unwrap_or(UNIX_EPOCH)
    });
    let files = list_notes(vault, "md")
        .into_iter()
        .filter(|f| min.map(|m| f.mtime >= m).unwrap_or(true))
        .collect();
    let files = sort_notes(files, "modified", limit.unwrap_or(20));
    Ok(listed(&rows(&files, fields == Some("path"))))
}

fn rows(files: &[VaultFile], only_path: bool) -> Vec<String> {
    let now = SystemTime::now();
    files
        .iter()
        .map(|f| {
            if only_path {
                f.rel.clone()
            } else {
                format!(
                    "{}\t{}\t{}\t{}",
                    f.rel,
                    iso_stamp(f.mtime),
                    ago(f.mtime, now),
                    f.size
                )
            }
        })
        .collect()
}

fn listed(rows: &[String]) -> String {
    if rows.is_empty() {
        return "count: 0\n".into();
    }
    format!("{}\ncount: {}\n", rows.join("\n"), rows.len())
}

fn read(vault: &Path, paths: &[String], full: bool, metadata: bool) -> Result<String, String> {
    let mut chunks = Vec::new();
    for path in paths {
        let (abs, rel) = resolve_note(vault, path)?;
        let st = fs::metadata(&abs).map_err(|_| format!("note not found: {rel}"))?;
        if !st.is_file() {
            return Err(format!("note not found: {rel}"));
        }
        let text = fs::read_to_string(&abs).map_err(|_| format!("note not found: {rel}"))?;
        let (body, fields) = split_note(&text);
        if metadata {
            chunks.push(meta_block(&rel, st.len(), &fields));
            continue;
        }
        let truncated = !full && body.len() > BODY_CAP;
        let shown = if truncated {
            body[..BODY_CAP].to_string()
        } else {
            body
        };
        let body = if truncated {
            format!("{shown}\n[truncated; use --full]")
        } else {
            shown
        };
        chunks.push(if paths.len() > 1 {
            format!(">>> {rel}\n{body}")
        } else {
            body
        });
    }
    let text = chunks.join("\n");
    Ok(if text.ends_with('\n') {
        text
    } else {
        format!("{text}\n")
    })
}

fn meta_block(rel: &str, size: u64, fields: &YamlMap) -> String {
    let mut lines = vec![format!("path: {rel}")];
    if let Some(id) = field_display(fields, "id") {
        lines.push(format!("id: {id}"));
    }
    if let Some(title) = field_display(fields, "title") {
        lines.push(format!("title: {title}"));
    }
    if let Some(tags) = field_display(fields, "tags") {
        lines.push(format!("tags: {tags}"));
    }
    lines.push(format!("size: {size}"));
    lines.join("\n")
}

fn search(
    vault: &Path,
    query: &str,
    regex: bool,
    case_sensitive: bool,
    context: Option<usize>,
    tag: &[String],
    path: Option<&str>,
    frontmatter: &[String],
    modified_since: Option<&str>,
    limit: Option<usize>,
) -> Result<String, String> {
    if query.is_empty()
        && tag.is_empty()
        && path.is_none()
        && frontmatter.is_empty()
        && modified_since.is_none()
    {
        return Err("missing search query".into());
    }
    let re = compile_query(query, regex, case_sensitive).map_err(|_| "bad --regex".to_string())?;
    let since = parse_since(modified_since, SystemTime::now());
    let limit = limit.unwrap_or(50);
    let ctx = context.unwrap_or(0);
    let mut hits = Vec::new();
    for file in list_notes(vault, "md") {
        if hits.len() >= limit {
            break;
        }
        if let Some(pat) = path.filter(|p| !p.is_empty()) {
            if !glob_match(&file.rel, pat) {
                continue;
            }
        }
        if let Some(min) = since {
            if file.mtime < min {
                continue;
            }
        }
        let Ok(text) = fs::read_to_string(&file.abs) else {
            continue;
        };
        if !tag_ok(&text, tag) || !fm_ok(&text, frontmatter) {
            continue;
        }
        let lines: Vec<&str> = text.split('\n').map(|l| l.trim_end_matches('\r')).collect();
        if re.is_none() {
            hits.push(format!(
                "{}:1:{}",
                file.rel,
                cap(lines.first().copied().unwrap_or(""), HIT_CAP)
            ));
            continue;
        }
        let re = re.as_ref().unwrap();
        for (i, line) in lines.iter().enumerate() {
            if hits.len() >= limit {
                break;
            }
            if !re.is_match(line) {
                continue;
            }
            hits.push(format!("{}:{}:{}", file.rel, i + 1, cap(line, HIT_CAP)));
            if ctx > 0 {
                let from = i.saturating_sub(ctx);
                let to = (i + 1 + ctx).min(lines.len());
                for extra in &lines[from..to] {
                    hits.push(format!("  {extra}"));
                }
            }
        }
    }
    Ok(listed(&hits))
}

fn compile_query(query: &str, regex: bool, case_sensitive: bool) -> Result<Option<Regex>, ()> {
    if query.is_empty() {
        return Ok(None);
    }
    let pattern = if regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!case_sensitive)
        .build()
        .map(Some)
        .map_err(|_| ())
}

fn glob_match(rel: &str, pattern: &str) -> bool {
    let normalized = pattern.replace('\\', "/");
    if !normalized.contains(['*', '?']) {
        let prefix = normalized.trim_end_matches('/');
        return rel == prefix || rel.starts_with(&format!("{prefix}/"));
    }
    let escaped = regex::escape(&normalized)
        .replace("\\*\\*", "\0")
        .replace("\\*", "[^/]*")
        .replace("\\?", ".")
        .replace('\0', ".*");
    Regex::new(&format!("^{escaped}$"))
        .map(|re| re.is_match(rel))
        .unwrap_or(false)
}

fn parse_since(spec: Option<&str>, now: SystemTime) -> Option<SystemTime> {
    let spec = spec.filter(|s| !s.is_empty())?;
    if Regex::new(r"^\d{4}-\d{2}-\d{2}$").ok()?.is_match(spec) {
        return datetime_utc(spec);
    }
    let cap = Regex::new(r"^(\d+)(h|d|w|mo|y)$").ok()?.captures(spec)?;
    let n: u64 = cap.get(1)?.as_str().parse().ok()?;
    let ms = match cap.get(2)?.as_str() {
        "h" => n * 3600,
        "d" => n * 86400,
        "w" => n * 7 * 86400,
        "mo" => n * 30 * 86400,
        _ => n * 365 * 86400,
    };
    now.checked_sub(Duration::from_secs(ms))
}

fn datetime_utc(ymd: &str) -> Option<SystemTime> {
    let mut parts = ymd.split('-');
    let y: i32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    let days = days_from_civil(y, m, d)? as i64;
    let unix = (days - 719468) * 86400;
    if unix >= 0 {
        Some(UNIX_EPOCH + Duration::from_secs(unix as u64))
    } else {
        UNIX_EPOCH.checked_sub(Duration::from_secs((-unix) as u64))
    }
}

fn days_from_civil(y: i32, m: u32, d: u32) -> Option<i32> {
    if !(1..=12).contains(&m) || d == 0 || d > 31 {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe as i32 - 719468 + 719468)
}

fn tag_ok(text: &str, tags: &[String]) -> bool {
    if tags.is_empty() {
        return true;
    }
    let have = note_tags(text);
    tags.iter().all(|t| {
        have.iter()
            .any(|h| h == t || h.starts_with(&format!("{t}/")))
    })
}

fn fm_ok(text: &str, filters: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    let fields = match parse_front_matter(text) {
        ParseFrontMatter::Ok(map) => map,
        ParseFrontMatter::Fault(_) => return false,
    };
    filters.iter().all(|f| {
        let Some((key, want)) = f.split_once('=') else {
            return false;
        };
        field_string(&fields, key).as_deref() == Some(want)
            || field_list(&fields, key).iter().any(|v| v == want)
    })
}

fn note_tags(text: &str) -> Vec<String> {
    let mut have = match parse_front_matter(text) {
        ParseFrontMatter::Ok(map) => field_list(&map, "tags"),
        ParseFrontMatter::Fault(_) => Vec::new(),
    };
    have.extend(hash_tags(text));
    have.sort();
    have.dedup();
    have
}

fn hash_tags(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'#' && (i == 0 || matches!(b[i - 1], b' ' | b'\t' | b'\n' | b'\r' | b'(')) {
            let start = i + 1;
            if start < b.len() && b[start].is_ascii_alphabetic() {
                let mut j = start + 1;
                while j < b.len()
                    && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'_' | b'/' | b'-'))
                {
                    j += 1;
                }
                out.push(text[start..j].to_string());
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn split_note(text: &str) -> (String, YamlMap) {
    let fields = match parse_front_matter(text) {
        ParseFrontMatter::Ok(map) => map,
        ParseFrontMatter::Fault(_) => YamlMap::new(),
    };
    let body = if let Some(rest) = text
        .strip_prefix("---\n")
        .and_then(|t| t.find("\n---").map(|end| t[end + 4..].to_string()))
    {
        rest
    } else {
        text.to_string()
    };
    (body, fields)
}

fn field_display(fields: &YamlMap, key: &str) -> Option<String> {
    let list = field_list(fields, key);
    if list.len() > 1
        || (list.len() == 1
            && fields
                .get(Value::String(key.into()))
                .map(|v| v.is_sequence())
                .unwrap_or(false))
    {
        return Some(list.join(", "));
    }
    field_string(fields, key).filter(|s| !s.is_empty())
}

fn field_string(fields: &YamlMap, key: &str) -> Option<String> {
    match fields.get(Value::String(key.into())) {
        Some(Value::String(s)) if !s.is_empty() => Some(unquote(s)),
        Some(Value::Bool(b)) => Some(b.to_string()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

fn field_list(fields: &YamlMap, key: &str) -> Vec<String> {
    match fields.get(Value::String(key.into())) {
        Some(Value::Sequence(seq)) => seq.iter().filter_map(|v| v.as_str().map(unquote)).collect(),
        Some(Value::String(s)) if !s.is_empty() => vec![unquote(s)],
        _ => Vec::new(),
    }
}

fn unquote(value: &str) -> String {
    let text = value.trim();
    let b = text.as_bytes();
    if text.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[0] == b[text.len() - 1] {
        text[1..text.len() - 1].to_string()
    } else {
        text.to_string()
    }
}

pub(crate) fn resolve_note(vault: &Path, path: &str) -> Result<(PathBuf, String), String> {
    let mut raw = path.trim().to_string();
    if raw.starts_with("[[") && raw.ends_with("]]") {
        raw = raw[2..raw.len() - 2]
            .split('|')
            .next()
            .unwrap_or("")
            .split('#')
            .next()
            .unwrap_or("")
            .to_string();
    }
    if !raw.ends_with(".md") && !raw.contains('.') {
        raw.push_str(".md");
    }
    let joined = if Path::new(&raw).is_absolute() {
        PathBuf::from(&raw)
    } else {
        vault.join(&raw)
    };
    let abs = normalize(&joined);
    let rel = match abs.strip_prefix(vault) {
        Ok(p) => p.to_string_lossy().replace('\\', "/"),
        Err(_) => return Err(format!("path outside vault: {path}")),
    };
    if rel.starts_with("..") || Path::new(&rel).is_absolute() {
        return Err(format!("path outside vault: {path}"));
    }
    if rel == ".obsidian" || rel.starts_with(".obsidian/") {
        return Err(format!("refuse .obsidian: {path}"));
    }
    Ok((abs, rel))
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

fn cap(text: &str, n: usize) -> String {
    if text.len() <= n {
        text.to_string()
    } else {
        text[..n].to_string()
    }
}

fn ago(mtime: SystemTime, now: SystemTime) -> String {
    let s = now.duration_since(mtime).unwrap_or_default().as_secs();
    if s < 60 {
        return format!("{s}s");
    }
    let m = (s + 30) / 60;
    if m < 60 {
        return format!("{m}m");
    }
    let h = (m + 30) / 60;
    if h < 48 {
        return format!("{h}h");
    }
    format!("{}d", (h + 12) / 24)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Commands};
    use crate::execute;
    use std::fs;
    use tempfile::tempdir;

    const NOTE: &str = "---\nid: \"guides-a\"\ntitle: \"A\"\nkind: guide\ntags: [stack]\n---\n\n# A\n\nHello heio.\n";

    fn write_rel(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn docs(root: &Path, vault: Option<&str>, cmd: DocsCmd) -> Result<String, String> {
        run_with_env(root, vault, None, &cmd)
    }

    fn cli(vault: Option<String>, cmd: DocsCmd) -> Cli {
        Cli {
            project: None,
            wt: vec![],
            command: Commands::Docs { vault, cmd },
        }
    }

    #[test]
    fn default_store_is_docs_walk_up() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/guides/a.md", NOTE);
        let nested = dir.path().join("packages/app");
        fs::create_dir_all(&nested).unwrap();
        let out = docs(
            &nested,
            None,
            DocsCmd::Ls {
                dir: None,
                recursive: true,
                sort: "path".into(),
                limit: None,
                ext: None,
                fields: Some("path".into()),
            },
        )
        .unwrap();
        assert!(out.contains("guides/a.md"), "{out}");
        assert!(out.contains("count: 1"), "{out}");
    }

    #[test]
    fn vault_flag_selects_another_store() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/skip.md", "docs store\n");
        write_rel(dir.path(), "other/hit.md", "---\n---\n\nsecret store\n");
        let out = docs(
            dir.path(),
            Some(dir.path().join("other").to_str().unwrap()),
            DocsCmd::Search {
                query: vec!["secret".into()],
                regex: false,
                case_sensitive: false,
                context: None,
                tag: vec![],
                path: None,
                frontmatter: vec![],
                modified_since: None,
                limit: None,
            },
        )
        .unwrap();
        assert!(out.contains("hit.md"), "{out}");
        assert!(!out.contains("skip.md"), "{out}");
    }

    #[test]
    fn search_finds_body_text() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/guides/a.md", NOTE);
        write_rel(
            dir.path(),
            "docs/guides/b.md",
            "---\ntags: [other]\n---\n\nnope\n",
        );
        let out = docs(
            dir.path(),
            None,
            DocsCmd::Search {
                query: vec!["Hello".into(), "heio".into()],
                regex: false,
                case_sensitive: false,
                context: None,
                tag: vec!["stack".into()],
                path: None,
                frontmatter: vec![],
                modified_since: None,
                limit: None,
            },
        )
        .unwrap();
        assert!(out.contains("guides/a.md:"), "{out}");
        assert!(out.contains("Hello heio"), "{out}");
        assert!(out.contains("count: 1"), "{out}");
        assert!(!out.contains("guides/b.md"), "{out}");
    }

    #[test]
    fn read_home_ls_recent_round_trip() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/guides/a.md", NOTE);
        let home = docs(dir.path(), None, DocsCmd::Home).unwrap();
        assert!(home.contains("notes: 1"), "{home}");
        assert!(home.contains("guides/a.md"), "{home}");
        let ls = docs(
            dir.path(),
            None,
            DocsCmd::Ls {
                dir: None,
                recursive: true,
                sort: "path".into(),
                limit: None,
                ext: None,
                fields: Some("path".into()),
            },
        )
        .unwrap();
        assert!(ls.contains("guides/a.md"), "{ls}");
        let read = docs(
            dir.path(),
            None,
            DocsCmd::Read {
                paths: vec!["guides/a.md".into()],
                full: false,
                metadata: false,
            },
        )
        .unwrap();
        assert!(read.contains("Hello heio."), "{read}");
        let recent = docs(
            dir.path(),
            None,
            DocsCmd::Recent {
                days: None,
                limit: None,
                fields: Some("path".into()),
            },
        )
        .unwrap();
        assert!(recent.contains("guides/a.md"), "{recent}");
    }

    #[test]
    fn missing_vault_fails_closed() {
        let dir = tempdir().unwrap();
        let err = docs(dir.path(), None, DocsCmd::Home).unwrap_err();
        assert!(err.contains("Pass --vault"), "{err}");
    }

    #[test]
    fn execute_search_with_vault_flag() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/guides/a.md", NOTE);
        let vault = dir.path().join("docs").to_string_lossy().into_owned();
        assert_eq!(
            execute(
                cli(
                    Some(vault),
                    DocsCmd::Search {
                        query: vec!["Hello".into()],
                        regex: false,
                        case_sensitive: false,
                        context: None,
                        tag: vec![],
                        path: None,
                        frontmatter: vec![],
                        modified_since: None,
                        limit: None,
                    }
                ),
                dir.path()
            ),
            0
        );
    }

    #[test]
    fn list_skips_scribble() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/guides/a.md", NOTE);
        write_rel(dir.path(), "docs/99_scribble/no.md", NOTE);
        let out = docs(
            dir.path(),
            None,
            DocsCmd::Ls {
                dir: None,
                recursive: true,
                sort: "path".into(),
                limit: None,
                ext: None,
                fields: Some("path".into()),
            },
        )
        .unwrap();
        assert!(out.contains("guides/a.md"), "{out}");
        assert!(!out.contains("scribble"), "{out}");
    }

    #[test]
    fn glob_prefix_and_star() {
        assert!(glob_match("guides/a.md", "guides"));
        assert!(glob_match("guides/a.md", "guides/**"));
        assert!(!glob_match("architecture/a.md", "guides/**"));
    }
}
