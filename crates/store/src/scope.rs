use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use hive_core::{abs_checkout, HiveError, Hive, HiveConfig};

/// A resolved Progen for store ops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedProgen {
    pub name: String,
    pub path: PathBuf,
}

/// Resolve read scope: default all, or union of `--progen` / `--progen-group`.
pub fn resolve_read_scope(
    ws: &Hive,
    progens: &[String],
    groups: &[String],
) -> Result<Vec<ScopedProgen>, HiveError> {
    if ws.config.progens.is_empty() {
        return Err(HiveError::usage(
            "no progens configured (add one with `odm progen add`)",
        ));
    }

    let names = if progens.is_empty() && groups.is_empty() {
        ws.config.progens.keys().cloned().collect::<Vec<_>>()
    } else {
        let mut set = BTreeSet::new();
        for name in progens {
            require_progen(&ws.config, name)?;
            set.insert(name.clone());
        }
        for g in groups {
            let members = ws.config.progen_groups.get(g).ok_or_else(|| {
                HiveError::usage(format!("unknown progen group '{g}'"))
            })?;
            for m in members {
                require_progen(&ws.config, m)?;
                set.insert(m.clone());
            }
        }
        set.into_iter().collect()
    };

    names
        .into_iter()
        .map(|name| scoped_from_config(&ws.root, &ws.config, &name))
        .collect()
}

/// Resolve exactly one Progen (sole configured or via `--progen`).
///
/// Used by single-root read and write paths. Error message is neutral (no “write”).
pub fn resolve_single_progen(
    ws: &Hive,
    progen: Option<&str>,
) -> Result<ScopedProgen, HiveError> {
    if ws.config.progens.is_empty() {
        return Err(HiveError::usage(
            "no progens configured (add one with `odm progen add`)",
        ));
    }
    let name = match progen {
        Some(n) => {
            require_progen(&ws.config, n)?;
            n.to_string()
        }
        None if ws.config.progens.len() == 1 => ws
            .config
            .progens
            .keys()
            .next()
            .cloned()
            .expect("len == 1"),
        None => {
            return Err(HiveError::usage(
                "requires --progen <name> when multiple progens are configured",
            ));
        }
    };
    scoped_from_config(&ws.root, &ws.config, &name)
}

/// Resolve exactly one Progen for writes (or sole configured).
pub fn resolve_write_progen(
    ws: &Hive,
    progen: Option<&str>,
) -> Result<ScopedProgen, HiveError> {
    resolve_single_progen(ws, progen)
}

fn require_progen(config: &HiveConfig, name: &str) -> Result<(), HiveError> {
    if config.progens.contains_key(name) {
        Ok(())
    } else {
        Err(HiveError::usage(format!("unknown progen '{name}'")))
    }
}

/// Resolve a configured Progen name to an absolute [`ScopedProgen`].
pub fn scoped_from_config(
    root: &Path,
    config: &HiveConfig,
    name: &str,
) -> Result<ScopedProgen, HiveError> {
    let entry = config
        .progens
        .get(name)
        .ok_or_else(|| HiveError::usage(format!("unknown progen '{name}'")))?;
    Ok(ScopedProgen {
        name: name.to_string(),
        path: abs_checkout(root, &entry.path)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hive_core::{ProgenEntry, HiveConfig};
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    fn ws_with(
        root: PathBuf,
        progens: BTreeMap<String, ProgenEntry>,
        groups: BTreeMap<String, Vec<String>>,
    ) -> Hive {
        Hive {
            root,
            config: HiveConfig {
                progens,
                progen_groups: groups,
                ..Default::default()
            },
            actions: BTreeMap::new(),
            generators: BTreeMap::new(),
        }
    }

    #[test]
    fn default_read_all() {
        let dir = tempdir().unwrap();
        let mut p = BTreeMap::new();
        p.insert(
            "a".into(),
            ProgenEntry {
                path: "va".into(),
                url: None,
                branch: None,
                ..Default::default()
            },
        );
        p.insert(
            "b".into(),
            ProgenEntry {
                path: "vb".into(),
                url: None,
                branch: None,
                ..Default::default()
            },
        );
        let ws = ws_with(dir.path().to_path_buf(), p, BTreeMap::new());
        let s = resolve_read_scope(&ws, &[], &[]).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name, "a");
        assert_eq!(s[1].name, "b");
    }

    #[test]
    fn group_union() {
        let dir = tempdir().unwrap();
        let mut p = BTreeMap::new();
        p.insert(
            "a".into(),
            ProgenEntry {
                path: "va".into(),
                url: None,
                branch: None,
                ..Default::default()
            },
        );
        p.insert(
            "b".into(),
            ProgenEntry {
                path: "vb".into(),
                url: None,
                branch: None,
                ..Default::default()
            },
        );
        p.insert(
            "c".into(),
            ProgenEntry {
                path: "vc".into(),
                url: None,
                branch: None,
                ..Default::default()
            },
        );
        let mut g = BTreeMap::new();
        g.insert("prod".into(), vec!["a".into(), "b".into()]);
        let ws = ws_with(dir.path().to_path_buf(), p, g);
        let s = resolve_read_scope(&ws, &[], &["prod".into()]).unwrap();
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn write_needs_flag_when_multi() {
        let dir = tempdir().unwrap();
        let mut p = BTreeMap::new();
        p.insert(
            "a".into(),
            ProgenEntry {
                path: "va".into(),
                url: None,
                branch: None,
                ..Default::default()
            },
        );
        p.insert(
            "b".into(),
            ProgenEntry {
                path: "vb".into(),
                url: None,
                branch: None,
                ..Default::default()
            },
        );
        let ws = ws_with(dir.path().to_path_buf(), p, BTreeMap::new());
        let err = resolve_single_progen(&ws, None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("requires --progen"), "{msg}");
        assert!(!msg.contains("write"), "{msg}");
        assert!(resolve_write_progen(&ws, None).is_err());
        assert_eq!(resolve_write_progen(&ws, Some("a")).unwrap().name, "a");
    }
}
