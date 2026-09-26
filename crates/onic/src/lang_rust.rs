use std::collections::HashSet;

use crate::types::{ExtractedGraph, Manifests, ScannedFile};

pub fn extract_rust(file: &ScannedFile, manifests: &Manifests) -> ExtractedGraph {
    let _ = manifests;
    let Some(tree) = crate::parsers::parse(crate::parsers::rust(), &file.text) else {
        return ExtractedGraph::default();
    };
    let mut graph = ExtractedGraph::default();
    crate::lang_rust_body::extract(file, tree.root_node(), &mut graph.nodes, &mut graph.edges);
    graph
}

pub fn resolve_rust_import(spec: &str, from: &str, known: &HashSet<String>) -> Option<String> {
    let segments: Vec<&str> = spec.split("::").collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return None;
    }
    if segments.contains(&"crate") && segments.first() != Some(&"crate") {
        return None;
    }
    let mut module_file = from.to_string();
    let mut dir = dirname(from);
    let mut start = 0usize;
    if segments.first() == Some(&"crate") {
        let crate_root = find_crate_root(from, known)?;
        if segments.len() == 1 {
            return Some(crate_root);
        }
        module_file = crate_root.clone();
        dir = dirname(&crate_root);
        start = 1;
    }
    let mut ident_updated_module_file = false;
    let walk = &segments[start..];
    for (offset, name) in walk.iter().enumerate() {
        let last = offset + 1 == walk.len();
        if *name == "self" {
            if last {
                return known.contains(&module_file).then_some(module_file);
            }
            dir = children_dir(&module_file);
            continue;
        }
        if *name == "super" {
            let parent_dir = dirname(&module_file);
            let candidates = if parent_dir.is_empty() {
                vec!["mod.rs".to_string()]
            } else {
                vec![format!("{parent_dir}/mod.rs"), format!("{parent_dir}.rs")]
            };
            let hit = first_known(&candidates, known)?;
            module_file = hit;
            if last {
                return Some(module_file);
            }
            dir = children_dir(&module_file);
            continue;
        }
        let candidates = if dir.is_empty() {
            vec![format!("{name}.rs"), format!("{name}/mod.rs")]
        } else {
            vec![format!("{dir}/{name}.rs"), format!("{dir}/{name}/mod.rs")]
        };
        let Some(hit) = first_known(&candidates, known) else {
            if last && ident_updated_module_file {
                return Some(module_file);
            }
            return None;
        };
        if last {
            return Some(hit);
        }
        module_file = hit;
        ident_updated_module_file = true;
        dir = children_dir(&module_file);
    }
    None
}

fn find_crate_root(from: &str, known: &HashSet<String>) -> Option<String> {
    let mut manifest_dir = dirname(from);
    loop {
        let lib = if manifest_dir.is_empty() {
            "src/lib.rs".to_string()
        } else {
            format!("{manifest_dir}/src/lib.rs")
        };
        let main = if manifest_dir.is_empty() {
            "src/main.rs".to_string()
        } else {
            format!("{manifest_dir}/src/main.rs")
        };
        if let Some(hit) = first_known(&[lib, main], known) {
            return Some(hit);
        }
        if manifest_dir.is_empty() {
            return None;
        }
        manifest_dir = dirname(&manifest_dir);
    }
}

fn first_known(candidates: &[String], known: &HashSet<String>) -> Option<String> {
    candidates.iter().find(|candidate| known.contains(*candidate)).cloned()
}

fn dirname(path: &str) -> String {
    match path.rfind('/') {
        Some(slash) => path[..slash].to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::types::ScannedFile;

    fn file(text: &str) -> ScannedFile {
        ScannedFile {
            path: "src/lib.rs".into(),
            abs_path: PathBuf::from("/tmp/src/lib.rs"),
            lang: "rust".into(),
            hash: String::new(),
            mtime: 0,
            size: text.len() as u64,
            text: text.into(),
        }
    }

    #[test]
    fn resolve_crate_item_lands_on_module_file() {
        let known = HashSet::from([
            "src/lib.rs".into(),
            "src/foo.rs".into(),
            "src/foo/bar.rs".into(),
        ]);
        assert_eq!(resolve_rust_import("crate", "src/lib.rs", &known).as_deref(), Some("src/lib.rs"));
        assert_eq!(resolve_rust_import("crate::foo", "src/lib.rs", &known).as_deref(), Some("src/foo.rs"));
        assert_eq!(
            resolve_rust_import("crate::foo::Bar", "src/lib.rs", &known).as_deref(),
            Some("src/foo.rs")
        );
        assert_eq!(resolve_rust_import("crate::missing", "src/lib.rs", &known), None);
    }

    #[test]
    fn extract_symbols_calls_and_imports() {
        let graph = extract_rust(
            &file(
                r#"
use crate::foo::Bar;
pub use super::baz;
fn hello() {
    world();
    obj.method();
    Color::Red(1);
    let f = || { g(); };
    f();
}
struct Point(i32);
enum Color { Red(i32) }
mod inline { fn nested() { inner(); } }
extern "C" { fn native_hash(); }
macro_rules! make { () => {}; }
"#,
            ),
            &crate::types::Manifests::default(),
        );
        let names: Vec<_> = graph.nodes.iter().map(|node| node.name.as_str()).collect();
        assert!(names.contains(&"hello"));
        assert!(names.contains(&"Point"));
        assert!(names.contains(&"Color"));
        assert!(names.contains(&"inline"));
        assert!(names.contains(&"nested"));
        assert!(names.contains(&"native_hash"));
        assert!(names.contains(&"make"));
        assert!(names.iter().any(|name| name.starts_with("closure:")));
        assert!(graph.nodes.iter().all(|node| node.kind != "file"));
        let calls: Vec<_> = graph.edges.iter().filter(|edge| edge.kind == "calls").map(|edge| edge.dst.as_str()).collect();
        assert!(calls.iter().any(|dst| *dst == "unresolved:world"));
        assert!(calls.iter().any(|dst| *dst == "unresolved:method"));
        assert!(calls.iter().any(|dst| *dst == "unresolved:Red"));
        assert!(calls.iter().any(|dst| dst.starts_with("unresolved:closure:")));
        let imports: Vec<_> = graph
            .edges
            .iter()
            .filter(|edge| edge.kind == "imports")
            .map(|edge| edge.props.get("spec").and_then(|value| value.as_str()).unwrap_or(""))
            .collect();
        assert!(imports.contains(&"crate::foo::Bar"));
        assert!(imports.contains(&"super::baz"));
        let native = graph.nodes.iter().find(|node| node.name == "native_hash").unwrap();
        assert_eq!(native.props.get("abi").and_then(|value| value.as_str()), Some("C"));
    }
}

fn children_dir(module_file: &str) -> String {
    if module_file == "mod.rs" {
        return String::new();
    }
    if let Some(dir) = module_file.strip_suffix("/mod.rs") {
        return dir.to_string();
    }
    if let Some(dir) = module_file.strip_suffix(".rs") {
        return dir.to_string();
    }
    module_file.to_string()
}
