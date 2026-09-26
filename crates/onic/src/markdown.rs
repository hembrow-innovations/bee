use std::collections::HashSet;
use std::sync::OnceLock;

use regex::{Captures, Regex};
use serde_json::{Map, Value};

use crate::ids::{as_rel_path, doc_id, file_id, heading_id, slugify, tag_id, unresolved_id};
use crate::types::{ExtractedGraph, GraphEdge, GraphNode, ScannedFile};

pub fn extract_markdown(file: &ScannedFile) -> ExtractedGraph {
    let (frontmatter, body, body_start) = split_frontmatter(&file.text);
    let title = string_field(frontmatter.get("title")).unwrap_or_else(|| stem(&file.path));
    let doc = doc_id(&file.path);
    let mut nodes = vec![GraphNode {
        id: doc.clone(),
        kind: "doc".to_string(),
        name: title,
        file_path: Some(file.path.clone()),
        start_line: Some(1),
        end_line: Some(split_lines(&file.text).len() as i64),
        body: Some(body.trim().to_string()),
        props: frontmatter.clone(),
    }];
    let mut edges = vec![GraphEdge {
        src: file_id(&file.path),
        dst: doc.clone(),
        kind: "contains".to_string(),
        confidence: "extracted".to_string(),
        props: Map::new(),
    }];
    for tag in string_list(frontmatter.get("tags")) {
        let id = tag_id(&tag);
        nodes.push(GraphNode {
            id: id.clone(),
            kind: "tag".to_string(),
            name: tag,
            file_path: None,
            start_line: None,
            end_line: None,
            body: None,
            props: Map::new(),
        });
        edges.push(GraphEdge {
            src: doc.clone(),
            dst: id,
            kind: "tagged".to_string(),
            confidence: "extracted".to_string(),
            props: Map::new(),
        });
    }
    for related in string_list(frontmatter.get("relates")) {
        edges.push(GraphEdge {
            src: doc.clone(),
            dst: unresolved_id(&related),
            kind: "relates".to_string(),
            confidence: "extracted".to_string(),
            props: Map::new(),
        });
    }
    let markup = visible_markup(&body);
    let lines = split_lines(&markup);
    let mut fence = None;
    for (index, line) in lines.iter().enumerate() {
        if let Some(open) = fence {
            if fence_close(line, open) {
                fence = None;
            }
            continue;
        }
        if let Some(opener) = fence_open(line) {
            fence = Some(opener);
            continue;
        }
        let Some(caps) = heading_re().captures(line) else { continue };
        let text = strip_atx_closing(caps.get(3).map(|m| m.as_str()).unwrap_or(""));
        let slug = slugify(&text);
        let id = heading_id(&file.path, &slug);
        let depth = caps.get(2).map(|m| m.as_str().len() as i64).unwrap_or(1);
        let mut props = Map::new();
        props.insert("depth".to_string(), Value::from(depth));
        props.insert("slug".to_string(), Value::String(slug));
        let line_no = body_start + index as i64;
        nodes.push(GraphNode {
            id: id.clone(),
            kind: "heading".to_string(),
            name: text.clone(),
            file_path: Some(file.path.clone()),
            start_line: Some(line_no),
            end_line: Some(line_no),
            body: Some(text),
            props,
        });
        edges.push(GraphEdge {
            src: doc.clone(),
            dst: id,
            kind: "contains".to_string(),
            confidence: "extracted".to_string(),
            props: Map::new(),
        });
    }
    let link_text = link_source(&body);
    for caps in wikilink_re().captures_iter(&link_text) {
        let at = caps.get(0).map(|m| m.start()).unwrap_or(0);
        if escaped_at(&link_text, at) || wikilink_is_embed(&link_text, at) {
            continue;
        }
        let name = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let fragment = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
        let href = if fragment.is_empty() { name.to_string() } else { format!("{name}#{fragment}") };
        edges.push(link_edge(&doc, &href, "wikilink"));
    }
    for caps in mdlink_re().captures_iter(&link_text) {
        let raw = caps.get(0).map(|m| m.as_str()).unwrap_or("");
        let at = caps.get(0).map(|m| m.start()).unwrap_or(0);
        if raw.starts_with('!') && !escaped_at(&link_text, at) {
            continue;
        }
        if !raw.starts_with('!') && escaped_at(&link_text, at) {
            continue;
        }
        let href = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
        if is_external_href(href) || strip_dest_title(href).is_empty() {
            continue;
        }
        edges.push(link_edge(&doc, href, "markdown"));
    }
    ExtractedGraph { nodes, edges }
}

