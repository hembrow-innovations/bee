use std::collections::HashSet;

use serde_json::{Map, Value};
use tree_sitter::Node;

use crate::ids::{file_id, symbol_id, unresolved_id};
use crate::lang_js_call::{
    apply_instance_binding, delete_function_bindings, delete_bound_names, function_instances, has_local_require,
    is_function_value, is_import_type, is_using_assignment, kids, node_text, specifier_is_type, take_call,
    take_declarative_binding, take_jsx_callee, take_require_or_import, unquote, unwrap_callee,
};
use crate::parsers;
use crate::types::{ExtractedGraph, GraphEdge, GraphNode, Manifests, ScannedFile};

pub fn extract_javascript(file: &ScannedFile, manifests: &Manifests) -> ExtractedGraph {
    let _ = manifests;
    let lang = if file.path.ends_with(".tsx") || file.path.ends_with(".jsx") {
        parsers::tsx()
    } else {
        parsers::typescript()
    };
    let Some(tree) = parsers::parse(lang, &file.text) else {
        return ExtractedGraph::default();
    };
    let mut walk = JsWalk {
        path: &file.path,
        source: &file.text,
        nodes: Vec::new(),
        edges: Vec::new(),
        file_node: file_id(&file.path),
        local_require: has_local_require(tree.root_node(), &file.text),
    };
    let mut instances = Some(HashSet::new());
    let file_node = walk.file_node.clone();
    walk.visit(tree.root_node(), None, &file_node, &mut instances);
    ExtractedGraph { nodes: walk.nodes, edges: walk.edges }
}

struct JsWalk<'a> {
    path: &'a str,
    source: &'a str,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    file_node: String,
    local_require: bool,
}

