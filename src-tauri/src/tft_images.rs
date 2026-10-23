//! Local image decoding; HID framing lives in ak820-protocol::legacy.
use ak820_protocol::commands::tft::{TftAnimation, TftFrame};
use image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use serde::Serialize;
use std::io::Cursor;

pub const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct PreparedImage {
    pub animation: TftAnimation,
    pub timing_adjusted: bool,
}

fn limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    limits
}

fn frame(image: DynamicImage, delay_ms: u16) -> Result<TftFrame, String> {
    let rgba = image
        .resize_to_fill(128, 128, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    // Transparent pixels are composited onto black before RGB565 conversion.
    let mut rgb = Vec::with_capacity(128 * 128 * 3);
    for p in rgba.pixels() {
        for channel in &p.0[..3] {
            rgb.push((u16::from(*channel) * u16::from(p[3]) / 255) as u8);
        }
    }
    TftFrame::from_rgb888(&rgb, delay_ms).map_err(|e| e.to_string())
}

pub fn prepare(bytes: &[u8]) -> Result<PreparedImage, String> {
    if bytes.is_empty() || bytes.len() > MAX_FILE_BYTES {
        return Err("Choose an image file no larger than 10 MB".into());
    }
    let format = image::guess_format(bytes).map_err(|e| e.to_string())?;
    let mut frames = Vec::new();
    let mut timing_adjusted = false;
    if format == ImageFormat::Gif {
        let mut decoder =
            image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        decoder.set_limits(limits()).map_err(|e| e.to_string())?;
        for f in decoder.into_frames().take(31) {
            if frames.len() == 30 {
                return Err("This GIF has more than 30 frames. Choose a shorter GIF.".into());
            }
            let f = f.map_err(|e| e.to_string())?;
            let (n, d) = f.delay().numer_denom_ms();
            let ms = n / d.max(1);
            let normalized = if ms == 0 {
                100
            } else {
                ((ms.clamp(2, 510) + 1) / 2) * 2
            };
            timing_adjusted |= normalized != ms;
            frames.push(frame(
                DynamicImage::ImageRgba8(f.into_buffer()),
                normalized as u16,
            )?);
        }
    } else if matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
        reader.limits(limits());
        frames.push(frame(reader.decode().map_err(|e| e.to_string())?, 0)?);
    } else {
        return Err("Choose PNG, JPEG, WebP or GIF".into());
    }
    if frames.len() == 1 {
        frames[0].delay_ms = 0;
        timing_adjusted = false;
    }
    let animation = TftAnimation { frames };
    ak820_protocol::legacy::encode_tft(&animation).map_err(|e| e.to_string())?;
    Ok(PreparedImage {
        animation,
        timing_adjusted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{codecs::gif::GifEncoder, Delay, Frame, Rgba, RgbaImage};

    #[test]
    fn transparent_static_image_is_flattened_on_black() {
        let rgba = RgbaImage::from_pixel(256, 128, Rgba([255, 0, 0, 0]));
        let mut png = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(rgba)
            .write_to(&mut png, ImageFormat::Png)
            .unwrap();
        let prepared = prepare(png.get_ref()).unwrap();
        assert_eq!(prepared.animation.frames.len(), 1);
        assert_eq!(prepared.animation.frames[0].pixels, vec![0; 32768]);
    }

    #[test]
    fn gif_frames_and_last_frame_delay_are_preserved_and_bounded() {
        let mut bytes = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut bytes);
            for delay in [100, 1000] {
                encoder
                    .encode_frame(Frame::from_parts(
                        RgbaImage::from_pixel(8, 8, Rgba([255, 0, 0, 255])),
                        0,
                        0,
                        Delay::from_numer_denom_ms(delay, 1),
                    ))
                    .unwrap();
            }
        }
        let prepared = prepare(&bytes).unwrap();
        assert!(prepared.timing_adjusted);
        assert_eq!(
            prepared
                .animation
                .frames
                .iter()
                .map(|f| f.delay_ms)
                .collect::<Vec<_>>(),
            [100, 510]
        );
        assert_eq!(
            &ak820_protocol::legacy::encode_tft(&prepared.animation).unwrap()[..3],
            &[2, 50, 255]
        );
    }

    #[test]
    fn too_many_gif_frames_are_rejected_without_silent_truncation() {
        let mut bytes = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut bytes);
            for _ in 0..31 {
                encoder
                    .encode_frame(Frame::new(RgbaImage::from_pixel(
                        1,
                        1,
                        Rgba([0, 0, 0, 255]),
                    )))
                    .unwrap();
            }
        }
        assert!(prepare(&bytes).unwrap_err().contains("30 frames"));
    }
}
