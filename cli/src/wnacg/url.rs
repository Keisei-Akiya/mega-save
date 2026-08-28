//! Validated public WNACG work and image URL handling.

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::sync::OnceLock;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkRef {
    pub(crate) url: String,
    pub(crate) aid: String,
}

pub(crate) fn parse_work_url(input: &str) -> Result<WorkRef> {
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

fn image_cdn_host_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^img[0-9]+\.qy0\.ru$").expect("WNACG image CDN regex"))
}

/// Restrict image fetches to the public WNACG image CDN. The image list is
/// untrusted page data, so do not allow it to select arbitrary network targets.
pub(crate) fn validate_image_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw).with_context(|| format!("invalid WNACG image URL: {raw}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("WNACG image URL has an unsupported scheme");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("WNACG image URL must not contain user credentials");
    }
    let host = url.host_str().unwrap_or_default();
    if !image_cdn_host_re().is_match(host) {
        bail!("WNACG image URL host is not an approved image CDN");
    }
    let standard_port = match url.scheme() {
        "http" => 80,
        "https" => 443,
        _ => unreachable!("scheme checked above"),
    };
    if let Some(port) = url.port() {
        if port != standard_port {
            bail!("WNACG image URL uses a non-standard port");
        }
    }
    Ok(url)
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
    fn accepts_only_standard_port_wnacg_image_cdn_urls() {
        for url in [
            "https://img1.qy0.ru/photos/page-1.jpg",
            "http://img99.qy0.ru/photos/page-1.jpg",
            "https://IMG2.QY0.RU/photos/page-1.jpg",
        ] {
            assert!(validate_image_url(url).is_ok(), "{url} should be accepted");
        }
        for url in [
            "https://www.wnacg.com/photos/page-1.jpg",
            "https://img1.qy0.ru.evil.example/page-1.jpg",
            "https://img.qy0.ru/page-1.jpg",
            "https://img1.qy0.ru:444/page-1.jpg",
            "http://img1.qy0.ru:443/page-1.jpg",
            "ftp://img1.qy0.ru/page-1.jpg",
            "http://127.0.0.1/page-1.jpg",
        ] {
            assert!(validate_image_url(url).is_err(), "{url} should be rejected");
        }
    }
}
