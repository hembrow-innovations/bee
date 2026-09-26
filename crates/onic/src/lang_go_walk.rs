use std::collections::{HashMap, HashSet};

use serde_json::Value;
use tree_sitter::Node;

use crate::ids::{file_id, symbol_id, unresolved_id};
use crate::types::{empty_props, ExtractedGraph, GraphEdge, GraphNode, ScannedFile};

const TYPE_SPECS: &[&str] = &["type_spec", "type_alias"];

pub(crate) fn extract(file: &ScannedFile) -> ExtractedGraph {
    let Some(tree) = crate::parsers::parse(crate::parsers::go(), &file.text) else {
        return ExtractedGraph::default();
    };
    let mut cx = Cx {
        file,
        file_node: file_id(&file.path),
        nodes: Vec::new(),
        edges: Vec::new(),
    };
    let root = tree.root_node();
    let mut init_count = 0i64;
    for child in named(root) {
        if child.kind() == "function_declaration" && cx.field_name(child).as_deref() == Some("init") {
            init_count += 1;
        }
    }
    let mut package_callables = HashMap::new();
    let mut type_names = HashSet::new();
    for child in named(root) {
        if child.kind() == "type_declaration" {
            cx.collect_type_spec_names(child, &mut type_names);
        }
    }
    for child in named(root) {
        match child.kind() {
            "function_declaration" => {
                let Some(name) = cx.field_name(child) else {
                    continue;
                };
                let symbol_name = if name == "init" && init_count >= 2 {
                    format!("init:{}", child.start_position().row + 1)
                } else {
                    name
                };
                let id = cx.add_symbol(&symbol_name, child, "function_declaration");
                let mut callables = package_callables.clone();
                cx.delete_parameter_identifiers(child.child_by_field_name("parameters"), &mut callables);
                cx.delete_parameter_identifiers(child.child_by_field_name("result"), &mut callables);
                let mut local_types = type_names.clone();
                cx.walk_calls(child.child_by_field_name("body"), &id, &mut callables, &mut local_types);
            }
            "type_declaration" => cx.visit_specs(child, TYPE_SPECS, "type_declaration", true),
            "const_declaration" => cx.visit_specs(child, &["const_spec"], "const_declaration", false),
            "var_declaration" => {
                cx.visit_specs(child, &["var_spec"], "var_declaration", false);
                cx.bind_file_root_var_specs(child, &mut package_callables);
            }
            "method_declaration" => cx.visit_method(child, &package_callables, &type_names),
            "import_declaration" => cx.visit_import_specs(child, &mut package_callables),
            _ => {}
        }
    }
    ExtractedGraph { nodes: cx.nodes, edges: cx.edges }
}

struct Cx<'a> {
    file: &'a ScannedFile,
    file_node: String,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

impl<'a> Cx<'a> {
    fn text(&self, node: Node) -> String {
        node.utf8_text(self.file.text.as_bytes()).unwrap_or("").to_string()
    }

    fn field_name(&self, node: Node) -> Option<String> {
        let named = node.child_by_field_name("name")?;
        let text = self.text(named);
        if text.is_empty() || text == "_" { None } else { Some(text) }
    }

    fn field_names(&self, spec: Node) -> Vec<String> {
        self.field_name(spec).into_iter().collect()
    }

    fn leading_identifier_names(&self, spec: Node) -> Vec<String> {
        let mut names = Vec::new();
        for child in named(spec) {
            if child.kind() != "identifier" {
                break;
            }
            let text = self.text(child);
            if !text.is_empty() && text != "_" {
                names.push(text);
            }
        }
        names
    }

    fn add_symbol(&mut self, name: &str, node: Node, syntax: &str) -> String {
        let id = symbol_id(&self.file.path, name);
        let mut props = empty_props();
        props.insert("syntax".into(), Value::String(syntax.to_string()));
        self.nodes.push(GraphNode {
            id: id.clone(),
            kind: "symbol".into(),
            name: name.to_string(),
            file_path: Some(self.file.path.clone()),
            start_line: Some(node.start_position().row as i64 + 1),
            end_line: Some(node.end_position().row as i64 + 1),
            body: None,
            props,
        });
        self.edges.push(GraphEdge {
            src: self.file_node.clone(),
            dst: id.clone(),
            kind: "contains".into(),
            confidence: "extracted".into(),
            props: empty_props(),
        });
        id
    }

