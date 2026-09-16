mod cli;
mod dest;
mod dest_config;
mod dest_match;
mod dest_note;
mod dest_once;
mod dest_scan;
mod dest_spawn;
#[cfg(test)]
mod git_fixture;
mod init;
mod layout;
mod ops;
mod paths;
mod pin;
mod project;
mod sync;

use std::path::Path;

pub use cli::{resolve_wt_flags, Cli, Commands, PinCmd, ProgenCmd, ProjectCmd};
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

pub fn execute(cli: Cli, root: &Path) -> u8 {
    let wt = match resolve_wt_flags(&cli.wt) {
        Ok(w) => w,
        Err(e) => return odm_core::exit_code(&e) as u8,
    };
    match cli.command {
        Commands::Init => match init_workbench(root) {
            Ok(()) => 0,
            Err(_) => 1,
        },
        Commands::Sync { names } => odm_exit(sync::sync_workbench(root, &names)),
        Commands::Pin { cmd } => match cmd {
            PinCmd::Record { names, force } => {
                odm_exit(pin::pin_record_primary(root, &names, force))
            }
            PinCmd::Apply { names, force } => {
                odm_exit(pin::pin_apply_primary(root, &names, force))
            }
        },
        Commands::Project { cmd } => match cmd {
            ProjectCmd::Add {
                name,
                path,
                url,
                branch,
                gitlink,
            } => odm_exit(project::add_project(root, &name, path, url, branch, gitlink)),
        },
        Commands::Progen { cmd } => match cmd {
            ProgenCmd::Add {
                name,
                path,
                url,
                branch,
                gitlink,
            } => odm_exit(project::add_progen_checkout(
                root, &name, path, url, branch, gitlink,
            )),
        },
        Commands::Doctor => odm_exit(ops::doctor(root)),
        Commands::Status => match ops::status(root) {
            Ok(text) => {
                print!("{text}");
                0
            }
            Err(e) => odm_core::exit_code(&e) as u8,
        },
        Commands::Find { query, limit } => match ops::find(root, query, limit) {
            Ok(text) => {
                print!("{text}");
                0
            }
            Err(e) => odm_core::exit_code(&e) as u8,
        },
        Commands::Context { id } => match ops::context(root, &id) {
            Ok(text) => {
                print!("{text}");
                0
            }
            Err(e) => odm_core::exit_code(&e) as u8,
        },
        Commands::Run { action, extra } => match ops::run(
            root,
            action,
            &extra,
            cli.project.as_deref(),
            wt.as_deref(),
        ) {
            Ok(code) => code as u8,
            Err(e) => odm_core::exit_code(&e) as u8,
        },
        Commands::Generate {
            name,
            dest,
            force,
            dry_run,
        } => odm_exit(ops::generate(root, name, dest, force, dry_run)),
        Commands::Once => match dest_once::run_once(root) {
            Ok(code) => code,
            Err(_) => 1,
        },
        _ => 2,
    }
}
