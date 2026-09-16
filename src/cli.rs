use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "bee", version, about = "Rust CLI of the Hive family.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
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
    Find,
    Context,
    Run,
    Generate,
    Once,
    Watch,
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
    },
}

#[derive(Debug, Subcommand)]
pub enum PinCmd {
    Record {
        names: Vec<String>,
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
    fn init_verb_writes_workbench_yaml() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(crate::execute(Commands::Init, dir.path()), 0);
        assert!(crate::workbench_path(dir.path()).is_file());
    }

    #[test]
    fn other_verbs_still_exit_two() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(crate::execute(Commands::Status, dir.path()), 2);
    }
}
