//! Bounded workspace image loading. No disk IO or decoding runs on the UI thread.
//!
//! One shared loader owns every markdown image. Decoded pixels are cached by
//! source so a row re-projected by a history refresh renders in the same
//! frame instead of falling back to its label; identical sources in flight
//! are coalesced into a single decode and every subscribed block model is
//! patched when the pixels arrive. Without the cache, each durable refresh
//! rebuilt the blocks with empty images and re-decoded everything, which is
//! what made restored pictures blink between thumbnail and path text.
use crate::{message_blocks::Blocks, MdBlock};
use slint::{Model, ModelRc, VecModel};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    sync::{mpsc, Arc, OnceLock},
    time::Duration,
};
use wunder_desktop::NativeDesktop;

/// Decoded thumbnails are at most 640×400 RGBA (~1 MB); the cache stays a
/// bounded fraction of the session's working set.
const CACHE_CAPACITY: usize = 32;
/// Total decoded RGBA kept by the cache. Animated GIFs keep every frame, so
/// the entry count alone is not a memory bound.
const CACHE_BYTE_BUDGET: usize = 96 * 1024 * 1024;
/// Animated GIF bounds: frames are display-sized and the whole animation has
/// to fit a fixed RGBA budget so a long GIF cannot exhaust the phone's memory.
const GIF_MAX_FRAMES: usize = 48;
const GIF_MAX_WIDTH: u32 = 480;
const GIF_MAX_HEIGHT: u32 = 360;
const GIF_FRAME_BUDGET: usize = 24 * 1024 * 1024;
/// Full worker queues are retried on a slow tick; a permanently saturated
/// worker releases the subscribers instead of parking them forever.
const VERSION_CAPACITY: usize = 512;
const RETRY_DELAY: Duration = Duration::from_millis(100);
const RETRY_LIMIT: u32 = 50;

struct Pixels {
    width: u32,
    height: u32,
    bytes: Vec<u8>,
}

/// One decoded image. A still has exactly one frame; a GIF keeps every
/// display-sized frame plus the delay the decoder measured.
struct Decoded {
    frames: Vec<Pixels>,
    delays: Vec<u32>,
}

/// UI-thread form of [`Decoded`]: Slint images plus delays.
#[derive(Clone)]
struct DecodedImage {
    frames: Vec<slint::Image>,
    delays: Vec<u32>,
}

/// Version of the file a still was decoded from: `(byte length, modified
/// nanos)`. A picture replaced in place keeps its path, so cached pixels are
/// only valid while this stamp matches.
type Stamp = (u64, u128);

struct Job {
    desktop: Arc<NativeDesktop>,
    agent: String,
    sources: Vec<(String, Option<Stamp>)>,
    result: mpsc::SyncSender<Vec<(String, Option<Stamp>, Option<Decoded>)>>,
}

struct CacheEntry {
    image: DecodedImage,
    stamp: Option<Stamp>,
}

struct Inflight {
    stamp: Option<Stamp>,
    models: Vec<ModelRc<MdBlock>>,
}

/// The loader's UI-thread state: pixel cache, LRU order and the models
/// waiting on each in-flight source.
#[derive(Default)]
struct Loader {
    cache: HashMap<String, CacheEntry>,
    order: VecDeque<String>,
    inflight: HashMap<String, Inflight>,
    /// File version each source was decoded from, kept after the pixels are
    /// evicted: an already painted block can still be recognized as stale
    /// without holding its megabytes of RGBA.
    versions: HashMap<String, Option<Stamp>>,
}

impl Loader {
    /// Cache hit for exactly this file version. A changed stamp drops the
    /// entry, so the caller decodes the new bytes instead of showing the
    /// picture that used to live at that path.
    fn cached(&mut self, source: &str, stamp: Option<&Stamp>) -> Option<DecodedImage> {
        let entry = self.cache.get(source)?;
        if entry.stamp.as_ref() != stamp {
            self.cache.remove(source);
            if let Some(position) = self.order.iter().position(|key| key == source) {
                self.order.remove(position);
            }
            return None;
        }
        let image = entry.image.clone();
        if let Some(position) = self.order.iter().position(|key| key == source) {
            self.order.remove(position);
        }
        self.order.push_back(source.to_string());
        Some(image)
    }

