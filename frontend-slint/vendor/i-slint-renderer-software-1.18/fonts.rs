// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

// cSpell: ignore pixelfont vectorfont
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;

use super::{Fixed, PhysicalLength, PhysicalSize};
use i_slint_core::graphics::{BitmapFont, BitmapGlyphs, FontRequest};
use i_slint_core::lengths::ScaleFactor;
use i_slint_core::textlayout::TextLayout;

i_slint_core::thread_local! {
    static BITMAP_FONTS: RefCell<Vec<&'static BitmapFont>> = RefCell::default()
}

i_slint_core::thread_local! {
    // Only glyphs that were actually painted are expanded. The cache belongs
    // to Slint's UI thread, matching bitmap-font rendering and avoiding locks
    // on the Win7 UI path.
    static PACKED_GLYPH_CACHE: RefCell<PackedGlyphCache> = RefCell::new(PackedGlyphCache::default())
}

const MAX_CACHED_PACKED_GLYPHS: usize = 1_024;
const MAX_CACHED_PACKED_GLYPH_BYTES: usize = 1_024 * 1_024;

struct CachedPackedGlyph {
    glyph_set: *const BitmapGlyphs,
    glyph_index: usize,
    alpha_map: Rc<[u8]>,
    last_used: u64,
}

#[derive(Default)]
struct PackedGlyphCache {
    entries: Vec<CachedPackedGlyph>,
    cached_bytes: usize,
    clock: u64,
}

impl PackedGlyphCache {
    fn next_clock(&mut self) -> u64 {
        self.clock = self.clock.wrapping_add(1);
        self.clock
    }

    fn get(&mut self, glyph_set: *const BitmapGlyphs, glyph_index: usize) -> Option<Rc<[u8]>> {
        let last_used = self.next_clock();
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.glyph_set == glyph_set && entry.glyph_index == glyph_index)?;
        entry.last_used = last_used;
        Some(entry.alpha_map.clone())
    }

    fn insert(&mut self, glyph_set: *const BitmapGlyphs, glyph_index: usize, alpha_map: Rc<[u8]>) {
        let bytes = alpha_map.len();
        if bytes > MAX_CACHED_PACKED_GLYPH_BYTES {
            return;
        }
        while !self.entries.is_empty()
            && (self.entries.len() >= MAX_CACHED_PACKED_GLYPHS
                || self.cached_bytes.saturating_add(bytes) > MAX_CACHED_PACKED_GLYPH_BYTES)
        {
            let oldest = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(index, _)| index)
                .expect("non-empty packed glyph cache");
            self.cached_bytes -= self.entries.swap_remove(oldest).alpha_map.len();
        }
        self.cached_bytes += bytes;
        let last_used = self.next_clock();
        self.entries.push(CachedPackedGlyph {
            glyph_set,
            glyph_index,
            alpha_map,
            last_used,
        });
    }
}

