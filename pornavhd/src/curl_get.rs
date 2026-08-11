//! Sole process boundary for `curl` HTML GET (bot-blocked sites).
//! Semgrep allows process spawn here among site crates (with ytdlp.rs).

use anyhow::{bail, Context, Result};
use tokio::process::Command;
use tracing::debug;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// GET URL as text via system curl (follows redirects).
pub async fn get_text(url: &str, referer: Option<&str>) -> Result<String> {
    let mut cmd = Command::new("curl");
    cmd.arg("-fsSL")
        .arg("--max-time")
        .arg("60")
        .arg("-A")
        .arg(UA)
        .arg("-H")
        .arg("Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .arg("-H")
        .arg("Accept-Language: en-US,en;q=0.9,ja;q=0.8");
    if let Some(r) = referer {
        cmd.arg("-e").arg(r);
    }
    cmd.arg(url);

    debug!(%url, "curl GET");
    let out = cmd.output().await.context("spawn curl")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("curl GET {url} failed: {err}");
    }
    String::from_utf8(out.stdout).context("curl body utf-8")
}
