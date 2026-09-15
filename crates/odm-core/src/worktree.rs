//! Worktree slot lifecycle — list/add/rm/prune under `worktrees/<project>/<slot>/`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use odm_git::Git;

use crate::config::Workspace;
use crate::error::OdmError;
use crate::paths::{abs_checkout, worktree_slot_path};

/// One slot after add or rm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeSlotOutcome {
    pub project: String,
    pub slot: String,
    /// Relative to workspace root: `worktrees/<project>/<slot>`.
    pub path: String,
}

/// One slot row for list / status / project info.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WorktreeSlotInfo {
    pub name: String,
    /// Relative to workspace root: `worktrees/<project>/<slot>`.
    pub path: String,
    /// `Some(true)` dirty, `Some(false)` clean, `None` if cleanliness probe failed.
    pub dirty: Option<bool>,
}

/// Orphan slot directory under `worktrees/<project>/` (not a registered git worktree).
///
/// No `dirty` — orphans are not registered worktrees.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WorktreeOrphanInfo {
    pub name: String,
    /// Relative to workspace root: `worktrees/<project>/<slot>`.
    pub path: String,
}

/// List outcome for a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeListOutcome {
    pub project: String,
    pub slots: Vec<WorktreeSlotInfo>,
}

/// One pruned/skipped orphan row (name + path only — no dirty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreePruneSlot {
    pub name: String,
    /// Relative to workspace root: `worktrees/<project>/<slot>`.
    pub path: String,
}

/// Outcome of pruning orphan slot directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreePruneOutcome {
    pub project: String,
    /// Orphans successfully removed.
    pub pruned: Vec<WorktreePruneSlot>,
    /// Non-empty orphans left when `force` was false (caller should exit 3 if non-empty).
    pub skipped_nonempty: Vec<WorktreePruneSlot>,
}

/// One pruned/skipped slot under `worktree_prune_all` (includes project).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreePruneAllSlot {
    pub project: String,
    pub name: String,
    pub path: String,
}

/// Aggregate outcome of pruning orphans across all configured projects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreePruneAllOutcome {
    pub pruned: Vec<WorktreePruneAllSlot>,
    pub skipped_nonempty: Vec<WorktreePruneAllSlot>,
}

/// Validate a worktree slot name; return trimmed name.
///
/// Rejects empty, `.` / `..`, path separators, NUL, and absolute-looking names.
pub fn validate_slot_name(name: &str) -> Result<String, OdmError> {
    use crate::paths::parse_path_token;
    match parse_path_token(name) {
        Ok(t) => Ok(t.to_string()),
        Err("empty") => Err(OdmError::usage("worktree slot name must not be empty")),
        Err("dot") => Err(OdmError::usage(format!(
            "invalid worktree slot name '{}'",
            name.trim()
        ))),
        Err("separator") => Err(OdmError::usage(format!(
            "invalid worktree slot name '{}': path separators not allowed",
            name.trim()
        ))),
        Err(_) => Err(OdmError::usage(format!(
            "invalid worktree slot name '{}': must be a simple name",
            name.trim()
        ))),
    }
}

/// List git worktree slots under `worktrees/<project>/` (sorted by name).
///
/// Each slot's `dirty` is probed via `git.is_clean` (soft: probe err → `None`).
pub fn worktree_list<R: odm_git::CommandRunner>(
    git: &Git<R>,
    ws: &Workspace,
    project: &str,
) -> Result<WorktreeListOutcome, OdmError> {
    let mut slots = list_registered_slots(git, ws, project)?;
    for slot in &mut slots {
        let abs = worktree_slot_path(&ws.root, project, &slot.name);
        slot.dirty = git.is_clean(&abs).ok().map(|c| !c);
    }
    Ok(WorktreeListOutcome {
        project: project.to_string(),
        slots,
    })
}

/// Registered slot names under `worktrees/<project>/` without dirty probes.
pub fn worktree_registered_names<R: odm_git::CommandRunner>(
    git: &Git<R>,
    ws: &Workspace,
    project: &str,
) -> Result<BTreeSet<String>, OdmError> {
    let slots = list_registered_slots(git, ws, project)?;
    Ok(slots.into_iter().map(|s| s.name).collect())
}

fn list_registered_slots<R: odm_git::CommandRunner>(
    git: &Git<R>,
    ws: &Workspace,
    project: &str,
) -> Result<Vec<WorktreeSlotInfo>, OdmError> {
    let primary = resolve_primary_git(git, ws, project)?;
    let prefix = ws.root.join("worktrees").join(project);
    let entries = git.worktree_list(&primary)?;
    let mut slots: Vec<WorktreeSlotInfo> = entries
        .into_iter()
        .filter_map(|e| slot_info_under_prefix(&prefix, project, &e.path))
        .collect();
    slots.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(slots)
}

