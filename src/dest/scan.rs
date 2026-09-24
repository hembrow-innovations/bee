use std::fs;
use std::path::{Path, PathBuf};

use serde_yaml::Value;

use crate::dest::config::{named_allowlist, DestConfig};
use crate::dest::note::{parse_front_matter, quarantine_note, ParseFrontMatter, YamlMap};

#[derive(Debug, Clone)]
pub struct ScannedNote {
    pub path: String,
    pub abs: PathBuf,
    pub front_matter: YamlMap,
}

#[derive(Debug, Clone)]
pub struct QuarantinedNote {
    pub path: String,
    pub fault: String,
}

#[derive(Debug, Clone)]
pub struct ScanResult {
    pub notes: Vec<ScannedNote>,
    pub quarantines: Vec<QuarantinedNote>,
}

enum FolderSchema {
    Named(String),
    Inline(std::collections::BTreeSet<String>),
}

struct FolderEntry {
    path: String,
    schema: FolderSchema,
    required: Vec<String>,
}

pub fn scan(cwd: &Path, config: &DestConfig, now: &str, readonly: bool) -> Result<ScanResult, String> {
    let folders = read_folders(&config.folders)?;
    let quarantine = folders
        .iter()
        .find(|f| is_quarantine(f))
        .ok_or("No quarantine folder configured")?;
    let dest_dir = cwd.join(&quarantine.path);
    let mut notes = Vec::new();
    let mut quarantines = Vec::new();
    for folder in &folders {
        if is_quarantine(folder) {
            continue;
        }
        if !include_folder(&folder.path, config.watch.as_deref()) {
            continue;
        }
        let dir = cwd.join(&folder.path);
        if !dir.is_dir() {
            continue;
        }
        for abs in list_markdown(&dir) {
            let origin = project_rel(cwd, &abs);
            let raw = fs::read_to_string(&abs).map_err(|e| e.to_string())?;
            if !raw.starts_with("---") {
                continue;
            }
            match parse_front_matter(&raw) {
                ParseFrontMatter::Fault(fault) => {
                    if !readonly {
                        quarantine_note(&abs, &dest_dir, &origin, fault, now);
                    }
                    quarantines.push(QuarantinedNote {
                        path: origin,
                        fault: fault.into(),
                    });
                }
                ParseFrontMatter::Ok(map) => {
                    if let Some(unknown) = unknown_key(&map, folder)? {
                        let fault = format!("unknown-key:{unknown}");
                        if !readonly {
                            quarantine_note(&abs, &dest_dir, &origin, &fault, now);
                        }
                        quarantines.push(QuarantinedNote { path: origin, fault });
                    } else if let Some(missing) = missing_key(&map, &folder.required) {
                        let fault = format!("missing-key:{missing}");
                        if !readonly {
                            quarantine_note(&abs, &dest_dir, &origin, &fault, now);
                        }
                        quarantines.push(QuarantinedNote { path: origin, fault });
                    } else {
                        notes.push(ScannedNote {
                            path: origin,
                            abs,
                            front_matter: map,
                        });
                    }
                }
            }
        }
    }
    Ok(ScanResult { notes, quarantines })
}

fn is_quarantine(folder: &FolderEntry) -> bool {
    matches!(&folder.schema, FolderSchema::Named(n) if n == "quarantine")
}

fn read_folders(folders: &[Value]) -> Result<Vec<FolderEntry>, String> {
    let mut out = Vec::new();
    for item in folders {
        let Value::Mapping(m) = item else {
            return Err("folders entries must be maps".into());
        };
        let path = m
            .get(Value::String("path".into()))
            .and_then(Value::as_str)
            .ok_or("folder path is required")?;
        let schema = read_schema(m.get(Value::String("schema".into())))?;
        let required = read_required(m.get(Value::String("required".into())))?;
        out.push(FolderEntry {
            path: path.into(),
            schema,
            required,
        });
    }
    Ok(out)
}

fn read_schema(value: Option<&Value>) -> Result<FolderSchema, String> {
    match value {
        Some(Value::String(name)) if !name.is_empty() => {
            named_allowlist(name)?;
            Ok(FolderSchema::Named(name.clone()))
        }
        Some(Value::Mapping(m)) => Ok(FolderSchema::Inline(
            m.keys()
                .filter_map(|k| k.as_str().map(str::to_string))
                .collect(),
        )),
        _ => Err("folder schema is required".into()),
    }
}

