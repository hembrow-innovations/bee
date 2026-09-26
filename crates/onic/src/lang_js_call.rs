use std::collections::HashSet;

use serde_json::{Map, Value};
use tree_sitter::Node;

use crate::ids::{file_id, unresolved_id};
use crate::types::GraphEdge;

pub(crate) fn node_text<'a>(node: Node, source: &'a str) -> &'a str {
    source.get(node.start_byte()..node.end_byte()).unwrap_or("")
}

pub(crate) fn kids(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn field<'a>(node: Node<'a>, name: &str) -> Option<Node<'a>> {
    node.child_by_field_name(name)
}

fn named_at(node: Node, index: usize) -> Option<Node> {
    node.named_child(index)
}

fn named_last(node: Node) -> Option<Node> {
    let count = node.named_child_count();
    if count == 0 { None } else { node.named_child(count - 1) }
}

fn empty_props() -> Map<String, Value> {
    Map::new()
}

fn bool_prop(key: &str) -> Map<String, Value> {
    let mut props = Map::new();
    props.insert(key.to_string(), Value::Bool(true));
    props
}

fn member_jsx_props() -> Map<String, Value> {
    let mut props = bool_prop("member");
    props.insert("jsx".to_string(), Value::Bool(true));
    props
}

fn push_import(edges: &mut Vec<GraphEdge>, path: &str, spec: &str) {
    let mut props = Map::new();
    props.insert("spec".to_string(), Value::String(spec.to_string()));
    edges.push(GraphEdge {
        src: file_id(path),
        dst: unresolved_id(spec),
        kind: "imports".to_string(),
        confidence: "extracted".to_string(),
        props,
    });
}

fn push_call(edges: &mut Vec<GraphEdge>, src: &str, name: &str, props: Map<String, Value>) {
    edges.push(GraphEdge {
        src: src.to_string(),
        dst: unresolved_id(name),
        kind: "calls".to_string(),
        confidence: "extracted".to_string(),
        props,
    });
}

pub(crate) fn unquote(text: &str) -> &str {
    let bytes = text.as_bytes();
    if bytes.len() >= 2 {
        let open = bytes[0];
        let close = bytes[bytes.len() - 1];
        if (open == b'"' && close == b'"') || (open == b'\'' && close == b'\'') || (open == b'`' && close == b'`') {
            return &text[1..text.len() - 1];
        }
    }
    text
}

pub(crate) fn unwrap_callee(node: Node) -> Node {
    let mut current = node;
    loop {
        let kind = current.kind();
        if !matches!(
            kind,
            "non_null_expression"
                | "parenthesized_expression"
                | "as_expression"
                | "type_assertion"
                | "satisfies_expression"
                | "instantiation_expression"
                | "sequence_expression"
        ) {
            return current;
        }
        if kind == "sequence_expression" {
            let Some(last) = named_last(current) else { return current };
            current = last;
            continue;
        }
        let Some(inner) = unwrap_inner(current) else { return current };
        current = inner;
    }
}

fn unwrap_inner(node: Node) -> Option<Node> {
    if let Some(field) = field(node, "expression") {
        return Some(field);
    }
    let mut cursor = node.walk();
    let found = node.named_children(&mut cursor).find(|child| child.kind() != "type_arguments");
    found
}

pub(crate) fn is_function_value(node: Node) -> bool {
    matches!(
        unwrap_callee(node).kind(),
        "arrow_function" | "function" | "function_expression" | "generator_function" | "generator_function_expression"
    )
}

pub(crate) fn is_using_assignment(node: Node, source: &str) -> bool {
    let text = node_text(node, source).trim_start();
    let Some(rest) = text.strip_prefix("using") else { return false };
    let mut chars = rest.chars();
    let Some(first) = chars.next() else { return false };
    if !first.is_whitespace() {
        return false;
    }
    let Some(next) = chars.find(|ch| !ch.is_whitespace()) else { return false };
    next != '='
}

pub(crate) fn take_require_or_import(
    path: &str,
    node: Node,
    source: &str,
    edges: &mut Vec<GraphEdge>,
    local_require: bool,
) -> bool {
    let Some(callee) = field(node, "function").or_else(|| named_at(node, 0)) else { return false };
    let core = unwrap_callee(callee);
    if core.kind() != "import" && !is_require(core, source, local_require) {
        return false;
    }
    let Some(spec) = first_string_arg(node, source) else { return false };
    push_import(edges, path, &spec);
    true
}

