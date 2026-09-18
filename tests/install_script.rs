#[test]
fn install_script_targets_bee_github_release() {
    let script = include_str!("../scripts/install.sh");
    assert!(script.contains("hembrow-innovations/bee"));
    assert!(script.contains("bee-${version}-${triple}.tar.gz"));
    assert!(script.contains("releases/download"));
    assert!(script.contains("${HOME}/.local/bin"));
    assert!(script.contains("aarch64-apple-darwin"));
    assert!(
        !script.contains("unknown-linux-gnu"),
        "MacOS-first release must not advertise Linux triples"
    );
}
