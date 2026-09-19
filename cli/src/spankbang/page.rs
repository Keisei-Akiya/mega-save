//! Extract a direct media URL from site HTML or reader markdown.

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::sync::OnceLock;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaSource {
    pub url: String,
    pub host: String,
}

fn media_url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"https://[^\s'\"<>)]+\.mp4(?:\?[^\s'\"<>)]+)?"#).expect("media URL regex")
    })
}

fn media_host_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^vdownload(?:-\d+)?\.sb-cd\.com$").expect("media host regex"))
}

pub fn find_media(document: &str) -> Result<MediaSource> {
    let normalized = document.replace("\\/", "/").replace("&amp;", "&");
    for found in media_url_re().find_iter(&normalized) {
        let candidate = found.as_str();
        let parsed = match Url::parse(candidate) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let host = match parsed.host_str() {
            Some(host) => host.to_ascii_lowercase(),
            None => continue,
        };
        if parsed.scheme() == "https"
            && parsed.username().is_empty()
            && parsed.password().is_none()
            && parsed.port().is_none()
            && media_host_re().is_match(&host)
        {
            return Ok(MediaSource {
                url: candidate.to_owned(),
                host,
            });
        }
    }

    bail!("no allowlisted SpankBang media URL found")
}

pub fn reader_url(page_url: &str) -> Result<String> {
    let page = super::url::parse_video_url(page_url).context("validate reader source URL")?;
    Ok(format!("https://r.jina.ai/{}", page.url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_allowlisted_media_from_reader_markdown() {
        let document = r#"
            [Preview](https://tbv.sb-cd.com/synthetic-preview.mp4)
            [Video 1](https://vdownload-12.sb-cd.com/synthetic/video.mp4?token=example&amp;expires=1)
        "#;

        let media = find_media(document).unwrap();
        assert_eq!(media.host, "vdownload-12.sb-cd.com");
        assert!(media.url.ends_with("token=example&expires=1"));
    }

    #[test]
    fn extracts_allowlisted_media_from_site_html() {
        let document = r#"
            <script>stream_url_1080p = 'https:\/\/vdownload-7.sb-cd.com\/synthetic\/video.mp4?token=example';</script>
        "#;

        let media = find_media(document).unwrap();
        assert_eq!(media.host, "vdownload-7.sb-cd.com");
    }

    #[test]
    fn rejects_untrusted_mp4_hosts() {
        let document = "[Video 1](https://example.com/synthetic/video.mp4?token=example)";
        assert!(find_media(document).is_err());
    }

    #[test]
    fn builds_reader_url_from_validated_page() {
        assert_eq!(
            reader_url("https://jp.spankbang.com/abc12/video/synthetic-example").unwrap(),
            "https://r.jina.ai/https://jp.spankbang.com/abc12/video/synthetic-example"
        );
    }
}