impl JsWalk<'_> {
    fn visit(&mut self, node: Node, class_name: Option<&str>, enclosing: &str, instances: &mut Option<HashSet<String>>) {
        let kind = node.kind();
        if kind == "import_statement" || kind == "export_statement" {
            self.take_import(node);
        } else if is_decl(kind) {
            let name = name_of(node, self.source);
            let is_class = kind == "class_declaration" || kind == "abstract_class_declaration";
            let is_fn = kind == "function_declaration" || kind == "generator_function_declaration";
            let symbol_name = name.as_ref().map(|name| {
                if is_class {
                    class_name.map(|class_name| format!("{class_name}.{name}")).unwrap_or_else(|| name.clone())
                } else {
                    name.clone()
                }
            });
            let owned_id = if let Some(symbol_name) = symbol_name.clone() {
                self.add_symbol(&symbol_name, node, Map::new())
            } else {
                enclosing.to_string()
            };
            let next_owned = if is_class { symbol_name.or_else(|| class_name.map(str::to_string)) } else { class_name.map(str::to_string) };
            let next_class = next_owned.as_deref();
            if is_class {
                let mut child_instances = Some(HashSet::new());
                for child in kids(node) {
                    self.visit(child, next_class, &owned_id, &mut child_instances);
                }
            } else if is_fn {
                let mut child_instances = Some(function_instances(instances.as_ref()));
                delete_function_bindings(node, self.source, child_instances.as_mut().unwrap());
                for child in kids(node) {
                    self.visit(child, next_class, &owned_id, &mut child_instances);
                }
            } else {
                for child in kids(node) {
                    self.visit(child, next_class, &owned_id, instances);
                }
            }
            return;
        } else if matches!(kind, "type_annotation" | "object_type" | "type_literal" | "type_arguments") {
            return;
        } else if kind == "method_signature" || kind == "abstract_method_signature" {
            if let (Some(method), Some(class_name)) = (name_of(node, self.source), class_name) {
                if method == "constructor" || method == "new" {
                    self.add_symbol(&format!("{class_name}.constructor"), node, Map::new());
                } else {
                    self.add_symbol(&member_symbol_name(class_name, &method, node, self.source), node, member_props(node, self.source));
                }
            }
            return;
        } else if matches!(kind, "class" | "class_expression" | "object") {
            let mut none = None;
            for child in kids(node) {
                self.visit(child, None, enclosing, &mut none);
            }
            return;
        } else if kind == "method_definition" {
            let method = name_of(node, self.source);
            if method.as_deref() == Some("constructor") {
                let owned_id = if let Some(class_name) = class_name {
                    self.add_symbol(&format!("{class_name}.constructor"), node, Map::new())
                } else {
                    enclosing.to_string()
                };
                let mut body = Some(function_instances(instances.as_ref()));
                delete_function_bindings(node, self.source, body.as_mut().unwrap());
                for child in kids(node) {
                    self.visit(child, None, &owned_id, &mut body);
                }
                return;
            }
            let full = method.as_ref().and_then(|method| class_name.map(|class_name| member_symbol_name(class_name, method, node, self.source)));
            let owned_id = if let Some(full) = full {
                self.add_symbol(&full, node, member_props(node, self.source))
            } else {
                enclosing.to_string()
            };
            let mut body = Some(function_instances(instances.as_ref()));
            delete_function_bindings(node, self.source, body.as_mut().unwrap());
            for child in kids(node) {
                self.visit(child, None, &owned_id, &mut body);
            }
            return;
        } else if kind == "public_field_definition" {
            let value = field(node, "value");
            let name_node = field(node, "name");
            if let Some(names) = instances.as_mut() {
                if let Some(name_node) = name_node {
                    if matches!(name_node.kind(), "property_identifier" | "identifier") {
                        apply_instance_binding(node_text(name_node, self.source), value, self.source, names);
                    }
                }
            }
            let is_fn = value.is_some_and(is_function_value);
            let method = name_of(node, self.source);
            let full = method.as_ref().and_then(|method| {
                if is_fn { class_name.map(|class_name| format!("{class_name}.{method}")) } else { None }
            });
            let owned_id = if let Some(full) = full.clone() {
                self.add_symbol(&full, node, member_props(node, self.source))
            } else {
                enclosing.to_string()
            };
            if let Some(value) = value {
                if full.is_some() {
                    self.visit_stamped_function(value, None, &owned_id, instances.as_ref());
                } else {
                    self.visit(value, None, &owned_id, instances);
                }
            }
            return;
        } else if kind == "class_static_block" {
            let mut body = Some(function_instances(instances.as_ref()));
            for child in kids(node) {
                self.visit(child, class_name, enclosing, &mut body);
            }
            return;
        } else if kind == "lexical_declaration" || kind == "variable_declaration" {
            for child in kids(node) {
                if child.kind() != "variable_declarator" {
                    self.visit(child, class_name, enclosing, instances);
                    continue;
                }
                let name_node = field(child, "name");
                let value = field(child, "value");
                if let (Some(names), Some(name_node)) = (instances.as_mut(), name_node) {
                    take_declarative_binding(name_node, value, self.source, names);
                }
                if name_node.is_some_and(|name_node| name_node.kind() == "identifier") && value.is_some() {
                    let object_node = unwrap_callee(value.unwrap());
                    if object_node.kind() == "object" {
                        self.visit_assigned_object(object_node, node_text(name_node.unwrap(), self.source), enclosing);
                        continue;
                    }
                }
                let stamped = name_node.is_some_and(|name_node| name_node.kind() == "identifier") && value.is_some_and(is_function_value);
                let owned_id = if stamped {
                    self.add_symbol(node_text(name_node.unwrap(), self.source), child, Map::new())
                } else {
                    enclosing.to_string()
                };
                if stamped {
                    if let Some(value) = value {
                        self.visit_stamped_function(value, class_name, &owned_id, instances.as_ref());
                    }
                } else {
                    self.visit(child, class_name, &owned_id, instances);
                }
            }
            return;
        } else if matches!(
            kind,
            "arrow_function" | "function" | "function_expression" | "generator_function" | "generator_function_expression"
        ) {
            let line_name = unnamed_callable_name(node);
            let owned_id = if let Some(line_name) = line_name {
                self.add_symbol(&line_name, node, Map::new())
            } else {
                enclosing.to_string()
            };
            let mut body = Some(function_instances(instances.as_ref()));
            delete_function_bindings(node, self.source, body.as_mut().unwrap());
            for child in kids(node) {
                self.visit(child, class_name, &owned_id, &mut body);
            }
            return;
        } else if kind == "assignment_expression" {
            if instances.is_some() && !is_using_assignment(node, self.source) {
                let left = field(node, "left").or_else(|| node.named_child(0));
                let right = field(node, "right").or_else(|| node.named_child(1));
                if let Some(left) = left {
                    take_declarative_binding(left, right, self.source, instances.as_mut().unwrap());
                }
            }
        } else if kind == "catch_clause" {
            let mut inner = instances.clone();
            if let Some(names) = inner.as_mut() {
                delete_bound_names(field(node, "parameter").or_else(|| node.named_child(0)), self.source, names);
            }
            for child in kids(node) {
                self.visit(child, class_name, enclosing, &mut inner);
            }
            return;
        } else if matches!(kind, "for_statement" | "for_in_statement" | "for_of_statement") {
            let mut inner = instances.clone();
            if let Some(names) = inner.as_mut() {
                let bound = if kind == "for_statement" { field(node, "initializer") } else { field(node, "left") };
                delete_bound_names(bound, self.source, names);
            }
            for child in kids(node) {
                self.visit(child, class_name, enclosing, &mut inner);
            }
            return;
        } else if kind == "call_expression" || kind == "new_expression" {
            let skipped = kind == "call_expression"
                && take_require_or_import(self.path, node, self.source, &mut self.edges, self.local_require);
            if !skipped {
                take_call(enclosing, node, self.source, &mut self.edges, self.local_require, instances.as_ref());
            }
        } else if kind == "jsx_opening_element" || kind == "jsx_self_closing_element" {
            take_jsx_callee(enclosing, node, self.source, &mut self.edges);
        }
        for child in kids(node) {
            let import_child = child.kind() == "import_statement";
            self.visit(child, class_name, enclosing, instances);
            if import_child && instances.is_some() && kind == "program" {
                seed_import_locals(child, self.source, instances.as_mut().unwrap());
            }
        }
    }

    fn visit_stamped_function(&mut self, node: Node, class_name: Option<&str>, enclosing: &str, instances: Option<&HashSet<String>>) {
        let core = unwrap_callee(node);
        let mut body = Some(function_instances(instances));
        delete_function_bindings(core, self.source, body.as_mut().unwrap());
        for child in kids(core) {
            self.visit(child, class_name, enclosing, &mut body);
        }
    }

    fn visit_assigned_object(&mut self, object_node: Node, lhs: &str, enclosing: &str) {
        for child in kids(object_node) {
            if self.stamp_assigned_object_member(child, lhs) {
                continue;
            }
            let mut none = None;
            self.visit(child, None, enclosing, &mut none);
        }
    }

    fn stamp_assigned_object_member(&mut self, child: Node, lhs: &str) -> bool {
        if child.kind() == "method_definition" {
            if accessor_of(child, self.source).is_some() {
                return false;
            }
            let Some(key) = object_member_key(child, self.source) else { return false };
            let id = self.add_symbol(&format!("{lhs}.{key}"), child, Map::new());
            self.visit_stamped_function(child, None, &id, None);
            return true;
        }
        if child.kind() == "pair" {
            let value = field(child, "value");
            if !value.is_some_and(is_function_value) {
                return false;
            }
            let Some(key) = object_member_key(child, self.source) else { return false };
            let id = self.add_symbol(&format!("{lhs}.{key}"), child, Map::new());
            self.visit_stamped_function(value.unwrap(), None, &id, None);
            return true;
        }
        false
    }

    fn add_symbol(&mut self, name: &str, node: Node, mut extra: Map<String, Value>) -> String {
        let id = symbol_id(self.path, name);
        extra.insert("syntax".to_string(), Value::String(node.kind().to_string()));
        self.nodes.push(GraphNode {
            id: id.clone(),
            kind: "symbol".to_string(),
            name: name.to_string(),
            file_path: Some(self.path.to_string()),
            start_line: Some(node.start_position().row as i64 + 1),
            end_line: Some(node.end_position().row as i64 + 1),
            body: None,
            props: extra,
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
        let source = field(node, "source").or_else(|| import_require_source(node));
        let Some(source) = source else { return };
        let spec = unquote(node_text(source, self.source));
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

fn field<'a>(node: Node<'a>, name: &str) -> Option<Node<'a>> {
    node.child_by_field_name(name)
}

fn is_decl(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration"
            | "function_signature"
            | "generator_function_declaration"
            | "class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
            | "abstract_class_declaration"
    )
}

