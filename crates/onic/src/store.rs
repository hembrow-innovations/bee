use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row};
use serde_json::{Map, Number, Value};

use crate::error::{OnicError, OnicResult};
use crate::ids::parse_node_id;

const COMPACT_NEIGHBOR_CAP: usize = 50;
const NODE_COLS: &str = "id, kind, name, file_path, start_line, end_line, body, props";
const SCHEMA_EXAMPLES: &[&str] = &[
    "SELECT kind, count(*) AS n FROM nodes GROUP BY kind",
    "SELECT n.id, n.kind, n.name FROM nodes_fts f JOIN nodes n ON n.rowid = f.rowid WHERE nodes_fts MATCH 'session*' LIMIT 20",
];

pub struct Store {
    conn: Connection,
}

#[derive(Clone)]
struct NodeRec {
    id: String,
    kind: String,
    name: String,
    file_path: Option<String>,
    start_line: Option<i64>,
    end_line: Option<i64>,
    body: Option<String>,
    props: Value,
}

struct Hit {
    id: String,
    kind: String,
    name: String,
    file_path: Option<String>,
}

struct Neighbor {
    direction: &'static str,
    edge_kind: String,
    confidence: String,
    id: String,
    name: String,
    kind: String,
}

impl Store {
    pub fn open(path: &Path) -> OnicResult<Self> {
        if !path.is_file() {
            return Err(OnicError::msg("no graph"));
        }
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|err| OnicError::msg(format!("no graph: {err}")))?;
        conn.pragma_update(None, "query_only", "ON")
            .map_err(|err| OnicError::msg(format!("no graph: {err}")))?;
        let exists: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'nodes'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| OnicError::msg(format!("no graph: {err}")))?;
        if exists.is_none() {
            return Err(OnicError::msg("no graph"));
        }
        Ok(Self { conn })
    }

    pub fn schema_text(&self) -> OnicResult<String> {
        let kinds = self
            .kind_counts("SELECT kind, count(*) AS n FROM nodes GROUP BY kind ORDER BY n DESC")?;
        let edges = self
            .kind_counts("SELECT kind, count(*) AS n FROM edges GROUP BY kind ORDER BY n DESC")?;
        let mut ddl = Vec::new();
        let mut stmt = self.conn.prepare(
            "SELECT sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'extract_%' ORDER BY name",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let sql: String = row.get(0)?;
            ddl.push(sql);
        }
        let kind_lines = count_lines(&kinds);
        let edge_lines = count_lines(&edges);
        let examples = SCHEMA_EXAMPLES
            .iter()
            .map(|example| format!("  {example}"))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(format!(
            "Node kinds in this database:\n{kind_lines}\n\nEdge kinds in this database:\n{edge_lines}\n\nDDL:\n{}\n\nExamples:\n{examples}",
            ddl.join(";\n\n")
        ))
    }

    pub fn sql_json(&self, query: &str) -> OnicResult<String> {
        let statement = one_read_statement(query)?;
        let rows = self.query_maps(&statement, &[])?;
        pretty(&Value::Array(rows.into_iter().map(Value::Object).collect()))
    }

    pub fn search_json(&self, text: &str, limit: usize) -> OnicResult<String> {
        let hits = self.search(text, limit, None)?;
        pretty(&Value::Array(hits.into_iter().map(hit_value).collect()))
    }

    pub fn explain_json(&self, name: &str) -> OnicResult<String> {
        let node = self.require_node(name)?;
        let neighbors = self.neighbor_rows(&node.id)?;
        let mut map = Map::new();
        map.insert("node".into(), node_value(&node, true));
        map.insert(
            "neighbors".into(),
            Value::Array(neighbors.iter().map(neighbor_value).collect()),
        );
        pretty(&Value::Object(map))
    }

    pub fn compact_json(&self, name: &str) -> OnicResult<String> {
        let node = self.require_node(name)?;
        let all = self.neighbor_rows(&node.id)?;
        let omitted = all.len().saturating_sub(COMPACT_NEIGHBOR_CAP);
        let neighbors: Vec<_> = all.into_iter().take(COMPACT_NEIGHBOR_CAP).collect();
        let mut map = Map::new();
        map.insert("node".into(), node_value(&node, false));
        map.insert(
            "neighbors".into(),
            Value::Array(neighbors.iter().map(neighbor_value).collect()),
        );
        map.insert("truncated".into(), Value::Bool(omitted > 0));
        map.insert("omitted".into(), Value::Number((omitted as i64).into()));
        pretty(&Value::Object(map))
    }

    pub fn neighbors_json(&self, name: &str) -> OnicResult<String> {
        let node = self.require_node(name)?;
        let neighbors = self.neighbor_rows(&node.id)?;
        pretty(&Value::Array(
            neighbors.iter().map(neighbor_value).collect(),
        ))
    }

    pub fn path_json(&self, from: &str, to: &str) -> OnicResult<String> {
        let hops = self.path_hops(from, to)?;
        if hops.is_empty() {
            return Err(OnicError::msg(format!("no path from {from} to {to}")));
        }
        pretty(&Value::Array(hops))
    }

    pub fn subgraph_json(
        &self,
        seed: Option<&str>,
        q: Option<&str>,
        kinds: Option<&str>,
        hops: i64,
        limit: i64,
    ) -> OnicResult<String> {
        let hops = hops.clamp(0, 3);
        let limit = limit.clamp(1, 500);
        let cap = limit;
        let kind_list = parse_kinds(kinds);
        if kind_list.as_ref().is_some_and(|list| list.is_empty()) {
            return pretty(&graph_payload(Vec::new(), Vec::new(), cap, hops));
        }
        let seed = seed.filter(|value| !value.is_empty());
        let q = q.filter(|value| !value.is_empty());
        let mut order = Vec::new();
        let mut keep = HashSet::new();
        if let Some(seed) = seed {
            let hits = self.find_nodes(seed)?;
            if hits.is_empty() {
                return Err(OnicError::msg(format!("no node named {seed}")));
            }
            let allowed: Vec<NodeRec> = match &kind_list {
                Some(list) => hits
                    .into_iter()
                    .filter(|node| list.iter().any(|kind| kind == &node.kind))
                    .collect(),
                None => hits,
            };
            if let Some(unique) = pick_unique(&allowed) {
                push_id(&mut order, &mut keep, unique.id.clone(), limit as usize);
            } else if allowed.len() > 1 {
                let ids = allowed
                    .iter()
                    .map(|node| node.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(OnicError::msg(format!("ambiguous name {seed}: {ids}")));
            }
        }
        if let Some(q) = q {
            let room = (limit as usize).saturating_sub(order.len());
            if room > 0 {
                for hit in self.search(q, room, kind_list.as_deref())? {
                    if order.len() >= limit as usize {
                        break;
                    }
                    push_id(&mut order, &mut keep, hit.id, limit as usize);
                }
            }
        }
        if order.is_empty() {
            if seed.is_some() || q.is_some() {
                return pretty(&graph_payload(Vec::new(), Vec::new(), cap, hops));
            }
            let nodes = self.seedless_nodes(kind_list.as_deref(), limit)?;
            let ids: HashSet<String> = nodes.iter().map(|node| node.id.clone()).collect();
            let edges = self.edges_among(&ids)?;
            let node_values = nodes.iter().map(|node| node_value(node, true)).collect();
            return pretty(&graph_payload(node_values, edges, cap, hops));
        }
        self.expand(
            &mut order,
            &mut keep,
            hops,
            limit as usize,
            kind_list.as_deref(),
        )?;
        let mut nodes = Vec::new();
        for id in &order {
            let Some(node) = self.node_by_id(id)? else {
                return Err(OnicError::msg(format!("missing node {id}")));
            };
            nodes.push(node_value(&node, true));
        }
        let edges = self.edges_among(&keep)?;
        pretty(&graph_payload(nodes, edges, cap, hops))
    }

    fn kind_counts(&self, sql: &str) -> OnicResult<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push((row.get(0)?, row.get(1)?));
        }
        Ok(out)
    }

    fn query_maps(&self, sql: &str, args: &[SqlValue]) -> OnicResult<Vec<Map<String, Value>>> {
        let mut stmt = self.conn.prepare(sql)?;
        let names: Vec<String> = stmt
            .column_names()
            .into_iter()
            .map(str::to_string)
            .collect();
        let mut rows = stmt.query(rusqlite::params_from_iter(args.iter()))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let mut obj = Map::new();
            for (i, name) in names.iter().enumerate() {
                obj.insert(name.clone(), sql_to_json(row.get_ref(i)?)?);
            }
            out.push(obj);
        }
        Ok(out)
    }

    fn find_nodes(&self, name_or_id: &str) -> OnicResult<Vec<NodeRec>> {
        if let Ok(id) = parse_node_id(name_or_id) {
            if let Some(node) = self.node_by_id(&id)? {
                return Ok(vec![node]);
            }
        }
        let named = self.nodes_where("name = ?1", &[SqlValue::Text(name_or_id.to_string())])?;
        if !named.is_empty() {
            return Ok(named);
        }
        self.nodes_where(
            "kind = 'symbol' AND name GLOB ?1",
            &[SqlValue::Text(glob_suffix(name_or_id))],
        )
    }

    fn require_node(&self, name_or_id: &str) -> OnicResult<NodeRec> {
        let hits = self.find_nodes(name_or_id)?;
        if hits.is_empty() {
            return Err(OnicError::msg(format!("no node named {name_or_id}")));
        }
        if let Some(unique) = pick_unique(&hits) {
            return Ok(unique.clone());
        }
        let ids = hits
            .iter()
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        Err(OnicError::msg(format!(
            "ambiguous name {name_or_id}: {ids}"
        )))
    }

    fn nodes_where(&self, filter: &str, args: &[SqlValue]) -> OnicResult<Vec<NodeRec>> {
        let sql = format!("SELECT {NODE_COLS} FROM nodes WHERE {filter}");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(args.iter()))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(read_node(row)?);
        }
        Ok(out)
    }

    fn node_by_id(&self, id: &str) -> OnicResult<Option<NodeRec>> {
        let sql = format!("SELECT {NODE_COLS} FROM nodes WHERE id = ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([id])?;
        if let Some(row) = rows.next()? {
            return Ok(Some(read_node(row)?));
        }
        Ok(None)
    }

    fn node_kind(&self, id: &str) -> OnicResult<Option<String>> {
        let kind = self
            .conn
            .query_row("SELECT kind FROM nodes WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(kind)
    }

    fn neighbor_rows(&self, id: &str) -> OnicResult<Vec<Neighbor>> {
        let mut out = self.directed(id, "out", "n.id = e.dst", "e.src = ?1")?;
        out.extend(self.directed(id, "in", "n.id = e.src", "e.dst = ?1")?);
        Ok(out)
    }

    fn directed(
        &self,
        id: &str,
        direction: &'static str,
        join: &str,
        filter: &str,
    ) -> OnicResult<Vec<Neighbor>> {
        let sql = format!(
            "SELECT e.kind AS edgeKind, e.confidence, n.id, n.name, n.kind FROM edges e JOIN nodes n ON {join} WHERE {filter}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([id])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(Neighbor {
                direction,
                edge_kind: row.get(0)?,
                confidence: row.get(1)?,
                id: row.get(2)?,
                name: row.get(3)?,
                kind: row.get(4)?,
            });
        }
        Ok(out)
    }

    fn search(&self, text: &str, limit: usize, kinds: Option<&[String]>) -> OnicResult<Vec<Hit>> {
        if let Some(exact) = self.search_exact(text, kinds)? {
            return Ok(exact.into_iter().take(limit).collect());
        }
        let Some(match_q) = fts_match_query(text) else {
            return Ok(Vec::new());
        };
        match self.search_fts(&match_q, limit, kinds) {
            Ok(rows) => Ok(rows),
            Err(_) => Ok(Vec::new()),
        }
    }

    fn search_exact(&self, text: &str, kinds: Option<&[String]>) -> OnicResult<Option<Vec<Hit>>> {
        let Ok(id) = parse_node_id(text.trim()) else {
            return Ok(None);
        };
        let hits = self
            .find_nodes(&id)?
            .into_iter()
            .filter(|node| node.id == id)
            .filter(|node| {
                kinds
                    .map(|list| list.iter().any(|kind| kind == &node.kind))
                    .unwrap_or(true)
            })
            .map(|node| Hit {
                id: node.id,
                kind: node.kind,
                name: node.name,
                file_path: node.file_path,
            })
            .collect::<Vec<_>>();
        if hits.is_empty() {
            Ok(None)
        } else {
            Ok(Some(hits))
        }
    }

    fn search_fts(
        &self,
        match_q: &str,
        limit: usize,
        kinds: Option<&[String]>,
    ) -> OnicResult<Vec<Hit>> {
        let kind_sql = match kinds {
            Some(list) if !list.is_empty() => {
                format!(
                    "AND n.kind IN ({})",
                    list.iter().map(|_| "?").collect::<Vec<_>>().join(",")
                )
            }
            _ => String::new(),
        };
        let sql = format!(
            "SELECT n.id, n.kind, n.name, n.file_path AS filePath FROM nodes_fts f JOIN nodes n ON n.rowid = f.rowid WHERE nodes_fts MATCH ?1 {kind_sql} ORDER BY rank, n.kind, n.name LIMIT ?{}"
            ,
            2 + kinds.map(|list| list.len()).unwrap_or(0)
        );
        let mut args = vec![SqlValue::Text(match_q.to_string())];
        if let Some(list) = kinds {
            if !list.is_empty() {
                for kind in list {
                    args.push(SqlValue::Text(kind.clone()));
                }
            }
        }
        args.push(SqlValue::Integer(limit as i64));
        let rows = self.query_maps(&sql, &args)?;
        Ok(rows
            .into_iter()
            .map(|row| Hit {
                id: field_str(&row, "id"),
                kind: field_str(&row, "kind"),
                name: field_str(&row, "name"),
                file_path: row
                    .get("filePath")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
            .collect())
    }

    fn path_hops(&self, from: &str, to: &str) -> OnicResult<Vec<Value>> {
        let start = self.require_node(from)?;
        let goal = self.require_node(to)?;
        if start.id == goal.id {
            return Ok(vec![hop_value(&start, None, None)]);
        }
        let mut stmt = self.conn.prepare("SELECT src, dst, kind FROM edges")?;
        let mut rows = stmt.query([])?;
        let mut adj: HashMap<String, Vec<(String, String, &'static str)>> = HashMap::new();
        while let Some(row) = rows.next()? {
            let src: String = row.get(0)?;
            let dst: String = row.get(1)?;
            let kind: String = row.get(2)?;
            adj.entry(src.clone())
                .or_default()
                .push((dst.clone(), kind.clone(), "out"));
            adj.entry(dst).or_default().push((src, kind, "in"));
        }
        let mut prev: HashMap<String, (String, String, &'static str)> = HashMap::new();
        let mut queue = VecDeque::from([start.id.clone()]);
        let mut seen = HashSet::from([start.id.clone()]);
        let mut found = false;
        while let Some(current) = queue.pop_front() {
            for (next, kind, direction) in adj.get(&current).into_iter().flatten() {
                if !seen.insert(next.clone()) {
                    continue;
                }
                prev.insert(next.clone(), (current.clone(), kind.clone(), *direction));
                if next == &goal.id {
                    found = true;
                    break;
                }
                queue.push_back(next.clone());
            }
            if found {
                break;
            }
        }
        if !found {
            return Ok(Vec::new());
        }
        let mut hops = Vec::new();
        let mut current = goal.id.clone();
        while current != start.id {
            let Some((from_id, kind, direction)) = prev.get(&current) else {
                return Ok(Vec::new());
            };
            let node = self.require_node(&current)?;
            hops.push(hop_value(&node, Some(kind), Some(*direction)));
            current = from_id.clone();
        }
        hops.push(hop_value(&start, None, None));
        hops.reverse();
        Ok(hops)
    }

    fn seedless_nodes(&self, kinds: Option<&[String]>, limit: i64) -> OnicResult<Vec<NodeRec>> {
        if let Some(list) = kinds {
            let placeholders = list.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT {NODE_COLS} FROM nodes WHERE kind IN ({placeholders}) ORDER BY name LIMIT ?{}",
                list.len() + 1
            );
            let mut args: Vec<SqlValue> = list.iter().cloned().map(SqlValue::Text).collect();
            args.push(SqlValue::Integer(limit));
            return self.nodes_sql(&sql, &args);
        }
        let sql = format!("SELECT {NODE_COLS} FROM nodes ORDER BY name LIMIT ?1");
        self.nodes_sql(&sql, &[SqlValue::Integer(limit)])
    }

    fn nodes_sql(&self, sql: &str, args: &[SqlValue]) -> OnicResult<Vec<NodeRec>> {
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(args.iter()))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(read_node(row)?);
        }
        Ok(out)
    }

    fn expand(
        &self,
        order: &mut Vec<String>,
        keep: &mut HashSet<String>,
        hops: i64,
        limit: usize,
        kinds: Option<&[String]>,
    ) -> OnicResult<()> {
        let mut frontier = order.clone();
        for _ in 0..hops {
            if keep.len() >= limit {
                break;
            }
            let mut next = Vec::new();
            for id in &frontier {
                let ends = self.edge_ends(id)?;
                for other in ends {
                    if keep.contains(&other) || keep.len() >= limit {
                        continue;
                    }
                    if let Some(list) = kinds {
                        let kind = self.node_kind(&other)?;
                        if !kind
                            .as_ref()
                            .is_some_and(|kind| list.iter().any(|want| want == kind))
                        {
                            continue;
                        }
                    }
                    keep.insert(other.clone());
                    order.push(other.clone());
                    next.push(other);
                }
            }
            frontier = next;
        }
        Ok(())
    }

    fn edge_ends(&self, id: &str) -> OnicResult<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT src, dst FROM edges WHERE src = ?1 OR dst = ?1")?;
        let mut rows = stmt.query([id])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let src: String = row.get(0)?;
            let dst: String = row.get(1)?;
            out.push(src);
            out.push(dst);
        }
        Ok(out)
    }

    fn edges_among(&self, ids: &HashSet<String>) -> OnicResult<Vec<Value>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self
            .conn
            .prepare("SELECT src, dst, kind, confidence, props FROM edges")?;
        let mut rows = stmt.query([])?;
        let mut edges = Vec::new();
        while let Some(row) = rows.next()? {
            let src: String = row.get(0)?;
            let dst: String = row.get(1)?;
            if !ids.contains(&src) || !ids.contains(&dst) {
                continue;
            }
            let confidence: String = row.get(3)?;
            let props_raw: String = row.get(4)?;
            let mut map = Map::new();
            map.insert("src".into(), Value::String(src));
            map.insert("dst".into(), Value::String(dst));
            map.insert("kind".into(), Value::String(row.get(2)?));
            map.insert(
                "confidence".into(),
                Value::String(if confidence == "resolved" {
                    "resolved".to_string()
                } else {
                    "extracted".to_string()
                }),
            );
            map.insert("props".into(), parse_props(&props_raw)?);
            edges.push(Value::Object(map));
        }
        Ok(edges)
    }
}