fn first_string_arg(node: Node, source: &str) -> Option<String> {
    let args = field(node, "arguments")?;
    if args.kind() != "arguments" {
        return None;
    }
    let first = named_at(args, 0)?;
    string_spec(unwrap_callee(first), source)
}

fn string_spec(node: Node, source: &str) -> Option<String> {
    if matches!(node.kind(), "as_expression" | "type_assertion" | "satisfies_expression") {
        let inner = unwrap_inner(node)?;
        return string_spec(unwrap_callee(inner), source);
    }
    if matches!(node.kind(), "string" | "string_fragment" | "template_string") {
        if node.kind() == "template_string" && kids(node).iter().any(|child| child.kind() == "template_substitution") {
            return None;
        }
        let spec = unquote(node_text(node, source));
        if spec.is_empty() { None } else { Some(spec.to_string()) }
    } else {
        None
    }
}

pub(crate) fn take_call(
    enclosing: &str,
    node: Node,
    source: &str,
    edges: &mut Vec<GraphEdge>,
    local_require: bool,
    instances: Option<&HashSet<String>>,
) {
    let Some(callee) = field(node, "function")
        .or_else(|| field(node, "constructor"))
        .or_else(|| named_at(node, 0))
    else {
        return;
    };
    let core = unwrap_callee(callee);
    if core.kind() == "subscript_expression" {
        let name = field(core, "index").and_then(|index| string_spec(index, source));
        if let Some(name) = name {
            push_call(edges, enclosing, &name, bool_prop("member"));
        }
        return;
    }
    if node.kind() == "call_expression" && matches!(core.kind(), "call_expression" | "new_expression") {
        if !skips_nested_call(core, source, local_require) {
            push_call(edges, enclosing, "__call__", bool_prop("member"));
        }
        return;
    }
    if node.kind() == "call_expression"
        && core.kind() == "identifier"
        && instances.is_some_and(|names| names.contains(node_text(core, source)))
        && !is_tagged_template_call(node)
        && !is_optional_call(node, callee, source)
    {
        push_call(edges, enclosing, "__call__", bool_prop("member"));
        return;
    }
    let Some(name) = callee_name(core, source) else { return };
    let props = if core.kind() == "member_expression" { bool_prop("member") } else { empty_props() };
    push_call(edges, enclosing, &name, props);
}

fn is_tagged_template_call(node: Node) -> bool {
    field(node, "arguments").is_some_and(|args| args.kind() == "template_string")
}

fn is_optional_call(node: Node, callee: Node, source: &str) -> bool {
    source
        .get(callee.end_byte()..node.end_byte())
        .unwrap_or("")
        .trim_start()
        .starts_with("?.")
}

fn skips_nested_call(inner: Node, source: &str, local_require: bool) -> bool {
    if inner.kind() != "call_expression" {
        return false;
    }
    let Some(callee) = field(inner, "function").or_else(|| named_at(inner, 0)) else { return false };
    let core = unwrap_callee(callee);
    core.kind() == "import" || is_require(core, source, local_require)
}

fn is_require(core: Node, source: &str, local_require: bool) -> bool {
    core.kind() == "identifier" && node_text(core, source) == "require" && !local_require
}

fn callee_name(node: Node, source: &str) -> Option<String> {
    if matches!(node.kind(), "non_null_expression" | "parenthesized_expression") {
        return named_at(node, 0).and_then(|inner| callee_name(inner, source));
    }
    if node.kind() == "identifier" {
        return Some(node_text(node, source).to_string());
    }
    if node.kind() == "member_expression" {
        return field(node, "property").map(|prop| node_text(prop, source).to_string());
    }
    None
}

pub(crate) fn take_jsx_callee(enclosing: &str, node: Node, source: &str, edges: &mut Vec<GraphEdge>) {
    let Some(name_node) = field(node, "name") else { return };
    let core = unwrap_callee(name_node);
    if core.kind() == "identifier" {
        let name = node_text(core, source);
        if !jsx_identifier_callable(name) {
            return;
        }
        push_call(edges, enclosing, name, bool_prop("jsx"));
        return;
    }
    if matches!(core.kind(), "member_expression" | "nested_identifier") {
        let Some(prop) = jsx_member_property(core, source) else { return };
        if !jsx_identifier_callable(&prop) {
            return;
        }
        push_call(edges, enclosing, &prop, member_jsx_props());
    }
}

fn jsx_identifier_callable(name: &str) -> bool {
    if name == "Fragment" || name.is_empty() {
        return false;
    }
    matches!(name.chars().next(), Some('$' | '_') | Some('A'..='Z'))
}

