//! `odm find` — handler + DTO.

use hive_core::{HiveError, Hive};
use hive_store::{find_notes, format_find_human, FindHit};
use serde::Serialize;

use crate::ctx::Ctx;
use crate::present::{json_value, Present, Ready};

/// `odm find --json` envelope.
#[derive(Debug, Clone, Serialize)]
pub struct FindDto {
    pub hits: Vec<FindHit>,
}

impl Present for FindDto {
    fn to_json(&self) -> Result<serde_json::Value, HiveError> {
        json_value(self)
    }
    fn to_human(&self) -> String {
        format_find_human(&self.hits)
    }
}

/// Library entrypoint: find notes across progen scope.
pub fn find_notes_dto(
    ws: &Hive,
    query: &str,
    progens: &[String],
    groups: &[String],
    limit: usize,
) -> Result<FindDto, HiveError> {
    let hits = find_notes(ws, query, progens, groups, limit)?;
    Ok(FindDto { hits })
}

pub fn find_cmd(ctx: &Ctx, query: Option<String>, limit: usize) -> Result<Ready<FindDto>, HiveError> {
    if limit == 0 {
        return Err(HiveError::usage("--limit must be at least 1"));
    }
    let q = query.unwrap_or_default();
    let dto = find_notes_dto(&ctx.ws, &q, &ctx.progen, &ctx.progen_group, limit)?;
    let human = format_find_human(&dto.hits);
    Ok(Ready::ok(dto, human))
}
