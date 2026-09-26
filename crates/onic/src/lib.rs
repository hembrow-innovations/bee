//! Project graph for `bee onic`.

mod build;
mod comments;
mod error;
mod extract;
mod graph;
mod ids;
mod lang_drac;
mod lang_go;
mod lang_go_walk;
mod lang_js;
mod lang_js_call;
mod lang_py;
mod lang_rust;
mod lang_rust_body;
mod markdown;
mod parsers;
mod schema;
mod serve;
mod store;
mod types;
mod walk;
mod write;

pub use build::{build_project, watch_project};
pub use error::{OnicError, OnicResult};
pub use ids::{as_rel_path, file_id, symbol_id};
pub use serve::{handle_request, serve};
pub use store::Store;
pub use types::{BuildReport, DEFAULT_PORT, EXTRACT_VERSION};
pub use walk::{find_project_root, resolve_db};
