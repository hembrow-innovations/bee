use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::OdmError;

/// Atomic write: temp sibling then rename over target.
pub fn atomic_write(path: &Path, contents: &str) -> Result<(), OdmError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| {
        OdmError::operation(format!("failed to create {}: {e}", parent.display()))
    })?;

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("odm.tmp");
    let tmp = parent.join(format!(".{file_name}.{nanos}.tmp"));

    {
        let mut f = fs::File::create(&tmp).map_err(|e| {
            OdmError::operation(format!("failed to create temp {}: {e}", tmp.display()))
        })?;
        f.write_all(contents.as_bytes()).map_err(|e| {
            OdmError::operation(format!("failed to write temp {}: {e}", tmp.display()))
        })?;
        f.sync_all().ok();
    }

    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        OdmError::operation(format!(
            "failed to rename {} -> {}: {e}",
            tmp.display(),
            path.display()
        ))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn atomic_write_creates_new_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("out.txt");
        atomic_write(&path, "hello\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello\n");
        assert_no_temps(dir.path());
    }

    #[test]
    fn atomic_write_replaces_existing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("out.txt");
        fs::write(&path, "old").unwrap();
        atomic_write(&path, "new content").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new content");
        assert_no_temps(dir.path());
    }

    #[test]
    fn atomic_write_creates_missing_parents() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a/b/c.txt");
        atomic_write(&path, "nested").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "nested");
    }

    #[test]
    fn atomic_write_rename_failure_leaves_final_untorn_and_cleans_temp() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("target");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("blocker"), "x").unwrap();

        let err = atomic_write(&path, "payload").unwrap_err();
        assert_eq!(err.code(), "operation");
        assert!(path.is_dir(), "final path must remain a directory (not torn)");
        assert!(path.join("blocker").is_file());
        assert_no_temps(dir.path());
    }

    #[test]
    fn atomic_write_parent_create_failure_no_final_file() {
        let dir = tempdir().unwrap();
        let blocker = dir.path().join("not-a-dir");
        fs::write(&blocker, "file").unwrap();
        let path = blocker.join("child.txt");

        let err = atomic_write(&path, "x").unwrap_err();
        assert_eq!(err.code(), "operation");
        assert!(!path.exists());
        assert_eq!(fs::read_to_string(&blocker).unwrap(), "file");
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_temp_create_failure_no_final_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let parent = dir.path().join("ro");
        fs::create_dir(&parent).unwrap();
        let path = parent.join("out.txt");
        let mut perms = fs::metadata(&parent).unwrap().permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&parent, perms).unwrap();

        let err = atomic_write(&path, "x");
        let mut perms = fs::metadata(&parent).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&parent, perms).unwrap();

        assert!(err.is_err());
        assert!(!path.exists());
    }

    fn assert_no_temps(dir: &Path) {
        for entry in fs::read_dir(dir).unwrap() {
            let name = entry.unwrap().file_name();
            let s = name.to_string_lossy();
            assert!(
                !s.ends_with(".tmp"),
                "leftover temp file: {s}"
            );
        }
    }
}