fn seed_import_locals(node: Node, source: &str, instances: &mut HashSet<String>) {
    if is_import_type(node_text(node, source)) {
        return;
    }
    let Some(clause) = kids(node).into_iter().find(|child| child.kind() == "import_clause") else { return };
    add_import_instance_names(clause, source, instances);
}

fn add_import_instance_name(name: &str, instances: &mut HashSet<String>) {
    if name != "require" {
        instances.insert(name.to_string());
    }
}

fn add_import_instance_names(node: Node, source: &str, instances: &mut HashSet<String>) {
    if node.kind() == "import_specifier" {
        if specifier_is_type(node_text(node, source)) {
            return;
        }
        let local = field(node, "alias").or_else(|| field(node, "name"));
        if let Some(local) = local {
            add_import_instance_name(node_text(local, source), instances);
        }
        return;
    }
    if node.kind() == "namespace_import" {
        let name = field(node, "name").or_else(|| kids(node).into_iter().find(|child| child.kind() == "identifier"));
        if let Some(name) = name {
            add_import_instance_name(node_text(name, source), instances);
        }
        return;
    }
    if node.kind() == "import_clause" {
        for child in kids(node) {
            if child.kind() == "identifier" {
                add_import_instance_name(node_text(child, source), instances);
            } else {
                add_import_instance_names(child, source, instances);
            }
        }
        return;
    }
    for child in kids(node) {
        add_import_instance_names(child, source, instances);
    }
}

