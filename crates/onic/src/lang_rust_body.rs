use std::collections::HashMap;

use serde_json::{Map, Value};
use tree_sitter::Node;

use crate::ids::{file_id, symbol_id, unresolved_id};
use crate::types::{empty_props, GraphEdge, GraphNode, ScannedFile};

pub(crate) fn extract(
    file: &ScannedFile,
    root: Node,
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
) {
    let mut cx = Cx {
        file,
        file_node: file_id(&file.path),
        nodes,
        edges,
    };
    let mut module_callables = HashMap::new();
    for child in named(root) {
        cx.add_declared_item(child, &mut module_callables);
        if child.kind() == "use_declaration" && cx.take_use_import(child) {
            cx.seed_use_local(child, &mut module_callables);
        }
        if child.kind() == "mod_item" {
            cx.take_mod_import(child);
            cx.add_inline_mod(child, "");
        }
    }
}

struct Cx<'a> {
    file: &'a ScannedFile,
    file_node: String,
    nodes: &'a mut Vec<GraphNode>,
    edges: &'a mut Vec<GraphEdge>,
}

impl<'a> Cx<'a> {
    fn text(&self, node: Node) -> String {
        node.utf8_text(self.file.text.as_bytes()).unwrap_or("").to_string()
    }

    fn name_of(&self, node: Node) -> Option<String> {
        let named = node.child_by_field_name("name")?;
        let text = self.text(named);
        if text.is_empty() { None } else { Some(text) }
    }

