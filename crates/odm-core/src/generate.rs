//! Local Generator materialize — copy a template directory under the Workspace.

use std::fs;
use std::path::PathBuf;

use crate::config::{GeneratorDef, Workspace};
use crate::error::OdmError;
use crate::fsutil::{self, ConflictPolicy};
use crate::paths::resolve_under_root;

/// Result of a successful local generate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerateOutcome {
    /// Number of files (and symlinks) written.
    pub copied: u32,
    /// Absolute destination path under the Workspace root.
    pub dest: PathBuf,
}

/// Resolve a Generator by name from the loaded Workspace.
pub fn generator<'a>(ws: &'a Workspace, name: &str) -> Result<&'a GeneratorDef, OdmError> {
    ws.generators
        .get(name)
        .ok_or_else(|| OdmError::usage(format!("unknown generator '{name}'")))
}

/// Materialize a local `template` directory into `dest_rel` under the Workspace root.
///
/// - Prefers `template` when both `template` and `url` are set.
/// - Url-only generators return a usage error (remote deferred).
/// - Without `force`, fails if dest exists as a file or non-empty directory.
/// - Empty dest directory is allowed without `force`.
/// - With `force`, overwrites files in place; removes file↔dir type conflicts
///   at each path; does not delete unrelated extras.
/// - With `dry_run`, validates and counts would-copy files but writes nothing
///   (including no force overwrites).
pub fn generate_local(
    ws: &Workspace,
    name: &str,
    dest_rel: &str,
    force: bool,
    dry_run: bool,
) -> Result<GenerateOutcome, OdmError> {
    let def = generator(ws, name)?;

    let template_rel = def
        .template
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    let Some(template_rel) = template_rel else {
        return Err(OdmError::usage(format!(
            "generator '{name}' has no local template (remote generators deferred)"
        )));
    };

    let template_path = resolve_under_root(&ws.root, template_rel)?;
    if !template_path.exists() {
        return Err(OdmError::operation(format!(
            "generator '{name}' template does not exist: {template_rel}"
        )));
    }
    if !template_path.is_dir() {
        return Err(OdmError::operation(format!(
            "generator '{name}' template is not a directory: {template_rel}"
        )));
    }

    let dest = resolve_under_root(&ws.root, dest_rel)?;

    if dest.exists() {
        let meta = fs::symlink_metadata(&dest).map_err(|e| {
            OdmError::operation(format!("failed to stat {}: {e}", dest.display()))
        })?;
        if !meta.is_dir() {
            return Err(OdmError::operation(format!(
                "destination exists and is not a directory: {dest_rel}"
            )));
        }
        if !force && !fsutil::is_dir_empty(&dest)? {
            return Err(OdmError::operation(format!(
                "destination is not empty: {dest_rel} (use --force)"
            )));
        }
    } else if dry_run {
        // Validate only — do not create dest or parents.
    } else if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            OdmError::operation(format!(
                "failed to create parent of {}: {e}",
                dest.display()
            ))
        })?;
        fs::create_dir(&dest).map_err(|e| {
            OdmError::operation(format!("failed to create {}: {e}", dest.display()))
        })?;
    } else {
        fs::create_dir_all(&dest).map_err(|e| {
            OdmError::operation(format!("failed to create {}: {e}", dest.display()))
        })?;
    }

    if dry_run {
        let copied = fsutil::count_tree(&template_path)?;
        return Ok(GenerateOutcome { copied, dest });
    }

    let copied = fsutil::copy_tree(&template_path, &dest, ConflictPolicy::ResolveTypeConflicts)?;
    Ok(GenerateOutcome { copied, dest })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::Write;
    use std::path::Path;

    use crate::config::WorkspaceConfig;
    use crate::error::exit_code;

    fn ws_with(
        root: PathBuf,
        generators: BTreeMap<String, GeneratorDef>,
    ) -> Workspace {
        Workspace {
            root,
            config: WorkspaceConfig::default(),
            actions: BTreeMap::new(),
            generators,
        }
    }

    fn gen_template(template: &str) -> GeneratorDef {
        GeneratorDef {
            template: Some(template.into()),
            url: None,
        }
    }

    fn setup_template(root: &Path, rel: &str, files: &[(&str, &str)]) -> PathBuf {
        let tpl = root.join(rel);
        fs::create_dir_all(&tpl).unwrap();
        for (name, body) in files {
            let p = tpl.join(name);
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            let mut f = fs::File::create(&p).unwrap();
            write!(f, "{body}").unwrap();
        }
        tpl
    }

    #[test]
    fn happy_copy() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_template(root, "templates/pkg", &[("README.md", "hello"), (".keep", "")]);

        let mut gens = BTreeMap::new();
        gens.insert("pkg".into(), gen_template("templates/pkg"));
        let ws = ws_with(root.to_path_buf(), gens);

        let out = generate_local(&ws, "pkg", "out/pkg", false, false).unwrap();
        assert_eq!(out.copied, 2);
        assert_eq!(out.dest, root.join("out/pkg"));
        assert_eq!(
            fs::read_to_string(root.join("out/pkg/README.md")).unwrap(),
            "hello"
        );
        assert!(root.join("out/pkg/.keep").is_file());
    }

    #[test]
    fn nested_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_template(
            root,
            "t",
            &[("a/b/c.txt", "deep"), ("a/top.txt", "top")],
        );

        let mut gens = BTreeMap::new();
        gens.insert("n".into(), gen_template("t"));
        let ws = ws_with(root.to_path_buf(), gens);

        let out = generate_local(&ws, "n", "dest/nested", false, false).unwrap();
        assert_eq!(out.copied, 2);
        assert_eq!(
            fs::read_to_string(root.join("dest/nested/a/b/c.txt")).unwrap(),
            "deep"
        );
        assert_eq!(
            fs::read_to_string(root.join("dest/nested/a/top.txt")).unwrap(),
            "top"
        );
    }

    #[test]
    fn force_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_template(root, "t", &[("f.txt", "new"), ("only_tpl.txt", "tpl")]);

        let mut gens = BTreeMap::new();
        gens.insert("g".into(), gen_template("t"));
        let ws = ws_with(root.to_path_buf(), gens);

        fs::create_dir_all(root.join("out")).unwrap();
        fs::write(root.join("out/f.txt"), "old").unwrap();
        fs::write(root.join("out/extra.txt"), "keep").unwrap();

        let err = generate_local(&ws, "g", "out", false, false).unwrap_err();
        assert!(err.to_string().contains("not empty"));
        assert_eq!(exit_code(&err), 3);

        let out = generate_local(&ws, "g", "out", true, false).unwrap();
        assert_eq!(out.copied, 2);
        assert_eq!(fs::read_to_string(root.join("out/f.txt")).unwrap(), "new");
        assert_eq!(
            fs::read_to_string(root.join("out/only_tpl.txt")).unwrap(),
            "tpl"
        );
        // Unrelated extra file preserved.
        assert_eq!(
            fs::read_to_string(root.join("out/extra.txt")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn force_file_over_dir_type_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_template(root, "t", &[("clash", "from-file")]);

        let mut gens = BTreeMap::new();
        gens.insert("g".into(), gen_template("t"));
        let ws = ws_with(root.to_path_buf(), gens);

        fs::create_dir_all(root.join("out/clash/nested")).unwrap();
        fs::write(root.join("out/clash/nested/stale.txt"), "debris").unwrap();
        fs::write(root.join("out/extra.txt"), "keep").unwrap();

        let out = generate_local(&ws, "g", "out", true, false).unwrap();
        assert_eq!(out.copied, 1);
        assert!(root.join("out/clash").is_file());
        assert_eq!(
            fs::read_to_string(root.join("out/clash")).unwrap(),
            "from-file"
        );
        assert_eq!(
            fs::read_to_string(root.join("out/extra.txt")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn force_dir_over_file_type_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_template(root, "t", &[("clash/inner.txt", "nested")]);

        let mut gens = BTreeMap::new();
        gens.insert("g".into(), gen_template("t"));
        let ws = ws_with(root.to_path_buf(), gens);

        fs::create_dir_all(root.join("out")).unwrap();
        fs::write(root.join("out/clash"), "was-file").unwrap();
        fs::write(root.join("out/extra.txt"), "keep").unwrap();

        let out = generate_local(&ws, "g", "out", true, false).unwrap();
        assert_eq!(out.copied, 1);
        assert!(root.join("out/clash").is_dir());
        assert_eq!(
            fs::read_to_string(root.join("out/clash/inner.txt")).unwrap(),
            "nested"
        );
        assert_eq!(
            fs::read_to_string(root.join("out/extra.txt")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn empty_dest_dir_ok_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_template(root, "t", &[("a.txt", "x")]);
        fs::create_dir_all(root.join("empty")).unwrap();

        let mut gens = BTreeMap::new();
        gens.insert("g".into(), gen_template("t"));
        let ws = ws_with(root.to_path_buf(), gens);

        let out = generate_local(&ws, "g", "empty", false, false).unwrap();
        assert_eq!(out.copied, 1);
        assert_eq!(fs::read_to_string(root.join("empty/a.txt")).unwrap(), "x");
    }

    #[test]
    fn reject_dest_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_template(root, "t", &[("a.txt", "x")]);
        fs::write(root.join("file"), "nope").unwrap();

        let mut gens = BTreeMap::new();
        gens.insert("g".into(), gen_template("t"));
        let ws = ws_with(root.to_path_buf(), gens);

        let err = generate_local(&ws, "g", "file", true, false).unwrap_err();
        assert!(err.to_string().contains("not a directory"));
    }

    #[test]
    fn reject_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_template(root, "t", &[("a.txt", "x")]);

        let mut gens = BTreeMap::new();
        gens.insert("g".into(), gen_template("t"));
        let ws = ws_with(root.to_path_buf(), gens);

        let err = generate_local(&ws, "g", "../outside", false, false).unwrap_err();
        assert!(err.to_string().contains("escape"));
        assert_eq!(exit_code(&err), 2);

        // Template path escape also rejected.
        let mut gens2 = BTreeMap::new();
        gens2.insert(
            "bad".into(),
            GeneratorDef {
                template: Some("../outside".into()),
                url: None,
            },
        );
        let ws2 = ws_with(root.to_path_buf(), gens2);
        let err2 = generate_local(&ws2, "bad", "out", false, false).unwrap_err();
        assert!(err2.to_string().contains("escape"));
    }

    #[test]
    fn missing_template() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let mut gens = BTreeMap::new();
        gens.insert("g".into(), gen_template("missing/tpl"));
        let ws = ws_with(root.to_path_buf(), gens);

        let err = generate_local(&ws, "g", "out", false, false).unwrap_err();
        assert!(err.to_string().contains("does not exist"));
        assert_eq!(exit_code(&err), 3);
    }

    #[test]
    fn unknown_name() {
        let dir = tempfile::tempdir().unwrap();
        let ws = ws_with(dir.path().to_path_buf(), BTreeMap::new());
        let err = generate_local(&ws, "nope", "out", false, false).unwrap_err();
        assert!(err.to_string().contains("unknown generator"));
        assert_eq!(exit_code(&err), 1);
        assert!(matches!(err, OdmError::Usage(_)));
    }

    #[test]
    fn url_only_generator_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let mut gens = BTreeMap::new();
        gens.insert(
            "remote".into(),
            GeneratorDef {
                template: None,
                url: Some("https://example.com/gen.git".into()),
            },
        );
        let ws = ws_with(root.to_path_buf(), gens);

        let err = generate_local(&ws, "remote", "out", false, false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("remote") || msg.contains("deferred") || msg.contains("template"));
        assert!(msg.to_lowercase().contains("remote") || msg.contains("deferred"));
        assert_eq!(exit_code(&err), 1);
    }

    #[test]
    fn prefer_template_when_both_set() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_template(root, "local", &[("ok.txt", "yes")]);

        let mut gens = BTreeMap::new();
        gens.insert(
            "both".into(),
            GeneratorDef {
                template: Some("local".into()),
                url: Some("https://example.com/gen.git".into()),
            },
        );
        let ws = ws_with(root.to_path_buf(), gens);

        let out = generate_local(&ws, "both", "dest", false, false).unwrap();
        assert_eq!(out.copied, 1);
        assert_eq!(fs::read_to_string(root.join("dest/ok.txt")).unwrap(), "yes");
    }

    #[test]
    fn empty_template_dir_success() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("empty_tpl")).unwrap();

        let mut gens = BTreeMap::new();
        gens.insert("e".into(), gen_template("empty_tpl"));
        let ws = ws_with(root.to_path_buf(), gens);

        let out = generate_local(&ws, "e", "out", false, false).unwrap();
        assert_eq!(out.copied, 0);
        assert!(root.join("out").is_dir());
    }

    #[test]
    fn generator_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let mut gens = BTreeMap::new();
        gens.insert("x".into(), gen_template("t"));
        let ws = ws_with(dir.path().to_path_buf(), gens);
        assert_eq!(
            generator(&ws, "x").unwrap().template.as_deref(),
            Some("t")
        );
        assert!(generator(&ws, "y").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn copies_symlink_as_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let tpl = root.join("t");
        fs::create_dir_all(&tpl).unwrap();
        fs::write(tpl.join("target.txt"), "data").unwrap();
        std::os::unix::fs::symlink("target.txt", tpl.join("link.txt")).unwrap();

        let mut gens = BTreeMap::new();
        gens.insert("s".into(), gen_template("t"));
        let ws = ws_with(root.to_path_buf(), gens);

        let out = generate_local(&ws, "s", "out", false, false).unwrap();
        assert_eq!(out.copied, 2);
        let link = root.join("out/link.txt");
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(fs::read_link(&link).unwrap(), PathBuf::from("target.txt"));
        assert_eq!(fs::read_to_string(&link).unwrap(), "data");
    }

    /// Snapshot relative paths under root (files, dirs, symlinks) for no-write checks.
    fn list_rel_paths(root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let rel = path.strip_prefix(base).unwrap().to_string_lossy().into_owned();
                out.push(rel);
                if entry.file_type().unwrap().is_dir() {
                    walk(base, &path, out);
                }
            }
        }
        walk(root, root, &mut out);
        out.sort();
        out
    }

    #[test]
    fn dry_run_happy_counts_without_writes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_template(
            root,
            "templates/pkg",
            &[
                ("README.md", "hello"),
                (".keep", ""),
                ("nested/a.txt", "a"),
            ],
        );

        let mut gens = BTreeMap::new();
        gens.insert("pkg".into(), gen_template("templates/pkg"));
        let ws = ws_with(root.to_path_buf(), gens);

        let before = list_rel_paths(root);
        let out = generate_local(&ws, "pkg", "out/pkg", false, true).unwrap();
        assert_eq!(out.copied, 3);
        assert_eq!(out.dest, root.join("out/pkg"));
        assert!(!root.join("out/pkg").exists());
        assert!(!root.join("out").exists());
        assert_eq!(list_rel_paths(root), before);
    }

    #[test]
    fn dry_run_force_existing_no_writes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_template(root, "t", &[("f.txt", "new"), ("only_tpl.txt", "tpl")]);

        let mut gens = BTreeMap::new();
        gens.insert("g".into(), gen_template("t"));
        let ws = ws_with(root.to_path_buf(), gens);

        fs::create_dir_all(root.join("out")).unwrap();
        fs::write(root.join("out/f.txt"), "old").unwrap();
        fs::write(root.join("out/extra.txt"), "keep").unwrap();

        let before = list_rel_paths(root);
        let out = generate_local(&ws, "g", "out", true, true).unwrap();
        assert_eq!(out.copied, 2);
        // Dest contents unchanged.
        assert_eq!(fs::read_to_string(root.join("out/f.txt")).unwrap(), "old");
        assert!(!root.join("out/only_tpl.txt").exists());
        assert_eq!(
            fs::read_to_string(root.join("out/extra.txt")).unwrap(),
            "keep"
        );
        assert_eq!(list_rel_paths(root), before);
    }

    #[test]
    fn dry_run_non_empty_dest_without_force_errors() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_template(root, "t", &[("a.txt", "x")]);
        fs::create_dir_all(root.join("out")).unwrap();
        fs::write(root.join("out/existing.txt"), "stay").unwrap();

        let mut gens = BTreeMap::new();
        gens.insert("g".into(), gen_template("t"));
        let ws = ws_with(root.to_path_buf(), gens);

        let before = list_rel_paths(root);
        let err = generate_local(&ws, "g", "out", false, true).unwrap_err();
        assert!(err.to_string().contains("not empty"));
        assert_eq!(exit_code(&err), 3);
        assert_eq!(
            fs::read_to_string(root.join("out/existing.txt")).unwrap(),
            "stay"
        );
        assert!(!root.join("out/a.txt").exists());
        assert_eq!(list_rel_paths(root), before);
    }

    #[test]
    fn dry_run_dest_is_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_template(root, "t", &[("a.txt", "x")]);
        fs::write(root.join("file"), "nope").unwrap();

        let mut gens = BTreeMap::new();
        gens.insert("g".into(), gen_template("t"));
        let ws = ws_with(root.to_path_buf(), gens);

        let err = generate_local(&ws, "g", "file", true, true).unwrap_err();
        assert!(err.to_string().contains("not a directory"));
        assert_eq!(fs::read_to_string(root.join("file")).unwrap(), "nope");
    }

    #[test]
    fn dry_run_url_only_errors() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let mut gens = BTreeMap::new();
        gens.insert(
            "remote".into(),
            GeneratorDef {
                template: None,
                url: Some("https://example.com/gen.git".into()),
            },
        );
        let ws = ws_with(root.to_path_buf(), gens);

        let err = generate_local(&ws, "remote", "out", false, true).unwrap_err();
        assert_eq!(exit_code(&err), 1);
        assert!(!root.join("out").exists());
    }

    #[test]
    fn real_run_still_writes_after_dry_run_param() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        setup_template(root, "t", &[("a.txt", "x"), ("b/c.txt", "y")]);

        let mut gens = BTreeMap::new();
        gens.insert("g".into(), gen_template("t"));
        let ws = ws_with(root.to_path_buf(), gens);

        let out = generate_local(&ws, "g", "dest", false, false).unwrap();
        assert_eq!(out.copied, 2);
        assert_eq!(fs::read_to_string(root.join("dest/a.txt")).unwrap(), "x");
        assert_eq!(fs::read_to_string(root.join("dest/b/c.txt")).unwrap(), "y");
    }
}
