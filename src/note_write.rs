use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_yaml::Value;

use crate::dest_note::{parse_front_matter, ParseFrontMatter, YamlMap};
use crate::{hive_root, lookup_notes, NotesDirs};

struct TrackerNote {
    id: String,
    abs: PathBuf,
    kind: Option<String>,
    status: Option<String>,
    blocked_by: Vec<String>,
    title: Option<String>,
}

pub fn iso_now() -> String {
    iso_stamp(SystemTime::now())
}

pub fn iso_stamp(now: SystemTime) -> String {
    let secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let days = (secs / 86400) as i64;
    let tod = secs % 86400;
    let (y, m, d) = civil_from_days(days);
    let h = tod / 3600;
    let min = (tod % 3600) / 60;
    let s = tod % 60;
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}Z")
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

fn legal(kind: &str) -> Option<&'static [&'static str]> {
    Some(match kind {
        "intent" => &["active", "superseded"],
        "roadmap" => &["draft", "active"],
        "location" => &["active", "done"],
        "sprint" => &["shaping", "active", "review", "closed"],
        "slice" => &["shaping", "frozen", "active", "met", "abandoned"],
        "ticket" => &["open", "parked", "promoted", "dropped", "closed"],
        "task" => &["draft", "ready", "claimed", "implemented", "completed"],
        "round" => &[
            "awaiting-answers",
            "ready-to-resume",
            "awaiting-confirm",
            "published",
            "parked",
        ],
        _ => return None,
    })
}

fn is_known_status(status: &str) -> bool {
    [
        "intent", "roadmap", "location", "sprint", "slice", "ticket", "task", "round",
    ]
    .iter()
    .filter_map(|k| legal(k))
    .any(|vals| vals.contains(&status))
}

fn yaml_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn map_str(map: &YamlMap, key: &str) -> Option<String> {
    map.get(Value::String(key.into())).and_then(yaml_str)
}

fn map_list(map: &YamlMap, key: &str) -> Vec<String> {
    match map.get(Value::String(key.into())) {
        Some(Value::Sequence(seq)) => seq.iter().filter_map(yaml_str).collect(),
        Some(v) => yaml_str(v).into_iter().collect(),
        None => vec![],
    }
}

fn note_id(rel: &str, base: &str) -> String {
    let norm = rel.replace('\\', "/");
    let parts: Vec<&str> = norm.split('/').collect();
    if base == "shape.md" && parts.len() >= 2 {
        return parts[parts.len() - 2].to_string();
    }
    base.strip_suffix(".md").unwrap_or(base).to_string()
}

fn collect(abs_dir: &Path, root: &Path, out: &mut Vec<TrackerNote>) {
    let Ok(entries) = fs::read_dir(abs_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect(&path, root, out);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let base = entry.file_name().to_string_lossy().into_owned();
        if !base.ends_with(".md") || base == "index.md" {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let fields = match parse_front_matter(&raw) {
            ParseFrontMatter::Ok(map) => map,
            ParseFrontMatter::Fault(_) => YamlMap::new(),
        };
        let rel = path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| base.clone());
        out.push(TrackerNote {
            id: note_id(&rel, &base),
            abs: path,
            kind: map_str(&fields, "kind"),
            status: map_str(&fields, "status"),
            blocked_by: map_list(&fields, "blocked_by"),
            title: map_str(&fields, "title"),
        });
    }
}

fn load_live(dirs: &NotesDirs, root: &Path) -> Vec<TrackerNote> {
    let mut out = Vec::new();
    collect(&dirs.planning, root, &mut out);
    out
}

fn load_all(dirs: &NotesDirs, root: &Path) -> Vec<TrackerNote> {
    let mut out = load_live(dirs, root);
    if let Some(archive) = &dirs.archive {
        collect(archive, root, &mut out);
    }
    out
}

fn find_live<'a>(notes: &'a [TrackerNote], id: &str) -> Option<&'a TrackerNote> {
    notes.iter().find(|n| n.id == id)
}

