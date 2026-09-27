use std::fs;
use std::path::Path;

use hive_core::HiveError;
use serde_yaml::Value;

use crate::dest::note::{parse_front_matter, ParseFrontMatter};

const ALWAYS_ON: &[&str] = &[
    "docs/guides/guides-intent-system.md",
    "docs/guides/guides-behaviour-contracts.md",
    "docs/guides/guides-agent-gotchas.md",
];

const HOT_ROOTS: &[&str] = &[
    "docs/specs",
    "docs/guides",
    "docs/overview",
    "docs/architecture",
    ".hivemind/planning/tickets",
    ".hivemind/planning/sprints",
    "docs/adr",
];

const SKIP_DIRS: &[&str] = &["archive", "closed", "completed"];

const EXCLUDED: &[&str] = &[
    "docs/log/changelog/**",
    "docs/99_scribble/**",
    ".hivemind/tickets/**/closed/**",
    ".hivemind/planning/**/abandoned/**",
];

const STOP: &[&str] = &[
    "the",
    "a",
    "an",
    "and",
    "or",
    "to",
    "of",
    "for",
    "in",
    "on",
    "is",
    "are",
    "be",
    "when",
    "with",
    "from",
    "that",
    "this",
    "it",
    "as",
    "at",
    "by",
    "not",
    "no",
    "do",
    "does",
    "user",
    "says",
    "fix",
    "bug",
    "wrong",
    "broken",
    "should",
    "must",
    "how",
    "what",
    "why",
    "can",
    "i",
    "we",
    "our",
    "my",
    "me",
    "about",
    "into",
    "over",
    "under",
    "status",
    "open",
    "closed",
    "ready",
    "agent",
    "brief",
    "scope",
    "acceptance",
    "verification",
];

const BRIEF_CAP: usize = 1200;

pub struct PackSelectors {
    pub area: Option<String>,
    pub query: Option<String>,
    pub unit: Option<String>,
    pub domain: Option<String>,
    pub k: usize,
}

struct Note {
    rel: String,
    body: String,
}

pub fn build_pack(vault: &Path, sel: PackSelectors) -> Result<String, HiveError> {
    let (area, query, domain) = resolve_selectors(vault, &sel)?;
    let k = sel.k;
    let notes = walk_hot(vault);
    let tokens = query_tokens(&query);
    let slug = area.as_deref().map(area_slug);
    let mut must = Vec::new();
    for rel in ALWAYS_ON {
        if vault.join(rel).is_file() {
            must.push((*rel).to_string());
        }
    }
    let area_notes: Vec<&Note> = match slug.as_deref() {
        Some(area) => notes
            .iter()
            .filter(|n| area_score(&n.rel, area).is_some())
            .collect(),
        None => Vec::new(),
    };
    let mut purposes: Vec<String> = area_notes
        .iter()
        .filter(|n| basename(&n.rel) == "purpose.md")
        .map(|n| n.rel.clone())
        .collect();
    purposes.sort();
    for p in purposes {
        push_unique(&mut must, p);
    }
    let mut contracts: Vec<&Note> = area_notes
        .iter()
        .copied()
        .filter(|n| basename(&n.rel) == "contract.md")
        .collect();
    contracts.sort_by(|a, b| {
        contract_rank(&b.rel, &tokens)
            .partial_cmp(&contract_rank(&a.rel, &tokens))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.rel.cmp(&b.rel))
    });
    for n in contracts.iter().take(4) {
        push_unique(&mut must, n.rel.clone());
    }
    let mut area_hits: Vec<(f64, String)> = slug
        .as_deref()
        .map(|area| {
            area_notes
                .iter()
                .filter_map(|n| area_score(&n.rel, area).map(|s| (s, n.rel.clone())))
                .collect()
        })
        .unwrap_or_default();
    area_hits.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(&b.1))
    });
    let cap = k.max(12);
    area_hits.truncate(cap);
    let mut related = Vec::new();
    for (_, rel) in area_hits {
        if must.iter().any(|m| m == &rel) {
            continue;
        }
        related.push(rel);
    }
    if !tokens.is_empty() {
        let mut ranked: Vec<(f64, String)> = notes
            .iter()
            .filter(|n| !must.iter().any(|m| m == &n.rel) && !related.iter().any(|r| r == &n.rel))
            .filter_map(|n| {
                let s = token_score(&n.body, &n.rel, &tokens);
                (s > 0.0).then_some((s, n.rel.clone()))
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.cmp(&b.1))
        });
        ranked.truncate(k);
        for (_, rel) in ranked {
            if related.len() >= k {
                break;
            }
            related.push(rel);
        }
    }
    related.truncate(k);
    Ok(render(&query, area.as_deref(), domain.as_deref(), &must, &related))
}

