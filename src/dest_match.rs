use std::collections::BTreeMap;
use std::path::Path;

use serde_yaml::Value;

use crate::dest_config::Lane;
use crate::dest_note::YamlMap;
use crate::dest_scan::ScannedNote;

#[derive(Debug, Clone)]
pub struct Match {
    pub lane: Lane,
    pub note: ScannedNote,
}

pub fn match_notes(
    lanes: &[Lane],
    notes: &[ScannedNote],
    disable: &[String],
    cwd: Option<&Path>,
) -> Vec<Match> {
    let disabled: std::collections::BTreeSet<&str> = disable.iter().map(String::as_str).collect();
    let by_id = notes_by_id(notes);
    let mut matches = Vec::new();
    for lane in lanes {
        if disabled.contains(lane.lane.as_str()) {
            continue;
        }
        for note in notes {
            if !matches_predicates(&note.front_matter, &lane.trigger) {
                continue;
            }
            if let Some(need) = &lane.need {
                if !matches_need(need, &note.front_matter, cwd, &by_id) {
                    continue;
                }
            }
            matches.push(Match {
                lane: lane.clone(),
                note: note.clone(),
            });
        }
    }
    matches
}

fn notes_by_id(notes: &[ScannedNote]) -> BTreeMap<String, ScannedNote> {
    let mut by_id = BTreeMap::new();
    for note in notes {
        if let Some(Value::String(id)) = note.front_matter.get(Value::String("id".into())) {
            if !id.is_empty() {
                by_id.entry(id.clone()).or_insert_with(|| note.clone());
            }
        }
    }
    by_id
}

fn matches_need(
    need: &BTreeMap<String, Value>,
    front: &YamlMap,
    cwd: Option<&Path>,
    by_id: &BTreeMap<String, ScannedNote>,
) -> bool {
    let mut scalars = BTreeMap::new();
    for (key, expected) in need {
        match key.as_str() {
            "exists" => {
                if !paths_present(cwd, expected, true) {
                    return false;
                }
            }
            "absent" => {
                if !paths_present(cwd, expected, false) {
                    return false;
                }
            }
            "status-of" => {
                if !status_of_matches(expected, front, by_id) {
                    return false;
                }
            }
            _ => {
                scalars.insert(key.clone(), expected.clone());
            }
        }
    }
    matches_predicates(front, &scalars)
}

fn status_of_matches(
    value: &Value,
    front: &YamlMap,
    by_id: &BTreeMap<String, ScannedNote>,
) -> bool {
    let Value::Mapping(map) = value else {
        return false;
    };
    for (field, expected) in map {
        let Some(field) = field.as_str() else {
            return false;
        };
        for id in id_list(front.get(Value::String(field.into()))) {
            let Some(target) = by_id.get(&id) else {
                return false;
            };
            if target.front_matter.get(Value::String("status".into())) != Some(expected) {
                return false;
            }
        }
    }
    true
}

fn id_list(value: Option<&Value>) -> Vec<String> {
    match value {
        None | Some(Value::Null) => vec![],
        Some(Value::String(s)) if s.is_empty() || s == "none" => vec![],
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Sequence(s)) => s
            .iter()
            .filter_map(|v| v.as_str())
            .filter(|s| !s.is_empty() && *s != "none")
            .map(str::to_string)
            .collect(),
        _ => vec![],
    }
}

fn paths_present(cwd: Option<&Path>, value: &Value, want: bool) -> bool {
    let Some(cwd) = cwd else {
        return false;
    };
    let Some(paths) = path_list(value) else {
        return false;
    };
    paths.iter().all(|rel| cwd.join(rel).exists() == want)
}

fn path_list(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::String(s) => Some(vec![s.clone()]),
        Value::Sequence(s) => {
            let mut paths = Vec::new();
            for item in s {
                paths.push(item.as_str()?.to_string());
            }
            Some(paths)
        }
        _ => None,
    }
}

fn matches_predicates(front: &YamlMap, predicates: &BTreeMap<String, Value>) -> bool {
    predicates
        .iter()
        .all(|(k, expected)| front.get(Value::String(k.clone())) == Some(expected))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dest_config::Lane;
    use serde_yaml::Mapping;
    use std::path::PathBuf;

    fn note(status: &str, extra: &[(&str, &str)]) -> ScannedNote {
        let mut map = Mapping::new();
        map.insert(
            Value::String("status".into()),
            Value::String(status.into()),
        );
        for (k, v) in extra {
            map.insert(Value::String((*k).into()), Value::String((*v).into()));
        }
        ScannedNote {
            path: "n.md".into(),
            abs: PathBuf::from("n.md"),
            front_matter: map,
        }
    }

    fn lane(trigger_status: &str) -> Lane {
        let mut trigger = BTreeMap::new();
        trigger.insert(
            "status".into(),
            Value::String(trigger_status.into()),
        );
        Lane {
            lane: "work".into(),
            trigger,
            need: None,
            claim_status: "claimed".into(),
        }
    }

    #[test]
    fn trigger_all_keys_ignores_extra() {
        let n = note("ready", &[("title", "x")]);
        let got = match_notes(&[lane("ready")], &[n], &[], None);
        assert_eq!(got.len(), 1);
        let n = note("ready-for-human", &[]);
        assert!(match_notes(&[lane("ready-for-agent")], &[n], &[], None).is_empty());
    }

    #[test]
    fn disable_omits_lane() {
        let n = note("ready", &[]);
        assert!(match_notes(&[lane("ready")], &[n], &["work".into()], None).is_empty());
    }

    #[test]
    fn need_exists_skips_without_fault() {
        let mut l = lane("ready");
        let mut need = BTreeMap::new();
        need.insert("exists".into(), Value::String("missing.txt".into()));
        l.need = Some(need);
        let n = note("ready", &[]);
        assert!(match_notes(&[l], &[n], &[], Some(Path::new("/tmp"))).is_empty());
    }
}
