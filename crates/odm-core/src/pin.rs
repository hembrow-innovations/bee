use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::pin_path;
use crate::error::OdmError;
use crate::io::atomic_write;

/// Optional pin lock file (`.odm/odm.lock.yaml`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinFile {
    pub version: u32,
    #[serde(default)]
    pub pins: BTreeMap<String, PinEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinEntry {
    pub rev: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

impl PinFile {
    pub fn new_v1() -> Self {
        Self {
            version: 1,
            pins: BTreeMap::new(),
        }
    }

    pub fn validate(&self) -> Result<(), OdmError> {
        if self.version != 1 {
            return Err(OdmError::workspace(format!(
                "unsupported pin file version {} (expected 1)",
                self.version
            )));
        }
        for (name, entry) in &self.pins {
            if name.trim().is_empty() {
                return Err(OdmError::workspace("pin name must not be empty"));
            }
            if !is_full_sha(&entry.rev) {
                return Err(OdmError::workspace(format!(
                    "pin '{name}' rev must be 40-char lowercase hex SHA"
                )));
            }
            if entry.url.trim().is_empty() {
                return Err(OdmError::workspace(format!(
                    "pin '{name}' url must not be empty"
                )));
            }
        }
        Ok(())
    }
}

pub fn is_full_sha(s: &str) -> bool {
    s.len() == 40
        && s.chars().all(|c| c.is_ascii_hexdigit())
        && s.chars().all(|c| !c.is_ascii_uppercase())
}

pub fn parse_pin_yaml(text: &str) -> Result<PinFile, OdmError> {
    let pin: PinFile = serde_yaml::from_str(text.trim()).map_err(|e| {
        OdmError::workspace(format!("invalid pin YAML: {e}"))
    })?;
    pin.validate()?;
    Ok(pin)
}

pub fn load_pin(root: &Path) -> Result<Option<PinFile>, OdmError> {
    let path = pin_path(root);
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).map_err(|e| {
        OdmError::workspace(format!("failed to read {}: {e}", path.display()))
    })?;
    Ok(Some(parse_pin_yaml(&text)?))
}

pub fn save_pin(root: &Path, pin: &PinFile) -> Result<(), OdmError> {
    pin.validate()?;
    let path = pin_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            OdmError::operation(format!("failed to create {}: {e}", parent.display()))
        })?;
    }
    let yaml = serde_yaml::to_string(pin)
        .map_err(|e| OdmError::operation(format!("failed to serialize pin: {e}")))?;
    atomic_write(&path, &yaml)
}

/// Drop pins whose names are not in `managed_names`; keep others.
pub fn prune_pins(pin: &mut PinFile, managed_names: &[&str]) {
    let set: std::collections::BTreeSet<&str> = managed_names.iter().copied().collect();
    pin.pins.retain(|k, _| set.contains(k.as_str()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn pin_v1_roundtrip() {
        let mut p = PinFile::new_v1();
        p.pins.insert(
            "api".into(),
            PinEntry {
                rev: "a".repeat(40),
                url: "https://example.com/a.git".into(),
                branch: Some("main".into()),
            },
        );
        let yaml = serde_yaml::to_string(&p).unwrap();
        let back = parse_pin_yaml(&yaml).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn bad_version_and_rev() {
        assert!(parse_pin_yaml("version: 2\npins: {}\n").is_err());
        let yaml = "version: 1\npins:\n  x:\n    rev: abcd\n    url: u\n";
        assert!(parse_pin_yaml(yaml).is_err());
    }

    #[test]
    fn save_load() {
        let dir = tempdir().unwrap();
        let mut p = PinFile::new_v1();
        p.pins.insert(
            "a".into(),
            PinEntry {
                rev: "b".repeat(40),
                url: "u".into(),
                branch: None,
            },
        );
        save_pin(dir.path(), &p).unwrap();
        let loaded = load_pin(dir.path()).unwrap().unwrap();
        assert_eq!(loaded, p);
    }

    #[test]
    fn prune() {
        let mut p = PinFile::new_v1();
        p.pins.insert(
            "keep".into(),
            PinEntry {
                rev: "c".repeat(40),
                url: "u".into(),
                branch: None,
            },
        );
        p.pins.insert(
            "drop".into(),
            PinEntry {
                rev: "d".repeat(40),
                url: "u".into(),
                branch: None,
            },
        );
        prune_pins(&mut p, &["keep"]);
        assert!(p.pins.contains_key("keep"));
        assert!(!p.pins.contains_key("drop"));
    }
}
