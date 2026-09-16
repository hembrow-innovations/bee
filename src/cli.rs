use clap::{Parser, Subcommand};
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
    use clap::CommandFactory;

    const UNION: &[&str] = &[
        "init", "sync", "pin", "status", "doctor", "project", "progen", "find", "context", "run",
        "generate", "once", "watch", "explain", "gc",
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
