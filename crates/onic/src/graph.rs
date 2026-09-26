use std::collections::{HashMap, HashSet};

use crate::ids::unresolved_id;
use crate::types::{empty_props, ExtractedGraph, GraphEdge, GraphNode, Manifests};

const PATH_EXTENSIONS: &[&str] = &[
    ".md", ".mdx", ".py", ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".rs",
    ".go", ".drac",
];

pub fn merge_graphs(parts: &[ExtractedGraph]) -> ExtractedGraph {
    let mut nodes = Vec::new();
    let mut seen_nodes = HashSet::new();
    let mut edges = Vec::new();
    let mut seen_edges = HashSet::new();
    for part in parts {
        for node in &part.nodes {
            if seen_nodes.insert(node.id.clone()) {
                nodes.push(node.clone());
            }
        }
        for edge in &part.edges {
            let spec = edge
                .props
                .get("spec")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let key = format!("{}\0{}\0{}\0{spec}", edge.src, edge.dst, edge.kind);
            if seen_edges.insert(key) {
                edges.push(edge.clone());
            }
        }
    }
    ExtractedGraph { nodes, edges }
}

pub fn resolve_names(graph: ExtractedGraph, manifests: &Manifests) -> ExtractedGraph {
    let _ = manifests;
    let mut by_name: HashMap<&str, Vec<&GraphNode>> = HashMap::new();
    let mut by_id: HashMap<&str, &GraphNode> = HashMap::new();
    for node in &graph.nodes {
        by_id.insert(node.id.as_str(), node);
        if node.kind == "unresolved" {
            continue;
        }
        by_name.entry(node.name.as_str()).or_default().push(node);
    }
    let imported = direct_imports(&graph, &by_id);
    let mut extra = Vec::new();
    let mut edges = Vec::new();
    for edge in &graph.edges {
        if !edge.dst.starts_with("unresolved:") {
            edges.push(edge.clone());
            continue;
        }
        let name = &edge.dst["unresolved:".len()..];
        if edge.kind == "imports" || looks_like_path(name) {
            extra.push(unresolved_node(name, &edge.dst));
            edges.push(edge.clone());
            continue;
        }
        let exact = by_name.get(name).cloned().unwrap_or_default();
        let suffix = suffix_matches(&graph.nodes, name);
        let candidates: Vec<&GraphNode> = if is_member(edge) {
            suffix
        } else if !exact.is_empty() {
            exact
        } else {
            suffix
        };
        let source = by_id.get(edge.src.as_str()).copied();
        let mut target = pick_unique(&candidates);
        if target.is_none() && edge.kind == "calls" {
            let same = if is_member(edge) {
                SamePackage::Miss
            } else {
                pick_same_package(&candidates, source)
            };
            target = match same {
                SamePackage::Hit(node) => Some(node),
                SamePackage::Tie => None,
                SamePackage::Miss => pick_imported(&candidates, source, &imported),
            };
        }
        let Some(target) = target else {
            extra.push(unresolved_node(name, &edge.dst));
            edges.push(edge.clone());
            continue;
        };
        let mut resolved = edge.clone();
        resolved.dst = target.id.clone();
        resolved.confidence = "resolved".to_string();
        edges.push(resolved);
    }
    merge_graphs(&[
        ExtractedGraph {
            nodes: graph.nodes,
            edges,
        },
        ExtractedGraph {
            nodes: extra,
            edges: Vec::new(),
        },
    ])
}

fn looks_like_path(name: &str) -> bool {
    if name.contains('/') || name.starts_with('.') {
        return true;
    }
    let Some(dot) = name.rfind('.') else {
        return false;
    };
    if dot < 1 {
        return false;
    }
    let ext = name[dot..].to_ascii_lowercase();
    PATH_EXTENSIONS.contains(&ext.as_str())
}

fn pick_unique<'a>(nodes: &[&'a GraphNode]) -> Option<&'a GraphNode> {
    let best = nodes.iter().copied().min_by_key(|node| rank(&node.kind))?;
    let best_rank = rank(&best.kind);
    let ties = nodes
        .iter()
        .filter(|node| rank(&node.kind) == best_rank)
        .count();
    if ties == 1 {
        Some(best)
    } else {
        None
    }
}

fn suffix_matches<'a>(nodes: &'a [GraphNode], name: &str) -> Vec<&'a GraphNode> {
    let suffix = format!(".{name}");
    nodes
        .iter()
        .filter(|node| node.kind == "symbol" && node.name.ends_with(&suffix))
        .collect()
}

