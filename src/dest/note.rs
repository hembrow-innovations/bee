use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_yaml::Value;

pub type YamlMap = serde_yaml::Mapping;

pub enum ParseFrontMatter {
    Ok(YamlMap),
    Fault(&'static str),
}

pub fn parse_front_matter(raw: &str) -> ParseFrontMatter {
    if !raw.starts_with("---") {
        return ParseFrontMatter::Fault("parse-error");
    }
    let after = &raw[3..];
    let close = after.find("\n---").or_else(|| after.find("\r\n---"));
    let Some(idx) = close else {
        return ParseFrontMatter::Fault("parse-error");
    };
    let yaml_text = after[..idx].trim_start_matches(['\r', '\n']);
    match serde_yaml::from_str::<Value>(yaml_text) {
        Ok(Value::Mapping(map)) => ParseFrontMatter::Ok(map),
        Ok(Value::Null) => ParseFrontMatter::Ok(YamlMap::new()),
        _ => ParseFrontMatter::Fault("parse-error"),
    }
}

pub fn quarantine_note(abs: &Path, dest_dir: &Path, origin: &str, fault: &str, at: &str) {
    if !abs.is_file() {
        return;
    }
    fs::create_dir_all(dest_dir).unwrap();
    let dest = dest_dir.join(abs.file_name().unwrap());
    match fs::rename(abs, &dest) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => return,
        Err(err) => panic!("{err}"),
    }
    fs::write(
        &dest,
        format!("---\norigin-location: {origin}\nquarantined-at: {at}\nfault: {fault}\n---\n"),
    )
    .unwrap();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimResult {
    Claimed,
    Skipped,
}

const CLAIMLOCK_STALE: Duration = Duration::from_secs(30);

pub fn claim(
    abs: &Path,
    trigger_status: &Value,
    claim_status: &str,
    run_id: &str,
    at: &str,
) -> ClaimResult {
    let lock_path = claimlock(abs);
    if !try_lock(&lock_path) {
        return ClaimResult::Skipped;
    }
    let result = (|| {
        let raw = fs::read_to_string(abs).ok()?;
        let ParseFrontMatter::Ok(map) = parse_front_matter(&raw) else {
            return Some(ClaimResult::Skipped);
        };
        if map.get(Value::String("status".into())) != Some(trigger_status) {
            return Some(ClaimResult::Skipped);
        }
        fs::write(abs, apply_claim(&raw, claim_status, run_id, at)).ok()?;
        Some(ClaimResult::Claimed)
    })();
    let _ = fs::remove_dir(&lock_path);
    result.unwrap_or(ClaimResult::Skipped)
}

fn claimlock(abs: &Path) -> PathBuf {
    let mut p = abs.as_os_str().to_os_string();
    p.push(".claimlock");
    PathBuf::from(p)
}

fn try_lock(lock_path: &Path) -> bool {
    match fs::create_dir(lock_path) {
        Ok(()) => true,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            if !reap_stale(lock_path) {
                return false;
            }
            fs::create_dir(lock_path).is_ok()
        }
        Err(_) => false,
    }
}

fn reap_stale(lock_path: &Path) -> bool {
    let Ok(meta) = fs::metadata(lock_path) else {
        return !lock_path.exists();
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    let Ok(age) = SystemTime::now().duration_since(modified) else {
        return false;
    };
    if age < CLAIMLOCK_STALE {
        return false;
    }
    fs::remove_dir(lock_path).is_ok()
}

fn apply_claim(raw: &str, claim_status: &str, run_id: &str, claimed_at: &str) -> String {
    let Some((pre, front, post, rest)) = split_fence(raw) else {
        return raw.to_string();
    };
    let mut front = upsert_key(front, "status", claim_status);
    front = upsert_key(&front, "claimed-by", run_id);
    front = upsert_key(&front, "claimed-at", claimed_at);
    format!("{pre}{front}{post}{rest}")
}

fn split_fence(raw: &str) -> Option<(&str, &str, &str, &str)> {
    if !raw.starts_with("---\n") && !raw.starts_with("---\r\n") {
        return None;
    }
    let pre_len = if raw.starts_with("---\r\n") { 5 } else { 4 };
    let rest = &raw[pre_len..];
    let close = rest.find("\n---")?;
    let front = &rest[..close];
    let after = &rest[close..];
    let post_end = after.find('\n').map(|i| i + 1).unwrap_or(after.len());
    let post = &after[..post_end];
    let body = &after[post_end..];
    Some((&raw[..pre_len], front, post, body))
}

fn upsert_key(front: &str, key: &str, value: &str) -> String {
    let line = format!("{key}: {value}");
    let mut out = String::new();
    let mut replaced = false;
    for existing in front.lines() {
        if existing.starts_with(&format!("{key}:")) {
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
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parse_ok_map() {
        let ParseFrontMatter::Ok(map) = parse_front_matter("---\nid: a\n---\nbody\n") else {
            panic!("ok");
        };
        assert_eq!(
            map.get(Value::String("id".into())),
            Some(&Value::String("a".into()))
        );
    }

    #[test]
    fn parse_unclosed_is_fault() {
        assert!(matches!(
            parse_front_matter("---\nid: a\n"),
            ParseFrontMatter::Fault("parse-error")
        ));
    }

    #[test]
    fn quarantine_missing_source_does_not_panic() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("gone.md");
        let q = dir.path().join("q");
        quarantine_note(&src, &q, "gone.md", "parse-error", "t");
        assert!(!q.join("gone.md").exists());
    }

    #[test]
    fn quarantine_writes_three_keys() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("n.md");
        fs::write(&src, "---\nid: a\nstatus: ready\n---\nkeep\n").unwrap();
        let q = dir.path().join("q");
        quarantine_note(&src, &q, "n.md", "unknown-key:x", "t");
        let dest = q.join("n.md");
        let text = fs::read_to_string(&dest).unwrap();
        assert!(text.contains("origin-location: n.md"));
        assert!(text.contains("fault: unknown-key:x"));
        assert!(!text.contains("status:"));
        assert!(!text.contains("keep"));
    }

    #[test]
    fn claim_is_cas_and_skips_when_locked_or_status_moved() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("n.md");
        fs::write(&src, "---\nstatus: ready\n---\nbody\n").unwrap();
        let ready = Value::String("ready".into());
        assert_eq!(
            claim(&src, &ready, "claimed", "run-1", "t"),
            ClaimResult::Claimed
        );
        let text = fs::read_to_string(&src).unwrap();
        assert!(text.contains("claimed-by: run-1"));
        assert!(text.contains("body"));
        assert_eq!(
            claim(&src, &ready, "claimed", "run-2", "t"),
            ClaimResult::Skipped
        );
    }
}
