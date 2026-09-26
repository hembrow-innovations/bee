use std::fs;
use std::path::{Path, PathBuf};

use hive_onic::{build_project, Store};
use serde_json::Value;

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), &to).unwrap();
        }
    }
}

fn oracle() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/oracle.json");
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn node_key(row: &Value) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        row["id"].as_str().unwrap_or(""),
        row["kind"].as_str().unwrap_or(""),
        row["name"].as_str().unwrap_or(""),
        row["file_path"].as_str().unwrap_or(""),
        row["start_line"].as_i64().map(|n| n.to_string()).unwrap_or_default(),
        row["end_line"].as_i64().map(|n| n.to_string()).unwrap_or_default()
    )
}

fn sorted_nodes(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_array().unwrap().iter().map(node_key).collect();
    keys.sort();
    keys
}

fn edge_key(row: &Value) -> String {
    let props = row.get("props").cloned().unwrap_or(Value::Null);
    let props = if let Some(text) = props.as_str() {
        serde_json::from_str(text).unwrap_or(Value::Null)
    } else {
        props
    };
    format!(
        "{}|{}|{}|{}|{}",
        row["src"].as_str().unwrap_or(""),
        row["dst"].as_str().unwrap_or(""),
        row["kind"].as_str().unwrap_or(""),
        row["confidence"].as_str().unwrap_or(""),
        props
    )
}

fn sorted_edges(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_array().unwrap().iter().map(edge_key).collect();
    keys.sort();
    keys
}

#[test]
fn mini_graph_matches_onic_oracle() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini");
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("mini");
    copy_dir(&fixture, &root);
    let db = root.join(".onic").join("graph.db");
    let report = build_project(&root, &db).expect("build");
    assert!(!report.reused);
    let line = format!(
        "wrote {} ({} files, {} nodes, {} edges)",
        db.display(),
        report.file_count,
        report.node_count,
        report.edge_count
    );
    assert!(line.contains("wrote"));
    let again = build_project(&root, &db).expect("reuse");
    assert!(again.reused, "second build should reuse");

    let expected = oracle();
    let store = Store::open(&db).expect("open");
    let nodes: Value = serde_json::from_str(
        &store
            .sql_json("SELECT id, kind, name, file_path, start_line, end_line, body FROM nodes ORDER BY id")
            .unwrap(),
    )
    .unwrap();
    let edges: Value = serde_json::from_str(
        &store
            .sql_json("SELECT src, dst, kind, confidence, props FROM edges ORDER BY src, dst, kind")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(sorted_nodes(&nodes), sorted_nodes(&expected["nodes"]));
    assert_eq!(sorted_edges(&edges), sorted_edges(&expected["edges"]));
    assert_eq!(report.file_count, 25);
    assert_eq!(report.node_count, 215);
    assert_eq!(report.edge_count, 329);

    let search: Value = serde_json::from_str(&store.search_json("login", 20).unwrap()).unwrap();
    assert!(search.as_array().unwrap().iter().any(|row| row["id"] == "symbol:src/auth.ts#login"));
    let explained: Value = serde_json::from_str(&store.explain_json("login").unwrap()).unwrap();
    assert_eq!(explained["node"]["id"], "symbol:src/auth.ts#login");
    println!("onic-oracle-proved files=25 nodes=215 edges=329");
}
