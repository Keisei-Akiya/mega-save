//! WNACG photo-slide work → single PDF.
//!
//! This module only requests the public work page and public image URLs exposed by it.
//! It does not send credentials, cookies, or attempt to circumvent access controls.

use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;
use image::codecs::jpeg::JpegEncoder;
use image::DynamicImage;
use mega_save_storage::{MegaRepository, Rclone, RemotePath};
use regex::Regex;
use reqwest::{header, Client, StatusCode};
use std::cmp::Ordering;
use std::fs::{self, File};
use std::io::{BufWriter, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;
use tracing::{info, warn};
use url::Url;

const UA: &str = "mega-save/0.1 (+https://github.com/Keisei-Akiya/mega-save)";
const MAX_IMAGES: usize = 1_000;
const HTTP_MAX_ATTEMPTS: u32 = 4;
const HTTP_RETRY_BASE_DELAY: Duration = Duration::from_secs(1);

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkRef {
    url: String,
    aid: String,
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

    let mut pages = Vec::with_capacity(images.len());
    for (index, image_url) in images.iter().enumerate() {
        let page_number = index + 1;
        let cache_path = cached_page_path(&pages_dir, page_number);
        if let Some(page) = load_cached_page(&cache_path, page_number)? {
            pages.push(page);
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
        pages.push(page);
    }
    write_pdf(&local, &pages)?;
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

fn build_http_client() -> Result<Client> {
    Client::builder()
        .user_agent(UA)
        .timeout(std::time::Duration::from_secs(120))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .context("build HTTP client")
}

async fn get_public_text(client: &Client, url: &str, referer: Option<&str>) -> Result<String> {
    let response = public_request(client, url, referer).await?;
    response.text().await.context("read public work page body")
}

async fn get_public_bytes(client: &Client, url: &str, referer: Option<&str>) -> Result<Vec<u8>> {
    let response = public_request(client, url, referer).await?;
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if content_type.contains("text/html") {
        bail!("image request returned HTML; access may be blocked or require login: {url}");
    }
    response
        .bytes()
        .await
        .context("read public image body")
        .map(|b| b.to_vec())
}

async fn public_request(
    client: &Client,
    url: &str,
    referer: Option<&str>,
) -> Result<reqwest::Response> {
    for attempt in 1..=HTTP_MAX_ATTEMPTS {
        let mut request = client.get(url).header(
            header::ACCEPT,
            "text/html,application/xhtml+xml,image/avif,image/webp,image/png,image/jpeg,*/*;q=0.8",
        );
        if let Some(referer) = referer {
            request = request.header(header::REFERER, referer);
        }
        match request.send().await {
            Ok(response) => {
                let status = response.status();
                if is_access_control_status(status) {
                    bail!("public access blocked for {url} (HTTP {status}); this command will not bypass login, DRM, or access controls");
                }
                if is_retryable_status(status) {
                    if attempt == HTTP_MAX_ATTEMPTS {
                        bail!("public GET {url}: retryable HTTP {status} persisted after {HTTP_MAX_ATTEMPTS} attempts");
                    }
                    let delay = retry_delay(attempt);
                    warn!(attempt, max_attempts = HTTP_MAX_ATTEMPTS, %status, delay_secs = delay.as_secs(), %url, "retrying transient public HTTP status");
                    tokio::time::sleep(delay).await;
                    continue;
                }
                return response
                    .error_for_status()
                    .with_context(|| format!("public GET {url}"));
            }
            Err(error) if is_retryable_transport_error(&error) => {
                if attempt == HTTP_MAX_ATTEMPTS {
                    return Err(error).with_context(|| {
                        format!("public GET {url}: transient transport failure persisted after {HTTP_MAX_ATTEMPTS} attempts")
                    });
                }
                let delay = retry_delay(attempt);
                warn!(attempt, max_attempts = HTTP_MAX_ATTEMPTS, error = %error, delay_secs = delay.as_secs(), %url, "retrying transient public transport failure");
                tokio::time::sleep(delay).await;
            }
            Err(error) => return Err(error).with_context(|| format!("GET {url}")),
        }
    }
    unreachable!("retry loop always returns on its final attempt")
}

fn is_access_control_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
    )
}

fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn is_retryable_transport_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect()
}

fn retry_delay(attempt: u32) -> Duration {
    HTTP_RETRY_BASE_DELAY.saturating_mul(2_u32.saturating_pow(attempt.saturating_sub(1)))
}

