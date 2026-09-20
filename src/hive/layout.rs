use std::fs;
use std::path::Path;

use hive_core::{parse_config_yaml, validate_and_load_bundles, HiveError, Workbench, WorkbenchConfig};

use crate::workbench_read_path;

pub fn parse_workbench_yaml(text: &str) -> Result<WorkbenchConfig, HiveError> {
    parse_config_yaml(text)
}

pub fn load_workbench(root: &Path) -> Result<Workbench, HiveError> {
    let path = workbench_read_path(root);
    if !path.is_file() {
        return Err(HiveError::hive(format!(
            "not a Workbench: missing {}",
            path.display()
        )));
    }
    let text = fs::read_to_string(&path)
        .map_err(|e| HiveError::hive(format!("failed to read {}: {e}", path.display())))?;
    let config = parse_workbench_yaml(&text)?;
    validate_and_load_bundles(root, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workbench_path;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn parse_workbench_yaml_named_project_checkout() {
        let cfg = parse_workbench_yaml(
            "name: hive\nprojects:\n  bee:\n    path: bee\n    url: https://example.com/bee.git\n",
        )
        .unwrap();
        assert_eq!(cfg.name.as_deref(), Some("hive"));
        let project = cfg.projects.get("bee").expect("project bee");
        assert_eq!(project.path, "bee");
        assert_ne!(project.path.as_str(), ".");
        assert_ne!(project.path.as_str(), "");
    }

    #[test]
    fn workbench_root_is_not_a_project() {
        let cfg = parse_workbench_yaml("{}\n").unwrap();
        assert!(cfg.projects.is_empty());
    }

    #[test]
    fn load_reads_workbench_yaml() {
        let dir = tempdir().unwrap();
        let catalog = workbench_path(dir.path());
        fs::create_dir_all(catalog.parent().unwrap()).unwrap();
        fs::write(&catalog, "projects:\n  hive-comb:\n    path: hive-comb\n").unwrap();
        fs::create_dir_all(dir.path().join(".odm")).unwrap();
        fs::write(
            dir.path().join(".odm/odm.config.yaml"),
            "projects:\n  wrong:\n    path: wrong\n",
        )
        .unwrap();
        let wb = load_workbench(dir.path()).unwrap();
        assert_eq!(wb.root, dir.path());
        assert_eq!(wb.config.projects["hive-comb"].path, "hive-comb");
        assert!(!wb.config.projects.contains_key("wrong"));
    }

    #[test]
    fn load_falls_back_to_odm_config_when_workbench_yaml_missing() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".odm")).unwrap();
        fs::write(
            dir.path().join(".odm/odm.config.yaml"),
            "projects:\n  legacy:\n    path: legacy\n",
        )
        .unwrap();
        let wb = load_workbench(dir.path()).unwrap();
        assert_eq!(wb.config.projects["legacy"].path, "legacy");
    }
}
