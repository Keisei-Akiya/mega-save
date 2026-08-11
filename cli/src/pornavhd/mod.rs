//! pornavhd.com site command.

mod curl_get;
mod packer;
mod page;
mod url;
mod ytdlp;

use anyhow::{Context, Result};
use clap::Parser;
use mega_save_storage::{MegaRepository, Rclone, RemotePath};
use std::path::PathBuf;
use tracing::info;

#[derive(Debug, Parser)]
pub struct Args {
    /// pornavhd post URL (…/YYYY/MM/DD/slug/)
    pub url: String,

    /// Destination remote path, e.g. mega:video/r18/1/raikun
    #[arg(short, long, env = "MEGA_SAVE_REMOTE")]
    pub remote: String,

    /// Output basename (default: <slug>.mp4)
    #[arg(long)]
    pub name: Option<String>,

    /// Keep local temp file after upload
    #[arg(long)]
    pub keep_temp: bool,

    /// Resolve HLS only; no download/upload
    #[arg(long)]
    pub dry_run: bool,

    /// rclone binary
    #[arg(long, default_value = "rclone", env = "RCLONE_BIN")]
    pub rclone: String,

    /// yt-dlp binary
    #[arg(long, default_value = "yt-dlp", env = "YT_DLP_BIN")]
    pub yt_dlp: String,

    /// yt-dlp -f format
    #[arg(long, default_value = "bv*+ba/b")]
    pub format: String,

    /// yt-dlp --concurrent-fragments
    #[arg(long, default_value_t = 8)]
    pub concurrent_fragments: u32,

    /// Work directory parent (default: system temp)
    #[arg(long)]
    pub workdir: Option<PathBuf>,
}

pub async fn run(cli: Args) -> Result<()> {
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

    let fname = url::ensure_mp4(
        cli.name
            .as_deref()
            .unwrap_or(&url::default_filename(&post.slug)),
    );

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
