use std::collections::HashSet;

use serde_json::{Map, Value};
use tree_sitter::Node;

use crate::ids::{file_id, symbol_id, unresolved_id};
use crate::parsers;
use crate::types::{ExtractedGraph, GraphEdge, GraphNode, Manifests, ScannedFile};

pub fn extract_python(file: &ScannedFile, manifests: &Manifests) -> ExtractedGraph {
    let _ = manifests;
    let Some(tree) = parsers::parse(parsers::python(), &file.text) else {
        return ExtractedGraph::default();
    };
    let mut walk = PyWalk {
        path: &file.path,
        source: &file.text,
        nodes: Vec::new(),
        edges: Vec::new(),
        file_node: file_id(&file.path),
    };
    let mut instances = Some(HashSet::new());
    let file_node = walk.file_node.clone();
    for child in kids(tree.root_node()) {
        if child.kind() == "import_from_statement" || child.kind() == "import_statement" {
            walk.take_import(child);
            if child.kind() == "import_from_statement" {
                seed_from_import(child, walk.source, instances.as_mut().unwrap());
            }
            continue;
        }
        walk.visit(child, &file_node, None, true, &mut instances);
    }
    ExtractedGraph { nodes: walk.nodes, edges: walk.edges }
}

struct PyWalk<'a> {
    path: &'a str,
    source: &'a str,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    file_node: String,
}

impl PyWalk<'_> {
    fn visit(
        &mut self,
        node: Node,
        enclosing: &str,
        module_class: Option<&str>,
        module_level: bool,
        instances: &mut Option<HashSet<String>>,
    ) {
        let def = unwrap_decorated(node);
        if def.kind() == "function_definition" {
            let name = name_of(def, self.source);
            let owned = if let Some(name) = name.as_deref() {
                let symbol_name = module_class.map(|class_name| format!("{class_name}.{name}")).unwrap_or_else(|| name.to_string());
                self.add_symbol(&symbol_name, def)
            } else {
                enclosing.to_string()
            };
            let mut body = Some(instances.clone().unwrap_or_default());
            if let Some(parameters) = field(def, "parameters") {
                for child in kids(parameters) {
                    if child.kind() == "identifier" {
                        body.as_mut().unwrap().remove(node_text(child, self.source));
                    }
                }
            }
            self.visit_body(node, def, &owned, None, false, &mut body);
            return;
        }
        if def.kind() == "class_definition" {
            if let Some(name) = name_of(def, self.source) {
                let symbol_name = if !module_level {
                    module_class.map(|class_name| format!("{class_name}.{name}")).unwrap_or(name)
                } else {
                    name
                };
                let owned = self.add_symbol(&symbol_name, def);
                let mut body = Some(HashSet::new());
                self.visit_body(node, def, &owned, Some(&symbol_name), false, &mut body);
                return;
            }
            let mut none = None;
            self.visit_body(node, def, enclosing, None, false, &mut none);
            return;
        }
        if node.kind() == "lambda" {
            let line = node.start_position().row as i64 + 1;
            let owned = self.add_symbol(&format!("lambda:{line}"), node);
            let mut lambda_instances = instances.clone();
            if let Some(names) = lambda_instances.as_mut() {
                if let Some(parameters) = field(node, "parameters") {
                    for child in kids(parameters) {
                        if child.kind() == "identifier" {
                            names.remove(node_text(child, self.source));
                        }
                    }
                }
            }
            for child in kids(node) {
                self.visit(child, &owned, None, false, &mut lambda_instances);
            }
            return;
        }
        if instances.is_some() && (node.kind() == "assignment" || node.kind() == "named_expression") {
            take_instance_binding(node, self.source, instances.as_mut().unwrap());
        }
        if node.kind() == "call" {
            take_identifier_call(enclosing, node, self.source, &mut self.edges, instances.as_ref());
        }
        for child in kids(node) {
            self.visit(child, enclosing, module_class, module_level, instances);
        }
    }

    fn visit_body(
        &mut self,
        node: Node,
        def: Node,
        enclosing: &str,
        module_class: Option<&str>,
        module_level: bool,
        instances: &mut Option<HashSet<String>>,
    ) {
        if node.kind() == "decorated_definition" {
            for child in kids(node) {
                if child.kind() == "function_definition" || child.kind() == "class_definition" {
                    for inner in kids(child) {
                        self.visit(inner, enclosing, module_class, module_level, instances);
                    }
                } else {
                    self.visit(child, enclosing, module_class, module_level, instances);
                }
            }
            return;
        }
        for child in kids(def) {
            self.visit(child, enclosing, module_class, module_level, instances);
        }
    }

    fn add_symbol(&mut self, name: &str, node: Node) -> String {
        let id = symbol_id(self.path, name);
        let mut props = Map::new();
        props.insert("syntax".to_string(), Value::String(node.kind().to_string()));
        self.nodes.push(GraphNode {
            id: id.clone(),
            kind: "symbol".to_string(),
            name: name.to_string(),
            file_path: Some(self.path.to_string()),
            start_line: Some(node.start_position().row as i64 + 1),
            end_line: Some(node.end_position().row as i64 + 1),
            body: None,
            props,
        });
        self.edges.push(GraphEdge {
            src: self.file_node.clone(),
            dst: id.clone(),
            kind: "contains".to_string(),
            confidence: "extracted".to_string(),
            props: Map::new(),
        });
        id
    }

    fn take_import(&mut self, node: Node) {
        if node.kind() == "import_from_statement" {
            if let Some(module) = field(node, "module_name") {
                self.add_import_edge(node_text(module, self.source));
            }
            return;
        }
        for child in kids(node) {
            let name_node = if child.kind() == "aliased_import" { field(child, "name").unwrap_or(child) } else { child };
            self.add_import_edge(node_text(name_node, self.source));
        }
    }

    fn add_import_edge(&mut self, spec: &str) {
        let mut props = Map::new();
        props.insert("spec".to_string(), Value::String(spec.to_string()));
        self.edges.push(GraphEdge {
            src: file_id(self.path),
            dst: unresolved_id(spec),
            kind: "imports".to_string(),
            confidence: "extracted".to_string(),
            props,
        });
    }
}