fn resolve_selectors(
    vault: &Path,
    sel: &PackSelectors,
) -> Result<(Option<String>, String, Option<String>), HiveError> {
    let mut area = sel.area.clone();
    let mut query = sel.query.clone().unwrap_or_default();
    let mut domain = sel.domain.clone();
    if let Some(unit) = &sel.unit {
        let path = unit_path(vault, unit);
        let text = fs::read_to_string(&path)
            .map_err(|_| HiveError::not_found(format!("unit not found: {unit}")))?;
        let (title, note_area, note_domain, brief) = unit_fields(&text);
        if area.is_none() {
            area = note_area.clone();
        }
        if domain.is_none() {
            domain = note_domain.clone();
        }
        if query.is_empty() {
            let mut parts = Vec::new();
            if let Some(t) = title {
                parts.push(t);
            }
            if let Some(a) = note_area {
                parts.push(a);
            }
            if let Some(d) = note_domain {
                parts.push(d);
            }
            if !brief.is_empty() {
                parts.push(brief);
            }
            query = parts.join(" ");
        }
    }
    Ok((area, query, domain))
}

fn unit_path(vault: &Path, unit: &str) -> std::path::PathBuf {
    let p = Path::new(unit);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        vault.join(p)
    }
}

fn unit_fields(text: &str) -> (Option<String>, Option<String>, Option<String>, String) {
    let mut title = None;
    let mut area = None;
    let mut domain = None;
    if let ParseFrontMatter::Ok(map) = parse_front_matter(text) {
        title = yaml_string(&map, "title");
        area = yaml_string(&map, "area");
        domain = yaml_string(&map, "domain");
    }
    let brief = agent_brief(text);
    (title, area, domain, brief)
}

