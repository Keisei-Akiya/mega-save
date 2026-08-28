//! Image decoding safeguards and a sequential JPEG-backed PDF writer.

use anyhow::{bail, Context, Result};
use image::codecs::jpeg::JpegEncoder;
use image::DynamicImage;
use std::fs::File;
use std::io::{BufWriter, Cursor, Seek, Write};
use std::path::Path;

const MAX_IMAGE_PIXELS: u64 = 20_000_000;
// A sequential writer retains only one decoded page, so this is an output-size
// guard rather than a process-memory budget. Keep enough headroom for typical
// long, high-resolution works while still bounding a hostile page list.
const MAX_TOTAL_PIXELS: u64 = 1_200_000_000;

pub(crate) struct JpegPage {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) jpeg: Vec<u8>,
}

pub(crate) struct PixelBudget {
    used: u64,
}

impl PixelBudget {
    pub(crate) fn new() -> Self {
        Self { used: 0 }
    }

    pub(crate) fn consume(&mut self, width: u32, height: u32) -> Result<()> {
        let pixels = validate_image_pixels(width, height)?;
        let total = self
            .used
            .checked_add(pixels)
            .context("total image pixel count overflow")?;
        if total > MAX_TOTAL_PIXELS {
            bail!("refusing to decode more than {MAX_TOTAL_PIXELS} total image pixels");
        }
        self.used = total;
        Ok(())
    }
}

fn validate_image_pixels(width: u32, height: u32) -> Result<u64> {
    if width == 0 || height == 0 {
        bail!("image has zero dimensions");
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_IMAGE_PIXELS {
        bail!("refusing to decode image with {pixels} pixels (maximum is {MAX_IMAGE_PIXELS})");
    }
    Ok(pixels)
}

pub(crate) fn jpeg_page(bytes: &[u8]) -> Result<JpegPage> {
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .context("identify supported image")?;
    let (width, height) = reader.into_dimensions().context("read image dimensions")?;
    validate_image_pixels(width, height)?;
    let image = image::load_from_memory(bytes).context("decode supported image")?;
    encode_jpeg(image)
}

fn encode_jpeg(image: DynamicImage) -> Result<JpegPage> {
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    validate_image_pixels(width, height)?;
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 92)
        .encode_image(&DynamicImage::ImageRgb8(rgb))
        .context("encode JPEG")?;
    Ok(JpegPage {
        width,
        height,
        jpeg,
    })
}

/// Sequential PDF writer that retains only offsets and one JPEG-backed page.
pub(crate) struct PdfWriter {
    writer: BufWriter<File>,
    offsets: Vec<u64>,
    page_count: usize,
    pages_written: usize,
}

impl PdfWriter {
    pub(crate) fn create(path: &Path, page_count: usize) -> Result<Self> {
        if page_count == 0 {
            bail!("cannot write a PDF with zero pages");
        }
        let file = File::create(path).with_context(|| format!("create {}", path.display()))?;
        let mut pdf = Self {
            writer: BufWriter::new(file),
            offsets: Vec::with_capacity(2 + page_count * 3),
            page_count,
            pages_written: 0,
        };
        pdf.writer.write_all(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n")?;
        pdf.write_object(1, |writer| {
            writer.write_all(b"<< /Type /Catalog /Pages 2 0 R >>")
        })?;
        pdf.write_object(2, |writer| {
            write!(writer, "<< /Type /Pages /Kids [")?;
            for index in 0..page_count {
                write!(writer, "{} 0 R ", 3 + index * 3)?;
            }
            write!(writer, "] /Count {page_count} >>")
        })?;
        Ok(pdf)
    }

    pub(crate) fn add_page(&mut self, page: JpegPage) -> Result<()> {
        if self.pages_written == self.page_count {
            bail!("cannot add more PDF pages than declared");
        }
        let index = self.pages_written;
        let page_id = 3 + index * 3;
        let content_id = page_id + 1;
        let image_id = page_id + 2;
        let image_name = index + 1;
        let width = page.width;
        let height = page.height;
        self.write_object(page_id, |writer| {
            write!(writer, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources << /XObject << /Im{image_name} {image_id} 0 R >> >> /Contents {content_id} 0 R >>")
        })?;
        let content = format!("q\n{width} 0 0 {height} 0 0 cm\n/Im{image_name} Do\nQ\n");
        self.write_object(content_id, |writer| {
            write!(
                writer,
                "<< /Length {} >>\nstream\n{}endstream",
                content.len(),
                content
            )
        })?;
        self.write_object(image_id, |writer| {
            write!(writer, "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n", page.jpeg.len())?;
            writer.write_all(&page.jpeg)?;
            writer.write_all(b"\nendstream")
        })?;
        self.pages_written += 1;
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<()> {
        if self.pages_written != self.page_count {
            bail!(
                "PDF declared {} pages but received {}",
                self.page_count,
                self.pages_written
            );
        }
        let xref = self.writer.stream_position()?;
        writeln!(
            self.writer,
            "xref\n0 {}\n0000000000 65535 f ",
            self.offsets.len() + 1
        )?;
        for offset in &self.offsets {
            writeln!(self.writer, "{offset:010} 00000 n ")?;
        }
        writeln!(
            self.writer,
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF",
            self.offsets.len() + 1
        )?;
        self.writer.flush().context("flush PDF")
    }

    fn write_object<F>(&mut self, id: usize, body: F) -> Result<()>
    where
        F: FnOnce(&mut BufWriter<File>) -> std::io::Result<()>,
    {
        self.offsets.push(self.writer.stream_position()?);
        writeln!(self.writer, "{id} 0 obj")?;
        body(&mut self.writer)?;
        self.writer.write_all(b"\nendobj\n")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_budget_allows_a_211_page_standard_work() {
        let mut budget = PixelBudget::new();
        for _ in 0..211 {
            budget.consume(1_057, 1_500).unwrap();
        }
    }

    #[test]
    fn pixel_budget_rejects_oversized_single_or_total_images() {
        assert!(PixelBudget::new().consume(20_001, 1_000).is_err());

        let mut budget = PixelBudget::new();
        for _ in 0..120 {
            budget.consume(10_000, 1_000).unwrap();
        }
        assert!(budget.consume(10_000, 1_000).is_err());
    }

    #[test]
    fn makes_a_parseable_pdf_with_one_page_per_image() {
        let page = encode_jpeg(DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            2,
            3,
            image::Rgb([12, 34, 56]),
        )))
        .unwrap();
        let temp = tempfile::NamedTempFile::new().unwrap();
        let mut pdf = PdfWriter::create(temp.path(), 1).unwrap();
        pdf.add_page(page).unwrap();
        pdf.finish().unwrap();
        let bytes = std::fs::read(temp.path()).unwrap();
        assert!(bytes.starts_with(b"%PDF-1.4"));
        assert!(bytes
            .windows(b"/Count 1".len())
            .any(|window| window == b"/Count 1"));
        assert!(bytes
            .windows(b"/DCTDecode".len())
            .any(|window| window == b"/DCTDecode"));
    }
}