#[derive(Clone)]
pub enum GlyphAlphaMap {
    Static(&'static [u8]),
    Shared(Rc<[u8]>),
    SharedSubpixel(Rc<[u8]>),
}

#[derive(Clone)]
pub struct RenderableGlyph {
    pub x: Fixed<i32, 8>,
    pub y: Fixed<i32, 8>,
    pub width: PhysicalLength,
    pub height: PhysicalLength,
    pub alpha_map: GlyphAlphaMap,
    pub pixel_stride: u16,
    /// Whether the glyph data contains one coverage byte for each RGB subpixel.
    pub subpixel: bool,
    /// Native GDI coverages are already in the panel's final channel order.
    pub native_subpixel: bool,
    pub sdf: bool,
}

impl RenderableGlyph {
    pub fn size(&self) -> PhysicalSize {
        PhysicalSize::from_lengths(self.width, self.height)
    }
}

// Subset of `RenderableGlyph`, specifically for VectorFonts.
#[cfg(feature = "systemfonts")]
#[derive(Clone)]
pub struct RenderableVectorGlyph {
    pub x: Fixed<i32, 8>,
    pub y: Fixed<i32, 8>,
    pub width: PhysicalLength,
    pub height: PhysicalLength,
    pub alpha_map: Rc<[u8]>,
    pub pixel_stride: u16,
    pub glyph_origin_x: f32,
}

#[cfg(feature = "systemfonts")]
impl RenderableVectorGlyph {
    pub fn size(&self) -> PhysicalSize {
        PhysicalSize::from_lengths(self.width, self.height)
    }
}

pub trait GlyphRenderer {
    fn render_glyph(
        &self,
        glyph_id: core::num::NonZeroU16,
        slint_context: &i_slint_core::SlintContext,
    ) -> Option<RenderableGlyph>;
    /// The amount of pixel in the original image that correspond to one pixel in the rendered image
    fn scale_delta(&self) -> Fixed<u16, 8>;
}

pub(super) use i_slint_core::textlayout::DEFAULT_FONT_SIZE;

mod pixelfont;
#[cfg(feature = "systemfonts")]
pub mod vectorfont;
#[allow(unsafe_code)]
mod win32_gdi;

#[cfg(feature = "systemfonts")]
pub mod systemfonts;

#[derive(derive_more::From)]
pub enum Font {
    PixelFont(pixelfont::PixelFont),
    #[cfg(feature = "systemfonts")]
    VectorFont(vectorfont::VectorFont),
}

/// Runs `$body` with `$bound` bound to the concrete font held by `$font`.
///
/// The bitmap and vector paths through the text code are the same code; they need two
/// match arms only because `PixelFont` and `VectorFont` are distinct types. The body is
/// monomorphized per variant, the way a generic function would be, so that it can be
/// written once.
///
/// Keep only font-dependent work in the body: it is emitted once per variant.
///
/// Callers that hand off to parley for vector fonts must do so before calling this, as
/// `sharedparley::` needs the font context rather than a laid-out font.
macro_rules! with_font {
    ($font:expr, |$bound:ident| $body:block) => {
        match $font {
            $crate::fonts::Font::PixelFont($bound) => $body,
            #[cfg(feature = "systemfonts")]
            $crate::fonts::Font::VectorFont($bound) => $body,
        }
    };
}
pub(crate) use with_font;

/// Returns the size of the pre-rendered font in pixels.
pub fn pixel_size(glyphs: &i_slint_core::graphics::BitmapGlyphs) -> PhysicalLength {
    PhysicalLength::new(glyphs.pixel_size)
}

impl i_slint_core::textlayout::FontMetrics<PhysicalLength> for Font {
    fn ascent(&self) -> PhysicalLength {
        with_font!(self, |font| { font.ascent() })
    }

    fn height(&self) -> PhysicalLength {
        with_font!(self, |font| { font.height() })
    }

    fn descent(&self) -> PhysicalLength {
        with_font!(self, |font| { font.descent() })
    }

    fn x_height(&self) -> PhysicalLength {
        with_font!(self, |font| { font.x_height() })
    }

