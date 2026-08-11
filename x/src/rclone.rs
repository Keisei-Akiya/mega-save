//! Upload via external rclone binary (MEGA remote already configured).

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tracing::info;

pub async fn ensure_remote_ok(rclone_bin: &str, remote_hint: &str) -> Result<()> {
    // remote_hint like mega:video/r18/0 → listremotes / lsd root
    let remote_name = remote_hint
        .split_once(':')
        .map(|(a, _)| format!("{a}:"))
        .unwrap_or_else(|| "mega:".into());

    let out = Command::new(rclone_bin)
        .arg("lsd")
        .arg(&remote_name)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .with_context(|| format!("spawn {rclone_bin}"))?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("rclone lsd {remote_name} failed: {err}");
    }
    Ok(())
}

pub async fn mkdir_p(rclone_bin: &str, remote_path: &str) -> Result<()> {
    // rclone mkdir is fine if exists
    let out = Command::new(rclone_bin)
        .arg("mkdir")
        .arg(remote_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .context("rclone mkdir")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        // some remotes error on exists — ignore soft failures with empty stderr patterns later if needed
        if !err.is_empty() {
            tracing::debug!(%err, "rclone mkdir stderr");
        }
    }
    Ok(())
}

pub async fn copy_file(rclone_bin: &str, local: &Path, remote_dir: &str) -> Result<()> {
    info!(
        from = %local.display(),
        to = %remote_dir,
        "rclone copy"
    );
    let status = Command::new(rclone_bin)
        .arg("copy")
        .arg(local)
        .arg(remote_dir)
        .arg("-P")
        .arg("--checkers")
        .arg("4")
        .arg("--transfers")
        .arg("2")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .context("rclone copy")?;

    if !status.success() {
        bail!("rclone copy failed with {status}");
    }
    Ok(())
}

pub async fn remote_size(rclone_bin: &str, remote_dir: &str, filename: &str) -> Result<Option<u64>> {
    let out = Command::new(rclone_bin)
        .arg("lsl")
        .arg(remote_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .context("rclone lsl")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("rclone lsl failed: {err}");
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        // format: "  SIZE YYYY-MM-DD HH:MM:SS.nnnnnnnnn name"
        if line.contains(filename) {
            let size_str = line.split_whitespace().next().unwrap_or("");
            if let Ok(n) = size_str.parse::<u64>() {
                return Ok(Some(n));
            }
        }
    }
    Ok(None)
}
