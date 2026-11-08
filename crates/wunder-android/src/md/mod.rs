//! Markdown rendering for the chat transcript.
//!
//! Slint's `StyledText` is an inline primitive: it parses a CommonMark subset
//! and rejects headings, lists-as-structure, code blocks, tables, quotes and
//! images, and it exposes no block spacing or per-span font size. Rendering a
//! document therefore needs two layers:
//!
//! | layer | owner | responsibility |
//! |---|---|---|
//! | block | `ui/markdown-rich.slint` + `project.rs` | kind, size, spacing, nesting, table columns |
//! | inline | `StyledText` via `inline.rs` | emphasis, inline code, links, colour |
//!
//! Parsing is deliberately *not* incremental. The chat publishes live output
//! as plain text and calls [`project::MarkdownRenderer::render`] once, when the
//! turn reaches a terminal state: a 300-block answer costs ~13 ms to project,
//! while re-projecting it on every stream tick would cost more than the model
//! takes to emit the next token.

pub mod ast;
pub mod block;
pub mod inline;
pub mod parse;
pub mod project;
pub mod source;

pub use project::MarkdownRenderer;

/// One decoded image: every frame shares the canvas size. A still image has
/// exactly one frame and no delays; a GIF keeps its display-sized frames plus
/// the per-frame delay measured by the decoder.
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<Vec<u8>>,
    pub delays: Vec<u32>,
}

/// Animated GIF bounds: frames are display-sized and the whole animation has
/// to fit a fixed RGBA budget so a long GIF cannot exhaust the phone's memory.
const GIF_MAX_FRAMES: usize = 48;
const GIF_MAX_WIDTH: u32 = 480;
const GIF_MAX_HEIGHT: u32 = 360;
const GIF_FRAME_BUDGET: usize = 24 * 1024 * 1024;

/// Decode workspace image bytes into display-sized RGBA. A GIF is decoded
/// frame by frame so the transcript can animate it; everything else becomes
/// one thumbnail.
pub fn decode_image(bytes: &[u8]) -> Option<DecodedImage> {
    if image::guess_format(bytes).ok() == Some(image::ImageFormat::Gif) {
        if let Some(animation) = decode_gif(bytes) {
            return Some(animation);
        }
    }
    let (width, height, rgba) = decode_still(bytes)?;
    Some(DecodedImage { width, height, frames: vec![rgba], delays: Vec::new() })
}

fn decode_still(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?.thumbnail(640, 400).to_rgba8();
    Some((image.width(), image.height(), image.into_raw()))
}

/// Decode every GIF frame up to the display budget. Returns `None` when the
/// file has a single frame (a static GIF falls through to the still path).
fn decode_gif(bytes: &[u8]) -> Option<DecodedImage> {
    use image::{AnimationDecoder, ImageDecoder};
    let mut decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    decoder.set_limits(limits).ok()?;
    let mut frames = Vec::new();
    let mut delays = Vec::new();
    let (mut width, mut height) = (0u32, 0u32);
    let mut total = 0usize;
    for frame in decoder.into_frames().take(GIF_MAX_FRAMES) {
        let Ok(frame) = frame else { break };
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let delay = if denominator == 0 {
            100
        } else {
            (numerator as f64 / denominator as f64).round() as u32
        };
        let image = bounded_frame(frame.into_buffer());
        let bytes = image.as_raw().len();
        if !frames.is_empty() && total.saturating_add(bytes) > GIF_FRAME_BUDGET {
            break;
        }
        total += bytes;
        width = image.width();
        height = image.height();
        frames.push(image.into_raw());
        // Browsers treat a missing or sub-frame delay as ~100ms; anything
        // else is bounded so a huge or zero delay cannot stall or spin.
        delays.push(if delay < 20 { 100 } else { delay.clamp(40, 1000) });
    }
    if frames.len() <= 1 {
        return None;
    }
    Some(DecodedImage { width, height, frames, delays })
}

/// Shrink one GIF frame to the display box without ever upscaling a small
/// animation into megabytes of texture.
fn bounded_frame(buffer: image::RgbaImage) -> image::RgbaImage {
    let (width, height) = buffer.dimensions();
    if width <= GIF_MAX_WIDTH && height <= GIF_MAX_HEIGHT {
        return buffer;
    }
    let ratio = (GIF_MAX_WIDTH as f32 / width as f32)
        .min(GIF_MAX_HEIGHT as f32 / height as f32);
    let target_width = ((width as f32 * ratio).round() as u32).max(1);
    let target_height = ((height as f32 * ratio).round() as u32).max(1);
    image::DynamicImage::ImageRgba8(buffer)
        .thumbnail(target_width, target_height)
        .to_rgba8()
}

#[cfg(test)]
mod tests {
    #[test]
    fn thumbnails_keep_pixels_and_reject_garbage() {
        let image = super::decode_image(include_bytes!(
            "../../../wunder-desktop/frontend/assets/markdown-example.png"
        ))
        .expect("decode the sample image");
        assert_eq!((image.width, image.height), (640, 280));
        assert_eq!(image.frames.len(), 1);
        assert!(image.delays.is_empty());
        assert_eq!(image.frames[0].len(), 640 * 280 * 4);
        assert!(super::decode_image(b"not an image").is_none());
    }

    #[test]
    fn gif_decoding_keeps_every_frame_and_delay() {
        use image::{Delay, Frame, Rgba, RgbaImage};
        let frame = |red: u8| {
            Frame::from_parts(
                RgbaImage::from_pixel(8, 6, Rgba([red, 0, 0, 255])),
                0,
                0,
                Delay::from_numer_denom_ms(120, 1),
            )
        };
        let mut encoded = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut encoded);
            encoder.encode_frame(frame(255)).unwrap();
            encoder.encode_frame(frame(128)).unwrap();
            encoder.encode_frame(frame(0)).unwrap();
        }
        let decoded = super::decode_image(&encoded).expect("animated GIF decodes");
        assert_eq!(decoded.frames.len(), 3);
        assert_eq!(decoded.delays, vec![120, 120, 120]);
        assert_eq!((decoded.width, decoded.height), (8, 6));
        assert_eq!(decoded.frames[0].len(), 8 * 6 * 4);
    }
}
