use serde_json::{Map, Value};

use crate::ids::{comment_id, file_id, tag_id, unresolved_id};
use crate::types::{ExtractedGraph, GraphEdge, GraphNode, ScannedFile};

#[derive(Clone, Copy)]
enum Syntax {
    Hash,
    Slash,
    Both,
}

pub fn extract_comments(file: &ScannedFile) -> ExtractedGraph {
    let Some(syntax) = syntax_for(&file.lang) else {
        return ExtractedGraph::default();
    };
    let nodes = &mut Vec::new();
    let edges = &mut Vec::new();
    let lines = split_lines(&file.text);
    let mut index = 0;
    let mut in_block = false;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();
        let already_in_block = in_block;
        if unquoted_block_open(line, syntax) >= 0 {
            in_block = true;
        }
        if !in_block {
            if let Some(rest) = match_onic_line(line, syntax) {
                let taken = read_line_comment(index as i64 + 1, rest, &lines, index + 1, syntax);
                push_comment(file, &taken, nodes, edges);
                index = taken.next_index;
                continue;
            }
        }
        let star = trimmed.strip_prefix('*').and_then(|rest| onic_rest(rest));
        let bare = if star.is_none() && in_block { onic_rest(trimmed) } else { None };
        if let Some(fields) = star.or(bare) {
            if in_block && has_block_closer(&lines, index) {
                let taken = read_block_comment(index as i64 + 1, &fields, &lines, index + 1);
                push_comment(file, &taken, nodes, edges);
                index = taken.next_index;
                in_block = false;
                continue;
            }
        }
        let block_at = unquoted_block_open(line, syntax);
        if block_at >= 0 && !already_in_block {
            if let Some(fields) = onic_block_open(&line[block_at as usize..]) {
                let taken = if let Some(end) = fields.find("*/") {
                    Taken {
                        fields: fields[..end].to_string(),
                        body: String::new(),
                        next_index: index + 1,
                        start_line: index as i64 + 1,
                        end_line: index as i64 + 1,
                    }
                } else {
                    read_block_comment(index as i64 + 1, &fields, &lines, index + 1)
                };
                push_comment(file, &taken, nodes, edges);
                index = taken.next_index;
                in_block = false;
                continue;
            }
        }
        if trimmed.contains("*/") {
            in_block = false;
        }
        index += 1;
    }
    ExtractedGraph { nodes: std::mem::take(nodes), edges: std::mem::take(edges) }
}

struct Taken {
    fields: String,
    body: String,
    next_index: usize,
    start_line: i64,
    end_line: i64,
}

fn syntax_for(lang: &str) -> Option<Syntax> {
    match lang {
        "md" => None,
        "py" | "python" => Some(Syntax::Hash),
        "" => Some(Syntax::Both),
        _ => Some(Syntax::Slash),
    }
}

fn allows_hash(syntax: Syntax) -> bool {
    matches!(syntax, Syntax::Hash | Syntax::Both)
}

fn allows_slash(syntax: Syntax) -> bool {
    matches!(syntax, Syntax::Slash | Syntax::Both)
}

fn has_block_closer(lines: &[&str], from: usize) -> bool {
    lines[from..].iter().any(|line| line.contains("*/"))
}

fn match_onic_line(line: &str, syntax: Syntax) -> Option<String> {
    if allows_slash(syntax) {
        let slash = unquoted_index(line, "//");
        if slash >= 0 {
            if let Some(rest) = slash_onic(&line[slash as usize..]) {
                return Some(rest);
            }
        }
    }
    if allows_hash(syntax) {
        let hash = unquoted_index(line, "#");
        if hash >= 0 {
            if let Some(rest) = hash_onic(&line[hash as usize..]) {
                return Some(rest);
            }
        }
    }
    None
}

fn slash_onic(text: &str) -> Option<String> {
    let rest = text.strip_prefix("//")?;
    onic_rest(rest)
}

fn hash_onic(text: &str) -> Option<String> {
    let rest = text.strip_prefix('#')?;
    onic_rest(rest)
}

fn onic_rest(text: &str) -> Option<String> {
    let rest = text.trim_start();
    let rest = rest.strip_prefix("@onic")?;
    if !word_boundary(rest) {
        return None;
    }
    Some(rest.to_string())
}

fn word_boundary(text: &str) -> bool {
    match text.chars().next() {
        None => true,
        Some(ch) => !ch.is_ascii_alphanumeric() && ch != '_',
    }
}

fn onic_block_open(text: &str) -> Option<String> {
    let rest = text.trim_start_matches('/');
    if text.starts_with("/*") {
        return onic_rest(rest.trim_start_matches('*'));
    }
    None
}

fn unquoted_index(line: &str, needle: &str) -> i64 {
    let chars: Vec<char> = line.chars().collect();
    let mut index = 0;
    let mut quote = None;
    while index < chars.len() {
        let ch = chars[index];
        if let Some(open) = quote {
            if ch == '\\' {
                index += 2;
                continue;
            }
            if ch == open {
                quote = None;
            }
            index += 1;
            continue;
        }
        if ch == '"' || ch == '\'' || ch == '`' {
            quote = Some(ch);
            index += 1;
            continue;
        }
        if line[char_byte(line, index)..].starts_with(needle) {
            return char_byte(line, index) as i64;
        }
        index += 1;
    }
    -1
}

