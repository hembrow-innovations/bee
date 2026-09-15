//! `odm project worktree` handlers, DTOs, and human formatters.

use odm_core::{
    worktree_add, worktree_list, worktree_prune, worktree_prune_all, worktree_rm, OdmError,
    WorktreeListOutcome, WorktreePruneAllOutcome, WorktreePruneOutcome, WorktreeSlotOutcome,
};
use serde::Serialize;

use crate::ctx::Ctx;
use crate::present::{json_value, Present, Ready};

/// `odm project worktree list --json`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorktreeListDto {
    pub project: String,
    pub slots: Vec<WorktreeSlotDto>,
}

/// One slot in list JSON.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorktreeSlotDto {
    pub name: String,
    pub path: String,
    /// `true` dirty, `false` clean, `null` if probe failed / unknown (e.g. prune rows).
    pub dirty: Option<bool>,
}

/// One pruned/skipped slot entry (`{name,path}` only — no dirty).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorktreePruneSlotDto {
    pub name: String,
    pub path: String,
}

/// One slot under `prune --all` JSON (includes project).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorktreePruneAllSlotDto {
    pub project: String,
    pub name: String,
    pub path: String,
}

/// `odm project worktree add|rm --json`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorktreeSlotActionDto {
    pub project: String,
    pub slot: String,
    pub path: String,
}

/// `odm project worktree prune <project> --json`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorktreePruneDto {
    pub project: String,
    pub pruned: Vec<WorktreePruneSlotDto>,
    pub skipped_nonempty: Vec<WorktreePruneSlotDto>,
}

/// `odm project worktree prune --all --json`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorktreePruneAllDto {
    pub all: bool,
    pub pruned: Vec<WorktreePruneAllSlotDto>,
    pub skipped_nonempty: Vec<WorktreePruneAllSlotDto>,
}

pub fn worktree_list_dto(out: &WorktreeListOutcome) -> WorktreeListDto {
    WorktreeListDto {
        project: out.project.clone(),
        slots: out.slots.iter().map(slot_dto).collect(),
    }
}

fn slot_dto(s: &odm_core::WorktreeSlotInfo) -> WorktreeSlotDto {
    WorktreeSlotDto {
        name: s.name.clone(),
        path: s.path.clone(),
        dirty: s.dirty,
    }
}

pub fn worktree_slot_action_dto(out: &WorktreeSlotOutcome) -> WorktreeSlotActionDto {
    WorktreeSlotActionDto {
        project: out.project.clone(),
        slot: out.slot.clone(),
        path: out.path.clone(),
    }
}

fn prune_slot_dto(s: &odm_core::WorktreePruneSlot) -> WorktreePruneSlotDto {
    WorktreePruneSlotDto {
        name: s.name.clone(),
        path: s.path.clone(),
    }
}

pub fn worktree_prune_dto(out: &WorktreePruneOutcome) -> WorktreePruneDto {
    WorktreePruneDto {
        project: out.project.clone(),
        pruned: out.pruned.iter().map(prune_slot_dto).collect(),
        skipped_nonempty: out.skipped_nonempty.iter().map(prune_slot_dto).collect(),
    }
}

pub fn worktree_prune_all_dto(out: &WorktreePruneAllOutcome) -> WorktreePruneAllDto {
    fn map_slots(
        slots: &[odm_core::WorktreePruneAllSlot],
    ) -> Vec<WorktreePruneAllSlotDto> {
        slots
            .iter()
            .map(|s| WorktreePruneAllSlotDto {
                project: s.project.clone(),
                name: s.name.clone(),
                path: s.path.clone(),
            })
            .collect()
    }
    WorktreePruneAllDto {
        all: true,
        pruned: map_slots(&out.pruned),
        skipped_nonempty: map_slots(&out.skipped_nonempty),
    }
}

pub fn format_worktree_list_human(out: &WorktreeListOutcome) -> String {
    let mut s = String::new();
    for slot in &out.slots {
        s.push_str(&slot.name);
        if slot.dirty == Some(true) {
            s.push_str(" dirty");
        }
        s.push('\n');
    }
    s
}

pub fn format_worktree_add_human(out: &WorktreeSlotOutcome) -> String {
    format!("added worktree slot {} -> {}", out.slot, out.path)
}

pub fn format_worktree_rm_human(out: &WorktreeSlotOutcome) -> String {
    format!("removed worktree slot {} ({})", out.slot, out.path)
}

pub fn format_worktree_prune_human(out: &WorktreePruneOutcome) -> String {
    let mut s = if out.pruned.is_empty() {
        "pruned 0 orphan worktree dirs".to_string()
    } else {
        let names: Vec<&str> = out.pruned.iter().map(|p| p.name.as_str()).collect();
        format!(
            "pruned {} orphan worktree dir{}: {}",
            out.pruned.len(),
            if out.pruned.len() == 1 { "" } else { "s" },
            names.join(", ")
        )
    };
    if !out.skipped_nonempty.is_empty() {
        let names: Vec<&str> = out
            .skipped_nonempty
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        s.push_str(&format!(
            "\nskipped non-empty orphan{} (use --force): {}",
            if out.skipped_nonempty.len() == 1 {
                ""
            } else {
                "s"
            },
            names.join(", ")
        ));
    }
    s
}