pub fn resolve_markdown_href(href: &str, from: &str, known: &HashSet<String>) -> Option<String> {
    let dest = strip_dest_title(href);
    let cleaned = dest.split('#').next().unwrap_or("").split('?').next().unwrap_or("");
    if cleaned.is_empty() {
        return dest.contains('#').then(|| doc_id(from));
    }
    if let Some(id) = resolve_known_markdown_path(from, cleaned, known) {
        return Some(id);
    }
    if looks_like_markdown_path(cleaned) {
        return Some(unresolved_id(cleaned));
    }
    let base = cleaned.rsplit('/').next().unwrap_or(cleaned);
    let name = base.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(base);
    if name.is_empty() { None } else { Some(unresolved_id(name)) }
}

pub fn retarget_heading_links(graph: ExtractedGraph) -> ExtractedGraph {
    let heading_ids: HashSet<String> = graph.nodes.iter().filter(|node| node.kind == "heading").map(|node| node.id.clone()).collect();
    let edges = graph
        .edges
        .into_iter()
        .map(|mut edge| {
            if edge.kind != "links" {
                return edge;
            }
            let Some(href) = edge.props.get("href").and_then(Value::as_str).map(str::to_string) else { return edge };
            let Some(hash) = href.find('#') else { return edge };
            let fragment = href[hash + 1..].split('?').next().unwrap_or("");
            if fragment.is_empty() || !edge.dst.starts_with("doc:") {
                return edge;
            }
            let Ok(path) = as_rel_path(&edge.dst["doc:".len()..]) else { return edge };
            let target = heading_id(&path, &slugify(&decode_fragment(fragment)));
            if !heading_ids.contains(&target) {
                return edge;
            }
            edge.dst = target;
            edge.confidence = "resolved".to_string();
            edge
        })
        .collect();
    ExtractedGraph { nodes: graph.nodes, edges }
}

fn link_edge(doc: &str, href: &str, style: &str) -> GraphEdge {
    let mut props = Map::new();
    props.insert("style".to_string(), Value::String(style.to_string()));
    props.insert("href".to_string(), Value::String(href.to_string()));
    GraphEdge {
        src: doc.to_string(),
        dst: unresolved_id(href),
        kind: "links".to_string(),
        confidence: "extracted".to_string(),
        props,
    }
}

fn decode_fragment(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return raw.to_string();
            }
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            let Ok(byte) = u8::from_str_radix(hex, 16) else { return raw.to_string() };
            out.push(byte);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| raw.to_string())
}

fn split_frontmatter(text: &str) -> (Map<String, Value>, String, i64) {
    let source = text.strip_prefix('\u{feff}').unwrap_or(text);
    if !source.starts_with("---") {
        return (Map::new(), source.to_string(), 1);
    }
    let lines = split_lines(source);
    if lines.first().is_none_or(|line| line.trim() != "---") {
        return (Map::new(), source.to_string(), 1);
    }
    let Some(close) = lines.iter().enumerate().find(|(index, line)| *index > 0 && line.trim() == "---").map(|(index, _)| index) else {
        return (Map::new(), source.to_string(), 1);
    };
    let yaml = lines[1..close].join("\n");
    (parse_simple_yaml(&yaml), lines[close + 1..].join("\n"), close as i64 + 2)
}

fn parse_simple_yaml(yaml: &str) -> Map<String, Value> {
    let mut out = Map::new();
    for raw_line in split_lines(yaml) {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(colon) = line.find(':') else { continue };
        if colon == 0 {
            continue;
        }
        let key = line[..colon].trim();
        let value = line[colon + 1..].trim();
        out.insert(key.to_string(), parse_yaml_value(value));
    }
    out
}

fn parse_yaml_value(value: &str) -> Value {
    if value.is_empty() {
        return Value::String(String::new());
    }
    if value == "true" {
        return Value::Bool(true);
    }
    if value == "false" {
        return Value::Bool(false);
    }
    if value.starts_with('[') && value.ends_with(']') {
        let items = value[1..value.len() - 1]
            .split(',')
            .map(|item| unquote(item.trim()).to_string())
            .filter(|item| !item.is_empty())
            .map(Value::String)
            .collect();
        return Value::Array(items);
    }
    Value::String(unquote(value).to_string())
}

fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let open = bytes[0];
        let close = bytes[bytes.len() - 1];
        if (open == b'"' && close == b'"') || (open == b'\'' && close == b'\'') {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn string_field(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).filter(|text| !text.is_empty()).map(str::to_string)
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items.iter().filter_map(|item| item.as_str()).filter(|item| !item.is_empty()).map(str::to_string).collect(),
        Some(Value::String(text)) if !text.is_empty() => text.split(',').map(str::trim).filter(|item| !item.is_empty()).map(str::to_string).collect(),
        _ => Vec::new(),
    }
}

fn stem(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    base.rsplit_once('.').map(|(name, _)| name).unwrap_or(base).to_string()
}

fn looks_like_markdown_path(name: &str) -> bool {
    name.contains('/') || name.starts_with('.') || ext_re().is_match(name)
}

fn link_source(body: &str) -> String {
    let markup = visible_markup(body);
    let lines = split_lines(&markup);
    let mut fence = None;
    let mut kept = Vec::new();
    for line in lines {
        if let Some(open) = fence {
            if fence_close(line, open) {
                fence = None;
            }
            kept.push(String::new());
            continue;
        }
        if let Some(opener) = fence_open(line) {
            fence = Some(opener);
            kept.push(String::new());
            continue;
        }
        if indent_re().is_match(line) {
            kept.push(String::new());
            continue;
        }
        kept.push(line.to_string());
    }
    inline_code_re().replace_all(&strip_images(&kept.join("\n")), "").into_owned()
}

#[derive(Clone, Copy)]
struct Fence {
    char: char,
    len: usize,
}

fn fence_open(line: &str) -> Option<Fence> {
    let caps = fence_open_re().captures(line)?;
    let token = caps.get(2).or_else(|| caps.get(3))?.as_str();
    let info = caps.get(4).map(|m| m.as_str()).unwrap_or("");
    if token.starts_with('`') && info.contains('`') {
        return None;
    }
    Some(Fence { char: token.chars().next().unwrap_or('`'), len: token.len() })
}

fn fence_close(line: &str, open: Fence) -> bool {
    let Some(caps) = fence_close_re().captures(line) else { return false };
    let Some(token) = caps.get(2).or_else(|| caps.get(3)) else { return false };
    token.as_str().starts_with(open.char) && token.as_str().len() >= open.len
}

fn visible_markup(text: &str) -> String {
    strip_ignored_markup(&blank_fences(text))
}

fn blank_fences(text: &str) -> String {
    let mut fence = None;
    let mut kept = Vec::new();
    for line in split_lines(text) {
        if let Some(open) = fence {
            if fence_close(line, open) {
                fence = None;
            }
            kept.push(String::new());
            continue;
        }
        if let Some(opener) = fence_open(line) {
            fence = Some(opener);
            kept.push(String::new());
            continue;
        }
        kept.push(line.to_string());
    }
    kept.join("\n")
}

fn strip_ignored_markup(text: &str) -> String {
    strip_mdx_comments(&strip_html_comments(text))
}

fn strip_html_comments(text: &str) -> String {
    html_comment_re().replace_all(text, |caps: &Captures| blank_non_nl(caps.get(0).unwrap().as_str())).into_owned()
}

fn strip_mdx_comments(text: &str) -> String {
    mdx_comment_re().replace_all(text, |caps: &Captures| blank_non_nl(caps.get(0).unwrap().as_str())).into_owned()
}

fn strip_images(text: &str) -> String {
    image_re()
        .replace_all(text, |caps: &Captures| {
            let block = caps.get(0).unwrap();
            if escaped_at(text, block.start()) { block.as_str().to_string() } else { blank_non_nl(block.as_str()) }
        })
        .into_owned()
}

fn blank_non_nl(block: &str) -> String {
    block.chars().map(|ch| if ch == '\n' { '\n' } else { ' ' }).collect()
}

fn is_external_href(href: &str) -> bool {
    let dest = strip_dest_title(href);
    scheme_re().is_match(&dest) || dest.starts_with("//")
}

fn escaped_at(text: &str, index: usize) -> bool {
    let bytes = text.as_bytes();
    let mut count = 0;
    let mut cursor = index;
    while cursor > 0 && bytes[cursor - 1] == b'\\' {
        count += 1;
        cursor -= 1;
    }
    count % 2 == 1
}