fn char_byte(text: &str, char_index: usize) -> usize {
    text.chars().take(char_index).map(|ch| ch.len_utf8()).sum()
}

fn unquoted_block_open(line: &str, syntax: Syntax) -> i64 {
    let slash = if allows_slash(syntax) { unquoted_index(line, "//") } else { -1 };
    let hash = if allows_hash(syntax) { unquoted_index(line, "#") } else { -1 };
    let block = unquoted_index(line, "/*");
    if block < 0 {
        return -1;
    }
    let line_comment = if slash >= 0 && hash >= 0 { slash.min(hash) } else if slash >= 0 { slash } else { hash };
    if line_comment >= 0 && line_comment < block { -1 } else { block }
}

fn read_line_comment(start_line: i64, first_fields: String, lines: &[&str], from: usize, syntax: Syntax) -> Taken {
    let mut body = Vec::new();
    let mut index = from;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        let Some(text) = line_comment_body(trimmed, syntax) else { break };
        if match_onic_line(lines[index], syntax).is_some() {
            break;
        }
        body.push(text.trim().to_string());
        index += 1;
    }
    Taken {
        fields: first_fields,
        body: body.join("\n").trim().to_string(),
        next_index: index,
        start_line,
        end_line: start_line.max(index as i64),
    }
}

fn line_comment_body(trimmed: &str, syntax: Syntax) -> Option<&str> {
    match syntax {
        Syntax::Hash => trimmed.strip_prefix('#'),
        Syntax::Slash => trimmed.strip_prefix("//"),
        Syntax::Both => trimmed.strip_prefix("//").or_else(|| trimmed.strip_prefix('#')),
    }
}

fn read_block_comment(start_line: i64, first_fields: &str, lines: &[&str], from: usize) -> Taken {
    let mut body = Vec::new();
    let mut index = from;
    let mut end_line = start_line;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if let Some(end) = trimmed.find("*/") {
            let before = trimmed[..end].trim_start_matches('*').trim();
            if !before.is_empty() {
                body.push(before.to_string());
            }
            end_line = index as i64 + 1;
            index += 1;
            break;
        }
        let text = star_line_body(trimmed).trim();
        if !text.is_empty() {
            body.push(text.to_string());
        }
        end_line = index as i64 + 1;
        index += 1;
    }
    Taken {
        fields: first_fields.to_string(),
        body: body.join("\n").trim().to_string(),
        next_index: index,
        start_line,
        end_line,
    }
}

fn star_line_body(trimmed: &str) -> &str {
    if let Some(rest) = trimmed.strip_prefix('*') {
        if rest.is_empty() || rest.starts_with(' ') {
            return rest;
        }
    }
    trimmed
}

fn push_comment(file: &ScannedFile, taken: &Taken, nodes: &mut Vec<GraphNode>, edges: &mut Vec<GraphEdge>) {
    let fields = parse_fields(&taken.fields);
    let kind = fields.get("kind").filter(|kind| !kind.is_empty()).cloned().unwrap_or_else(|| "note".to_string());
    let id = comment_id(&file.path, taken.start_line);
    let mut props = Map::new();
    for (key, value) in &fields {
        props.insert(key.clone(), Value::String(value.clone()));
    }
    props.insert("commentKind".to_string(), Value::String(kind.clone()));
    nodes.push(GraphNode {
        id: id.clone(),
        kind: "comment".to_string(),
        name: format!("{kind}@{}:{}", file.path, taken.start_line),
        file_path: Some(file.path.clone()),
        start_line: Some(taken.start_line),
        end_line: Some(taken.end_line),
        body: Some(taken.body.clone()),
        props,
    });
    edges.push(GraphEdge {
        src: file_id(&file.path),
        dst: id.clone(),
        kind: "contains".to_string(),
        confidence: "extracted".to_string(),
        props: Map::new(),
    });
    for related in split_list(fields.get("relates").map(String::as_str)) {
        edges.push(GraphEdge {
            src: id.clone(),
            dst: unresolved_id(&related),
            kind: "relates".to_string(),
            confidence: "extracted".to_string(),
            props: Map::new(),
        });
    }
    for tag in split_list(fields.get("tags").map(String::as_str)) {
        let tid = tag_id(&tag);
        nodes.push(GraphNode {
            id: tid.clone(),
            kind: "tag".to_string(),
            name: tag,
            file_path: None,
            start_line: None,
            end_line: None,
            body: None,
            props: Map::new(),
        });
        edges.push(GraphEdge {
            src: id.clone(),
            dst: tid,
            kind: "tagged".to_string(),
            confidence: "extracted".to_string(),
            props: Map::new(),
        });
    }
}

fn parse_fields(raw: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return out;
    }
    for part in trimmed.split_whitespace() {
        let Some(eq) = part.find('=') else { continue };
        if eq == 0 {
            continue;
        }
        out.insert(part[..eq].to_string(), part[eq + 1..].to_string());
    }
    out
}

fn split_list(value: Option<&str>) -> Vec<String> {
    let Some(value) = value.filter(|value| !value.is_empty()) else { return Vec::new() };
    value.split(',').map(str::trim).filter(|item| !item.is_empty()).map(str::to_string).collect()
}

fn split_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\n' {
            let end = if index > start && bytes[index - 1] == b'\r' { index - 1 } else { index };
            lines.push(&text[start..end]);
            index += 1;
            start = index;
        } else {
            index += 1;
        }
    }
    lines.push(&text[start..]);
    lines
}
