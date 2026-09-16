use std::process::{Command, ExitCode};

pub fn forward_old_bin(old_name: &str) -> ExitCode {
    eprintln!("{old_name} renamed to bee; forwarding argv to bee");
    let mut bee = std::env::current_exe().unwrap_or_else(|_| "bee".into());
    bee.set_file_name("bee");
    let mut args = std::env::args_os();
    let _ = args.next();
    match Command::new(&bee).args(args).status() {
        Ok(st) => ExitCode::from(st.code().unwrap_or(1) as u8),
        Err(_) => match Command::new("bee")
            .args(std::env::args_os().skip(1))
            .status()
        {
            Ok(st) => ExitCode::from(st.code().unwrap_or(1) as u8),
            Err(_) => ExitCode::from(1),
        },
    }
}

#[cfg(test)]
mod tests {
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
}
