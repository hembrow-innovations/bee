pub(crate) mod write;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::cli::NoteKind;
use crate::{hive_root, lookup_notes};

pub use write::{claim, housekeep, iso_now, iso_stamp, set_status};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdCollision {
    pub kind: &'static str,
    pub padded: String,
    pub live: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SeqKind {
    Ticket,
    Task,
    Slice,
    Location,
    Rounds,
}

const SEQ_KINDS: [SeqKind; 5] = [
    SeqKind::Ticket,
    SeqKind::Task,
    SeqKind::Slice,
    SeqKind::Location,
    SeqKind::Rounds,
];

struct KindIndex {
    high: u32,
    live: BTreeMap<u32, Vec<String>>,
}

struct Census {
    ticket: KindIndex,
    task: KindIndex,
    slice: KindIndex,
    location: KindIndex,
    rounds: KindIndex,
}

impl SeqKind {
    fn name(self) -> &'static str {
        match self {
            SeqKind::Ticket => "ticket",
            SeqKind::Task => "task",
            SeqKind::Slice => "slice",
            SeqKind::Location => "location",
            SeqKind::Rounds => "rounds",
        }
    }
}

impl Census {
    fn empty() -> Self {
        Self {
            ticket: KindIndex {
                high: 0,
                live: BTreeMap::new(),
            },
            task: KindIndex {
                high: 0,
                live: BTreeMap::new(),
            },
            slice: KindIndex {
                high: 0,
                live: BTreeMap::new(),
            },
            location: KindIndex {
                high: 0,
                live: BTreeMap::new(),
            },
            rounds: KindIndex {
                high: 0,
                live: BTreeMap::new(),
            },
        }
    }

    fn slot(&mut self, kind: SeqKind) -> &mut KindIndex {
        match kind {
            SeqKind::Ticket => &mut self.ticket,
            SeqKind::Task => &mut self.task,
            SeqKind::Slice => &mut self.slice,
            SeqKind::Location => &mut self.location,
            SeqKind::Rounds => &mut self.rounds,
        }
    }

    fn slot_ref(&self, kind: SeqKind) -> &KindIndex {
        match kind {
            SeqKind::Ticket => &self.ticket,
            SeqKind::Task => &self.task,
            SeqKind::Slice => &self.slice,
            SeqKind::Location => &self.location,
            SeqKind::Rounds => &self.rounds,
        }
    }
}

fn seq_kind(kind: NoteKind) -> SeqKind {
    match kind {
        NoteKind::Ticket => SeqKind::Ticket,
        NoteKind::Task => SeqKind::Task,
        NoteKind::Slice => SeqKind::Slice,
        NoteKind::Location => SeqKind::Location,
        NoteKind::Round => SeqKind::Rounds,
    }
}

fn pad_id(n: u32) -> String {
    format!("{n:02}")
}

fn parse_stem(base: &str) -> Option<(SeqKind, u32)> {
    let (kind, rest) = if let Some(rest) = base.strip_prefix("ticket-") {
        (SeqKind::Ticket, rest)
    } else if let Some(rest) = base.strip_prefix("task-") {
        (SeqKind::Task, rest)
    } else if let Some(rest) = base.strip_prefix("slice-") {
        (SeqKind::Slice, rest)
    } else if let Some(rest) = base.strip_prefix("location-") {
        (SeqKind::Location, rest)
    } else if let Some(rest) = base.strip_prefix("rounds-") {
        (SeqKind::Rounds, rest)
    } else {
        return None;
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok().map(|n| (kind, n))
}

fn absorb(census: &mut Census, kind: SeqKind, n: u32, live: bool, rel: String) {
    let slot = census.slot(kind);
    if n > slot.high {
        slot.high = n;
    }
    if !live {
        return;
    }
    slot.live.entry(n).or_default().push(rel);
}

fn walk_files(abs_dir: &Path, root: &Path, live: bool, census: &mut Census) {
    let Ok(entries) = fs::read_dir(abs_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk_files(&path, root, live, census);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let base = entry.file_name().to_string_lossy().into_owned();
        let Some((kind, n)) = parse_stem(&base) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| base.clone());
        absorb(census, kind, n, live, rel);
    }
}

