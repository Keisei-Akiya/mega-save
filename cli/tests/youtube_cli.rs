use std::process::Command;

#[test]
fn youtube_help_describes_mp3_upload_to_a_remote_path() {
    let output = Command::new(env!("CARGO_BIN_EXE_mega-save"))
        .args(["youtube", "--help"])
        .output()
        .expect("run mega-save youtube --help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 help output");
    assert!(stdout.contains("YouTube"));
    assert!(stdout.contains("mp3"));
    assert!(stdout.contains("Destination remote path"));
}