fn read_required(value: Option<&Value>) -> Result<Vec<String>, String> {
    match value {
        None => Ok(vec![]),
        Some(Value::Sequence(s)) => s
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "folder required must be a list of strings".to_string())
            })
            .collect(),
        _ => Err("folder required must be a list of strings".into()),
    }
}

fn missing_key(map: &YamlMap, required: &[String]) -> Option<String> {
    required
        .iter()
        .find(|k| !map.contains_key(Value::String((*k).clone())))
        .cloned()
}

fn unknown_key(map: &YamlMap, folder: &FolderEntry) -> Result<Option<String>, String> {
    let allowed = match &folder.schema {
        FolderSchema::Inline(s) => s.clone(),
        FolderSchema::Named(n) => named_allowlist(n)?,
    };
    for key in map.keys() {
        let Some(k) = key.as_str() else {
            continue;
        };
        if k == "claimed-at" || k == "claimed-by" {
            continue;
        }
        if !allowed.contains(k) {
            return Ok(Some(k.into()));
        }
    }
    Ok(None)
}

fn include_folder(folder_path: &str, watch: Option<&[String]>) -> bool {
    let Some(watch) = watch else {
        return true;
    };
    if watch.is_empty() {
        return true;
    }
    let folder = normalize(folder_path);
    watch.iter().any(|root| {
        let w = normalize(root);
        folder == w || folder.starts_with(&format!("{w}/")) || w.starts_with(&format!("{folder}/"))
    })
}

fn list_markdown(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(dir) else {
        return out;
    };
    let mut ents: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    ents.sort_by_key(|e| e.file_name());
    for ent in ents {
        let abs = ent.path();
        if abs.is_dir() {
            out.extend(list_markdown(&abs));
        } else if abs.extension().and_then(|s| s.to_str()) == Some("md") {
            out.push(abs);
        }
    }
    out
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_string()
}

fn project_rel(cwd: &Path, abs: &Path) -> String {
    abs.strip_prefix(cwd)
        .unwrap_or(abs)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dest::config::load_dest_config;
    use std::fs;
    use tempfile::tempdir;

    fn write_cfg(root: &Path, extra_folders: &str) {
        fs::create_dir_all(root.join(".hivemind")).unwrap();
        fs::write(
            root.join(".hivemind/hivemind.yaml"),
            format!(
                "folders:\n  - path: inbox\n    schema:\n      id: string\n      status: string\n    required: [id, status]\n  - path: quarantine\n    schema: quarantine\n{extra_folders}lanes: {{}}\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn unknown_key_quarantines_with_origin() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_cfg(root, "");
        fs::create_dir_all(root.join("inbox")).unwrap();
        fs::write(
            root.join("inbox/n.md"),
            "---\nid: a\nstatus: ready\nextra: 1\n---\n",
        )
        .unwrap();
        let cfg = load_dest_config(root).unwrap();
        let result = scan(root, &cfg, "t", false).unwrap();
        assert_eq!(result.notes.len(), 0);
        assert_eq!(result.quarantines[0].fault, "unknown-key:extra");
        let q = fs::read_to_string(root.join("quarantine/n.md")).unwrap();
        assert!(q.contains("origin-location: inbox/n.md"));
        assert!(!q.contains("extra:"));
        assert!(!q.contains("status:"));
    }

    #[test]
    fn missing_key_quarantines_and_scan_continues() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_cfg(root, "");
        fs::create_dir_all(root.join("inbox")).unwrap();
        fs::write(root.join("inbox/bad.md"), "---\nid: a\n---\n").unwrap();
        fs::write(
            root.join("inbox/good.md"),
            "---\nid: b\nstatus: ready\n---\n",
        )
        .unwrap();
        let cfg = load_dest_config(root).unwrap();
        let result = scan(root, &cfg, "t", false).unwrap();
        assert_eq!(result.notes.len(), 1);
        assert_eq!(result.notes[0].path, "inbox/good.md");
        assert!(result
            .quarantines
            .iter()
            .any(|q| q.fault == "missing-key:status"));
    }
}
