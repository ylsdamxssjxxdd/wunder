//! Companion spritesheet decoding and caching, mirroring the web
//! `CompanionSprite.vue` layout: 192x208 frames, one animation state per row,
//! frames laid out horizontally. The desktop keeps the decoded sheet as one
//! RGBA buffer and reads frames through offsets, so a companion costs a single
//! decode and a single allocation instead of nine cropped copies.

use slint::{Image, SharedPixelBuffer};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub const FRAME_WIDTH: u32 = 192;
pub const FRAME_HEIGHT: u32 = 208;

/// One animation state: sheet row, frame count and total cycle duration, taken
/// verbatim from the web sprite table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anim {
    pub row: usize,
    pub frames: usize,
    pub duration_ms: u64,
}

impl Anim {
    /// The web derives per-frame timing as duration / frames.
    pub fn frame_millis(&self) -> u64 {
        (self.duration_ms / self.frames as u64).max(1)
    }
}

pub const IDLE: Anim = Anim { row: 0, frames: 6, duration_ms: 1100 };
pub const RUNNING_RIGHT: Anim = Anim { row: 1, frames: 8, duration_ms: 1060 };
pub const RUNNING_LEFT: Anim = Anim { row: 2, frames: 8, duration_ms: 1060 };
pub const WAVING: Anim = Anim { row: 3, frames: 4, duration_ms: 700 };
pub const JUMPING: Anim = Anim { row: 4, frames: 5, duration_ms: 840 };
pub const FAILED: Anim = Anim { row: 5, frames: 8, duration_ms: 1220 };
pub const WAITING: Anim = Anim { row: 6, frames: 6, duration_ms: 1010 };
pub const RUNNING: Anim = Anim { row: 7, frames: 6, duration_ms: 820 };
pub const REVIEW: Anim = Anim { row: 8, frames: 6, duration_ms: 1030 };

/// A decoded sheet: `columns` frames wide and `rows` animation rows tall.
/// Pixels stay in one RGBA buffer; `frame` locates a frame inside it.
pub struct Sheet {
    pub columns: usize,
    pub rows: usize,
    pub data: Vec<u8>,
}

impl Sheet {
    /// Bytes per pixel line of the whole sheet.
    pub fn stride(&self) -> usize {
        self.columns * FRAME_WIDTH as usize * 4
    }

    /// Byte offset and stride of one frame, or `None` when the sheet is shorter
    /// than the animation asks for. Real packages ship fewer columns than a
    /// state declares, so both dimensions are bounds-checked here.
    pub fn frame(&self, row: usize, index: usize) -> Option<(usize, usize)> {
        if row >= self.rows || index >= self.columns {
            return None;
        }
        Some((
            row * FRAME_HEIGHT as usize * self.stride() + index * FRAME_WIDTH as usize * 4,
            self.stride(),
        ))
    }

    /// Frames this animation can actually show from this sheet.
    pub fn frames_for(&self, anim: Anim) -> usize {
        if anim.row >= self.rows {
            return 0;
        }
        anim.frames.min(self.columns)
    }
}

/// Decode a companion sheet into one RGBA buffer. Runs off the UI thread: the
/// pixel vector is plain data, only `slint::Image` is thread-hostile.
pub fn decode_sheet(bytes: &[u8]) -> Option<Sheet> {
    let decoded = image::load_from_memory(bytes).ok()?;
    let pixels = decoded.into_rgba8();
    let (width, height) = (pixels.width(), pixels.height());
    let columns = (width / FRAME_WIDTH) as usize;
    let rows = (height / FRAME_HEIGHT) as usize;
    if columns == 0 || rows == 0 {
        return None;
    }
    Some(Sheet {
        columns,
        rows,
        data: pixels.into_raw(),
    })
}

/// Decode the idle row into per-frame pixel buffers for `slint::Image` upload.
pub fn decode_idle_frames(bytes: &[u8]) -> Option<Vec<SharedPixelBuffer<slint::Rgba8Pixel>>> {
    let sheet = decode_sheet(bytes)?;
    let columns = sheet.frames_for(IDLE);
    let pitch = FRAME_WIDTH as usize * 4;
    let height = FRAME_HEIGHT as usize;
    let mut frames = Vec::with_capacity(columns);
    for index in 0..columns {
        let (offset, stride) = sheet.frame(IDLE.row, index)?;
        let mut buffer = SharedPixelBuffer::<slint::Rgba8Pixel>::new(FRAME_WIDTH, FRAME_HEIGHT);
        let target = buffer.make_mut_bytes();
        for line in 0..height {
            let source = line * stride + offset;
            target[line * pitch..(line + 1) * pitch].copy_from_slice(&sheet.data[source..source + pitch]);
        }
        frames.push(buffer);
    }
    (!frames.is_empty()).then_some(frames)
}