pub fn resolve_python_import(spec: &str, from: &str, known: &HashSet<String>) -> Option<String> {
    if !spec.starts_with('.') {
        return match_python_module(&spec.replace('.', "/"), known);
    }
    let dir = from.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let path_spec = python_relative_to_path(spec);
    let joined_raw = if dir.is_empty() { path_spec } else { format!("{dir}/{path_spec}") };
    let joined = normalize_rel(&joined_raw)?;
    match_python_module(&joined, known)
}

fn seed_from_import(node: Node, source: &str, instances: &mut HashSet<String>) {
    let module_name = field(node, "module_name");
    for named in kids(node) {
        if module_name.is_some_and(|module| module.id() == named.id()) || named.kind() == "wildcard_import" {
            continue;
        }
        if named.kind() == "aliased_import" {
            if let Some(alias) = field(named, "alias") {
                instances.insert(node_text(alias, source).to_string());
            }
            continue;
        }
        if named.kind() == "dotted_name" || named.kind() == "identifier" {
            instances.insert(node_text(named, source).to_string());
        }
    }
}

fn match_python_module(joined: &str, known: &HashSet<String>) -> Option<String> {
    [format!("{joined}.py"), format!("{joined}/__init__.py")].into_iter().find(|candidate| known.contains(candidate))
}

fn python_relative_to_path(spec: &str) -> String {
    let dots = spec.chars().take_while(|ch| *ch == '.').count();
    let rest = spec[dots..].replace('.', "/");
    let prefix = if dots == 1 { ".".to_string() } else { vec![".."; dots.saturating_sub(1)].join("/") };
    if rest.is_empty() { prefix } else { format!("{prefix}/{rest}") }
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

fn unwrap_decorated(node: Node) -> Node {
    if node.kind() != "decorated_definition" { node } else { field(node, "definition").unwrap_or(node) }
}

fn name_of(node: Node, source: &str) -> Option<String> {
    field(node, "name").and_then(|named| {
        let text = node_text(named, source);
        if text.is_empty() { None } else { Some(text.to_string()) }
    })
}

fn kids(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn field<'a>(node: Node<'a>, name: &str) -> Option<Node<'a>> {
    node.child_by_field_name(name)
}