    fn push_import(&mut self, spec: &str) {
        let mut props = empty_props();
        props.insert("spec".into(), Value::String(spec.to_string()));
        self.edges.push(GraphEdge {
            src: self.file_node.clone(),
            dst: unresolved_id(spec),
            kind: "imports".into(),
            confidence: "extracted".into(),
            props,
        });
    }

    fn push_call(&mut self, src: &str, name: &str, member: bool) {
        let mut props = empty_props();
        if member {
            props.insert("member".into(), Value::Bool(true));
        }
        self.edges.push(GraphEdge {
            src: src.to_string(),
            dst: unresolved_id(name),
            kind: "calls".into(),
            confidence: "extracted".into(),
            props,
        });
    }

    fn visit_import_specs(&mut self, node: Node, package_callables: &mut HashMap<String, String>) {
        if node.kind() == "import_spec" {
            self.take_path_only_import(node);
            if let Some(name) = node.child_by_field_name("name") {
                if matches!(name.kind(), "package_identifier" | "identifier") {
                    let text = self.text(name);
                    if !text.is_empty() && text != "_" {
                        package_callables.insert(text.clone(), text);
                    }
                }
            }
            return;
        }
        for child in named(node) {
            self.visit_import_specs(child, package_callables);
        }
    }

    fn take_path_only_import(&mut self, spec_node: Node) {
        let Some(path_node) = spec_node.child_by_field_name("path") else {
            return;
        };
        if path_node.kind() != "interpreted_string_literal" && path_node.kind() != "raw_string_literal" {
            return;
        }
        let Some(unquoted) = unquote_import_path(&self.text(path_node)) else {
            return;
        };
        if unquoted.is_empty() {
            return;
        }
        self.push_import(&unquoted);
    }

    fn collect_type_spec_names(&self, node: Node, type_names: &mut HashSet<String>) {
        if TYPE_SPECS.contains(&node.kind()) {
            for name in self.field_names(node) {
                type_names.insert(name);
            }
            return;
        }
        for child in named(node) {
            self.collect_type_spec_names(child, type_names);
        }
    }

    fn visit_specs(&mut self, node: Node, spec_types: &[&str], syntax: &str, typed: bool) {
        if spec_types.contains(&node.kind()) {
            let names = if typed { self.field_names(node) } else { self.leading_identifier_names(node) };
            for name in names {
                let id = self.add_symbol(&name, node, syntax);
                if node.kind() != "type_spec" && node.kind() != "type_alias" {
                    continue;
                }
                let Some(type_node) = node.child_by_field_name("type") else {
                    continue;
                };
                if type_node.kind() == "struct_type" {
                    let embeds = self.collect_struct_embeds(type_node);
                    if !embeds.is_empty() {
                        if let Some(added) = self.nodes.last_mut() {
                            if added.id == id {
                                added.props.insert(
                                    "embed".into(),
                                    Value::Array(embeds.into_iter().map(Value::String).collect()),
                                );
                            }
                        }
                    }
                    continue;
                }
                if type_node.kind() != "interface_type" {
                    continue;
                }
                for child in named(type_node) {
                    if child.kind() != "method_elem" {
                        continue;
                    }
                    let Some(method_name) = self.field_name(child) else {
                        continue;
                    };
                    self.add_symbol(&format!("{name}.{method_name}"), child, "method_elem");
                }
            }
            return;
        }
        for child in named(node) {
            self.visit_specs(child, spec_types, syntax, typed);
        }
    }

    fn collect_struct_embeds(&self, struct_type: Node) -> Vec<String> {
        let mut embeds = Vec::new();
        for list in named(struct_type) {
            if list.kind() != "field_declaration_list" {
                continue;
            }
            for field in named(list) {
                if field.kind() != "field_declaration" || field.child_by_field_name("name").is_some() {
                    continue;
                }
                let Some(type_field) = field.child_by_field_name("type") else {
                    continue;
                };
                if type_field.kind() != "type_identifier"
                    && type_field.kind() != "qualified_type"
                    && type_field.kind() != "generic_type"
                {
                    continue;
                }
                let text = self.text(type_field);
                if !text.is_empty() {
                    embeds.push(text);
                }
            }
        }
        embeds
    }