fn import_require_source(node: Node) -> Option<Node> {
    kids(node).into_iter().find(|child| child.kind() == "import_require_clause").and_then(|child| field(child, "source"))
}

fn unnamed_callable_name(node: Node) -> Option<String> {
    let line = node.start_position().row as i64 + 1;
    if node.kind() == "arrow_function" {
        return Some(format!("arrow:{line}"));
    }
    if matches!(node.kind(), "function" | "function_expression" | "generator_function" | "generator_function_expression") {
        if field(node, "name").is_some() {
            return None;
        }
        return Some(format!("function:{line}"));
    }
    None
}

fn object_member_key(node: Node, source: &str) -> Option<String> {
    let named = field(node, "name").or_else(|| field(node, "key"))?;
    if named.kind() == "computed_property_name" {
        let inner = named.named_child(0)?;
        let core = unwrap_callee(inner);
        if matches!(core.kind(), "string" | "string_fragment") {
            let text = unquote(node_text(core, source));
            return if text.is_empty() { None } else { Some(text.to_string()) };
        }
        return None;
    }
    if matches!(named.kind(), "string" | "string_fragment") {
        let text = unquote(node_text(named, source));
        return if text.is_empty() { None } else { Some(text.to_string()) };
    }
    if matches!(named.kind(), "property_identifier" | "identifier") {
        let text = node_text(named, source);
        return if text.is_empty() { None } else { Some(text.to_string()) };
    }
    None
}

