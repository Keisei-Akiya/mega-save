fn bad_site_spawns() {
    let _ = tokio::process::Command::new("synthetic-tool");
    let _ = std::process::Command::new("synthetic-tool");
}