    fn visit_method(&mut self, node: Node, package_callables: &HashMap<String, String>, type_names: &HashSet<String>) {
        let Some(method_name) = self.field_name(node) else {
            return;
        };
        let Some(type_name) = self.receiver_type_name(node) else {
            return;
        };
        let id = self.add_symbol(&format!("{type_name}.{method_name}"), node, "method_declaration");
        let mut callables = package_callables.clone();
        self.delete_parameter_identifiers(node.child_by_field_name("receiver"), &mut callables);
        self.delete_parameter_identifiers(node.child_by_field_name("parameters"), &mut callables);
        self.delete_parameter_identifiers(node.child_by_field_name("result"), &mut callables);
        let mut local_types = type_names.clone();
        self.walk_calls(node.child_by_field_name("body"), &id, &mut callables, &mut local_types);
    }

    fn receiver_type_name(&self, method: Node) -> Option<String> {
        let receiver = method.child_by_field_name("receiver")?;
        let params = named(receiver);
        if params.len() != 1 || params[0].kind() != "parameter_declaration" {
            return None;
        }
        let mut type_node = params[0].child_by_field_name("type")?;
        if type_node.kind() == "pointer_type" {
            type_node = type_node.child_by_field_name("type").or_else(|| named(type_node).into_iter().next())?;
        }
        if type_node.kind() == "type_identifier" {
            let text = self.text(type_node);
            return if text.is_empty() { None } else { Some(text) };
        }
        if type_node.kind() == "generic_type" {
            let inner = type_node.child_by_field_name("type")?;
            if inner.kind() != "type_identifier" {
                return None;
            }
            let text = self.text(inner);
            return if text.is_empty() { None } else { Some(text) };
        }
        None
    }

    fn delete_parameter_identifiers(&self, list: Option<Node>, callables: &mut HashMap<String, String>) {
        let Some(list) = list else {
            return;
        };
        if list.kind() != "parameter_list" {
            return;
        }
        for child in named(list) {
            if child.kind() != "parameter_declaration" {
                continue;
            }
            for name in self.leading_identifier_names(child) {
                callables.remove(&name);
            }
        }
    }

    fn bind_file_root_var_specs(&self, node: Node, callables: &mut HashMap<String, String>) {
        if node.kind() == "var_spec" {
            self.bind_var_spec(node, callables);
            return;
        }
        for child in named(node) {
            self.bind_file_root_var_specs(child, callables);
        }
    }

    fn bind_var_spec(&self, node: Node, callables: &mut HashMap<String, String>) {
        let Some(value) = node.child_by_field_name("value") else {
            return;
        };
        if value.kind() != "expression_list" {
            return;
        }
        let mut lhs_nodes = Vec::new();
        for child in named(node) {
            if child.kind() != "identifier" {
                break;
            }
            lhs_nodes.push(child);
        }
        self.bind_paired_callables(&lhs_nodes, &named(value), callables);
    }

    fn bind_short_var(&self, node: Node, callables: &mut HashMap<String, String>) {
        let Some(left) = node.child_by_field_name("left") else {
            return;
        };
        let Some(right) = node.child_by_field_name("right") else {
            return;
        };
        if left.kind() != "expression_list" || right.kind() != "expression_list" {
            return;
        }
        self.bind_paired_callables(&named(left), &named(right), callables);
    }

    fn bind_paired_callables(&self, lhs_nodes: &[Node], rhs_nodes: &[Node], callables: &mut HashMap<String, String>) {
        if lhs_nodes.len() != rhs_nodes.len() {
            for lhs_node in lhs_nodes {
                if let Some(lhs) = self.bindable_lhs_name(*lhs_node) {
                    callables.remove(&lhs);
                }
            }
            return;
        }
        for (lhs_node, rhs_node) in lhs_nodes.iter().zip(rhs_nodes.iter()) {
            let Some(lhs) = self.bindable_lhs_name(*lhs_node) else {
                continue;
            };
            self.snapshot_callable_rhs(&lhs, *rhs_node, callables);
        }
    }

    fn bindable_lhs_name(&self, node: Node) -> Option<String> {
        if node.kind() != "identifier" {
            return None;
        }
        let text = self.text(node);
        if text.is_empty() || text == "_" { None } else { Some(text) }
    }