fn push_id(order: &mut Vec<String>, keep: &mut HashSet<String>, id: String, limit: usize) {
    if keep.contains(&id) || keep.len() >= limit {
        return;
    }
    keep.insert(id.clone());
    order.push(id);
}

fn pick_unique(nodes: &[NodeRec]) -> Option<&NodeRec> {
    if nodes.is_empty() {
        return None;
    }
    if nodes.len() == 1 {
        return Some(&nodes[0]);
    }
    let mut ranked: Vec<&NodeRec> = nodes.iter().collect();
    ranked.sort_by_key(|node| name_priority(&node.kind));
    let best = name_priority(&ranked[0].kind);
    let ties = ranked
        .iter()
        .filter(|node| name_priority(&node.kind) == best)
        .count();
    if ties == 1 {
        Some(ranked[0])
    } else {
        None
    }
}

fn name_priority(kind: &str) -> i32 {
    match kind {
        "symbol" => 0,
        "heading" => 1,
        "doc" => 2,
        "file" => 3,
        "comment" => 4,
        "tag" => 5,
        _ => 99,
    }
}

fn glob_suffix(name: &str) -> String {
    let escaped = name
        .replace('[', "[[]")
        .replace('*', "[*]")
        .replace('?', "[?]");
    format!("*.{escaped}")
}

