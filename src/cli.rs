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
    Sync,
    Pin,
    Status,
    Doctor,
    Project,
    Progen,
    Find,
    Context,
    Run,
    Generate,
    Once,
    Watch,
    Explain,
    Gc,
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
}