    fn snapshot_callable_rhs(&self, lhs: &str, rhs: Node, callables: &mut HashMap<String, String>) {
        let Some(core) = unwrap_parenthesized(Some(rhs)) else {
            callables.remove(lhs);
            return;
        };
        if core.kind() == "identifier" {
            let text = self.text(core);
            if !text.is_empty() && text != "_" {
                let mapped = callables.get(&text).cloned().unwrap_or(text);
                callables.insert(lhs.to_string(), mapped);
                return;
            }
        }
        if core.kind() == "func_literal" {
            callables.insert(lhs.to_string(), format!("func:{}", core.start_position().row + 1));
            return;
        }
        if core.kind() == "selector_expression" {
            if let Some(field) = core.child_by_field_name("field") {
                if matches!(field.kind(), "field_identifier" | "identifier") {
                    let text = self.text(field);
                    if !text.is_empty() && text != "_" {
                        callables.insert(lhs.to_string(), format!("member:{text}"));
                        return;
                    }
                }
            }
        }
        callables.remove(lhs);
    }

    fn walk_calls(
        &mut self,
        node: Option<Node>,
        enclosing: &str,
        callables: &mut HashMap<String, String>,
        type_names: &mut HashSet<String>,
    ) {
        let Some(node) = node else {
            return;
        };
        if TYPE_SPECS.contains(&node.kind()) {
            for name in self.field_names(node) {
                type_names.insert(name);
            }
        }
        if node.kind() == "func_literal" {
            let id = self.add_symbol(&format!("func:{}", node.start_position().row + 1), node, "func_literal");
            let mut inner = callables.clone();
            self.delete_parameter_identifiers(node.child_by_field_name("parameters"), &mut inner);
            self.delete_parameter_identifiers(node.child_by_field_name("result"), &mut inner);
            let mut inner_types = type_names.clone();
            for child in named(node) {
                self.walk_calls(Some(child), &id, &mut inner, &mut inner_types);
            }
            return;
        }
        if node.kind() == "short_var_declaration" {
            self.bind_short_var(node, callables);
            for child in named(node) {
                self.walk_calls(Some(child), enclosing, callables, type_names);
            }
            return;
        }
        if node.kind() == "var_spec" {
            self.bind_var_spec(node, callables);
            for child in named(node) {
                self.walk_calls(Some(child), enclosing, callables, type_names);
            }
            return;
        }
        if node.kind() == "range_clause" && self.text(node).contains(":=") {
            if let Some(left) = node.child_by_field_name("left") {
                if left.kind() == "expression_list" {
                    for child in named(left) {
                        if let Some(lhs) = self.bindable_lhs_name(child) {
                            callables.remove(&lhs);
                        }
                    }
                }
            }
        }
        if node.kind() == "call_expression" {
            self.emit_call(node, enclosing, callables, type_names);
        }
        for child in named(node) {
            self.walk_calls(Some(child), enclosing, callables, type_names);
        }
    }

    fn emit_call(&mut self, node: Node, enclosing: &str, callables: &HashMap<String, String>, type_names: &HashSet<String>) {
        let Some(callee) = unwrap_parenthesized(node.child_by_field_name("function")) else {
            return;
        };
        if callee.kind() == "identifier" {
            let name = self.text(callee);
            if name.is_empty() {
                return;
            }
            if let Some(mapped) = callables.get(&name) {
                if let Some(member) = mapped.strip_prefix("member:") {
                    self.push_call(enclosing, member, true);
                } else {
                    self.push_call(enclosing, mapped, false);
                }
            } else if !type_names.contains(&name) {
                self.push_call(enclosing, &name, false);
            }
            return;
        }
        if callee.kind() == "selector_expression" {
            let Some(field) = callee.child_by_field_name("field") else {
                return;
            };
            if field.kind() != "field_identifier" && field.kind() != "identifier" {
                return;
            }
            let text = self.text(field);
            if !text.is_empty() {
                self.push_call(enclosing, &text, true);
            }
        }
    }
}

fn named(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn unwrap_parenthesized(node: Option<Node>) -> Option<Node> {
    let mut current = node;
    while let Some(node) = current {
        if node.kind() != "parenthesized_expression" {
            break;
        }
        let Some(inner) = named(node).into_iter().next() else {
            break;
        };
        current = Some(inner);
    }
    current
}

fn unquote_import_path(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let start = bytes[0];
    let end = bytes[bytes.len() - 1];
    if start != end || (start != b'"' && start != b'`') {
        return None;
    }
    Some(text[1..text.len() - 1].to_string())
}
