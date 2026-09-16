use std::process::ExitCode;

use bee::{execute, Cli};
use clap::Parser;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match std::env::current_dir() {
        Ok(root) => ExitCode::from(execute(cli, &root)),
        Err(_) => ExitCode::from(1),
    }
}