fn parse_work_url(input: &str) -> Result<WorkRef> {
    let raw = input.trim();
    let normalized = if raw.starts_with("www.wnacg.com/") || raw.starts_with("wnacg.com/") {
        format!("https://{raw}")
    } else {
        raw.to_string()
    };
    let url = Url::parse(&normalized).with_context(|| format!("invalid WNACG URL: {input}"))?;
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if host != "www.wnacg.com" && host != "wnacg.com" {
        bail!("not a WNACG URL: {input}");
    }
    let aid = work_path_re()
        .captures(url.path())
        .and_then(|captures| captures.name("aid"))
        .map(|capture| capture.as_str().to_string())
        .with_context(|| format!("not a WNACG photo-slide work URL: {input}"))?;
    Ok(WorkRef {
        url: format!("https://www.wnacg.com/photos-slide-aid-{aid}.html"),
        aid,
    })
}

fn work_path_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^/photos-slide-aid-(?P<aid>\d+)\.html$").expect("work URL regex")
    })
}

fn item_data_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?s)mReader\.initData\s*\(\s*(?P<json>\{.*?\})\s*\)")
            .expect("WNACG item data regex")
    })
}

fn trailing_json_comma_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r",\s*([\]}])").expect("trailing JSON comma regex"))
}

/// Parse WNACG's same-origin `mReader.initData` payload. `page_url` is already
/// in reader order, so deliberately do not sort it.
fn parse_item_image_urls(script: &str) -> Result<Vec<String>> {
    let json = item_data_re()
        .captures(script)
        .and_then(|captures| captures.name("json"))
        .map(|capture| capture.as_str())
        .context("no mReader.initData JSON payload in WNACG item response")?;
    // The endpoint emits JavaScript object syntax with a trailing array comma;
    // normalize only that syntax before handing it to the strict JSON parser.
    let normalized = trailing_json_comma_re().replace_all(json, "$1");
    let value: serde_json::Value =
        serde_json::from_str(&normalized).context("parse WNACG item JSON")?;
    let pages = value
        .get("page_url")
        .and_then(serde_json::Value::as_array)
        .context("WNACG item JSON has no page_url array")?;
    let mut urls = Vec::with_capacity(pages.len());
    for (index, page) in pages.iter().enumerate() {
        let raw = page
            .as_str()
            .with_context(|| format!("WNACG page_url[{index}] is not a string"))?;
        let url = Url::parse(raw).with_context(|| format!("invalid WNACG page_url[{index}]"))?;
        if !matches!(url.scheme(), "http" | "https") {
            bail!("WNACG page_url[{index}] has an unsupported scheme");
        }
        urls.push(url.to_string());
    }
    Ok(urls)
}

fn image_tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<img\b[^>]*>").expect("image tag regex"))
}

fn attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?is)(?:data-original|data-src|data-echo|src)\s*=\s*[\"'](?P<url>[^\"']+)[\"']"#,
        )
        .expect("image attribute regex")
    })
}

/// Extract visible/lazy image URLs and put numbered pages into natural order.
fn parse_image_urls(html: &str, page_url: &str) -> Result<Vec<String>> {
    let base = Url::parse(page_url).context("parse work page URL")?;
    let mut urls = Vec::new();
    for tag in image_tag_re().find_iter(html) {
        let tag = tag.as_str();
        let candidate = ["data-original", "data-src", "data-echo", "src"]
            .iter()
            .find_map(|wanted| {
                attr_re().captures_iter(tag).find_map(|captures| {
                    let matched = captures.get(0)?.as_str().to_ascii_lowercase();
                    if matched.starts_with(wanted) {
                        captures.name("url").map(|value| value.as_str())
                    } else {
                        None
                    }
                })
            });
        let Some(candidate) = candidate else { continue };
        let candidate = candidate.trim();
        if candidate.is_empty()
            || candidate.starts_with("data:")
            || candidate.starts_with("javascript:")
        {
            continue;
        }
        let resolved = base
            .join(candidate)
            .with_context(|| format!("resolve image URL {candidate}"))?;
        let path = resolved.path().to_ascii_lowercase();
        if !matches!(
            path.rsplit('.').next(),
            Some("jpg" | "jpeg" | "png" | "webp" | "gif")
        ) {
            continue;
        }
        // A plain `src` logo/thumbnail is not a work page. Accept plain sources only
        // when their path is explicitly under a photos collection; lazy attributes are
        // the markup used for actual slide pages.
        let tag_lower = tag.to_ascii_lowercase();
        let is_lazy = tag_lower.contains("data-original=")
            || tag_lower.contains("data-src=")
            || tag_lower.contains("data-echo=");
        if !is_lazy && !path.contains("/photos/") {
            continue;
        }
        urls.push(resolved.to_string());
    }
    urls.sort_by(|a, b| natural_url_cmp(a, b));
    urls.dedup();
    Ok(urls)
}