/// Add a worktree slot at `worktrees/<project>/<slot>/`.
pub fn worktree_add<R: odm_git::CommandRunner>(
    git: &Git<R>,
    ws: &Workspace,
    project: &str,
    slot: &str,
    branch: Option<&str>,
) -> Result<WorktreeSlotOutcome, OdmError> {
    let slot = validate_slot_name(slot)?;
    let primary = resolve_primary_git(git, ws, project)?;
    let slot_path = worktree_slot_path(&ws.root, project, &slot);
    if slot_path.exists() {
        return Err(OdmError::operation(format!(
            "worktree slot path already exists: {}",
            rel_slot_path(project, &slot)
        )));
    }
    if let Some(parent) = slot_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            OdmError::operation(format!(
                "failed to create worktrees parent {}: {e}",
                parent.display()
            ))
        })?;
    }
    git.worktree_add(&primary, &slot_path, branch)?;
    Ok(WorktreeSlotOutcome {
        project: project.to_string(),
        slot: slot.clone(),
        path: rel_slot_path(project, &slot),
    })
}

/// Remove a worktree slot.
pub fn worktree_rm<R: odm_git::CommandRunner>(
    git: &Git<R>,
    ws: &Workspace,
    project: &str,
    slot: &str,
    force: bool,
) -> Result<WorktreeSlotOutcome, OdmError> {
    let slot = validate_slot_name(slot)?;
    let primary = resolve_primary_git(git, ws, project)?;
    let slot_path = worktree_slot_path(&ws.root, project, &slot);
    if !slot_path.exists() {
        return Err(OdmError::not_found(format!(
            "worktree slot not found: {}",
            rel_slot_path(project, &slot)
        )));
    }
    let entries = git.worktree_list(&primary)?;
    let registered = entries.iter().any(|e| paths_equal(&e.path, &slot_path));
    if !registered {
        return Err(OdmError::operation(format!(
            "path exists but is not a registered git worktree: {}",
            rel_slot_path(project, &slot)
        )));
    }
    git.worktree_remove(&primary, &slot_path, force)?;
    // Best-effort: remove empty worktrees/<project>/ directory.
    let project_wt = ws.root.join("worktrees").join(project);
    let _ = fs::remove_dir(&project_wt);
    Ok(WorktreeSlotOutcome {
        project: project.to_string(),
        slot: slot.clone(),
        path: rel_slot_path(project, &slot),
    })
}

/// Remove orphan slot directories under `worktrees/<project>/`.
///
/// Orphan = valid slot-name directory not in the registered `worktree_list` set
/// (same definition as doctor). Default removes empty dirs only (`remove_dir`);
/// `--force` uses `remove_dir_all`. Never deletes registered worktree paths.
/// Partial progress: empty orphans are removed even if non-empty ones remain
/// (then `skipped_nonempty` is non-empty for exit 3).
pub fn worktree_prune<R: odm_git::CommandRunner>(
    git: &Git<R>,
    ws: &Workspace,
    project: &str,
    force: bool,
) -> Result<WorktreePruneOutcome, OdmError> {
    let registered = worktree_registered_names(git, ws, project)?;
    let orphans = orphan_slot_names(ws, project, &registered);

    let mut pruned = Vec::new();
    let mut skipped_nonempty = Vec::new();

    for slot in orphans {
        let abs = worktree_slot_path(&ws.root, project, &slot);
        let info = WorktreePruneSlot {
            name: slot.clone(),
            path: rel_slot_path(project, &slot),
        };
        if force {
            fs::remove_dir_all(&abs).map_err(|e| {
                OdmError::operation(format!(
                    "failed to remove orphan worktree dir {}: {e}",
                    info.path
                ))
            })?;
            pruned.push(info);
        } else if fs::remove_dir(&abs).is_ok() {
            pruned.push(info);
        } else {
            skipped_nonempty.push(info);
        }
    }

    // Best-effort: remove empty worktrees/<project>/ directory.
    let project_wt = ws.root.join("worktrees").join(project);
    let _ = fs::remove_dir(&project_wt);

    Ok(WorktreePruneOutcome {
        project: project.to_string(),
        pruned,
        skipped_nonempty,
    })
}

