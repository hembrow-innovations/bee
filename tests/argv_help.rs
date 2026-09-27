fn run(args: &[&str]) -> (i32, String, String) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_bee"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn help_stdout() -> String {
    let (code, stdout, stderr) = run(&["--help"]);
    assert_eq!(code, 0, "{stderr}");
    stdout
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

fn usage_line(text: &str) -> &str {
    text.lines()
        .find(|line| line.trim().starts_with("Usage:"))
        .unwrap_or("")
}

#[test]
fn argv_help_verb_matches_long_flag() {
    let expected = help_stdout();
    let (code, stdout, stderr) = run(&["help"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(first_line(&stdout), first_line(&expected));
    assert_eq!(usage_line(&stdout), usage_line(&expected));
}

fn status_help_stdout() -> String {
    let (code, stdout, stderr) = run(&["status", "--help"]);
    assert_eq!(code, 0, "{stderr}");
    stdout
}

#[test]
fn argv_help_status_prints_status_help() {
    let expected = status_help_stdout();
    let root = help_stdout();
    let (code, stdout, stderr) = run(&["help", "status"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(first_line(&stdout), first_line(&expected));
    assert_eq!(usage_line(&stdout), usage_line(&expected));
    assert_ne!(usage_line(&stdout), usage_line(&root));
}

#[test]
fn argv_long_help_status_prints_status_help() {
    let expected = status_help_stdout();
    let root = help_stdout();
    let (code, stdout, stderr) = run(&["--help", "status"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(first_line(&stdout), first_line(&expected));
    assert_eq!(usage_line(&stdout), usage_line(&expected));
    assert_ne!(usage_line(&stdout), usage_line(&root));
}

#[test]
fn argv_help_short_h() {
    let (code, _, stderr) = run(&["-h"]);
    assert_eq!(code, 0, "{stderr}");
}

#[test]
fn argv_root_help_footer_names_help_command() {
    let stdout = help_stdout();
    let footer = stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    assert!(
        footer.contains("bee help <command>"),
        "{stdout}"
    );
}