fn natural_url_cmp(a: &str, b: &str) -> Ordering {
    natural_key(a).cmp(&natural_key(b)).then_with(|| a.cmp(b))
}

fn natural_key(url: &str) -> (String, u64) {
    let path = Url::parse(url)
        .ok()
        .map(|url| url.path().to_string())
        .unwrap_or_else(|| url.to_string());
    let stem = path
        .rsplit('/')
        .next()
        .unwrap_or(&path)
        .rsplit_once('.')
        .map_or(path.as_str(), |(stem, _)| stem);
    let digits = stem
        .chars()
        .rev()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    let number = digits.parse::<u64>().unwrap_or(u64::MAX);
    let prefix = stem
        .trim_end_matches(|character: char| character.is_ascii_digit())
        .to_ascii_lowercase();
    (prefix, number)
}

/// Validate an output basename and preserve the `.pdf` completion.
///
/// This is deliberately platform-independent because the name is also sent to
/// rclone and may be stored on a Windows-backed remote.
fn output_filename(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || matches!(name, "." | "..") {
        bail!("output name must be a non-empty basename, not '.' or '..'");
    }
    if Path::new(name).is_absolute()
        || name.contains(['/', '\\'])
        || windows_drive_path_re().is_match(name)
    {
        bail!("output name must be a basename, not an absolute or separated path");
    }
    if name.chars().any(char::is_control) {
        bail!("output name must not contain control characters");
    }
    Ok(ensure_pdf(name))
}

fn ensure_pdf(name: &str) -> String {
    if name.to_ascii_lowercase().ends_with(".pdf") {
        name.to_string()
    } else {
        format!("{name}.pdf")
    }
}

fn windows_drive_path_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^[a-z]:").expect("Windows drive path regex"))
}

fn title_tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?is)<title\b[^>]*>(?P<title>.*?)</title>").expect("title tag regex")
    })
}

/// Extract only WNACG's work-title segment, not page-kind or site branding.
/// Requiring the known template delimiters makes this fail closed if the page
/// changes, rather than treating a site description as the work title.
fn filename_from_work_title(html: &str) -> Result<String> {
    let raw = title_tag_re()
        .captures(html)
        .and_then(|captures| captures.name("title"))
        .map(|capture| html_unescape(capture.as_str()))
        .context("could not extract a WNACG work title; pass --name with a safe PDF basename")?;
    let (work_with_credit, site_suffix) = raw
        .split_once(" - 列表 - ")
        .context("WNACG title format is unrecognized; pass --name with a safe PDF basename")?;
    if site_suffix.trim().is_empty() || !site_suffix.contains("紳士漫畫") {
        bail!("WNACG title format is unrecognized; pass --name with a safe PDF basename");
    }
    let title = strip_leading_creator_credit(work_with_credit);
    if title.is_empty()
        || ["列表", "紳士漫畫", "專註分享", "邪惡漫畫"]
            .iter()
            .any(|boilerplate| title.contains(boilerplate))
    {
        bail!("WNACG work title is missing or ambiguous; pass --name with a safe PDF basename");
    }
    output_filename(title)
}

/// WNACG prefixes the HTML title with `[creator] `. Remove that metadata field
/// only; bracketed text later in the work title is retained.
fn strip_leading_creator_credit(title: &str) -> &str {
    let title = title.trim();
    if let Some((creator, remainder)) = title
        .strip_prefix('[')
        .and_then(|text| text.split_once(']'))
    {
        if !creator.trim().is_empty()
            && (remainder.is_empty() || remainder.starts_with(char::is_whitespace))
        {
            return remainder.trim();
        }
    }
    title
}

