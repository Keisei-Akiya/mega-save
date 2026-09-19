//! Sole process boundary for `curl` HTML/markdown GET.

use anyhow::{bail, Context, Result};
use tokio::process::Command;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

pub async fn get_text(url: &str) -> Result<String> {
    get(url, Some(UA)).await
}

pub async fn get_reader_text(url: &str) -> Result<String> {
    // The reader service forwards browser-like user agents to the source, which
    // triggers the same challenge as a direct request. Its own default client
    // identity is intentional here.
    get(url, None).await
}

async fn get(url: &str, user_agent: Option<&str>) -> Result<String> {
    let mut command = Command::new("curl");
    command
        .arg("-fsSL")
        .arg("--max-time")
        .arg("90")
        .arg("--retry")
        .arg("2");
    if let Some(user_agent) = user_agent {
        command.arg("-A").arg(user_agent);
    }
    let output = command
        .arg("-H")
        .arg("Accept-Language: en-US,en;q=0.9")
        .arg(url)
        .output()
        .await
        .context("spawn curl")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("curl GET failed with {}: {}", output.status, stderr.trim());
    }
    String::from_utf8(output.stdout).context("curl response was not UTF-8")
}
