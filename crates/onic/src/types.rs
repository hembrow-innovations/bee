use std::path::PathBuf;

use serde_json::{Map, Value};

pub const EXTRACT_VERSION: &str = "109";
pub const SCHEMA_VERSION: &str = "1";
pub const DEFAULT_PORT: u16 = 4747;

#[derive(Clone, Debug)]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub file_path: Option<String>,
    pub start_line: Option<i64>,
    pub end_line: Option<i64>,
    pub body: Option<String>,
    pub props: Map<String, Value>,
}

#[derive(Clone, Debug)]
pub struct GraphEdge {
    pub src: String,
    pub dst: String,
    pub kind: String,
    pub confidence: String,
    pub props: Map<String, Value>,
}

#[derive(Clone, Debug, Default)]
pub struct ExtractedGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Clone, Debug)]
pub struct ScannedFile {
    pub path: String,
    pub abs_path: PathBuf,
    pub lang: String,
    pub hash: String,
    pub mtime: i64,
    pub size: u64,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct GoManifest {
    pub dir: String,
    pub module: String,
}

#[derive(Clone, Debug)]
pub struct DracManifest {
    pub dir: String,
    pub module: String,
}

#[derive(Clone, Debug, Default)]
pub struct Manifests {
    pub go: Vec<GoManifest>,
    pub drac: Vec<DracManifest>,
}

#[derive(Clone, Debug)]
pub struct BuildMeta {
    pub root: String,
    pub built_at: String,
    pub file_count: usize,
    pub extract_version: String,
}

#[derive(Clone, Debug)]
pub struct BuildReport {
    pub db_path: PathBuf,
    pub file_count: usize,
    pub node_count: usize,
    pub edge_count: usize,
    pub reused: bool,
}

pub fn empty_props() -> Map<String, Value> {
    Map::new()
}

pub fn props_json(props: &Map<String, Value>) -> String {
    serde_json::to_string(&Value::Object(props.clone())).unwrap_or_else(|_| "{}".to_string())
}
