//! Resolve public video mp4 URLs via fxtwitter / vxtwitter (no yt-dlp).

use crate::url::StatusRef;
use anyhow::{anyhow, bail, Context, Result};
use reqwest::Client;
use serde_json::Value;
use tracing::{info, warn};

const UA: &str = "Mozilla/5.0 (compatible; mega-save-x/0.1; +https://github.com/local/mega-save)";

#[derive(Debug, Clone)]
pub struct VideoAsset {
    pub mp4_url: String,
    pub bitrate: i64,
    pub duration_s: Option<f64>,
    pub source: &'static str,
    pub media_index: usize,
}

pub async fn resolve_videos(client: &Client, status: &StatusRef) -> Result<Vec<VideoAsset>> {
    match fetch_fxtwitter(client, &status.id).await {
        Ok(v) if !v.is_empty() => {
            info!(count = v.len(), source = "fxtwitter", "resolved videos");
            return Ok(v);
        }
        Ok(_) => warn!("fxtwitter returned no video"),
        Err(e) => warn!(error = %e, "fxtwitter failed"),
    }

    let user = status
        .user
        .as_deref()
        .ok_or_else(|| anyhow!("fxtwitter failed and no screen_name in URL for vxtwitter"))?;

    let v = fetch_vxtwitter(client, user, &status.id)
        .await
        .context("vxtwitter")?;
    if v.is_empty() {
        bail!("no video in tweet (image-only, deleted, or protected)");
    }
    info!(count = v.len(), source = "vxtwitter", "resolved videos");
    Ok(v)
}

async fn fetch_fxtwitter(client: &Client, id: &str) -> Result<Vec<VideoAsset>> {
    let url = format!("https://api.fxtwitter.com/status/{id}");
    let resp = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, UA)
        .send()
        .await
        .context("GET fxtwitter")?
        .error_for_status()
        .context("fxtwitter HTTP")?;
    let data: Value = resp.json().await.context("fxtwitter JSON")?;
    parse_fxtwitter(&data)
}

fn parse_fxtwitter(data: &Value) -> Result<Vec<VideoAsset>> {
    let tweet = data.get("tweet").unwrap_or(data);
    let media = tweet
        .pointer("/media/all")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut out = Vec::new();
    for (idx, m) in media.iter().enumerate() {
        let ty = m.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if ty != "video" && ty != "gif" {
            continue;
        }
        // Prefer real video over gif unless that's all we have
        let duration = m
            .get("duration")
            .and_then(|d| d.as_f64())
            .or_else(|| m.get("duration").and_then(|d| d.as_i64()).map(|i| i as f64));

        let mut best_url: Option<String> = None;
        let mut best_br: i64 = -1;

        if let Some(formats) = m.get("formats").and_then(|f| f.as_array()) {
            for f in formats {
                let url = f.get("url").and_then(|u| u.as_str()).unwrap_or("");
                if url.is_empty() {
                    continue;
                }
                let container = f
                    .get("container")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if !container.contains("mp4") && !url.contains(".mp4") {
                    continue;
                }
                let br = f
                    .get("bitrate")
                    .and_then(|b| b.as_i64())
                    .or_else(|| f.get("bitrate").and_then(|b| b.as_u64()).map(|u| u as i64))
                    .unwrap_or(0);
                if br >= best_br {
                    best_br = br;
                    best_url = Some(url.to_string());
                }
            }
        }

        if best_url.is_none() {
            if let Some(u) = m.get("url").and_then(|u| u.as_str()) {
                if !u.is_empty() {
                    best_url = Some(u.to_string());
                    best_br = 0;
                }
            }
        }

        if let Some(mp4_url) = best_url {
            out.push(VideoAsset {
                mp4_url,
                bitrate: best_br,
                duration_s: duration,
                source: "fxtwitter",
                media_index: idx,
            });
        }
    }
    Ok(out)
}

async fn fetch_vxtwitter(client: &Client, user: &str, id: &str) -> Result<Vec<VideoAsset>> {
    let url = format!("https://api.vxtwitter.com/{user}/status/{id}");
    let resp = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, UA)
        .send()
        .await
        .context("GET vxtwitter")?
        .error_for_status()
        .context("vxtwitter HTTP")?;
    let data: Value = resp.json().await.context("vxtwitter JSON")?;
    parse_vxtwitter(&data)
}

fn parse_vxtwitter(data: &Value) -> Result<Vec<VideoAsset>> {
    let mut out = Vec::new();

    if let Some(ext) = data.get("media_extended").and_then(|v| v.as_array()) {
        for (idx, m) in ext.iter().enumerate() {
            let ty = m.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if ty != "video" && ty != "gif" {
                continue;
            }
            if let Some(u) = m.get("url").and_then(|u| u.as_str()) {
                if !u.is_empty() {
                    out.push(VideoAsset {
                        mp4_url: u.to_string(),
                        bitrate: 0,
                        duration_s: m.get("duration_millis").and_then(|d| d.as_f64()).map(|ms| ms / 1000.0),
                        source: "vxtwitter",
                        media_index: idx,
                    });
                }
            }
        }
    }

    if out.is_empty() {
        if let Some(urls) = data.get("mediaURLs").and_then(|v| v.as_array()) {
            for (idx, u) in urls.iter().enumerate() {
                if let Some(s) = u.as_str() {
                    if s.contains(".mp4") || s.contains("video") {
                        out.push(VideoAsset {
                            mp4_url: s.to_string(),
                            bitrate: 0,
                            duration_s: None,
                            source: "vxtwitter",
                            media_index: idx,
                        });
                    }
                }
            }
        }
    }

    Ok(out)
}

pub fn build_http_client() -> Result<Client> {
    Client::builder()
        .user_agent(UA)
        .timeout(std::time::Duration::from_secs(120))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .context("build reqwest client")
}
