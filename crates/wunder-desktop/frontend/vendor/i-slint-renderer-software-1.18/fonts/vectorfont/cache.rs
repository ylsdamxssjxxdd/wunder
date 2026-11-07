// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

//! Reuse parsed face metrics for glyph runs. A scrolling label otherwise parses
//! the same font tables and allocates variation coordinates on every frame.
use super::*;

i_slint_core::thread_local! {
    static FONTS: core::cell::RefCell<std::collections::VecDeque<Rc<VectorFont>>> = Default::default();
}

pub(crate) fn for_run(
    blob: &fontique::Blob<u8>,
    index: u32,
    size: PhysicalLength,
    coords: &[i16],
) -> Rc<VectorFont> {
    FONTS.with(|fonts| {
        let mut fonts = fonts.borrow_mut();
        if let Some(index) = fonts.iter().position(|font| {
            font.font_blob.id() == blob.id() && font.font_index == index
                && font.pixel_size == size && font.normalized_coords == coords
        }) {
            let font = fonts.remove(index).unwrap();
            fonts.push_front(Rc::clone(&font));
            return font;
        }
        let (key, offset) = super::super::systemfonts::get_swash_font_info(blob, index);
        let font = Rc::new(VectorFont::new_from_blob_and_index_with_coords(
            blob.clone(), index, key, offset, size, coords,
        ));
        if fonts.len() >= 32 { fonts.pop_back(); }
        fonts.push_front(Rc::clone(&font));
        font
    })
}
