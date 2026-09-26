use std::path::{Path, PathBuf};

use hive_onic::{
    build_project, find_project_root, resolve_db, serve, watch_project, Store, DEFAULT_PORT,
};

use crate::cli::OnicCmd;

pub fn run(
    cwd: &Path,
    db: Option<&Path>,
    root_flag: Option<&Path>,
    port: Option<u16>,
    cmd: &OnicCmd,
) -> Result<String, String> {
    match cmd {
        OnicCmd::Build { dir, watch } => {
            let project = project_dir(cwd, dir.as_deref(), root_flag);
            let db_path = resolve_db(&project, db, Some(&project));
            let report = build_project(&project, &db_path).map_err(|e| e.to_string())?;
            let verb = if report.reused { "reused" } else { "wrote" };
            let line = format!(
                "{verb} {} ({} files, {} nodes, {} edges)",
                report.db_path.display(),
                report.file_count,
                report.node_count,
                report.edge_count
            );
            if *watch {
                println!("{line}");
                watch_project(&project, &db_path).map_err(|e| e.to_string())?;
                return Ok(String::new());
            }
            Ok(line)
        }
        OnicCmd::Serve => {
            let project = project_dir(cwd, None, root_flag);
            let db_path = resolve_db(cwd, db, Some(&project));
            let port = port.unwrap_or(DEFAULT_PORT);
            println!(
                "onic viewer on http://127.0.0.1:{port}  (db {})",
                db_path.display()
            );
            serve(&db_path, port).map_err(|e| e.to_string())?;
            Ok(String::new())
        }
        other => {
            let project = project_dir(cwd, None, root_flag);
            let db_path = resolve_db(cwd, db, Some(&project));
            let store = Store::open(&db_path).map_err(|e| missing_graph(&db_path, &project, e.to_string()))?;
            match other {
                OnicCmd::Schema => store.schema_text().map_err(|e| e.to_string()),
                OnicCmd::Sql { query } => {
                    if query.trim().is_empty() {
                        return Err("usage: bee onic sql \"SELECT ...\"".to_string());
                    }
                    store.sql_json(query).map_err(|e| e.to_string())
                }
                OnicCmd::Search { text } => {
                    if text.trim().is_empty() {
                        return Err("usage: bee onic search <text>".to_string());
                    }
                    store.search_json(text, 20).map_err(|e| e.to_string())
                }
                OnicCmd::Explain { name } => store.explain_json(name).map_err(|e| e.to_string()),
                OnicCmd::Compact { name } => store.compact_json(name).map_err(|e| e.to_string()),
                OnicCmd::Neighbors { name } => store.neighbors_json(name).map_err(|e| e.to_string()),
                OnicCmd::Path { from, to } => store.path_json(from, to).map_err(|e| e.to_string()),
                OnicCmd::Build { .. } | OnicCmd::Serve => unreachable!(),
            }
        }
    }
}

fn project_dir(cwd: &Path, dir: Option<&Path>, root_flag: Option<&Path>) -> PathBuf {
    if let Some(dir) = dir {
        return if dir.is_absolute() { dir.to_path_buf() } else { cwd.join(dir) };
    }
    if let Some(root) = root_flag {
        return if root.is_absolute() { root.to_path_buf() } else { cwd.join(root) };
    }
    find_project_root(cwd)
}

fn missing_graph(db: &Path, root: &Path, err: String) -> String {
    if err.contains("no graph") || err.contains("not ported") {
        return format!("no graph at {}. run: bee onic build {}", db.display(), root.display());
    }
    err
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use crate::cli::Cli;

    #[test]
    fn onic_help_lists_graph_commands() {
        let help = Cli::command()
            .find_subcommand("onic")
            .expect("onic")
            .clone()
            .render_help()
            .to_string();
        for name in [
            "build", "schema", "sql", "search", "explain", "compact", "neighbors", "path", "serve",
        ] {
            assert!(help.contains(name), "missing {name} in {help}");
        }
    }
}
