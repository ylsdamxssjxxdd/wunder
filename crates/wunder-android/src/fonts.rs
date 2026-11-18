//! Bundled CJK face. Slint's shared font collection is created without host
//! fonts in this fork, so 桌面版 injects Microsoft YaHei at startup; 安卓版 follows the
//! same pattern. Without a generic family and a Hani fallback the software
//! renderer's fallback query matches nothing and panics on first text layout.
//!
//! 微软雅黑没有 emoji 与大多数符号字形，安卓版再补一个系统彩色 emoji 字体作为
//! 回退，否则 Skia 只能把 emoji 画成 .notdef 方块。

static YAHEI_LZ4: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/msyh.ttc.lz4"));

/// 安卓系统自带的彩色 emoji 字体（按常见程度排序）。直接读取系统文件即可，
/// 不必把字体打进 APK；读不到就退化为没有 emoji 回退，不影响其余文本。
const SYSTEM_EMOJI_FACES: &[&str] = &[
    "NotoColorEmoji.ttf",
    "NotoColorEmoji-Ro.ttf",
    "NotoEmoji-Regular.ttf",
    "SamsungColorEmoji.ttf",
    "ColorEmoji.ttf",
];

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
        // 桌面版 uses the same const-checked constructor; the replacement
        // (`Script::from_bytes`) is not re-exported by this fork.
        #[allow(deprecated)]
        fontique::FallbackKey::new(fontique::Script::from_str_unchecked("Hani"), None),
        families.iter().copied(),
    );

    if let Some(emoji) = register_system_emoji(&mut collection) {
        // 排在雅黑之后：雅黑有字形的字符仍走雅黑，只有缺字（emoji、☁、⚠ 等
        // 符号）才落到 emoji 字体上。
        for generic in [
            fontique::GenericFamily::SansSerif,
            fontique::GenericFamily::SystemUi,
            fontique::GenericFamily::UiSansSerif,
        ] {
            collection.append_generic_families(generic, core::iter::once(emoji));
        }
        collection.set_generic_families(fontique::GenericFamily::Emoji, core::iter::once(emoji));
        // 无变体选择符、按文本展示的符号（Common script）同样走 emoji 回退。
        #[allow(deprecated)]
        collection.append_fallbacks(
            fontique::FallbackKey::new(fontique::Script::from_str_unchecked("Zyyy"), None),
            core::iter::once(emoji),
        );
    }
}

/// 读取并注册系统 emoji 字体，返回它的 family id。
fn register_system_emoji(
    collection: &mut slint::fontique_011::fontique::Collection,
) -> Option<slint::fontique_011::fontique::FamilyId> {
    use slint::fontique_011::fontique;
    let root = std::env::var("ANDROID_ROOT").unwrap_or_else(|_| "/system".to_owned());
    let dir = std::path::Path::new(&root).join("fonts");
    for face in SYSTEM_EMOJI_FACES {
        let Ok(bytes) = std::fs::read(dir.join(face)) else {
            continue;
        };
        let registered =
            collection.register_fonts(fontique::Blob::new(std::sync::Arc::new(bytes)), None);
        if let Some((family, _)) = registered.first() {
            return Some(*family);
        }
    }
    None
}
