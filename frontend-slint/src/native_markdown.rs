//! Bounded workspace image loading. No disk IO or decoding runs on the UI thread.
//!
//! One shared loader owns every markdown image. Decoded pixels are cached by
//! source so a row re-projected by a history refresh renders in the same
//! frame instead of falling back to its label; identical sources in flight
//! are coalesced into a single decode and every subscribed block model is
//! patched when the pixels arrive. Without the cache, each durable refresh
//! rebuilt the blocks with empty images and re-decoded everything, which is
//! what made restored pictures blink between thumbnail and path text.
use crate::{message_blocks::Blocks, TextBlock};
use slint::{Model, ModelRc};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    sync::{mpsc, Arc, OnceLock},
    time::Duration,
};
use wunder_desktop::NativeDesktop;

/// Decoded thumbnails are at most 640×400 RGBA (~1 MB); the cache stays a
/// bounded fraction of the session's working set.
const CACHE_CAPACITY: usize = 32;
/// Full worker queues are retried on a slow tick; a permanently saturated
/// worker releases the subscribers instead of parking them forever.
const RETRY_DELAY: Duration = Duration::from_millis(100);
const RETRY_LIMIT: u32 = 50;

struct Pixels {
    width: u32,
    height: u32,
    bytes: Vec<u8>,
}
struct Job {
    desktop: Arc<NativeDesktop>,
    agent: String,
    sources: Vec<String>,
    result: mpsc::SyncSender<Vec<(String, Option<Pixels>)>>,
}

/// The loader's UI-thread state: pixel cache, LRU order and the models
/// waiting on each in-flight source.
#[derive(Default)]
struct Loader {
    cache: HashMap<String, slint::Image>,
    order: VecDeque<String>,
    inflight: HashMap<String, Vec<ModelRc<TextBlock>>>,
}

impl Loader {
    fn cached(&mut self, source: &str) -> Option<slint::Image> {
        let image = self.cache.get(source)?.clone();
        if let Some(position) = self.order.iter().position(|key| key == source) {
            self.order.remove(position);
        }
        self.order.push_back(source.to_string());
        Some(image)
    }

