//! Parse SpankBang video URLs (pure).

use anyhow::{bail, Context, Result};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoRef {
    pub url: String,
    pub id: String,
}

pub fn parse_video_url(input: &str) -> Result<VideoRef> {
    let input = input.trim();
    let normalized = if input.starts_with("http://") || input.starts_with("https://") {
        input.to_owned()
    } else if input.contains("spankbang.com/") || input.contains("spankbang.party/") {
        format!("https://{input}")
    } else {
        input.to_owned()
    };

    let mut url = Url::parse(&normalized).context("parse SpankBang URL")?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("SpankBang URL must use http or https");
    }
    let host = url
        .host_str()
        .context("SpankBang URL is missing a host")?
        .to_ascii_lowercase();
    let supported_host = ["spankbang.com", "spankbang.party"]
        .iter()
        .any(|base| host == *base || host.ends_with(&format!(".{base}")));
    if !supported_host {
        bail!("not a supported SpankBang host");
    }

    let segments: Vec<_> = url
        .path_segments()
        .context("SpankBang URL cannot be a base URL")?
        .filter(|part| !part.is_empty())
        .collect();
    if segments.len() < 2
        || !matches!(segments[1], "video" | "play" | "embed")
        || segments[0].is_empty()
        || !segments[0]
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        bail!("not a single SpankBang video URL");
    }

    let id = segments[0].to_ascii_lowercase();
    url.set_query(None);
    url.set_fragment(None);

    Ok(VideoRef {
        url: url.to_string().trim_end_matches('/').to_owned(),
        id,
    })
}

pub fn default_filename(id: &str) -> String {
    format!("{id}.mp4")
}

pub fn ensure_mp4(name: &str) -> String {
    if name.to_ascii_lowercase().ends_with(".mp4") {
        name.to_owned()
    } else {
        format!("{name}.mp4")
    }
}

pub fn output_filename(requested: Option<&str>, id: &str) -> Result<String> {
    let Some(requested) = requested else {
        return Ok(default_filename(id));
    };
    let requested = requested.trim();
    if requested.is_empty()
        || requested == "."
        || requested == ".."
        || requested.contains(['/', '\\'])
        || requested.chars().any(char::is_control)
    {
        bail!("output name must be a non-empty basename");
    }
    Ok(ensure_mp4(requested))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_video_url_and_strips_tracking() {
        let page = parse_video_url(
            "https://jp.spankbang.com/abc12/video/synthetic-example?ref=test#player",
        )
        .unwrap();

        assert_eq!(page.id, "abc12");
        assert_eq!(
            page.url,
            "https://jp.spankbang.com/abc12/video/synthetic-example"
        );
    }

    #[test]
    fn rejects_non_spankbang_hosts() {
        assert!(parse_video_url("https://example.com/abc12/video/synthetic-example").is_err());
    }

    #[test]
    fn rejects_listing_pages() {
        assert!(parse_video_url("https://spankbang.com/category/synthetic").is_err());
    }

    #[test]
    fn creates_safe_default_filename() {
        assert_eq!(default_filename("abc12"), "abc12.mp4");
        assert_eq!(ensure_mp4("synthetic-name"), "synthetic-name.mp4");
        assert_eq!(ensure_mp4("synthetic-name.MP4"), "synthetic-name.MP4");
    }

    #[test]
    fn rejects_names_that_are_not_basenames() {
        assert!(output_filename(Some("../synthetic"), "abc12").is_err());
        assert!(output_filename(Some("nested/synthetic"), "abc12").is_err());
        assert_eq!(
            output_filename(Some("synthetic"), "abc12").unwrap(),
            "synthetic.mp4"
        );
    }
}
