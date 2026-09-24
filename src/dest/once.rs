use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::dest::config::{load_dest_config, CmdSpec};
use crate::dest::matcher::match_notes;
use crate::dest::scan::scan;
use crate::dest::scan_match_claim;
use crate::dest::spawn::spawn_cmds;

pub fn run_tick(cwd: &Path, max_spawns: Option<u64>) -> Result<(u8, usize), String> {
    let run_id = format!("bee-{}", std::process::id());
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into());
    let (_, matches, _) = scan_match_claim(cwd, &run_id, &at, max_spawns)?;
    let mut code: u8 = 0;
    let mut n = 0;
    for m in matches {
        if m.lane.cmds.is_empty() {
            continue;
        }
        let c = spawn_cmds(cwd, &m.lane.lane, &run_id, &m.note.path, &m.lane.cmds)?;
        n += 1;
        if c != 0 && code == 0 {
            code = c as u8;
        }
    }
    Ok((code, n))
}

pub fn run_once(cwd: &Path) -> Result<u8, String> {
    Ok(run_tick(cwd, None)?.0)
}

pub fn dry_run(cwd: &Path) -> Result<String, String> {
    let config = load_dest_config(cwd)?;
    let scanned = scan(cwd, &config, "t", true)?;
    let matches = match_notes(&config.lanes, &scanned.notes, &config.disable, Some(cwd));
    let mut out = String::new();
    for m in &matches {
        if m.lane.cmds.is_empty() {
            continue;
        }
        out.push_str(&m.lane.lane);
        out.push('\t');
        out.push_str(&m.note.path);
        push_stored_tokens(&mut out, &m.lane.cmds);
        out.push('\n');
    }
    if out.is_empty() {
        out.push_str("(no matches)\n");
    }
    Ok(out)
}

fn push_stored_tokens(out: &mut String, cmds: &[CmdSpec]) {
    for cmd in cmds {
        match cmd {
            CmdSpec::String(token) => {
                out.push('\t');
                out.push_str(token);
            }
            CmdSpec::List(tokens) => {
                for token in tokens {
                    out.push('\t');
                    out.push_str(token);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn fixture(cmd: &str) -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".hivemind")).unwrap();
        fs::write(
            root.join(".hivemind/hivemind.yaml"),
            format!(
                "folders:\n  - path: inbox\n    schema:\n      id: string\n      status: string\n    required: [id, status]\n  - path: quarantine\n    schema: quarantine\nlanes:\n  work:\n    type: single\n    trigger:\n      status: ready\n    claim-status: claimed\n    cmd: {cmd}\n"
            ),
        )
        .unwrap();
        fs::create_dir_all(root.join("inbox")).unwrap();
        fs::write(root.join("inbox/n.md"), "---\nid: a\nstatus: ready\n---\n").unwrap();
        dir
    }

    #[test]
    fn once_exits_after_one_tick_no_shell() {
        let dir = fixture("[\"true\", \"x; rm -rf nowhere\"]");
        assert_eq!(run_once(dir.path()).unwrap(), 0);
        let text = fs::read_to_string(dir.path().join("inbox/n.md")).unwrap();
        assert!(text.contains("claimed-by:"));
        assert_eq!(run_once(dir.path()).unwrap(), 0);
    }

    #[test]
    fn once_missing_yaml_fails() {
        let dir = tempdir().unwrap();
        assert!(run_once(dir.path()).is_err());
    }

    #[test]
    fn once_concurrency_one_leaves_the_other_ready() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".hivemind")).unwrap();
        fs::create_dir_all(root.join("inbox")).unwrap();
        fs::write(
            root.join(".hivemind/hivemind.yaml"),
            "folders:\n  - path: inbox\n    schema:\n      id: string\n      status: string\n    required: [id, status]\n  - path: quarantine\n    schema: quarantine\nlanes:\n  work:\n    type: single\n    concurrency: 1\n    trigger:\n      status: ready\n    claim-status: claimed\n    cmd: [\"true\"]\n",
        )
        .unwrap();
        fs::write(root.join("inbox/a.md"), "---\nid: a\nstatus: ready\n---\n").unwrap();
        fs::write(root.join("inbox/b.md"), "---\nid: b\nstatus: ready\n---\n").unwrap();
        assert_eq!(run_once(root).unwrap(), 0);
        let a = fs::read_to_string(root.join("inbox/a.md")).unwrap();
        let b = fs::read_to_string(root.join("inbox/b.md")).unwrap();
        assert!(a.contains("claimed-by:"), "{a}");
        assert!(b.contains("status: ready"), "{b}");
        assert!(!b.contains("claimed-by"), "{b}");
    }

    #[test]
    fn once_dry_run_prints_plan_without_claim() {
        println!("hivemind.cli:once-dry-run");
        let dir = tempdir().unwrap();
        let root = dir.path();
        let marker = root.join("touched");
        let marker_s = marker.display().to_string();
        let cmd = format!("[\"touch\", \"{marker_s}\", \"{{{{env.SECRET}}}}\"]");
        fs::create_dir_all(root.join(".hivemind")).unwrap();
        fs::write(
            root.join(".hivemind/hivemind.yaml"),
            format!(
                "folders:\n  - path: inbox\n    schema:\n      id: string\n      status: string\n    required: [id, status]\n  - path: quarantine\n    schema: quarantine\nlanes:\n  work:\n    type: single\n    trigger:\n      status: ready\n    claim-status: claimed\n    cmd: {cmd}\n"
            ),
        )
        .unwrap();
        fs::create_dir_all(root.join("inbox")).unwrap();
        fs::write(root.join("inbox/n.md"), "---\nid: a\nstatus: ready\n---\n").unwrap();
        std::env::set_var("SECRET", "hunter2");
        let plan = dry_run(root).unwrap();
        std::env::remove_var("SECRET");
        assert_eq!(
            plan,
            format!("work\tinbox/n.md\ttouch\t{marker_s}\t{{{{env.SECRET}}}}\n")
        );
        let note = fs::read_to_string(root.join("inbox/n.md")).unwrap();
        assert!(!note.contains("claimed-by"), "{note}");
        assert!(!marker.is_file(), "{}", marker.display());
    }
}
