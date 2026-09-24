use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::dest::once::run_tick;

pub fn run_watch(
    cwd: &Path,
    until_quiet: bool,
    until_target: Option<&Path>,
    max_spawns: Option<u64>,
    sleep: Duration,
) -> Result<u8, String> {
    let mut spawned = 0u64;
    loop {
        if let Some(target) = until_target {
            if cwd.join(target).exists() || target.exists() {
                return Ok(0);
            }
        }
        if cwd.join(".hivemind/STOP").is_file() {
            return Ok(0);
        }
        let remaining = max_spawns.map(|n| n.saturating_sub(spawned));
        let (code, n) = run_tick(cwd, remaining)?;
        spawned = spawned.saturating_add(n as u64);
        if until_quiet && n == 0 {
            return Ok(code);
        }
        thread::sleep(sleep);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use clap::Parser;
    use std::fs;
    use tempfile::tempdir;

    fn empty_cfg(root: &Path) {
        fs::create_dir_all(root.join(".hivemind")).unwrap();
        fs::write(
            root.join(".hivemind/hivemind.yaml"),
            "folders:\n  - path: inbox\n    schema:\n      id: string\n      status: string\n  - path: quarantine\n    schema: quarantine\nlanes: {}\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("inbox")).unwrap();
    }

    #[test]
    fn until_quiet_exits_after_one_quiet_scan() {
        let dir = tempdir().unwrap();
        empty_cfg(dir.path());
        assert_eq!(
            run_watch(dir.path(), true, None, None, Duration::from_millis(1)).unwrap(),
            0
        );
    }

    #[test]
    fn default_watch_loops_until_stop() {
        let dir = tempdir().unwrap();
        empty_cfg(dir.path());
        let stop = dir.path().join(".hivemind/STOP");
        let stop2 = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(40));
            fs::write(&stop2, "").unwrap();
        });
        assert_eq!(
            run_watch(dir.path(), false, None, None, Duration::from_millis(5)).unwrap(),
            0
        );
        assert!(stop.is_file());
    }

    #[test]
    fn until_target_exits_when_path_exists() {
        let dir = tempdir().unwrap();
        empty_cfg(dir.path());
        fs::write(dir.path().join("done"), "").unwrap();
        assert_eq!(
            run_watch(
                dir.path(),
                false,
                Some(Path::new("done")),
                None,
                Duration::from_millis(1)
            )
            .unwrap(),
            0
        );
    }

    #[test]
    pub(crate) fn watch_max_spawns_stops_new_claims() {
        println!("hivemind.cli:max-spawns");
        assert!(crate::cli::Cli::try_parse_from(["bee", "watch", "--max-spawns"]).is_err());
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".hivemind")).unwrap();
        fs::create_dir_all(root.join("inbox")).unwrap();
        fs::write(
            root.join(".hivemind/hivemind.yaml"),
            "folders:\n  - path: inbox\n    schema:\n      id: string\n      status: string\n    required: [id, status]\n  - path: quarantine\n    schema: quarantine\nlanes:\n  work:\n    type: single\n    trigger:\n      status: ready\n    claim-status: claimed\n    cmd: [\"true\"]\n",
        )
        .unwrap();
        fs::write(root.join("inbox/a.md"), "---\nid: a\nstatus: ready\n---\n").unwrap();
        fs::write(root.join("inbox/b.md"), "---\nid: b\nstatus: ready\n---\n").unwrap();
        let max_spawns = Some(1);
        assert_eq!(
            run_watch(root, true, None, max_spawns, Duration::from_millis(1)).unwrap(),
            0
        );
        let a = fs::read_to_string(root.join("inbox/a.md")).unwrap();
        let b = fs::read_to_string(root.join("inbox/b.md")).unwrap();
        assert!(a.contains("claimed-by:"), "{a}");
        assert!(b.contains("status: ready"), "{b}");
        assert!(!b.contains("claimed-by"), "{b}");
    }
}
