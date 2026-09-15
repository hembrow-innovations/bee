mod cli;
mod init;
mod paths;

use std::path::Path;

pub use cli::{Cli, Commands};
pub use init::init_workbench;
pub use paths::{
    actors_dir, hivemind_dir, lanes_path, pin_path, pin_read_path, workbench_path,
    workbench_read_path,
};

pub fn execute(command: Commands, root: &Path) -> u8 {
    match command {
        Commands::Init => match init_workbench(root) {
            Ok(()) => 0,
            Err(_) => 1,
        },
        _ => 2,
    }
}
