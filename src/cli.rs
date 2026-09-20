use clap::{Parser, Subcommand, ValueEnum};
use odm_core::OdmError;

#[derive(Debug, Parser)]
#[command(name = "bee", version, about = "Rust CLI of the Hive family.")]
pub struct Cli {
    #[arg(long, global = true)]
    pub project: Option<String>,
    #[arg(long, global = true, action = clap::ArgAction::Append)]
    pub wt: Vec<String>,
    #[command(subcommand)]
    pub command: Commands,
}

pub fn resolve_wt_flags(flags: &[String]) -> Result<Option<String>, OdmError> {
    match flags {
        [] => Ok(None),
        [w] => Ok(Some(w.clone())),
        [first, rest @ ..] => {
            if rest.iter().all(|w| w == first) {
                Ok(Some(first.clone()))
            } else {
                Err(OdmError::usage(format!(
                    "conflicting --wt values: {}",
                    flags.join(", ")
                )))
            }
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Init,
    Sync {
        names: Vec<String>,
    },
    Pin {
        #[command(subcommand)]
        cmd: PinCmd,
    },
    Status,
    Doctor,
    Project {
        #[command(subcommand)]
        cmd: ProjectCmd,
    },
    Progen {
        #[command(subcommand)]
        cmd: ProgenCmd,
    },
    Find {
        query: Option<String>,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    Context {
        id: String,
    },
    Run {
        action: Option<String>,
        #[arg(last = true)]
        extra: Vec<String>,
    },
    Generate {
        name: Option<String>,
        #[arg(long, requires = "name")]
        dest: Option<String>,
        #[arg(long, requires = "name")]
        force: bool,
        #[arg(long, requires = "name")]
        dry_run: bool,
    },
    Once,
    Watch {
        #[arg(long)]
        until_quiet: bool,
        #[arg(long)]
        until_target: Option<std::path::PathBuf>,
    },
    Explain,
    Gc,
    Note {
        #[command(subcommand)]
        cmd: NoteCmd,
    },
    Docs {
        #[arg(long, global = true)]
        vault: Option<String>,
        #[command(subcommand)]
        cmd: DocsCmd,
    },
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
        "generate", "once", "watch", "explain", "gc", "note", "docs",
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
