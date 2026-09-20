use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::cli::DocsCmd;
use crate::note::write::iso_now;

const KINDS: &[&str] = &[
    "overview",
    "architecture",
    "system-design",
    "adr",
    "rfc",
    "purpose",
    "contract",
    "test",
    "spec",
    "api",
    "schema",
    "non-functional",
    "standard",
    "style",
    "guide",
];

struct Dest {
    rel: String,
    abs: PathBuf,
    id: String,
    title: String,
    template: String,
}

struct NewFields<'a> {
    kind: &'a str,
    domain: &'a str,
    area: &'a str,
    title: &'a str,
    slug: Option<&'a str>,
    tags: &'a [String],
}

pub fn run(vault: &Path, start: &Path, cmd: &DocsCmd) -> Result<String, String> {
    match cmd {
        DocsCmd::New {
            kind,
            title,
            domain,
            area,
            slug,
            tag,
        } => {
            let day = iso_now();
            create(
                vault,
                start,
                kind,
                title.as_deref(),
                domain.as_deref(),
                area.as_deref(),
                slug.as_deref(),
                tag,
                &day[..10],
            )
        }
        _ => Err("internal".into()),
    }
}

fn create(
    vault: &Path,
    start: &Path,
    kind: &str,
    title: Option<&str>,
    domain: Option<&str>,
    area: Option<&str>,
    slug: Option<&str>,
    tags: &[String],
    day: &str,
) -> Result<String, String> {
    if !KINDS.contains(&kind) {
        return Err(format!("unknown kind: {kind}"));
    }
    let domain = require(domain, "missing --domain")?;
    let area = require(area, "missing --area")?;
    let title = require(title, "missing --title")?;
    let fields = NewFields {
        kind,
        domain,
        area,
        title,
        slug,
        tags,
    };
    let templates =
        find_docs_templates(start, std::env::var("HEIO_DOCS_TEMPLATES").ok().as_deref())
            .ok_or_else(|| "docs templates not found".to_string())?;
    let rel = create_doc(vault, &fields, &templates, day)?;
    Ok(format!("{rel}\n"))
}

fn require<'a>(value: Option<&'a str>, msg: &str) -> Result<&'a str, String> {
    match value {
        Some(s) if !s.is_empty() => Ok(s),
        _ => Err(msg.into()),
    }
}

fn create_doc(
    vault: &Path,
    fields: &NewFields,
    templates_dir: &Path,
    day: &str,
) -> Result<String, String> {
    let dest = dest_for(vault, fields)?;
    if dest.abs.exists() {
        return Err(format!("file exists: {}", dest.rel));
    }
    let template_path = templates_dir.join(&dest.template);
    if !template_path.is_file() {
        return Err(format!("template not found: {}", dest.template));
    }
    let raw = fs::read_to_string(&template_path)
        .map_err(|_| format!("template not found: {}", dest.template))?;
    let filled = fill_template(
        &raw,
        &dest.id,
        &dest.title,
        fields.domain,
        fields.area,
        day,
        fields.tags,
    )
    .ok_or_else(|| format!("bad template: {}", dest.template))?;
    if let Some(parent) = dest.abs.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&dest.abs, filled).map_err(|e| e.to_string())?;
    Ok(dest.rel)
}

fn find_docs_templates(start: &Path, env: Option<&str>) -> Option<PathBuf> {
    if let Some(raw) = env.filter(|s| !s.is_empty()) {
        let p = PathBuf::from(raw);
        return if p.is_dir() { Some(p) } else { None };
    }
    let mut dir = if start.is_absolute() {
        start.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(start)
    };
    loop {
        for rel in [
            "ai-stack/skills/docs/templates",
            ".opencode/skills/docs/templates",
        ] {
            let candidate = dir.join(rel);
            if candidate.is_dir() {
                return Some(candidate);
            }
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent.to_path_buf(),
            _ => return None,
        }
    }
}