fn jsx_member_property(node: Node, source: &str) -> Option<String> {
    if node.kind() == "member_expression" {
        return field(node, "property").map(|prop| node_text(prop, source).to_string());
    }
    named_last(node).map(|last| node_text(last, source).to_string())
}

pub(crate) fn has_local_require(root: Node, source: &str) -> bool {
    fn walk(node: Node, source: &str) -> bool {
        if decl_named_require(node, source) {
            return true;
        }
        if matches!(node.kind(), "internal_module" | "module") && name_text(node, source).as_deref() == Some("require") {
            return true;
        }
        if node.kind() == "import_statement" {
            if is_import_type(node_text(node, source)) {
                return false;
            }
            return kids(node).into_iter().any(|child| walk(child, source));
        }
        if matches!(node.kind(), "lexical_declaration" | "variable_declaration") {
            for child in kids(node) {
                if child.kind() == "variable_declarator" && binds_name(field(child, "name"), "require", source) {
                    return true;
                }
            }
        }
        if matches!(node.kind(), "type_annotation" | "function_type" | "call_signature" | "type_arguments") {
            return false;
        }
        if node.kind() == "arrow_function" {
            let param = field(node, "parameter").or_else(|| named_at(node, 0));
            if param.is_some_and(|param| param.kind() == "identifier" && node_text(param, source) == "require") {
                return true;
            }
        }
        if node.kind() == "catch_clause" {
            let param = field(node, "parameter").or_else(|| named_at(node, 0));
            if binds_name(param, "require", source) {
                return true;
            }
        }
        if matches!(node.kind(), "for_in_statement" | "for_of_statement") && for_binds_require(node, source) {
            return true;
        }
        if import_binds_require(node, source) || param_binds_require(node, source) {
            return true;
        }
        kids(node).into_iter().any(|child| walk(child, source))
    }
    walk(root, source)
}

fn decl_named_require(node: Node, source: &str) -> bool {
    matches!(
        node.kind(),
        "function_declaration"
            | "function_signature"
            | "generator_function_declaration"
            | "class_declaration"
            | "abstract_class_declaration"
            | "enum_declaration"
    ) && name_text(node, source).as_deref() == Some("require")
}

fn name_text(node: Node, source: &str) -> Option<String> {
    let named = field(node, "name")?;
    if named.kind() == "computed_property_name" {
        return None;
    }
    if matches!(named.kind(), "string" | "string_fragment") {
        let text = unquote(node_text(named, source));
        return if text.is_empty() { None } else { Some(text.to_string()) };
    }
    let text = node_text(named, source);
    if text.is_empty() { None } else { Some(text.to_string()) }
}

fn binds_name(node: Option<Node>, name: &str, source: &str) -> bool {
    let Some(node) = node else { return false };
    if matches!(node.kind(), "identifier" | "shorthand_property_identifier" | "shorthand_property_identifier_pattern")
        && node_text(node, source) == name
    {
        return true;
    }
    if node.kind() == "assignment_pattern" {
        return binds_name(field(node, "left").or_else(|| named_at(node, 0)), name, source);
    }
    if node.kind() == "object_assignment_pattern" {
        return binds_name(
            field(node, "left").or_else(|| field(node, "name")).or_else(|| named_at(node, 0)),
            name,
            source,
        );
    }
    if node.kind() == "pair_pattern" {
        return binds_name(field(node, "value").or_else(|| named_last(node)), name, source);
    }
    if matches!(node.kind(), "object_pattern" | "array_pattern") {
        return kids(node).into_iter().any(|child| binds_name(Some(child), name, source));
    }
    false
}

fn import_binds_require(node: Node, source: &str) -> bool {
    if node.kind() == "import_specifier" {
        if specifier_is_type(node_text(node, source)) {
            return false;
        }
        let alias = field(node, "alias");
        let name = field(node, "name");
        return alias.is_some_and(|alias| node_text(alias, source) == "require")
            || (alias.is_none() && name.is_some_and(|name| node_text(name, source) == "require"));
    }
    if node.kind() == "namespace_import" {
        let name = field(node, "name").or_else(|| kids(node).into_iter().find(|child| child.kind() == "identifier"));
        return name.is_some_and(|name| node_text(name, source) == "require");
    }
    if node.kind() == "import_clause" {
        return kids(node)
            .into_iter()
            .any(|child| child.kind() == "identifier" && node_text(child, source) == "require");
    }
    false
}