    fn remember(&mut self, source: String, image: slint::Image) {
        // The composer's data-URL echo is replaced by the durable stored path
        // at the next history refresh; caching megabytes of URL text would
        // only bloat the key space.
        if source.starts_with("data:") {
            return;
        }
        if self.cache.insert(source.clone(), image).is_some() {
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

fn same_model(a: &ModelRc<TextBlock>, b: &ModelRc<TextBlock>) -> bool {
    std::ptr::eq(a.as_any(), b.as_any())
}

/// Byte source for one reference: the composer's data URLs decode inline,
/// everything else resolves through the private workspace remap.
fn load(desktop: &NativeDesktop, agent: &str, source: &str) -> Option<Pixels> {
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

fn worker() -> &'static mpsc::SyncSender<Job> {
    static QUEUE: OnceLock<mpsc::SyncSender<Job>> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Job>(64);
        std::thread::spawn(move || {
            while let Ok(job) = rx.recv() {
                let result = job
                    .sources
                    .into_iter()
                    .map(|source| {
                        let pixels = load(&job.desktop, &job.agent, &source);
                        (source, pixels)
                    })
                    .collect();
                let _ = job.result.send(result);
            }
        });
        tx
    })
}

fn decode(bytes: &[u8]) -> Option<Pixels> {
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

pub fn hydrate(blocks: &mut Blocks, desktop: Arc<NativeDesktop>, agent: String) {
    hydrate_model(slint::ModelRc::from(blocks.model.clone()), desktop, agent);
}

/// Hydrate an already projected block model. History rows are projected as
/// `TextBlock` models directly, so they cannot retain the temporary `Blocks`
/// builder used by streaming output. Blocks that already hold pixels are left
/// alone, cached sources apply synchronously, and only the remaining sources
/// queue a decode — repeated publishes of the same history are therefore
/// free and never blank a loaded image.
pub fn hydrate_model(model: slint::ModelRc<TextBlock>, desktop: Arc<NativeDesktop>, agent: String) {
    let mut to_fetch = Vec::new();
    LOADER.with(|loader| {
        let mut loader = loader.borrow_mut();
        let mut seen = HashSet::new();
        for index in 0..model.row_count() {
            let Some(mut block) = model.row_data(index) else {
                continue;
            };
            if block.kind != crate::message_blocks::KIND_IMAGE || block.image_source.is_empty() {
                continue;
            }
            if block.image.size().width > 0 {
                continue;
            }
            let source = block.image_source.to_string();
            if let Some(image) = loader.cached(&source) {
                block.image = image;
                model.set_row_data(index, block);
                continue;
            }
            if !seen.insert(source.clone()) {
                // One subscription per model and source; the delivery pass
                // patches every matching row of the subscriber.
                continue;
            }
            match loader.inflight.entry(source.clone()) {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    let subscribers = entry.get_mut();
                    if !subscribers.iter().any(|held| same_model(held, &model)) {
                        subscribers.push(model.clone());
                    }
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(vec![model.clone()]);
                    to_fetch.push(source);
                }
            }
        }
    });
    dispatch(desktop, agent, to_fetch);
}

fn dispatch(desktop: Arc<NativeDesktop>, agent: String, sources: Vec<String>) {
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

fn send(job: Job, rx: mpsc::Receiver<Vec<(String, Option<Pixels>)>>, attempt: u32) {
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
fn deliver(images: Vec<(String, Option<Pixels>)>) {
    LOADER.with(|loader| {
        let mut loader = loader.borrow_mut();
        for (source, pixels) in images {
            let image = pixels.map(|p| {
                slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(
                    &p.bytes, p.width, p.height,
                ))
            });
            if let Some(image) = &image {
                loader.remember(source.clone(), image.clone());
            }
            let subscribers = loader.inflight.remove(&source).unwrap_or_default();
            for model in subscribers {
                apply(&model, &source, image.as_ref());
            }
        }
    });
}

/// A source whose job never ran frees its subscribers; they stay on their
/// label until a later hydration re-queues the decode.
fn release(sources: &[String]) {
    LOADER.with(|loader| {
        let mut loader = loader.borrow_mut();
        for source in sources {
            loader.inflight.remove(source);
        }
    });
}

fn apply(model: &ModelRc<TextBlock>, source: &str, image: Option<&slint::Image>) {
    for index in 0..model.row_count() {
        let Some(mut block) = model.row_data(index) else {
            continue;
        };
        if block.image_source != source {
            continue;
        }
        if let Some(image) = image {
            block.image = image.clone();
            // A missing/unsupported image retains its label and source.
            model.set_row_data(index, block);
        }
    }
}

fn poll(rx: mpsc::Receiver<Vec<(String, Option<Pixels>)>>) {
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
        assert_eq!((image.width, image.height), (640, 280));
        assert_eq!(image.bytes.len(), 640 * 280 * 4);
        assert!(decode(b"not an image").is_none());
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
        let pixel = || {
            slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(
                &[255, 0, 0, 255],
                1,
                1,
            ))
        };
        for index in 0..CACHE_CAPACITY + 4 {
            loader.remember(format!("workspaces/pic-{index}.png"), pixel());
        }
        assert_eq!(loader.cache.len(), CACHE_CAPACITY);
        assert!(loader.cached("workspaces/pic-0.png").is_none());
        assert!(loader
            .cached(&format!("workspaces/pic-{}.png", CACHE_CAPACITY + 3))
            .is_some());
        loader.remember("data:image/png;base64,AAAA".into(), pixel());
        assert!(loader.cached("data:image/png;base64,AAAA").is_none());
    }
}
