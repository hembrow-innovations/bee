mod cli;
#[cfg(test)]
mod git_fixture;
mod init;
mod layout;
mod paths;
mod pin;
mod sync;

use std::path::Path;

pub use cli::{Cli, Commands, PinCmd};
pub use init::init_workbench;
pub use layout::{load_workbench, parse_workbench_yaml};
pub use odm_core::{ProjectEntry, Workbench, WorkbenchConfig};
pub use paths::{
    actors_dir, hivemind_dir, lanes_path, pin_path, pin_read_path, workbench_path,
    workbench_read_path,
};

fn odm_exit(result: Result<(), odm_core::OdmError>) -> u8 {
    match result {
        Ok(()) => 0,
        Err(e) => odm_core::exit_code(&e) as u8,
    }
}

pub fn execute(command: Commands, root: &Path) -> u8 {
    match command {
        Commands::Init => match init_workbench(root) {
            Ok(()) => 0,
            Err(_) => 1,
        },
        Commands::Sync { names } => odm_exit(sync::sync_workbench(root, &names)),
        Commands::Pin { cmd } => match cmd {
            PinCmd::Record { names } => odm_exit(pin::pin_record_primary(root, &names)),
            PinCmd::Apply { names, force } => {
                odm_exit(pin::pin_apply_primary(root, &names, force))
            }
        },
        _ => 2,
    }
}
