//! Sole process boundary for yt-dlp when saving YouTube audio as MP3.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::info;

#[derive(Debug, Clone)]
pub struct YtDlp {
    pub bin: String,
    pub format: String,
    pub js_runtime: String,
}

pub async fn inspect(ytdlp: &YtDlp, video_url: &str) -> Result<()> {
    let status = Command::new(&ytdlp.bin)
        .arg("--no-playlist")
        .arg("--js-runtimes")
        .arg(&ytdlp.js_runtime)
        .arg("--extractor-args")
        .arg("youtube:player_client=mweb")
        .arg("--skip-download")
        .arg("--print")
        .arg("title=%(title)s id=%(id)s duration_s=%(duration)s")
        .arg(video_url)
        .status()
        .await
        .with_context(|| format!("spawn {}", ytdlp.bin))?;
    if !status.success() {
        bail!("yt-dlp metadata lookup failed with {status}");
    }
    Ok(())
}

pub async fn download_mp3(
    ytdlp: &YtDlp,
    video_url: &str,
    output_template: &Path,
) -> Result<PathBuf> {
    let dir = output_template.parent().unwrap_or_else(|| Path::new("."));
    tokio::fs::create_dir_all(dir)
        .await
        .with_context(|| format!("mkdir {}", dir.display()))?;

    info!(bin = %ytdlp.bin, %video_url, out = %output_template.display(), "yt-dlp MP3 download");
    let status = Command::new(&ytdlp.bin)
        .arg("--no-playlist")
        .arg("--js-runtimes")
        .arg(&ytdlp.js_runtime)
        .arg("--extractor-args")
        .arg("youtube:player_client=mweb")
        .arg("-f")
        .arg(&ytdlp.format)
        .arg("-x")
        .arg("--audio-format")
        .arg("mp3")
        .arg("--audio-quality")
        .arg("0")
        .arg("--no-overwrites")
        .arg("--newline")
        .arg("-o")
        .arg(output_template)
        .arg(video_url)
        .current_dir(dir)
        .status()
        .await
        .with_context(|| format!("spawn {}", ytdlp.bin))?;
    if !status.success() {
        bail!("yt-dlp failed with {status}");
    }

    find_one_mp3(dir).await
}

async fn find_one_mp3(dir: &Path) -> Result<PathBuf> {
    let mut entries = tokio::fs::read_dir(dir)
        .await
        .context("read output directory")?;
    let mut matches = Vec::new();
    while let Some(entry) = entries.next_entry().await.context("read output entry")? {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("mp3"))
        {
            matches.push(path);
        }
    }
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => bail!("yt-dlp finished but no MP3 output was found"),
        count => bail!("yt-dlp produced {count} MP3 files; expected one"),
    }
}
