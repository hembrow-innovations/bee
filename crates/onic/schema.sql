PRAGMA foreign_keys = ON;
PRAGMA journal_mode = DELETE;

CREATE TABLE meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE files (
  path TEXT PRIMARY KEY,
  lang TEXT,
  hash TEXT NOT NULL,
  mtime INTEGER NOT NULL,
  size INTEGER NOT NULL
);

CREATE TABLE nodes (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  file_path TEXT,
  start_line INTEGER,
  end_line INTEGER,
  body TEXT,
  props TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE edges (
  id INTEGER PRIMARY KEY,
  src TEXT NOT NULL,
  dst TEXT NOT NULL,
  kind TEXT NOT NULL,
  confidence TEXT NOT NULL DEFAULT 'extracted',
  props TEXT NOT NULL DEFAULT '{}',
  FOREIGN KEY (src) REFERENCES nodes(id),
  FOREIGN KEY (dst) REFERENCES nodes(id)
);

CREATE INDEX nodes_kind ON nodes(kind);
CREATE INDEX nodes_name ON nodes(name);
CREATE INDEX nodes_file ON nodes(file_path);
CREATE INDEX edges_src ON edges(src);
CREATE INDEX edges_dst ON edges(dst);
CREATE INDEX edges_kind ON edges(kind);

CREATE VIRTUAL TABLE nodes_fts USING fts5(
  name,
  body,
  content='nodes',
  content_rowid='rowid'
);

CREATE TRIGGER nodes_ai AFTER INSERT ON nodes BEGIN
  INSERT INTO nodes_fts(rowid, name, body) VALUES (new.rowid, new.name, new.body);
END;

CREATE TRIGGER nodes_ad AFTER DELETE ON nodes BEGIN
  INSERT INTO nodes_fts(nodes_fts, rowid, name, body) VALUES ('delete', old.rowid, old.name, old.body);
END;

CREATE TRIGGER nodes_au AFTER UPDATE ON nodes BEGIN
  INSERT INTO nodes_fts(nodes_fts, rowid, name, body) VALUES ('delete', old.rowid, old.name, old.body);
  INSERT INTO nodes_fts(rowid, name, body) VALUES (new.rowid, new.name, new.body);
END;
