use std::fs;
use std::io;
use std::path::Path;

use crate::paths::{actors_dir, lanes_path, pin_path, workbench_path};

pub fn init_workbench(root: &Path) -> io::Result<()> {
    let catalog = workbench_path(root);
    if catalog.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            catalog.display().to_string(),
        ));
    }
    fs::create_dir_all(actors_dir(root))?;
    fs::write(&catalog, "{}\n")?;
    let pin = pin_path(root);
    if !pin.exists() {
        fs::write(&pin, "version: 1\npins: {}\n")?;
    }
    let lanes = lanes_path(root);
    if !lanes.exists() {
        fs::write(&lanes, "folders: []\nlanes: {}\n")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{actors_dir, lanes_path, pin_path, workbench_path};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn init_writes_workbench_yaml() {
        let dir = tempdir().unwrap();
        init_workbench(dir.path()).unwrap();
        assert!(workbench_path(dir.path()).is_file());
    }

    #[test]
    fn init_writes_pin_at_workbench_lock_yaml() {
        let dir = tempdir().unwrap();
        init_workbench(dir.path()).unwrap();
        assert!(pin_path(dir.path()).is_file());
    }

    #[test]
    fn init_writes_lane_file_hivemind_yaml() {
        let dir = tempdir().unwrap();
        init_workbench(dir.path()).unwrap();
        assert!(lanes_path(dir.path()).is_file());
    }

    #[test]
    fn init_keeps_actors_dir() {
        let dir = tempdir().unwrap();
        init_workbench(dir.path()).unwrap();
        assert!(actors_dir(dir.path()).is_dir());
    }

    #[test]
    fn init_does_not_write_odm_filenames() {
        let dir = tempdir().unwrap();
        init_workbench(dir.path()).unwrap();
        assert!(!dir.path().join(".odm").exists());
        assert!(!dir.path().join(".hivemind/odm.config.yaml").exists());
        assert!(!dir.path().join(".hivemind/odm.lock.yaml").exists());
    }

    #[test]
    fn init_does_not_overwrite_existing_workbench_yaml() {
        let dir = tempdir().unwrap();
        let catalog = workbench_path(dir.path());
        fs::create_dir_all(catalog.parent().unwrap()).unwrap();
        fs::write(&catalog, "keep: true\n").unwrap();
        assert!(init_workbench(dir.path()).is_err());
        assert_eq!(fs::read_to_string(&catalog).unwrap(), "keep: true\n");
    }

    #[test]
    fn init_does_not_overwrite_existing_lane_file() {
        let dir = tempdir().unwrap();
        let lanes = lanes_path(dir.path());
        fs::create_dir_all(lanes.parent().unwrap()).unwrap();
        fs::write(&lanes, "# preserved\nfolders: []\nlanes: {}\n").unwrap();
        init_workbench(dir.path()).unwrap();
        assert!(fs::read_to_string(&lanes)
            .unwrap()
            .starts_with("# preserved"));
    }
}
