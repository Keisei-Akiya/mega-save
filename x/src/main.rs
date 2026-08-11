//! mega-save-x: public X video → MEGA via fxtwitter/vxtwitter + rclone.

mod download;
mod fetch;
mod rclone;
mod url;

use anyhow::{bail, Context, Result};
use clap::Parser;
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "mega-save-x",
    about = "Download public X/Twitter video (fxtwitter/vxtwitter) and upload to MEGA with rclone. No yt-dlp.",
    version
)]
struct Cli {
    /// X/Twitter status URL or bare numeric status id
    url: String,

    /// Destination remote path, e.g. mega:video/r18/0
    #[arg(short, long, env = "MEGA_SAVE_REMOTE")]
    remote: String,

    /// Local/remote basename (default: {user}_{id}.mp4 or {id}.mp4)
    #[arg(long)]
    name: Option<String>,

    /// Keep local temp file after successful upload
    #[arg(long)]
    keep_temp: bool,

    /// Resolve video URL only; skip download and upload
    #[arg(long)]
    dry_run: bool,

    /// rclone binary name or path
    #[arg(long, default_value = "rclone", env = "RCLONE_BIN")]
    rclone: String,

    /// Which video when multiple (0-based). Default: all
    #[arg(long)]
    index: Option<usize>,

    /// Work directory parent (default: system temp)
    #[arg(long)]
    workdir: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(Level::INFO.into())
                .from_env_lossy(),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let remote = normalize_remote(&cli.remote)?;
    let status = url::parse_status_input(&cli.url)?;
    info!(id = %status.id, user = ?status.user, remote = %remote, "start");

    let client = fetch::build_http_client()?;
    let videos = fetch::resolve_videos(&client, &status).await?;

    let selected: Vec<_> = if let Some(i) = cli.index {
        let one = videos
            .get(i)
            .with_context(|| format!("--index {i} out of range (0..{})", videos.len()))?
            .clone();
        vec![one]
    } else {
        videos
    };

    if selected.is_empty() {
        bail!("no videos selected");
    }

    if cli.dry_run {
        for (n, v) in selected.iter().enumerate() {
            println!(
                "dry-run[{n}] source={} bitrate={} duration_s={:?}\n  {}",
                v.source, v.bitrate, v.duration_s, v.mp4_url
            );
        }
        return Ok(());
    }

    rclone::ensure_remote_ok(&cli.rclone, &remote).await?;
    rclone::mkdir_p(&cli.rclone, &remote).await?;

    let base_name = cli.name.clone().unwrap_or_else(|| default_basename(&status));

    let tmp_root = if let Some(w) = &cli.workdir {
        std::fs::create_dir_all(w).ok();
        tempfile::Builder::new()
            .prefix("mega-save-x-")
            .tempdir_in(w)
            .context("tempdir_in workdir")?
    } else {
        tempfile::Builder::new()
            .prefix("mega-save-x-")
            .tempdir()
            .context("tempdir")?
    };

    for (n, asset) in selected.iter().enumerate() {
        let fname = if selected.len() == 1 && cli.name.is_some() {
            ensure_mp4(&base_name)
        } else if selected.len() == 1 {
            ensure_mp4(&base_name)
        } else {
            let stem = base_name.trim_end_matches(".mp4");
            format!("{stem}_{}.mp4", asset.media_index.max(n))
        };

        let local = tmp_root.path().join(&fname);
        let bytes = download::download_file(&client, &asset.mp4_url, &local).await?;
        rclone::copy_file(&cli.rclone, &local, &remote).await?;

        match rclone::remote_size(&cli.rclone, &remote, &fname).await? {
            Some(remote_bytes) if remote_bytes == bytes => {
                info!(%fname, bytes, "verified remote size");
            }
            Some(remote_bytes) => {
                tracing::warn!(local = bytes, remote = remote_bytes, "size mismatch");
            }
            None => tracing::warn!(%fname, "file not listed on remote after copy"),
        }

        println!(
            "ok source={} file={} remote={}/{} bytes={} bitrate={} duration_s={:?}",
            asset.source,
            fname,
            remote.trim_end_matches('/'),
            fname,
            bytes,
            asset.bitrate,
            asset.duration_s
        );

        if cli.keep_temp {
            let keep = tmp_root.path().parent().unwrap_or(tmp_root.path()).join(&fname);
            // copy out of tempdir before drop
            tokio::fs::copy(&local, &keep).await.ok();
            println!("kept_local={}", keep.display());
        }
    }

    if !cli.keep_temp {
        // tempdir drop cleans
        info!("temp cleaned");
    } else {
        // prevent auto-delete of whole dir contents we care about — already copied out
        let _ = tmp_root.keep();
    }

    Ok(())
}

fn normalize_remote(s: &str) -> Result<String> {
    let t = s.trim().trim_end_matches('/').to_string();
    if t.is_empty() {
        bail!("--remote is empty");
    }
    if t.contains(':') {
        Ok(t)
    } else {
        // allow video/r18/0 → mega:video/r18/0
        Ok(format!("mega:{t}"))
    }
}

fn default_basename(status: &url::StatusRef) -> String {
    match &status.user {
        Some(u) => format!("{u}_{}.mp4", status.id),
        None => format!("{}.mp4", status.id),
    }
}

fn ensure_mp4(name: &str) -> String {
    if name.to_ascii_lowercase().ends_with(".mp4") {
        name.to_string()
    } else {
        format!("{name}.mp4")
    }
}
