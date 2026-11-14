//! 工作区文件图标，与桌面版同一张表。
//!
//! The textures and lookup tables are generated at build time by
//! `wunder-icon-builder` from `crates/wunder-desktop/frontend/assets/
//! vscode-icons/file-icon-map.json` — the same assets and the same resolver
//! order the desktop workspace panel and transcript use, so a `.pdf` link in a
//! phone answer carries the profile of the desktop card instead of a blank
//! square.
//!
//! Decoded icons live in thread-local storage: resolution runs on the UI thread
//! and `slint::Image` is not `Send` in the vendored renderer build.

use slint::Image;
use std::cell::RefCell;
use std::collections::HashMap;

include!(concat!(env!("OUT_DIR"), "/file_icons_data.rs"));

/// Resolve the icon for one workspace entry. Mirrors 桌面版: folders use the
/// shared folder icon, drawio files the diagram icon, then the theme extension
/// table, then the file-name table, then the default file icon.
pub fn workspace_file_icon(name: &str, entry_type: &str) -> Image {
    CACHE.with(|cache| cached_image(cache, icon_key(name, entry_type)))
}

fn icon_key(name: &str, entry_type: &str) -> &'static str {
    if entry_type == "dir" {
        return FOLDER_ICON;
    }
    let lowered = name.to_lowercase();
    let extension = entry_extension(&lowered);
    if DIAGRAM_EXTENSIONS.contains(&extension) {
        return DIAGRAM_ICON;
    }
    // An extension hit always wins, even when it resolves to the default icon:
    // the shared resolver never falls through to the file-name table.
    if let Ok(index) = EXTENSIONS.binary_search_by(|(key, _)| (*key).cmp(extension)) {
        return EXTENSIONS[index].1;
    }
    if let Ok(index) = NAMES.binary_search_by(|(key, _)| (*key).cmp(lowered.as_str())) {
        return NAMES[index].1;
    }
    DEFAULT_ICON
}

/// Same rule as the desktop resolver, including the `.drawio.xml` special case
/// and hidden names whose extension starts at the first byte.
fn entry_extension(lowered_name: &str) -> &str {
    if lowered_name.ends_with(".drawio.xml") {
        return "drawio.xml";
    }
    match lowered_name.rfind('.') {
        Some(index) if index + 1 < lowered_name.len() => &lowered_name[index + 1..],
        _ => "",
    }
}

thread_local! {
    static CACHE: RefCell<HashMap<&'static str, Image>> = RefCell::new(HashMap::new());
}

fn cached_image(cache: &RefCell<HashMap<&'static str, Image>>, key: &'static str) -> Image {
    let mut cache = cache.borrow_mut();
    if let Some(image) = cache.get(key) {
        return image.clone();
    }
    let bytes = ICON_BYTES
        .iter()
        .find(|(candidate, _)| *candidate == key)
        .map(|(_, bytes)| *bytes);
    let image = bytes.and_then(decode_png).unwrap_or_default();
    cache.insert(key, image.clone());
    image
}

/// Icons are generated at 2x and drawn at 28px, so no resampling is needed
/// here: hand Slint the straight-alpha pixels as they are.
fn decode_png(bytes: &[u8]) -> Option<Image> {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    let (width, height) = image.dimensions();
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        image.as_raw(),
        width,
        height,
    );
    Some(Image::from_rgba8(buffer))
}

#[cfg(test)]
mod tests {
    use super::icon_key;

    #[test]
    fn resolves_like_the_desktop_resolver() {
        assert_eq!(icon_key("main.rs", "file"), "file_type_rust.png");
        assert_eq!(icon_key("notes.md", "file"), "file_type_markdown.png");
        assert_eq!(icon_key("Cargo.toml", "file"), "file_type_toml.png");
        assert_eq!(icon_key(".gitignore", "file"), "file_type_git.png");
        assert_eq!(icon_key("flow.drawio", "file"), "processon_flow.png");
        assert_eq!(icon_key("flow.drawio.xml", "file"), "processon_flow.png");
        assert_eq!(icon_key("report", "dir"), "folder.png");
        // Extension-less and trailing-dot names both land on the default text
        // glyph, exactly like the desktop table.
        assert_eq!(icon_key("未命名", "file"), "file_type_text.png");
        assert_eq!(icon_key("README.", "file"), "file_type_text.png");
    }

    #[test]
    fn document_cards_get_a_real_pixel_image() {
        let image = super::workspace_file_icon("季度报告.pdf", "file");
        assert!(image.size().width > 0 && image.size().height > 0);
    }
}
