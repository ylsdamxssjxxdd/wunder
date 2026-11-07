//! Bundled CJK face. Slint's shared font collection is created without host
//! fonts in this fork, so 蛹 injects Microsoft YaHei at startup; 茧 follows the
//! same pattern. Without a generic family and a Hani fallback the software
//! renderer's fallback query matches nothing and panics on first text layout.

static YAHEI_LZ4: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/msyh.ttc.lz4"));

pub fn install() {
    use slint::fontique_011::fontique;
    // Slint keeps the font collection per thread's context, and Android gives
    // every activity instance its own native thread. A process-wide "installed"
    // flag leaves the relaunched activity with an empty collection: all text
    // disappears while SVG icons keep drawing. Detect the face in the current
    // collection instead.
    let mut collection = slint::fontique_011::shared_collection();
    if collection.family_id("Microsoft YaHei").is_some() {
        return;
    }
    let Ok(bytes) = lz4_flex::decompress_size_prepended(YAHEI_LZ4) else {
        return;
    };
    let registered =
        collection.register_fonts(fontique::Blob::new(std::sync::Arc::new(bytes)), None);
    let families: Vec<_> = registered.iter().map(|(family, _)| *family).collect();
    if families.is_empty() {
        return;
    }
    for generic in [
        fontique::GenericFamily::SansSerif,
        fontique::GenericFamily::SystemUi,
        fontique::GenericFamily::UiSansSerif,
    ] {
        collection.set_generic_families(generic, families.iter().copied());
    }
    collection.append_fallbacks(
        // 蛹 uses the same const-checked constructor; the replacement
        // (`Script::from_bytes`) is not re-exported by this fork.
        #[allow(deprecated)]
        fontique::FallbackKey::new(fontique::Script::from_str_unchecked("Hani"), None),
        families.iter().copied(),
    );
}
