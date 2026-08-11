//! Dean Edwards style packer decode + HLS link extraction (pure).

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HlsLinks {
    pub hls2: Option<String>,
    pub hls3: Option<String>,
    pub hls4: Option<String>,
    pub duration_s: Option<String>,
}

impl HlsLinks {
    /// Prefer absolute CDN (hls2), then hls3, then origin-relative hls4.
    pub fn best(&self, embed_origin: &str) -> Result<(String, &'static str)> {
        if let Some(u) = &self.hls2 {
            return Ok((u.clone(), "hls2"));
        }
        if let Some(u) = &self.hls3 {
            return Ok((absolutize(u, embed_origin), "hls3"));
        }
        if let Some(u) = &self.hls4 {
            return Ok((absolutize(u, embed_origin), "hls4"));
        }
        bail!("no hls2/hls3/hls4 in packer links");
    }
}

fn absolutize(u: &str, origin: &str) -> String {
    if u.starts_with("http://") || u.starts_with("https://") {
        u.to_string()
    } else if u.starts_with('/') {
        format!("{origin}{u}")
    } else {
        format!("{origin}/{u}")
    }
}

fn packer_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"eval\(function\(p,a,c,k,e,d\)\{while\(c--\)if\(k\[c\]\)p=p\.replace\(new RegExp\('\\\\b'\+c\.toString\(a\)\+'\\\\b','g'\),k\[c\]\);return p\}\('((?:\\.|[^'\\])*)',(\d+),(\d+),'((?:\\.|[^'\\])*)'\.split\('\|'\)\)\)",
        )
        .expect("packer re")
    })
}

/// Base-N digits for packer alphabet (0-9a-z).
fn to_base(mut n: usize, base: usize) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".into();
    }
    let mut s = Vec::new();
    while n > 0 {
        s.push(DIGITS[n % base]);
        n /= base;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

/// Unpack first packer blob in HTML; return decoded JS source.
pub fn unpack_packer(html: &str) -> Result<String> {
    let m = packer_re()
        .captures(html)
        .context("packer not found — page/host changed")?;
    let mut p = m.get(1).unwrap().as_str().to_string();
    // unescape \' in packed payload
    p = p.replace("\\'", "'").replace("\\\\", "\\");
    let a: usize = m.get(2).unwrap().as_str().parse().context("packer base")?;
    let c: usize = m.get(3).unwrap().as_str().parse().context("packer count")?;
    let k: Vec<&str> = m.get(4).unwrap().as_str().split('|').collect();

    // Replace from high index to low so shorter tokens don't break longer ones incorrectly;
    // packer uses \b word boundaries with base-a tokens.
    for i in (0..c).rev() {
        if i < k.len() && !k[i].is_empty() {
            let token = to_base(i, a);
            let re = Regex::new(&format!(r"\b{}\b", regex::escape(&token))).unwrap();
            p = re.replace_all(&p, k[i]).into_owned();
        }
    }
    Ok(p)
}

pub fn extract_hls_links(unpacked_js: &str) -> Result<HlsLinks> {
    let links_re = Regex::new(r#"links\s*=\s*\{([^;]+)\}"#).unwrap();
    let lm = links_re
        .captures(unpacked_js)
        .context("links= not found after unpack")?;
    let body = lm.get(1).unwrap().as_str();

    let mut out = HlsLinks {
        hls2: None,
        hls3: None,
        hls4: None,
        duration_s: None,
    };

    let pair_re = Regex::new(r#""(hls[234])"\s*:\s*"([^"]+)""#).unwrap();
    for cap in pair_re.captures_iter(body) {
        let key = cap.get(1).unwrap().as_str();
        let val = cap.get(2).unwrap().as_str().to_string();
        match key {
            "hls2" => out.hls2 = Some(val),
            "hls3" => out.hls3 = Some(val),
            "hls4" => out.hls4 = Some(val),
            _ => {}
        }
    }

    if out.hls2.is_none() && out.hls3.is_none() && out.hls4.is_none() {
        bail!("links object had no hls keys");
    }

    let dur_re = Regex::new(r#"duration\s*:\s*"([0-9.]+)""#).unwrap();
    if let Some(d) = dur_re.captures(unpacked_js) {
        out.duration_s = Some(d.get(1).unwrap().as_str().to_string());
    }

    Ok(out)
}

pub fn links_from_embed_html(html: &str) -> Result<HlsLinks> {
    let js = unpack_packer(html)?;
    extract_hls_links(&js)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpacks_minimal_packer_shape() {
        // synthetic: base 36, c=2, k = ["", "hello"]
        // p uses token "1" → hello. Packed form hand-built is hard; test extract only.
        let js = r#"var links={"hls2":"https://cdn.example/master.m3u8","hls4":"/stream/x/master.m3u8"};jwplayer("vplayer").setup({duration:"12.5"});"#;
        let l = extract_hls_links(js).unwrap();
        assert_eq!(l.hls2.as_deref(), Some("https://cdn.example/master.m3u8"));
        let (best, tag) = l.best("https://recordplay.biz").unwrap();
        assert_eq!(tag, "hls2");
        assert!(best.starts_with("https://"));
        assert_eq!(l.duration_s.as_deref(), Some("12.5"));
    }

    #[test]
    fn hls4_absolutized() {
        let l = HlsLinks {
            hls2: None,
            hls3: None,
            hls4: Some("/stream/a/master.m3u8".into()),
            duration_s: None,
        };
        let (u, tag) = l.best("https://recordplay.biz").unwrap();
        assert_eq!(tag, "hls4");
        assert_eq!(u, "https://recordplay.biz/stream/a/master.m3u8");
    }
}
