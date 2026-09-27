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

fn version_line() -> String {
    let (code, stdout, stderr) = run(&["--version"]);
    assert_eq!(code, 0, "{stderr}");
    stdout
}

fn assert_version_usage_failure(args: &[&str]) {
    let (code, stdout, stderr) = run(args);
    assert_ne!(code, 0, "stdout={stdout:?} stderr={stderr:?}");
    assert_ne!(stdout.trim(), "bee 0.1.0", "{stdout:?}");
    let combined = format!("{stdout}{stderr}").to_lowercase();
    assert!(
        combined.contains("unexpected") || combined.contains("unknown"),
        "{stdout}{stderr}"
    );
    assert!(combined.contains("usage"), "{stdout}{stderr}");
    assert!(combined.contains("version"), "{stdout}{stderr}");
}

#[test]
fn argv_version_short_v() {
    let expected = version_line();
    let (code, stdout, stderr) = run(&["-v"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, expected);
}

#[test]
fn argv_version_verb() {
    let expected = version_line();
    let (code, stdout, stderr) = run(&["version"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, expected);
}

#[test]
fn argv_version_after_project() {
    let expected = version_line();
    let (code, stdout, stderr) = run(&["--project", "x", "--version"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, expected);
}

#[test]
fn argv_version_after_wt() {
    let expected = version_line();
    let (code, stdout, stderr) = run(&["--wt", "x", "--version"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, expected);
}

#[test]
fn argv_version_short_v_after_project() {
    let expected = version_line();
    let (code, stdout, stderr) = run(&["--project", "x", "-v"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, expected);
}

#[test]
fn argv_version_reject_junk() {
    assert_version_usage_failure(&["--version", "--bogus"]);
    assert_version_usage_failure(&["version", "--bogus"]);
}
