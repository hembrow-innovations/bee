use std::path::{Component, Path, PathBuf};

/// Normalize a git remote / config URL for equality comparison.
pub fn normalize_git_url(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return String::new();
    }

    // Bare filesystem path (no scheme, not SCP-like)
    if looks_like_bare_path(s) {
        return normalize_file_path(s);
    }

    let urlish = if let Some(rewritten) = scp_to_ssh(s) {
        rewritten
    } else {
        s.to_string()
    };

    normalize_urlish(&urlish)
}

/// Compare two git URLs after normalization.
///
/// When `workspace_root` is set, relative sides are resolved against it before compare
/// (for core-desk style relative fixture urls vs absolute `file://` origins).
pub fn urls_match(a: &str, b: &str) -> bool {
    urls_match_with_root(a, b, None)
}

pub fn urls_match_with_root(a: &str, b: &str, workspace_root: Option<&Path>) -> bool {
    let na = normalize_for_compare(a, workspace_root);
    let nb = normalize_for_compare(b, workspace_root);
    na == nb
}

fn normalize_for_compare(raw: &str, workspace_root: Option<&Path>) -> String {
    let trimmed = raw.trim();
    if let Some(root) = workspace_root {
        if is_relative_url(trimmed) {
            let resolved = root.join(trimmed.trim_start_matches("./"));
            if let Ok(canon) = resolved.canonicalize() {
                return normalize_git_url(&canon.to_string_lossy());
            }
            return normalize_git_url(&resolved.to_string_lossy());
        }
    }
    normalize_git_url(trimmed)
}

fn is_relative_url(s: &str) -> bool {
    if s.contains("://") {
        return false;
    }
    if scp_to_ssh(s).is_some() {
        return false;
    }
    !Path::new(s).is_absolute()
}

fn looks_like_bare_path(s: &str) -> bool {
    if s.contains("://") {
        return false;
    }
    if scp_to_ssh(s).is_some() {
        return false;
    }
    Path::new(s).is_absolute()
        || s.starts_with("./")
        || s.starts_with("../")
        || (!s.contains(':') && (s.contains('/') || s.contains('\\')))
}

/// `git@host:path` → `ssh://git@host/path`
fn scp_to_ssh(s: &str) -> Option<String> {
    if s.contains("://") {
        return None;
    }
    // user@host:path — colon after host, not Windows drive
    let colon = s.find(':')?;
    let (left, right) = s.split_at(colon);
    let path = &right[1..];
    if path.is_empty() || path.starts_with("//") {
        return None;
    }
    // Windows drive letter: C:\ or C:/
    if left.len() == 1 && left.chars().next()?.is_ascii_alphabetic() {
        return None;
    }
    if !left.contains('@') {
        // host:path without user is uncommon for SCP git; skip if looks like port
        return None;
    }
    Some(format!("ssh://{left}/{path}"))
}

fn normalize_urlish(s: &str) -> String {
    let (scheme, rest) = split_scheme(s);
    let scheme_l = scheme.to_ascii_lowercase();

    if scheme_l == "file" {
        return normalize_file_url(rest);
    }

    let (authority, path_and_more) = split_authority(rest);
    let (userinfo, hostport) = split_userinfo(authority);
    let (host, port) = split_host_port(hostport);
    let host_l = host.to_ascii_lowercase();

    let port = match port {
        Some(p) if is_default_port(&scheme_l, p) => None,
        other => other,
    };

    let mut path = path_and_more.to_string();
    // drop query/fragment for compare
    if let Some(i) = path.find(['?', '#']) {
        path.truncate(i);
    }
    path = strip_trailing_slashes(&path);
    path = strip_trailing_git(&path);

    let mut out = format!("{scheme_l}://");
    if !userinfo.is_empty() {
        out.push_str(userinfo);
        out.push('@');
    }
    out.push_str(&host_l);
    if let Some(p) = port {
        out.push(':');
        out.push_str(p);
    }
    if !path.is_empty() {
        if !path.starts_with('/') {
            out.push('/');
        }
        out.push_str(&path);
    }
    out
}

fn split_scheme(s: &str) -> (&str, &str) {
    if let Some(i) = s.find("://") {
        (&s[..i], &s[i + 3..])
    } else {
        ("", s)
    }
}

fn split_authority(rest: &str) -> (&str, &str) {
    if let Some(i) = rest.find('/') {
        (&rest[..i], &rest[i..])
    } else {
        (rest, "")
    }
}

fn split_userinfo(authority: &str) -> (&str, &str) {
    if let Some(i) = authority.rfind('@') {
        (&authority[..i], &authority[i + 1..])
    } else {
        ("", authority)
    }
}

