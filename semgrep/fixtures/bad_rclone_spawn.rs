// Negative fixture: architecture rules MUST flag this file.
// Not compiled by Cargo. Used only by scripts/semgrep-test.sh.

fn bad_spawn() {
    let _ = std::process::Command::new("rclone");
}