fn for_binds_require(node: Node, source: &str) -> bool {
    let Some(left) = field(node, "left").or_else(|| named_at(node, 0)) else { return false };
    if left.kind() == "identifier" {
        return node_text(left, source) == "require";
    }
    if matches!(left.kind(), "lexical_declaration" | "variable_declaration") {
        for child in kids(left) {
            if child.kind() == "variable_declarator" && binds_name(field(child, "name"), "require", source) {
                return true;
            }
        }
    }
    binds_name(Some(left), "require", source)
}

fn param_binds_require(node: Node, source: &str) -> bool {
    if !matches!(node.kind(), "required_parameter" | "optional_parameter" | "rest_parameter") {
        return false;
    }
    binds_name(
        field(node, "pattern").or_else(|| field(node, "name")).or_else(|| named_at(node, 0)),
        "require",
        source,
    )
}

pub(crate) fn function_instances(instances: Option<&HashSet<String>>) -> HashSet<String> {
    instances.cloned().unwrap_or_default()
}

pub(crate) fn delete_function_bindings(node: Node, source: &str, instances: &mut HashSet<String>) {
    delete_bound_names(field(node, "parameters"), source, instances);
    delete_bound_names(field(node, "parameter"), source, instances);
}

pub(crate) fn delete_bound_names(node: Option<Node>, source: &str, instances: &mut HashSet<String>) {
    let Some(node) = node else { return };
    if matches!(node.kind(), "identifier" | "shorthand_property_identifier" | "shorthand_property_identifier_pattern") {
        instances.remove(node_text(node, source));
        return;
    }
    if node.kind() == "assignment_pattern" {
        delete_bound_names(field(node, "left").or_else(|| named_at(node, 0)), source, instances);
        return;
    }
    if node.kind() == "object_assignment_pattern" {
        delete_bound_names(
            field(node, "left").or_else(|| field(node, "name")).or_else(|| named_at(node, 0)),
            source,
            instances,
        );
        return;
    }
    if node.kind() == "pair_pattern" {
        delete_bound_names(field(node, "value").or_else(|| named_last(node)), source, instances);
        return;
    }
    if node.kind() == "rest_pattern" {
        delete_bound_names(named_at(node, 0), source, instances);
        return;
    }
    if matches!(node.kind(), "required_parameter" | "optional_parameter" | "rest_parameter") {
        delete_bound_names(
            field(node, "pattern").or_else(|| field(node, "name")).or_else(|| named_at(node, 0)),
            source,
            instances,
        );
        return;
    }
    if matches!(
        node.kind(),
        "object_pattern" | "array_pattern" | "formal_parameters" | "lexical_declaration" | "variable_declaration"
    ) {
        for child in kids(node) {
            delete_bound_names(Some(child), source, instances);
        }
        return;
    }
    if node.kind() == "variable_declarator" {
        delete_bound_names(field(node, "name"), source, instances);
    }
}

pub(crate) fn take_declarative_binding(
    left: Node,
    value: Option<Node>,
    source: &str,
    instances: &mut HashSet<String>,
) {
    if left.kind() != "identifier" {
        delete_bound_names(Some(left), source, instances);
        return;
    }
    apply_instance_binding(node_text(left, source), value, source, instances);
}

pub(crate) fn apply_instance_binding(name: &str, value: Option<Node>, source: &str, instances: &mut HashSet<String>) {
    let Some(value) = value else {
        instances.remove(name);
        return;
    };
    let core = unwrap_callee(value);
    if core.kind() == "new_expression" {
        let Some(raw) = field(core, "constructor").or_else(|| named_at(core, 0)) else {
            instances.remove(name);
            return;
        };
        let ctor = unwrap_callee(raw);
        if matches!(ctor.kind(), "identifier" | "member_expression") {
            instances.insert(name.to_string());
        } else {
            instances.remove(name);
        }
        return;
    }
    if core.kind() == "identifier" {
        if instances.contains(node_text(core, source)) {
            instances.insert(name.to_string());
        } else {
            instances.remove(name);
        }
        return;
    }
    instances.remove(name);
}

pub(crate) fn is_import_type(text: &str) -> bool {
    let rest = text.trim_start();
    let Some(rest) = rest.strip_prefix("import") else { return false };
    let rest = rest.trim_start();
    word_after(rest, "type")
}

pub(crate) fn specifier_is_type(text: &str) -> bool {
    word_after(text.trim_start(), "type")
}

fn word_after(text: &str, word: &str) -> bool {
    let Some(rest) = text.strip_prefix(word) else { return false };
    match rest.chars().next() {
        None => true,
        Some(ch) => !ch.is_ascii_alphanumeric() && ch != '_',
    }
}
