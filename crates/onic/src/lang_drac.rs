use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

use serde_json::{Map, Value};

use crate::ids::{file_id, symbol_id, unresolved_id};
use crate::types::{empty_props, DracManifest, ExtractedGraph, GraphEdge, GraphNode, Manifests, ScannedFile};

pub fn extract_draconic(file: &ScannedFile, manifests: &Manifests) -> ExtractedGraph {
    let _ = manifests;
    let output = match Command::new("draconic").arg("extract").arg(&file.abs_path).output() {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            panic!("draconic extract is not on PATH");
        }
        Err(err) => panic!("draconic extract failed: {err}"),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let code = output.status.code().map(|code| code.to_string()).unwrap_or_else(|| "null".to_string());
        if stderr.is_empty() {
            panic!("draconic extract exited {code}");
        }
        panic!("draconic extract exited {code}: {stderr}");
    }
    let stdout = String::from_utf8(output.stdout).unwrap_or_else(|_| panic!("draconic extract stdout is not JSON"));
    let parsed: Value = serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("draconic extract stdout is not JSON"));
    map_extract(file, &parsed)
}

pub fn resolve_draconic_import(
    from: &str,
    spec: &str,
    known: &HashSet<String>,
    manifests: &[DracManifest],
) -> Option<String> {
    if spec.starts_with('.') {
        return resolve_relative(from, spec, known);
    }
    let dest = package_remainder_path(from, spec, manifests)?;
    match_package_dest(&dest.joined, &dest.remainder, known)
}