fn set_fields(text: &str, patch: &[(&str, &str)]) -> Option<String> {
    if !text.starts_with("---\n") {
        return None;
    }
    let rel = text[4..].find("\n---")?;
    let end = rel + 4;
    let mut yaml = text[4..end].to_string();
    let rest = text.get(end + 4..)?;
    for (key, value) in patch {
        let line = format!("{key}: {value}");
        let needle = format!("{key}:");
        let mut replaced = false;
        let mut out = String::new();
        for existing in yaml.lines() {
            if !out.is_empty() {
                out.push('\n');
            }
            if !replaced && existing.starts_with(&needle) {
                out.push_str(&line);
                replaced = true;
            } else {
                out.push_str(existing);
            }
        }
        yaml = if replaced {
            out
        } else {
            format!("{}\n{line}\n", yaml.trim_end())
        };
    }
    Some(format!("---\n{}\n---{rest}", yaml.trim_end()))
}

fn quoted(now: &str) -> String {
    format!("\"{now}\"")
}

fn is_sprint_shape(note: &TrackerNote) -> bool {
    note.abs.file_name().and_then(|n| n.to_str()) == Some("shape.md")
        && note
            .abs
            .to_string_lossy()
            .replace('\\', "/")
            .contains("/sprints/")
}

fn file_dest(archive: &Path, note: &TrackerNote) -> Option<PathBuf> {
    let base = note.abs.file_name()?;
    let kind = note.kind.as_deref()?;
    let status = note.status.as_deref()?;
    let folder = match (kind, status) {
        ("task", "completed") => "tasks",
        ("ticket", "closed" | "dropped") => "tickets",
        ("location", "done") => "locations",
        ("round", "published") => "rounds",
        _ => return None,
    };
    Some(archive.join("planning").join(folder).join(base))
}

fn append_index(archive: &Path, id: &str, title: &str) {
    let path = archive.join("index.md");
    let header = "# Archive\n\nOne-liners of what landed. Newest first.\n";
    let text = fs::read_to_string(&path).unwrap_or_else(|_| header.to_string());
    if text.contains(&format!("**{id}**")) {
        return;
    }
    let line = format!("- **{id}**: {title}");
    let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    let at = lines.iter().position(|item| item.starts_with("- **"));
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let next = match at {
        None => format!("{}\n\n{line}\n", text.trim_end()),
        Some(i) => {
            lines.insert(i, line);
            lines.join("\n")
        }
    };
    let _ = fs::write(path, next);
}

fn move_path(
    src: &Path,
    dest: &Path,
    archive: &Path,
    id: &str,
    title: &str,
    moved: &mut Vec<String>,
) {
    if !src.exists() || dest.exists() {
        return;
    }
    if let Some(parent) = dest.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if fs::rename(src, dest).is_ok() {
        append_index(archive, id, title);
        moved.push(id.to_string());
    }
}

fn housekeep_dirs(root: &Path, dirs: &NotesDirs) -> Vec<String> {
    let Some(archive) = &dirs.archive else {
        return vec![];
    };
    let live = load_live(dirs, root);
    let mut moved = Vec::new();
    for note in &live {
        if is_sprint_shape(note) && note.status.as_deref() == Some("closed") {
            if let Some(src) = note.abs.parent() {
                let dest = archive.join("planning").join("sprints").join(&note.id);
                let title = note.title.clone().unwrap_or_else(|| note.id.clone());
                move_path(src, &dest, archive, &note.id, &title, &mut moved);
            }
        }
    }
    let live = load_live(dirs, root);
    for note in &live {
        if let Some(dest) = file_dest(archive, note) {
            let title = note.title.clone().unwrap_or_else(|| note.id.clone());
            move_path(&note.abs, &dest, archive, &note.id, &title, &mut moved);
        }
    }
    moved
}

pub fn housekeep(start: &Path) -> Result<Vec<String>, String> {
    let dirs = lookup_notes(start)?;
    let root = hive_root(start)?;
    Ok(housekeep_dirs(&root, &dirs))
}