fn split_host_port(hostport: &str) -> (&str, Option<&str>) {
    // IPv6 in brackets
    if hostport.starts_with('[') {
        if let Some(end) = hostport.find(']') {
            let host = &hostport[..=end];
            let rest = &hostport[end + 1..];
            if let Some(p) = rest.strip_prefix(':') {
                return (host, Some(p));
            }
            return (host, None);
        }
    }
    if let Some(i) = hostport.rfind(':') {
        // avoid treating bare IPv6 without brackets wrongly — only split if port is digits
        let maybe_port = &hostport[i + 1..];
        if !maybe_port.is_empty() && maybe_port.chars().all(|c| c.is_ascii_digit()) {
            return (&hostport[..i], Some(maybe_port));
        }
    }
    (hostport, None)
}

fn is_default_port(scheme: &str, port: &str) -> bool {
    matches!(
        (scheme, port),
        ("https", "443") | ("http", "80") | ("ssh", "22")
    )
}

fn strip_trailing_slashes(path: &str) -> String {
    let mut p = path.to_string();
    while p.len() > 1 && p.ends_with('/') {
        p.pop();
    }
    p
}

fn strip_trailing_git(path: &str) -> String {
    if let Some(stripped) = path.strip_suffix(".git") {
        stripped.to_string()
    } else {
        path.to_string()
    }
}

fn normalize_file_url(rest: &str) -> String {
    // file://localhost/path or file:///path or file://path
    let path_part = if let Some(stripped) = rest.strip_prefix("localhost") {
        stripped
    } else if rest.starts_with('/') {
        rest
    } else if rest.contains('/') {
        // host/path — rare for file
        if let Some(i) = rest.find('/') {
            &rest[i..]
        } else {
            rest
        }
    } else {
        rest
    };

    let normalized = normalize_file_path(path_part);
    if normalized.starts_with("file://") {
        normalized
    } else {
        format!("file://{normalized}")
    }
}

fn normalize_file_path(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    s = strip_trailing_git(&strip_trailing_slashes(&s));

    // lowercase Windows drive letter
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        if bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            let mut chars: Vec<char> = s.chars().collect();
            chars[0] = chars[0].to_ascii_lowercase();
            s = chars.into_iter().collect();
        }
    }

    let path = PathBuf::from(&s);
    if path.exists() {
        if let Ok(canon) = path.canonicalize() {
            return format_file_path(&canon);
        }
    }

    format_file_path(&lexical_normalize(&path))
}

fn format_file_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    // ensure consistent file:// form for absolute paths
    if path.is_absolute() {
        if s.starts_with('/') {
            format!("file://{s}")
        } else {
            // Windows
            format!("file:///{}", s.replace('\\', "/"))
        }
    } else {
        let mut t = s.replace('\\', "/");
        if let Some(stripped) = t.strip_prefix("./") {
            t = stripped.to_string();
        }
        t = strip_trailing_git(&strip_trailing_slashes(&t));
        t
    }
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn https_strip_git_and_slash_and_port() {
        assert_eq!(
            normalize_git_url("https://GitHub.com/Org/Repo.git/"),
            "https://github.com/Org/Repo"
        );
        assert_eq!(
            normalize_git_url("https://example.com:443/a.git"),
            "https://example.com/a"
        );
    }

    #[test]
    fn scp_like() {
        assert_eq!(
            normalize_git_url("git@github.com:org/repo.git"),
            "ssh://git@github.com/org/repo"
        );
    }

    #[test]
    fn https_and_ssh_not_equal() {
        assert!(!urls_match(
            "https://github.com/org/repo.git",
            "git@github.com:org/repo.git"
        ));
    }

    #[test]
    fn keep_userinfo() {
        assert_ne!(
            normalize_git_url("https://user@host/repo"),
            normalize_git_url("https://host/repo")
        );
    }

    #[test]
    fn relative_fixture_urls() {
        assert_eq!(
            normalize_git_url("./fixtures/alpha.git"),
            "fixtures/alpha"
        );
        assert_eq!(normalize_git_url("fixtures/alpha.git"), "fixtures/alpha");
    }

    #[test]
    fn relative_vs_absolute_file_with_root() {
        let dir = tempdir().unwrap();
        let fixture = dir.path().join("fixtures").join("alpha");
        fs::create_dir_all(&fixture).unwrap();
        // make real path
        let abs = fixture.canonicalize().unwrap();
        let file_url = format!("file://{}", abs.display());
        assert!(urls_match_with_root(
            "./fixtures/alpha",
            &file_url,
            Some(dir.path())
        ));
    }

    #[test]
    fn path_case_preserved() {
        let a = normalize_git_url("https://host/Org/Repo");
        assert!(a.ends_with("/Org/Repo"));
    }
}