fn yaml_string(map: &serde_yaml::Mapping, key: &str) -> Option<String> {
    match map.get(Value::String(key.into())) {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

fn agent_brief(text: &str) -> String {
    let mut lines = text.lines();
    let mut depth = None;
    let mut out = String::new();
    for line in lines.by_ref() {
        let name = line.trim_start_matches('#').trim();
        if line.trim().starts_with('#') && name == "Agent Brief" {
            depth = Some(line.chars().take_while(|c| *c == '#').count());
            break;
        }
    }
    let Some(depth) = depth else {
        return String::new();
    };
    for line in lines {
        let name = line.trim_start_matches('#').trim();
        if line.trim().starts_with('#') && !name.is_empty() {
            let n = line.chars().take_while(|c| *c == '#').count();
            if n <= depth {
                break;
            }
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
        if out.len() >= BRIEF_CAP {
            out.truncate(BRIEF_CAP);
            break;
        }
    }
    out
}

fn walk_hot(vault: &Path) -> Vec<Note> {
    let mut out = Vec::new();
    for rel in HOT_ROOTS {
        let dir = vault.join(rel);
        if dir.is_dir() {
            visit(vault, &dir, &mut out);
        }
    }
    out
}

fn visit(vault: &Path, dir: &Path, out: &mut Vec<Note>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if SKIP_DIRS.contains(&name) {
            continue;
        }
        let full = entry.path();
        let Ok(st) = entry.metadata() else { continue };
        if st.is_dir() {
            visit(vault, &full, out);
            continue;
        }
        if !name.ends_with(".md") {
            continue;
        }
        let rel = posix_rel(vault, &full);
        if excluded(&rel) {
            continue;
        }
        let Ok(body) = fs::read_to_string(&full) else {
            continue;
        };
        out.push(Note { rel, body });
    }
}

fn posix_rel(vault: &Path, abs: &Path) -> String {
    abs.strip_prefix(vault)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

fn excluded(rel: &str) -> bool {
    rel.starts_with("docs/log/changelog/")
        || rel.starts_with("docs/99_scribble/")
        || rel.contains("/abandoned/")
        || closed_tickets(rel)
}

fn closed_tickets(rel: &str) -> bool {
    rel.strip_prefix(".hivemind/tickets/")
        .is_some_and(|rest| rest.contains("/closed/"))
}

fn area_slug(area: &str) -> String {
    area.to_lowercase().replace('_', "-")
}

fn under_area(rel: &str, area: &str) -> bool {
    let path = format!("/{rel}");
    let slash = format!("/{area}/");
    let hyphen = format!("/{area}-");
    let base = basename(rel);
    path.contains(&slash)
        || path.contains(&hyphen)
        || base == format!("{area}.md")
        || base == format!("{area}-contract.md")
}

fn basename(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

fn area_score(rel: &str, area: &str) -> Option<f64> {
    let adr_hit = rel.contains("/adr/") && rel.to_lowercase().contains(area);
    if !under_area(rel, area) && !adr_hit {
        return None;
    }
    let base = basename(rel);
    let mut s: f64 = if base == "contract.md" {
        3.0
    } else if base == "purpose.md" {
        2.5
    } else if base == "index.md" {
        1.4
    } else {
        1.1
    };
    if adr_hit {
        s = s.max(2.0);
    }
    Some(s)
}

fn contract_rank(rel: &str, tokens: &[String]) -> f64 {
    let lower = rel.to_lowercase();
    tokens.iter().filter(|t| lower.contains(t.as_str())).count() as f64 * 2.0
}

fn query_tokens(query: &str) -> Vec<String> {
    let lower = query.to_lowercase();
    let mut out = Vec::new();
    let chars: Vec<char> = lower.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !token_start(chars[i]) {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < chars.len() && token_cont(chars[i]) {
            i += 1;
        }
        if i - start >= 3 {
            let tok: String = chars[start..i].iter().collect();
            if !STOP.contains(&tok.as_str()) && !out.contains(&tok) {
                out.push(tok);
            }
        }
    }
    out
}

fn token_start(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

fn token_cont(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '\'' || c == '/' || c == '-'
}

fn token_score(body: &str, rel: &str, tokens: &[String]) -> f64 {
    if tokens.is_empty() {
        return 0.0;
    }
    let hay = body.to_lowercase();
    let path = rel.to_lowercase();
    let n = tokens.len() as f64;
    let mut hits = 0.0;
    let mut score = 0.0;
    for t in tokens {
        if !hay.contains(t) {
            continue;
        }
        hits += 1.0;
        if path.contains(t) {
            score += 0.5;
        }
    }
    if hits == 0.0 {
        return 0.0;
    }
    score += hits / n;
    let base = basename(rel);
    if base == "contract.md" {
        score += 0.35;
    }
    if base == "purpose.md" {
        score += 0.2;
    }
    if rel.contains("/adr/") {
        score += 0.25;
    }
    if base == "glossary.md" {
        score += 0.15;
    }
    score
}

fn push_unique(out: &mut Vec<String>, rel: String) {
    if !out.iter().any(|x| x == &rel) {
        out.push(rel);
    }
}

fn render(
    query: &str,
    area: Option<&str>,
    domain: Option<&str>,
    must: &[String],
    related: &[String],
) -> String {
    let mut s = String::from("# Query\n");
    if !query.is_empty() {
        s.push_str(query);
        s.push('\n');
    }
    s.push_str("\n# Area\n");
    if let Some(a) = area {
        s.push_str(a);
        s.push('\n');
    }
    if let Some(d) = domain {
        s.push_str(d);
        s.push('\n');
    }
    s.push_str("\n# Must read\n");
    for p in must {
        s.push_str("- ");
        s.push_str(p);
        s.push('\n');
    }
    s.push_str("\n# Related\n");
    for p in related {
        s.push_str("- ");
        s.push_str(p);
        s.push('\n');
    }
    s.push_str("\n# Excluded\n");
    for p in EXCLUDED {
        s.push_str("- ");
        s.push_str(p);
        s.push('\n');
    }
    s.push_str("\n# Next\n");
    s
}
