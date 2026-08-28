//! WNACG reader payload and fallback HTML parsing.

use crate::wnacg::url::validate_image_url;
use anyhow::{bail, Context, Result};
use regex::Regex;
use std::cmp::Ordering;
use std::path::Path;
use std::sync::OnceLock;
use url::Url;

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
pub(crate) fn parse_item_image_urls(script: &str) -> Result<Vec<String>> {
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
        let url =
            validate_image_url(raw).with_context(|| format!("invalid WNACG page_url[{index}]"))?;
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
pub(crate) fn parse_image_urls(html: &str, page_url: &str) -> Result<Vec<String>> {
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
        let Ok(resolved) = validate_image_url(resolved.as_str()) else {
            continue;
        };
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
pub(crate) fn output_filename(name: &str) -> Result<String> {
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
pub(crate) fn filename_from_work_title(html: &str) -> Result<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let script = r#"mReader.initData({"page_url":["https://img1.qy0.ru/page-10.jpg","https://img1.qy0.ru/page-2.jpg","https://img1.qy0.ru/page-1.jpg",],"hidden":0});"#;
        let urls = parse_item_image_urls(script).unwrap();
        assert_eq!(
            urls,
            vec![
                "https://img1.qy0.ru/page-10.jpg",
                "https://img1.qy0.ru/page-2.jpg",
                "https://img1.qy0.ru/page-1.jpg",
            ]
        );
    }

    #[test]
    fn extracts_lazy_images_in_page_order_without_duplicates() {
        let html = r#"
          <img src="/static/logo.png">
          <img data-original="https://img1.qy0.ru/pages/page-10.jpg">
          <img data-original="https://img1.qy0.ru/pages/page-2.jpg" src="/placeholder.gif">
          <img data-src="https://img1.qy0.ru/pages/page-1.jpg">
          <img data-original="https://img1.qy0.ru/pages/page-2.jpg">
          <img data-original="http://127.0.0.1/secret.jpg">
        "#;
        let urls =
            parse_image_urls(html, "https://www.wnacg.com/photos-slide-aid-248039.html").unwrap();
        assert_eq!(
            urls,
            vec![
                "https://img1.qy0.ru/pages/page-1.jpg",
                "https://img1.qy0.ru/pages/page-2.jpg",
                "https://img1.qy0.ru/pages/page-10.jpg",
            ]
        );
    }
}