fn html_unescape(value: &str) -> String {
    let output = value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    static NUMERIC_ENTITY: OnceLock<Regex> = OnceLock::new();
    let numeric = NUMERIC_ENTITY.get_or_init(|| {
        Regex::new(r"&#(?P<value>x[0-9a-fA-F]+|[0-9]+);").expect("numeric entity regex")
    });
    numeric
        .replace_all(&output, |captures: &regex::Captures<'_>| {
            let value = captures
                .name("value")
                .expect("numeric entity capture")
                .as_str();
            let number = value
                .strip_prefix('x')
                .map(|digits| u32::from_str_radix(digits, 16))
                .unwrap_or_else(|| value.parse());
            number
                .ok()
                .and_then(char::from_u32)
                .unwrap_or('\u{FFFD}')
                .to_string()
        })
        .into_owned()
}

/// A per-work directory that persists only when the user explicitly owns its root.
///
/// The guard intentionally covers every early return from `run`, including a failed
/// image download or upload. Explicit workdirs retain their valid page cache for a
/// later retry; implicit temporary page caches are removed at the end of the command.
struct Workdir {
    path: PathBuf,
    cleanup_on_drop: bool,
}

impl Workdir {
    fn new(workdir: Option<&Path>, aid: &str) -> Result<Self> {
        let (path, cleanup_on_drop) = match workdir {
            Some(root) => (root.join(format!("mega-save-wnacg-{aid}")), false),
            None => (
                tempfile::Builder::new()
                    .prefix(&format!("mega-save-wnacg-{aid}-"))
                    .tempdir_in(std::env::temp_dir())
                    .context("create temporary WNACG workdir")?
                    .keep(),
                true,
            ),
        };
        fs::create_dir_all(&path)
            .with_context(|| format!("mkdir work cache {}", path.display()))?;
        Ok(Self {
            path,
            cleanup_on_drop,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Workdir {
    fn drop(&mut self) {
        if self.cleanup_on_drop {
            let pages_dir = self.path.join("pages");
            match fs::remove_dir_all(&pages_dir) {
                Ok(()) => info!(path = %pages_dir.display(), "removed temporary WNACG page cache"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    warn!(path = %pages_dir.display(), error = %error, "could not remove temporary WNACG page cache")
                }
            }
            match fs::remove_dir(&self.path) {
                Ok(()) => {
                    info!(path = %self.path.display(), "removed empty temporary WNACG workdir")
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
                Err(error) => {
                    warn!(path = %self.path.display(), error = %error, "could not remove empty temporary WNACG workdir")
                }
            }
        }
    }
}

fn cached_page_path(pages_dir: &Path, page_number: usize) -> PathBuf {
    pages_dir.join(format!("page-{page_number:04}.image"))
}

fn load_cached_page(path: &Path, page_number: usize) -> Result<Option<JpegPage>> {
    match fs::read(path) {
        Ok(bytes) => match jpeg_page(&bytes) {
            Ok(page) => {
                info!(page = page_number, path = %path.display(), "resuming cached WNACG page");
                Ok(Some(page))
            }
            Err(error) => {
                warn!(page = page_number, path = %path.display(), error = %error, "discarding invalid cached WNACG page");
                fs::remove_file(path)
                    .with_context(|| format!("remove invalid cached page {}", path.display()))?;
                Ok(None)
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read cached page {}", path.display())),
    }
}

fn atomically_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("cached page path has no parent directory")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("create temporary page file in {}", parent.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("write temporary page file for {}", path.display()))?;
    file.flush()
        .with_context(|| format!("flush temporary page file for {}", path.display()))?;
    file.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("persist cached page {}", path.display()))?;
    Ok(())
}

struct JpegPage {
    width: u32,
    height: u32,
    jpeg: Vec<u8>,
}

fn jpeg_page(bytes: &[u8]) -> Result<JpegPage> {
    let image = image::load_from_memory(bytes).context("decode supported image")?;
    encode_jpeg(image)
}

fn encode_jpeg(image: DynamicImage) -> Result<JpegPage> {
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    if width == 0 || height == 0 {
        bail!("image has zero dimensions");
    }
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 92)
        .encode_image(&DynamicImage::ImageRgb8(rgb))
        .context("encode JPEG")?;
    Ok(JpegPage {
        width,
        height,
        jpeg,
    })
}

/// Write a minimal PDF with one JPEG-backed page per source image at 72 DPI.
fn write_pdf(path: &Path, pages: &[JpegPage]) -> Result<()> {
    if pages.is_empty() {
        bail!("cannot write a PDF with zero pages");
    }
    let mut objects: Vec<Vec<u8>> = vec![b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()];
    let mut kids = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        let page_id = 3 + index * 3;
        let content_id = page_id + 1;
        let image_id = page_id + 2;
        kids.push(format!("{page_id} 0 R"));
        let content = format!(
            "q\n{} 0 0 {} 0 0 cm\n/Im{} Do\nQ\n",
            page.width,
            page.height,
            index + 1
        );
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /XObject << /Im{} {} 0 R >> >> /Contents {} 0 R >>", page.width, page.height, index + 1, image_id, content_id).into_bytes());
        objects.push(stream_object(content.as_bytes()));
        let mut image = format!("<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n", page.width, page.height, page.jpeg.len()).into_bytes();
        image.extend_from_slice(&page.jpeg);
        image.extend_from_slice(b"\nendstream");
        objects.push(image);
    }
    objects.insert(
        1,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            pages.len()
        )
        .into_bytes(),
    );

    let file = File::create(path).with_context(|| format!("create {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    writer.write_all(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n")?;
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(writer.stream_position()?);
        writeln!(writer, "{} 0 obj", index + 1)?;
        writer.write_all(object)?;
        writer.write_all(b"\nendobj\n")?;
    }
    let xref = writer.stream_position()?;
    writeln!(writer, "xref\n0 {}\n0000000000 65535 f ", objects.len() + 1)?;
    for offset in offsets {
        writeln!(writer, "{:010} 00000 n ", offset)?;
    }
    writeln!(
        writer,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF",
        objects.len() + 1,
        xref
    )?;
    writer.flush().context("flush PDF")
}

fn stream_object(data: &[u8]) -> Vec<u8> {
    let mut object = format!("<< /Length {} >>\nstream\n", data.len()).into_bytes();
    object.extend_from_slice(data);
    object.extend_from_slice(b"endstream");
    object
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_canonicalizes_work_url() {
        let work =
            parse_work_url("https://www.wnacg.com/photos-slide-aid-248039.html?foo=bar").unwrap();
        assert_eq!(work.aid, "248039");
        assert_eq!(
            work.url,
            "https://www.wnacg.com/photos-slide-aid-248039.html"
        );
        assert!(parse_work_url("https://www.wnacg.com/photos-index-aid-248039.html").is_err());
        assert!(parse_work_url("https://other.example/photos-slide-aid-248039.html").is_err());
    }

    #[test]
    fn accepts_basename_and_preserves_pdf_extension_ux() {
        assert_eq!(output_filename("my work").unwrap(), "my work.pdf");
        assert_eq!(output_filename("MY-WORK.PDF").unwrap(), "MY-WORK.PDF");
    }

    #[test]
    fn rejects_unsafe_output_basenames() {
        for name in [
            "",
            "   ",
            ".",
            "..",
            "../escape",
            "nested/file",
            r"nested\\file",
            "/absolute",
            r"C:\\absolute",
        ] {
            assert!(output_filename(name).is_err(), "{name:?} should fail");
        }
    }

    #[test]
    fn extracts_only_work_title_without_creator_or_site_boilerplate() {
        let html = r#"<title>[ゆずや (ユズハ)] 銭湯のおねえさんと交わる、4日間の夏 (陰毛なし) [DL版] - 列表 - 紳士漫畫-專註分享漢化本子&#124;邪惡漫畫</title>"#;
        assert_eq!(
            filename_from_work_title(html).unwrap(),
            "銭湯のおねえさんと交わる、4日間の夏 (陰毛なし) [DL版].pdf"
        );
    }

    #[test]
    fn refuses_ambiguous_title_instead_of_falling_back_to_aid() {
        for html in [
            "<title>紳士漫畫-專註分享漢化本子</title>",
            "<title>[creator] - 列表 - 紳士漫畫</title>",
            "<title>Some work - 列表 - another site</title>",
        ] {
            assert!(filename_from_work_title(html).is_err(), "{html}");
        }
    }

    #[test]
    fn preserves_item_json_page_order() {
        let script = r#"mReader.initData({"page_url":["https://img.example/page-10.jpg","https://img.example/page-2.jpg","https://img.example/page-1.jpg",],"hidden":0});"#;
        let urls = parse_item_image_urls(script).unwrap();
        assert_eq!(
            urls,
            vec![
                "https://img.example/page-10.jpg",
                "https://img.example/page-2.jpg",
                "https://img.example/page-1.jpg",
            ]
        );
    }

    #[test]
    fn extracts_lazy_images_in_page_order_without_duplicates() {
        let html = r#"
          <img src="/static/logo.png">
          <img data-original="https://img.example/pages/page-10.jpg">
          <img data-original="https://img.example/pages/page-2.jpg" src="/placeholder.gif">
          <img data-src="https://img.example/pages/page-1.jpg">
          <img data-original="https://img.example/pages/page-2.jpg">
        "#;
        let urls =
            parse_image_urls(html, "https://www.wnacg.com/photos-slide-aid-248039.html").unwrap();
        assert_eq!(
            urls,
            vec![
                "https://img.example/pages/page-1.jpg",
                "https://img.example/pages/page-2.jpg",
                "https://img.example/pages/page-10.jpg",
            ]
        );
    }

    #[test]
    fn retries_only_transient_http_statuses() {
        assert!(is_retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable_status(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_retryable_status(StatusCode::from_u16(520).unwrap()));
        assert!(!is_retryable_status(StatusCode::BAD_REQUEST));
        assert!(!is_retryable_status(StatusCode::UNAUTHORIZED));
        assert!(!is_retryable_status(StatusCode::FORBIDDEN));
        assert!(!is_retryable_status(StatusCode::NOT_FOUND));
        assert!(is_access_control_status(StatusCode::UNAUTHORIZED));
        assert!(is_access_control_status(StatusCode::FORBIDDEN));
        assert!(is_access_control_status(StatusCode::NOT_FOUND));
    }

    #[test]
    fn retry_backoff_is_bounded_exponential() {
        assert_eq!(retry_delay(1), Duration::from_secs(1));
        assert_eq!(retry_delay(2), Duration::from_secs(2));
        assert_eq!(retry_delay(3), Duration::from_secs(4));
        assert_eq!(retry_delay(4), Duration::from_secs(8));
    }

    #[test]
    fn resumes_only_complete_decodable_cached_pages() {
        let temp = tempfile::tempdir().unwrap();
        let cached = cached_page_path(temp.path(), 7);
        let expected = encode_jpeg(DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            2,
            3,
            image::Rgb([12, 34, 56]),
        )))
        .unwrap();
        atomically_write(&cached, &expected.jpeg).unwrap();

        let resumed = load_cached_page(&cached, 7).unwrap().unwrap();
        assert_eq!((resumed.width, resumed.height), (2, 3));

        fs::write(&cached, b"incomplete image").unwrap();
        assert!(load_cached_page(&cached, 7).unwrap().is_none());
        assert!(!cached.exists());
    }

    #[test]
    fn explicit_workdir_keeps_cached_pages_after_an_aborted_workflow() {
        let root = tempfile::tempdir().unwrap();
        let pages_dir;
        {
            let workdir = Workdir::new(Some(root.path()), "248039").unwrap();
            pages_dir = workdir.path().join("pages");
            fs::create_dir_all(&pages_dir).unwrap();
            fs::write(
                cached_page_path(&pages_dir, 107),
                b"already-downloaded-page",
            )
            .unwrap();
            // Dropping here models an early return from a failed download/upload workflow.
        }

        assert!(cached_page_path(&pages_dir, 107).exists());
    }

    #[test]
    fn implicit_temp_workdir_is_cleaned_when_workflow_ends() {
        let path;
        {
            let workdir = Workdir::new(None, "248039").unwrap();
            path = workdir.path().to_path_buf();
            assert!(path.exists());
        }
        assert!(!path.exists());
    }

    #[test]
    fn implicit_cleanup_does_not_remove_a_kept_pdf() {
        let path;
        {
            let workdir = Workdir::new(None, "248039").unwrap();
            path = workdir.path().join("kept.pdf");
            fs::write(&path, b"PDF").unwrap();
        }
        assert!(path.exists());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn makes_a_parseable_pdf_with_one_page_per_image() {
        let page = encode_jpeg(DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            2,
            3,
            image::Rgb([12, 34, 56]),
        )))
        .unwrap();
        let temp = tempfile::NamedTempFile::new().unwrap();
        write_pdf(temp.path(), &[page]).unwrap();
        let bytes = std::fs::read(temp.path()).unwrap();
        assert!(bytes.starts_with(b"%PDF-1.4"));
        assert!(bytes
            .windows(b"/Count 1".len())
            .any(|window| window == b"/Count 1"));
        assert!(bytes
            .windows(b"/DCTDecode".len())
            .any(|window| window == b"/DCTDecode"));
    }
}