pub fn collect_drac_manifests(root: &Path, files: &[ScannedFile]) -> Vec<DracManifest> {
    let mut out = Vec::new();
    let mut by_dir = HashSet::new();
    let mut seen_abs = HashSet::new();
    for file in files {
        if file.lang != "drac" {
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
                if let Ok(text) = std::fs::read_to_string(abs_dir.join("draconic.toml")) {
                    if let Some(module) = parse_draconic_module(&text) {
                        if !module.is_empty() && by_dir.insert(dir.clone()) {
                            out.push(DracManifest { dir: dir.clone(), module });
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

fn map_extract(file: &ScannedFile, json: &Value) -> ExtractedGraph {
    let record = json.as_object().cloned().unwrap_or_default();
    if !is_version_one(record.get("version")) {
        panic!("draconic extract version must be integer 1, got {}", version_got(&record));
    }
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut seen = HashSet::new();
    let owner = file_id(&file.path);
    for key in ["functions", "classes", "typeAliases", "externFunctions", "methods", "constructors", "accessors"] {
        for item in array_of(record.get(key)) {
            let Some(span) = named_span(item) else {
                continue;
            };
            let id = symbol_id(&file.path, &span.name);
            if !seen.insert(id.clone()) {
                continue;
            }
            let mut props = empty_props();
            match key {
                "typeAliases" => {
                    if item.get("native") == Some(&Value::Bool(true)) {
                        props.insert("native".into(), Value::Bool(true));
                    }
                }
                "externFunctions" => {
                    let abi = item.get("abi").and_then(Value::as_str).unwrap_or("C");
                    props.insert("abi".into(), Value::String(abi.to_string()));
                }
                "methods" => {
                    if item.get("static") == Some(&Value::Bool(true)) {
                        props.insert("static".into(), Value::Bool(true));
                    }
                }
                "accessors" => {
                    if let Some(accessor) = item.get("accessor").and_then(Value::as_str) {
                        if accessor == "get" || accessor == "set" {
                            props.insert("accessor".into(), Value::String(accessor.to_string()));
                        }
                    }
                }
                _ => {}
            }
            nodes.push(GraphNode {
                id: id.clone(),
                kind: "symbol".into(),
                name: span.name,
                file_path: Some(file.path.clone()),
                start_line: Some(span.start_line),
                end_line: Some(span.end_line),
                body: None,
                props,
            });
            edges.push(GraphEdge {
                src: owner.clone(),
                dst: id,
                kind: "contains".into(),
                confidence: "extracted".into(),
                props: empty_props(),
            });
        }
    }
    for item in array_of(record.get("imports")) {
        let Some(span) = named_span(item) else {
            continue;
        };
        let mut props = empty_props();
        props.insert("spec".into(), Value::String(span.name.clone()));
        edges.push(GraphEdge {
            src: owner.clone(),
            dst: unresolved_id(&span.name),
            kind: "imports".into(),
            confidence: "extracted".into(),
            props,
        });
    }
    let extern_names = extern_function_names(record.get("externFunctions"));
    for item in array_of(record.get("calls")) {
        let Some(span) = named_span(item) else {
            continue;
        };
        let member = item.get("member") == Some(&Value::Bool(true));
        if extern_names.contains(&span.name) && member {
            continue;
        }
        let mut props = empty_props();
        if member {
            props.insert("member".into(), Value::Bool(true));
        }
        edges.push(GraphEdge {
            src: call_source(file, item, &seen, &owner),
            dst: unresolved_id(&span.name),
            kind: "calls".into(),
            confidence: "extracted".into(),
            props,
        });
    }
    ExtractedGraph { nodes, edges }
}

struct Span {
    name: String,
    start_line: i64,
    end_line: i64,
}

fn named_span(value: &Value) -> Option<Span> {
    let record = value.as_object()?;
    let name = record.get("name")?.as_str()?.to_string();
    Some(Span {
        name,
        start_line: json_line(record.get("startLine")?)?,
        end_line: json_line(record.get("endLine")?)?,
    })
}

fn json_line(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_u64().and_then(|n| i64::try_from(n).ok())).or_else(|| value.as_f64().map(|n| n as i64))
}

fn array_of(value: Option<&Value>) -> Vec<&Value> {
    value.and_then(Value::as_array).map(|items| items.iter().collect()).unwrap_or_default()
}

fn is_version_one(value: Option<&Value>) -> bool {
    let Some(Value::Number(number)) = value else {
        return false;
    };
    number.as_i64() == Some(1) || number.as_u64() == Some(1) || number.as_f64() == Some(1.0)
}

fn version_got(record: &Map<String, Value>) -> String {
    match record.get("version") {
        None => "undefined".to_string(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::String(text) => text.clone(),
                Value::Number(number) => number.to_string(),
                Value::Bool(flag) => flag.to_string(),
                Value::Null => "null".to_string(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".to_string(),
    }
}

fn extern_function_names(value: Option<&Value>) -> HashSet<String> {
    let mut names = HashSet::new();
    for item in array_of(value) {
        if let Some(span) = named_span(item) {
            names.insert(span.name);
        }
    }
    names
}

fn call_source(file: &ScannedFile, item: &Value, seen: &HashSet<String>, owner: &str) -> String {
    let Some(record) = item.as_object() else {
        return owner.to_string();
    };
    let Some(enclosing) = record.get("enclosing").and_then(Value::as_str) else {
        return owner.to_string();
    };
    let id = symbol_id(&file.path, enclosing);
    if seen.contains(&id) { id } else { owner.to_string() }
}

fn parse_draconic_module(text: &str) -> Option<String> {
    let mut ignored_table = false;
    for raw in text.split('\n') {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            ignored_table = ignored_toml_table(line);
            continue;
        }
        if ignored_table {
            continue;
        }
        let Some(module) = module_assignment(line) else {
            continue;
        };
        if module.is_empty() {
            return None;
        }
        return Some(module);
    }
    None
}

fn ignored_toml_table(line: &str) -> bool {
    let Some(close) = line.find(']') else {
        return false;
    };
    let mut inner = line[1..close].trim();
    if let Some(rest) = inner.strip_prefix('[') {
        inner = rest.trim();
    }
    matches!(inner, "dependencies" | "urls" | "toolchain")
}

fn module_assignment(line: &str) -> Option<String> {
    let rest = line.strip_prefix("module")?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('=')?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    let value = rest[..end].to_string();
    let tail = rest[end + 1..].trim_start();
    if tail.is_empty() || tail.starts_with('#') {
        Some(value)
    } else {
        None
    }
}

struct PackageDest {
    joined: String,
    remainder: String,
}

fn resolve_relative(from: &str, spec: &str, known: &HashSet<String>) -> Option<String> {
    let dir = match from.rfind('/') {
        Some(slash) => &from[..slash],
        None => "",
    };
    let raw = if dir.is_empty() { spec.to_string() } else { format!("{dir}/{spec}") };
    let joined = normalize_rel(&raw)?;
    if joined.is_empty() {
        return None;
    }
    let candidate = if joined.ends_with(".drac") { joined.clone() } else { format!("{joined}.drac") };
    if known.contains(&candidate) {
        return Some(candidate);
    }
    if !joined.ends_with(".drac") {
        let index = format!("{joined}/index.drac");
        if known.contains(&index) {
            return Some(index);
        }
    }
    None
}

fn package_remainder_path(from: &str, spec: &str, manifests: &[DracManifest]) -> Option<PackageDest> {
    let remainder = strip_module_prefix(from, spec, manifests)?;
    let manifest = nearest_manifest(from, manifests)?;
    if remainder.is_empty() {
        return Some(PackageDest { joined: manifest.dir.clone(), remainder });
    }
    let raw = if manifest.dir.is_empty() {
        remainder.clone()
    } else {
        format!("{}/{}", manifest.dir, remainder)
    };
    let joined = normalize_rel(&raw)?;
    Some(PackageDest { joined, remainder })
}

fn match_package_dest(joined: &str, remainder: &str, known: &HashSet<String>) -> Option<String> {
    if !remainder.is_empty() {
        let candidate = if joined.ends_with(".drac") { joined.to_string() } else { format!("{joined}.drac") };
        if known.contains(&candidate) {
            return Some(candidate);
        }
    }
    if !joined.is_empty() {
        let pkg = basename(joined);
        let preferred = format!("{joined}/{pkg}.drac");
        if known.contains(&preferred) {
            return Some(preferred);
        }
    }
    let index = if joined.is_empty() { "index.drac".to_string() } else { format!("{joined}/index.drac") };
    if known.contains(&index) {
        return Some(index);
    }
    let mut children: Vec<&String> = known.iter().filter(|path| dirname(path) == joined && path.ends_with(".drac")).collect();
    children.sort();
    children.first().map(|path| (*path).clone())
}

fn strip_module_prefix(from: &str, spec: &str, manifests: &[DracManifest]) -> Option<String> {
    let manifest = nearest_manifest(from, manifests)?;
    if manifest.module.is_empty() {
        return None;
    }
    if spec == manifest.module {
        return Some(String::new());
    }
    let prefix = format!("{}/", manifest.module);
    spec.strip_prefix(&prefix).map(str::to_string)
}

fn nearest_manifest<'a>(from: &str, manifests: &'a [DracManifest]) -> Option<&'a DracManifest> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::types::ScannedFile;

    #[test]
    fn resolve_relative_and_package() {
        let known = HashSet::from(["src/hash.drac".into(), "src/auth/auth.drac".into(), "src/auth/index.drac".into()]);
        assert_eq!(
            resolve_draconic_import("src/check.drac", "./hash", &known, &[]).as_deref(),
            Some("src/hash.drac")
        );
        assert_eq!(
            resolve_draconic_import("src/check.drac", "./auth", &known, &[]).as_deref(),
            Some("src/auth/index.drac")
        );
        let manifests = [DracManifest { dir: "src".into(), module: "app".into() }];
        assert_eq!(
            resolve_draconic_import("src/check.drac", "app/auth", &known, &manifests).as_deref(),
            Some("src/auth/auth.drac")
        );
    }

    #[test]
    fn extract_fixture_maps_symbols_not_exports() {
        let path = "/Users/jaredhembrow/workbench/core-onic/test/fixtures/mini/src/hash.drac";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let text = std::fs::read_to_string(path).unwrap();
        let file = ScannedFile {
            path: "src/hash.drac".into(),
            abs_path: PathBuf::from(path),
            lang: "drac".into(),
            hash: String::new(),
            mtime: 0,
            size: text.len() as u64,
            text,
        };
        let graph = extract_draconic(&file, &crate::types::Manifests::default());
        assert!(graph.nodes.iter().all(|node| node.kind != "file"));
        assert!(graph.nodes.iter().any(|node| node.name == "hashPassword"));
        let alias = graph.nodes.iter().find(|node| node.name == "HashBuf").unwrap();
        assert_eq!(alias.props.get("native"), Some(&serde_json::Value::Bool(true)));
        let extern_fn = graph.nodes.iter().find(|node| node.name == "nativeHash").unwrap();
        assert_eq!(extern_fn.props.get("abi").and_then(|value| value.as_str()), Some("C"));
        assert!(graph.edges.iter().any(|edge| {
            edge.kind == "calls" && edge.dst == "unresolved:encodePassword" && edge.src.ends_with("#hashPassword")
        }));
        assert!(!graph.edges.iter().any(|edge| edge.kind == "exports"));
    }
}

fn rel_dir(root: &Path, abs_dir: &Path) -> Option<String> {
    let rel = abs_dir.strip_prefix(root).ok()?;
    let text = rel.to_string_lossy().replace('\\', "/");
    if text == ".." || text.starts_with("../") {
        return None;
    }
    if text.is_empty() || text == "." { Some(String::new()) } else { Some(text) }
}