fn dest_for(vault: &Path, fields: &NewFields) -> Result<Dest, String> {
    let slug = fields.slug.filter(|s| !s.is_empty());
    let need_slug = !matches!(fields.kind, "purpose" | "contract" | "test");
    if need_slug && slug.is_none() {
        return Err("missing --slug".into());
    }
    if let Some(s) = slug {
        if !slug_ok(s) {
            return Err(format!("bad slug: {s}"));
        }
    }
    match fields.kind {
        "purpose" | "contract" | "test" => {
            let name = format!("{}.md", fields.kind);
            let rel = posix(&["specs", fields.domain, fields.area, &name]);
            Ok(Dest {
                abs: vault.join(PathBuf::from(&rel)),
                id: format!("{}-{}", fields.kind, fields.area),
                title: fields.title.to_string(),
                template: format!("{}.md", fields.kind),
                rel,
            })
        }
        "spec" => {
            let s = slug.expect("slug");
            stem_custom(
                vault,
                &posix(&["specs", fields.domain, fields.area]),
                &format!("spec-{s}"),
                fields.title,
                "spec.md",
            )
        }
        "adr" => {
            let s = slug.expect("slug");
            let dir = adr_dir(vault);
            let n = next_number(&dir, Regex::new(r"^(\d+)-").expect("adr"));
            let padded = format!("{n:04}");
            let rel = posix(&[&dir_rel(vault, &dir), &format!("{padded}-{s}.md")]);
            Ok(Dest {
                abs: vault.join(PathBuf::from(&rel)),
                id: format!("adr-{n}"),
                title: format!("ADR-{padded}: {}", fields.title),
                template: "adr.md".into(),
                rel,
            })
        }
        "rfc" => {
            let s = slug.expect("slug");
            let n = next_number(
                &vault.join("decisions/rfc"),
                Regex::new(r"^rfc(\d+)-").expect("rfc"),
            );
            let rel = posix(&["decisions", "rfc", &format!("rfc{n}-{s}.md")]);
            Ok(Dest {
                abs: vault.join(PathBuf::from(&rel)),
                id: format!("rfc{n}-{s}"),
                title: format!("RFC-{n}: {}", fields.title),
                template: "rfc.md".into(),
                rel,
            })
        }
        "overview" => stem(
            vault,
            "overview",
            &format!("overview-{}", slug.expect("slug")),
            fields.title,
            None,
        ),
        "architecture" => stem(
            vault,
            "architecture",
            &format!("architecture-{}", slug.expect("slug")),
            fields.title,
            None,
        ),
        "system-design" => stem(
            vault,
            "architecture",
            &format!("system-design-{}", slug.expect("slug")),
            fields.title,
            Some("system-design.md"),
        ),
        "api" => stem(
            vault,
            "api",
            &format!("api-{}", slug.expect("slug")),
            fields.title,
            None,
        ),
        "schema" => stem(
            vault,
            "api/schema",
            &format!("schema-{}", slug.expect("slug")),
            fields.title,
            None,
        ),
        "non-functional" => stem(
            vault,
            "non-functional",
            slug.expect("slug"),
            fields.title,
            Some("non-functional.md"),
        ),
        "standard" => stem(
            vault,
            "standards",
            &format!("standards-{}", slug.expect("slug")),
            fields.title,
            Some("standard.md"),
        ),
        "style" => stem(
            vault,
            "style",
            &format!("style-{}", slug.expect("slug")),
            fields.title,
            None,
        ),
        "guide" => stem(
            vault,
            "guides",
            &format!("guides-{}", slug.expect("slug")),
            fields.title,
            Some("guide.md"),
        ),
        other => Err(format!("unknown kind: {other}")),
    }
}

fn stem(
    vault: &Path,
    dir: &str,
    stem_name: &str,
    title: &str,
    template: Option<&str>,
) -> Result<Dest, String> {
    let tmpl = template
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}.md", stem_name.split('-').next().unwrap_or(stem_name)));
    stem_custom(vault, dir, stem_name, title, &tmpl)
}

