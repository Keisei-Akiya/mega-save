//! SpankBang video command.

mod curl_get;
mod page;
mod url;
mod ytdlp;

use anyhow::{Context, Result};
use clap::Parser;
use mega_save_storage::{MegaRepository, Rclone, RemotePath};
use std::path::PathBuf;
use tracing::{info, warn};

#[derive(Debug, Parser)]
pub struct Args {
    /// SpankBang video page URL
    pub url: String,

    /// Destination remote path
    #[arg(short, long, env = "MEGA_SAVE_REMOTE")]
    pub remote: String,

    /// Output basename (default: <video-id>.mp4)
    #[arg(long)]
    pub name: Option<String>,

    /// Resolve the media URL only; do not download or upload
    #[arg(long)]
    pub dry_run: bool,

    /// Keep the temporary download after upload
    #[arg(long)]
    pub keep_temp: bool,

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

#[derive(Debug, Clone, Copy)]
enum ExtractionRoute {
    SiteHtml,
    ReaderProxy,
}

impl ExtractionRoute {
    fn label(self) -> &'static str {
        match self {
            Self::SiteHtml => "site-html",
            Self::ReaderProxy => "reader-proxy",
        }
    }
}

async fn resolve_media(page_url: &str) -> Result<(page::MediaSource, ExtractionRoute)> {
    match curl_get::get_text(page_url).await {
        Ok(document) => match page::find_media(&document) {
            Ok(media) => return Ok((media, ExtractionRoute::SiteHtml)),
            Err(error) => warn!(%error, "direct page had no usable media URL; trying reader proxy"),
        },
        Err(error) => warn!(%error, "direct page fetch failed; trying reader proxy"),
    }

    let reader_url = page::reader_url(page_url)?;
    let document = curl_get::get_reader_text(&reader_url)
        .await
        .context("fetch reader proxy")?;
    let media = page::find_media(&document).context("extract media from reader proxy")?;
    Ok((media, ExtractionRoute::ReaderProxy))
}

pub async fn run(args: Args) -> Result<()> {
    let video = url::parse_video_url(&args.url)?;
    let destination = RemotePath::parse(&args.remote).map_err(|error| anyhow::anyhow!(error))?;
    let filename = url::output_filename(args.name.as_deref(), &video.id)?;
    info!(video_id = %video.id, "resolving SpankBang media");

    let (media, route) = resolve_media(&video.url).await?;
    info!(route = route.label(), media_host = %media.host, "SpankBang media resolved");

    if args.dry_run {
        println!(
            "dry-run source=spankbang route={} media_host={} output={filename}",
            route.label(),
            media.host
        );
        return Ok(());
    }

    let temp = if let Some(workdir) = &args.workdir {
        std::fs::create_dir_all(workdir)
            .with_context(|| format!("create work directory {}", workdir.display()))?;
        tempfile::Builder::new()
            .prefix("mega-save-spankbang-")
            .tempdir_in(workdir)
            .context("create temporary work directory")?
    } else {
        tempfile::Builder::new()
            .prefix("mega-save-spankbang-")
            .tempdir()
            .context("create temporary work directory")?
    };
    let local = temp.path().join(&filename);

    let downloader = ytdlp::YtDlp {
        bin: args.yt_dlp,
        format: args.format,
        concurrent_fragments: args.concurrent_fragments,
    };
    let bytes = ytdlp::download_to(&downloader, &media.url, &video.url, &local).await?;

    let repository = MegaRepository::new(Rclone::new(args.rclone));
    repository
        .upload_and_verify(&local, &destination, bytes)
        .await
        .map_err(|error| anyhow::anyhow!(error))?;

    println!(
        "ok source=spankbang route={} file={filename} remote={destination}/{filename} bytes={bytes}",
        route.label()
    );
    if args.keep_temp {
        let kept_path = local.clone();
        let _ = temp.keep();
        println!("kept_local={}", kept_path.display());
    } else {
        info!("temporary download cleaned");
    }

    Ok(())
}
