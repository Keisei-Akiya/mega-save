//! Sole process boundary for yt-dlp (HLS → mp4).
//!
//! Semgrep allows tokio::process only in this file among site crates.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::info;

#[derive(Debug, Clone)]
pub struct YtDlp {
    pub bin: String,
    pub format: String,
    pub concurrent_fragments: u32,
}

impl Default for YtDlp {
    fn default() -> Self {
        Self {
            bin: "yt-dlp".into(),
            format: "bv*+ba/b".into(),
            concurrent_fragments: 8,
        }
    }
}

/// Download HLS/master URL to `out` (exact path, including .mp4).
pub async fn download_to(ytdlp: &YtDlp, media_url: &str, referer: &str, out: &Path) -> Result<u64> {
    if let Some(parent) = out.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("mkdir {}", parent.display()))?;
    }
    // yt-dlp -o must be a template; give exact filename via directory + fixed name
    let dir = out.parent().unwrap_or_else(|| Path::new("."));
    let name = out
        .file_name()
        .and_then(|s| s.to_str())
        .context("output file name")?;

    // Use out path directly as template (no placeholders)
    let out_tmpl = out.to_string_lossy().into_owned();

    info!(bin = %ytdlp.bin, %media_url, out = %out.display(), "yt-dlp download");

    let status = Command::new(&ytdlp.bin)
        .arg("--no-playlist")
        .arg("-f")
        .arg(&ytdlp.format)
        .arg("-o")
        .arg(&out_tmpl)
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
        .current_dir(dir)
        .status()
        .await
        .with_context(|| format!("spawn {}", ytdlp.bin))?;

    if !status.success() {
        bail!("yt-dlp failed with {status}");
    }

    // yt-dlp may write exact path or path with extension tweak
    let final_path = if out.is_file() {
        out.to_path_buf()
    } else {
        find_output(dir, name)
            .await
            .with_context(|| format!("yt-dlp finished but output missing: {}", out.display()))?
    };

    if final_path != out {
        tokio::fs::rename(&final_path, out)
            .await
            .with_context(|| format!("rename {} → {}", final_path.display(), out.display()))?;
    }

    let meta = tokio::fs::metadata(out)
        .await
        .with_context(|| format!("stat {}", out.display()))?;
    let bytes = meta.len();
    if bytes == 0 {
        bail!("downloaded 0 bytes");
    }
    info!(bytes, path = %out.display(), "yt-dlp complete");
    Ok(bytes)
}

async fn find_output(dir: &Path, want_name: &str) -> Result<PathBuf> {
    let stem = Path::new(want_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(want_name);
    let mut rd = tokio::fs::read_dir(dir).await.context("read_dir")?;
    let mut candidates = Vec::new();
    while let Some(ent) = rd.next_entry().await.context("read_dir next")? {
        let n = ent.file_name();
        let ns = n.to_string_lossy();
        if ns == want_name || ns.starts_with(stem) {
            let p = ent.path();
            if p.is_file() {
                candidates.push(p);
            }
        }
    }
    candidates
        .into_iter()
        .max_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .context("no output candidate")
}
