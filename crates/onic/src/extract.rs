use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use crate::graph::{merge_graphs, resolve_names};
use crate::ids::{as_rel_path, file_id, unresolved_id};
use crate::markdown::retarget_heading_links;
use crate::types::{empty_props, ExtractedGraph, GraphEdge, GraphNode, Manifests, ScannedFile};

pub fn extract_file(file: &ScannedFile, manifests: &Manifests) -> ExtractedGraph {
    let mut parts = vec![file_node(file), crate::comments::extract_comments(file)];
    if file.lang == "md" {
        parts.push(crate::markdown::extract_markdown(file));
    } else if let Some(part) = language_extract(file, manifests) {
        parts.push(part);
    }
    merge_graphs(&parts)
}

pub fn materialize_graph(
    parts: Vec<ExtractedGraph>,
    known: &HashSet<String>,
    manifests: &Manifests,
) -> ExtractedGraph {
    let merged = merge_graphs(&parts);
    let retargeted = retarget_path_edges(merged, known, manifests);
    let connected = drop_missing(retargeted);
    let commented = attach_adjacent_comments(connected);
    retarget_heading_links(resolve_names(commented, manifests))
}

fn file_node(file: &ScannedFile) -> ExtractedGraph {
    let name = file.path.rsplit('/').next().unwrap_or(file.path.as_str());
    let mut props = empty_props();
    props.insert("lang".to_string(), Value::String(file.lang.clone()));
    ExtractedGraph {
        nodes: vec![GraphNode {
            id: file_id(&file.path),
            kind: "file".to_string(),
            name: name.to_string(),
            file_path: Some(file.path.clone()),
            start_line: Some(1),
            end_line: Some(count_lines(&file.text)),
            body: None,
            props,
        }],
        edges: Vec::new(),
    }
}

fn language_extract(file: &ScannedFile, manifests: &Manifests) -> Option<ExtractedGraph> {
    match file.lang.as_str() {
        "ts" | "js" => Some(crate::lang_js::extract_javascript(file, manifests)),
        "py" => Some(crate::lang_py::extract_python(file, manifests)),
        "rust" => Some(crate::lang_rust::extract_rust(file, manifests)),
        "go" => Some(crate::lang_go::extract_go(file, manifests)),
        "drac" => Some(crate::lang_drac::extract_draconic(file, manifests)),
        _ => None,
    }
}

fn retarget_path_edges(
    graph: ExtractedGraph,
    known: &HashSet<String>,
    manifests: &Manifests,
) -> ExtractedGraph {
    let langs = lang_by_file_id(&graph);
    let mut edges = Vec::new();
    for edge in graph.edges {
        if edge.kind == "imports" {
            let spec = edge.props.get("spec").and_then(|value| value.as_str());
            let from = path_from_prefixed(&edge.src, "file:");
            let (Some(spec), Some(from)) = (spec, from) else {
                edges.push(edge);
                continue;
            };
            let Some(lang) = langs.get(edge.src.as_str()).map(String::as_str) else {
                edges.push(edge);
                continue;
            };
            let Some(dst) = resolve_import(lang, &from, spec, known, manifests) else {
                edges.push(edge);
                continue;
            };
            let confidence = if dst.starts_with("file:") {
                "resolved"
            } else {
                "extracted"
            };
            edges.push(with_dst(edge, dst, confidence));
            continue;
        }
        if edge.kind == "links" {
            let href = edge.props.get("href").and_then(|value| value.as_str());
            let from = path_from_prefixed(&edge.src, "doc:")
                .or_else(|| path_from_prefixed(&edge.src, "file:"));
            let (Some(href), Some(from)) = (href, from) else {
                edges.push(edge);
                continue;
            };
            let Some(dst) = crate::markdown::resolve_markdown_href(href, &from, known) else {
                continue;
            };
            let confidence = if dst.starts_with("unresolved:") || hash_only_href(href) {
                "extracted"
            } else {
                "resolved"
            };
            edges.push(with_dst(edge, dst, confidence));
            continue;
        }
        edges.push(edge);
    }
    ExtractedGraph {
        nodes: graph.nodes,
        edges,
    }
}

