//! Sole process boundary for yt-dlp (direct media URL → MP4).

use anyhow::{bail, Context, Result};
use std::path::Path;
use tokio::process::Command;
use tracing::info;

#[derive(Debug, Clone)]
pub struct YtDlp {
    pub bin: String,
    pub format: String,
    pub concurrent_fragments: u32,
}

pub async fn download_to(
    ytdlp: &YtDlp,
    media_url: &str,
    referer: &str,
    output: &Path,
) -> Result<u64> {
    if let Some(parent) = output.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }

    info!(bin = %ytdlp.bin, output = %output.display(), "yt-dlp direct media download");
    let status = Command::new(&ytdlp.bin)
        .arg("--no-playlist")
        .arg("-f")
        .arg(&ytdlp.format)
        .arg("-o")
        .arg(output)
        .arg("--no-overwrites")
        .arg("--newline")
        .arg("--concurrent-fragments")
        .arg(ytdlp.concurrent_fragments.to_string())
        .arg("--retries")
        .arg("10")
        .arg("--fragment-retries")
        .arg("10")
        .arg("--referer")
        .arg(referer)
        .arg("--merge-output-format")
        .arg("mp4")
        .arg(media_url)
        .status()
        .await
        .with_context(|| format!("spawn {}", ytdlp.bin))?;

    if !status.success() {
        bail!("yt-dlp failed with {status}");
    }

    let metadata = tokio::fs::metadata(output)
        .await
        .with_context(|| format!("yt-dlp output missing: {}", output.display()))?;
    if metadata.len() == 0 {
        bail!("yt-dlp downloaded zero bytes");
    }
    Ok(metadata.len())
}
