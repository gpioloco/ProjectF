use crate::theme::{Effects, Wallpaper};
use anyhow::{Context, Result, ensure};
use image::{DynamicImage, ImageReader, Rgb, RgbImage, codecs::jpeg::JpegEncoder};
use std::{
    fs::File,
    io::{BufReader, Cursor},
};

pub fn load(source: &Wallpaper) -> Result<RgbImage> {
    fn decode<R: std::io::BufRead + std::io::Seek>(input: R) -> Result<DynamicImage> {
        let reader = ImageReader::new(input).with_guessed_format()?;
        let mut decoder = reader.into_decoder()?;
        use image::ImageDecoder;
        let (width, height) = decoder.dimensions();
        ensure!(
            width > 0 && height > 0 && width as u64 * height as u64 <= 40_000_000,
            "Choose an image of up to 40 megapixels."
        );
        let orientation = decoder.orientation()?;
        let mut img = DynamicImage::from_decoder(decoder)?;
        img.apply_orientation(orientation);
        Ok(img)
    }
    let img = match source {
        Wallpaper::Bundled(photo) => decode(Cursor::new(photo.bytes()))?,
        Wallpaper::Local(path) => decode(BufReader::new(
            File::open(path).with_context(|| format!("Open {}", path.display()))?,
        ))?,
    };
    Ok(img.into_rgb8())
}

fn random(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state >> 8) as f32 / 16_777_216.0
}

/// A fixed seed keeps drop positions consistent between the thumbnail and the desktop.
pub fn render(input: &RgbImage, fx: Effects) -> RgbImage {
    let (w, h) = input.dimensions();
    let scale = w.min(h) as f32;
    let mut out = if fx.blur == 0 {
        input.clone()
    } else {
        image::imageops::fast_blur(
            input,
            (scale * 0.019 * f32::from(fx.blur) / 100.0).max(0.25),
        )
    };
    if fx.rain > 0 {
        let under = out.clone();
        let mut seed = 0x6b91_ea27;
        for _ in 0..usize::from(fx.rain) * 3 {
            let cx = random(&mut seed) * w as f32;
            let cy = random(&mut seed) * h as f32;
            let rx = (0.002 + random(&mut seed).powi(2) * 0.012) * scale;
            let ry = rx * (1.08 + random(&mut seed) * 0.53);
            for y in ((cy - ry).max(0.0) as u32)..(((cy + ry).ceil() as u32).min(h)) {
                for x in ((cx - rx).max(0.0) as u32)..(((cx + rx).ceil() as u32).min(w)) {
                    let dx = (x as f32 - cx) / rx;
                    let dy = (y as f32 - cy) / ry;
                    let d = dx * dx + dy * dy;
                    if d >= 1.0 {
                        continue;
                    }
                    let lens = (1.0 - d).sqrt();
                    let sx = (x as f32 - dx * rx * lens * 1.6).clamp(0.0, (w - 1) as f32) as u32;
                    let sy = (y as f32 - dy * ry * lens * 1.6).clamp(0.0, (h - 1) as f32) as u32;
                    let glint = (1.0 - ((dx + 0.34).powi(2) + (dy + 0.58).powi(2)) * 13.0).max(0.0);
                    let light = glint * 86.0 + dy * d * 30.0 - 11.0 * d;
                    let edge = ((1.0 - d) * 8.0).min(1.0);
                    let original = under.get_pixel(x, y);
                    let refracted = under.get_pixel(sx, sy);
                    out.put_pixel(
                        x,
                        y,
                        Rgb(std::array::from_fn(|i| {
                            (original[i] as f32 * (1.0 - edge)
                                + (refracted[i] as f32 + light) * edge)
                                .clamp(0.0, 255.0) as u8
                        })),
                    );
                }
            }
        }
    }
    if fx.grain > 0 {
        let mut seed = 0xae3c_1b85;
        for pixel in out.pixels_mut() {
            let offset = (random(&mut seed) - 0.5) * f32::from(fx.grain) * 0.82;
            pixel.0 = pixel
                .0
                .map(|v| (f32::from(v) + offset).round().clamp(0.0, 255.0) as u8);
        }
    }
    out
}

pub fn jpeg(image: &RgbImage) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 93).encode_image(image)?;
    Ok(bytes)
}
