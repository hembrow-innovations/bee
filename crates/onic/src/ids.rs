use crate::error::{OnicError, OnicResult};

const PREFIXES: &[&str] = &[
    "file:",
    "symbol:",
    "doc:",
    "heading:",
    "comment:",
    "tag:",
    "unresolved:",
];

pub fn as_rel_path(path: &str) -> OnicResult<String> {
    let normalized = path.replace('\\', "/");
    let normalized = normalized.trim_start_matches("./");
    if normalized.is_empty() || normalized.starts_with('/') || normalized.contains('\0') {
        return Err(OnicError::msg(format!("invalid relative path: {path}")));
    }
    Ok(normalized.to_string())
}

pub fn file_id(path: &str) -> String {
    format!("file:{path}")
}

pub fn symbol_id(path: &str, name: &str) -> String {
    format!("symbol:{path}#{name}")
}

pub fn unresolved_id(name: &str) -> String {
    format!("unresolved:{name}")
}

pub fn doc_id(path: &str) -> String {
    format!("doc:{path}")
}

pub fn heading_id(path: &str, slug: &str) -> String {
    format!("heading:{path}#{slug}")
}

pub fn comment_id(path: &str, line: i64) -> String {
    format!("comment:{path}:{line}")
}

pub fn tag_id(name: &str) -> String {
    format!("tag:{name}")
}

pub fn parse_node_id(raw: &str) -> OnicResult<String> {
    if raw.contains('\0') || !PREFIXES.iter().any(|prefix| raw.starts_with(prefix)) {
        return Err(OnicError::msg(format!("invalid node id: {raw}")));
    }
    Ok(raw.to_string())
}

pub fn slugify(text: &str) -> String {
    let mut slug = String::new();
    let mut dash = false;
    for ch in text.trim().chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                slug.push(lower);
            }
            dash = false;
        } else if !slug.is_empty() && !dash {
            slug.push('-');
            dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() { "heading".to_string() } else { slug }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_drops_punctuation() {
        assert_eq!(slugify("Session Store"), "session-store");
    }
}
