//! Work-directory lifecycle and crash-safe page-cache persistence.

use crate::wnacg::pdf::{jpeg_page, JpegPage};
use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// A per-work directory that persists only when the user explicitly owns its root.
///
/// The guard intentionally covers every early return from `run`, including a failed
/// image download or upload. Explicit workdirs retain their valid page cache for a
/// later retry; implicit temporary page caches are removed at the end of the command.
pub(crate) struct Workdir {
    path: PathBuf,
    cleanup_on_drop: bool,
}

impl Workdir {
    pub(crate) fn new(workdir: Option<&Path>, aid: &str) -> Result<Self> {
        let (path, cleanup_on_drop) = match workdir {
            Some(root) => (root.join(format!("mega-save-wnacg-{aid}")), false),
            None => (
                tempfile::Builder::new()
                    .prefix(&format!("mega-save-wnacg-{aid}-"))
                    .tempdir_in(std::env::temp_dir())
                    .context("create temporary WNACG workdir")?
                    .keep(),
                true,
            ),
        };
        fs::create_dir_all(&path)
            .with_context(|| format!("mkdir work cache {}", path.display()))?;
        Ok(Self {
            path,
            cleanup_on_drop,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Workdir {
    fn drop(&mut self) {
        if self.cleanup_on_drop {
            let pages_dir = self.path.join("pages");
            match fs::remove_dir_all(&pages_dir) {
                Ok(()) => info!(path = %pages_dir.display(), "removed temporary WNACG page cache"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    warn!(path = %pages_dir.display(), error = %error, "could not remove temporary WNACG page cache")
                }
            }
            match fs::remove_dir(&self.path) {
                Ok(()) => {
                    info!(path = %self.path.display(), "removed empty temporary WNACG workdir")
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
                Err(error) => {
                    warn!(path = %self.path.display(), error = %error, "could not remove empty temporary WNACG workdir")
                }
            }
        }
    }
}

pub(crate) fn cached_page_path(pages_dir: &Path, page_number: usize) -> PathBuf {
    pages_dir.join(format!("page-{page_number:04}.image"))
}

pub(crate) fn load_cached_page(path: &Path, page_number: usize) -> Result<Option<JpegPage>> {
    match fs::read(path) {
        Ok(bytes) => match jpeg_page(&bytes) {
            Ok(page) => {
                info!(page = page_number, path = %path.display(), "resuming cached WNACG page");
                Ok(Some(page))
            }
            Err(error) => {
                warn!(page = page_number, path = %path.display(), error = %error, "discarding invalid cached WNACG page");
                fs::remove_file(path)
                    .with_context(|| format!("remove invalid cached page {}", path.display()))?;
                Ok(None)
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read cached page {}", path.display())),
    }
}

pub(crate) fn atomically_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("cached page path has no parent directory")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("create temporary page file in {}", parent.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("write temporary page file for {}", path.display()))?;
    file.flush()
        .with_context(|| format!("flush temporary page file for {}", path.display()))?;
    file.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("persist cached page {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::DynamicImage;
    use std::fs;

    #[test]
    fn resumes_only_complete_decodable_cached_pages() {
        let temp = tempfile::tempdir().unwrap();
        let cached = cached_page_path(temp.path(), 7);
        let image =
            DynamicImage::ImageRgb8(image::RgbImage::from_pixel(2, 3, image::Rgb([12, 34, 56])));
        let mut source = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut source)
            .encode_image(&image)
            .unwrap();
        let expected = jpeg_page(&source).unwrap();
        atomically_write(&cached, &expected.jpeg).unwrap();

        let resumed = load_cached_page(&cached, 7).unwrap().unwrap();
        assert_eq!((resumed.width, resumed.height), (2, 3));

        fs::write(&cached, b"incomplete image").unwrap();
        assert!(load_cached_page(&cached, 7).unwrap().is_none());
        assert!(!cached.exists());
    }

    #[test]
    fn explicit_workdir_keeps_cached_pages_after_an_aborted_workflow() {
        let root = tempfile::tempdir().unwrap();
        let pages_dir;
        {
            let workdir = Workdir::new(Some(root.path()), "248039").unwrap();
            pages_dir = workdir.path().join("pages");
            fs::create_dir_all(&pages_dir).unwrap();
            fs::write(
                cached_page_path(&pages_dir, 107),
                b"already-downloaded-page",
            )
            .unwrap();
            // Dropping here models an early return from a failed download/upload workflow.
        }

        assert!(cached_page_path(&pages_dir, 107).exists());
    }

    #[test]
    fn implicit_temp_workdir_is_cleaned_when_workflow_ends() {
        let path;
        {
            let workdir = Workdir::new(None, "248039").unwrap();
            path = workdir.path().to_path_buf();
            assert!(path.exists());
        }
        assert!(!path.exists());
    }

    #[test]
    fn implicit_cleanup_does_not_remove_a_kept_pdf() {
        let path;
        {
            let workdir = Workdir::new(None, "248039").unwrap();
            path = workdir.path().join("kept.pdf");
            fs::write(&path, b"PDF").unwrap();
        }
        assert!(path.exists());
        fs::remove_file(path).unwrap();
    }
}