fn direct_imports<'a>(
    graph: &'a ExtractedGraph,
    by_id: &HashMap<&str, &'a GraphNode>,
) -> HashMap<String, HashSet<String>> {
    let mut imported = HashMap::new();
    for edge in &graph.edges {
        if edge.kind != "imports" {
            continue;
        }
        let Some(src) = by_id.get(edge.src.as_str()).copied() else {
            continue;
        };
        let Some(dst) = by_id.get(edge.dst.as_str()).copied() else {
            continue;
        };
        if src.kind != "file" || dst.kind != "file" {
            continue;
        }
        let (Some(src_path), Some(dst_path)) = (src.file_path.clone(), dst.file_path.clone())
        else {
            continue;
        };
        imported
            .entry(src_path)
            .or_insert_with(HashSet::new)
            .insert(dst_path);
    }
    imported
}

enum SamePackage<'a> {
    Hit(&'a GraphNode),
    Tie,
    Miss,
}

fn pick_same_package<'a>(
    candidates: &[&'a GraphNode],
    source: Option<&GraphNode>,
) -> SamePackage<'a> {
    let Some(source_path) = source.and_then(|node| node.file_path.as_deref()) else {
        return SamePackage::Miss;
    };
    if crate::walk::lang_for_path(source_path) != Some("go") {
        return SamePackage::Miss;
    }
    let local: Vec<&GraphNode> = candidates
        .iter()
        .copied()
        .filter(|node| {
            node.file_path
                .as_deref()
                .is_some_and(|path| crate::lang_go::same_package(source_path, path))
        })
        .collect();
    if local.is_empty() {
        return SamePackage::Miss;
    }
    match pick_unique(&local) {
        Some(node) => SamePackage::Hit(node),
        None => SamePackage::Tie,
    }
}

fn pick_imported<'a>(
    candidates: &[&'a GraphNode],
    source: Option<&GraphNode>,
    imported: &HashMap<String, HashSet<String>>,
) -> Option<&'a GraphNode> {
    let source_path = source.and_then(|node| node.file_path.as_deref())?;
    let nearby = imported.get(source_path);
    let mut matches = candidates.iter().copied().filter(|node| {
        node.file_path
            .as_deref()
            .is_some_and(|path| path == source_path || nearby.is_some_and(|set| set.contains(path)))
    });
    let first = matches.next()?;
    if matches.next().is_some() {
        None
    } else {
        Some(first)
    }
}

fn unresolved_node(name: &str, id: &str) -> GraphNode {
    GraphNode {
        id: if id.starts_with("unresolved:") {
            id.to_string()
        } else {
            unresolved_id(name)
        },
        kind: "unresolved".to_string(),
        name: name.to_string(),
        file_path: None,
        start_line: None,
        end_line: None,
        body: None,
        props: empty_props(),
    }
}

fn is_member(edge: &GraphEdge) -> bool {
    edge.props.get("member").and_then(|value| value.as_bool()) == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn same_kind_tie_stays_unresolved() {
        let mut edge = GraphEdge {
            src: "symbol:a.py#caller".to_string(),
            dst: "unresolved:Target".to_string(),
            kind: "relates".to_string(),
            confidence: "extracted".to_string(),
            props: empty_props(),
        };
        let graph = resolve_names(
            ExtractedGraph {
                nodes: vec![
                    node("symbol:a.py#caller", "symbol", "caller", Some("a.py")),
                    node("symbol:a.py#Target", "symbol", "Target", Some("a.py")),
                    node("symbol:b.py#Target", "symbol", "Target", Some("b.py")),
                ],
                edges: vec![edge.clone()],
            },
            &Manifests::default(),
        );
        assert_eq!(graph.edges[0].dst, "unresolved:Target");
        edge.kind = "calls".to_string();
        edge.props.insert("member".to_string(), json!(true));
        let graph = resolve_names(
            ExtractedGraph {
                nodes: vec![
                    node("symbol:a.py#caller", "symbol", "caller", Some("a.py")),
                    node(
                        "symbol:a.py#Foo.Target",
                        "symbol",
                        "Foo.Target",
                        Some("a.py"),
                    ),
                    node(
                        "symbol:a.py#Bar.Target",
                        "symbol",
                        "Bar.Target",
                        Some("a.py"),
                    ),
                ],
                edges: vec![edge],
            },
            &Manifests::default(),
        );
        assert_eq!(graph.edges[0].dst, "unresolved:Target");
    }
}

fn rank(kind: &str) -> i32 {
    match kind {
        "symbol" => 0,
        "heading" => 1,
        "doc" => 2,
        "file" => 3,
        "comment" => 4,
        "tag" => 5,
        _ => 99,
    }
}