fn stem_custom(
    vault: &Path,
    dir: &str,
    stem_name: &str,
    title: &str,
    template: &str,
) -> Result<Dest, String> {
    let rel = posix(&[dir, &format!("{stem_name}.md")]);
    Ok(Dest {
        abs: vault.join(PathBuf::from(&rel)),
        id: stem_name.to_string(),
        title: title.to_string(),
        template: template.to_string(),
        rel,
    })
}

fn posix(parts: &[&str]) -> String {
    parts
        .iter()
        .flat_map(|p| p.split('/'))
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

fn adr_dir(vault: &Path) -> PathBuf {
    let nested = vault.join("decisions/adr");
    if nested.is_dir() {
        nested
    } else {
        vault.join("adr")
    }
}

fn dir_rel(vault: &Path, abs: &Path) -> String {
    abs.strip_prefix(vault)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

fn next_number(dir: &Path, re: Regex) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 1;
    };
    let mut max = 0usize;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if let Some(c) = re.captures(name) {
            if let Ok(n) = c.get(1).unwrap().as_str().parse::<usize>() {
                max = max.max(n);
            }
        }
    }
    max + 1
}

fn slug_ok(slug: &str) -> bool {
    let mut parts = slug.split('-');
    let some = parts.next().is_some_and(|p| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    });
    some && parts.all(|p| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    })
}

fn fill_template(
    text: &str,
    id: &str,
    title: &str,
    domain: &str,
    area: &str,
    day: &str,
    tags: &[String],
) -> Option<String> {
    let mut patch = vec![
        ("id", json_string(id)),
        ("title", json_string(title)),
        ("domain", domain.to_string()),
        ("area", area.to_string()),
        ("created_at", json_string(day)),
        ("updated_at", json_string(day)),
    ];
    if !tags.is_empty() {
        patch.push(("tags", format!("[{}]", tags.join(", "))));
    }
    let owned: Vec<(String, String)> = patch.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    let refs: Vec<(&str, &str)> = owned
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let next = set_fields(text, &refs)?;
    let re = Regex::new(r"(?m)^# .+$").ok()?;
    Some(re.replace(&next, format!("# {title}")).into_owned())
}

