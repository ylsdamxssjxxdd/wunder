// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

//! Uses Windows' installed fonts; no proprietary font data is copied into the repo.
use super::*;
use swash::scale::{ScaleContext, StrikeWith};

struct TestPlatform;
impl i_slint_core::platform::Platform for TestPlatform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn i_slint_core::window::WindowAdapter>, i_slint_core::api::PlatformError> {
        Ok(crate::MinimalSoftwareWindow::new(
            crate::RepaintBufferType::NewBuffer,
        ))
    }
}

fn installed_font(file: &str, pixel_size: i16) -> VectorFont {
    let directory = std::env::var_os("WINDIR").expect("Windows font tests require WINDIR");
    let bytes = std::fs::read(std::path::PathBuf::from(directory).join("Fonts").join(file))
        .expect("Windows font fixture must be installed");
    let blob: fontique::Blob<u8> = bytes.into();
    let font_ref = swash::FontRef::from_index(blob.data(), 0).unwrap();
    let (key, offset) = (font_ref.key, font_ref.offset);
    VectorFont::new_from_blob_and_index(blob, 0, key, offset, PhysicalLength::new(pixel_size))
}

#[test]
fn system_simsun_keeps_exact_bitmap_pixels_and_baseline() {
    let context = i_slint_core::SlintContext::new(alloc::boxed::Box::new(TestPlatform));
    let mut bitmap_context = ScaleContext::new();
    for size in [12, 13, 14, 15, 16] {
        let font = installed_font("simsun.ttc", size);
        let font_ref = font.swash_font_ref();
        let mut scaler = bitmap_context.builder(font_ref).size(size as f32).build();
        for ch in "雷达回波强度速度谱宽".chars() {
            let id = NonZeroU16::new(font_ref.charmap().map(ch)).unwrap();
            let bitmap = scaler
                .scale_bitmap(id.get(), StrikeWith::ExactSize)
                .expect("SimSun CJK must have the matching small-size strike");
            let actual = font.render_vector_glyph(id, 0, &context).unwrap();
            assert_eq!(
                &*actual.alpha_map,
                bitmap.data.as_slice(),
                "{ch} at {size}px"
            );
            assert!(
                actual
                    .alpha_map
                    .iter()
                    .all(|&alpha| alpha == 0 || alpha == 255)
            );
            assert_eq!(actual.width.get() as u32, bitmap.placement.width);
            assert_eq!(actual.height.get() as u32, bitmap.placement.height);
            assert_eq!(actual.glyph_origin_x, bitmap.placement.left as f32);
            assert_eq!(
                actual.y.truncate(),
                bitmap.placement.top - bitmap.placement.height as i32
            );
            let cached = font.render_vector_glyph(id, 0, &context).unwrap();
            assert!(Rc::ptr_eq(&actual.alpha_map, &cached.alpha_map));
        }
    }
}

#[test]
fn missing_bitmap_uses_hinted_outline_at_requested_size() {
    let context = i_slint_core::SlintContext::new(alloc::boxed::Box::new(TestPlatform));
    let mut reference_context = ScaleContext::new();
    let mut hinting_changed_pixels = false;
    for (file, text, sizes) in [
        ("simsun.ttc", "雷达回波", &[19, 24, 32][..]),
        ("segoeui.ttf", "Abg0129", &[12, 13, 16][..]),
    ] {
        for &size in sizes {
            let font = installed_font(file, size);
            let font_ref = font.swash_font_ref();
            for ch in text.chars() {
                let id = NonZeroU16::new(font_ref.charmap().map(ch)).unwrap();
                let mut scaler = reference_context
                    .builder(font_ref)
                    .size(size as f32)
                    .hint(true)
                    .build();
                assert!(
                    scaler
                        .scale_bitmap(id.get(), StrikeWith::ExactSize)
                        .is_none()
                );
                let expected = swash::scale::Render::new(&[swash::scale::Source::Outline])
                    .render(&mut scaler, id.get())
                    .unwrap();
                let actual = font.render_vector_glyph(id, 0, &context).unwrap();
                assert_eq!(
                    &*actual.alpha_map,
                    expected.data.as_slice(),
                    "{file} {ch} at {size}px"
                );
                assert_eq!(actual.width.get() as u32, expected.placement.width);
                assert_eq!(actual.height.get() as u32, expected.placement.height);
                let mut unhinted = reference_context
                    .builder(font_ref)
                    .size(size as f32)
                    .build();
                let old = swash::scale::Render::new(&[swash::scale::Source::Outline])
                    .render(&mut unhinted, id.get())
                    .unwrap();
                hinting_changed_pixels |= expected.data != old.data;
            }
        }
    }
    assert!(
        hinting_changed_pixels,
        "fixtures must distinguish hinted from unhinted rendering"
    );
}