fn resolve_import(
    lang: &str,
    from: &str,
    spec: &str,
    known: &HashSet<String>,
    manifests: &Manifests,
) -> Option<String> {
    let resolved = match lang {
        "py" => return Some(resolve_python(from, spec, known)),
        "ts" | "js" => crate::lang_js::resolve_js_import(spec, from, known),
        "rust" => crate::lang_rust::resolve_rust_import(spec, from, known),
        "go" => crate::lang_go::resolve_go_import(from, spec, known, &manifests.go),
        "drac" => crate::lang_drac::resolve_draconic_import(from, spec, known, &manifests.drac),
        _ => return None,
    };
    Some(import_dst(resolved, spec))
}

fn import_dst(resolved: Option<String>, spec: &str) -> String {
    match resolved {
        Some(value) if is_node_id(&value) => value,
        Some(path) => file_id(&path),
        None => unresolved_id(spec),
    }
}

fn is_node_id(value: &str) -> bool {
    [
        "file:",
        "unresolved:",
        "doc:",
        "symbol:",
        "heading:",
        "comment:",
        "tag:",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix))
}

fn resolve_python(from: &str, spec: &str, known: &HashSet<String>) -> String {
    if let Some(hit) = crate::lang_py::resolve_python_import(spec, from, known) {
        return import_dst(Some(hit), spec);
    }
    let joined = if spec.starts_with('.') {
        let raw = join_rel(file_dir(from), &python_relative_to_path(spec));
        match normalize_rel(&raw) {
            Some(path) => path,
            None => return unresolved_id(spec),
        }
    } else {
        spec.replace('.', "/")
    };
    for candidate in [format!("{joined}.py"), format!("{joined}/__init__.py")] {
        if known.contains(&candidate) {
            return file_id(&candidate);
        }
    }
    unresolved_id(spec)
}