fn fts_match_query(q: &str) -> Option<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in q.trim().chars() {
        if ch == '_' || ch.is_alphabetic() || ch.is_numeric() {
            current.push(ch);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    if tokens.is_empty() {
        return None;
    }
    Some(
        tokens
            .iter()
            .map(|token| fts_token(token))
            .collect::<Vec<_>>()
            .join(" AND "),
    )
}

fn fts_token(token: &str) -> String {
    if matches!(
        token.to_ascii_lowercase().as_str(),
        "and" | "or" | "not" | "near"
    ) {
        format!("\"{token}\"*")
    } else {
        format!("{token}*")
    }
}

fn one_read_statement(query: &str) -> OnicResult<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err(OnicError::msg("empty query"));
    }
    if trimmed.contains('\0') {
        return Err(OnicError::msg("only a single read statement is allowed"));
    }
    let statement = if trimmed.ends_with(';') {
        trimmed[..trimmed.len() - 1].trim_end().to_string()
    } else {
        trimmed.to_string()
    };
    if has_unquoted_semicolon(&statement)
        || mentions_extract(&statement)
        || !matches!(first_sql_keyword(&statement).as_str(), "SELECT" | "WITH")
    {
        return Err(OnicError::msg("only a single read statement is allowed"));
    }
    Ok(statement)
}