fn wikilink_is_embed(text: &str, index: usize) -> bool {
    index > 0 && text.as_bytes().get(index - 1) == Some(&b'!') && !escaped_at(text, index - 1)
}

fn strip_atx_closing(text: &str) -> String {
    let stripped = atx_close_re().replace(text, "");
    if stripped.is_empty() { text.to_string() } else { stripped.into_owned() }
}

fn strip_dest_title(href: &str) -> String {
    let trimmed = href.trim();
    let dest = titled_dest(trimmed).unwrap_or_else(|| trimmed.to_string());
    if dest.starts_with('<') && dest.ends_with('>') {
        return dest[1..dest.len() - 1].trim().to_string();
    }
    if has_unescaped_whitespace(&dest) {
        return String::new();
    }
    escaped_space_re().replace_all(&dest, "$1").into_owned()
}

fn has_unescaped_whitespace(dest: &str) -> bool {
    dest.char_indices().any(|(index, ch)| (ch == ' ' || ch == '\t') && !escaped_at(dest, index))
}

fn resolve_known_markdown_path(from: &str, cleaned: &str, known: &HashSet<String>) -> Option<String> {
    let dir = from.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let mut seeds = Vec::new();
    if cleaned.starts_with('.') || !cleaned.contains('/') {
        let joined_path = if dir.is_empty() { cleaned.to_string() } else { format!("{dir}/{cleaned}") };
        if let Some(joined) = normalize_rel(&joined_path) {
            seeds.push(joined);
        }
    }
    seeds.push(cleaned.to_string());
    for seed in seeds {
        for joined in markdown_path_candidates(&seed) {
            if !known.contains(&joined) {
                continue;
            }
            let path = as_rel_path(&joined).ok()?;
            return Some(if joined.ends_with(".md") || joined.ends_with(".mdx") { doc_id(&path) } else { file_id(&path) });
        }
    }
    None
}

fn markdown_path_candidates(path: &str) -> Vec<String> {
    if path.ends_with(".md") || path.ends_with(".mdx") {
        vec![path.to_string()]
    } else {
        vec![path.to_string(), format!("{path}.md"), format!("{path}.mdx")]
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

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("regex")
}

fn wikilink_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| re(r"\[\[([^\n\]|#]+)(?:#([^\n\]|]+))?(?:\|[^\n\]]+)?\]\]"))
}

fn mdlink_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| re(r"!?\[([^\n\]]*)\]\(([^)\n]+)\)"))
}

fn heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| re(r"^( {0,3})(#{1,6})\s+(.+?)\s*$"))
}

fn fence_open_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| re(r"^( {0,3})(?:(`{3,})|(~{3,}))(.*)$"))
}

fn fence_close_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| re(r"^( {0,3})(?:(`{3,})|(~{3,}))[ \t]*$"))
}

fn indent_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| re(r"^(?: {4}| {0,3}\t)"))
}

fn html_comment_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| re(r"(?s)<!--.*?(?:-->|$)"))
}

fn mdx_comment_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| re(r"(?s)\{\/\*.*?(?:\*\/\}|$)"))
}

fn image_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| re(r"!\[[^\]]*\](?:\([^)]*\))?"))
}

fn inline_code_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| re(r"`[^`\n]+`"))
}

fn scheme_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| re(r"^[a-zA-Z][a-zA-Z0-9+.-]*:"))
}

fn atx_close_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| re(r"[ \t]+#+$"))
}

fn titled_dest(trimmed: &str) -> Option<String> {
    let chars: Vec<char> = trimmed.chars().collect();
    let end_quote = *chars.last()?;
    if end_quote != '"' && end_quote != '\'' {
        return None;
    }
    let mut index = 0;
    while index < chars.len() {
        if chars[index].is_whitespace() {
            let mut next = index;
            while next < chars.len() && chars[next].is_whitespace() {
                next += 1;
            }
            if next < chars.len() && chars[next] == end_quote {
                return Some(chars[..index].iter().collect::<String>().trim().to_string());
            }
            index = next;
            continue;
        }
        index += 1;
    }
    None
}

fn escaped_space_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| re(r"\\([ \t])"))
}

fn ext_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| re(r"(?i)\.(?:mdx?|tsx?|jsx?)$"))
}
