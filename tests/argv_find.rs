fn run_in(dir: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_bee"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn argv_find_missing_catalog_prints_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let (code, stdout, stderr) = run_in(dir.path(), &["find"]);
    assert_ne!(code, 0, "stdout={stdout:?} stderr={stderr:?}");
    assert!(
        stderr.contains("not a Workbench: missing"),
        "{stderr:?}"
    );
    assert!(
        stderr.contains(".hivemind/workbench.yaml"),
        "{stderr:?}"
    );
}
