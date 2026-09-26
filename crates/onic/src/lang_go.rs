use std::collections::{HashSet, HashMap};
use std::path::Path;

use crate::types::{ExtractedGraph, GoManifest, Manifests, ScannedFile};

pub fn extract_go(file: &ScannedFile, manifests: &Manifests) -> ExtractedGraph {
    let _ = manifests;
    crate::lang_go_walk::extract(file)
}

pub fn resolve_go_import(
    from: &str,
    spec: &str,
    known: &HashSet<String>,
    manifests: &[GoManifest],
) -> Option<String> {
    let remainder = strip_module_prefix(from, spec, manifests);
    let dir_spec = remainder.unwrap_or_else(|| spec.to_string());
    let hit = match_go_package_dir(&dir_spec, known)?;
    if let Some(parent_dir) = last_internal_parent_dir(&dir_spec) {
        if !importer_in_internal_parent(from, &parent_dir) {
            return None;
        }
    }
    Some(hit)
}

pub fn collect_go_manifests(root: &Path, files: &[ScannedFile]) -> Vec<GoManifest> {
    collect_manifests(root, files, "go", "go.mod", parse_go_module)
}

pub fn same_package(source: &str, candidate: &str) -> bool {
    source.ends_with(".go") && candidate.ends_with(".go") && dirname(source) == dirname(candidate)
}

fn collect_manifests(
    root: &Path,
    files: &[ScannedFile],
    lang: &str,
    file_name: &str,
    parse: fn(&str) -> Option<String>,
) -> Vec<GoManifest> {
    let mut out = Vec::new();
    let mut by_dir = HashSet::new();
    let mut seen_abs = HashSet::new();
    for file in files {
        if file.lang != lang {
            continue;
        }
        let Some(mut abs_dir) = file.abs_path.parent().map(|path| path.to_path_buf()) else {
            continue;
        };
        loop {
            let Some(dir) = rel_dir(root, &abs_dir) else {
                break;
            };
            if seen_abs.insert(abs_dir.clone()) {
                if let Ok(text) = std::fs::read_to_string(abs_dir.join(file_name)) {
                    if let Some(module) = parse(&text) {
                        if !module.is_empty() && by_dir.insert(dir.clone()) {
                            out.push(GoManifest { dir: dir.clone(), module });
                        }
                    }
                }
            }
            if dir.is_empty() {
                break;
            }
            match abs_dir.parent() {
                Some(parent) if parent != abs_dir => abs_dir = parent.to_path_buf(),
                _ => break,
            }
        }
    }
    out
}

fn parse_go_module(text: &str) -> Option<String> {
    for raw in text.split('\n') {
        let mut line = raw.trim().to_string();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if let Some(comment) = line.find("//") {
            line = line[..comment].trim().to_string();
            if line.is_empty() {
                continue;
            }
        }
        if line == "module" || !line.starts_with("module") {
            continue;
        }
        let after = &line["module".len()..];
        let Some(lead) = after.chars().next() else {
            continue;
        };
        if lead != ' ' && lead != '\t' {
            continue;
        }
        let mut path = after.trim().to_string();
        if path.is_empty() {
            continue;
        }
        if let Some(space) = path.find(char::is_whitespace) {
            path = path[..space].to_string();
        }
        path = unquote_module_path(&path);
        if path.is_empty() {
            return None;
        }
        return Some(path);
    }
    None
}

fn unquote_module_path(path: &str) -> String {
    let bytes = path.as_bytes();
    if bytes.len() < 2 {
        return path.to_string();
    }
    let start = bytes[0];
    let end = bytes[bytes.len() - 1];
    if start == end && (start == b'"' || start == b'`') {
        return path[1..path.len() - 1].to_string();
    }
    path.to_string()
}

fn strip_module_prefix(from: &str, spec: &str, manifests: &[GoManifest]) -> Option<String> {
    let manifest = nearest_manifest(from, manifests)?;
    if manifest.module.is_empty() {
        return None;
    }
    if spec == manifest.module {
        return Some(String::new());
    }
    let prefix = format!("{}/", manifest.module);
    if let Some(rest) = spec.strip_prefix(&prefix) {
        return Some(rest.to_string());
    }
    None
}

fn nearest_manifest<'a>(from: &str, manifests: &'a [GoManifest]) -> Option<&'a GoManifest> {
    if manifests.is_empty() {
        return None;
    }
    let mut by_dir = HashMap::new();
    for manifest in manifests {
        by_dir.entry(manifest.dir.as_str()).or_insert(manifest);
    }
    let mut current = from.to_string();
    loop {
        if let Some(found) = by_dir.get(current.as_str()) {
            return Some(*found);
        }
        if current.is_empty() {
            return None;
        }
        current = dirname(&current);
    }
}