pub fn claim(start: &Path, id: &str, now: &str) -> Result<String, String> {
    let dirs = lookup_notes(start)?;
    let root = hive_root(start)?;
    let catalog = load_all(&dirs, &root);
    let live: Vec<&TrackerNote> = catalog
        .iter()
        .filter(|n| n.abs.starts_with(&dirs.planning))
        .collect();
    let note = live
        .iter()
        .find(|n| n.id == id)
        .ok_or_else(|| format!("unknown id: {id}"))?;
    if note.kind.as_deref() != Some("task") {
        return Err(format!("not a task: {id}"));
    }
    if note.status.as_deref() != Some("ready") {
        return Err(format!("not ready: {id}"));
    }
    let archived: Vec<&TrackerNote> = catalog
        .iter()
        .filter(|n| !n.abs.starts_with(&dirs.planning))
        .collect();
    for blocker in &note.blocked_by {
        let status = live
            .iter()
            .find(|n| n.id == *blocker)
            .or_else(|| archived.iter().find(|n| n.id == *blocker))
            .and_then(|n| n.status.as_deref());
        if status != Some("completed") {
            return Err(format!("blocked by {blocker}"));
        }
    }
    let abs = note.abs.clone();
    let raw = fs::read_to_string(&abs).map_err(|e| e.to_string())?;
    let stamp = quoted(now);
    let next = set_fields(&raw, &[("status", "claimed"), ("updated_at", &stamp)])
        .ok_or_else(|| format!("not a task: {id}"))?;
    fs::write(&abs, next).map_err(|e| e.to_string())?;
    housekeep(start)?;
    Ok(id.to_string())
}

