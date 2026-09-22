// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

use crate::PhysicalLength;
use crate::fixed::Fixed;
use i_slint_core::graphics::{BitmapFont, BitmapGlyphs};
use i_slint_core::textlayout::{FontMetrics, Glyph, TextShaper};

use super::{GlyphAlphaMap, GlyphRenderer, RenderableGlyph, glyph_alpha_map, native_gdi_glyph};

// A font that is resolved to a specific pixel size.
pub struct PixelFont {
    pub bitmap_font: &'static BitmapFont,
    pub glyphs: &'static BitmapGlyphs,
    pub pixel_size: PhysicalLength,
}

impl PixelFont {
    pub fn glyph_index_to_glyph_id(index: usize) -> core::num::NonZeroU16 {
        core::num::NonZeroU16::new(index as u16 + 1).unwrap()
    }
    pub fn glyph_id_to_glyph_index(id: core::num::NonZeroU16) -> usize {
        id.get() as usize - 1
    }
}

impl GlyphRenderer for PixelFont {
    fn render_glyph(
        &self,
        glyph_id: core::num::NonZeroU16,
        _slint_context: &i_slint_core::SlintContext,
    ) -> Option<RenderableGlyph> {
        let glyph_index = Self::glyph_id_to_glyph_index(glyph_id);
        let bitmap_glyph = &self.glyphs.glyph_data[glyph_index];
        if bitmap_glyph.data.is_empty() {
            // For example, ' ' has no glyph data
            return None;
        }
        if bitmap_glyph.data_packing == 2 {
            if let Some(glyph) = native_gdi_glyph(self.glyphs, glyph_index, self.pixel_size, || {
                self.bitmap_font
                    .character_map
                    .iter()
                    .find(|entry| entry.glyph_index as usize == glyph_index)
                    .map(|entry| entry.code_point)
            }) {
                return Some(RenderableGlyph {
                    x: Fixed::from_integer(glyph.x),
                    y: Fixed::from_integer(glyph.y),
                    width: PhysicalLength::new(glyph.width as i16),
                    height: PhysicalLength::new(glyph.height as i16),
                    alpha_map: GlyphAlphaMap::SharedSubpixel(glyph.data),
                    pixel_stride: glyph.stride,
                    subpixel: true,
                    native_subpixel: true,
                    sdf: false,
                });
            }
        }
        // t represent the target coordinate system, and s the source glyph coordinate system.
        // We want to align the glyph such that Δ(hₜ+yₜ)+offset = hₛ+yₛ
        // where hₜ is the integer height of the glyph in the target coordinate system
        // and offset is smaller than Δ
        // We also want that Δ(hₜ-1)+offset ≤ hₛ-1
        // Similar for x but that's easier since x is not subtracted from the width
        let delta = Fixed::<i32, 8>::from_fixed(self.scale_delta());
        let src_x = Fixed::<i32, 8>::from_fixed(Fixed::<_, 6>(bitmap_glyph.x));
        let src_y = Fixed::<i32, 8>::from_fixed(Fixed::<_, 6>(bitmap_glyph.y));
        let h_plus_y = Fixed::<i32, 8>::from_integer(bitmap_glyph.height as i32) + src_y;
        let h_plus_y = Fixed::<i32, 8>::from_fraction(h_plus_y.0, delta.0);
        let off_y = Fixed::<i32, 8>(h_plus_y.0 & 0xff);
        let height = (Fixed::from_integer(bitmap_glyph.height as i32 - 1) - off_y) / delta + 1;
        let x = Fixed::from_fraction(src_x.0, delta.0);
        let off_x = Fixed::<i32, 8>(-x.0 & 0xff);
        let width = (Fixed::from_integer(bitmap_glyph.width as i32 - 1) - off_x) / delta + 1;
        Some(RenderableGlyph {
            x,
            y: h_plus_y - Fixed::from_integer(height),
            width: PhysicalLength::new(width as i16),
            height: PhysicalLength::new(height as i16),
            alpha_map: glyph_alpha_map(self.glyphs, glyph_index),
            pixel_stride: bitmap_glyph.width as u16,
            subpixel: bitmap_glyph.data_packing == 2,
            native_subpixel: false,
            sdf: self.bitmap_font.sdf,
        })
    }
    fn scale_delta(&self) -> Fixed<u16, 8> {
        Fixed::try_from_fixed(Fixed::<u32, 8>::from_fraction(
            self.glyphs.pixel_size as u32,
            self.pixel_size.get() as u32,
        ))
        .unwrap()
    }
}

