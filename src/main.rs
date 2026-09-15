use std::process::ExitCode;

use bee::Cli;
use clap::Parser;

fn main() -> ExitCode {
    let _cli = Cli::parse();
    ExitCode::from(2)
}
