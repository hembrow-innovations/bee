use std::ffi::{OsStr, OsString};
use std::process::{Command, ExitCode};

const TRACKER: &[&str] = &["next-id", "check-ids", "claim", "status", "housekeep"];
const VAULT: &[&str] = &[
    "home", "ls", "read", "search", "recent", "write", "append", "patch", "rm", "mv", "links",
    "tags", "vault", "new",
];

fn remap_heio_argv(args: impl IntoIterator<Item = impl Into<OsString>>) -> Vec<OsString> {
    let mut args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let Some(first) = args.first().and_then(|s| s.to_str()) else {
        return args;
    };
    let noun = if TRACKER.contains(&first) {
        "note"
    } else if VAULT.contains(&first) {
        "docs"
    } else {
        return args;
    };
    args.insert(0, OsString::from(noun));
    args
}

fn exec_bee(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> ExitCode {
    let args: Vec<_> = args.into_iter().collect();
    let mut bee = std::env::current_exe().unwrap_or_else(|_| "bee".into());
    bee.set_file_name("bee");
    match Command::new(&bee).args(&args).status() {
        Ok(st) => ExitCode::from(st.code().unwrap_or(1) as u8),
        Err(_) => match Command::new("bee").args(&args).status() {
            Ok(st) => ExitCode::from(st.code().unwrap_or(1) as u8),
            Err(_) => ExitCode::from(1),
        },
    }
}

pub fn forward_old_bin(old_name: &str) -> ExitCode {
    eprintln!("{old_name} renamed to bee; forwarding argv to bee");
    exec_bee(std::env::args_os().skip(1))
}

pub fn forward_heio_bin() -> ExitCode {
    eprintln!("heio renamed to bee; forwarding argv to bee");
    exec_bee(remap_heio_argv(std::env::args_os().skip(1)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn remap(args: &[&str]) -> Vec<String> {
        remap_heio_argv(args.iter().map(|s| OsString::from(*s)))
            .into_iter()
            .map(|s| s.into_string().unwrap())
            .collect()
    }

    #[test]
    fn wrappers_are_not_silent() {
        let hivemind = include_str!("bin/hivemind.rs");
        let odm = include_str!("bin/odm.rs");
        assert!(hivemind.contains("forward_old_bin"));
        assert!(odm.contains("forward_old_bin"));
        let src = include_str!("forward.rs");
        assert!(src.contains("renamed to bee"));
        assert!(src.contains("bee"));
    }

    #[test]
    fn heio_status_remaps_to_note_status() {
        assert_eq!(
            remap(&["status", "task-01-x", "claimed"]),
            ["note", "status", "task-01-x", "claimed"]
        );
    }

    #[test]
    fn heio_search_remaps_to_docs_search() {
        assert_eq!(remap(&["search"]), ["docs", "search"]);
    }

    #[test]
    fn heio_next_id_remaps_to_note_next_id() {
        assert_eq!(remap(&["next-id", "task"]), ["note", "next-id", "task"]);
    }

    #[test]
    fn heio_tracker_verbs_remap_to_note() {
        for verb in ["next-id", "check-ids", "claim", "status", "housekeep"] {
            assert_eq!(remap(&[verb]), ["note", verb]);
        }
    }

    #[test]
    fn heio_vault_verbs_remap_to_docs() {
        for verb in [
            "home", "ls", "read", "search", "recent", "write", "append", "patch", "rm", "mv",
            "links", "tags", "vault", "new",
        ] {
            assert_eq!(remap(&[verb]), ["docs", verb]);
        }
    }

    #[test]
    fn heio_wrapper_is_not_silent() {
        let heio = include_str!("bin/heio.rs");
        assert!(heio.contains("forward_heio_bin"));
        let cargo = include_str!("../Cargo.toml");
        assert!(cargo.contains("name = \"heio\""));
        let src = include_str!("forward.rs");
        assert!(src.contains("heio renamed to bee"));
    }
}