    fn cap_height(&self) -> PhysicalLength {
        with_font!(self, |font| { font.cap_height() })
    }
}

pub fn match_font(
    request: &FontRequest,
    scale_factor: ScaleFactor,
    #[cfg(feature = "systemfonts")]
    font_context: &mut i_slint_core::textlayout::sharedparley::parley::FontContext,
) -> Font {
    let requested_weight = request
        .weight
        .and_then(|weight| weight.try_into().ok())
        .unwrap_or(/* CSS normal */ 400);

    let bitmap_font = BITMAP_FONTS.with(|fonts| {
        let fonts = fonts.borrow();

        request.family.as_ref().and_then(|requested_family| {
            fonts
                .iter()
                .filter(|bitmap_font| {
                    core::str::from_utf8(bitmap_font.family_name.as_slice()).unwrap()
                        == requested_family.as_str()
                        && bitmap_font.italic == request.italic
                })
                .min_by_key(|bitmap_font| bitmap_font.weight.abs_diff(requested_weight))
                .copied()
        })
    });

    let font = match bitmap_font {
        Some(bitmap_font) => bitmap_font,
        None => {
            #[cfg(feature = "systemfonts")]
            if let Some(vectorfont) = systemfonts::match_font(
                request,
                scale_factor,
                &mut font_context.collection,
                &mut font_context.source_cache,
            ) {
                return vectorfont.into();
            }
            if let Some(fallback_bitmap_font) = BITMAP_FONTS.with(|fonts| {
                let fonts = fonts.borrow();
                fonts
                    .iter()
                    .cloned()
                    .filter(|bitmap_font| bitmap_font.italic == request.italic)
                    .min_by_key(|bitmap_font| bitmap_font.weight.abs_diff(requested_weight))
                    .or_else(|| fonts.first().cloned())
            }) {
                fallback_bitmap_font
            } else {
                #[cfg(feature = "systemfonts")]
                return systemfonts::fallbackfont(
                    request,
                    scale_factor,
                    &mut font_context.collection,
                    &mut font_context.source_cache,
                )
                .into();
                #[cfg(not(feature = "systemfonts"))]
                panic!(
                    "No font fallback found. The software renderer requires enabling the `EmbedForSoftwareRenderer` option when compiling slint files."
                )
            }
        }
    };

    let requested_pixel_size: PhysicalLength =
        (request.pixel_size.unwrap_or(DEFAULT_FONT_SIZE).cast() * scale_factor).cast();

    // Pick the closest embedded size, preferring the larger one (downscaling
    // keeps strokes much crisper than upscaling a smaller bitmap). Upstream
    // always snaps down, which visibly softens text at 150-200% display
    // scaling where logical sizes rarely match an embedded size exactly.
    let upper = font
        .glyphs
        .partition_point(|glyphs| pixel_size(glyphs) < requested_pixel_size);
    let nearest_pixel_size = match (upper.checked_sub(1), font.glyphs.get(upper)) {
        (Some(lower), Some(upper_glyphs)) => {
            let lower_delta = requested_pixel_size - pixel_size(&font.glyphs[lower]);
            let upper_delta = pixel_size(upper_glyphs) - requested_pixel_size;
            if upper_delta <= lower_delta {
                upper
            } else {
                lower
            }
        }
        (Some(lower), None) => lower,
        (None, _) => 0,
    };
    let matching_glyphs = &font.glyphs[nearest_pixel_size];

    let pixel_size = if font.sdf {
        requested_pixel_size
    } else {
        pixel_size(matching_glyphs)
    };

    pixelfont::PixelFont {
        bitmap_font: font,
        glyphs: matching_glyphs,
        pixel_size,
    }
    .into()
}

#[cfg(feature = "systemfonts")]
pub(crate) fn has_bitmap_fonts() -> bool {
    BITMAP_FONTS.with(|fonts| !fonts.borrow().is_empty())
}

pub fn text_layout_for_font<'a, Font>(
    font: &'a Font,
    font_request: &FontRequest,
    scale_factor: ScaleFactor,
) -> TextLayout<'a, Font>
where
    Font: i_slint_core::textlayout::AbstractFont
        + i_slint_core::textlayout::TextShaper<Length = PhysicalLength>,
{
    let letter_spacing =
        font_request.letter_spacing.map(|spacing| (spacing.cast() * scale_factor).cast());
    let line_height = font_request.line_height_for_natural_height(font.height().get() as f32).map(
        |line_height| PhysicalLength::new(num_traits::Float::round(line_height).max(0.) as i16),
    );

    TextLayout { font, letter_spacing, line_height }
}

pub fn register_bitmap_font(font_data: &'static BitmapFont) {
    BITMAP_FONTS.with(|fonts| fonts.borrow_mut().push(font_data))
}

/// Register the project-owned face used by the Win7 native ASCII path.
pub fn register_win32_gdi_font(family: &'static str, data: &'static [u8]) {
    win32_gdi::register_font(family, data);
}

struct CachedNativeGlyph {
    glyph_set: *const BitmapGlyphs,
    glyph_index: usize,
    pixel_size: i16,
    glyph: Option<win32_gdi::NativeGlyph>,
}

i_slint_core::thread_local! {
    static NATIVE_GLYPH_CACHE: RefCell<Vec<CachedNativeGlyph>> = RefCell::default()
}

pub(super) fn native_gdi_glyph(
    glyphs: &'static BitmapGlyphs,
    glyph_index: usize,
    pixel_size: PhysicalLength,
    code_point: impl FnOnce() -> Option<char>,
) -> Option<win32_gdi::NativeGlyph> {
    let glyph_set = core::ptr::from_ref(glyphs);
    let pixel_size = pixel_size.get();
    NATIVE_GLYPH_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(entry) = cache.iter().find(|entry| {
            entry.glyph_set == glyph_set
                && entry.glyph_index == glyph_index
                && entry.pixel_size == pixel_size
        }) {
            return entry.glyph.clone();
        }
        let glyph = if win32_gdi::enabled() {
            code_point().and_then(|ch| win32_gdi::rasterize(ch, pixel_size))
        } else {
            None
        };
        if cache.len() >= 512 {
            cache.remove(0);
        }
        cache.push(CachedNativeGlyph {
            glyph_set,
            glyph_index,
            pixel_size,
            glyph: glyph.clone(),
        });
        glyph
    })
}

