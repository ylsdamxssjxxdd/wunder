//! 安卓版不内嵌字体，直接用手机自带的字体排版。
//!
//! Slint 的共享字体集合创建时不做宿主机字体枚举（桌面版据此在启动时注入雅黑），
//! 所以安卓版需要在首帧前把系统字体注册进来，否则软件/ Skia 文本排版找不到
//! 通用字族和汉字回退，文字会整片消失。
//!
//! 做法与手机系统一致：拉丁字母优先用系统 Roboto，汉字用系统自带的
//! Noto Sans CJK（或厂商的等价字体），符号与 emoji 用系统的彩色 emoji 字体。
//! 这些文件都在 `/system/fonts` 下，只读取，不复制进 APK。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use slint::fontique_011::fontique;

/// 系统自带的 CJK 无衬线字体，按常见程度排序。
const SYSTEM_CJK_FACES: &[&str] = &[
    "NotoSansCJK-Regular.ttc",
    "NotoSansCJKsc-Regular.otf",
    "NotoSansSC-Regular.otf",
    "NotoSansHans-Regular.otf",
    "NotoSansSC-VF.ttf",
    "DroidSansFallbackFull.ttf",
    "DroidSansFallback.ttf",
    "HarmonyOS_Sans_SC_Regular.ttf",
    "MiSans-Regular.ttf",
];

/// 系统拉丁字体。找不到就只用 CJK 字体里的拉丁字形，不影响中文。
const SYSTEM_LATIN_FACES: &[&str] = &["Roboto-Regular.ttf", "RobotoFlex-Regular.ttf"];

/// 安卓系统自带的彩色 emoji 字体（按常见程度排序）。
const SYSTEM_EMOJI_FACES: &[&str] = &[
    "NotoColorEmoji.ttf",
    "NotoColorEmoji-Ro.ttf",
    "NotoEmoji-Regular.ttf",
    "SamsungColorEmoji.ttf",
    "ColorEmoji.ttf",
];

/// 文件名里出现这些片段的字体视为 CJK 字体；系统文件名不规范时用它兜底。
const CJK_NAME_HINTS: &[&str] = &[
    "cjk",
    "fallback",
    "hans",
    "sanssc",
    "simplified",
    "-sc",
    "_sc",
];

