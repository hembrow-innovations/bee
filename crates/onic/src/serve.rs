use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

use serde_json::{Map, Value};

use crate::error::{OnicError, OnicResult};
use crate::store::Store;

const TEXT: &str = "text/plain; charset=utf-8";
const JSON: &str = "application/json; charset=utf-8";
const HTML: &str = "text/html; charset=utf-8";
const JS: &str = "text/javascript; charset=utf-8";
const CSS: &str = "text/css; charset=utf-8";
const INDEX_HTML: &str = include_str!("../assets/index.html");
const APP_JS: &str = include_str!("../assets/app.js");
const STYLE_CSS: &str = include_str!("../assets/style.css");

pub fn serve(db: &Path, port: u16) -> OnicResult<()> {
    if port == 0 {
        return Err(OnicError::msg("invalid port"));
    }
    let store = Store::open(db)?;
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    for conn in listener.incoming() {
        let Ok(mut stream) = conn else {
            continue;
        };
        let _ = respond(&store, &mut stream);
    }
    Ok(())
}

pub fn handle_request(store: &Store, method: &str, target: &str) -> (u16, String, &'static str) {
    let _ = method;
    let (path_raw, query, hash) = split_target(target);
    let path = percent_decode(path_raw);
    let params = api_params(query, hash);
    if path == "/api/schema" {
        return match store.schema_text() {
            Ok(text) => (200, text, TEXT),
            Err(err) => (500, err.to_string(), TEXT),
        };
    }
    if path == "/api/search" {
        return match store.search_json(params.get("q").unwrap_or(""), 30) {
            Ok(body) => (200, body, JSON),
            Err(err) => (500, err.to_string(), TEXT),
        };
    }
    if path == "/api/node" {
        let Some(id) = params.get("id").filter(|id| !id.is_empty()) else {
            return (400, error_json("id required"), JSON);
        };
        return match store.explain_json(id) {
            Ok(body) => (200, body, JSON),
            Err(err) => {
                let message = err.to_string();
                let status = if message.starts_with("no node named") {
                    404
                } else {
                    400
                };
                (status, error_json(&message), JSON)
            }
        };
    }
    if path == "/api/subgraph" {
        let hops = js_clamp(params.get("hops"), 1, 0, 3);
        let limit = js_clamp(params.get("limit"), 200, 1, 500);
        let kinds = params.get("kinds");
        return match store.subgraph_json(params.get("seed"), params.get("q"), kinds, hops, limit) {
            Ok(body) => (200, body, JSON),
            Err(err) => (400, error_json(&err.to_string()), JSON),
        };
    }
    match asset(&path) {
        Some((body, ctype)) => (200, body.to_string(), ctype),
        None => (404, "not found".to_string(), TEXT),
    }
}

fn respond(store: &Store, stream: &mut TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(Duration::from_secs(15)))?;
    let Some((method, target)) = read_head(stream)? else {
        return Ok(());
    };
    let (status, body, ctype) = handle_request(store, &method, &target);
    let bytes = body.as_bytes();
    let header = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reason(status),
        bytes.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(bytes)?;
    stream.flush()
}

fn read_head(stream: &mut TcpStream) -> std::io::Result<Option<(String, String)>> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    loop {
        if buf.len() > 64 * 1024 {
            return Ok(None);
        }
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.windows(2).any(|w| w == b"\n\n")
                {
                    break;
                }
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                return Ok(None);
            }
            Err(err) => return Err(err),
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let mut parts = text.lines().next().unwrap_or("").split_whitespace();
    let Some(method) = parts.next() else {
        return Ok(None);
    };
    let Some(target) = parts.next() else {
        return Ok(None);
    };
    Ok(Some((method.to_string(), target.to_string())))
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Error",
    }
}

fn asset(path: &str) -> Option<(&'static str, &'static str)> {
    let rel = path.trim_start_matches('/');
    if rel.contains("..") || rel.contains('\\') || rel.contains('\0') {
        return None;
    }
    match rel {
        "" | "index.html" => Some((INDEX_HTML, HTML)),
        "app.js" => Some((APP_JS, JS)),
        "style.css" => Some((STYLE_CSS, CSS)),
        _ => None,
    }
}