fn set_fields(text: &str, patch: &[(&str, &str)]) -> Option<String> {
    if !text.starts_with("---\n") {
        return None;
    }
    let rel = text[4..].find("\n---")?;
    let end = rel + 4;
    let mut yaml = text[4..end].to_string();
    let rest = text.get(end + 4..)?;
    for (key, value) in patch {
        let line = format!("{key}: {value}");
        let needle = format!("{key}:");
        let mut replaced = false;
        let mut out = String::new();
        for existing in yaml.lines() {
            if !out.is_empty() {
                out.push('\n');
            }
            if !replaced && existing.starts_with(&needle) {
                out.push_str(&line);
                replaced = true;
            } else {
                out.push_str(existing);
            }
        }
        yaml = if replaced {
            out
        } else {
            format!("{}\n{line}\n", yaml.trim_end())
        };
    }
    Some(format!("---\n{}\n---{rest}", yaml.trim_end()))
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::run_with_env;
    use std::fs;
    use tempfile::tempdir;

    const GUIDE_TMPL: &str = "---\nid: \"guides-slug\"\ntitle: \"guide title\"\nkind: guide\ndomain: system\narea: guides\ntags: []\ncreated_at: \"YYYY-MM-DD\"\nupdated_at: \"YYYY-MM-DD\"\n---\n\n# guide title\n\nbody\n";
    const ADR_TMPL: &str = "---\nid: \"adr-N\"\ntitle: \"ADR-NNNN: decision title\"\nkind: adr\ndomain: system\narea: decisions\ntags: []\ncreated_at: \"YYYY-MM-DD\"\nupdated_at: \"YYYY-MM-DD\"\n---\n\n# ADR-NNNN: decision title\n\n## Context\n";

    fn write_rel(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn docs(root: &Path, cmd: DocsCmd) -> Result<String, String> {
        run_with_env(root, None, None, &cmd)
    }

    fn plant_templates(root: &Path) {
        write_rel(root, ".opencode/skills/docs/templates/guide.md", GUIDE_TMPL);
        write_rel(root, ".opencode/skills/docs/templates/adr.md", ADR_TMPL);
    }

    fn new_cmd(
        kind: &str,
        title: Option<&str>,
        domain: Option<&str>,
        area: Option<&str>,
        slug: Option<&str>,
    ) -> DocsCmd {
        DocsCmd::New {
            kind: kind.into(),
            title: title.map(str::to_string),
            domain: domain.map(str::to_string),
            area: area.map(str::to_string),
            slug: slug.map(str::to_string),
            tag: vec![],
        }
    }

    #[test]
    fn new_guide_fills_template() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/.keep", "");
        let templates = dir.path().join("tmpl");
        fs::create_dir_all(&templates).unwrap();
        fs::write(templates.join("guide.md"), GUIDE_TMPL).unwrap();
        let tags = ["stack".to_string()];
        let fields = NewFields {
            kind: "guide",
            domain: "heio",
            area: "guides",
            title: "Stack",
            slug: Some("stack"),
            tags: &tags,
        };
        let vault = dir.path().join("docs");
        let rel = create_doc(&vault, &fields, &templates, "2026-09-13").unwrap();
        assert_eq!(rel, "guides/guides-stack.md");
        let text = fs::read_to_string(vault.join(&rel)).unwrap();
        assert!(text.contains("id: \"guides-stack\""), "{text}");
        assert!(text.contains("title: \"Stack\""), "{text}");
        assert!(text.contains("domain: heio"), "{text}");
        assert!(text.contains("created_at: \"2026-09-13\""), "{text}");
        assert!(text.contains("# Stack"), "{text}");
    }

    #[test]
    fn new_adr_uses_host_folder() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/decisions/adr/.keep", "");
        plant_templates(dir.path());
        let out = docs(
            dir.path(),
            new_cmd(
                "adr",
                Some("Pick"),
                Some("hive"),
                Some("decisions"),
                Some("pick"),
            ),
        )
        .unwrap();
        assert_eq!(out, "decisions/adr/0001-pick.md\n");
        let text = fs::read_to_string(dir.path().join("docs/decisions/adr/0001-pick.md")).unwrap();
        assert!(text.contains("id: \"adr-1\""), "{text}");
        assert!(text.contains("title: \"ADR-0001: Pick\""), "{text}");
        assert!(text.contains("# ADR-0001: Pick"), "{text}");
        assert!(text.contains("kind: adr"), "{text}");
    }

    #[test]
    fn purpose_uses_area_id_and_refuses_bad_slug() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/.keep", "");
        let vault = dir.path().join("docs");
        let fields = NewFields {
            kind: "purpose",
            domain: "loop",
            area: "watchdog",
            title: "Watchdog purpose",
            slug: None,
            tags: &[],
        };
        let dest = dest_for(&vault, &fields).unwrap();
        assert_eq!(dest.rel, "specs/loop/watchdog/purpose.md");
        assert_eq!(dest.id, "purpose-watchdog");
        let bad = NewFields {
            kind: "guide",
            domain: "heio",
            area: "guides",
            title: "X",
            slug: Some("Nope"),
            tags: &[],
        };
        assert!(dest_for(&vault, &bad).is_err());
    }

    #[test]
    fn create_refuses_existing() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/.keep", "");
        plant_templates(dir.path());
        let cmd = new_cmd(
            "guide",
            Some("Stack"),
            Some("heio"),
            Some("guides"),
            Some("stack"),
        );
        assert_eq!(docs(dir.path(), cmd).unwrap(), "guides/guides-stack.md\n");
        let again = docs(
            dir.path(),
            new_cmd(
                "guide",
                Some("Stack"),
                Some("heio"),
                Some("guides"),
                Some("stack"),
            ),
        )
        .unwrap_err();
        assert!(again.contains("file exists"), "{again}");
    }

    #[test]
    fn missing_new_fields_fail() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/.keep", "");
        plant_templates(dir.path());
        let err = docs(
            dir.path(),
            new_cmd("guide", None, Some("heio"), Some("guides"), Some("stack")),
        )
        .unwrap_err();
        assert_eq!(err, "missing --title");
    }
}
