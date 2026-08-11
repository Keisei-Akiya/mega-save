//! Parse X / Twitter status URLs.

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusRef {
    pub id: String,
    /// screen_name when present in the URL path
    pub user: Option<String>,
}

fn status_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // (?x) treats bare # as start-of-comment — escape \# in the class.
        Regex::new(
            r"(?ix)
            ^https?://(?:www\.)?(?:x\.com|twitter\.com|mobile\.twitter\.com)
            /(?P<user>[A-Za-z0-9_]+)/status(?:es)?/(?P<id>\d+)
            (?:[/?\#].*)?$
            ",
        )
        .expect("status regex")
    })
}

/// Also accept bare numeric status id.
fn bare_id_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{5,25}$").expect("bare id regex"))
}

pub fn parse_status_input(input: &str) -> Result<StatusRef> {
    let s = input.trim();
    if bare_id_re().is_match(s) {
        return Ok(StatusRef {
            id: s.to_string(),
            user: None,
        });
    }

    let url = if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else if s.starts_with("x.com/") || s.starts_with("twitter.com/") {
        format!("https://{s}")
    } else {
        s.to_string()
    };

    let caps = status_re()
        .captures(&url)
        .with_context(|| format!("not an X/Twitter status URL: {input}"))?;

    let id = caps
        .name("id")
        .map(|m| m.as_str().to_string())
        .context("missing status id")?;
    let user = caps.name("user").map(|m| m.as_str().to_string()).filter(|u| {
        // path junk
        !u.eq_ignore_ascii_case("i") && !u.eq_ignore_ascii_case("intent")
    });

    if id.is_empty() {
        bail!("empty status id");
    }

    Ok(StatusRef { id, user })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_x_url() {
        let r = parse_status_input("https://x.com/MadFrnrNpf/status/2071418683029676461?s=20")
            .unwrap();
        assert_eq!(r.id, "2071418683029676461");
        assert_eq!(r.user.as_deref(), Some("MadFrnrNpf"));
    }

    #[test]
    fn parses_twitter_url() {
        let r = parse_status_input("https://twitter.com/foo_bar/statuses/1234567890").unwrap();
        assert_eq!(r.id, "1234567890");
        assert_eq!(r.user.as_deref(), Some("foo_bar"));
    }

    #[test]
    fn parses_bare_id() {
        let r = parse_status_input("2071418683029676461").unwrap();
        assert_eq!(r.id, "2071418683029676461");
        assert!(r.user.is_none());
    }
}