fn mentions_extract(sql: &str) -> bool {
    let chars: Vec<char> = sql.to_ascii_lowercase().chars().collect();
    let needle = ['e', 'x', 't', 'r', 'a', 'c', 't', '_'];
    for i in 0..chars.len() {
        if chars[i..].starts_with(&needle) {
            let boundary = i == 0 || !is_ident_char(chars[i - 1]);
            let after = i + needle.len();
            if boundary && after < chars.len() && is_ident_char(chars[after]) {
                return true;
            }
        }
    }
    false
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn has_unquoted_semicolon(sql: &str) -> bool {
    let chars: Vec<char> = sql.chars().collect();
    let mut quote = None;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            i += 1;
            continue;
        }
        if ch == '-' && chars.get(i + 1) == Some(&'-') {
            i += 2;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            if i < chars.len() {
                i += 1;
            }
            continue;
        }
        if ch == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i = if i + 1 < chars.len() {
                i + 2
            } else {
                chars.len()
            };
            continue;
        }
        if ch == ';' {
            return true;
        }
        i += 1;
    }
    false
}

fn first_sql_keyword(sql: &str) -> String {
    let mut text = sql.to_string();
    loop {
        let next = strip_sql_lead(&text);
        if next == text {
            break;
        }
        text = next;
    }
    text.chars()
        .take_while(|ch| ch.is_ascii_alphabetic() || *ch == '_')
        .collect::<String>()
        .to_ascii_uppercase()
}

