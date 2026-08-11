//! Stream mp4 to a local path.

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use std::path::Path;
use tokio::io::AsyncWriteExt;
use tracing::info;

const UA: &str = "Mozilla/5.0 (compatible; mega-save-x/0.1)";

pub async fn download_file(client: &Client, url: &str, dest: &Path) -> Result<u64> {
    let resp = client
        .get(url)
        .header(reqwest::header::USER_AGENT, UA)
        .send()
        .await
        .context("GET mp4")?
        .error_for_status()
        .context("mp4 HTTP")?;

    let mut stream = resp.bytes_stream();
    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("create {}", dest.display()))?;
    let mut written: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("read body chunk")?;
        file.write_all(&chunk).await.context("write chunk")?;
        written += chunk.len() as u64;
    }
    file.flush().await.ok();

    if written == 0 {
        bail!("downloaded 0 bytes");
    }
    info!(bytes = written, path = %dest.display(), "download complete");
    Ok(written)
}