fn match_go_package_dir(dir_spec: &str, known: &HashSet<String>) -> Option<String> {
    if dir_spec.is_empty() {
        let mut roots: Vec<&String> = known
            .iter()
            .filter(|path| dirname(path).is_empty() && path.ends_with(".go"))
            .collect();
        roots.sort();
        return roots.first().map(|path| (*path).clone());
    }
    let dir = normalize_rel(dir_spec)?;
    if dir.is_empty() {
        return None;
    }
    let pkg = match dir.rfind('/') {
        Some(slash) => &dir[slash + 1..],
        None => dir.as_str(),
    };
    let preferred = format!("{dir}/{pkg}.go");
    let mut children: Vec<&String> = known
        .iter()
        .filter(|path| dirname(path) == dir && basename(path).ends_with(".go"))
        .collect();
    if children.iter().any(|path| *path == &preferred) {
        return Some(preferred);
    }
    if children.is_empty() {
        return None;
    }
    children.sort();
    children.first().map(|path| (*path).clone())
}

fn last_internal_parent_dir(dir_spec: &str) -> Option<String> {
    let parts: Vec<&str> = dir_spec.split('/').collect();
    let mut last = None;
    for (index, part) in parts.iter().enumerate() {
        if *part == "internal" {
            last = Some(index);
        }
    }
    let index = last?;
    Some(parts[..index].join("/"))
}

fn importer_in_internal_parent(from: &str, parent_dir: &str) -> bool {
    let importer_dir = dirname(from);
    parent_dir.is_empty()
        || importer_dir == parent_dir
        || importer_dir.starts_with(&format!("{parent_dir}/"))
}

fn normalize_rel(path: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            if parts.is_empty() {
                return None;
            }
            parts.pop();
            continue;
        }
        parts.push(part);
    }
    Some(parts.join("/"))
}

fn dirname(path: &str) -> String {
    match path.rfind('/') {
        Some(slash) => path[..slash].to_string(),
        None => String::new(),
    }
}

fn basename(path: &str) -> String {
    match path.rfind('/') {
        Some(slash) => path[slash + 1..].to_string(),
        None => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::types::ScannedFile;

    fn file(path: &str, text: &str) -> ScannedFile {
        ScannedFile {
            path: path.into(),
            abs_path: PathBuf::from("/tmp").join(path),
            lang: "go".into(),
            hash: String::new(),
            mtime: 0,
            size: text.len() as u64,
            text: text.into(),
        }
    }

    #[test]
    fn extract_and_resolve_go() {
        let graph = extract_go(
            &file(
                "src/main.go",
                r#"
package main
import (
    "fmt"
    . "other"
    _ "blank"
    alias "aliased"
)
func init() {}
func main() {
    fmt.Println("x")
    T(1)
    alias.Run()
}
type T struct { U }
func (t *T) M() { t.N() }
"#,
            ),
            &crate::types::Manifests::default(),
        );
        let names: Vec<_> = graph.nodes.iter().map(|node| node.name.as_str()).collect();
        assert!(names.contains(&"init"));
        assert!(names.contains(&"main"));
        assert!(names.contains(&"T"));
        assert!(names.contains(&"T.M"));
        let specs: Vec<_> = graph
            .edges
            .iter()
            .filter(|edge| edge.kind == "imports")
            .map(|edge| edge.props.get("spec").and_then(|v| v.as_str()).unwrap_or(""))
            .collect();
        assert!(specs.contains(&"fmt"));
        assert!(specs.contains(&"other"));
        assert!(specs.contains(&"blank"));
        assert!(specs.contains(&"aliased"));
        let calls: Vec<_> = graph.edges.iter().filter(|edge| edge.kind == "calls").map(|edge| edge.dst.as_str()).collect();
        assert!(calls.contains(&"unresolved:Println"));
        assert!(!calls.contains(&"unresolved:T"));
        assert!(calls.contains(&"unresolved:N"));
        let known = HashSet::from(["src/fmt.go".into(), "pkg/internal/secret/secret.go".into()]);
        let manifests = [GoManifest { dir: String::new(), module: "example.com/app".into() }];
        assert_eq!(
            resolve_go_import("src/main.go", "example.com/app/src", &known, &manifests).as_deref(),
            Some("src/fmt.go")
        );
        assert_eq!(
            resolve_go_import("src/main.go", "example.com/app/pkg/internal/secret", &known, &manifests),
            None
        );
        assert_eq!(
            resolve_go_import("pkg/main.go", "example.com/app/pkg/internal/secret", &known, &manifests).as_deref(),
            Some("pkg/internal/secret/secret.go")
        );
        assert!(same_package("src/a.go", "src/b.go"));
        assert!(!same_package("src/a.go", "other/b.go"));
    }
}

fn rel_dir(root: &Path, abs_dir: &Path) -> Option<String> {
    let rel = abs_dir.strip_prefix(root).ok()?;
    let text = rel.to_string_lossy().replace('\\', "/");
    if text == ".." || text.starts_with("../") {
        return None;
    }
    if text.is_empty() || text == "." {
        Some(String::new())
    } else {
        Some(text)
    }
}
