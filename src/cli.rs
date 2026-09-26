use clap::{Parser, Subcommand, ValueEnum};
use hive_core::HiveError;

const SITUATION_HELP: &str = "\
These are common Bee commands used in various situations:

Hive layout:
  init      Create Hive layout on disk
  sync      Fetch remotes without moving HEAD
  pin       Record or apply checkout pins
  status    Show Hive catalog status
  doctor    Check Hive layout without fixing
  project   Add or remove a named project
  progen    Add a nested progen
  find      Search catalog notes
  context   Show note context by id
  run       Run a named action
  generate  Copy a generator template

dest lanes:
  once      Run one dest tick
  watch     Watch dest lanes until stop
  explain   Explain dest skip reasons
  gc        Garbage-collect dest scratch

tracker notes:
  note      Add or inspect tracker notes

docs:
  docs      Read and write the docs vault

graph:
  onic      Map a project into a SQLite graph

See 'bee <command> --help' to read about a specific command.
";

const HELP_TEMPLATE: &str = "\
{about-with-newline}{usage-heading} {usage}

Options:
{options}

{after-help}";

#[derive(Debug, Parser)]
#[command(
    name = "bee",
    version,
    about = "Rust CLI for a Hive.",
    after_help = SITUATION_HELP,
    help_template = HELP_TEMPLATE,
    override_usage = "bee [--project=<PROJECT>] [--wt=<WT>] <command> [<args>]",
    disable_help_subcommand = true
)]
pub struct Cli {
    #[arg(long, global = true, help = "Select a named project")]
    pub project: Option<String>,
    #[arg(long, global = true, action = clap::ArgAction::Append, help = "Select a worktree slot")]
    pub wt: Vec<String>,
    #[command(subcommand)]
    pub command: Commands,
}

