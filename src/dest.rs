use std::path::Path;

use serde_yaml::Value;

use crate::dest_config::load_dest_config;
use crate::dest_match::{match_notes, Match};
use crate::dest_note::{claim, ClaimResult};
use crate::dest_scan::{scan, ScanResult};

pub fn scan_match_claim(
    cwd: &Path,
    run_id: &str,
    at: &str,
) -> Result<(ScanResult, Vec<Match>, usize), String> {
    let config = load_dest_config(cwd)?;
    let scanned = scan(cwd, &config, at, false)?;
    let matches = match_notes(&config.lanes, &scanned.notes, &config.disable, Some(cwd));
    let mut claimed = 0;
    for m in &matches {
        let trigger = m
            .lane
            .trigger
            .get("status")
            .cloned()
            .unwrap_or(Value::Null);
        if claim(&m.note.abs, &trigger, &m.lane.claim_status, run_id, at) == ClaimResult::Claimed {
            claimed += 1;
        }
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
}
