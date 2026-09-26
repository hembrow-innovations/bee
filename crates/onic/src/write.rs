use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};

use crate::error::OnicResult;
use crate::schema::SCHEMA_SQL;
use crate::types::{
    props_json, BuildMeta, ExtractedGraph, GraphEdge, GraphNode, ScannedFile, EXTRACT_VERSION,
    SCHEMA_VERSION,
};

pub fn replace_file(
    path: &Path,
    files: &[ScannedFile],
    graph: &ExtractedGraph,
    meta: &BuildMeta,
    extracts: &HashMap<String, ExtractedGraph>,
) -> OnicResult<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let tmp = tmp_path(path);
    unlink_if_exists(&tmp)?;
    let mut conn = Connection::open(&tmp)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    conn.execute_batch(SCHEMA_SQL)?;
    conn.execute_batch(
        "CREATE TABLE extract_parts (path TEXT PRIMARY KEY, nodes TEXT NOT NULL, edges TEXT NOT NULL)",
    )?;
    let tx = conn.transaction()?;
    {
        let mut insert_file = tx.prepare(
            "INSERT INTO files(path, lang, hash, mtime, size) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        let mut insert_extract =
            tx.prepare("INSERT INTO extract_parts(path, nodes, edges) VALUES (?1, ?2, ?3)")?;
        let mut insert_node = tx.prepare(
            "INSERT INTO nodes(id, kind, name, file_path, start_line, end_line, body, props) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        let mut insert_edge = tx.prepare(
            "INSERT INTO edges(src, dst, kind, confidence, props) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        let mut insert_meta = tx.prepare("INSERT INTO meta(key, value) VALUES (?1, ?2)")?;
        for file in files {
            insert_file.execute(params![
                file.path,
                file.lang,
                file.hash,
                file.mtime,
                file.size as i64
            ])?;
            if let Some(part) = extracts.get(&file.path) {
                let (nodes, edges) = graph_json(part)?;
                insert_extract.execute(params![file.path, nodes, edges])?;
            }
        }
        for node in &graph.nodes {
            insert_node.execute(params![
                node.id,
                node.kind,
                node.name,
                node.file_path,
                node.start_line,
                node.end_line,
                node.body,
                props_json(&node.props)
            ])?;
        }
        for edge in &graph.edges {
            insert_edge.execute(params![
                edge.src,
                edge.dst,
                edge.kind,
                edge.confidence,
                props_json(&edge.props)
            ])?;
        }
        insert_meta.execute(params!["schema_version", SCHEMA_VERSION])?;
        insert_meta.execute(params!["extract_version", meta.extract_version])?;
        insert_meta.execute(params!["root", meta.root])?;
        insert_meta.execute(params!["built_at", meta.built_at])?;
        insert_meta.execute(params!["file_count", meta.file_count.to_string()])?;
    }
    tx.commit()?;
    drop(conn);
    unlink_if_exists(&sidecar(path, "wal"))?;
    unlink_if_exists(&sidecar(path, "shm"))?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn inspect_cache(path: &Path, files: &[ScannedFile]) -> OnicResult<CachePlan> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA query_only = ON")?;
    if !files_match(&conn, files)? {
        return Ok(CachePlan::Rebuild {
            cached: load_matching(&conn, files),
        });
    }
    Ok(CachePlan::Reuse {
        file_count: count(&conn, "SELECT count(*) FROM files")?,
        node_count: count(&conn, "SELECT count(*) FROM nodes")?,
        edge_count: count(&conn, "SELECT count(*) FROM edges")?,
    })
}

pub enum CachePlan {
    Reuse {
        file_count: usize,
        node_count: usize,
        edge_count: usize,
    },
    Rebuild {
        cached: HashMap<String, ExtractedGraph>,
    },
}

fn files_match(conn: &Connection, files: &[ScannedFile]) -> OnicResult<bool> {
    if meta_value(conn, "schema_version")? != Some(SCHEMA_VERSION.to_string())
        || meta_value(conn, "extract_version")? != Some(EXTRACT_VERSION.to_string())
    {
        return Ok(false);
    }
    let mut stmt = conn.prepare("SELECT path, hash FROM files")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut seen = HashMap::new();
    for row in rows {
        let (path, hash) = row?;
        seen.insert(path, hash);
    }
    if seen.len() != files.len() {
        return Ok(false);
    }
    Ok(files
        .iter()
        .all(|file| seen.get(&file.path) == Some(&file.hash)))
}

fn load_matching(conn: &Connection, files: &[ScannedFile]) -> HashMap<String, ExtractedGraph> {
    load_matching_inner(conn, files).unwrap_or_default()
}

fn load_matching_inner(
    conn: &Connection,
    files: &[ScannedFile],
) -> OnicResult<HashMap<String, ExtractedGraph>> {
    if meta_value(conn, "schema_version")? != Some(SCHEMA_VERSION.to_string())
        || meta_value(conn, "extract_version")? != Some(EXTRACT_VERSION.to_string())
    {
        return Ok(HashMap::new());
    }
    let exists = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'extract_parts'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if exists.is_none() {
        return Ok(HashMap::new());
    }
    let hashes: HashMap<&str, &str> = files
        .iter()
        .map(|file| (file.path.as_str(), file.hash.as_str()))
        .collect();
    let mut stmt = conn.prepare(
        "SELECT e.path, e.nodes, e.edges, f.hash FROM extract_parts e JOIN files f ON f.path = e.path",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (path, nodes, edges, hash) = row?;
        if hashes.get(path.as_str()) != Some(&hash.as_str()) {
            continue;
        }
        let nodes: Value = serde_json::from_str(&nodes)?;
        let edges: Value = serde_json::from_str(&edges)?;
        let (Some(nodes), Some(edges)) = (nodes.as_array(), edges.as_array()) else {
            return Ok(HashMap::new());
        };
        out.insert(path, graph_from_json(nodes, edges));
    }
    Ok(out)
}

fn graph_from_json(nodes: &[Value], edges: &[Value]) -> ExtractedGraph {
    ExtractedGraph {
        nodes: nodes.iter().filter_map(node_from_json).collect(),
        edges: edges.iter().filter_map(edge_from_json).collect(),
    }
}

fn node_from_json(value: &Value) -> Option<GraphNode> {
    let obj = value.as_object()?;
    Some(GraphNode {
        id: string_field(obj, "id").unwrap_or_default(),
        kind: string_field(obj, "kind").unwrap_or_default(),
        name: string_field(obj, "name").unwrap_or_default(),
        file_path: string_field(obj, "filePath").or_else(|| string_field(obj, "file_path")),
        start_line: i64_field(obj, "startLine").or_else(|| i64_field(obj, "start_line")),
        end_line: i64_field(obj, "endLine").or_else(|| i64_field(obj, "end_line")),
        body: match obj.get("body") {
            Some(Value::String(text)) => Some(text.clone()),
            _ => None,
        },
        props: obj
            .get("props")
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default(),
    })
}

fn edge_from_json(value: &Value) -> Option<GraphEdge> {
    let obj = value.as_object()?;
    Some(GraphEdge {
        src: string_field(obj, "src").unwrap_or_default(),
        dst: string_field(obj, "dst").unwrap_or_default(),
        kind: string_field(obj, "kind").unwrap_or_default(),
        confidence: string_field(obj, "confidence").unwrap_or_else(|| "extracted".to_string()),
        props: obj
            .get("props")
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default(),
    })
}

fn string_field(obj: &Map<String, Value>, key: &str) -> Option<String> {
    obj.get(key)
        .and_then(|value| value.as_str().map(|text| text.to_string()))
}

fn i64_field(obj: &Map<String, Value>, key: &str) -> Option<i64> {
    obj.get(key).and_then(|value| value.as_i64())
}

fn graph_json(graph: &ExtractedGraph) -> OnicResult<(String, String)> {
    let nodes = Value::Array(graph.nodes.iter().map(node_json).collect());
    let edges = Value::Array(graph.edges.iter().map(edge_json).collect());
    Ok((
        serde_json::to_string(&nodes)?,
        serde_json::to_string(&edges)?,
    ))
}

fn node_json(node: &GraphNode) -> Value {
    serde_json::json!({
        "id": node.id,
        "kind": node.kind,
        "name": node.name,
        "filePath": node.file_path,
        "startLine": node.start_line,
        "endLine": node.end_line,
        "body": node.body,
        "props": node.props,
    })
}

fn edge_json(edge: &GraphEdge) -> Value {
    serde_json::json!({
        "src": edge.src,
        "dst": edge.dst,
        "kind": edge.kind,
        "confidence": edge.confidence,
        "props": edge.props,
    })
}

fn meta_value(conn: &Connection, key: &str) -> OnicResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?)
}

fn count(conn: &Connection, sql: &str) -> OnicResult<usize> {
    let n: i64 = conn.query_row(sql, [], |row| row.get(0))?;
    Ok(n.max(0) as usize)
}

fn tmp_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.tmp", path.display()))
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}-{suffix}", path.display()))
}

fn unlink_if_exists(path: &Path) -> OnicResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}