impl TextShaper for PixelFont {
    type LengthPrimitive = i16;
    type Length = PhysicalLength;
    fn shape_text<GlyphStorage: core::iter::Extend<Glyph<PhysicalLength>>>(
        &self,
        text: &str,
        glyphs: &mut GlyphStorage,
    ) {
        let glyphs_iter = text.char_indices().map(|(byte_offset, char)| {
            let glyph_index = self
                .bitmap_font
                .character_map
                .binary_search_by_key(&char, |char_map_entry| char_map_entry.code_point)
                .ok()
                .map(|char_map_index| {
                    self.bitmap_font.character_map[char_map_index].glyph_index as usize
                });
            let x_advance = glyph_index.map_or_else(
                || self.pixel_size,
                |glyph_index| {
                    ((self.pixel_size.cast()
                        * self.glyphs.glyph_data[glyph_index].x_advance as i32
                        / self.glyphs.pixel_size as i32
                        + euclid::Length::new(32))
                        / 64)
                        .cast()
                },
            );
            Glyph {
                glyph_id: glyph_index.map(Self::glyph_index_to_glyph_id),
                advance: x_advance,
                text_byte_offset: byte_offset,
                ..Default::default()
            }
        });
        glyphs.extend(glyphs_iter);
    }

    fn glyph_for_char(&self, ch: char) -> Option<Glyph<PhysicalLength>> {
        self.bitmap_font
            .character_map
            .binary_search_by_key(&ch, |char_map_entry| char_map_entry.code_point)
            .ok()
            .map(|char_map_index| {
                let glyph_index =
                    self.bitmap_font.character_map[char_map_index].glyph_index as usize;
                let bitmap_glyph = &self.glyphs.glyph_data[glyph_index];
                let x_advance = ((self.pixel_size.cast() * bitmap_glyph.x_advance as i32
                    / self.glyphs.pixel_size as i32
                    + euclid::Length::new(32))
                    / 64)
                    .cast();
                Glyph {
                    glyph_id: Some(Self::glyph_index_to_glyph_id(glyph_index)),
                    advance: x_advance,
                    text_byte_offset: 0,
                    ..Default::default()
                }
            })
    }
}

impl FontMetrics<PhysicalLength> for PixelFont {
    fn ascent(&self) -> PhysicalLength {
        (self.pixel_size.cast() * self.bitmap_font.ascent / self.bitmap_font.units_per_em).cast()
    }

    fn descent(&self) -> PhysicalLength {
        (self.pixel_size.cast() * self.bitmap_font.descent / self.bitmap_font.units_per_em).cast()
    }

    fn height(&self) -> PhysicalLength {
        // The descent is negative (relative to the baseline)
        (self.pixel_size.cast() * (self.bitmap_font.ascent - self.bitmap_font.descent)
            / self.bitmap_font.units_per_em)
            .cast()
    }

    fn x_height(&self) -> PhysicalLength {
        (self.pixel_size.cast() * self.bitmap_font.x_height / self.bitmap_font.units_per_em).cast()
    }

    fn cap_height(&self) -> PhysicalLength {
        (self.pixel_size.cast() * self.bitmap_font.cap_height / self.bitmap_font.units_per_em)
            .cast()
    }
}