fn load_census(start: &Path) -> Result<Census, String> {
    let notes = lookup_notes(start)?;
    let root = hive_root(start)?;
    let mut census = Census::empty();
    walk_files(&notes.planning, &root, true, &mut census);
    if let Some(archive) = &notes.archive {
        walk_files(archive, &root, false, &mut census);
    }
    Ok(census)
}

pub fn next_id(start: &Path, kind: NoteKind) -> Result<String, String> {
    let census = load_census(start)?;
    Ok(pad_id(census.slot_ref(seq_kind(kind)).high + 1))
}

pub fn check_ids(start: &Path) -> Result<Vec<IdCollision>, String> {
    let census = load_census(start)?;
    let mut hits = Vec::new();
    for kind in SEQ_KINDS {
        for (n, live) in &census.slot_ref(kind).live {
            if live.len() > 1 {
                hits.push(IdCollision {
                    kind: kind.name(),
                    padded: pad_id(*n),
                    live: live.clone(),
                });
            }
        }
    }
    Ok(hits)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::cli::{Cli, Commands, NoteCmd};
    use crate::execute;
    use clap::Parser;
    use std::fs;
    use tempfile::tempdir;

    fn write_hive(root: &Path, body: &str) {
        fs::create_dir_all(root.join(".hivemind")).unwrap();
        fs::write(root.join(".hivemind/hivemind.yaml"), body).unwrap();
    }

    fn notes_yaml() -> &'static str {
        "lanes: {}\nnotes:\n  planning: .heio/planning\n  archive: .heio/archive\n"
    }

    fn write_rel(root: &Path, rel: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }

    fn cli(cmd: NoteCmd) -> Cli {
        Cli {
            project: None,
            wt: vec![],
            command: Commands::Note { cmd },
        }
    }

    #[test]
    fn empty_planning_next_id_is_01() {
        let dir = tempdir().unwrap();
        write_hive(dir.path(), notes_yaml());
        fs::create_dir_all(dir.path().join(".heio/planning")).unwrap();
        assert_eq!(next_id(dir.path(), NoteKind::Ticket).unwrap(), "01");
        assert_eq!(next_id(dir.path(), NoteKind::Round).unwrap(), "01");
        assert!(check_ids(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn archive_occupies_and_kinds_do_not_share() {
        let dir = tempdir().unwrap();
        write_hive(dir.path(), notes_yaml());
        write_rel(dir.path(), ".heio/planning/tickets/ticket-02-live.md");
        write_rel(dir.path(), ".heio/planning/tasks/task-80-big.md");
        write_rel(
            dir.path(),
            ".heio/archive/planning/tickets/ticket-04-old.md",
        );
        assert_eq!(next_id(dir.path(), NoteKind::Ticket).unwrap(), "05");
        assert_eq!(next_id(dir.path(), NoteKind::Task).unwrap(), "81");
    }

    #[test]
    fn live_duplicate_stems_fail_check_ids() {
        let dir = tempdir().unwrap();
        write_hive(dir.path(), notes_yaml());
        write_rel(dir.path(), ".heio/planning/tickets/ticket-03-a.md");
        write_rel(dir.path(), ".heio/planning/sprints/x/ticket-03-b.md");
        let hits = check_ids(dir.path()).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, "ticket");
        assert_eq!(hits[0].padded, "03");
        assert_eq!(hits[0].live.len(), 2);
        assert_eq!(execute(cli(NoteCmd::CheckIds), dir.path()), 1);
    }

    #[test]
    fn archive_duplicate_is_not_collision() {
        let dir = tempdir().unwrap();
        write_hive(dir.path(), notes_yaml());
        write_rel(dir.path(), ".heio/planning/tickets/ticket-03-live.md");
        write_rel(
            dir.path(),
            ".heio/archive/planning/tickets/ticket-03-old.md",
        );
        assert!(check_ids(dir.path()).unwrap().is_empty());
        assert_eq!(next_id(dir.path(), NoteKind::Ticket).unwrap(), "04");
    }

    #[test]
    fn missing_notes_planning_fails() {
        let dir = tempdir().unwrap();
        write_hive(dir.path(), "lanes: {}\n");
        let err = next_id(dir.path(), NoteKind::Task).unwrap_err();
        assert!(err.contains("notes.planning"), "{err}");
    }

    #[test]
    fn nested_cwd_uses_notes_planning_tree() {
        let dir = tempdir().unwrap();
        write_hive(dir.path(), notes_yaml());
        write_rel(dir.path(), ".heio/planning/tasks/task-02-live.md");
        write_rel(dir.path(), ".heio/archive/planning/tasks/task-04-old.md");
        let nested = dir.path().join("packages/app");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(next_id(&nested, NoteKind::Task).unwrap(), "05");
        assert_eq!(
            execute(
                cli(NoteCmd::NextId {
                    kind: NoteKind::Task
                }),
                &nested
            ),
            0
        );
    }

    #[test]
    fn round_kind_reads_rounds_stems() {
        let dir = tempdir().unwrap();
        write_hive(dir.path(), notes_yaml());
        write_rel(dir.path(), ".heio/planning/rounds/rounds-07-x.md");
        assert_eq!(next_id(dir.path(), NoteKind::Round).unwrap(), "08");
    }

    #[test]
    pub(crate) fn note_lookup_planning() {
        let dir = tempdir().unwrap();
        write_hive(dir.path(), notes_yaml());
        let body_rel = ".heio/planning/tasks/zxbodyhit.md";
        let stem_rel = ".heio/planning/tasks/zxstemquery.md";
        let body_line = "zxbodyhit on this line";
        let body_path = dir.path().join(body_rel);
        fs::create_dir_all(body_path.parent().unwrap()).unwrap();
        fs::write(&body_path, format!("{body_line}\n")).unwrap();
        fs::write(dir.path().join(stem_rel), "no match in this body\n").unwrap();

        let parsed = Cli::try_parse_from(["bee", "note", "lookup", "zxbodyhit"]);
        assert!(parsed.is_ok());
        let (code, out) = crate::pack::capture_out(|| execute(parsed.unwrap(), dir.path()));
        assert_eq!(code, 0);
        let body_hit = format!("{body_rel}:1:{body_line}");
        assert!(out.contains(&body_hit), "{out}");
        assert!(!out.contains(&format!("{body_rel}:0:")), "{out}");

        let parsed = Cli::try_parse_from(["bee", "note", "lookup", "zxstemquery"]);
        assert!(parsed.is_ok());
        let (code, out) = crate::pack::capture_out(|| execute(parsed.unwrap(), dir.path()));
        assert_eq!(code, 0);
        let stem_hit = format!("{stem_rel}:0:zxstemquery");
        assert!(out.contains(&stem_hit), "{out}");

        let parsed = Cli::try_parse_from(["bee", "note", "lookup", "zxnomatch"]);
        assert!(parsed.is_ok());
        let (code, out) = crate::pack::capture_out(|| execute(parsed.unwrap(), dir.path()));
        assert_eq!(code, 0);
        assert_eq!(out, "");

        assert!(Cli::try_parse_from(["bee", "note", "lookup"]).is_err());

        println!("bee.note:lookup");
    }

    #[test]
    pub(crate) fn note_lookup_scope_regex() {
        let dir = tempdir().unwrap();
        write_hive(dir.path(), notes_yaml());
        let plan_rel = ".heio/planning/tasks/zxplan.md";
        let arch_rel = ".heio/archive/planning/tasks/zxarch.md";
        let stem_rel = ".heio/planning/tasks/zxregexstem.md";
        let plan_line = "zxshared zxplanbody";
        let arch_line = "zxshared zxarchbody";
        let plan_path = dir.path().join(plan_rel);
        fs::create_dir_all(plan_path.parent().unwrap()).unwrap();
        fs::write(&plan_path, format!("{plan_line}\n")).unwrap();
        let arch_path = dir.path().join(arch_rel);
        fs::create_dir_all(arch_path.parent().unwrap()).unwrap();
        fs::write(&arch_path, format!("{arch_line}\n")).unwrap();
        fs::write(dir.path().join(stem_rel), "no match in this body\n").unwrap();
        let plan_hit = format!("{plan_rel}:1:{plan_line}");
        let arch_hit = format!("{arch_rel}:1:{arch_line}");
        let stem_hit = format!("{stem_rel}:0:zxregexstem");

        let parsed_default = Cli::try_parse_from(["bee", "note", "lookup", "zxshared"]);
        assert!(parsed_default.is_ok());
        let (default_code, default_out) =
            crate::pack::capture_out(|| execute(parsed_default.unwrap(), dir.path()));
        assert_eq!(default_code, 0);
        assert!(default_out.contains(&plan_hit), "{default_out}");
        assert!(!default_out.contains(&arch_hit), "{default_out}");

        let parsed_archive =
            Cli::try_parse_from(["bee", "note", "lookup", "--scope", "archive", "zxshared"]);
        assert!(parsed_archive.is_ok());
        let (code, out) = crate::pack::capture_out(|| execute(parsed_archive.unwrap(), dir.path()));
        assert_eq!(code, 0);
        assert!(out.contains(&arch_hit), "{out}");
        assert!(!out.contains(&plan_hit), "{out}");

        let parsed_all =
            Cli::try_parse_from(["bee", "note", "lookup", "--scope", "all", "zxshared"]);
        assert!(parsed_all.is_ok());
        let (code, out) = crate::pack::capture_out(|| execute(parsed_all.unwrap(), dir.path()));
        assert_eq!(code, 0);
        assert!(out.contains(&plan_hit), "{out}");
        assert!(out.contains(&arch_hit), "{out}");

        let parsed_planning =
            Cli::try_parse_from(["bee", "note", "lookup", "--scope", "planning", "zxshared"]);
        assert!(parsed_planning.is_ok());
        let (code, out) =
            crate::pack::capture_out(|| execute(parsed_planning.unwrap(), dir.path()));
        assert_eq!(code, 0);
        assert_eq!(out, default_out);

        let unknown =
            Cli::try_parse_from(["bee", "note", "lookup", "--scope", "other", "zxshared"]);
        let unknown_failed = match unknown {
            Ok(cli) => {
                let (code, _) = crate::pack::capture_out(|| execute(cli, dir.path()));
                code != 0
            }
            Err(_) => true,
        };
        assert!(unknown_failed);

        let parsed_re_body =
            Cli::try_parse_from(["bee", "note", "lookup", "--regex", "zxplanb.dy"]);
        assert!(parsed_re_body.is_ok());
        let (code, out) = crate::pack::capture_out(|| execute(parsed_re_body.unwrap(), dir.path()));
        assert_eq!(code, 0);
        assert!(out.contains(&plan_hit), "{out}");

        let parsed_re_stem =
            Cli::try_parse_from(["bee", "note", "lookup", "--regex", "zxregexst.m"]);
        assert!(parsed_re_stem.is_ok());
        let (code, out) = crate::pack::capture_out(|| execute(parsed_re_stem.unwrap(), dir.path()));
        assert_eq!(code, 0);
        assert!(out.contains(&stem_hit), "{out}");
        assert!(!out.contains(&format!("{stem_rel}:1:")), "{out}");

        let bad = Cli::try_parse_from(["bee", "note", "lookup", "--regex", "["]);
        let bad_failed = match bad {
            Ok(cli) => {
                let (code, _) = crate::pack::capture_out(|| execute(cli, dir.path()));
                code != 0
            }
            Err(_) => true,
        };
        assert!(bad_failed);

        println!("bee.note:lookup-scope");
    }
}