thread_local! {
    // Idle frames as textures, bounded to the companions actually on screen
    // (dialog preview + selected agent avatars).
    static FRAME_CACHE: RefCell<HashMap<String, Vec<Image>>> = RefCell::new(HashMap::new());
    // Decoded sheets for the floating pets, with their insertion sequence so an
    // over-budget cache can drop the oldest idle one. One sheet is the whole
    // animation set (about 11 MB RGBA for an 8x9 sheet).
    static SHEET_CACHE: RefCell<HashMap<String, (u64, Arc<Sheet>)>> = RefCell::new(HashMap::new());
    static SHEET_SEQ: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static LOAD_MARKS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static SHEET_LOAD_MARKS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

const FRAME_CACHE_CAP: usize = 4;
const SHEET_CACHE_CAP: usize = 4;

pub fn has_idle_frames(id: &str) -> bool {
    FRAME_CACHE.with(|cache| cache.borrow().contains_key(id))
}

/// One frame by index; the cache holds `Vec<Image>` so a single clone is the
/// only copy a projection or an animation tick has to pay for.
pub fn idle_frame(id: &str, frame: usize) -> Image {
    FRAME_CACHE.with(|cache| {
        let cache = cache.borrow();
        cache
            .get(id)
            .and_then(|frames| frames.get(frame))
            .cloned()
            .unwrap_or_default()
    })
}

pub fn idle_frame_count(id: &str) -> usize {
    FRAME_CACHE.with(|slot| {
        slot.borrow()
            .get(id)
            .map(|frames| frames.len())
            .unwrap_or_default()
    })
}

/// Store converted frames on the UI thread. Returns true when the cache
/// changed (so callers can re-project avatar visuals).
pub fn store_idle_frames(id: &str, frames: Vec<SharedPixelBuffer<slint::Rgba8Pixel>>) -> bool {
    if frames.is_empty() {
        return false;
    }
    FRAME_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= FRAME_CACHE_CAP && !cache.contains_key(id) {
            // Simple bound: drop the whole cache rather than tracking LRU for
            // at most four companions.
            cache.clear();
        }
        let images: Vec<Image> = frames.into_iter().map(Image::from_rgba8).collect();
        cache.insert(id.to_string(), images);
        true
    })
}

/// Cached decoded sheet, if a background decode already finished.
pub fn sheet(id: &str) -> Option<Arc<Sheet>> {
    SHEET_CACHE.with(|cache| cache.borrow().get(id).map(|(_, sheet)| Arc::clone(sheet)))
}

/// Publish a decoded sheet on the UI thread. Returns true when the cache
/// changed, so a sync pass can tell "new pixels" from "nothing to do".
pub fn store_sheet(id: &str, sheet: Sheet) -> bool {
    let sheet = Arc::new(sheet);
    SHEET_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let sequence = SHEET_SEQ.with(|sequence| {
            let value = sequence.get();
            sequence.set(value + 1);
            value
        });
        cache.insert(id.to_string(), (sequence, Arc::clone(&sheet)));
        if cache.len() > SHEET_CACHE_CAP {
            // Drop the oldest sheet no window is holding: evicting a live one
            // frees nothing and only forces a re-decode on the next state flip.
            let oldest = cache
                .iter()
                .filter(|(_, (_, cached))| Arc::strong_count(cached) == 1)
                .min_by_key(|(_, (sequence, _))| *sequence)
                .map(|(id, _)| id.clone());
            if let Some(oldest) = oldest {
                cache.remove(&oldest);
            }
        }
        true
    })
}

/// Whether a background decode for this companion is already in flight, so a
/// state tick never re-reads the same package. `true` means "you own it".
pub fn mark_sheet_load_scheduled(id: &str) -> bool {
    SHEET_LOAD_MARKS.with(|marks| marks.borrow_mut().insert(id.to_string()))
}

pub fn clear_sheet_load_mark(id: &str) {
    SHEET_LOAD_MARKS.with(|marks| marks.borrow_mut().remove(id));
}

/// Whether a background idle-frame load for this companion was already
/// scheduled, so a list re-projection does not re-read the package per row.
pub fn mark_load_scheduled(id: &str) -> bool {
    LOAD_MARKS.with(|marks| marks.borrow_mut().insert(id.to_string()))
}

