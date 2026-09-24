//! Dest predicate machine: scan notes, match lanes, claim, and spawn.

pub(crate) mod config;
pub(crate) mod explain;
pub(crate) mod journal;
pub(crate) mod matcher;
pub(crate) mod note;
pub(crate) mod once;
pub(crate) mod scan;
pub(crate) mod spawn;
pub(crate) mod watch;

use std::collections::BTreeMap;
use std::path::Path;

use serde_yaml::Value;

use self::config::load_dest_config;
use self::matcher::{match_notes, Match};
use self::note::{claim, ClaimResult};
use self::scan::{scan, ScanResult};

pub use config::{lookup_notes, NotesDirs};
pub use explain::{explain, gc};
pub use once::run_once;
pub use watch::run_watch;

pub fn scan_match_claim(
    cwd: &Path,
    run_id: &str,
    at: &str,
) -> Result<(ScanResult, Vec<Match>, usize), String> {
    let config = load_dest_config(cwd)?;
    let scanned = scan(cwd, &config, at, false)?;
    let found = match_notes(&config.lanes, &scanned.notes, &config.disable, Some(cwd));
    let mut claimed = 0;
    let mut taken: BTreeMap<String, u64> = BTreeMap::new();
    let mut matches = Vec::new();
    for m in found {
        if m.lane.cmds.is_empty() {
            matches.push(m);
            continue;
        }
        if let Some(&cap) = config.concurrency.get(&m.lane.lane) {
            let used = taken.get(&m.lane.lane).copied().unwrap_or(0);
            if used >= cap {
                continue;
            }
            taken.insert(m.lane.lane.clone(), used + 1);
        }
        let trigger = m
            .lane
            .trigger
            .get("status")
            .cloned()
            .unwrap_or(Value::Null);
        if claim(&m.note.abs, &trigger, &m.lane.claim_status, run_id, at) == ClaimResult::Claimed {
            claimed += 1;
        }
        matches.push(m);
    }
    Ok((scanned, matches, claimed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn claim_cas_from_scan_match() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".hivemind")).unwrap();
        fs::write(
            root.join(".hivemind/hivemind.yaml"),
            "folders:\n  - path: inbox\n    schema:\n      id: string\n      status: string\n    required: [id, status]\n  - path: quarantine\n    schema: quarantine\nlanes:\n  work:\n    type: single\n    trigger:\n      status: ready\n    claim-status: claimed\n    cmd: [true]\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("inbox")).unwrap();
        fs::write(root.join("inbox/n.md"), "---\nid: a\nstatus: ready\n---\nbody\n").unwrap();
        let (_, matches, claimed) = scan_match_claim(root, "run-1", "t").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(claimed, 1);
        let text = fs::read_to_string(root.join("inbox/n.md")).unwrap();
        assert!(text.contains("claimed-by: run-1"));
        let (_, _, claimed2) = scan_match_claim(root, "run-2", "t").unwrap();
        assert_eq!(claimed2, 0);
    }

    fn hive(root: &std::path::Path, lanes: &str) {
        fs::create_dir_all(root.join(".hivemind")).unwrap();
        fs::create_dir_all(root.join("inbox")).unwrap();
        fs::write(
            root.join(".hivemind/hivemind.yaml"),
            format!(
                "folders:\n  - path: inbox\n    schema:\n      id: string\n      status: string\n    required: [id, status]\n  - path: quarantine\n    schema: quarantine\nlanes:\n{lanes}"
            ),
        )
        .unwrap();
    }

    fn note(root: &std::path::Path, name: &str, id: &str, status: &str) {
        fs::write(
            root.join("inbox").join(name),
            format!("---\nid: {id}\nstatus: {status}\n---\n"),
        )
        .unwrap();
    }

    #[test]
    fn concurrency_one_claims_one_then_the_other() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        hive(
            root,
            "  work:\n    type: single\n    concurrency: 1\n    trigger:\n      status: ready\n    claim-status: claimed\n    cmd: [\"true\"]\n",
        );
        note(root, "a.md", "a", "ready");
        note(root, "b.md", "b", "ready");
        let (_, matches, claimed) = scan_match_claim(root, "run-1", "t").unwrap();
        assert_eq!(claimed, 1);
        assert_eq!(matches.len(), 1);
        let a = fs::read_to_string(root.join("inbox/a.md")).unwrap();
        let b = fs::read_to_string(root.join("inbox/b.md")).unwrap();
        assert!(a.contains("claimed-by: run-1"), "{a}");
        assert!(b.contains("status: ready"), "{b}");
        assert!(!b.contains("claimed-by"), "{b}");
        let (_, matches2, claimed2) = scan_match_claim(root, "run-2", "t2").unwrap();
        assert_eq!(claimed2, 1);
        assert_eq!(matches2.len(), 1);
        let b2 = fs::read_to_string(root.join("inbox/b.md")).unwrap();
        assert!(b2.contains("claimed-by: run-2"), "{b2}");
    }

    #[test]
    fn missing_concurrency_claims_both() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        hive(
            root,
            "  work:\n    type: single\n    trigger:\n      status: ready\n    claim-status: claimed\n    cmd: [\"true\"]\n",
        );
        note(root, "a.md", "a", "ready");
        note(root, "b.md", "b", "ready");
        let (_, matches, claimed) = scan_match_claim(root, "run-1", "t").unwrap();
        assert_eq!(claimed, 2);
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn empty_cmd_does_not_claim() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        hive(
            root,
            "  work:\n    type: single\n    concurrency: 1\n    trigger:\n      status: ready\n    claim-status: claimed\n",
        );
        note(root, "a.md", "a", "ready");
        let (_, matches, claimed) = scan_match_claim(root, "run-1", "t").unwrap();
        assert_eq!(claimed, 0);
        assert_eq!(matches.len(), 1);
        let text = fs::read_to_string(root.join("inbox/a.md")).unwrap();
        assert!(text.contains("status: ready"), "{text}");
        assert!(!text.contains("claimed-by"), "{text}");
    }

    #[test]
    fn lane_caps_are_independent() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        hive(
            root,
            "  a:\n    type: single\n    concurrency: 1\n    trigger:\n      status: ready\n    claim-status: claimed\n    cmd: [\"true\"]\n  b:\n    type: single\n    concurrency: 1\n    trigger:\n      status: queued\n    claim-status: claimed\n    cmd: [\"true\"]\n",
        );
        note(root, "a.md", "a", "ready");
        note(root, "b.md", "b", "queued");
        let (_, _, claimed) = scan_match_claim(root, "run-1", "t").unwrap();
        assert_eq!(claimed, 2);
    }
}