fn strip_sql_lead(text: &str) -> String {
    let trimmed = text.trim_start();
    if trimmed.len() != text.len() {
        return trimmed.to_string();
    }
    if trimmed.starts_with('(') {
        return trimmed.trim_start_matches('(').to_string();
    }
    if trimmed.starts_with("--") {
        return match trimmed.find('\n') {
            Some(nl) => trimmed[nl + 1..].to_string(),
            None => String::new(),
        };
    }
    if let Some(rest) = trimmed.strip_prefix("/*") {
        if let Some(end) = rest.find("*/") {
            return rest[end + 2..].to_string();
        }
    }
    text.to_string()
}

fn parse_props(raw: &str) -> OnicResult<Value> {
    let value: Value = serde_json::from_str(raw)?;
    Ok(match value {
        Value::Object(map) => Value::Object(map),
        _ => Value::Object(Map::new()),
    })
}

fn read_node(row: &Row<'_>) -> OnicResult<NodeRec> {
    let props_raw: String = row.get(7)?;
    Ok(NodeRec {
        id: row.get(0)?,
        kind: row.get(1)?,
        name: row.get(2)?,
        file_path: row.get(3)?,
        start_line: row.get(4)?,
        end_line: row.get(5)?,
        body: row.get(6)?,
        props: parse_props(&props_raw)?,
    })
}