impl Present for WorktreeListDto {
    fn to_json(&self) -> Result<serde_json::Value, OdmError> {
        json_value(self)
    }
    fn to_human(&self) -> String {
        let mut s = String::new();
        for slot in &self.slots {
            s.push_str(&slot.name);
            if slot.dirty == Some(true) {
                s.push_str(" dirty");
            }
            s.push('\n');
        }
        s
    }
}

pub fn list_cmd(ctx: &Ctx, project: &str) -> Result<Ready<WorktreeListDto>, OdmError> {
    let outcome = worktree_list(&ctx.git, &ctx.ws, project)?;
    let dto = worktree_list_dto(&outcome);
    let human = format_worktree_list_human(&outcome);
    Ok(Ready::ok(dto, human))
}

pub fn add_cmd(
    ctx: &Ctx,
    project: &str,
    slot: &str,
    branch: Option<&str>,
) -> Result<Ready<WorktreeSlotActionDto>, OdmError> {
    let outcome = worktree_add(&ctx.git, &ctx.ws, project, slot, branch)?;
    let dto = worktree_slot_action_dto(&outcome);
    Ok(Ready::ok(dto, format_worktree_add_human(&outcome)))
}

pub fn rm_cmd(
    ctx: &Ctx,
    project: &str,
    slot: &str,
    force: bool,
) -> Result<Ready<WorktreeSlotActionDto>, OdmError> {
    let outcome = worktree_rm(&ctx.git, &ctx.ws, project, slot, force)?;
    let dto = worktree_slot_action_dto(&outcome);
    Ok(Ready::ok(dto, format_worktree_rm_human(&outcome)))
}

pub fn prune_cmd(
    ctx: &Ctx,
    project: Option<String>,
    all: bool,
    force: bool,
) -> Result<Ready<serde_json::Value>, OdmError> {
    if all {
        let outcome = worktree_prune_all(&ctx.git, &ctx.ws, force)?;
        let dto = worktree_prune_all_dto(&outcome);
        let exit = if outcome.skipped_nonempty.is_empty() {
            0
        } else {
            3
        };
        let human = format_worktree_prune_all_human(&outcome);
        let val = json_value(&dto)?;
        Ok(Ready::with_exit(val, human, exit))
    } else {
        let project = project.expect("clap requires project unless --all");
        let outcome = worktree_prune(&ctx.git, &ctx.ws, &project, force)?;
        let dto = worktree_prune_dto(&outcome);
        let exit = if outcome.skipped_nonempty.is_empty() {
            0
        } else {
            3
        };
        let human = format_worktree_prune_human(&outcome);
        let val = json_value(&dto)?;
        Ok(Ready::with_exit(val, human, exit))
    }
}

