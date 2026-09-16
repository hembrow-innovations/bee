use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde_yaml::Value;

use crate::lanes_path;

const CONFIG_KEYS: &[&str] = &[
    "watch", "folders", "lanes", "disable", "history", "actors", "stop",
];

#[derive(Debug, Clone)]
pub struct DestConfig {
    pub folders: Vec<Value>,
    pub lanes: Vec<Lane>,
    pub disable: Vec<String>,
    pub watch: Option<Vec<String>>,
    pub history: Option<String>,
}

#[derive(Debug, Clone)]
pub enum CmdSpec {
    String(String),
    List(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct Lane {
    pub lane: String,
    pub trigger: BTreeMap<String, Value>,
    pub need: Option<BTreeMap<String, Value>>,
    pub claim_status: String,
    pub cmds: Vec<CmdSpec>,
}

pub fn load_dest_config(cwd: &Path) -> Result<DestConfig, String> {
    let file = lanes_path(cwd);
    if !file.is_file() {
        return Err("Missing .hivemind/hivemind.yaml".into());
    }
    let raw: Value = serde_yaml::from_str(&fs::read_to_string(&file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let Value::Mapping(map) = raw else {
        return Err("config must be a map".into());
    };
    for key in map.keys() {
        let Some(name) = key.as_str() else {
            return Err("unknown key".into());
        };
        if !CONFIG_KEYS.contains(&name) {
            return Err(format!("Unknown key \"{name}\""));
        }
    }
    let folders = match map.get(Value::String("folders".into())) {
        Some(Value::Sequence(s)) => s.clone(),
        None => vec![],
        _ => return Err("folders must be a list".into()),
    };
    let lanes = parse_lanes(map.get(Value::String("lanes".into())))?;
    let disable = parse_string_list(map.get(Value::String("disable".into())))?;
    let watch = match map.get(Value::String("watch".into())) {
        None => None,
        Some(v) => Some(parse_string_list(Some(v))?),
    };
    let history = map
        .get(Value::String("history".into()))
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(DestConfig {
        folders,
        lanes,
        disable,
        watch,
        history,
    })
}

fn parse_lanes(value: Option<&Value>) -> Result<Vec<Lane>, String> {
    let Some(value) = value else {
        return Err("\"lanes\" is required".into());
    };
    if value.is_sequence() {
        return Err("\"lanes\" must be a map".into());
    }
    let Value::Mapping(map) = value else {
        return Err("\"lanes\" must be a map".into());
    };
    let mut lanes = Vec::new();
    for (k, item) in map {
        let id = k.as_str().ok_or("lane id is required")?.to_string();
        if id.is_empty() {
            return Err("lane id is required".into());
        }
        let Value::Mapping(item) = item else {
            return Err(format!("lane \"{id}\" must be a map"));
        };
        let type_ = item
            .get(Value::String("type".into()))
            .and_then(Value::as_str)
            .unwrap_or("");
        if type_ != "single" && type_ != "pipeline" {
            if type_.is_empty() {
                return Err(format!("lane \"{id}\" is missing type"));
            }
            return Err(format!("lane \"{id}\" has unknown type \"{type_}\""));
        }
        let trigger = as_map(item.get(Value::String("trigger".into())))?;
        let need = match item.get(Value::String("need".into())) {
            None => None,
            Some(v) => Some(as_map(Some(v))?),
        };
        let claim_status = item
            .get(Value::String("claim-status".into()))
            .and_then(Value::as_str)
            .unwrap_or("claimed")
            .to_string();
        let cmds = parse_cmds(type_, item)?;
        lanes.push(Lane {
            lane: id,
            trigger,
            need,
            claim_status,
            cmds,
        });
    }
    Ok(lanes)
}

fn parse_cmds(type_: &str, item: &serde_yaml::Mapping) -> Result<Vec<CmdSpec>, String> {
    if type_ == "pipeline" {
        let Some(Value::Sequence(stages)) = item.get(Value::String("stages".into())) else {
            return Ok(vec![]);
        };
        let mut cmds = Vec::new();
        for stage in stages {
            let Value::Mapping(stage) = stage else {
                continue;
            };
            if let Some(cmd) = parse_cmd(stage.get(Value::String("cmd".into()))) {
                cmds.push(cmd);
            }
        }
        return Ok(cmds);
    }
    Ok(parse_cmd(item.get(Value::String("cmd".into()))).into_iter().collect())
}

fn parse_cmd(value: Option<&Value>) -> Option<CmdSpec> {
    match value {
        Some(Value::String(s)) => Some(CmdSpec::String(s.clone())),
        Some(Value::Sequence(s)) => Some(CmdSpec::List(
            s.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
        )),
        _ => None,
    }
}

fn as_map(value: Option<&Value>) -> Result<BTreeMap<String, Value>, String> {
    let Some(Value::Mapping(m)) = value else {
        return Ok(BTreeMap::new());
    };
    let mut out = BTreeMap::new();
    for (k, v) in m {
        if let Some(s) = k.as_str() {
            out.insert(s.to_string(), v.clone());
        }
    }
    Ok(out)
}

fn parse_string_list(value: Option<&Value>) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(vec![]);
    };
    match value {
        Value::Sequence(s) => s
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "list must be strings".to_string())
            })
            .collect(),
        Value::String(s) => Ok(vec![s.clone()]),
        _ => Err("list must be strings".into()),
    }
}

pub fn named_allowlist(name: &str) -> Result<BTreeSet<String>, String> {
    if name == "quarantine" {
        return Ok(BTreeSet::from([
            "origin-location".into(),
            "quarantined-at".into(),
            "fault".into(),
        ]));
    }
    Err(format!("Unknown folder schema \"{name}\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn missing_yaml_fails() {
        let dir = tempdir().unwrap();
        assert!(load_dest_config(dir.path()).unwrap_err().contains("Missing"));
    }

    #[test]
    fn ignores_root_hivemind_yaml() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("hivemind.yaml"), "lanes: {}\n").unwrap();
        assert!(load_dest_config(dir.path()).is_err());
    }

    #[test]
    fn reads_only_hivemind_dir_file() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".hivemind")).unwrap();
        fs::write(
            dir.path().join(".hivemind/hivemind.yaml"),
            "folders: []\nlanes: {}\n",
        )
        .unwrap();
        fs::write(dir.path().join("hivemind.yaml"), "lanes: boom\n").unwrap();
        let cfg = load_dest_config(dir.path()).unwrap();
        assert!(cfg.lanes.is_empty());
    }
}