fn node_value(node: &NodeRec, full: bool) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), Value::String(node.id.clone()));
    map.insert("kind".into(), Value::String(node.kind.clone()));
    map.insert("name".into(), Value::String(node.name.clone()));
    map.insert("filePath".into(), opt_string(node.file_path.as_deref()));
    map.insert("startLine".into(), opt_i64(node.start_line));
    map.insert("endLine".into(), opt_i64(node.end_line));
    if full {
        map.insert("body".into(), opt_string(node.body.as_deref()));
        map.insert("props".into(), node.props.clone());
    }
    Value::Object(map)
}

fn neighbor_value(row: &Neighbor) -> Value {
    let mut map = Map::new();
    map.insert("direction".into(), Value::String(row.direction.to_string()));
    map.insert("edgeKind".into(), Value::String(row.edge_kind.clone()));
    map.insert("confidence".into(), Value::String(row.confidence.clone()));
    map.insert("id".into(), Value::String(row.id.clone()));
    map.insert("name".into(), Value::String(row.name.clone()));
    map.insert("kind".into(), Value::String(row.kind.clone()));
    Value::Object(map)
}

fn hit_value(hit: Hit) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), Value::String(hit.id));
    map.insert("kind".into(), Value::String(hit.kind));
    map.insert("name".into(), Value::String(hit.name));
    map.insert("filePath".into(), opt_string(hit.file_path.as_deref()));
    Value::Object(map)
}

