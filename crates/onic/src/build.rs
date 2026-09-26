use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::mpsc::channel;
use std::time::{SystemTime, UNIX_EPOCH};

use notify::{Event, EventKind, RecursiveMode, Watcher};

use crate::error::{OnicError, OnicResult};
use crate::extract::{extract_file, materialize_graph};
use crate::types::{BuildMeta, BuildReport, ExtractedGraph, Manifests, EXTRACT_VERSION};
use crate::walk::{absolute, is_default_ignored, walk_project};
use crate::write::{inspect_cache, replace_file, CachePlan};

pub fn build_project(root: &Path, db_path: &Path) -> OnicResult<BuildReport> {
    let root = absolute(root);
    let root_stat = match fs::metadata(&root) {
        Ok(meta) => meta,
        Err(_) => {
            return Err(OnicError::msg(format!("no project at {}", root.display())));
        }
    };
    if !root_stat.is_dir() {
        return Err(OnicError::msg(format!(
            "not a directory: {}",
            root.display()
        )));
    }
    let files = walk_project(&root)?;
    let cached = match plan_build(db_path, &files) {
        CachePlan::Reuse {
            file_count,
            node_count,
            edge_count,
        } => {
            return Ok(BuildReport {
                db_path: db_path.to_path_buf(),
                file_count,
                node_count,
                edge_count,
                reused: true,
            });
        }
        CachePlan::Rebuild { cached } => cached,
    };
    let known = files
        .iter()
        .map(|file| file.path.clone())
        .collect::<HashSet<_>>();
    let manifests = Manifests {
        go: crate::lang_go::collect_go_manifests(&root, &files),
        drac: crate::lang_drac::collect_drac_manifests(&root, &files),
    };
    let mut extracts: HashMap<String, ExtractedGraph> = HashMap::new();
    for file in &files {
        if let Some(part) = cached.get(&file.path) {
            extracts.insert(file.path.clone(), part.clone());
        } else {
            extracts.insert(file.path.clone(), extract_file(file, &manifests));
        }
    }
    let mut parts = Vec::with_capacity(files.len());
    for file in &files {
        let part = extracts
            .get(&file.path)
            .cloned()
            .ok_or_else(|| OnicError::msg(format!("missing extract for {}", file.path)))?;
        parts.push(part);
    }
    let graph = materialize_graph(parts, &known, &manifests);
    replace_file(
        db_path,
        &files,
        &graph,
        &BuildMeta {
            root: root.display().to_string(),
            built_at: iso_now(),
            file_count: files.len(),
            extract_version: EXTRACT_VERSION.to_string(),
        },
        &extracts,
    )?;
    Ok(BuildReport {
        db_path: db_path.to_path_buf(),
        file_count: files.len(),
        node_count: graph.nodes.len(),
        edge_count: graph.edges.len(),
        reused: false,
    })
}

pub fn watch_project(root: &Path, db_path: &Path) -> OnicResult<()> {
    let root = absolute(root);
    let db_path = db_path.to_path_buf();
    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = tx.send(event);
    })
    .map_err(|err| OnicError::msg(err.to_string()))?;
    watcher
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|err| OnicError::msg(err.to_string()))?;
    loop {
        let event = rx.recv().map_err(|err| OnicError::msg(err.to_string()))?;
        match event {
            Err(err) => {
                eprintln!("{err}");
                return Err(OnicError::msg(err.to_string()));
            }
            Ok(event) => {
                if !event_triggers(&root, &event) {
                    continue;
                }
                loop {
                    if let Err(err) = build_project(&root, &db_path) {
                        eprintln!("{err}");
                    }
                    let mut dirty = false;
                    loop {
                        match rx.try_recv() {
                            Ok(Err(err)) => {
                                eprintln!("{err}");
                                return Err(OnicError::msg(err.to_string()));
                            }
                            Ok(Ok(next)) if event_triggers(&root, &next) => dirty = true,
                            Ok(Ok(_)) => {}
                            Err(_) => break,
                        }
                    }
                    if !dirty {
                        break;
                    }
                }
            }
        }
    }
}

fn plan_build(db_path: &Path, files: &[crate::types::ScannedFile]) -> CachePlan {
    if !db_path.exists() {
        return CachePlan::Rebuild {
            cached: HashMap::new(),
        };
    }
    inspect_cache(db_path, files).unwrap_or(CachePlan::Rebuild {
        cached: HashMap::new(),
    })
}

fn event_triggers(root: &Path, event: &Event) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    if event.paths.is_empty() {
        return true;
    }
    event
        .paths
        .iter()
        .any(|path| match path.strip_prefix(root) {
            Ok(rel) => {
                let rel = rel.to_string_lossy().replace('\\', "/");
                !rel.is_empty() && !is_default_ignored(&rel)
            }
            Err(_) => true,
        })
}

fn iso_now() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let tod = secs % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn writes_and_reuses() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.py"), "x = 1\n").unwrap();
        fs::write(root.join(".gitignore"), "skip/\n").unwrap();
        fs::create_dir_all(root.join("skip")).unwrap();
        fs::write(root.join("skip/b.py"), "y = 2\n").unwrap();
        let db = root.join(".onic").join("graph.db");
        let report = build_project(&root, &db).unwrap();
        assert!(!report.reused);
        assert_eq!(report.file_count, 1);
        assert!(report.node_count >= 1);
        let again = build_project(&root, &db).unwrap();
        assert!(again.reused);
        assert_eq!(again.file_count, report.file_count);
        assert_eq!(again.node_count, report.node_count);
        assert_eq!(again.edge_count, report.edge_count);
    }
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    (year, month as u32, day as u32)
}
