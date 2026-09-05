//! YouTube video → MP3 → MEGA command.

mod ytdlp;

use anyhow::{bail, Context, Result};
use clap::Parser;
use mega_save_storage::{MegaRepository, Rclone, RemotePath};
use std::path::PathBuf;
use tracing::info;

#[derive(Debug, Parser)]
pub struct Args {
    /// Public YouTube video URL (playlists are ignored)
    pub url: String,

    /// Destination remote path, e.g. mega:music
    #[arg(short, long, env = "MEGA_SAVE_REMOTE")]
    pub remote: String,

    /// Output basename (default: YouTube title [video id].mp3)
    #[arg(long)]
    pub name: Option<String>,

    /// Resolve metadata only; do not download or upload
    #[arg(long)]
    pub dry_run: bool,

    /// Keep local MP3 after successful upload
    #[arg(long)]
    pub keep_temp: bool,

    /// rclone binary name or path
    #[arg(long, default_value = "rclone", env = "RCLONE_BIN")]
    pub rclone: String,

    /// yt-dlp binary name or path
    #[arg(long, default_value = "yt-dlp", env = "YT_DLP_BIN")]
    pub yt_dlp: String,

    /// yt-dlp format selector (default: YouTube mweb 360p muxed stream)
    #[arg(long, default_value = "18")]
    pub format: String,

    /// JavaScript runtime passed to yt-dlp
    #[arg(long, default_value = "node")]
    pub js_runtime: String,

    /// Work directory parent (default: system temp)
    #[arg(long)]
    pub workdir: Option<PathBuf>,
}

pub async fn run(cli: Args) -> Result<()> {
    if !is_youtube_url(&cli.url) {
        bail!("expected a YouTube URL");
    }
    let dest = RemotePath::parse(&cli.remote).map_err(|e| anyhow::anyhow!(e))?;
    if cli.dry_run {
        let ytdlp = ytdlp::YtDlp {
            bin: cli.yt_dlp,
            format: cli.format,
            js_runtime: cli.js_runtime,
        };
        ytdlp::inspect(&ytdlp, &cli.url).await?;
        println!(
            "dry-run source=youtube audio=mp3 remote={dest} url={}",
            cli.url
        );
        return Ok(());
    }

    let tmp_root = if let Some(workdir) = &cli.workdir {
        std::fs::create_dir_all(workdir).context("create work directory")?;
        tempfile::Builder::new()
            .prefix("mega-save-youtube-")
            .tempdir_in(workdir)
            .context("create temp directory")?
    } else {
        tempfile::Builder::new()
            .prefix("mega-save-youtube-")
            .tempdir()
            .context("create temp directory")?
    };
    let filename_template = match cli.name.as_deref() {
        Some(name) => ensure_mp3(name)?,
        None => "%(title).120B [%(id)s].mp3".to_string(),
    };
    let output_template = tmp_root.path().join(filename_template);
    let ytdlp = ytdlp::YtDlp {
        bin: cli.yt_dlp,
        format: cli.format,
        js_runtime: cli.js_runtime,
    };
    let local = ytdlp::download_mp3(&ytdlp, &cli.url, &output_template).await?;
    let filename = local
        .file_name()
        .and_then(|name| name.to_str())
        .context("MP3 file name is not UTF-8")?
        .to_string();
    let bytes = tokio::fs::metadata(&local)
        .await
        .with_context(|| format!("stat {}", local.display()))?
        .len();
    if bytes == 0 {
        bail!("downloaded MP3 is empty");
    }

    let repo = MegaRepository::new(Rclone::new(cli.rclone));
    repo.upload_and_verify(&local, &dest, bytes)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    println!("ok source=youtube file={filename} remote={dest}/{filename} bytes={bytes}");

    if cli.keep_temp {
        let kept = tmp_root
            .path()
            .parent()
            .unwrap_or(tmp_root.path())
            .join(&filename);
        tokio::fs::copy(&local, &kept)
            .await
            .with_context(|| format!("keep local MP3 at {}", kept.display()))?;
        println!("kept_local={}", kept.display());
        let _ = tmp_root.keep();
    } else {
        info!("temp cleaned");
    }
    Ok(())
}

fn ensure_mp3(name: &str) -> Result<String> {
    let path = std::path::Path::new(name);
    if name.is_empty() || path.file_name().and_then(|value| value.to_str()) != Some(name) {
        bail!("--name must be a non-empty basename, not a path");
    }
    if name.to_ascii_lowercase().ends_with(".mp3") {
        Ok(name.to_string())
    } else {
        Ok(format!("{name}.mp3"))
    }
}

fn is_youtube_url(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    matches!(
        url.host_str(),
        Some("youtube.com" | "www.youtube.com" | "m.youtube.com" | "youtu.be")
    )
}

#[cfg(test)]
mod tests {
    use super::{ensure_mp3, is_youtube_url};

    #[test]
    fn adds_mp3_suffix_and_rejects_paths() {
        assert_eq!(ensure_mp3("track").unwrap(), "track.mp3");
        assert_eq!(ensure_mp3("track.MP3").unwrap(), "track.MP3");
        assert!(ensure_mp3("../track.mp3").is_err());
        assert!(ensure_mp3("nested/track.mp3").is_err());
    }

    #[test]
    fn accepts_supported_youtube_hosts_only() {
        assert!(is_youtube_url(
            "https://www.youtube.com/watch?v=PmkN1iH4Ci4"
        ));
        assert!(is_youtube_url("https://youtu.be/PmkN1iH4Ci4"));
        assert!(!is_youtube_url("https://example.com/watch?v=PmkN1iH4Ci4"));
    }
}
