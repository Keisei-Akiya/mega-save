//! mega-save-pornavhd: pornavhd post → recordplay HLS → yt-dlp → MEGA.

mod curl_get;
mod packer;
mod page;
mod url;
mod ytdlp;

use anyhow::{Context, Result};
use clap::Parser;
use mega_save_storage::{MegaRepository, RemotePath, Rclone};
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "mega-save-pornavhd",
    about = "pornavhd.com post → recordplay HLS → yt-dlp → MEGA. Do not pass post URL to yt-dlp directly.",
    version
)]
struct Cli {
    /// pornavhd post URL (…/YYYY/MM/DD/slug/)
    url: String,

    /// Destination remote path, e.g. mega:video/r18/1/raikun
    #[arg(short, long, env = "MEGA_SAVE_REMOTE")]
    remote: String,

    /// Output basename (default: <slug>.mp4)
    #[arg(long)]
    name: Option<String>,

    /// Keep local temp file after upload
    #[arg(long)]
    keep_temp: bool,

    /// Resolve HLS only; no download/upload
    #[arg(long)]
    dry_run: bool,

    /// rclone binary
    #[arg(long, default_value = "rclone", env = "RCLONE_BIN")]
    rclone: String,

    /// yt-dlp binary
    #[arg(long, default_value = "yt-dlp", env = "YT_DLP_BIN")]
    yt_dlp: String,

    /// yt-dlp -f format
    #[arg(long, default_value = "bv*+ba/b")]
    format: String,

    /// yt-dlp --concurrent-fragments
    #[arg(long, default_value_t = 8)]
    concurrent_fragments: u32,

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
    url::reject_non_post(&cli.url)?;
    let post = url::parse_post_url(&cli.url)?;
    let dest = RemotePath::parse(&cli.remote).map_err(|e| anyhow::anyhow!(e))?;
    info!(%post.url, slug = %post.slug, remote = %dest, "start");

    let post_html = page::fetch_text(&post.url, None).await?;
    let embed_url = page::find_embed_url(&post_html)?;
    let embed_origin = page::embed_origin(&embed_url)?;
    let embed_html = page::fetch_text(&embed_url, Some(&post.url)).await?;
    let links = packer::links_from_embed_html(&embed_html)?;
    let (hls_url, hls_tag) = links.best(&embed_origin)?;
    info!(%hls_tag, duration = ?links.duration_s, "resolved HLS");

    if cli.dry_run {
        println!(
            "dry-run source=pornavhd→recordplay tag={hls_tag} duration_s={:?}\n  embed={embed_url}\n  hls={hls_url}",
            links.duration_s
        );
        return Ok(());
    }

    let fname = url::ensure_mp4(cli.name.as_deref().unwrap_or(&url::default_filename(&post.slug)));

    let tmp_root = if let Some(w) = &cli.workdir {
        std::fs::create_dir_all(w).ok();
        tempfile::Builder::new()
            .prefix("mega-save-pornavhd-")
            .tempdir_in(w)
            .context("tempdir_in")?
    } else {
        tempfile::Builder::new()
            .prefix("mega-save-pornavhd-")
            .tempdir()
            .context("tempdir")?
    };

    let local = tmp_root.path().join(&fname);
    let ytdlp = ytdlp::YtDlp {
        bin: cli.yt_dlp.clone(),
        format: cli.format.clone(),
        concurrent_fragments: cli.concurrent_fragments,
    };
    let bytes = ytdlp::download_to(&ytdlp, &hls_url, &embed_url, &local).await?;

    let repo = MegaRepository::new(Rclone::new(cli.rclone.clone()));
    repo.upload_and_verify(&local, &dest, bytes)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    println!(
        "ok source=pornavhd→recordplay/{hls_tag} file={fname} remote={dest}/{fname} bytes={bytes} duration_s={:?}",
        links.duration_s
    );

    if cli.keep_temp {
        let keep = tmp_root
            .path()
            .parent()
            .unwrap_or(tmp_root.path())
            .join(&fname);
        tokio::fs::copy(&local, &keep).await.ok();
        println!("kept_local={}", keep.display());
        let _ = tmp_root.keep();
    } else {
        info!("temp cleaned");
    }

    Ok(())
}