pub fn set_status(start: &Path, id: &str, status: &str, now: &str) -> Result<String, String> {
    if !is_known_status(status) {
        return Err(format!("unknown status: {status}"));
    }
    let dirs = lookup_notes(start)?;
    let root = hive_root(start)?;
    let live = load_live(&dirs, &root);
    let note = find_live(&live, id).ok_or_else(|| format!("unknown id: {id}"))?;
    let kind = note.kind.as_deref().unwrap_or("unknown");
    if legal(kind).map(|vals| vals.contains(&status)) != Some(true) {
        return Err(format!("illegal status {status} for {kind}"));
    }
    let abs = note.abs.clone();
    let raw = fs::read_to_string(&abs).map_err(|e| e.to_string())?;
    let stamp = quoted(now);
    let next = set_fields(&raw, &[("status", status), ("updated_at", &stamp)])
        .ok_or_else(|| format!("unknown id: {id}"))?;
    fs::write(&abs, next).map_err(|e| e.to_string())?;
    housekeep(start)?;
    Ok(id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Commands, NoteCmd};
    use crate::execute;
    use std::time::Duration;
    use tempfile::tempdir;

    const NOW_TEST: &str = "2026-09-13T12:00:00Z";

    fn write_hive(root: &Path) {
        fs::create_dir_all(root.join(".hivemind")).unwrap();
        fs::write(
            root.join(".hivemind/hivemind.yaml"),
            "lanes: {}\nnotes:\n  planning: .heio/planning\n  archive: .heio/archive\n",
        )
        .unwrap();
    }

    fn write_rel(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn task(id: &str, status: &str, extra: &str) -> String {
        format!(
            "---\nid: \"{id}\"\ntitle: \"{id}\"\nkind: task\nstatus: {status}\nmode: afk\n{extra}updated_at: \"2026-09-12T00:00:00Z\"\n---\n"
        )
    }

    fn cli(cmd: NoteCmd) -> Cli {
        Cli {
            project: None,
            wt: vec![],
            command: Commands::Note { cmd },
        }
    }

    #[test]
    fn iso_stamp_is_utc_without_millis() {
        assert_eq!(iso_stamp(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(
            iso_stamp(UNIX_EPOCH + Duration::from_secs(1_000_000_000)),
            "2001-09-09T01:46:40Z"
        );
        assert_eq!(
            iso_stamp(UNIX_EPOCH + Duration::from_secs(1_789_300_800)),
            "2026-09-13T12:00:00Z"
        );
    }

    #[test]
    fn claim_ready_unblocked_task() {
        let dir = tempdir().unwrap();
        write_hive(dir.path());
        write_rel(
            dir.path(),
            ".heio/planning/tasks/task-02-foo.md",
            &task("task-02-foo", "ready", "blocked_by: []\n"),
        );
        assert_eq!(
            claim(dir.path(), "task-02-foo", NOW_TEST).unwrap(),
            "task-02-foo"
        );
        let text =
            fs::read_to_string(dir.path().join(".heio/planning/tasks/task-02-foo.md")).unwrap();
        assert!(text.contains("status: claimed"), "{text}");
        assert!(
            text.contains("updated_at: \"2026-09-13T12:00:00Z\""),
            "{text}"
        );
        assert!(!text.contains("claimed-by"), "{text}");
    }

    #[test]
    fn claim_ignores_mode() {
        let dir = tempdir().unwrap();
        write_hive(dir.path());
        write_rel(
            dir.path(),
            ".heio/planning/tasks/task-02-foo.md",
            &task("task-02-foo", "ready", "mode: hitl\nblocked_by: []\n")
                .replace("mode: afk\n", ""),
        );
        assert!(claim(dir.path(), "task-02-foo", NOW_TEST).is_ok());
    }

    #[test]
    fn claim_requires_ready_live_unblocked_task() {
        let dir = tempdir().unwrap();
        write_hive(dir.path());
        write_rel(
            dir.path(),
            ".heio/planning/tickets/ticket-01-a.md",
            "---\nkind: ticket\nstatus: open\n---\n",
        );
        write_rel(
            dir.path(),
            ".heio/planning/tasks/task-02-draft.md",
            &task("task-02-draft", "draft", "blocked_by: []\n"),
        );
        write_rel(
            dir.path(),
            ".heio/planning/tasks/task-03-wait.md",
            &task("task-03-wait", "ready", "blocked_by:\n  - task-01-gone\n"),
        );
        write_rel(
            dir.path(),
            ".heio/planning/tasks/task-01-a.md",
            &task("task-01-a", "ready", "blocked_by: []\n"),
        );
        write_rel(
            dir.path(),
            ".heio/planning/tasks/task-04-wait.md",
            &task("task-04-wait", "ready", "blocked_by:\n  - task-01-a\n"),
        );
        assert!(claim(dir.path(), "ticket-01-a", NOW_TEST)
            .unwrap_err()
            .contains("not a task"));
        assert!(claim(dir.path(), "task-02-draft", NOW_TEST)
            .unwrap_err()
            .contains("not ready"));
        assert!(claim(dir.path(), "task-99-gone", NOW_TEST)
            .unwrap_err()
            .contains("unknown id"));
        assert!(claim(dir.path(), "task-03-wait", NOW_TEST)
            .unwrap_err()
            .contains("blocked by"));
        assert!(claim(dir.path(), "task-04-wait", NOW_TEST)
            .unwrap_err()
            .contains("blocked by"));
    }

    #[test]
    fn claim_archived_completed_blocker_unblocks() {
        let dir = tempdir().unwrap();
        write_hive(dir.path());
        write_rel(
            dir.path(),
            ".heio/archive/planning/tasks/task-01-a.md",
            &task("task-01-a", "completed", "blocked_by: []\n"),
        );
        write_rel(
            dir.path(),
            ".heio/planning/tasks/task-02-foo.md",
            &task("task-02-foo", "ready", "blocked_by:\n  - task-01-a\n"),
        );
        assert!(claim(dir.path(), "task-02-foo", NOW_TEST).is_ok());
    }

    #[test]
    fn claim_live_completed_blocker_housekeeps() {
        let dir = tempdir().unwrap();
        write_hive(dir.path());
        write_rel(
            dir.path(),
            ".heio/planning/tasks/task-01-a.md",
            &task("task-01-a", "completed", "blocked_by: []\n"),
        );
        write_rel(
            dir.path(),
            ".heio/planning/tasks/task-02-foo.md",
            &task("task-02-foo", "ready", "blocked_by:\n  - task-01-a\n"),
        );
        assert!(claim(dir.path(), "task-02-foo", NOW_TEST).is_ok());
        assert!(!dir
            .path()
            .join(".heio/planning/tasks/task-01-a.md")
            .exists());
        assert!(dir
            .path()
            .join(".heio/archive/planning/tasks/task-01-a.md")
            .exists());
    }

    #[test]
    fn set_status_writes_legal_token_and_housekeeps() {
        let dir = tempdir().unwrap();
        write_hive(dir.path());
        write_rel(
            dir.path(),
            ".heio/planning/tasks/task-02-foo.md",
            "---\nkind: task\nstatus: claimed\nupdated_at: \"2026-09-12T00:00:00Z\"\n---\n",
        );
        assert!(set_status(dir.path(), "task-02-foo", "implemented", NOW_TEST).is_ok());
        let text =
            fs::read_to_string(dir.path().join(".heio/planning/tasks/task-02-foo.md")).unwrap();
        assert!(text.contains("status: implemented"), "{text}");
        assert!(
            text.contains("updated_at: \"2026-09-13T12:00:00Z\""),
            "{text}"
        );
    }

    #[test]
    fn set_status_rejects_unknown_and_illegal() {
        let dir = tempdir().unwrap();
        write_hive(dir.path());
        write_rel(
            dir.path(),
            ".heio/planning/tasks/task-02-foo.md",
            "---\nkind: task\nstatus: ready\n---\n",
        );
        assert!(set_status(dir.path(), "task-99-gone", "ready", NOW_TEST)
            .unwrap_err()
            .contains("unknown id"));
        assert!(set_status(dir.path(), "task-02-foo", "banana", NOW_TEST)
            .unwrap_err()
            .contains("unknown status"));
        assert!(set_status(dir.path(), "task-02-foo", "open", NOW_TEST)
            .unwrap_err()
            .contains("illegal status"));
    }

    #[test]
    fn set_status_completed_archives() {
        let dir = tempdir().unwrap();
        write_hive(dir.path());
        write_rel(
            dir.path(),
            ".heio/planning/tasks/task-02-foo.md",
            "---\nkind: task\nstatus: implemented\ntitle: \"ship it\"\nupdated_at: \"2026-09-12T00:00:00Z\"\n---\n",
        );
        assert!(set_status(dir.path(), "task-02-foo", "completed", NOW_TEST).is_ok());
        assert!(!dir
            .path()
            .join(".heio/planning/tasks/task-02-foo.md")
            .exists());
        assert!(dir
            .path()
            .join(".heio/archive/planning/tasks/task-02-foo.md")
            .exists());
        let index = fs::read_to_string(dir.path().join(".heio/archive/index.md")).unwrap();
        assert!(index.contains("**task-02-foo**: ship it"), "{index}");
    }

    #[test]
    fn slice_met_is_not_archived() {
        let dir = tempdir().unwrap();
        write_hive(dir.path());
        write_rel(
            dir.path(),
            ".heio/planning/sprints/week-1/slice-01-a.md",
            "---\nkind: slice\nstatus: active\n---\n",
        );
        assert!(set_status(dir.path(), "slice-01-a", "met", NOW_TEST).is_ok());
        assert!(dir
            .path()
            .join(".heio/planning/sprints/week-1/slice-01-a.md")
            .exists());
    }

    #[test]
    fn missing_notes_planning_fails() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".hivemind")).unwrap();
        fs::write(dir.path().join(".hivemind/hivemind.yaml"), "lanes: {}\n").unwrap();
        let err = claim(dir.path(), "task-02-foo", NOW_TEST).unwrap_err();
        assert!(err.contains("notes.planning"), "{err}");
    }

    #[test]
    fn execute_claim_and_status() {
        let dir = tempdir().unwrap();
        write_hive(dir.path());
        write_rel(
            dir.path(),
            ".heio/planning/tasks/task-02-foo.md",
            &task("task-02-foo", "ready", "blocked_by: []\n"),
        );
        assert_eq!(
            execute(
                cli(NoteCmd::Claim {
                    id: "task-02-foo".into()
                }),
                dir.path()
            ),
            0
        );
        assert_eq!(
            execute(
                cli(NoteCmd::Status {
                    id: "task-02-foo".into(),
                    status: "implemented".into()
                }),
                dir.path()
            ),
            0
        );
        let text =
            fs::read_to_string(dir.path().join(".heio/planning/tasks/task-02-foo.md")).unwrap();
        assert!(text.contains("status: implemented"), "{text}");
    }
}
