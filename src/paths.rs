use std::path::{Path, PathBuf};

pub fn hivemind_dir(root: &Path) -> PathBuf {
    root.join(".hivemind")
}

pub fn hive_root(start: &Path) -> Result<PathBuf, String> {
    let mut dir = start.to_path_buf();
    if let Ok(c) = dir.canonicalize() {
        dir = c;
    }
    loop {
        if lanes_path(&dir).is_file() {
            return Ok(dir);
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent.to_path_buf(),
            _ => return Err("Missing .hivemind/hivemind.yaml".into()),
        }
    }
}

pub fn workbench_path(root: &Path) -> PathBuf {
    hivemind_dir(root).join("workbench.yaml")
}

pub fn pin_path(root: &Path) -> PathBuf {
    hivemind_dir(root).join("workbench.lock.yaml")
}

pub fn lanes_path(root: &Path) -> PathBuf {
    hivemind_dir(root).join("hivemind.yaml")
}

pub fn actors_dir(root: &Path) -> PathBuf {
    hivemind_dir(root).join("actors")
}

fn odm_config_path(root: &Path) -> PathBuf {
    root.join(".odm/odm.config.yaml")
}

fn odm_lock_path(root: &Path) -> PathBuf {
    root.join(".odm/odm.lock.yaml")
}

fn workbench_yaml_present(root: &Path) -> bool {
    workbench_path(root).is_file()
}

pub fn workbench_read_path(root: &Path) -> PathBuf {
    if workbench_yaml_present(root) {
        return workbench_path(root);
    }
    let legacy = odm_config_path(root);
    if legacy.is_file() {
        return legacy;
    }
    workbench_path(root)
}

pub fn pin_read_path(root: &Path) -> PathBuf {
    if workbench_yaml_present(root) {
        return pin_path(root);
    }
    let legacy = odm_lock_path(root);
    if legacy.is_file() {
        return legacy;
    }
    pin_path(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn catalog_is_workbench_yaml() {
        assert_eq!(
            workbench_path(Path::new("/wb")),
            Path::new("/wb/.hivemind/workbench.yaml")
        );
    }

    #[test]
    fn pin_is_workbench_lock_yaml() {
        assert_eq!(
            pin_path(Path::new("/wb")),
            Path::new("/wb/.hivemind/workbench.lock.yaml")
        );
    }

    #[test]
    fn lanes_stay_hivemind_yaml() {
        assert_eq!(
            lanes_path(Path::new("/wb")),
            Path::new("/wb/.hivemind/hivemind.yaml")
        );
    }

    #[test]
    fn actors_stay_under_hivemind() {
        assert_eq!(
            actors_dir(Path::new("/wb")),
            Path::new("/wb/.hivemind/actors")
        );
    }

    #[test]
    fn missing_workbench_yaml_falls_back_to_odm_config() {
        let dir = tempfile::tempdir().unwrap();
        let odm = dir.path().join(".odm");
        fs::create_dir_all(&odm).unwrap();
        let legacy = odm.join("odm.config.yaml");
        fs::write(&legacy, "from: odm\n").unwrap();
        assert_eq!(workbench_read_path(dir.path()), legacy);
        assert_eq!(
            fs::read_to_string(workbench_read_path(dir.path())).unwrap(),
            "from: odm\n"
        );
    }

    #[test]
    fn missing_workbench_yaml_falls_back_to_odm_lock() {
        let dir = tempfile::tempdir().unwrap();
        let odm = dir.path().join(".odm");
        fs::create_dir_all(&odm).unwrap();
        let legacy = odm.join("odm.lock.yaml");
        fs::write(&legacy, "lock: odm\n").unwrap();
        assert_eq!(pin_read_path(dir.path()), legacy);
        assert_eq!(
            fs::read_to_string(pin_read_path(dir.path())).unwrap(),
            "lock: odm\n"
        );
    }

    #[test]
    fn present_workbench_yaml_wins_over_odm_config() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = workbench_path(dir.path());
        fs::create_dir_all(catalog.parent().unwrap()).unwrap();
        fs::write(&catalog, "from: hivemind\n").unwrap();
        let odm = dir.path().join(".odm");
        fs::create_dir_all(&odm).unwrap();
        fs::write(odm.join("odm.config.yaml"), "from: odm\n").unwrap();
        assert_eq!(workbench_read_path(dir.path()), catalog);
        assert_eq!(
            fs::read_to_string(workbench_read_path(dir.path())).unwrap(),
            "from: hivemind\n"
        );
    }

    #[test]
    fn missing_both_catalogs_stays_on_workbench_yaml() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(workbench_read_path(dir.path()), workbench_path(dir.path()));
        assert_eq!(pin_read_path(dir.path()), pin_path(dir.path()));
    }

    #[test]
    fn present_workbench_yaml_wins_over_odm_lock() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = workbench_path(dir.path());
        fs::create_dir_all(catalog.parent().unwrap()).unwrap();
        fs::write(&catalog, "from: hivemind\n").unwrap();
        fs::write(pin_path(dir.path()), "lock: hivemind\n").unwrap();
        let odm = dir.path().join(".odm");
        fs::create_dir_all(&odm).unwrap();
        fs::write(odm.join("odm.lock.yaml"), "lock: odm\n").unwrap();
        assert_eq!(pin_read_path(dir.path()), pin_path(dir.path()));
        assert_eq!(
            fs::read_to_string(pin_read_path(dir.path())).unwrap(),
            "lock: hivemind\n"
        );
    }
}
