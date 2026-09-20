use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use regex::Regex;

use crate::cli::DocsCmd;
use crate::dest::note::{parse_front_matter, ParseFrontMatter};
use crate::docs::{field_list, list_notes, listed, note_tags, VaultFile};

pub fn run(vault: &Path, source: &str, cmd: &DocsCmd) -> Result<String, String> {
    match cmd {
        DocsCmd::Links {
            path,
            broken,
            orphans,
            outlinks,
            backlinks,
        } => links(
            vault,
            path.as_deref(),
            *broken,
            *orphans,
            *outlinks,
            *backlinks,
        ),
        DocsCmd::Tags { limit, spec } => tags(vault, spec, *limit),
        DocsCmd::Vault { info } => vault_info(vault, source, info.as_deref()),
        _ => Err("internal".into()),
    }
}

fn links(
    vault: &Path,
    path: Option<&str>,
    broken: bool,
    orphans: bool,
    outlinks_only: bool,
    backlinks_only: bool,
) -> Result<String, String> {
    if broken && orphans {
        return Err("use --broken or --orphans, not both".into());
    }
    if !broken && !orphans && path.unwrap_or("").is_empty() {
        return Err("missing path, --broken, or --orphans".into());
    }
    let notes = list_notes(vault, "md");
    let report = links_vault(&notes, path, broken, orphans);
    if broken {
        return Ok(listed(
            &report
                .broken
                .iter()
                .map(|(from, target)| format!("{from}\t{target}"))
                .collect::<Vec<_>>(),
        ));
    }
    if orphans {
        return Ok(listed(&report.orphans));
    }
    if backlinks_only {
        return Ok(listed(&report.backlinks));
    }
    if outlinks_only {
        return Ok(listed(&report.outlinks));
    }
    let mut lines = vec![format!("outlinks: {}", report.outlinks.len())];
    lines.extend(report.outlinks);
    lines.push(format!("backlinks: {}", report.backlinks.len()));
    lines.extend(report.backlinks);
    Ok(format!("{}\n", lines.join("\n")))
}

struct LinkReport {
    outlinks: Vec<String>,
    backlinks: Vec<String>,
    broken: Vec<(String, String)>,
    orphans: Vec<String>,
}

fn links_vault(notes: &[VaultFile], path: Option<&str>, broken: bool, orphans: bool) -> LinkReport {
    let aliases = alias_map(notes);
    let mut edges: Vec<(String, String, Option<String>)> = Vec::new();
    for note in notes {
        let Ok(text) = fs::read_to_string(&note.abs) else {
            continue;
        };
        for target in wiki_targets(&text) {
            let resolved = resolve_link(notes, &aliases, &note.rel, &target);
            edges.push((note.rel.clone(), target, resolved));
        }
    }
    if broken {
        return LinkReport {
            outlinks: Vec::new(),
            backlinks: Vec::new(),
            broken: edges
                .into_iter()
                .filter(|e| e.2.is_none())
                .map(|e| (e.0, e.1))
                .collect(),
            orphans: Vec::new(),
        };
    }
    if orphans {
        let pointed: HashSet<&str> = edges.iter().filter_map(|e| e.2.as_deref()).collect();
        return LinkReport {
            outlinks: Vec::new(),
            backlinks: Vec::new(),
            broken: Vec::new(),
            orphans: notes
                .iter()
                .map(|n| n.rel.clone())
                .filter(|rel| !pointed.contains(rel.as_str()))
                .collect(),
        };
    }
    let rel = path.unwrap_or("");
    let resolved_path = notes
        .iter()
        .find(|n| {
            n.rel == rel || n.rel == format!("{rel}.md") || n.rel.strip_suffix(".md") == Some(rel)
        })
        .map(|n| n.rel.as_str());
    let self_rel = resolved_path.unwrap_or(rel);
    let mut seen_out = HashSet::new();
    let mut outlinks = Vec::new();
    for (from, _, resolved) in &edges {
        if from == self_rel {
            if let Some(r) = resolved {
                if seen_out.insert(r.clone()) {
                    outlinks.push(r.clone());
                }
            }
        }
    }
    let mut seen_back = HashSet::new();
    let mut backlinks = Vec::new();
    for (from, _, resolved) in &edges {
        if resolved.as_deref() == Some(self_rel) && seen_back.insert(from.clone()) {
            backlinks.push(from.clone());
        }
    }
    LinkReport {
        outlinks,
        backlinks,
        broken: Vec::new(),
        orphans: Vec::new(),
    }
}

