//! Remote path value type (pure).
//!
//! Examples: `mega:video/r18/0`, `mega:video/r18/0/foo.mp4`

use crate::error::{StorageError, StorageResult};
use std::fmt;

/// rclone 形式のリモートパス: `{remote}:{path}`（path に leading `/` は付けない）。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RemotePath {
    /// e.g. `mega`
    remote: String,
    /// e.g. `video/r18/0` or `video/r18/0/a.mp4`（空 = remote root）
    path: String,
}

impl RemotePath {
    /// `mega:video/r18/0` または `video/r18/0`（default remote = mega）。
    pub fn parse(input: &str) -> StorageResult<Self> {
        parse_with_default_remote(input, "mega")
    }

    pub fn parse_with_default(input: &str, default_remote: &str) -> StorageResult<Self> {
        parse_with_default_remote(input, default_remote)
    }

    pub fn new(remote: impl Into<String>, path: impl Into<String>) -> StorageResult<Self> {
        let remote = remote.into().trim().trim_end_matches(':').to_string();
        if remote.is_empty() {
            return Err(StorageError::InvalidPath("empty remote name".into()));
        }
        let path = normalize_path_part(&path.into());
        Ok(Self { remote, path })
    }

    pub fn remote(&self) -> &str {
        &self.remote
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// `mega:` のように remote root を指す文字列。
    pub fn remote_root(&self) -> String {
        format!("{}:", self.remote)
    }

    /// rclone に渡すフルパス `mega:video/r18/0`。
    pub fn as_rclone(&self) -> String {
        if self.path.is_empty() {
            self.remote_root()
        } else {
            format!("{}:{}", self.remote, self.path)
        }
    }

    /// 末尾セグメント（ファイル名 or 最終ディレクトリ名）。
    pub fn name(&self) -> Option<&str> {
        if self.path.is_empty() {
            None
        } else {
            self.path.rsplit('/').next()
        }
    }

    pub fn parent(&self) -> Option<Self> {
        if self.path.is_empty() {
            return None;
        }
        match self.path.rsplit_once('/') {
            Some((parent, _)) => Some(Self {
                remote: self.remote.clone(),
                path: parent.to_string(),
            }),
            None => Some(Self {
                remote: self.remote.clone(),
                path: String::new(),
            }),
        }
    }

    /// 子パスを結合（純粋）。`name` に `/` や `..` を含めないこと。
    pub fn join(&self, name: &str) -> StorageResult<Self> {
        let name = name.trim().trim_matches('/');
        if name.is_empty() {
            return Err(StorageError::InvalidPath("empty join name".into()));
        }
        if name.contains("..") || name.contains('/') || name.contains('\\') {
            return Err(StorageError::InvalidPath(format!(
                "join name must be a single segment: {name}"
            )));
        }
        let path = if self.path.is_empty() {
            name.to_string()
        } else {
            format!("{}/{}", self.path, name)
        };
        Ok(Self {
            remote: self.remote.clone(),
            path,
        })
    }

    pub fn is_root(&self) -> bool {
        self.path.is_empty()
    }
}

impl fmt::Display for RemotePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_rclone())
    }
}

fn parse_with_default_remote(input: &str, default_remote: &str) -> StorageResult<RemotePath> {
    let s = input.trim().trim_end_matches('/');
    if s.is_empty() {
        return Err(StorageError::InvalidPath("empty path".into()));
    }
    if let Some((remote, rest)) = s.split_once(':') {
        let remote = remote.trim();
        if remote.is_empty() {
            return Err(StorageError::InvalidPath("empty remote before ':'".into()));
        }
        RemotePath::new(remote, rest)
    } else {
        RemotePath::new(default_remote, s)
    }
}

fn normalize_path_part(path: &str) -> String {
    path.trim()
        .trim_start_matches('/')
        .trim_end_matches('/')
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_and_default() {
        let a = RemotePath::parse("mega:video/r18/0").unwrap();
        assert_eq!(a.remote(), "mega");
        assert_eq!(a.path(), "video/r18/0");
        assert_eq!(a.as_rclone(), "mega:video/r18/0");

        let b = RemotePath::parse("video/r18/0").unwrap();
        assert_eq!(b.as_rclone(), "mega:video/r18/0");
    }

    #[test]
    fn join_and_parent() {
        let d = RemotePath::parse("mega:video/r18/0").unwrap();
        let f = d.join("a.mp4").unwrap();
        assert_eq!(f.as_rclone(), "mega:video/r18/0/a.mp4");
        assert_eq!(f.parent().unwrap().as_rclone(), "mega:video/r18/0");
        assert_eq!(f.name(), Some("a.mp4"));
    }

    #[test]
    fn reject_join_traversal() {
        let d = RemotePath::parse("mega:video").unwrap();
        assert!(d.join("../x").is_err());
        assert!(d.join("a/b").is_err());
    }
}
