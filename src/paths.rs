use std::path::{Path, PathBuf};

pub fn hivemind_dir(root: &Path) -> PathBuf {
    root.join(".hivemind")
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
