#[test]
fn install_script_targets_bee_github_release() {
    let script = include_str!("../scripts/install.sh");
    assert!(script.contains("hembrow-innovations/bee"));
    assert!(script.contains("bee-${version}-${triple}.tar.gz"));
    assert!(script.contains("releases/download"));
    assert!(script.contains("${HOME}/.local/bin"));
}