    /// True when the cached pixels belong to an older version of the file.
    /// Sources without a cache entry claim nothing, so an evicted entry never
    /// forces a needless decode of an already painted block.
    fn stale(&self, source: &str, stamp: Option<&Stamp>) -> bool {
        match self.versions.get(source) {
            Some(known) => known.as_ref() != stamp,
            None => false,
        }
    }

    fn remember(&mut self, source: String, image: DecodedImage, stamp: Option<Stamp>) {
        // The composer's data-URL echo is replaced by the durable stored path
        // at the next history refresh; caching megabytes of URL text would
        // only bloat the key space.
        if source.starts_with("data:") {
            return;
        }
        let bytes = decoded_bytes(&image);
        let held: usize = self
            .cache
            .values()
            .map(|entry| decoded_bytes(&entry.image))
            .fold(0usize, usize::saturating_add);
        if held.saturating_add(bytes) > CACHE_BYTE_BUDGET {
            // Block models already hold their own image handles; dropping the
            // lookup only makes a later projection decode again.
            self.cache.clear();
            self.order.clear();
        }
        self.versions.insert(source.clone(), stamp.clone());
        if self.versions.len() > VERSION_CAPACITY {
            self.versions.clear();
        }
        if self
            .cache
            .insert(source.clone(), CacheEntry { image, stamp })
            .is_some()
        {
            if let Some(position) = self.order.iter().position(|key| *key == source) {
                self.order.remove(position);
            }
        }
        self.order.push_back(source);
        while self.order.len() > CACHE_CAPACITY {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.cache.remove(&oldest);
        }
    }
}

thread_local! {
    static LOADER: RefCell<Loader> = RefCell::new(Loader::default());
}

fn same_model(a: &ModelRc<MdBlock>, b: &ModelRc<MdBlock>) -> bool {
    std::ptr::eq(a.as_any(), b.as_any())
}

fn decoded_bytes(image: &DecodedImage) -> usize {
    image
        .frames
        .iter()
        .map(|frame| {
            let size = frame.size();
            size.width as usize * size.height as usize * 4
        })
        .fold(0usize, usize::saturating_add)
}

/// Byte source for one reference: the composer's data URLs decode inline,
/// everything else resolves through the private workspace remap.
fn load(desktop: &NativeDesktop, agent: &str, source: &str) -> Option<Decoded> {
    if let Some(bytes) = inline_data_bytes(source) {
        return decode(&bytes);
    }
    desktop
        .workspace_image_bytes(agent, source)
        .ok()
        .and_then(|bytes| decode(&bytes))
}

fn inline_data_bytes(source: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let meta_payload = source.strip_prefix("data:")?;
    let (meta, payload) = meta_payload.split_once(',')?;
    if !meta
        .split(';')
        .any(|part| part.trim().eq_ignore_ascii_case("base64"))
    {
        return None;
    }
    base64::engine::general_purpose::STANDARD
        .decode(payload)
        .ok()
}

/// File version for one source. The composer's data URLs decode inline and
/// carry no file, so they never age out.
fn image_stamp(desktop: &NativeDesktop, agent: &str, source: &str) -> Option<Stamp> {
    if source.starts_with("data:") {
        return None;
    }
    desktop.workspace_image_stamp(agent, source).ok()
}

fn worker() -> &'static mpsc::SyncSender<Job> {
    static QUEUE: OnceLock<mpsc::SyncSender<Job>> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Job>(64);
        std::thread::spawn(move || {
            while let Ok(job) = rx.recv() {
                let result = job
                    .sources
                    .into_iter()
                    .map(|(source, stamp)| {
                        let pixels = load(&job.desktop, &job.agent, &source);
                        (source, stamp, pixels)
                    })
                    .collect();
                let _ = job.result.send(result);
            }
        });
        tx
    })
}

fn decode(bytes: &[u8]) -> Option<Decoded> {
    if image::guess_format(bytes).ok() == Some(image::ImageFormat::Gif) {
        if let Some(animation) = decode_gif(bytes) {
            return Some(animation);
        }
    }
    Some(Decoded {
        frames: vec![decode_still(bytes)?],
        delays: Vec::new(),
    })
}

