use std::path::Path;

use crate::dest_config::load_dest_config;
use crate::dest_journal::{append_history, gc_history, journal_line};
use crate::dest_match::match_notes;
use crate::dest_scan::scan;

pub fn explain(cwd: &Path) -> Result<String, String> {
    let config = load_dest_config(cwd)?;
    let scanned = scan(cwd, &config, "t", true)?;
    let matches = match_notes(&config.lanes, &scanned.notes, &config.disable, Some(cwd));
    let mut out = String::new();
    for m in &matches {
        journal_line("match", &m.lane.lane, &m.note.path, "trigger");
        let _ = append_history(
            config.history.as_ref().map(Path::new),
            "match",
            &m.lane.lane,
            &m.note.path,
        );
        out.push_str(&format!("{}\t{}\ttrigger\n", m.lane.lane, m.note.path));
    }
    if out.is_empty() {
        out.push_str("(no matches)\n");
    }
    Ok(out)
}

pub fn gc(cwd: &Path) -> Result<(), String> {
    let config = load_dest_config(cwd)?;
    let history = config.history.map(|h| cwd.join(h));
    gc_history(history.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn explain_prints_match_without_claiming() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".hivemind")).unwrap();
        fs::write(
            root.join(".hivemind/hivemind.yaml"),
            "folders:\n  - path: inbox\n    schema:\n      id: string\n      status: string\n    required: [id, status]\n  - path: quarantine\n    schema: quarantine\nlanes:\n  work:\n    type: single\n    trigger:\n      status: ready\n    cmd: [\"true\"]\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("inbox")).unwrap();
        fs::write(root.join("inbox/n.md"), "---\nid: a\nstatus: ready\n---\nbody\n").unwrap();
        let text = explain(root).unwrap();
        assert!(text.contains("work"));
        assert!(text.contains("inbox/n.md"));
        let note = fs::read_to_string(root.join("inbox/n.md")).unwrap();
        assert!(!note.contains("claimed-by"));
    }
}
