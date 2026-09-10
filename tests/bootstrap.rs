use std::process::Command;

#[test]
fn version_flag_starts_and_exits_cleanly() {
    let output = Command::new(env!("CARGO_BIN_EXE_clawcode"))
        .arg("--version")
        .output()
        .expect("clawcode binary should start");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "clawcode 0.1.0"
    );
    assert!(output.stderr.is_empty());
}