thread_local! {
    /// Slint 的字体集合按线程上下文保存，安卓重建 Activity 会换新线程，
    /// 所以幂等标记也必须是线程级的：新线程要重新注册一次。
    static INSTALLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn install() {
    if INSTALLED.with(|installed| installed.replace(true)) {
        return;
    }
    let mut collection = slint::fontique_011::shared_collection();
    let dir = system_font_dir();

    let cjk = system_cjk_families(&mut collection, &dir);
    if cjk.is_empty() {
        log::warn!("no CJK font found under {}; text may not render", dir.display());
        return;
    }
    let latin = register_first(&mut collection, &dir, SYSTEM_LATIN_FACES);
    let emoji = register_first(&mut collection, &dir, SYSTEM_EMOJI_FACES);

    // 通用字族：拉丁在前、汉字在后，缺字时才落到 emoji。
    let mut generic_families = latin.into_iter().collect::<Vec<_>>();
    generic_families.extend(cjk.iter().copied());
    for generic in [
        fontique::GenericFamily::SansSerif,
        fontique::GenericFamily::SystemUi,
        fontique::GenericFamily::UiSansSerif,
    ] {
        collection.set_generic_families(generic, generic_families.iter().copied());
        if let Some(emoji) = emoji {
            collection.append_generic_families(generic, core::iter::once(emoji));
        }
    }

    // 汉字脚本回退：简体优先简体字形，繁体优先繁体字形。
    // 桌面版用同一个构造器；`Script::from_bytes` 在这个 fork 里没有导出。
    #[allow(deprecated)]
    {
        let hans = fontique::FallbackKey::new(fontique::Script::from_str_unchecked("Hans"), None);
        collection.set_fallbacks(hans, cjk.iter().copied());
        let hani = fontique::FallbackKey::new(fontique::Script::from_str_unchecked("Hani"), None);
        collection.set_fallbacks(hani, cjk.iter().copied());
        let hant = fontique::FallbackKey::new(fontique::Script::from_str_unchecked("Hant"), None);
        let hant_families = ordered_by_script(&mut collection, cjk.clone(), "Hant");
        collection.set_fallbacks(hant, hant_families.into_iter());
        if let Some(emoji) = emoji {
            collection.set_generic_families(
                fontique::GenericFamily::Emoji,
                core::iter::once(emoji),
            );
            // 无变体选择符、按文本展示的符号（Common script）同样走 emoji。
            let common =
                fontique::FallbackKey::new(fontique::Script::from_str_unchecked("Zyyy"), None);
            collection.set_fallbacks(common, core::iter::once(emoji));
        }
    }
}

fn system_font_dir() -> PathBuf {
    let root = std::env::var("ANDROID_ROOT").unwrap_or_else(|_| "/system".to_owned());
    Path::new(&root).join("fonts")
}

/// 注册系统 CJK 字体，返回按简体优先排好序的 family 列表。
fn system_cjk_families(
    collection: &mut fontique::Collection,
    dir: &Path,
) -> Vec<fontique::FamilyId> {
    if let Some(path) = first_existing(dir, SYSTEM_CJK_FACES) {
        let families = register_fonts(collection, &path);
        if !families.is_empty() {
            return ordered_by_script(collection, families, "Hans");
        }
    }
    // 厂商改过字体文件名：按片段猜一个，仍然读系统文件，不内嵌。
    for path in scan_dir(dir) {
        let name = file_name_lower(&path);
        if CJK_NAME_HINTS.iter().any(|hint| name.contains(hint)) {
            let families = register_fonts(collection, &path);
            if !families.is_empty() {
                return ordered_by_script(collection, families, "Hans");
            }
        }
    }
    // 最后兜底：整目录交给 fontique，再从 family 名里挑中文。
    collection.load_fonts_from_paths(core::iter::once(dir));
    let mut names: Vec<String> = collection.family_names().map(str::to_owned).collect();
    names.sort();
    let mut families = Vec::new();
    for name in names {
        let lower = name.to_ascii_lowercase();
        if CJK_NAME_HINTS.iter().any(|hint| lower.contains(hint)) {
            if let Some(id) = collection.family_id(&name) {
                families.push(id);
            }
        }
    }
    ordered_by_script(collection, families, "Hans")
}

fn register_first(
    collection: &mut fontique::Collection,
    dir: &Path,
    faces: &[&str],
) -> Option<fontique::FamilyId> {
    let path = first_existing(dir, faces)?;
    register_fonts(collection, &path).into_iter().next()
}

fn first_existing(dir: &Path, faces: &[&str]) -> Option<PathBuf> {
    faces.iter().map(|face| dir.join(*face)).find(|path| path.is_file())
}

fn scan_dir(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "ttf" | "ttc" | "otf"))
        })
        .collect();
    paths.sort_by_key(|path| !file_name_lower(path).contains("regular"));
    paths
}

fn file_name_lower(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn register_fonts(
    collection: &mut fontique::Collection,
    path: &Path,
) -> Vec<fontique::FamilyId> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    collection
        .register_fonts(fontique::Blob::new(Arc::new(bytes)), None)
        .into_iter()
        .map(|(family, _)| family)
        .collect()
}

/// 同一份 TTC 往往包含 JP/KR/SC/TC 多套字形，按目标脚本把最合适的排前面，
/// 否则简体界面会先命中日文字形。
fn ordered_by_script(
    collection: &mut fontique::Collection,
    families: Vec<fontique::FamilyId>,
    script: &str,
) -> Vec<fontique::FamilyId> {
    let mut ranked: Vec<(u8, fontique::FamilyId)> = families
        .into_iter()
        .map(|family| {
            let rank = collection
                .family_name(family)
                .map(|name| family_rank(name, script))
                .unwrap_or(2);
            (rank, family)
        })
        .collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    ranked.into_iter().map(|(_, family)| family).collect()
}

fn family_rank(name: &str, script: &str) -> u8 {
    let name = name.to_ascii_lowercase();
    let sc = name.contains(" sc") || name.contains("hans") || name.contains("simplified");
    let tc = name.contains(" tc")
        || name.contains("hant")
        || name.contains("traditional")
        || name.contains(" hk");
    let other_han = name.contains(" jp") || name.contains(" kr");
    if script == "Hant" {
        if tc {
            0
        } else if sc || other_han {
            3
        } else {
            1
        }
    } else if sc {
        0
    } else if tc || other_han {
        3
    } else {
        1
    }
}