fn decode_still(bytes: &[u8]) -> Option<Pixels> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?.thumbnail(640, 400).to_rgba8();
    Some(Pixels {
        width: image.width(),
        height: image.height(),
        bytes: image.into_raw(),
    })
}

/// Shrink one GIF frame to the display box without ever upscaling a small
/// animation into megabytes of texture.
fn bounded_frame(buffer: image::RgbaImage) -> image::RgbaImage {
    let (width, height) = buffer.dimensions();
    if width <= GIF_MAX_WIDTH && height <= GIF_MAX_HEIGHT {
        return buffer;
    }
    let ratio = (GIF_MAX_WIDTH as f32 / width as f32).min(GIF_MAX_HEIGHT as f32 / height as f32);
    let target_width = ((width as f32 * ratio).round() as u32).max(1);
    let target_height = ((height as f32 * ratio).round() as u32).max(1);
    image::DynamicImage::ImageRgba8(buffer)
        .thumbnail(target_width, target_height)
        .to_rgba8()
}

/// Decode every GIF frame up to the display budget. Returns `None` when the
/// file has a single frame (a static GIF falls through to the still path).
fn decode_gif(bytes: &[u8]) -> Option<Decoded> {
    use image::{AnimationDecoder, ImageDecoder};
    let mut decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    decoder.set_limits(limits).ok()?;
    let mut frames = Vec::new();
    let mut delays = Vec::new();
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
        frames.push(Pixels {
            width: image.width(),
            height: image.height(),
            bytes: image.into_raw(),
        });
        // Browsers treat a missing or sub-frame delay as ~100ms; anything
        // else is bounded so a huge or zero delay cannot stall or spin.
        delays.push(if delay < 20 {
            100
        } else {
            delay.clamp(40, 1000)
        });
    }
    (frames.len() > 1).then_some(Decoded { frames, delays })
}

pub fn hydrate(blocks: &mut Blocks, desktop: Arc<NativeDesktop>, agent: String) {
    hydrate_model(slint::ModelRc::from(blocks.model.clone()), desktop, agent);
}

/// Hydrate an already projected block model. History rows are projected as
/// `TextBlock` models directly, so they cannot retain the temporary `Blocks`
/// builder used by streaming output. Cached sources apply synchronously and
/// only the remaining sources queue a decode. A picture whose file changed on
/// disk is dropped and decoded again: the cache is keyed by path, so without
/// the version check an image replaced in place kept showing the pixels that
/// used to live at that path for the rest of the session.
///
/// Versions come from file metadata only and each source is stat'ed once per
/// call, so repeated publishes of the same history stay cheap.
pub fn hydrate_model(model: slint::ModelRc<MdBlock>, desktop: Arc<NativeDesktop>, agent: String) {
    let mut to_fetch: Vec<(String, Option<Stamp>)> = Vec::new();
    let mut stamps: HashMap<String, Option<Stamp>> = HashMap::new();
    LOADER.with(|loader| {
        let mut loader = loader.borrow_mut();
        for index in 0..model.row_count() {
            let Some(mut block) = model.row_data(index) else {
                continue;
            };
            if block.kind != crate::message_blocks::KIND_IMAGE || block.image_source.is_empty() {
                continue;
            }
            let source = block.image_source.to_string();
            let stamp = stamps
                .entry(source.clone())
                .or_insert_with(|| image_stamp(&desktop, &agent, &source))
                .clone();
            if block.image.size().width > 0 {
                // Painted already: only a newer version of the file replaces
                // it, so a steady history keeps its textures untouched.
                if block.image_error || !loader.stale(&source, stamp.as_ref()) {
                    continue;
                }
                block.image = slint::Image::default();
                block.image_width = 0.0;
                block.image_height = 0.0;
                block.image_frames = ModelRc::default();
                block.frame_delays = ModelRc::default();
                model.set_row_data(index, block.clone());
            }
            if let Some(image) = loader.cached(&source, stamp.as_ref()) {
                fill(&mut block, &image);
                model.set_row_data(index, block);
                continue;
            }
            match loader.inflight.entry(source.clone()) {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    let inflight = entry.get_mut();
                    if inflight.stamp.as_ref() != stamp.as_ref() {
                        // The file moved on while the old bytes were decoding.
                        inflight.stamp = stamp.clone();
                        to_fetch.push((source.clone(), stamp.clone()));
                    }
                    if !inflight.models.iter().any(|held| same_model(held, &model)) {
                        inflight.models.push(model.clone());
                    }
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Inflight {
                        stamp: stamp.clone(),
                        models: vec![model.clone()],
                    });
                    to_fetch.push((source.clone(), stamp.clone()));
                }
            }
        }
    });
    dispatch(desktop, agent, to_fetch);
}