fn hop_value(node: &NodeRec, via: Option<&str>, direction: Option<&str>) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), Value::String(node.id.clone()));
    map.insert("name".into(), Value::String(node.name.clone()));
    map.insert("kind".into(), Value::String(node.kind.clone()));
    map.insert("via".into(), opt_string(via));
    map.insert("direction".into(), opt_string(direction));
    Value::Object(map)
}

fn graph_payload(nodes: Vec<Value>, edges: Vec<Value>, cap: i64, hops: i64) -> Value {
    let mut map = Map::new();
    map.insert("nodes".into(), Value::Array(nodes));
    map.insert("edges".into(), Value::Array(edges));
    map.insert("cap".into(), Value::Number(cap.into()));
    map.insert("hops".into(), Value::Number(hops.into()));
    Value::Object(map)
}

fn opt_string(value: Option<&str>) -> Value {
    value
        .map(|text| Value::String(text.to_string()))
        .unwrap_or(Value::Null)
}

fn opt_i64(value: Option<i64>) -> Value {
    value
        .map(|n| Value::Number(n.into()))
        .unwrap_or(Value::Null)
}

fn sql_to_json(value: rusqlite::types::ValueRef<'_>) -> OnicResult<Value> {
    Ok(match value {
        rusqlite::types::ValueRef::Null => Value::Null,
        rusqlite::types::ValueRef::Integer(n) => Value::Number(n.into()),
        rusqlite::types::ValueRef::Real(n) => real_to_json(n),
        rusqlite::types::ValueRef::Text(bytes) => {
            Value::String(String::from_utf8_lossy(bytes).into_owned())
        }
        rusqlite::types::ValueRef::Blob(bytes) => {
            Value::String(String::from_utf8_lossy(bytes).into_owned())
        }
    })
}

fn real_to_json(n: f64) -> Value {
    if !n.is_finite() {
        return Value::Null;
    }
    if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
        return Value::Number((n as i64).into());
    }
    Number::from_f64(n)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

fn pretty(value: &Value) -> OnicResult<String> {
    Ok(serde_json::to_string_pretty(value)?)
}

fn parse_kinds(kinds: Option<&str>) -> Option<Vec<String>> {
    kinds.map(|raw| {
        raw.split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect()
    })
}

fn field_str(row: &Map<String, Value>, key: &str) -> String {
    row.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn count_lines(rows: &[(String, i64)]) -> String {
    if rows.is_empty() {
        "  (none)".into()
    } else {
        rows.iter()
            .map(|(k, n)| format!("  {k}\t{n}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::Store;
    use crate::schema::SCHEMA_SQL;

    #[test]
    fn sql_rejects_writes() {
        let file = tempfile::NamedTempFile::new().expect("temp");
        {
            let conn = rusqlite::Connection::open(file.path()).expect("db");
            conn.execute_batch(SCHEMA_SQL).expect("schema");
        }
        let store = Store::open(file.path()).expect("open");
        let msg = store
            .sql_json("INSERT INTO nodes (id, kind, name) VALUES ('file:a', 'file', 'a')")
            .expect_err("insert")
            .to_string();
        assert!(
            msg.contains("only a single read statement is allowed")
                || msg.to_lowercase().contains("readonly"),
            "{msg}"
        );
    }
}