#[test]
fn empty_space_bitmaps_do_not_panic_or_lose_layout_advance() {
    let context = i_slint_core::SlintContext::new(alloc::boxed::Box::new(TestPlatform));
    let mut empty_bitmap_seen = false;
    for size in [12, 13, 14, 15, 16, 17] {
        let font = installed_font("simsun.ttc", size);
        let font_ref = font.swash_font_ref();
        for ch in [' ', '\u{a0}', '\u{3000}'] {
            let id = NonZeroU16::new(font_ref.charmap().map(ch)).unwrap();
            if let Some(bitmap) = font.exact_bitmap(id) {
                empty_bitmap_seen |= bitmap.width == 0 || bitmap.height == 0;
            }
            let image = font.render_vector_glyph(id, 0, &context);
            assert!(image.is_none_or(|glyph| glyph.alpha_map.iter().all(|&v| v == 0)));
            assert!(font.glyph_for_char(ch).unwrap().advance.get() > 0);
        }
    }
    assert!(
        empty_bitmap_seen,
        "fixture must exercise Swash's zero-size bitmap edge case"
    );
}

/// The bundled subset face (`res/fonts/simsun-subset.ttf`, produced by
/// `scripts/subset_simsun.py`) must render kept glyphs exactly like the full
/// master: identical alpha maps, placement and origin at both bitmap-strike
/// and outline sizes. This guards the custom EBDT/EBLC rebuild against
/// silent corruption.
#[test]
fn bundled_subset_matches_master_rendering() {
    let context = i_slint_core::SlintContext::new(alloc::boxed::Box::new(TestPlatform));
    let load = |file: &str, pixel_size: i16| {
        let bytes = std::fs::read(std::path::PathBuf::from("../../../res/fonts").join(file))
            .expect("repo font fixture must exist");
        let blob: fontique::Blob<u8> = bytes.into();
        let font_ref = swash::FontRef::from_index(blob.data(), 0).unwrap();
        let (key, offset) = (font_ref.key, font_ref.offset);
        VectorFont::new_from_blob_and_index(blob, 0, key, offset, PhysicalLength::new(pixel_size))
    };
    // NB: the master SimSun cmap itself lacks U+2212/U+2194/U+25BE (they fall
    // back to .notdef both before and after subsetting), so they must not be
    // sampled here — only characters the master actually covers.
    let sample = "记录登记帮助雷达回波强度速度谱宽编报整编监视区外推跟踪识别\
                  产品转换模拟训练遮蔽角填表报文警报0123456789N/E/S/W°℃埗";
    for size in [12, 13, 14, 15, 16, 17, 19, 24] {
        let master = load("simsun.ttf", size);
        let subset = load("simsun-subset.ttf", size);
        for ch in sample.chars() {
            let master_ref = master.swash_font_ref();
            let subset_ref = subset.swash_font_ref();
            let master_id = NonZeroU16::new(master_ref.charmap().map(ch)).unwrap();
            let subset_id = NonZeroU16::new(subset_ref.charmap().map(ch)).unwrap();
            assert_eq!(master_id, subset_id, "{ch} glyph id changed at {size}px");
            let expected = master.render_vector_glyph(master_id, 0, &context).unwrap();
            let actual = subset.render_vector_glyph(subset_id, 0, &context).unwrap();
            assert_eq!(&*actual.alpha_map, &*expected.alpha_map, "{ch} at {size}px");
            assert_eq!(actual.width, expected.width, "{ch} width at {size}px");
            assert_eq!(actual.height, expected.height, "{ch} height at {size}px");
            assert_eq!(actual.glyph_origin_x, expected.glyph_origin_x, "{ch} at {size}px");
        }
    }
}