pub fn format_worktree_prune_all_human(out: &WorktreePruneAllOutcome) -> String {
    let mut s = if out.pruned.is_empty() {
        "pruned 0 orphan worktree dirs".to_string()
    } else {
        let names: Vec<String> = out
            .pruned
            .iter()
            .map(|p| format!("{}/{}", p.project, p.name))
            .collect();
        format!(
            "pruned {} orphan worktree dir{}: {}",
            out.pruned.len(),
            if out.pruned.len() == 1 { "" } else { "s" },
            names.join(", ")
        )
    };
    if !out.skipped_nonempty.is_empty() {
        let names: Vec<String> = out
            .skipped_nonempty
            .iter()
            .map(|p| format!("{}/{}", p.project, p.name))
            .collect();
        s.push_str(&format!(
            "\nskipped non-empty orphan{} (use --force): {}",
            if out.skipped_nonempty.len() == 1 {
                ""
            } else {
                "s"
            },
            names.join(", ")
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use odm_core::{
        WorktreeListOutcome, WorktreePruneOutcome, WorktreeSlotInfo, WorktreeSlotOutcome,
    };

    #[test]
    fn list_dto_json_shape() {
        let out = WorktreeListOutcome {
            project: "alpha".into(),
            slots: vec![
                WorktreeSlotInfo {
                    name: "a".into(),
                    path: "worktrees/alpha/a".into(),
                    dirty: Some(false),
                },
                WorktreeSlotInfo {
                    name: "b".into(),
                    path: "worktrees/alpha/b".into(),
                    dirty: Some(true),
                },
                WorktreeSlotInfo {
                    name: "c".into(),
                    path: "worktrees/alpha/c".into(),
                    dirty: None,
                },
            ],
        };
        let dto = worktree_list_dto(&out);
        let v = serde_json::to_value(&dto).unwrap();
        assert_eq!(v["project"], "alpha");
        assert_eq!(v["slots"][0]["name"], "a");
        assert_eq!(v["slots"][0]["path"], "worktrees/alpha/a");
        assert_eq!(v["slots"][0]["dirty"], false);
        assert_eq!(v["slots"][1]["name"], "b");
        assert_eq!(v["slots"][1]["dirty"], true);
        assert!(v["slots"][2]["dirty"].is_null());
    }

    #[test]
    fn action_dto_json_shape() {
        let out = WorktreeSlotOutcome {
            project: "alpha".into(),
            slot: "s1".into(),
            path: "worktrees/alpha/s1".into(),
        };
        let v = serde_json::to_value(worktree_slot_action_dto(&out)).unwrap();
        assert_eq!(v["project"], "alpha");
        assert_eq!(v["slot"], "s1");
        assert_eq!(v["path"], "worktrees/alpha/s1");
    }

    #[test]
    fn list_human_one_name_per_line() {
        let out = WorktreeListOutcome {
            project: "alpha".into(),
            slots: vec![
                WorktreeSlotInfo {
                    name: "a".into(),
                    path: "worktrees/alpha/a".into(),
                    dirty: Some(false),
                },
                WorktreeSlotInfo {
                    name: "b".into(),
                    path: "worktrees/alpha/b".into(),
                    dirty: Some(true),
                },
                WorktreeSlotInfo {
                    name: "c".into(),
                    path: "worktrees/alpha/c".into(),
                    dirty: None,
                },
            ],
        };
        assert_eq!(format_worktree_list_human(&out), "a\nb dirty\nc\n");
    }

    #[test]
    fn prune_dto_json_shape() {
        let out = WorktreePruneOutcome {
            project: "alpha".into(),
            pruned: vec![odm_core::WorktreePruneSlot {
                name: "stale".into(),
                path: "worktrees/alpha/stale".into(),
            }],
            skipped_nonempty: vec![odm_core::WorktreePruneSlot {
                name: "full".into(),
                path: "worktrees/alpha/full".into(),
            }],
        };
        let v = serde_json::to_value(worktree_prune_dto(&out)).unwrap();
        assert_eq!(v["project"], "alpha");
        assert_eq!(v["pruned"][0]["name"], "stale");
        assert_eq!(v["pruned"][0]["path"], "worktrees/alpha/stale");
        assert!(v["pruned"][0].get("dirty").is_none());
        assert_eq!(v["skipped_nonempty"][0]["name"], "full");
        assert_eq!(v["skipped_nonempty"][0]["path"], "worktrees/alpha/full");
        assert!(v["skipped_nonempty"][0].get("dirty").is_none());
    }

    #[test]
    fn prune_human_zero_and_skipped() {
        let empty = WorktreePruneOutcome {
            project: "alpha".into(),
            pruned: vec![],
            skipped_nonempty: vec![],
        };
        assert_eq!(
            format_worktree_prune_human(&empty),
            "pruned 0 orphan worktree dirs"
        );

        let partial = WorktreePruneOutcome {
            project: "alpha".into(),
            pruned: vec![odm_core::WorktreePruneSlot {
                name: "empty".into(),
                path: "worktrees/alpha/empty".into(),
            }],
            skipped_nonempty: vec![odm_core::WorktreePruneSlot {
                name: "full".into(),
                path: "worktrees/alpha/full".into(),
            }],
        };
        let h = format_worktree_prune_human(&partial);
        assert!(h.contains("pruned 1 orphan worktree dir: empty"));
        assert!(h.contains("skipped non-empty orphan (use --force): full"));
    }

    #[test]
    fn prune_all_dto_json_shape() {
        use odm_core::{WorktreePruneAllOutcome, WorktreePruneAllSlot};
        let out = WorktreePruneAllOutcome {
            pruned: vec![WorktreePruneAllSlot {
                project: "alpha".into(),
                name: "stale".into(),
                path: "worktrees/alpha/stale".into(),
            }],
            skipped_nonempty: vec![WorktreePruneAllSlot {
                project: "beta".into(),
                name: "full".into(),
                path: "worktrees/beta/full".into(),
            }],
        };
        let v = serde_json::to_value(worktree_prune_all_dto(&out)).unwrap();
        assert_eq!(v["all"], true);
        assert_eq!(v["pruned"][0]["project"], "alpha");
        assert_eq!(v["pruned"][0]["name"], "stale");
        assert_eq!(v["pruned"][0]["path"], "worktrees/alpha/stale");
        assert_eq!(v["skipped_nonempty"][0]["project"], "beta");
        assert_eq!(v["skipped_nonempty"][0]["name"], "full");
    }

    #[test]
    fn prune_all_human_qualified_names() {
        use odm_core::{WorktreePruneAllOutcome, WorktreePruneAllSlot};
        let out = WorktreePruneAllOutcome {
            pruned: vec![WorktreePruneAllSlot {
                project: "alpha".into(),
                name: "empty".into(),
                path: "worktrees/alpha/empty".into(),
            }],
            skipped_nonempty: vec![WorktreePruneAllSlot {
                project: "beta".into(),
                name: "full".into(),
                path: "worktrees/beta/full".into(),
            }],
        };
        let h = format_worktree_prune_all_human(&out);
        assert!(h.contains("pruned 1 orphan worktree dir: alpha/empty"));
        assert!(h.contains("skipped non-empty orphan (use --force): beta/full"));
    }
}
