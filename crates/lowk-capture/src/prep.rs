//! Screenshots for the model: downscale to a long edge, then JPEG (q85) or PNG, whichever is smaller
//! (text-heavy UI shots often compress better as PNG). Pure and platform independent.
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageEncoder, ImageFormat, RgbImage};
use lowk_types::{AppError, ErrorCode, Result};

/// Long edge of the first attempt (BRIEF / ARCHITECTURE: <= 1568 px).
pub const SEND_EDGE: u32 = 1568;
/// Long edges of the retries after a "too large" refusal (413).
pub const RETRY_EDGES: [u32; 2] = [1024, 640];
pub const JPEG_QUALITY: u8 = 85;

#[derive(Debug, Clone)]
pub struct Prepared {
    /// `jpg` or `png`.
    pub ext: &'static str,
    pub mime: &'static str,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Prepared {
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.mime, crate::base64(&self.bytes))
    }
}

fn bad(what: &str, e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Unknown).with_detail(format!("{what}: {e}"))
}

/// Target size for `long_edge`, keeping the aspect ratio; never upscales.
pub fn target_size(w: u32, h: u32, long_edge: u32) -> (u32, u32) {
    let long = w.max(h).max(1);
    if long <= long_edge {
        return (w.max(1), h.max(1));
    }
    let k = f64::from(long_edge) / f64::from(long);
    (
        ((f64::from(w) * k).round() as u32).max(1),
        ((f64::from(h) * k).round() as u32).max(1),
    )
}

/// Decodes `png`, scales it to `long_edge` and encodes the smaller of JPEG q85 and PNG.
pub fn prepare(png: &[u8], long_edge: u32) -> Result<Prepared> {
    let img = image::load_from_memory_with_format(png, ImageFormat::Png).map_err(|e| bad("decode", e))?;
    let (w, h) = target_size(img.width(), img.height(), long_edge);
    let img = if (w, h) == (img.width(), img.height()) {
        img
    } else {
        img.resize_exact(w, h, FilterType::Triangle)
    };
    let rgb: RgbImage = DynamicImage::ImageRgb8(img.to_rgb8()).to_rgb8();
    let mut jpg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpg, JPEG_QUALITY)
        .write_image(rgb.as_raw(), w, h, image::ExtendedColorType::Rgb8)
        .map_err(|e| bad("jpeg", e))?;
    let mut out_png = Vec::new();
    image::codecs::png::PngEncoder::new_with_quality(
        &mut out_png,
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Adaptive,
    )
    .write_image(rgb.as_raw(), w, h, image::ExtendedColorType::Rgb8)
    .map_err(|e| bad("png", e))?;
    Ok(if out_png.len() < jpg.len() {
        Prepared {
            ext: "png",
            mime: "image/png",
            bytes: out_png,
            width: w,
            height: h,
        }
    } else {
        Prepared {
            ext: "jpg",
            mime: "image/jpeg",
            bytes: jpg,
            width: w,
            height: h,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_of(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&f(x, y));
            }
        }
        crate::encode_png(&px, w, h).unwrap()
    }

    #[test]
    fn size_keeps_aspect_and_never_upscales() {
        assert_eq!(target_size(3000, 1500, 1568), (1568, 784));
        assert_eq!(target_size(1000, 2000, 1000), (500, 1000));
        assert_eq!(target_size(800, 600, 1568), (800, 600));
    }

    #[test]
    fn flat_ui_picks_png_noise_picks_jpeg_and_both_are_scaled() {
        let flat = png_of(2000, 1000, |x, _| {
            if x % 40 < 20 {
                [255, 255, 255, 255]
            } else {
                [20, 20, 20, 255]
            }
        });
        let p = prepare(&flat, SEND_EDGE).unwrap();
        assert_eq!((p.width, p.height), (1568, 784));
        assert_eq!(p.ext, "png");
        let noise = png_of(900, 600, |x, y| {
            let v = (x.wrapping_mul(2654435761u32) ^ y.wrapping_mul(40503)) as u8;
            [v, v.wrapping_mul(3), v.wrapping_add(90), 255]
        });
        let p = prepare(&noise, SEND_EDGE).unwrap();
        assert_eq!(p.ext, "jpg");
        assert!(p.data_url().starts_with("data:image/jpeg;base64,"));
        let small = prepare(&flat, RETRY_EDGES[0]).unwrap();
        assert!(small.width <= 1024);
    }
}
