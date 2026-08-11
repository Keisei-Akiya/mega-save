//! Parse pornavhd.com post URLs (pure).

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::sync::OnceLock;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostRef {
    pub url: String,
    pub slug: String,
}

fn post_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // (?x): escape \#
        Regex::new(
            r"(?ix)
            ^https?://(?:www\.)?pornavhd\.com
            /(?P<y>\d{4})/(?P<m>\d{2})/(?P<d>\d{2})
            /(?P<slug>[A-Za-z0-9_-]+)
            /?
            (?:[/?\#].*)?$
            ",
        )
        .expect("pornavhd post regex")
    })
}

pub fn parse_post_url(input: &str) -> Result<PostRef> {
    let s = input.trim();
    let normalized = if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else if s.starts_with("pornavhd.com/") {
        format!("https://{s}")
    } else {
        s.to_string()
    };

    let caps = post_re()
        .captures(&normalized)
        .with_context(|| format!("not a pornavhd post URL: {input}"))?;

    let slug = caps
        .name("slug")
        .map(|m| m.as_str().to_string())
        .context("missing slug")?;

    // Canonical URL without query
    let mut u = Url::parse(&normalized).context("parse url")?;
    u.set_query(None);
    u.set_fragment(None);
    let mut path = u.path().to_string();
    if !path.ends_with('/') {
        path.push('/');
        u.set_path(&path);
    }

    Ok(PostRef {
        url: u.to_string(),
        slug,
    })
}

pub fn default_filename(slug: &str) -> String {
    if slug.to_ascii_lowercase().ends_with(".mp4") {
        slug.to_string()
    } else {
        format!("{slug}.mp4")
    }
}

pub fn ensure_mp4(name: &str) -> String {
    if name.to_ascii_lowercase().ends_with(".mp4") {
        name.to_string()
    } else {
        format!("{name}.mp4")
    }
}

/// Reject category/actor listing URLs early with a clear error.
pub fn reject_non_post(input: &str) -> Result<()> {
    let s = input.to_ascii_lowercase();
    if s.contains("/category/") || s.contains("/actor/") || s.contains("/tag/") {
        bail!("category/actor/tag pages are not supported; pass a single post URL");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_post() {
        let p = parse_post_url("https://pornavhd.com/2026/07/25/raikun325_20/?utm=1").unwrap();
        assert_eq!(p.slug, "raikun325_20");
        assert!(p.url.contains("pornavhd.com/2026/07/25/raikun325_20/"));
        assert!(!p.url.contains("utm"));
    }

    #[test]
    fn rejects_category() {
        assert!(reject_non_post("https://pornavhd.com/category/myfans/").is_err());
    }
}