/// Prune orphans for every configured project (sorted name order).
///
/// Same empty/`force` rules as [`worktree_prune`]. Missing primary, non-git, or
/// list failures skip that project (no failure row). Aggregates pruned and
/// skipped_nonempty across projects.
pub fn worktree_prune_all<R: odm_git::CommandRunner>(
    git: &Git<R>,
    ws: &Workspace,
    force: bool,
) -> Result<WorktreePruneAllOutcome, OdmError> {
    let mut pruned = Vec::new();
    let mut skipped_nonempty = Vec::new();
    // BTreeMap keys iterate in sorted order.
    for project in ws.config.projects.keys() {
        match worktree_prune(git, ws, project, force) {
            Ok(out) => {
                for s in out.pruned {
                    pruned.push(WorktreePruneAllSlot {
                        project: out.project.clone(),
                        name: s.name,
                        path: s.path,
                    });
                }
                for s in out.skipped_nonempty {
                    skipped_nonempty.push(WorktreePruneAllSlot {
                        project: out.project.clone(),
                        name: s.name,
                        path: s.path,
                    });
                }
            }
            // Soft-skip: missing primary, non-git, list fail (doctor spirit).
            Err(OdmError::NotFound(_)) | Err(OdmError::Operation(_)) => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(WorktreePruneAllOutcome {
        pruned,
        skipped_nonempty,
    })
}

/// List orphan slot dirs under `worktrees/<project>/` (sorted by name).
///
/// Orphan = valid slot-name directory not in the registered `worktree_list` set
/// (same definition as doctor/prune). List/primary errors propagate; callers that
/// soft-fail should map `Err` to `[]`.
pub fn worktree_orphans<R: odm_git::CommandRunner>(
    git: &Git<R>,
    ws: &Workspace,
    project: &str,
) -> Result<Vec<WorktreeOrphanInfo>, OdmError> {
    let registered = worktree_registered_names(git, ws, project)?;
    Ok(worktree_orphan_infos(ws, project, &registered))
}

/// Orphan infos from disk vs a precomputed registered-name set (sorted by name).
///
/// Missing `worktrees/<project>/`, unreadable dir, files, and invalid names → ignored.
pub fn worktree_orphan_infos(
    ws: &Workspace,
    project: &str,
    registered: &BTreeSet<String>,
) -> Vec<WorktreeOrphanInfo> {
    orphan_slot_names(ws, project, registered)
        .into_iter()
        .map(|name| WorktreeOrphanInfo {
            path: rel_slot_path(project, &name),
            name,
        })
        .collect()
}

/// Disk dirs under `worktrees/<project>/` with valid slot names not in `registered`.
fn orphan_slot_names(ws: &Workspace, project: &str, registered: &BTreeSet<String>) -> Vec<String> {
    let project_wt = ws.root.join("worktrees").join(project);
    if !project_wt.is_dir() {
        return Vec::new();
    }
    let Ok(rd) = fs::read_dir(&project_wt) else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    for ent in rd.flatten() {
        let Ok(ft) = ent.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let name = ent.file_name();
        let Some(s) = name.to_str() else {
            continue;
        };
        if validate_slot_name(s).is_err() {
            continue;
        }
        if registered.contains(s) {
            continue;
        }
        names.push(s.to_string());
    }
    names.sort();
    names
}

fn rel_slot_path(project: &str, slot: &str) -> String {
    format!("worktrees/{project}/{slot}")
}

fn resolve_primary_git<R: odm_git::CommandRunner>(
    git: &Git<R>,
    ws: &Workspace,
    project: &str,
) -> Result<PathBuf, OdmError> {
    let entry = ws.config.projects.get(project).ok_or_else(|| {
        OdmError::usage(format!("unknown project '{project}'"))
    })?;
    let path = abs_checkout(&ws.root, &entry.path)?;
    if !path.exists() {
        return Err(OdmError::not_found(format!(
            "project path missing: {}",
            entry.path
        )));
    }
    if !git.is_repo(&path)? {
        return Err(OdmError::operation(format!(
            "project path is not a git repo: {}",
            entry.path
        )));
    }
    Ok(path)
}

fn slot_info_under_prefix(
    prefix: &Path,
    project: &str,
    abs_path: &Path,
) -> Option<WorktreeSlotInfo> {
    let rel = abs_path.strip_prefix(prefix).ok()?;
    let mut comps = rel.components();
    match (comps.next(), comps.next()) {
        (Some(Component::Normal(name)), None) => {
            let name = name.to_string_lossy().into_owned();
            if name.is_empty() || name == "." || name == ".." {
                return None;
            }
            Some(WorktreeSlotInfo {
                name: name.clone(),
                path: rel_slot_path(project, &name),
                dirty: None,
            })
        }
        _ => None,
    }
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    // Compare component-wise after normalizing `.` only (no symlink resolve).
    let na: Vec<_> = a.components().filter(|c| *c != Component::CurDir).collect();
    let nb: Vec<_> = b.components().filter(|c| *c != Component::CurDir).collect();
    na == nb
}

#[cfg(test)]
#[path = "worktree_tests.rs"]
mod tests;