fn wiki_targets(text: &str) -> Vec<String> {
    let re = Regex::new(r"\[\[([^\]|#]+)(?:#[^\]|]+)?(?:\|[^\]]+)?\]\]").expect("wiki");
    let mut out = Vec::new();
    for cap in re.captures_iter(text) {
        let m = cap.get(0).expect("match");
        if m.start() > 0 && text.as_bytes()[m.start() - 1] == b'!' {
            continue;
        }
        let t = cap.get(1).map(|g| g.as_str().trim()).unwrap_or("");
        if !t.is_empty() {
            out.push(t.to_string());
        }
    }
    out
}

fn alias_map(notes: &[VaultFile]) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    for note in notes {
        let Ok(text) = fs::read_to_string(&note.abs) else {
            continue;
        };
        let ParseFrontMatter::Ok(fields) = parse_front_matter(&text) else {
            continue;
        };
        let aliases = field_list(&fields, "aliases");
        if !aliases.is_empty() {
            map.insert(note.rel.clone(), aliases);
        }
    }
    map
}

fn resolve_link(
    notes: &[VaultFile],
    aliases: &HashMap<String, Vec<String>>,
    from_rel: &str,
    target: &str,
) -> Option<String> {
    let t = target.trim_end_matches(".md").replace('\\', "/");
    if let Some(exact) = notes.iter().find(|n| {
        n.rel == target
            || n.rel == format!("{t}.md")
            || n.rel.strip_suffix(".md") == Some(t.as_str())
    }) {
        return Some(exact.rel.clone());
    }
    let from_dir = from_rel.rfind('/').map(|i| &from_rel[..i]).unwrap_or("");
    let rel_try = if from_dir.is_empty() {
        t.clone()
    } else {
        format!("{from_dir}/{t}")
    }
    .replace('\\', "/");
    if let Some(hit) = notes
        .iter()
        .find(|n| n.rel == rel_try || n.rel == format!("{rel_try}.md"))
    {
        return Some(hit.rel.clone());
    }
    let base = t.rsplit('/').next().unwrap_or(&t);
    let mut matches: Vec<&VaultFile> = notes
        .iter()
        .filter(|n| {
            let name = n.rel.rsplit('/').next().unwrap_or("");
            name == format!("{base}.md") || name.strip_suffix(".md") == Some(base)
        })
        .collect();
    if !matches.is_empty() {
        matches.sort_by(|a, b| a.rel.cmp(&b.rel));
        return Some(matches[0].rel.clone());
    }
    for (rel, als) in aliases {
        if als.iter().any(|a| a == &t || a == target) {
            return Some(rel.clone());
        }
    }
    None
}

fn tags(vault: &Path, spec: &[String], limit: Option<usize>) -> Result<String, String> {
    let files_for = match spec {
        [] => None,
        [m] if m == "list" => None,
        [m] if m == "files" => return Err("Usage: bee docs tags [list|files <tag>]".into()),
        [extra] => return Err(format!("unexpected argument: {extra}")),
        [m, tag] if m == "files" => {
            if tag.is_empty() {
                return Err("Usage: bee docs tags [list|files <tag>]".into());
            }
            Some(tag.as_str())
        }
        [_, extra, ..] => return Err(format!("unexpected argument: {extra}")),
    };
    let notes = list_notes(vault, "md");
    let report = tags_vault(&notes, files_for);
    let cap = limit.unwrap_or(100);
    if files_for.is_some() {
        return Ok(listed(
            &report.files.into_iter().take(cap).collect::<Vec<_>>(),
        ));
    }
    Ok(listed(
        &report
            .counts
            .into_iter()
            .take(cap)
            .map(|(tag, n)| format!("{tag}\t{n}"))
            .collect::<Vec<_>>(),
    ))
}