/// Drop everything cached for a companion, used when its package is deleted.
pub fn forget(id: &str) {
    FRAME_CACHE.with(|cache| cache.borrow_mut().remove(id));
    SHEET_CACHE.with(|cache| cache.borrow_mut().remove(id));
    LOAD_MARKS.with(|marks| marks.borrow_mut().remove(id));
    SHEET_LOAD_MARKS.with(|marks| marks.borrow_mut().remove(id));
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder, RgbaImage};

    fn sheet_bytes(columns: u32, rows: u32) -> Vec<u8> {
        let image = RgbaImage::from_fn(columns * FRAME_WIDTH, rows * FRAME_HEIGHT, |x, y| {
            image::Rgba([x as u8, y as u8, (x + y) as u8, 255])
        });
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(image.as_raw(), image.width(), image.height(), ExtendedColorType::Rgba8)
            .expect("encode test sheet");
        bytes
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode_idle_frames(b"not an image").is_none());
        assert!(decode_sheet(b"not an image").is_none());
    }

    /// The full web animation table, in row order.
    const STATES: [Anim; 9] = [
        IDLE, RUNNING_RIGHT, RUNNING_LEFT, WAVING, JUMPING, FAILED, WAITING, RUNNING, REVIEW,
    ];

    #[test]
    fn web_state_table_rows_and_frame_timing() {
        for (index, anim) in STATES.iter().enumerate() {
            assert_eq!(anim.row, index, "row order must match the web sprite");
        }
        assert_eq!(IDLE.frame_millis(), 183);
        assert_eq!(RUNNING.frame_millis(), 136);
        assert_eq!(REVIEW.row, 8);
    }

    #[test]
    fn short_sheets_clamp_the_frames_they_have() {
        // 2 columns x 9 rows: running asks for 8 frames but must clamp to 2.
        let sheet = decode_sheet(&sheet_bytes(2, 9)).expect("sheet decodes");
        assert_eq!(sheet.columns, 2);
        assert_eq!(sheet.rows, 9);
        assert_eq!(sheet.frames_for(RUNNING), 2);
        assert_eq!(
            sheet.data.len(),
            sheet.stride() * FRAME_HEIGHT as usize * sheet.rows
        );
        assert!(sheet.frame(RUNNING.row, 2).is_none());
    }

    #[test]
    fn absent_rows_are_rejected() {
        let sheet = decode_sheet(&sheet_bytes(6, 1)).expect("sheet decodes");
        assert_eq!(sheet.rows, 1);
        assert!(sheet.frame(IDLE.row, 0).is_some());
        assert_eq!(sheet.frames_for(REVIEW), 0);
        assert!(sheet.frame(REVIEW.row, 0).is_none());
    }

    #[test]
    fn frames_address_their_own_row_and_column() {
        let sheet = decode_sheet(&sheet_bytes(6, 9)).expect("sheet decodes");
        let (offset, stride) = sheet.frame(WAVING.row, 3).expect("frame present");
        assert_eq!(stride, sheet.stride());
        let first = (WAVING.row as usize * FRAME_HEIGHT as usize * stride) + 3 * 192 * 4;
        assert_eq!(offset, first);
        // The synthetic sheet encodes its coordinates in the pixels, so the
        // top-left of this frame must be that column.
        assert_eq!(sheet.data[offset], (3 * FRAME_WIDTH) as u8);
        assert_eq!(sheet.data[offset + 1], (WAVING.row * FRAME_HEIGHT as usize) as u8);
    }

    #[test]
    fn idle_frames_split_out_of_the_sheet() {
        let frames = decode_idle_frames(&sheet_bytes(6, 9)).expect("idle row decodes");
        assert_eq!(frames.len(), 6);
        assert_eq!(frames[0].width(), FRAME_WIDTH);
        assert_eq!(frames[3].as_slice()[0].r, (3 * FRAME_WIDTH) as u8);
    }

    #[test]
    fn the_sheet_cache_drops_idle_entries_first() {
        assert!(store_sheet(
            "held-pet",
            decode_sheet(&sheet_bytes(1, 1)).expect("sheet decodes")
        ));
        let held = sheet("held-pet").expect("cached sheet");
        for index in 0..SHEET_CACHE_CAP + 2 {
            assert!(store_sheet(
                &format!("idle-pet-{index}"),
                decode_sheet(&sheet_bytes(1, 1)).expect("sheet decodes")
            ));
        }
        // A sheet a pet window still references costs nothing to keep, so the
        // cache only ever gives up the ones nobody is using.
        assert!(sheet("held-pet").is_some(), "a referenced sheet survives");
        assert!(sheet("idle-pet-0").is_none(), "the oldest idle sheet goes");
        drop(held);
    }
}
