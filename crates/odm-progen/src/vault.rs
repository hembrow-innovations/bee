use std::fs;
use std::path::{Path, PathBuf};

use odm_core::{abs_checkout, OdmError, Workspace};

use crate::note::{parse_markdown, NoteDoc};
use crate::scope::ScopedProgen;

/// Absolute path to a Progen vault root.
pub fn vault_path(ws: &Workspace, name: &str) -> Result<PathBuf, OdmError> {
    let entry = ws
        .config
        .progens
        .get(name)
        .ok_or_else(|| OdmError::usage(format!("unknown progen '{name}'")))?;
    abs_checkout(&ws.root, &entry.path)
}

#[derive(Debug, Clone)]
pub struct VaultInfo {
    pub name: String,
    pub path: PathBuf,
    pub on_disk: bool,
    pub note_count: usize,
    pub has_obsidian: bool,
}

/// Ensure a local vault directory exists and is Obsidian-openable.
/// Creates `README.md` and minimal `.obsidian/app.json` when missing.
/// Does not overwrite an existing `.obsidian/`.
pub fn ensure_vault(path: &Path) -> Result<(), OdmError> {
    fs::create_dir_all(path).map_err(|e| {
        OdmError::operation(format!("failed to create vault {}: {e}", path.display()))
    })?;

    let readme = path.join("README.md");
    if !readme.exists() {
        fs::write(
            &readme,
            "# Vault\n\nObsidian-compatible Progen store managed by ODM.\n",
        )
        .map_err(|e| OdmError::operation(format!("write README: {e}")))?;
    }

    let obsidian = path.join(".obsidian");
    if !obsidian.exists() {
        fs::create_dir_all(&obsidian).map_err(|e| {
            OdmError::operation(format!("create .obsidian: {e}"))
        })?;
        let app = obsidian.join("app.json");
        if !app.exists() {
            fs::write(&app, "{\n  \"legacyEditor\": false\n}\n").map_err(|e| {
                OdmError::operation(format!("write .obsidian/app.json: {e}"))
            })?;
        }
    }

    // Obsidian + git: ignore nothing mandatory; engine index lives under .odm/
    let gitignore = path.join(".gitignore");
    if !gitignore.exists() {
        fs::write(&gitignore, ".obsidian/workspace.json\n.obsidian/workspace-mobile.json\n.trash/\n")
            .map_err(|e| OdmError::operation(format!("write vault .gitignore: {e}")))?;
    }

    Ok(())
}

/// Walk all `.md` files under vault (skip `.obsidian`, `.git`, dot-dirs).
pub fn walk_notes(vault: &Path) -> Result<Vec<NoteDoc>, OdmError> {
    if !vault.is_dir() {
        return Err(OdmError::not_found(format!(
            "progen vault missing: {}",
            vault.display()
        )));
    }
    let mut out = Vec::new();
    walk_dir(vault, vault, &mut out)?;
    out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(out)
}

fn walk_dir(root: &Path, dir: &Path, out: &mut Vec<NoteDoc>) -> Result<(), OdmError> {
    let entries = fs::read_dir(dir).map_err(|e| {
        OdmError::operation(format!("read {}: {e}", dir.display()))
    })?;
    for ent in entries {
        let ent = ent.map_err(|e| OdmError::operation(e.to_string()))?;
        let path = ent.path();
        let name = ent.file_name();
        let name_s = name.to_string_lossy();
        if name_s.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk_dir(root, &path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let text = fs::read_to_string(&path).map_err(|e| {
                OdmError::operation(format!("read {}: {e}", path.display()))
            })?;
            out.push(parse_markdown(&rel, &path, &text)?);
        }
    }
    Ok(())
}

pub fn vault_info(sp: &ScopedProgen) -> Result<VaultInfo, OdmError> {
    let on_disk = sp.path.is_dir();
    let (note_count, has_obsidian) = if on_disk {
        let notes = walk_notes(&sp.path).unwrap_or_default();
        (notes.len(), sp.path.join(".obsidian").is_dir())
    } else {
        (0, false)
    };
    Ok(VaultInfo {
        name: sp.name.clone(),
        path: sp.path.clone(),
        on_disk,
        note_count,
        has_obsidian,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn ensure_and_walk() {
        let d = tempdir().unwrap();
        let v = d.path().join("vault");
        ensure_vault(&v).unwrap();
        assert!(v.join("README.md").is_file());
        assert!(v.join(".obsidian/app.json").is_file());
        fs::write(v.join("note.md"), "---\nid: x\n---\nHi [[Y]]\n").unwrap();
        let notes = walk_notes(&v).unwrap();
        assert!(notes.iter().any(|n| n.id.as_str() == "x"));
    }

    #[test]
    fn walk_nested_dirs() {
        let d = tempdir().unwrap();
        let v = d.path().join("vault");
        fs::create_dir_all(v.join("deep/nested")).unwrap();
        fs::write(v.join("root.md"), "---\nid: root\n---\nR\n").unwrap();
        fs::write(v.join("deep/nested/leaf.md"), "---\nid: leaf\n---\nL\n").unwrap();
        let notes = walk_notes(&v).unwrap();
        let ids: Vec<_> = notes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"root"));
        assert!(ids.contains(&"leaf"));
        assert!(notes.iter().any(|n| n.rel_path == "deep/nested/leaf.md"));
    }

    #[test]
    fn walk_skips_dot_dirs() {
        let d = tempdir().unwrap();
        let v = d.path().join("vault");
        fs::create_dir_all(v.join(".obsidian")).unwrap();
        fs::create_dir_all(v.join(".git/objects")).unwrap();
        fs::create_dir_all(v.join(".trash")).unwrap();
        fs::create_dir_all(v.join("keep")).unwrap();
        fs::write(v.join(".obsidian/secret.md"), "---\nid: obs\n---\n\n").unwrap();
        fs::write(v.join(".git/objects/g.md"), "---\nid: git\n---\n\n").unwrap();
        fs::write(v.join(".trash/t.md"), "---\nid: trash\n---\n\n").unwrap();
        fs::write(v.join("keep/k.md"), "---\nid: keep\n---\n\n").unwrap();
        fs::write(v.join("visible.md"), "---\nid: vis\n---\n\n").unwrap();
        let notes = walk_notes(&v).unwrap();
        let ids: Vec<_> = notes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["keep", "vis"]);
        assert!(!ids.iter().any(|id| *id == "obs" || *id == "git" || *id == "trash"));
    }

    #[test]
    fn walk_missing_vault_errors() {
        let d = tempdir().unwrap();
        let err = walk_notes(&d.path().join("nope")).unwrap_err();
        assert!(err.to_string().contains("missing"));
    }
}