fn dispatch(desktop: Arc<NativeDesktop>, agent: String, sources: Vec<(String, Option<Stamp>)>) {
    if sources.is_empty() {
        return;
    }
    let (tx, rx) = mpsc::sync_channel(1);
    let job = Job {
        desktop,
        agent,
        sources,
        result: tx,
    };
    send(job, rx, 0);
}

fn send(job: Job, rx: mpsc::Receiver<Vec<(String, Option<Stamp>, Option<Decoded>)>>, attempt: u32) {
    match worker().try_send(job) {
        Ok(()) => poll(rx),
        Err(mpsc::TrySendError::Full(job)) if attempt < RETRY_LIMIT => {
            slint::Timer::single_shot(RETRY_DELAY, move || send(job, rx, attempt + 1));
        }
        Err(error) => {
            // The worker is gone or permanently saturated: release the
            // subscribers so the next hydration can retry instead of waiting
            // on a decode that never starts.
            let sources = match error {
                mpsc::TrySendError::Full(job) | mpsc::TrySendError::Disconnected(job) => {
                    job.sources
                }
            };
            release(&sources);
        }
    }
}

/// Deliver one finished job: fill the cache, then patch every subscribed
/// model. Runs on the UI thread through the polling timer.
fn deliver(images: Vec<(String, Option<Stamp>, Option<Decoded>)>) {
    LOADER.with(|loader| {
        let mut loader = loader.borrow_mut();
        for (source, stamp, decoded) in images {
            let image = decoded.map(|decoded| DecodedImage {
                frames: decoded
                    .frames
                    .into_iter()
                    .map(|p| {
                        slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(
                            &p.bytes, p.width, p.height,
                        ))
                    })
                    .collect(),
                delays: decoded.delays,
            });
            if let Some(image) = &image {
                loader.remember(source.clone(), image.clone(), stamp.clone());
            }
            let Some(inflight) = loader.inflight.remove(&source) else {
                continue;
            };
            if inflight.stamp != stamp {
                // The file changed again while these bytes decoded: keep the
                // subscribers for the newer request instead of painting stale
                // pixels over the fresh ones.
                loader.inflight.insert(source.clone(), inflight);
                continue;
            }
            for model in inflight.models {
                apply(&model, &source, image.as_ref());
            }
        }
    });
}

/// A source whose job never ran frees its subscribers; they stay on their
/// label until a later hydration re-queues the decode.
fn release(sources: &[(String, Option<Stamp>)]) {
    LOADER.with(|loader| {
        let mut loader = loader.borrow_mut();
        for (source, _) in sources {
            loader.inflight.remove(source);
        }
    });
}

/// Store the pixels together with their aspect ratio: the Slint side sizes a
/// picture from `image-width`/`image-height` before the texture arrives, and a
/// zero ratio would render every image at the 16:9 default.
fn fill(block: &mut MdBlock, image: &DecodedImage) {
    let Some(first) = image.frames.first() else {
        return;
    };
    let size = first.size();
    block.image_width = size.width as f32;
    block.image_height = size.height as f32;
    block.image = first.clone();
    if image.frames.len() > 1 {
        block.image_frames = ModelRc::new(VecModel::from(image.frames.clone()));
        block.frame_delays = ModelRc::new(VecModel::from(
            image
                .delays
                .iter()
                .map(|delay| *delay as i32)
                .collect::<Vec<_>>(),
        ));
    } else {
        block.image_frames = ModelRc::default();
        block.frame_delays = ModelRc::default();
    }
}

fn apply(model: &ModelRc<MdBlock>, source: &str, image: Option<&DecodedImage>) {
    for index in 0..model.row_count() {
        let Some(mut block) = model.row_data(index) else {
            continue;
        };
        if block.image_source != source {
            continue;
        }
        if let Some(image) = image {
            fill(&mut block, image);
            model.set_row_data(index, block);
        } else {
            // A missing/unsupported image keeps its label but collapses the
            // card to a one-line notice instead of a blank 220px box.
            block.image_error = true;
            model.set_row_data(index, block);
        }
    }
}

