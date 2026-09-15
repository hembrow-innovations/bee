//! `odm generate` — handlers, list/run DTOs, human formatting.

use std::path::PathBuf;

use odm_core::{generate_local, path_buf_to_rel, OdmError, Workspace};
use serde::Serialize;

use crate::ctx::Ctx;
use crate::present::{json_value, Present, Ready};

/// `odm generate --json` (no name) envelope.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GeneratorListDto {
    pub generators: Vec<GeneratorListItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GeneratorListItem {
    pub name: String,
    /// Present as JSON `null` when unset (do not skip).
    pub template: Option<String>,
    /// Present as JSON `null` when unset (do not skip).
    pub url: Option<String>,
}

/// Library entrypoint: list configured generators as a serializable DTO (sorted by name).
pub fn list_generators_dto(ws: &Workspace) -> GeneratorListDto {
    let generators = ws
        .generators
        .iter()
        .map(|(name, def)| GeneratorListItem {
            name: name.clone(),
            template: def.template.clone(),
            url: def.url.clone(),
        })
        .collect();
    GeneratorListDto { generators }
}

/// Human one-name-per-line list (beside DTO).
pub fn format_generator_list_human(dto: &GeneratorListDto) -> String {
    if dto.generators.is_empty() {
        return "(no generators)\n".into();
    }
    let mut out = String::new();
    for g in &dto.generators {
        out.push_str(&g.name);
        out.push('\n');
    }
    out
}

/// Human success one-liner after materialize (or dry-run preview).
pub fn format_generate_run_human(name: &str, dest: &str, copied: u32, dry_run: bool) -> String {
    if dry_run {
        format!("would generate {name} -> {dest} ({copied} files)\n")
    } else {
        format!("generated {name} -> {dest} ({copied} files)\n")
    }
}

/// `odm generate <name> --json` envelope.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GenerateRunDto {
    pub generator: String,
    pub dest: String,
    pub copied: u32,
    pub dry_run: bool,
}

impl Present for GeneratorListDto {
    fn to_json(&self) -> Result<serde_json::Value, OdmError> {
        json_value(self)
    }
    fn to_human(&self) -> String {
        format_generator_list_human(self)
    }
}

impl Present for GenerateRunDto {
    fn to_json(&self) -> Result<serde_json::Value, OdmError> {
        json_value(self)
    }
    fn to_human(&self) -> String {
        format_generate_run_human(&self.generator, &self.dest, self.copied, self.dry_run)
    }
}

pub fn generate_cmd(
    ctx: &Ctx,
    name: Option<String>,
    dest: Option<PathBuf>,
    force: bool,
    dry_run: bool,
) -> Result<Ready<serde_json::Value>, OdmError> {
    match name {
        None => {
            let dto = list_generators_dto(&ctx.ws);
            let human = format_generator_list_human(&dto);
            Ok(Ready::ok(json_value(&dto)?, human))
        }
        Some(name) => {
            let dest = dest.ok_or_else(|| {
                OdmError::usage("generate requires --dest <path> when a name is given")
            })?;
            let dest_rel = path_buf_to_rel(&dest)?;
            let outcome = generate_local(&ctx.ws, &name, &dest_rel, force, dry_run)?;
            let dto = GenerateRunDto {
                generator: name.clone(),
                dest: dest_rel.clone(),
                copied: outcome.copied,
                dry_run,
            };
            let human = format_generate_run_human(&name, &dest_rel, outcome.copied, dry_run);
            Ok(Ready::ok(json_value(&dto)?, human))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use odm_core::{GeneratorDef, Workspace, WorkspaceConfig};

    fn ws_with(generators: BTreeMap<String, GeneratorDef>) -> Workspace {
        Workspace {
            root: PathBuf::from("/tmp/ws"),
            config: WorkspaceConfig::default(),
            actions: BTreeMap::new(),
            generators,
        }
    }

    #[test]
    fn generator_list_dto_json_nulls_and_sorted_names() {
        let mut generators = BTreeMap::new();
        generators.insert(
            "zeta".into(),
            GeneratorDef {
                template: Some("t/z".into()),
                url: None,
            },
        );
        generators.insert(
            "alpha".into(),
            GeneratorDef {
                template: None,
                url: Some("https://example.com/g".into()),
            },
        );
        let dto = list_generators_dto(&ws_with(generators));
        let v = serde_json::to_value(&dto).unwrap();
        let arr = v["generators"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], "alpha");
        assert!(arr[0]["template"].is_null());
        assert_eq!(arr[0]["url"], "https://example.com/g");
        assert_eq!(arr[1]["name"], "zeta");
        assert_eq!(arr[1]["template"], "t/z");
        assert!(arr[1]["url"].is_null());
    }

    #[test]
    fn empty_generator_list_human() {
        let dto = list_generators_dto(&ws_with(BTreeMap::new()));
        assert_eq!(format_generator_list_human(&dto), "(no generators)\n");
    }

    #[test]
    fn generate_run_dto_shape() {
        let dto = GenerateRunDto {
            generator: "pkg".into(),
            dest: "out/pkg".into(),
            copied: 3,
            dry_run: false,
        };
        let v = serde_json::to_value(&dto).unwrap();
        assert_eq!(v["generator"], "pkg");
        assert_eq!(v["dest"], "out/pkg");
        assert_eq!(v["copied"], 3);
        assert_eq!(v["dry_run"], false);
    }

    #[test]
    fn generate_run_dto_dry_run_true() {
        let dto = GenerateRunDto {
            generator: "pkg".into(),
            dest: "out/pkg".into(),
            copied: 2,
            dry_run: true,
        };
        let v = serde_json::to_value(&dto).unwrap();
        assert_eq!(v["dry_run"], true);
        assert_eq!(v["copied"], 2);
    }

    #[test]
    fn format_generate_run_human_line() {
        assert_eq!(
            format_generate_run_human("pkg", "out/pkg", 2, false),
            "generated pkg -> out/pkg (2 files)\n"
        );
    }

    #[test]
    fn format_generate_run_human_dry_run_line() {
        assert_eq!(
            format_generate_run_human("pkg", "out/pkg", 2, true),
            "would generate pkg -> out/pkg (2 files)\n"
        );
    }
}
