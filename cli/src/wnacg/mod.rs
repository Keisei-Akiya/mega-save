//! WNACG public photo-slide work → single PDF.
//!
//! This module only requests the public work page and public image URLs exposed by it.
//! It does not send credentials, cookies, or attempt to circumvent access controls.

mod fetch;
mod page;
mod pdf;
mod url;
mod workdir;

use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;
use fetch::{build_http_client, get_public_bytes, get_public_text};
use mega_save_storage::{MegaRepository, Rclone, RemotePath};
use page::{filename_from_work_title, output_filename, parse_image_urls, parse_item_image_urls};
use pdf::{jpeg_page, PdfWriter, PixelBudget};
use std::fs;
use std::path::PathBuf;
use tracing::info;
use url::parse_work_url;
use workdir::{atomically_write, cached_page_path, load_cached_page, Workdir};

const MAX_IMAGES: usize = 1_000;

#[derive(Debug, Parser)]
pub struct Args {
    /// Public WNACG photo-slide URL, e.g. https://www.wnacg.com/photos-slide-aid-248039.html
    pub url: String,

    /// Destination remote directory, e.g. mega:books/manga/r18/0
    #[arg(short, long, env = "MEGA_SAVE_REMOTE")]
    pub remote: String,

    /// Output basename. If omitted, use the work title extracted from the public page.
    #[arg(long)]
    pub name: Option<String>,

    /// Resolve and list ordered image URLs only; skip image download, PDF creation, and upload
    #[arg(long)]
    pub dry_run: bool,

    /// Keep the generated PDF in the work directory after a successful upload (does not affect page cache)
    #[arg(long)]
    pub keep_temp: bool,

    /// rclone binary name or path
    #[arg(long, default_value = "rclone", env = "RCLONE_BIN")]
    pub rclone: String,

    /// Persistent page-cache root. Pages beneath an explicit workdir survive success and failure; the default temporary cache is removed when the command ends.
    #[arg(long)]
    pub workdir: Option<PathBuf>,
}

pub async fn run(cli: Args) -> Result<()> {
    let work = parse_work_url(&cli.url)?;
    let explicit_filename = cli.name.as_deref().map(output_filename).transpose()?;
    let dest = RemotePath::parse(&cli.remote).map_err(|e| anyhow!(e))?;
    let client = build_http_client()?;
    info!(url = %work.url, aid = %work.aid, remote = %dest, "start WNACG public work fetch");

    let html = get_public_text(&client, &work.url, None).await?;
    let filename = match explicit_filename {
        Some(filename) => filename,
        None => filename_from_work_title(&html)?,
    };
    // WNACG loads the ordered `page_url` list from this same-origin public endpoint.
    // It is not an authenticated API and we do not add credentials or browser cookies.
    let item_url = format!("https://www.wnacg.com/photos-item-aid-{}.html", work.aid);
    let item_data = get_public_text(&client, &item_url, Some(&work.url)).await?;
    let images =
        parse_item_image_urls(&item_data).or_else(|_| parse_image_urls(&html, &work.url))?;
    if images.is_empty() {
        bail!("no public work images found; the page structure may have changed or access is restricted");
    }
    if images.len() > MAX_IMAGES {
        bail!(
            "refusing to process {} images (maximum is {MAX_IMAGES})",
            images.len()
        );
    }

    if cli.dry_run {
        println!(
            "dry-run source=wnacg aid={} title_file={} pages={}",
            work.aid,
            filename,
            images.len()
        );
        for (index, image) in images.iter().enumerate() {
            println!("{:04} {image}", index + 1);
        }
        return Ok(());
    }

    let workdir = Workdir::new(cli.workdir.as_deref(), &work.aid)?;
    let cache_dir = workdir.path();
    let pages_dir = cache_dir.join("pages");
    fs::create_dir_all(&pages_dir)
        .with_context(|| format!("mkdir page cache {}", pages_dir.display()))?;
    let local = cache_dir.join(&filename);

    let mut pdf = PdfWriter::create(&local, images.len())?;
    let mut pixel_budget = PixelBudget::new();
    for (index, image_url) in images.iter().enumerate() {
        let page_number = index + 1;
        let cache_path = cached_page_path(&pages_dir, page_number);
        if let Some(page) = load_cached_page(&cache_path, page_number)? {
            pixel_budget.consume(page.width, page.height)?;
            pdf.add_page(page)?;
            continue;
        }
        info!(page = page_number, total = images.len(), url = %image_url, "downloading public WNACG image");
        let bytes = get_public_bytes(&client, image_url, Some(&work.url))
            .await
            .with_context(|| format!("download page {page_number} ({image_url})"))?;
        let page = jpeg_page(&bytes)
            .with_context(|| format!("decode page {page_number} ({image_url})"))?;
        atomically_write(&cache_path, &bytes)
            .with_context(|| format!("cache page {page_number} at {}", cache_path.display()))?;
        pixel_budget.consume(page.width, page.height)?;
        pdf.add_page(page)?;
    }
    pdf.finish()?;
    let bytes = std::fs::metadata(&local)
        .with_context(|| format!("stat {}", local.display()))?
        .len();
    if bytes == 0 {
        bail!("generated PDF is empty");
    }

    let repo = MegaRepository::new(Rclone::new(cli.rclone));
    repo.upload_and_verify(&local, &dest, bytes)
        .await
        .map_err(|e| anyhow!(e))?;
    println!(
        "ok source=wnacg aid={} pages={} file={} remote={}/{} bytes={}",
        work.aid,
        images.len(),
        filename,
        dest,
        filename,
        bytes
    );

    if cli.keep_temp {
        println!("kept_local={}", local.display());
    } else {
        fs::remove_file(&local)
            .with_context(|| format!("remove temporary PDF {}", local.display()))?;
    }
    Ok(())
}