pub fn resolve_wt_flags(flags: &[String]) -> Result<Option<String>, HiveError> {
    match flags {
        [] => Ok(None),
        [w] => Ok(Some(w.clone())),
        [first, rest @ ..] => {
            if rest.iter().all(|w| w == first) {
                Ok(Some(first.clone()))
            } else {
                Err(HiveError::usage(format!(
                    "conflicting --wt values: {}",
                    flags.join(", ")
                )))
            }
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    #[command(about = "Create Hive layout on disk")]
    Init,
    #[command(about = "Fetch remotes without moving HEAD")]
    Sync {
        names: Vec<String>,
    },
    #[command(about = "Record or apply checkout pins")]
    Pin {
        #[command(subcommand)]
        cmd: PinCmd,
    },
    #[command(about = "Show Hive catalog status")]
    Status,
    #[command(about = "Check Hive layout without fixing")]
    Doctor,
    #[command(about = "Add or remove a named project")]
    Project {
        #[command(subcommand)]
        cmd: ProjectCmd,
    },
    #[command(about = "Add a nested progen")]
    Progen {
        #[command(subcommand)]
        cmd: ProgenCmd,
    },
    #[command(about = "Search catalog notes")]
    Find {
        query: Option<String>,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    #[command(about = "Show note context by id")]
    Context {
        id: String,
    },
    #[command(about = "Run a named action")]
    Run {
        action: Option<String>,
        #[arg(last = true)]
        extra: Vec<String>,
    },
    #[command(about = "Copy a generator template")]
    Generate {
        name: Option<String>,
        #[arg(long, requires = "name")]
        dest: Option<String>,
        #[arg(long, requires = "name")]
        force: bool,
        #[arg(long, requires = "name")]
        dry_run: bool,
    },
    #[command(about = "Run one dest tick")]
    Once {
        #[arg(long)]
        dry_run: bool,
    },
    #[command(about = "Watch dest lanes until stop")]
    Watch {
        #[arg(long)]
        until_quiet: bool,
        #[arg(long)]
        until_target: Option<std::path::PathBuf>,
        #[arg(long)]
        max_spawns: Option<u64>,
    },
    #[command(about = "Explain dest skip reasons")]
    Explain,
    #[command(about = "Garbage-collect dest scratch")]
    Gc,
    #[command(about = "Add or inspect tracker notes")]
    Note {
        #[command(subcommand)]
        cmd: NoteCmd,
    },
    #[command(about = "Read and write the docs vault")]
    Docs {
        #[arg(long, global = true)]
        vault: Option<String>,
        #[command(subcommand)]
        cmd: DocsCmd,
    },
    #[command(about = "Map a project into a SQLite graph")]
    Onic {
        #[arg(long, global = true)]
        db: Option<std::path::PathBuf>,
        #[arg(long, global = true)]
        root: Option<std::path::PathBuf>,
        #[arg(long, global = true)]
        port: Option<u16>,
        #[command(subcommand)]
        cmd: OnicCmd,
    },
}

#[derive(Debug, Subcommand)]
pub enum OnicCmd {
    #[command(about = "Write .onic/graph.db")]
    Build {
        dir: Option<std::path::PathBuf>,
        #[arg(long)]
        watch: bool,
    },
    #[command(about = "Print graph schema text")]
    Schema,
    #[command(about = "Run one read-only SQL statement")]
    Sql { query: String },
    #[command(about = "Search nodes by name")]
    Search { text: String },
    #[command(about = "Explain one node")]
    Explain { name: String },
    #[command(about = "Compact one node")]
    Compact { name: String },
    #[command(about = "List neighbors of one node")]
    Neighbors { name: String },
    #[command(about = "Find a path between two nodes")]
    Path { from: String, to: String },
    #[command(about = "Serve the local viewer")]
    Serve,
}

#[derive(Debug, Subcommand)]
pub enum NoteCmd {
    NextId { kind: NoteKind },
    CheckIds,
    Claim { id: String },
    Status { id: String, status: String },
    Housekeep,
}

#[derive(Debug, Subcommand)]
pub enum DocsCmd {
    Home,
    Ls {
        dir: Option<String>,
        #[arg(short = 'r', long)]
        recursive: bool,
        #[arg(long, default_value = "path")]
        sort: String,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        ext: Option<String>,
        #[arg(long)]
        fields: Option<String>,
    },
    Read {
        #[arg(required = true)]
        paths: Vec<String>,
        #[arg(long)]
        full: bool,
        #[arg(long)]
        metadata: bool,
    },
    Search {
        query: Vec<String>,
        #[arg(long)]
        regex: bool,
        #[arg(long)]
        case_sensitive: bool,
        #[arg(long)]
        context: Option<usize>,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        frontmatter: Vec<String>,
        #[arg(long)]
        modified_since: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
    Recent {
        #[arg(long)]
        days: Option<u64>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        fields: Option<String>,
    },
    Write {
        path: String,
        #[arg(long)]
        content: Option<String>,
        #[arg(long)]
        content_file: Option<String>,
        #[arg(long)]
        if_absent: bool,
    },
    Append {
        path: String,
        #[arg(long)]
        content: Option<String>,
        #[arg(long)]
        content_file: Option<String>,
        #[arg(long)]
        if_missing: bool,
    },
    Patch {
        path: String,
        #[arg(long)]
        target_type: Option<String>,
        #[arg(long, action = clap::ArgAction::Append)]
        target: Vec<String>,
        #[arg(long)]
        op: Option<String>,
        #[arg(long)]
        content: Option<String>,
        #[arg(long)]
        content_file: Option<String>,
    },
    Rm {
        path: String,
        #[arg(long)]
        permanent: bool,
    },
    Mv {
        from: String,
        to: String,
        #[arg(long)]
        overwrite: bool,
    },
    Links {
        path: Option<String>,
        #[arg(long)]
        broken: bool,
        #[arg(long)]
        orphans: bool,
        #[arg(long)]
        outlinks: bool,
        #[arg(long)]
        backlinks: bool,
    },
    Tags {
        #[arg(long)]
        limit: Option<usize>,
        #[arg(num_args = 0..=2)]
        spec: Vec<String>,
    },
    Vault {
        info: Option<String>,
    },
    New {
        kind: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        domain: Option<String>,
        #[arg(long)]
        area: Option<String>,
        #[arg(long)]
        slug: Option<String>,
        #[arg(long)]
        tag: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum NoteKind {
    Ticket,
    Task,
    Slice,
    Location,
    #[value(alias = "rounds")]
    Round,
}

#[derive(Debug, Subcommand)]
pub enum ProjectCmd {
    Add {
        name: String,
        #[arg(long)]
        path: String,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long, requires = "url")]
        gitlink: bool,
    },
    Rm {
        name: String,
    },
    #[command(about = "Worktree slots")]
    Worktree {
        #[command(subcommand)]
        cmd: WorktreeCmd,
    },
}

#[derive(Debug, Subcommand)]
pub enum WorktreeCmd {
    #[command(about = "Print registered slots for a project")]
    List {
        project: String,
    },
    #[command(about = "Add a worktree slot")]
    Add {
        project: String,
        slot: String,
        #[arg(long)]
        branch: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProgenCmd {
    Add {
        name: String,
        #[arg(long)]
        path: String,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long, requires = "url")]
        gitlink: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum PinCmd {
    Record {
        names: Vec<String>,
        #[arg(long)]
        force: bool,
    },
    Apply {
        names: Vec<String>,
        #[arg(long)]
        force: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};

    const UNION: &[&str] = &[
        "init", "sync", "pin", "status", "doctor", "project", "progen", "find", "context", "run",
        "generate", "once", "watch", "explain", "gc", "note", "docs", "onic",
    ];

    fn help_text() -> String {
        Cli::command().render_help().to_string()
    }

    fn subcommand_names() -> Vec<String> {
        Cli::command()
            .get_subcommands()
            .map(|c| c.get_name().to_string())
            .collect()
    }

    #[test]
    fn help_lists_every_union_verb() {
        let help = help_text();
        for verb in UNION {
            assert!(help.contains(verb), "missing {verb} in {help}");
        }
    }

    #[test]
    fn help_has_git_style_about_lines() {
        let help = help_text();
        assert!(help.contains("Show Hive catalog status"), "{help}");
        assert!(help.contains("Run one dest tick"), "{help}");
        assert!(help.contains("Watch dest lanes until stop"), "{help}");
        assert!(help.contains("Create Hive layout on disk"), "{help}");
        assert!(help.contains("Add or inspect tracker notes"), "{help}");
        assert!(help.contains("Read and write the docs vault"), "{help}");
        assert!(!help.contains("verbs"), "{help}");
    }

    fn situation_rows<'a>(help: &'a str, heading: &str) -> Vec<&'a str> {
        let mut iter = help.lines();
        assert!(
            iter.any(|line| {
                line.trim()
                    .trim_end_matches(':')
                    .eq_ignore_ascii_case(heading)
            }),
            "missing situation heading {heading} in {help}"
        );
        iter.take_while(|line| {
            let t = line.trim();
            t.is_empty() || line.starts_with(' ') || line.starts_with('\t')
        })
        .filter(|line| !line.trim().is_empty())
        .collect()
    }

    fn is_verb_about_row(line: &str, verb: &str, about: &str) -> bool {
        let t = line.trim();
        let Some(rest) = t.strip_prefix(verb) else {
            return false;
        };
        rest.starts_with(char::is_whitespace) && rest.contains(about) && !t.contains(',')
    }

    #[test]
    fn help_groups_porcelain_by_situation() {
        let help = help_text();
        assert!(
            !help.lines().any(|line| line.trim() == "Commands:"),
            "{help}"
        );
        assert!(!help.contains("init, sync"), "{help}");
        for heading in ["Hive layout", "dest lanes", "tracker notes", "docs"] {
            let found = help.lines().any(|line| {
                line.trim()
                    .trim_end_matches(':')
                    .eq_ignore_ascii_case(heading)
            });
            assert!(found, "missing situation heading {heading} in {help}");
        }
        let layout = situation_rows(&help, "Hive layout");
        assert!(
            layout
                .iter()
                .any(|l| is_verb_about_row(l, "init", "Create Hive layout on disk")),
            "{help}"
        );
        assert!(
            layout
                .iter()
                .any(|l| is_verb_about_row(l, "status", "Show Hive catalog status")),
            "{help}"
        );
        let lanes = situation_rows(&help, "dest lanes");
        assert!(
            lanes
                .iter()
                .any(|l| is_verb_about_row(l, "once", "Run one dest tick")),
            "{help}"
        );
        let notes = situation_rows(&help, "tracker notes");
        assert!(
            notes
                .iter()
                .any(|l| is_verb_about_row(l, "note", "Add or inspect tracker notes")),
            "{help}"
        );
        let docs = situation_rows(&help, "docs");
        assert!(
            docs.iter()
                .any(|l| is_verb_about_row(l, "docs", "Read and write the docs vault")),
            "{help}"
        );
        for row in &docs {
            assert!(
                is_verb_about_row(row, "docs", "Read and write the docs vault"),
                "docs group must be verb-plus-about only: {row} in {help}"
            );
            for flag in ["--project", "--wt", "-h", "-V"] {
                assert!(!row.contains(flag), "{flag} under docs in {help}");
            }
        }
        let options_at = help.lines().position(|line| {
            let trimmed = line.trim();
            line == trimmed
                && trimmed
                    .trim_end_matches(':')
                    .eq_ignore_ascii_case("Options")
        });
        let hive_at = help.lines().position(|line| {
            line.trim()
                .trim_end_matches(':')
                .eq_ignore_ascii_case("Hive layout")
        });
        assert!(
            options_at.is_some() && hive_at.is_some() && options_at < hive_at,
            "Options must be its own block before situation groups: {help}"
        );
        for row in layout
            .iter()
            .chain(&lanes)
            .chain(&notes)
            .chain(&docs)
        {
            assert!(!row.contains(','), "csv dump in situation rows: {help}");
        }
    }

    #[test]
    fn help_does_not_lead_with_flat_commands_list() {
        let help = help_text();
        assert!(
            !help.lines().any(|line| line.trim() == "Commands:"),
            "{help}"
        );
        let hive = help
            .lines()
            .position(|line| {
                line.trim()
                    .trim_end_matches(':')
                    .eq_ignore_ascii_case("Hive layout")
            })
            .expect(&help);
        let usage = help
            .lines()
            .position(|line| line.trim().starts_with("Usage:"))
            .expect(&help);
        assert!(usage < hive, "{help}");
    }

    #[test]
    fn project_rm_matches_queen_argv() {
        let cli = Cli::try_parse_from(["bee", "project", "rm", "alpha"]).unwrap();
        match cli.command {
            Commands::Project {
                cmd: ProjectCmd::Rm { name },
            } => assert_eq!(name, "alpha"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn help_has_no_dest_or_workspace_subcommand() {
        let names = subcommand_names();
        assert!(
            !names.iter().any(|n| n == "dest" || n == "workspace"),
            "{names:?}"
        );
    }

    #[test]
    fn help_has_one_status_verb() {
        let count = subcommand_names()
            .iter()
            .filter(|n| n.as_str() == "status")
            .count();
        assert_eq!(count, 1, "{:?}", subcommand_names());
    }

    #[test]
    fn help_binds_project_and_wt() {
        let help = help_text();
        assert!(help.contains("--project"), "{help}");
        assert!(help.contains("--wt"), "{help}");
    }

    #[test]
    fn help_usage_lists_project_and_wt_like_git() {
        let help = help_text();
        let usage = help
            .lines()
            .find(|line| line.trim().starts_with("Usage:"))
            .expect(&help);
        assert!(usage.contains("--project"), "{help}");
        assert!(usage.contains("--wt"), "{help}");
        assert!(!usage.contains("[OPTIONS]"), "{help}");
    }

    #[test]
    fn help_has_situation_intro_before_grouped_verbs() {
        let help = help_text();
        let intro = help.lines().position(|line| {
            line.contains("common Bee commands used in various situations")
        });
        let usage = help
            .lines()
            .position(|line| line.trim().starts_with("Usage:"));
        let hive = help.lines().position(|line| {
            line.trim()
                .trim_end_matches(':')
                .eq_ignore_ascii_case("Hive layout")
        });
        assert!(
            intro.is_some()
                && usage.is_some()
                && hive.is_some()
                && usage.unwrap() < intro.unwrap()
                && intro.unwrap() < hive.unwrap(),
            "{help}"
        );
    }

    #[test]
    fn help_has_command_help_footer() {
        let help = help_text();
        let foot = help.lines().position(|line| {
            line.contains("bee <command> --help") || line.contains("bee help <command>")
        });
        let hive = help.lines().position(|line| {
            line.trim()
                .trim_end_matches(':')
                .eq_ignore_ascii_case("Hive layout")
        });
        assert!(
            foot.is_some() && hive.is_some() && foot.unwrap() > hive.unwrap(),
            "{help}"
        );
    }

    #[test]
    fn help_project_and_wt_have_nonempty_abouts() {
        let help = help_text();
        for flag in ["--project", "--wt"] {
            let row = help
                .lines()
                .find(|line| line.contains(flag))
                .unwrap_or_else(|| panic!("missing {flag} in {help}"));
            let after = row
                .splitn(2, flag)
                .nth(1)
                .unwrap_or("")
                .trim_start_matches(|c: char| {
                    c.is_whitespace()
                        || c == '<'
                        || c == '>'
                        || c.is_ascii_uppercase()
                        || c == '_'
                })
                .trim();
            assert!(!after.is_empty(), "{flag} about empty in {row} of {help}");
        }
    }

    #[test]
    fn status_takes_no_id_args() {
        let status = Cli::command().find_subcommand("status").unwrap().clone();
        assert!(status.get_positionals().next().is_none());
    }

    #[test]
    fn note_help_lists_next_id_and_check_ids() {
        let note = Cli::command().find_subcommand("note").unwrap().clone();
        let names: Vec<String> = note
            .get_subcommands()
            .map(|c| c.get_name().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "next-id"), "{names:?}");
        assert!(names.iter().any(|n| n == "check-ids"), "{names:?}");
    }

    #[test]
    fn note_help_lists_claim_and_status() {
        let note = Cli::command().find_subcommand("note").unwrap().clone();
        let names: Vec<String> = note
            .get_subcommands()
            .map(|c| c.get_name().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "claim"), "{names:?}");
        assert!(names.iter().any(|n| n == "status"), "{names:?}");
    }

    #[test]
    fn note_help_lists_housekeep() {
        let note = Cli::command().find_subcommand("note").unwrap().clone();
        let names: Vec<String> = note
            .get_subcommands()
            .map(|c| c.get_name().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "housekeep"), "{names:?}");
        assert!(Cli::try_parse_from(["bee", "note", "housekeep"]).is_ok());
    }

    #[test]
    fn status_rejects_id_args() {
        assert!(Cli::try_parse_from(["bee", "status", "task-1", "claimed"]).is_err());
    }

    #[test]
    fn docs_help_lists_read_verbs() {
        let docs = Cli::command().find_subcommand("docs").unwrap().clone();
        let names: Vec<String> = docs
            .get_subcommands()
            .map(|c| c.get_name().to_string())
            .collect();
        for verb in ["home", "ls", "read", "search", "recent"] {
            assert!(
                names.iter().any(|n| n == verb),
                "missing {verb} in {names:?}"
            );
        }
        assert!(Cli::try_parse_from(["bee", "docs", "home"]).is_ok());
        assert!(Cli::try_parse_from(["bee", "docs", "search", "q", "--vault", "docs"]).is_ok());
    }

    #[test]
    fn docs_help_lists_write_verbs() {
        let docs = Cli::command().find_subcommand("docs").unwrap().clone();
        let names: Vec<String> = docs
            .get_subcommands()
            .map(|c| c.get_name().to_string())
            .collect();
        for verb in ["write", "append", "patch", "rm", "mv"] {
            assert!(
                names.iter().any(|n| n == verb),
                "missing {verb} in {names:?}"
            );
        }
        assert!(Cli::try_parse_from(["bee", "docs", "write", "a.md", "--content", "x"]).is_ok());
        assert!(Cli::try_parse_from(["bee", "docs", "append", "a.md", "--content", "x"]).is_ok());
        assert!(Cli::try_parse_from([
            "bee",
            "docs",
            "patch",
            "a.md",
            "--target-type",
            "heading",
            "--target",
            "T",
            "--content",
            "x"
        ])
        .is_ok());
        assert!(Cli::try_parse_from(["bee", "docs", "rm", "a.md"]).is_ok());
        assert!(Cli::try_parse_from(["bee", "docs", "mv", "a.md", "b.md"]).is_ok());
        assert!(Cli::try_parse_from(["bee", "docs", "write", "a.md", "--vault", "docs"]).is_ok());
    }

    #[test]
    fn docs_help_lists_graph_new_verbs() {
        let docs = Cli::command().find_subcommand("docs").unwrap().clone();
        let names: Vec<String> = docs
            .get_subcommands()
            .map(|c| c.get_name().to_string())
            .collect();
        for verb in ["links", "tags", "vault", "new"] {
            assert!(
                names.iter().any(|n| n == verb),
                "missing {verb} in {names:?}"
            );
        }
        assert!(Cli::try_parse_from(["bee", "docs", "links", "a.md"]).is_ok());
        assert!(Cli::try_parse_from(["bee", "docs", "links", "--broken"]).is_ok());
        assert!(Cli::try_parse_from(["bee", "docs", "tags"]).is_ok());
        assert!(Cli::try_parse_from(["bee", "docs", "tags", "files", "stack"]).is_ok());
        assert!(Cli::try_parse_from(["bee", "docs", "vault"]).is_ok());
        assert!(Cli::try_parse_from([
            "bee",
            "docs",
            "new",
            "adr",
            "--title",
            "Pick",
            "--domain",
            "hive",
            "--area",
            "decisions",
            "--slug",
            "pick"
        ])
        .is_ok());
        assert!(Cli::try_parse_from(["bee", "docs", "links", "a.md", "--vault", "docs"]).is_ok());
    }

    #[test]
    fn find_stays_catalog_when_docs_exists() {
        assert!(
            subcommand_names().iter().any(|n| n == "find"),
            "{:?}",
            subcommand_names()
        );
        let cli = Cli::try_parse_from(["bee", "find", "q"]).unwrap();
        match cli.command {
            Commands::Find { query, .. } => assert_eq!(query.as_deref(), Some("q")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn note_status_takes_id_and_token() {
        let cli = Cli::try_parse_from(["bee", "note", "status", "task-1", "claimed"]).unwrap();
        match cli.command {
            Commands::Note {
                cmd: NoteCmd::Status { id, status },
            } => {
                assert_eq!(id, "task-1");
                assert_eq!(status, "claimed");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn init_verb_writes_workbench_yaml() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            crate::execute(
                Cli {
                    project: None,
                    wt: vec![],
                    command: Commands::Init,
                },
                dir.path()
            ),
            0
        );
        assert!(crate::workbench_path(dir.path()).is_file());
    }
}
