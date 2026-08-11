//! mega-save-x: public X video → MEGA via fxtwitter/vxtwitter + storage repository.

mod download;
mod fetch;
mod url;

use anyhow::{bail, Context, Result};
use clap::Parser;
use mega_save_storage::{MegaRepository, Rclone, RemotePath};
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "mega-save-x",
    about = "Download public X/Twitter video (fxtwitter/vxtwitter) and upload to MEGA. No yt-dlp.",
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
    let dest = RemotePath::parse(&cli.remote).map_err(|e| anyhow::anyhow!(e))?;
    let status = url::parse_status_input(&cli.url)?;
    info!(id = %status.id, user = ?status.user, remote = %dest, "start");

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

    let repo = MegaRepository::new(Rclone::new(cli.rclone.clone()));
    repo.ensure_dir(&dest)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    let base_name = cli
        .name
        .clone()
        .unwrap_or_else(|| default_basename(&status));

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
        let fname = if selected.len() == 1 {
            ensure_mp4(&base_name)
        } else {
            let stem = base_name.trim_end_matches(".mp4");
            format!("{stem}_{}.mp4", asset.media_index.max(n))
        };

        let local = tmp_root.path().join(&fname);
        let bytes = download::download_file(&client, &asset.mp4_url, &local).await?;

        repo.upload_and_verify(&local, &dest, bytes)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        info!(%fname, bytes, "verified remote size");
        println!(
            "ok source={} file={} remote={}/{} bytes={} bitrate={} duration_s={:?}",
            asset.source, fname, dest, fname, bytes, asset.bitrate, asset.duration_s
        );

        if cli.keep_temp {
            let keep = tmp_root
                .path()
                .parent()
                .unwrap_or(tmp_root.path())
                .join(&fname);
            tokio::fs::copy(&local, &keep).await.ok();
            println!("kept_local={}", keep.display());
        }
    }

    if !cli.keep_temp {
        info!("temp cleaned");
    } else {
        let _ = tmp_root.keep();
    }

    Ok(())
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