fn name_of(node: Node, source: &str) -> Option<String> {
    if let Some(named) = field(node, "name") {
        if named.kind() == "computed_property_name" {
            return None;
        }
        if matches!(named.kind(), "string" | "string_fragment") {
            let text = unquote(node_text(named, source));
            return if text.is_empty() { None } else { Some(text.to_string()) };
        }
        let text = node_text(named, source);
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    kids(node).into_iter().find_map(|child| {
        if matches!(child.kind(), "identifier" | "type_identifier" | "property_identifier" | "private_property_identifier") {
            Some(node_text(child, source).to_string())
        } else {
            None
        }
    })
}

fn member_prefix(text: &str) -> &str {
    let mut index = 0;
    loop {
        let rest = &text[index..];
        if rest.starts_with('*') {
            index += '*'.len_utf8();
            index += text[index..].chars().take_while(|ch| ch.is_whitespace()).map(|ch| ch.len_utf8()).sum::<usize>();
            continue;
        }
        let word_len = rest.find(|ch: char| ch.is_whitespace()).unwrap_or(rest.len());
        let word = &rest[..word_len];
        if matches!(word, "public" | "private" | "protected" | "readonly" | "abstract" | "declare" | "override" | "static" | "async")
            && rest[word_len..].starts_with(|ch: char| ch.is_whitespace())
        {
            index += word_len;
            index += text[index..].chars().take_while(|ch| ch.is_whitespace()).map(|ch| ch.len_utf8()).sum::<usize>();
            continue;
        }
        break;
    }
    &text[..index]
}

fn accessor_of(node: Node, source: &str) -> Option<&'static str> {
    let text = node_text(node, source);
    let rest = &text[member_prefix(text).len()..];
    if rest.starts_with("get ") { Some("get") } else if rest.starts_with("set ") { Some("set") } else { None }
}

fn is_static_member(node: Node, source: &str) -> bool {
    member_prefix(node_text(node, source)).split_whitespace().any(|word| word == "static")
}

fn member_symbol_name(class_name: &str, method: &str, node: Node, source: &str) -> String {
    match accessor_of(node, source) {
        Some(accessor) => format!("{class_name}.{accessor}.{method}"),
        None => format!("{class_name}.{method}"),
    }
}

fn member_props(node: Node, source: &str) -> Map<String, Value> {
    let mut props = Map::new();
    if let Some(accessor) = accessor_of(node, source) {
        props.insert("accessor".to_string(), Value::String(accessor.to_string()));
    }
    if is_static_member(node, source) {
        props.insert("static".to_string(), Value::Bool(true));
    }
    props
}

pub fn resolve_js_import(spec: &str, from: &str, known: &HashSet<String>) -> Option<String> {
    if !spec.starts_with('.') {
        return None;
    }
    let dir = from.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let joined_raw = if dir.is_empty() { spec.to_string() } else { format!("{dir}/{spec}") };
    let joined = normalize_rel(&joined_raw)?;
    import_candidates(&joined).into_iter().find(|candidate| known.contains(candidate))
}

fn import_candidates(joined: &str) -> Vec<String> {
    if joined.is_empty() {
        return ["index.ts", "index.tsx", "index.js", "index.jsx", "index.mts", "index.cts", "index.mjs", "index.cjs"]
            .into_iter()
            .map(str::to_string)
            .collect();
    }
    let mut out = typescript_remaps(joined);
    for ext in [".ts", ".tsx", ".js", ".jsx", ".mts", ".mjs", ".cts", ".cjs", ".d.ts"] {
        out.push(format!("{joined}{ext}"));
    }
    for ext in [".ts", ".tsx", ".js", ".jsx", ".mts", ".cts", ".mjs", ".cjs"] {
        out.push(format!("{joined}/index{ext}"));
    }
    out
}

fn typescript_remaps(path: &str) -> Vec<String> {
    let remaps = [(".js", ".ts"), (".js", ".tsx"), (".js", ".mts"), (".js", ".d.ts"), (".jsx", ".tsx"), (".mjs", ".mts"), (".cjs", ".cts")];
    let mut out = vec![path.to_string()];
    for (from, to) in remaps {
        if let Some(stem) = path.strip_suffix(from) {
            out.push(format!("{stem}{to}"));
        }
    }
    out
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
