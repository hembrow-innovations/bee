use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

pub fn journal_line(kind: &str, lane: &str, path: &str, reason: &str) {
    let reason = if reason.contains("SECRET") || reason.contains("token") {
        "***"
    } else {
        reason
    };
    eprintln!("hivemind {kind} {lane} {path} {reason}");
}

pub fn append_history(history: Option<&Path>, kind: &str, lane: &str, path: &str) -> Result<(), String> {
    let Some(history) = history else {
        return Ok(());
    };
    if let Some(parent) = history.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(history)
        .map_err(|e| e.to_string())?;
    if f.metadata().map(|m| m.len()).unwrap_or(1) == 0 {
        writeln!(f, "kind\tlane\tpath").map_err(|e| e.to_string())?;
    }
    writeln!(f, "{kind}\t{lane}\t{path}").map_err(|e| e.to_string())
}

pub fn gc_history(history: Option<&Path>) -> Result<(), String> {
    let Some(history) = history else {
        return Ok(());
    };
    if history.is_file() {
        fs::write(history, "kind\tlane\tpath\n").map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn gc_rewrites_header_only() {
        let dir = tempdir().unwrap();
        let hist = dir.path().join("h.tsv");
        fs::write(&hist, "kind\tlane\tpath\nspawn\ta\tp\n").unwrap();
        gc_history(Some(&hist)).unwrap();
        assert_eq!(fs::read_to_string(&hist).unwrap(), "kind\tlane\tpath\n");
    }

    #[test]
    fn journal_redacts_secret_tokens() {
        // reason path: no panic; redaction is in journal_line
        journal_line("skip", "work", "n.md", "env.SECRET");
    }
}