fn poll(rx: mpsc::Receiver<Vec<(String, Option<Stamp>, Option<Decoded>)>>) {
    slint::Timer::single_shot(Duration::from_millis(33), move || match rx.try_recv() {
        Ok(images) => deliver(images),
        Err(mpsc::TryRecvError::Empty) => poll(rx),
        Err(mpsc::TryRecvError::Disconnected) => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_image_decoder_keeps_pixels_and_rejects_invalid_data() {
        let image = decode(include_bytes!("../assets/markdown-example.png")).unwrap();
        assert_eq!(image.frames.len(), 1);
        assert!(image.delays.is_empty());
        assert_eq!((image.frames[0].width, image.frames[0].height), (640, 280));
        assert_eq!(image.frames[0].bytes.len(), 640 * 280 * 4);
        assert!(decode(b"not an image").is_none());
    }

    #[test]
    fn gif_decoding_keeps_frames_and_delays() {
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
        let decoded = decode(&encoded).expect("animated GIF decodes");
        assert_eq!(decoded.frames.len(), 3);
        assert_eq!(decoded.delays, vec![120, 120, 120]);
        assert_eq!((decoded.frames[0].width, decoded.frames[0].height), (8, 6));
    }

    #[test]
    fn inline_data_bytes_parses_base64_data_urls_only() {
        use base64::Engine;
        let payload = base64::engine::general_purpose::STANDARD.encode(b"pixels");
        let url = format!("data:image/png;base64,{payload}");
        assert_eq!(inline_data_bytes(&url), Some(b"pixels".to_vec()));
        assert_eq!(inline_data_bytes("data:image/png;base64"), None);
        assert_eq!(inline_data_bytes("data:image/png;utf8,pixels"), None);
        assert_eq!(inline_data_bytes("workspaces/pic.png"), None);
    }

    #[test]
    fn cache_evicts_the_oldest_entry_and_skips_data_urls() {
        let mut loader = Loader::default();
        let pixel = || DecodedImage {
            frames: vec![slint::Image::from_rgba8(
                slint::SharedPixelBuffer::clone_from_slice(&[255, 0, 0, 255], 1, 1),
            )],
            delays: Vec::new(),
        };
        for index in 0..CACHE_CAPACITY + 4 {
            loader.remember(format!("workspaces/pic-{index}.png"), pixel(), Some((1, 0)));
        }
        assert_eq!(loader.cache.len(), CACHE_CAPACITY);
        assert!(loader
            .cached("workspaces/pic-0.png", Some(&(1, 0)))
            .is_none());
        assert!(loader
            .cached(
                &format!("workspaces/pic-{}.png", CACHE_CAPACITY + 3),
                Some(&(1, 0))
            )
            .is_some());
        loader.remember("data:image/png;base64,AAAA".into(), pixel(), None);
        assert!(loader.cached("data:image/png;base64,AAAA", None).is_none());
    }

    #[test]
    fn a_changed_file_version_invalidates_cached_pixels() {
        let mut loader = Loader::default();
        let pixel = DecodedImage {
            frames: vec![slint::Image::from_rgba8(
                slint::SharedPixelBuffer::clone_from_slice(&[255, 0, 0, 255], 1, 1),
            )],
            delays: Vec::new(),
        };
        loader.remember("workspaces/pic.png".into(), pixel, Some((10, 1)));
        // Same file version: the cached pixels are reused.
        assert!(loader
            .cached("workspaces/pic.png", Some(&(10, 1)))
            .is_some());
        assert!(!loader.stale("workspaces/pic.png", Some(&(10, 1))));
        // Rewritten file: the entry is reported stale and dropped.
        assert!(loader.stale("workspaces/pic.png", Some(&(11, 1))));
        assert!(loader
            .cached("workspaces/pic.png", Some(&(11, 1)))
            .is_none());
        assert!(loader.cache.is_empty());
        // A source without a cached entry never claims to be stale.
        assert!(!loader.stale("workspaces/other.png", Some(&(1, 1))));
    }
}
