//! `odm-progen` — federation/scope + Obsidian-compatible vault store ops.

mod index;
mod membership;
mod note;
mod ops;
mod scope;
mod store;
mod vault;

pub use index::IndexStats;
pub use membership::{add_progen, rm_progen};
pub use note::{parse_wikilinks, NoteDoc, NoteId};
pub use ops::{
    doctor_progens, format_context_human, format_find_human, format_get_human, format_ls_human,
    ProgenDoctorCheck,
};
pub use scope::{
    resolve_read_scope, resolve_single_progen, resolve_write_progen, scoped_from_config,
    ScopedProgen,
};
pub use store::{
    context_notes, find_notes, get_note, list_notes, one_progen_flag, open_for_id, open_single,
    reindex_for_cli, reindex_scope, ContextHit, FindHit, GetResult, LsHit, ProgenStore,
};
pub use vault::{ensure_vault, vault_info, vault_path, VaultInfo};