    fn named_kind<'b>(&self, node: Node<'b>, kind: &str) -> Option<Node<'b>> {
        named(node).into_iter().find(|child| child.kind() == kind)
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

    fn add_symbol(&mut self, name: &str, node: Node, fallback_abi: Option<String>) -> String {
        let id = symbol_id(&self.file.path, name);
        let mut props = Map::new();
        props.insert("syntax".into(), Value::String(node.kind().to_string()));
        let abi = self.abi_from_function_modifiers(node).or(fallback_abi);
        if let Some(abi) = abi {
            props.insert("abi".into(), Value::String(abi));
        }
        self.insert_keyword_props(node, &mut props);
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

    fn take_mod_import(&mut self, node: Node) {
        if node.child_by_field_name("body").is_some() {
            return;
        }
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let spec = self.text(name);
        if spec.is_empty() {
            return;
        }
        self.push_import(&spec);
    }

    fn take_use_import(&mut self, node: Node) -> bool {
        if let Some(visibility) = named(node).into_iter().find(|child| child.kind() == "visibility_modifier") {
            if self.text(visibility) != "pub" {
                return false;
            }
        }
        let Some(argument) = node.child_by_field_name("argument") else {
            return false;
        };
        let Some(spec) = self.use_import_spec(argument) else {
            return false;
        };
        if spec.is_empty() {
            return false;
        }
        self.push_import(&spec);
        true
    }

    fn use_import_spec(&self, argument: Node) -> Option<String> {
        if is_simple_use_path(argument) {
            return Some(self.text(argument));
        }
        if argument.kind() == "use_as_clause" {
            let path = argument.child_by_field_name("path")?;
            if !is_simple_use_path(path) {
                return None;
            }
            let text = self.text(path);
            return if text.is_empty() { None } else { Some(text) };
        }
        if argument.kind() == "use_wildcard" {
            let child = named(argument).into_iter().next()?;
            if !is_simple_use_path(child) {
                return None;
            }
            let text = self.text(child);
            return if text.is_empty() { None } else { Some(text) };
        }
        None
    }

    fn seed_use_local(&self, node: Node, callables: &mut HashMap<String, String>) {
        let Some(argument) = node.child_by_field_name("argument") else {
            return;
        };
        if is_simple_use_path(argument) {
            let Some(last_ident) = self.last_use_ident(argument) else {
                return;
            };
            if last_ident == "crate" || last_ident == "self" || last_ident == "super" || last_ident == "_" {
                return;
            }
            callables.insert(last_ident.clone(), last_ident);
            return;
        }
        if argument.kind() != "use_as_clause" {
            return;
        }
        let local = argument.child_by_field_name("alias").map(|alias| self.text(alias)).unwrap_or_default();
        if local.is_empty() || local == "_" {
            return;
        }
        let Some(path) = argument.child_by_field_name("path") else {
            return;
        };
        if !is_simple_use_path(path) {
            return;
        }
        let Some(last_ident) = self.last_use_ident(path) else {
            return;
        };
        callables.insert(local, last_ident);
    }

    fn last_use_ident(&self, path: Node) -> Option<String> {
        if path.kind() == "identifier" {
            let text = self.text(path);
            return if text.is_empty() { None } else { Some(text) };
        }
        if path.kind() == "scoped_identifier" {
            let name = path.child_by_field_name("name")?;
            if name.kind() != "identifier" {
                return None;
            }
            let text = self.text(name);
            return if text.is_empty() { None } else { Some(text) };
        }
        if matches!(path.kind(), "self" | "super" | "crate") {
            let text = self.text(path);
            return if text.is_empty() { None } else { Some(text) };
        }
        None
    }

    fn strip_abi_quotes(&self, text: &str) -> Option<String> {
        let bytes = text.as_bytes();
        if bytes.len() >= 2 {
            let start = bytes[0] as char;
            let end = bytes[bytes.len() - 1] as char;
            if (start == '"' && end == '"') || (start == '\'' && end == '\'') {
                let inner = &text[1..text.len() - 1];
                return if inner.is_empty() { None } else { Some(inner.to_string()) };
            }
        }
        if text.is_empty() { None } else { Some(text.to_string()) }
    }

    fn abi_string(&self, modifier: Node) -> Option<String> {
        if let Some(direct) = self.named_kind(modifier, "string_content") {
            let text = self.text(direct);
            if !text.is_empty() {
                return Some(text);
            }
        }
        let literal = self.named_kind(modifier, "string_literal")?;
        if let Some(nested) = self.named_kind(literal, "string_content") {
            let text = self.text(nested);
            if !text.is_empty() {
                return Some(text);
            }
        }
        self.strip_abi_quotes(&self.text(literal))
    }

    fn abi_from_extern_modifier(&self, node: Node) -> Option<String> {
        self.abi_string(self.named_kind(node, "extern_modifier")?)
    }

    fn abi_from_function_modifiers(&self, node: Node) -> Option<String> {
        self.abi_from_extern_modifier(self.named_kind(node, "function_modifiers")?)
    }

    fn insert_keyword_props(&self, node: Node, props: &mut Map<String, Value>) {
        let Some(modifiers) = self.named_kind(node, "function_modifiers") else {
            return;
        };
        let mut remainder = self.text(modifiers);
        for child in named(modifiers) {
            if child.kind() == "extern_modifier" {
                remainder = remainder.replacen(&self.text(child), "", 1);
            }
        }
        for token in remainder.split_whitespace() {
            if token == "async" || token == "const" || token == "unsafe" {
                props.insert(token.to_string(), Value::Bool(true));
            }
        }
    }

    fn inherent_type_name(&self, impl_node: Node) -> Option<String> {
        let type_node = impl_node.child_by_field_name("type")?;
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

    fn add_trait_methods(&mut self, trait_node: Node, trait_name: &str) {
        let Some(body) = trait_node.child_by_field_name("body") else {
            return;
        };
        for member in named(body) {
            if member.kind() == "macro_definition" {
                if let Some(name) = self.name_of(member) {
                    self.add_symbol(&name, member, None);
                }
                continue;
            }
            if member.kind() != "function_signature_item" && member.kind() != "function_item" {
                continue;
            }
            let Some(fn_name) = self.name_of(member) else {
                continue;
            };
            let id = self.add_symbol(&format!("{trait_name}.{fn_name}"), member, None);
            if member.kind() == "function_item" {
                let mut callables = HashMap::new();
                self.walk_calls(member.child_by_field_name("body"), &id, &mut callables);
            }
        }
    }

    fn add_impl_methods(&mut self, impl_node: Node) {
        let Some(type_name) = self.inherent_type_name(impl_node) else {
            return;
        };
        let Some(body) = impl_node.child_by_field_name("body") else {
            return;
        };
        for member in named(body) {
            if member.kind() == "macro_definition" {
                if let Some(name) = self.name_of(member) {
                    self.add_symbol(&name, member, None);
                }
                continue;
            }
            if member.kind() != "function_item" {
                continue;
            }
            let Some(fn_name) = self.name_of(member) else {
                continue;
            };
            let id = self.add_symbol(&format!("{type_name}.{fn_name}"), member, None);
            let mut callables = HashMap::new();
            self.walk_calls(member.child_by_field_name("body"), &id, &mut callables);
        }
    }

    fn add_declared_item(&mut self, child: Node, module_callables: &mut HashMap<String, String>) {
        if child.kind() == "let_declaration" {
            self.bind_let_callable(child, module_callables);
            return;
        }
        if is_file_root_item(child.kind()) {
            let Some(name) = self.name_of(child) else {
                return;
            };
            let id = self.add_symbol(&name, child, None);
            if child.kind() == "function_item" {
                let mut callables = module_callables.clone();
                self.delete_parameter_bindings(child.child_by_field_name("parameters"), &mut callables);
                self.walk_calls(child.child_by_field_name("body"), &id, &mut callables);
            }
            if child.kind() == "trait_item" {
                self.add_trait_methods(child, &name);
            }
            return;
        }
        if child.kind() == "impl_item" {
            self.add_impl_methods(child);
            return;
        }
        if child.kind() == "macro_definition" {
            if let Some(name) = self.name_of(child) {
                self.add_symbol(&name, child, None);
            }
            return;
        }
        if child.kind() == "foreign_mod_item" {
            self.add_foreign_mod_fns(child, module_callables);
        }
    }

    fn add_foreign_mod_fns(&mut self, block: Node, module_callables: &HashMap<String, String>) {
        let block_abi = self.abi_from_extern_modifier(block);
        let body = block
            .child_by_field_name("body")
            .or_else(|| self.named_kind(block, "declaration_list"));
        let Some(body) = body else {
            return;
        };
        for member in named(body) {
            if member.kind() != "function_signature_item" && member.kind() != "function_item" {
                continue;
            }
            let Some(name) = self.name_of(member) else {
                continue;
            };
            let id = self.add_symbol(&name, member, block_abi.clone());
            if member.kind() == "function_item" {
                let mut callables = module_callables.clone();
                self.delete_parameter_bindings(member.child_by_field_name("parameters"), &mut callables);
                self.walk_calls(member.child_by_field_name("body"), &id, &mut callables);
            }
        }
    }

    fn add_inline_mod(&mut self, node: Node, parent_prefix: &str) {
        let Some(body) = node.child_by_field_name("body") else {
            return;
        };
        let Some(name) = self.name_of(node) else {
            return;
        };
        let module_name = if parent_prefix.is_empty() {
            name.clone()
        } else {
            format!("{parent_prefix}.{name}")
        };
        self.add_symbol(&module_name, node, None);
        let mut module_callables = HashMap::new();
        for child in named(body) {
            self.add_declared_item(child, &mut module_callables);
            if child.kind() == "mod_item" {
                self.add_inline_mod(child, &module_name);
            }
        }
    }

    fn bind_let_callable(&self, node: Node, callables: &mut HashMap<String, String>) {
        let Some(pattern) = node.child_by_field_name("pattern") else {
            return;
        };
        if pattern.kind() != "identifier" {
            return;
        }
        let pattern_text = self.text(pattern);
        if pattern_text.is_empty() {
            return;
        }
        let Some(value) = node.child_by_field_name("value") else {
            return;
        };
        let Some(core) = unwrap_parenthesized(Some(value)) else {
            callables.remove(&pattern_text);
            return;
        };
        if core.kind() == "identifier" {
            let core_text = self.text(core);
            if !core_text.is_empty() {
                let mapped = callables.get(&core_text).cloned().unwrap_or(core_text);
                callables.insert(pattern_text, mapped);
                return;
            }
        }
        if core.kind() == "closure_expression" {
            callables.insert(pattern_text, format!("closure:{}", core.start_position().row + 1));
            return;
        }
        callables.remove(&pattern_text);
    }

    fn delete_binding_identifiers(&self, pattern: Option<Node>, callables: &mut HashMap<String, String>) {
        let Some(pattern) = pattern else {
            return;
        };
        if pattern.kind() == "identifier" || pattern.kind() == "shorthand_field_identifier" {
            let text = self.text(pattern);
            if !text.is_empty() {
                callables.remove(&text);
            }
            return;
        }
        if pattern.kind() == "self_parameter" {
            return;
        }
        if pattern.kind() == "tuple_struct_pattern" {
            for child in named(pattern).into_iter().skip(1) {
                self.delete_binding_identifiers(Some(child), callables);
            }
            return;
        }
        if is_nested_pattern(pattern.kind()) {
            for child in named(pattern) {
                self.delete_binding_identifiers(Some(child), callables);
            }
        }
    }

    fn delete_parameter_bindings(&self, params: Option<Node>, callables: &mut HashMap<String, String>) {
        let Some(params) = params else {
            return;
        };
        for child in named(params) {
            if child.kind() == "self_parameter" {
                continue;
            }
            if child.kind() == "parameter" {
                self.delete_binding_identifiers(child.child_by_field_name("pattern"), callables);
                continue;
            }
            if child.kind() == "identifier" {
                let text = self.text(child);
                if !text.is_empty() {
                    callables.remove(&text);
                }
            }
        }
    }

    fn walk_calls(&mut self, node: Option<Node>, enclosing: &str, callables: &mut HashMap<String, String>) {
        let Some(node) = node else {
            return;
        };
        if node.kind() == "let_declaration" {
            self.bind_let_callable(node, callables);
        }
        if self.walk_bound_scope(node, enclosing, callables) {
            return;
        }
        if node.kind() == "call_expression" {
            self.emit_call(node, enclosing, callables);
        }
        if node.kind() == "macro_definition" {
            if let Some(name) = self.name_of(node) {
                self.add_symbol(&name, node, None);
            }
            return;
        }
        if node.kind() == "macro_invocation" {
            self.emit_macro_call(node, enclosing);
            return;
        }
        for child in named(node) {
            self.walk_calls(Some(child), enclosing, callables);
        }
    }

    fn walk_bound_scope(&mut self, node: Node, enclosing: &str, callables: &mut HashMap<String, String>) -> bool {
        match node.kind() {
            "if_expression" => self.walk_if_let(node, enclosing, callables),
            "while_expression" => self.walk_while_let(node, enclosing, callables),
            "match_arm" => {
                self.walk_match_arm(node, enclosing, callables);
                true
            }
            "for_expression" => {
                self.walk_for(node, enclosing, callables);
                true
            }
            "closure_expression" => {
                self.walk_closure(node, enclosing, callables);
                true
            }
            "function_item" => {
                self.walk_nested_fn(node, enclosing, callables);
                true
            }
            _ => false,
        }
    }

    fn walk_if_let(&mut self, node: Node, enclosing: &str, callables: &mut HashMap<String, String>) -> bool {
        let Some(condition) = node.child_by_field_name("condition") else {
            return false;
        };
        if condition.kind() != "let_condition" {
            return false;
        }
        let mut inner = callables.clone();
        self.delete_binding_identifiers(condition.child_by_field_name("pattern"), &mut inner);
        self.walk_calls(condition.child_by_field_name("value"), enclosing, callables);
        self.walk_calls(node.child_by_field_name("consequence"), enclosing, &mut inner);
        self.walk_calls(node.child_by_field_name("alternative"), enclosing, callables);
        true
    }

    fn walk_while_let(&mut self, node: Node, enclosing: &str, callables: &mut HashMap<String, String>) -> bool {
        let Some(condition) = node.child_by_field_name("condition") else {
            return false;
        };
        if condition.kind() != "let_condition" {
            return false;
        }
        let mut inner = callables.clone();
        self.delete_binding_identifiers(condition.child_by_field_name("pattern"), &mut inner);
        self.walk_calls(condition.child_by_field_name("value"), enclosing, callables);
        self.walk_calls(node.child_by_field_name("body"), enclosing, &mut inner);
        true
    }

    fn walk_match_arm(&mut self, node: Node, enclosing: &str, callables: &mut HashMap<String, String>) {
        let mut inner = callables.clone();
        self.delete_binding_identifiers(node.child_by_field_name("pattern"), &mut inner);
        self.walk_calls(node.child_by_field_name("value"), enclosing, &mut inner);
    }

    fn walk_for(&mut self, node: Node, enclosing: &str, callables: &mut HashMap<String, String>) {
        let mut inner = callables.clone();
        self.delete_binding_identifiers(node.child_by_field_name("pattern"), &mut inner);
        self.walk_calls(node.child_by_field_name("value"), enclosing, callables);
        self.walk_calls(node.child_by_field_name("body"), enclosing, &mut inner);
    }

    fn walk_closure(&mut self, node: Node, enclosing: &str, callables: &HashMap<String, String>) {
        let _ = enclosing;
        let name = format!("closure:{}", node.start_position().row + 1);
        let id = self.add_symbol(&name, node, None);
        let mut inner = callables.clone();
        self.delete_parameter_bindings(node.child_by_field_name("parameters"), &mut inner);
        for child in named(node) {
            self.walk_calls(Some(child), &id, &mut inner);
        }
    }

    fn walk_nested_fn(&mut self, node: Node, enclosing: &str, callables: &HashMap<String, String>) {
        let _ = enclosing;
        let Some(name) = self.name_of(node) else {
            return;
        };
        let id = self.add_symbol(&name, node, None);
        let mut inner = callables.clone();
        self.delete_parameter_bindings(node.child_by_field_name("parameters"), &mut inner);
        self.walk_calls(node.child_by_field_name("body"), &id, &mut inner);
    }

    fn emit_call(&mut self, node: Node, enclosing: &str, callables: &HashMap<String, String>) {
        let Some(callee) = unwrap_parenthesized(node.child_by_field_name("function")) else {
            return;
        };
        if callee.kind() == "identifier" {
            let name = self.text(callee);
            if name.is_empty() {
                return;
            }
            let mapped = callables.get(&name).cloned().unwrap_or(name);
            self.push_call(enclosing, &mapped, false);
            return;
        }
        if callee.kind() == "field_expression" {
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
            return;
        }
        if callee.kind() == "scoped_identifier" {
            let Some(name) = callee.child_by_field_name("name") else {
                return;
            };
            if name.kind() != "identifier" {
                return;
            }
            let text = self.text(name);
            if text.is_empty() {
                return;
            }
            self.push_call(enclosing, &text, callee.child_by_field_name("path").is_some());
            return;
        }
        if callee.kind() == "call_expression" {
            self.push_call(enclosing, "__call__", true);
        }
    }

    fn emit_macro_call(&mut self, node: Node, enclosing: &str) {
        let Some(callee) = node.child_by_field_name("macro") else {
            return;
        };
        let callee_name = if callee.kind() == "identifier" {
            let text = self.text(callee);
            if text.is_empty() { None } else { Some(text) }
        } else if callee.kind() == "scoped_identifier" {
            callee.child_by_field_name("name").and_then(|last| {
                if last.kind() != "identifier" {
                    return None;
                }
                let text = self.text(last);
                if text.is_empty() { None } else { Some(text) }
            })
        } else {
            None
        };
        if let Some(callee_name) = callee_name {
            self.push_call(enclosing, &callee_name, false);
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

fn is_simple_use_path(node: Node) -> bool {
    if matches!(node.kind(), "identifier" | "self" | "super" | "crate") {
        return true;
    }
    if node.kind() != "scoped_identifier" {
        return false;
    }
    named(node).into_iter().all(is_simple_use_path)
}

fn is_file_root_item(kind: &str) -> bool {
    matches!(
        kind,
        "function_item"
            | "struct_item"
            | "enum_item"
            | "union_item"
            | "trait_item"
            | "type_item"
            | "const_item"
            | "static_item"
    )
}

fn is_nested_pattern(kind: &str) -> bool {
    matches!(
        kind,
        "tuple_pattern"
            | "slice_pattern"
            | "ref_pattern"
            | "reference_pattern"
            | "struct_pattern"
            | "match_pattern"
            | "mut_pattern"
            | "field_pattern"
            | "or_pattern"
            | "captured_pattern"
    )
}