fn node_text<'a>(node: Node, source: &'a str) -> &'a str {
    source.get(node.start_byte()..node.end_byte()).unwrap_or("")
}

fn unwrap_parenthesized(node: Node) -> Node {
    let mut current = node;
    while current.kind() == "parenthesized_expression" {
        let Some(inner) = current.named_child(0) else { break };
        current = inner;
    }
    current
}

fn take_instance_binding(node: Node, source: &str, instances: &mut HashSet<String>) {
    let Some(left) = field(node, "left").or_else(|| field(node, "name")) else { return };
    let name = if left.kind() == "identifier" {
        Some(node_text(left, source).to_string())
    } else if left.kind() == "attribute" {
        let object = field(left, "object");
        let attr = field(left, "attribute");
        if object.is_some_and(|object| object.kind() == "identifier") {
            attr.map(|attr| node_text(attr, source).to_string())
        } else {
            None
        }
    } else {
        None
    };
    let Some(name) = name else { return };
    let Some(right) = field(node, "right").or_else(|| field(node, "value")) else {
        instances.remove(&name);
        return;
    };
    let value = unwrap_parenthesized(right);
    if value.kind() == "identifier" {
        if instances.contains(node_text(value, source)) {
            instances.insert(name);
        } else {
            instances.remove(&name);
        }
        return;
    }
    if value.kind() != "call" {
        instances.remove(&name);
        return;
    }
    let Some(raw) = field(value, "function").or_else(|| value.named_child(0)) else {
        instances.remove(&name);
        return;
    };
    let function = unwrap_parenthesized(raw);
    if function.kind() == "identifier" && node_text(function, source) != "getattr" {
        instances.insert(name);
        return;
    }
    if function.kind() == "attribute" {
        instances.insert(name);
        return;
    }
    instances.remove(&name);
}

fn take_identifier_call(
    enclosing: &str,
    node: Node,
    source: &str,
    edges: &mut Vec<GraphEdge>,
    instances: Option<&HashSet<String>>,
) {
    let Some(raw) = field(node, "function").or_else(|| node.named_child(0)) else { return };
    let callee = unwrap_parenthesized(raw);
    let mut name = None;
    let mut member = false;
    if callee.kind() == "identifier" {
        let text = node_text(callee, source);
        if text == "super" {
            return;
        }
        if instances.is_some_and(|names| names.contains(text)) {
            name = Some("__call__".to_string());
            member = true;
        } else {
            name = Some(text.to_string());
        }
    } else if callee.kind() == "attribute" {
        name = field(callee, "attribute").map(|attr| node_text(attr, source).to_string());
        member = true;
        if name.as_deref().is_some_and(|name| instances.is_some_and(|names| names.contains(name))) {
            name = Some("__call__".to_string());
        }
    } else if callee.kind() == "call" {
        let inner = field(callee, "function").or_else(|| callee.named_child(0));
        if inner.is_none_or(|inner| inner.kind() != "identifier" || node_text(inner, source) != "getattr") {
            name = Some("__call__".to_string());
            member = true;
        } else {
            let second = field(callee, "arguments").and_then(|args| args.named_child(1));
            let content = second.and_then(|second| {
                if second.kind() == "string" {
                    kids(second).into_iter().find(|child| child.kind() == "string_content")
                } else {
                    None
                }
            });
            name = content.map(|content| node_text(content, source).to_string());
            member = true;
        }
    } else if callee.kind() == "subscript" {
        let key = field(callee, "subscript");
        let content = key.and_then(|key| {
            if key.kind() == "string" {
                kids(key).into_iter().find(|child| child.kind() == "string_content")
            } else {
                None
            }
        });
        name = content.map(|content| node_text(content, source).to_string());
        member = true;
    }
    let Some(name) = name else { return };
    let mut props = Map::new();
    if member {
        props.insert("member".to_string(), Value::Bool(true));
    }
    edges.push(GraphEdge {
        src: enclosing.to_string(),
        dst: unresolved_id(&name),
        kind: "calls".to_string(),
        confidence: "extracted".to_string(),
        props,
    });
}