fn python_relative_to_path(spec: &str) -> String {
    let bytes = spec.as_bytes();
    let mut dots = 0;
    while dots < bytes.len() && bytes[dots] == b'.' {
        dots += 1;
    }
    let rest = spec[dots..].replace('.', "/");
    let prefix = if dots <= 1 {
        ".".to_string()
    } else {
        std::iter::repeat("..")
            .take(dots - 1)
            .collect::<Vec<_>>()
            .join("/")
    };
    if rest.is_empty() {
        prefix
    } else {
        format!("{prefix}/{rest}")
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

fn join_rel(dir: &str, spec: &str) -> String {
    if dir.is_empty() {
        spec.to_string()
    } else {
        format!("{dir}/{spec}")
    }
}

fn file_dir(from: &str) -> &str {
    match from.rfind('/') {
        Some(index) => &from[..index],
        None => "",
    }
}

fn drop_missing(graph: ExtractedGraph) -> ExtractedGraph {
    let ids: HashSet<String> = graph.nodes.iter().map(|node| node.id.clone()).collect();
    let edges = graph
        .edges
        .into_iter()
        .filter(|edge| {
            ids.contains(&edge.src)
                && (ids.contains(&edge.dst) || edge.dst.starts_with("unresolved:"))
        })
        .collect();
    ExtractedGraph {
        nodes: graph.nodes,
        edges,
    }
}

fn lang_by_file_id(graph: &ExtractedGraph) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for node in &graph.nodes {
        if node.kind != "file" {
            continue;
        }
        if let Some(lang) = node.props.get("lang").and_then(|value| value.as_str()) {
            out.insert(node.id.clone(), lang.to_string());
        }
    }
    out
}

fn path_from_prefixed(id: &str, prefix: &str) -> Option<String> {
    let rest = id.strip_prefix(prefix)?;
    as_rel_path(rest).ok()
}

fn attach_adjacent_comments(graph: ExtractedGraph) -> ExtractedGraph {
    let mut by_file: HashMap<String, (Vec<usize>, Vec<usize>)> = HashMap::new();
    for (index, node) in graph.nodes.iter().enumerate() {
        let Some(path) = node.file_path.clone() else {
            continue;
        };
        if node.start_line.is_none() {
            continue;
        }
        let bucket = by_file.entry(path).or_default();
        if node.kind == "comment" {
            bucket.0.push(index);
        } else if node.kind == "symbol" {
            bucket.1.push(index);
        }
    }
    let mut extra = Vec::new();
    for (comments, symbols) in by_file.values() {
        for &comment_index in comments {
            let comment = &graph.nodes[comment_index];
            let Some(comment_end) = comment.end_line else {
                continue;
            };
            let mut next: Option<&GraphNode> = None;
            for &symbol_index in symbols {
                let symbol = &graph.nodes[symbol_index];
                let Some(start) = symbol.start_line else {
                    continue;
                };
                if start < comment_end {
                    continue;
                }
                let keep = next
                    .and_then(|best| best.start_line)
                    .is_none_or(|best| start < best);
                if keep {
                    next = Some(symbol);
                }
            }
            let Some(next) = next else {
                continue;
            };
            let Some(start) = next.start_line else {
                continue;
            };
            if start - comment_end > 2 {
                continue;
            }
            let mut props = Map::new();
            props.insert("adjacent".to_string(), Value::Bool(true));
            extra.push(GraphEdge {
                src: next.id.clone(),
                dst: comment.id.clone(),
                kind: "relates".to_string(),
                confidence: "extracted".to_string(),
                props,
            });
        }
    }
    merge_graphs(&[
        graph,
        ExtractedGraph {
            nodes: Vec::new(),
            edges: extra,
        },
    ])
}

fn hash_only_href(href: &str) -> bool {
    let dest = href.trim();
    let dest = dest
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'))
        .unwrap_or(dest)
        .trim();
    let cleaned = dest.split('#').next().unwrap_or("");
    let cleaned = cleaned.split('?').next().unwrap_or("").trim();
    cleaned.is_empty() && dest.contains('#')
}

fn with_dst(mut edge: GraphEdge, dst: String, confidence: &str) -> GraphEdge {
    edge.dst = dst;
    edge.confidence = confidence.to_string();
    edge
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::file_id;
    use crate::types::empty_props;
    use serde_json::json;

    fn node(id: &str, kind: &str, name: &str, file: Option<&str>) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            file_path: file.map(str::to_string),
            start_line: Some(1),
            end_line: Some(1),
            body: None,
            props: empty_props(),
        }
    }

    fn import(from: &str, spec: &str) -> GraphEdge {
        let mut props = empty_props();
        props.insert("spec".to_string(), json!(spec));
        GraphEdge {
            src: file_id(from),
            dst: unresolved_id(spec),
            kind: "imports".to_string(),
            confidence: "extracted".to_string(),
            props,
        }
    }

    #[test]
    fn python_relative_and_absolute_specs() {
        let known = HashSet::from([
            "pkg/sib.py".to_string(),
            "pkg/sub/__init__.py".to_string(),
            "os/path.py".to_string(),
        ]);
        let file = node("file:pkg/mod.py", "file", "mod.py", Some("pkg/mod.py"));
        let mut props = empty_props();
        props.insert("lang".to_string(), json!("py"));
        let mut file = file;
        file.props = props;
        let parts = vec![ExtractedGraph {
            nodes: vec![
                file.clone(),
                node("file:pkg/sib.py", "file", "sib.py", Some("pkg/sib.py")),
                node(
                    "file:pkg/sub/__init__.py",
                    "file",
                    "__init__.py",
                    Some("pkg/sub/__init__.py"),
                ),
                node("file:os/path.py", "file", "path.py", Some("os/path.py")),
            ],
            edges: vec![
                import("pkg/mod.py", ".sib"),
                import("pkg/mod.py", ".sub"),
                import("pkg/mod.py", "os.path"),
            ],
        }];
        let graph = materialize_graph(parts, &known, &Manifests::default());
        let dst = |spec: &str| {
            graph
                .edges
                .iter()
                .find(|edge| edge.props.get("spec").and_then(|v| v.as_str()) == Some(spec))
                .map(|edge| edge.dst.clone())
                .unwrap()
        };
        assert_eq!(dst(".sib"), "file:pkg/sib.py");
        assert_eq!(dst(".sub"), "file:pkg/sub/__init__.py");
        assert_eq!(dst("os.path"), "file:os/path.py");
    }
}

fn count_lines(text: &str) -> i64 {
    if text.is_empty() {
        1
    } else {
        text.split('\n').count() as i64
    }
}
