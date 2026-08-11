//! Post + embed HTML helpers. Network via `curl_get` (effect).

use crate::curl_get;
use anyhow::{bail, Context, Result};
use regex::Regex;
use std::sync::OnceLock;
use tracing::info;

fn embed_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)https?://[a-z0-9._-]+/e/[A-Za-z0-9]+"#).expect("embed re")
    })
}

pub async fn fetch_text(url: &str, referer: Option<&str>) -> Result<String> {
    curl_get::get_text(url, referer).await
}

/// Find player embed URL (`.../e/FILECODE`) from post HTML. Prefer non-ad hosts.
pub fn find_embed_url(post_html: &str) -> Result<String> {
    let mut found: Vec<String> = embed_re()
        .find_iter(post_html)
        .map(|m| m.as_str().to_string())
        .collect();
    found.sort();
    found.dedup();

    if found.is_empty() {
        bail!("no /e/ embed URL in post HTML (page structure changed?)");
    }

    let preferred = ["recordplay", "streamhg", "dood", "filemoon", "voe"];
    for host in preferred {
        if let Some(u) = found.iter().find(|u| u.to_ascii_lowercase().contains(host)) {
            info!(%u, "embed selected");
            return Ok(u.clone());
        }
    }

    let u = found.into_iter().next().unwrap();
    info!(%u, "embed selected (fallback)");
    Ok(u)
}

pub fn embed_origin(embed_url: &str) -> Result<String> {
    let u = url::Url::parse(embed_url).context("embed url")?;
    let origin = format!(
        "{}://{}",
        u.scheme(),
        u.host_str().context("embed host")?
    );
    Ok(origin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_recordplay_embed() {
        let html = r#"
        <iframe src="https://ads.example/e/xxx"></iframe>
        <iframe src="https://recordplay.biz/e/sxa7z853tvoj" width="640"></iframe>
        "#;
        let u = find_embed_url(html).unwrap();
        assert!(u.contains("recordplay.biz/e/sxa7z853tvoj"));
    }
}