fn split_target(target: &str) -> (&str, &str, &str) {
    let (before_hash, hash) = match target.find('#') {
        Some(i) => (&target[..i], &target[i..]),
        None => (target, ""),
    };
    match before_hash.find('?') {
        Some(i) => (&before_hash[..i], &before_hash[i + 1..], hash),
        None => (before_hash, "", hash),
    }
}

struct Params {
    pairs: Vec<(String, String)>,
}

impl Params {
    fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    fn set(&mut self, key: &str, value: String) {
        if let Some(slot) = self.pairs.iter_mut().find(|(name, _)| name == key) {
            slot.1 = value;
        } else {
            self.pairs.push((key.to_string(), value));
        }
    }

    fn has(&self, key: &str) -> bool {
        self.pairs.iter().any(|(name, _)| name == key)
    }
}

fn api_params(query: &str, hash: &str) -> Params {
    let mut params = parse_query(query);
    if hash.len() <= 1 {
        return params;
    }
    let rest = &hash[1..];
    let (fragment, extra) = match rest.find('&') {
        Some(amp) => (&rest[..amp], &rest[amp + 1..]),
        None => (rest, ""),
    };
    let fragment = percent_decode(fragment);
    if !fragment.is_empty() {
        let existing: Vec<(String, String)> = ["id", "seed", "q"]
            .into_iter()
            .filter_map(|key| {
                params
                    .get(key)
                    .map(|value| (key.to_string(), value.to_string()))
            })
            .collect();
        for (key, value) in existing {
            if !value.contains('#') {
                params.set(&key, format!("{value}#{fragment}"));
            }
        }
    }
    if !extra.is_empty() {
        for (key, value) in parse_query(extra).pairs {
            if !params.has(&key) {
                params.set(&key, value);
            }
        }
    }
    params
}

fn parse_query(query: &str) -> Params {
    let mut pairs = Vec::new();
    if !query.is_empty() {
        for part in query.split('&') {
            if part.is_empty() {
                continue;
            }
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            pairs.push((decode_query(key), decode_query(value)));
        }
    }
    Params { pairs }
}

fn decode_query(input: &str) -> String {
    percent_decode(&input.replace('+', " "))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn js_clamp(raw: Option<&str>, default: i64, min: i64, max: i64) -> i64 {
    let Some(raw) = raw else {
        return default.clamp(min, max);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return 0_i64.clamp(min, max);
    }
    let Ok(value) = trimmed.parse::<f64>() else {
        return min;
    };
    if !value.is_finite() {
        return min;
    }
    let trunc = value.trunc();
    if trunc >= i64::MAX as f64 {
        return max;
    }
    if trunc <= i64::MIN as f64 {
        return min;
    }
    (trunc as i64).clamp(min, max)
}

fn error_json(message: &str) -> String {
    let mut map = Map::new();
    map.insert("error".into(), Value::String(message.to_string()));
    serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_else(|_| {
        format!(
            "{{\"error\":{}}}",
            serde_json::to_string(message).unwrap_or_else(|_| "\"error\"".to_string())
        )
    })
}

#[cfg(test)]
mod tests {
    use super::handle_request;
    use crate::schema::SCHEMA_SQL;
    use crate::store::Store;

    #[test]
    fn serve_subgraph_caps_nodes() {
        let file = tempfile::NamedTempFile::new().expect("temp");
        {
            let conn = rusqlite::Connection::open(file.path()).expect("db");
            conn.execute_batch(SCHEMA_SQL).expect("schema");
            conn.execute(
                "INSERT INTO nodes (id, kind, name) VALUES ('file:a.ts', 'file', 'a.ts')",
                [],
            )
            .expect("node");
        }
        let store = Store::open(file.path()).expect("open");
        let (status, body, _) = handle_request(&store, "GET", "/api/subgraph");
        assert_eq!(status, 200);
        assert!(body.contains("nodes"), "{body}");
        assert!(body.contains("cap"), "{body}");
        let bytes = std::fs::read(file.path()).expect("bytes");
        assert_ne!(body.as_bytes(), bytes.as_slice());
    }
}
