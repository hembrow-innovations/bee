//! Bee is the Rust CLI for a Hive: layout, dest lanes, tracker notes, and doc-store verbs.

mod cli;
mod dest;
mod docs;
mod forward;
mod hive;
mod note;
mod onic;

pub use dest::{lookup_notes, NotesDirs};
pub use forward::{forward_heio_bin, forward_old_bin};
pub use hive::paths;
pub use hive::{project, sync};

use std::path::Path;

pub use cli::{
    resolve_wt_flags, Cli, Commands, DocsCmd, NoteCmd, NoteKind, OnicCmd, PinCmd, ProgenCmd,
    ProjectCmd, WorktreeCmd,
};
pub use hive::{
    actors_dir, hive_root, hivemind_dir, init_workbench, lanes_path, load_workbench,
    parse_workbench_yaml, pin_path, pin_read_path, workbench_path, workbench_read_path,
};
pub use hive_core::{ProjectEntry, Workbench, WorkbenchConfig};

#[cfg(test)]
pub use hive::git_fixture;

fn hive_exit(result: Result<(), hive_core::HiveError>) -> u8 {
    match result {
        Ok(()) => 0,
        Err(e) => hive_core::exit_code(&e) as u8,
    }
}

pub fn execute(cli: Cli, root: &Path) -> u8 {
    let wt = match resolve_wt_flags(&cli.wt) {
        Ok(w) => w,
        Err(e) => return hive_core::exit_code(&e) as u8,
    };
    match cli.command {
        Commands::Init => match init_workbench(root) {
            Ok(()) => 0,
            Err(_) => 1,
        },
        Commands::Sync { names } => hive_exit(hive::sync::sync_workbench(root, &names)),
        Commands::Pin { cmd } => match cmd {
            PinCmd::Record { names, force } => {
                hive_exit(hive::pin::pin_record_primary(root, &names, force))
            }
            PinCmd::Apply { names, force } => {
                hive_exit(hive::pin::pin_apply_primary(root, &names, force))
            }
        },
        Commands::Project { cmd } => match cmd {
            ProjectCmd::Add {
                name,
                path,
                url,
                branch,
                gitlink,
            } => hive_exit(hive::project::add_project(
                root, &name, path, url, branch, gitlink,
            )),
            ProjectCmd::Rm { name } => hive_exit(hive::project::rm_project(root, &name)),
            ProjectCmd::Git { name, git_args } => {
                match hive::project::git_project(root, &name, &git_args, wt.as_deref()) {
                    Ok(status) => status.code().unwrap_or(1) as u8,
                    Err(e) => hive_core::exit_code(&e) as u8,
                }
            }
            ProjectCmd::Worktree { cmd } => match cmd {
                WorktreeCmd::List { project } => {
                    hive_exit(hive::worktree::list_worktrees(root, &project))
                }
                WorktreeCmd::Add {
                    project,
                    slot,
                    branch,
                } => hive_exit(hive::worktree::add_worktree(
                    root,
                    &project,
                    &slot,
                    branch.as_deref(),
                )),
            },
        },
        Commands::Progen { cmd } => match cmd {
            ProgenCmd::Add {
                name,
                path,
                url,
                branch,
                gitlink,
            } => hive_exit(hive::project::add_progen_checkout(
                root, &name, path, url, branch, gitlink,
            )),
        },
        Commands::Doctor => hive_exit(hive::ops::doctor(root)),
        Commands::Status => match hive::ops::status(root) {
            Ok(text) => {
                print!("{text}");
                0
            }
            Err(e) => hive_core::exit_code(&e) as u8,
        },
        Commands::Find { query, limit } => match hive::ops::find(root, query, limit) {
            Ok(text) => {
                print!("{text}");
                0
            }
            Err(e) => hive_core::exit_code(&e) as u8,
        },
        Commands::Context { id } => match hive::ops::context(root, &id) {
            Ok(text) => {
                print!("{text}");
                0
            }
            Err(e) => hive_core::exit_code(&e) as u8,
        },
        Commands::Run { action, extra } => {
            match hive::ops::run(root, action, &extra, cli.project.as_deref(), wt.as_deref()) {
                Ok(code) => code as u8,
                Err(e) => hive_core::exit_code(&e) as u8,
            }
        }
        Commands::Generate {
            name,
            dest,
            force,
            dry_run,
        } => hive_exit(hive::ops::generate(root, name, dest, force, dry_run)),
        Commands::Once { dry_run } => {
            if dry_run {
                match dest::dry_run(root) {
                    Ok(text) => {
                        print!("{text}");
                        0
                    }
                    Err(_) => 1,
                }
            } else {
                match dest::run_once(root) {
                    Ok(code) => code,
                    Err(_) => 1,
                }
            }
        }
        Commands::Watch {
            until_quiet,
            until_target,
            max_spawns,
        } => match dest::run_watch(
            root,
            until_quiet,
            until_target.as_deref(),
            max_spawns,
            std::time::Duration::from_millis(200),
        ) {
            Ok(code) => code,
            Err(_) => 1,
        },
        Commands::Explain => match dest::explain(root) {
            Ok(text) => {
                print!("{text}");
                0
            }
            Err(_) => 1,
        },
        Commands::Gc => match dest::gc(root) {
            Ok(()) => 0,
            Err(_) => 1,
        },
        Commands::Note { cmd } => match cmd {
            NoteCmd::NextId { kind } => match note::next_id(root, kind) {
                Ok(id) => {
                    println!("{id}");
                    0
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            },
            NoteCmd::CheckIds => match note::check_ids(root) {
                Ok(hits) if hits.is_empty() => 0,
                Ok(hits) => {
                    for hit in hits {
                        eprintln!(
                            "duplicate live {}-{}: {}",
                            hit.kind,
                            hit.padded,
                            hit.live.join(" ")
                        );
                    }
                    1
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            },
            NoteCmd::Claim { id } => match note::claim(root, &id, &note::iso_now()) {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            },
            NoteCmd::Status { id, status } => {
                match note::set_status(root, &id, &status, &note::iso_now()) {
                    Ok(_) => 0,
                    Err(e) => {
                        eprintln!("{e}");
                        1
                    }
                }
            }
            NoteCmd::Housekeep => match note::housekeep(root) {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            },
        },
        Commands::Docs { vault, cmd } => match docs::run(root, vault.as_deref(), &cmd) {
            Ok(text) => {
                print!("{text}");
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        Commands::Onic {
            db,
            root: project_root,
            port,
            cmd,
        } => match onic::run(root, db.as_deref(), project_root.as_deref(), port, &cmd) {
            Ok(text) => {
                if !text.is_empty() {
                    println!("{text}");
                }
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
    }
}

#[cfg(test)]
#[test]
fn watch_max_spawns_stops_new_claims() {
    dest::watch::tests::watch_max_spawns_stops_new_claims();
}

#[cfg(test)]
#[test]
fn wt_list_prints_slot_path() {
    hive::worktree::tests::wt_list_prints_slot_path();
}

#[cfg(test)]
#[test]
fn wt_list_non_git_exits_3() {
    hive::worktree::tests::wt_list_non_git_exits_3();
}

#[cfg(test)]
#[test]
fn wt_add_branch_primary_unmoved() {
    hive::worktree::tests::wt_add_branch_primary_unmoved();
}

#[cfg(test)]
#[test]
fn wt_add_refuses_existing_path() {
    hive::worktree::tests::wt_add_refuses_existing_path();
}

#[cfg(test)]
#[test]
fn project_git_wt_missing_slot() {
    hive::project::tests::project_git_wt_missing_slot();
}

#[cfg(test)]
#[test]
fn project_git_wt_skips_pin() {
    hive::project::tests::project_git_wt_skips_pin();
}
