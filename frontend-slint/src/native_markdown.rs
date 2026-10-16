//! Bounded workspace image loading. No disk IO or decoding runs on the UI thread.
use crate::{message_blocks::Blocks, TextBlock};
use slint::Model;
use std::{
    sync::{mpsc, Arc, OnceLock},
    time::Duration,
};
use wunder_desktop::NativeDesktop;

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
                        let pixels = job
                            .desktop
                            .workspace_image_bytes(&job.agent, &source)
                            .ok()
                            .and_then(|bytes| decode(&bytes));
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
}

pub fn hydrate(blocks: &mut Blocks, desktop: Arc<NativeDesktop>, agent: String) {
    hydrate_model(slint::ModelRc::from(blocks.model.clone()), desktop, agent);
}

/// Hydrate an already projected block model. History rows are projected as
/// `TextBlock` models directly, so they cannot retain the temporary `Blocks`
/// builder used by streaming output. Keeping the same bounded worker here
/// makes live and restored messages follow one image loading path.
pub fn hydrate_model(model: slint::ModelRc<TextBlock>, desktop: Arc<NativeDesktop>, agent: String) {
    let sources = model
        .iter()
        .filter_map(|block| {
            (block.kind == crate::message_blocks::KIND_IMAGE && !block.image_source.is_empty())
                .then(|| block.image_source.to_string())
        })
        .collect::<Vec<_>>();
    if sources.is_empty() {
        return;
    }
    let (tx, rx) = mpsc::sync_channel(1);
    if worker()
        .try_send(Job {
            desktop,
            agent,
            sources,
            result: tx,
        })
        .is_ok()
    {
        poll(model, rx);
    }
}

fn poll(target: slint::ModelRc<TextBlock>, rx: mpsc::Receiver<Vec<(String, Option<Pixels>)>>) {
    slint::Timer::single_shot(Duration::from_millis(33), move || {
        let model = target.clone();
        match rx.try_recv() {
            Ok(images) => {
                for (source, pixels) in images {
                    let image = pixels.map(|p| {
                        slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(
                            &p.bytes, p.width, p.height,
                        ))
                    });
                    for index in 0..model.row_count() {
                        let Some(mut block) = model.row_data(index) else {
                            continue;
                        };
                        if block.image_source != source {
                            continue;
                        }
                        if let Some(image) = &image {
                            block.image = image.clone();
                        }
                        // A missing/unsupported image retains its label and source.
                        model.set_row_data(index, block);
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => poll(target, rx),
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    });
}