/// Expand only one 1-bit glyph at the point where the scene needs it. A full
/// SimSun strike contains over 22k glyphs, whereas the first workbench frame
/// paints only a small subset of them. Keeping the cache bounded is important
/// for 32-bit Win7 processes that open data with many distinct Chinese names.
pub(super) fn glyph_alpha_map(glyphs: &'static BitmapGlyphs, glyph_index: usize) -> GlyphAlphaMap {
    let glyph = &glyphs.glyph_data[glyph_index];
    if glyph.data_packing != 1 {
        return GlyphAlphaMap::Static(glyph.data.as_slice());
    }

    let glyph_set = core::ptr::from_ref(glyphs);
    PACKED_GLYPH_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(alpha_map) = cache.get(glyph_set, glyph_index) {
            return GlyphAlphaMap::Shared(alpha_map);
        }

        let alpha_map = Rc::<[u8]>::from(expand_packed_alpha(glyph));
        cache.insert(glyph_set, glyph_index, alpha_map.clone());
        GlyphAlphaMap::Shared(alpha_map)
    })
}

fn expand_packed_alpha(glyph: &i_slint_core::graphics::BitmapGlyph) -> Vec<u8> {
    let width = glyph.width.max(0) as usize;
    let height = glyph.height.max(0) as usize;
    let stride = width.div_ceil(8);
    let packed = glyph.data.as_slice();
    let mut alpha_map = alloc::vec![0; width.saturating_mul(height)];
    for y in 0..height {
        for x in 0..width {
            let packed_index = y.saturating_mul(stride).saturating_add(x / 8);
            if packed.get(packed_index).copied().unwrap_or(0) & (0x80 >> (x % 8)) != 0 {
                alpha_map[y * width + x] = 255;
            }
        }
    }
    alpha_map
}

#[cfg(test)]
mod packed_glyph_tests {
    use super::expand_packed_alpha;
    use i_slint_core::graphics::BitmapGlyph;
    use i_slint_core::slice::Slice;

    #[test]
    fn expands_msb_first_packed_rows_to_alpha() {
        static PACKED: [u8; 2] = [0b1010_0000, 0b0101_0000];
        let glyph = BitmapGlyph {
            x: 0,
            y: 0,
            width: 4,
            height: 2,
            x_advance: 0,
            data_packing: 1,
            data: Slice::from_slice(&PACKED),
        };
        assert_eq!(
            expand_packed_alpha(&glyph),
            [255, 0, 255, 0, 0, 255, 0, 255]
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use i_slint_core::lengths::LogicalLength;
    use i_slint_core::textlayout::{FontMetrics, Glyph, TextShaper};

    struct TestFont;

    impl FontMetrics<PhysicalLength> for TestFont {
        fn ascent(&self) -> PhysicalLength {
            PhysicalLength::new(18)
        }

        fn descent(&self) -> PhysicalLength {
            PhysicalLength::new(-6)
        }

        fn x_height(&self) -> PhysicalLength {
            PhysicalLength::new(10)
        }

        fn cap_height(&self) -> PhysicalLength {
            PhysicalLength::new(14)
        }
    }

    impl TextShaper for TestFont {
        type LengthPrimitive = i16;
        type Length = PhysicalLength;

        fn shape_text<GlyphStorage: core::iter::Extend<Glyph<Self::Length>>>(
            &self,
            _text: &str,
            _glyphs: &mut GlyphStorage,
        ) {
        }

        fn glyph_for_char(&self, _ch: char) -> Option<Glyph<Self::Length>> {
            None
        }
    }

    #[test]
    fn line_height_factor_scales_natural_height() {
        let font_request = FontRequest {
            pixel_size: Some(LogicalLength::new(20.)),
            line_height_factor: Some(1.5),
            ..Default::default()
        };

        let layout = text_layout_for_font(&TestFont, &font_request, ScaleFactor::new(1.));

        assert_eq!(TestFont.height(), PhysicalLength::new(24));
        assert_eq!(layout.line_height, Some(PhysicalLength::new(36)));
    }

    #[test]
    fn line_height_factor_zero_collapses_lines() {
        let font_request = FontRequest {
            pixel_size: Some(LogicalLength::new(20.)),
            line_height_factor: Some(0.),
            ..Default::default()
        };

        let layout = text_layout_for_font(&TestFont, &font_request, ScaleFactor::new(1.));

        assert_eq!(layout.line_height, Some(PhysicalLength::new(0)));
    }
}
