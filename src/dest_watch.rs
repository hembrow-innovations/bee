use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::dest_once::run_tick;

pub fn run_watch(
    cwd: &Path,
    until_quiet: bool,
    until_target: Option<&Path>,
    sleep: Duration,
) -> Result<u8, String> {
    loop {
        if let Some(target) = until_target {
            if cwd.join(target).exists() || target.exists() {
                return Ok(0);
            }
        }
        if cwd.join(".hivemind/STOP").is_file() {
            return Ok(0);
        }
        let (code, n) = run_tick(cwd)?;
        if until_quiet && n == 0 {
            return Ok(code);
        }
        if until_target.is_none() && !until_quiet {
            return Ok(code);
        }
        thread::sleep(sleep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            run_watch(dir.path(), true, None, Duration::from_millis(1)).unwrap(),
            0
        );
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
                Duration::from_millis(1)
            )
            .unwrap(),
            0
        );
    }
}