struct TagReport {
    counts: Vec<(String, usize)>,
    files: Vec<String>,
}

fn tags_vault(notes: &[VaultFile], files_for: Option<&str>) -> TagReport {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut files = Vec::new();
    for note in notes {
        let Ok(text) = fs::read_to_string(&note.abs) else {
            continue;
        };
        let tags = note_tags(&text);
        if let Some(want) = files_for {
            if tags
                .iter()
                .any(|t| t == want || t.starts_with(&format!("{want}/")))
            {
                files.push(note.rel.clone());
            }
        }
        for tag in tags {
            bump(&mut counts, &tag);
            let parts: Vec<&str> = tag.split('/').collect();
            for i in 1..parts.len() {
                bump(&mut counts, &parts[..i].join("/"));
            }
        }
    }
    let mut list: Vec<(String, usize)> = counts.into_iter().collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    TagReport {
        counts: list,
        files,
    }
}

fn bump(counts: &mut HashMap<String, usize>, tag: &str) {
    *counts.entry(tag.to_string()).or_insert(0) += 1;
}

fn vault_info(vault: &Path, source: &str, info: Option<&str>) -> Result<String, String> {
    if let Some(tok) = info {
        if tok != "info" {
            return Err(format!("unexpected argument: {tok}"));
        }
    }
    let n = list_notes(vault, "md").len();
    Ok(format!(
        "vault: {}\nsource: {source}\nnotes: {n}\n",
        vault.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Commands};
    use crate::docs::run_with_env;
    use crate::execute;
    use std::fs;
    use tempfile::tempdir;

    fn write_rel(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn docs(root: &Path, vault: Option<&str>, cmd: DocsCmd) -> Result<String, String> {
        run_with_env(root, vault, None, &cmd)
    }

    fn links_cmd(path: Option<&str>) -> DocsCmd {
        DocsCmd::Links {
            path: path.map(str::to_string),
            broken: false,
            orphans: false,
            outlinks: false,
            backlinks: false,
        }
    }

    fn cli(vault: Option<String>, cmd: DocsCmd) -> Cli {
        Cli {
            project: None,
            wt: vec![],
            command: Commands::Docs { vault, cmd },
        }
    }

    #[test]
    fn links_resolve_filename_and_report_broken() {
        let dir = tempdir().unwrap();
        write_rel(
            dir.path(),
            "docs/guides/a.md",
            "---\naliases: [alias-a]\n---\n\nSee [[b]] and [[missing]].\n",
        );
        write_rel(
            dir.path(),
            "docs/architecture/b.md",
            "back to [[guides/a]]\n",
        );
        write_rel(dir.path(), "docs/orphan.md", "alone\n");
        let a = docs(dir.path(), None, links_cmd(Some("guides/a.md"))).unwrap();
        assert!(a.contains("outlinks: 1"), "{a}");
        assert!(a.contains("architecture/b.md"), "{a}");
        assert!(a.contains("backlinks: 1"), "{a}");
        let broken = docs(
            dir.path(),
            None,
            DocsCmd::Links {
                path: None,
                broken: true,
                orphans: false,
                outlinks: false,
                backlinks: false,
            },
        )
        .unwrap();
        assert!(broken.contains("guides/a.md\tmissing"), "{broken}");
        assert!(broken.contains("count: 1"), "{broken}");
        let orphans = docs(
            dir.path(),
            None,
            DocsCmd::Links {
                path: None,
                broken: false,
                orphans: true,
                outlinks: false,
                backlinks: false,
            },
        )
        .unwrap();
        assert!(orphans.contains("orphan.md"), "{orphans}");
        assert!(!orphans.contains("guides/a.md"), "{orphans}");
    }

    #[test]
    fn tags_roll_up_nested_and_files_for() {
        let dir = tempdir().unwrap();
        write_rel(
            dir.path(),
            "docs/a.md",
            "---\ntags: [project/axi, stack]\n---\n\n# x\n",
        );
        write_rel(
            dir.path(),
            "docs/b.md",
            "---\ntags: [project]\n---\n\n# y\n",
        );
        let all = docs(
            dir.path(),
            None,
            DocsCmd::Tags {
                limit: None,
                spec: vec![],
            },
        )
        .unwrap();
        assert!(all.contains("project\t2"), "{all}");
        let files = docs(
            dir.path(),
            None,
            DocsCmd::Tags {
                limit: None,
                spec: vec!["files".into(), "project".into()],
            },
        )
        .unwrap();
        assert!(files.contains("a.md"), "{files}");
        assert!(files.contains("b.md"), "{files}");
        assert!(files.contains("count: 2"), "{files}");
    }

    #[test]
    fn vault_prints_identity() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/guides/a.md", "# a\n");
        let out = docs(dir.path(), None, DocsCmd::Vault { info: None }).unwrap();
        assert!(out.contains("source: walk-up"), "{out}");
        assert!(out.contains("notes: 1"), "{out}");
        assert!(
            out.contains(&format!("vault: {}", dir.path().join("docs").display())),
            "{out}"
        );
    }

    #[test]
    fn default_store_graph_walk_up() {
        let dir = tempdir().unwrap();
        write_rel(
            dir.path(),
            "docs/a.md",
            "---\ntags: [stack]\n---\n\nSee [[missing]].\n",
        );
        let nested = dir.path().join("packages/app");
        fs::create_dir_all(&nested).unwrap();
        let out = run_with_env(
            &nested,
            None,
            None,
            &DocsCmd::Tags {
                limit: None,
                spec: vec![],
            },
        )
        .unwrap();
        assert!(out.contains("stack\t1"), "{out}");
        let info = run_with_env(&nested, None, None, &DocsCmd::Vault { info: None }).unwrap();
        assert!(info.contains("source: walk-up"), "{info}");
    }

    #[test]
    fn vault_flag_graph() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/skip.md", "---\ntags: [docs]\n---\n");
        write_rel(dir.path(), "other/hit.md", "---\ntags: [secret]\n---\n");
        let other = dir.path().join("other").to_string_lossy().into_owned();
        let out = docs(
            dir.path(),
            Some(&other),
            DocsCmd::Tags {
                limit: None,
                spec: vec![],
            },
        )
        .unwrap();
        assert!(out.contains("secret\t1"), "{out}");
        assert!(!out.contains("docs\t"), "{out}");
        let info = docs(
            dir.path(),
            Some(&other),
            DocsCmd::Vault {
                info: Some("info".into()),
            },
        )
        .unwrap();
        assert!(info.contains("source: flag"), "{info}");
    }

    #[test]
    fn links_require_path_or_audit() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/.keep", "");
        let err = docs(
            dir.path(),
            None,
            DocsCmd::Links {
                path: None,
                broken: false,
                orphans: false,
                outlinks: false,
                backlinks: false,
            },
        )
        .unwrap_err();
        assert_eq!(err, "missing path, --broken, or --orphans");
    }

    #[test]
    fn execute_links_with_vault_flag() {
        let dir = tempdir().unwrap();
        write_rel(dir.path(), "docs/a.md", "See [[missing]].\n");
        let vault = dir.path().join("docs").to_string_lossy().into_owned();
        assert_eq!(
            execute(
                cli(
                    Some(vault),
                    DocsCmd::Links {
                        path: None,
                        broken: true,
                        orphans: false,
                        outlinks: false,
                        backlinks: false,
                    }
                ),
                dir.path()
            ),
            0
        );
    }
}
