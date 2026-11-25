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

use slint::fontique_012::fontique;

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

/// 系统等宽字体。行内代码在 Slint 里走通用字族 Monospace，见 `install()`。
const SYSTEM_MONO_FACES: &[&str] = &[
    "RobotoMono-Regular.ttf",
    "RobotoMono-Regular.otf",
    "DroidSansMono.ttf",
    "NotoSansMono-Regular.ttf",
    "CutiveMono.ttf",
];

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
    let mut collection = slint::fontique_012::shared_collection();
    let dir = system_font_dir();

    // 先按系统自己的字族链注册（`/system/etc/fonts.xml`）。原生应用和微信
    // 用的就是这份：厂商把默认正文字体换成 MiSans / HarmonyOS Sans / OPPO
    // Sans 这类更轻的字面时，界面必须跟着换；此前写死 Noto Sans CJK 会把这
    // 些覆盖掉，中文看上去就比微信更黑更粗。
    let mut latin: Vec<fontique::FamilyId> = Vec::new();
    let mut cjk: Vec<fontique::FamilyId> = Vec::new();
    if let Some(plan) = platform_font_plan(&dir) {
        let latin_families = register_fonts(&mut collection, &dir.join(&plan.latin));
        let cjk_families = if plan.cjk == plan.latin {
            latin_families.clone()
        } else {
            register_fonts(&mut collection, &dir.join(&plan.cjk))
        };
        // A vendor file that turns out not to carry Han must not take over:
        // keeping a Latin-only family as the Han fallback would blank the UI.
        if covers_han(&mut collection, &cjk_families) {
            log::info!("platform fonts: latin={} cjk={}", plan.latin, plan.cjk);
            latin = latin_families;
            cjk = ordered_by_script(&mut collection, cjk_families, "Hans");
        }
    }

    // 读不到 fonts.xml（ROM 定制过头）时退回按文件名挑。
    if cjk.is_empty() {
        cjk = system_cjk_families(&mut collection, &dir);
    }
    if cjk.is_empty() {
        log::warn!("no CJK font found under {}; text may not render", dir.display());
        return;
    }
    if latin.is_empty() {
        latin = register_first(&mut collection, &dir, SYSTEM_LATIN_FACES).into_iter().collect();
    }
    let emoji = register_first(&mut collection, &dir, SYSTEM_EMOJI_FACES);
    let mono = register_first(&mut collection, &dir, SYSTEM_MONO_FACES);

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

    // 行内代码走的是通用字族 Monospace（Slint 的 `Style::Code`），不是字族名。
    // 不指定它，排版会逐字符走脚本回退：字母落到正文字体、数字落到符号字体，
    // 一个标识符被切成好几段字形，渲染层再按每段各画一个圆角底纹，看起来就是
    // 被一格一格框住。系统没有等宽字体时退回正文字族，至少整段只用一个字体。
    match mono {
        Some(mono) => collection.set_generic_families(
            fontique::GenericFamily::Monospace,
            core::iter::once(mono),
        ),
        None => collection.set_generic_families(
            fontique::GenericFamily::Monospace,
            generic_families.iter().copied(),
        ),
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

/// The two files the shell needs to draw like every other app on the phone.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PlatformFonts {
    /// The face the platform resolves `sans-serif` to (Latin and often Han).
    latin: String,
    /// The Han fallback the platform appends to that family. Equal to `latin`
    /// when one file covers both scripts, as MiSans / HarmonyOS Sans do.
    cjk: String,
}

/// Read the platform's own font families and pick the two files that carry the
/// default prose face.
///
/// Android declares the resolution order in `/system/etc/fonts.xml`: the first
/// `sans-serif` family is the default, and the `zh-Hans` (or unnamed fallback)
/// families after it supply Han glyphs. Native text views — and therefore
/// WeChat — draw exactly with that chain, so following it is what keeps the
/// app's Chinese as light as the rest of the phone instead of forcing our
/// preferred Noto face over the vendor's.
fn platform_font_plan(dir: &Path) -> Option<PlatformFonts> {
    let root = dir.parent()?;
    for relative in ["etc/fonts.xml", "etc/fonts_base.xml"] {
        let path = root.join(relative);
        let Ok(xml) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(plan) = plan_from_xml(&xml) {
            return Some(plan);
        }
    }
    None
}

/// Pure half of [`platform_font_plan`], so the selection rules can be tested
/// without a phone: document order, weight 400 / normal first, Han fallback
/// after the Latin default.
fn plan_from_xml(xml: &str) -> Option<PlatformFonts> {
    let families = families_in(xml);
    let latin_family = families
        .iter()
        .find(|family| family.name.to_ascii_lowercase().contains("sans-serif"))
        .or_else(|| families.first())?;
    let latin = latin_family.face()?.file.clone();

    // The Han fallback must stay in the sans family: `fonts.xml` also lists a
    // `serif`/`monospace` Han fallback, and a serif body would be a different
    // product, not a different weight.
    let by_language = families
        .iter()
        .filter(|family| family.lang.to_ascii_lowercase().contains("zh"))
        .filter(|family| is_sans_fallback(family))
        .find_map(|family| family.face().map(|face| face.file.clone()));
    let by_name = families
        .iter()
        .filter(|family| is_sans_fallback(family))
        .find_map(|family| {
            let face = family.face()?;
            (looks_like_cjk(&face.file) || looks_like_cjk(&family.name))
                .then(|| face.file.clone())
        });

    Some(PlatformFonts {
        latin: latin.clone(),
        cjk: by_language.or(by_name).unwrap_or(latin),
    })
}

#[derive(Debug)]
struct FontFamily {
    name: String,
    lang: String,
    faces: Vec<FontFace>,
}

#[derive(Debug)]
struct FontFace {
    file: String,
    weight: u16,
    style: String,
}

impl FontFamily {
    /// The face the platform would use for body copy: weight 400, upright, and
    /// otherwise the one closest to it. A family that only ships a heavy cut
    /// must not be picked just because it is listed first.
    fn face(&self) -> Option<&FontFace> {
        self.faces.iter().min_by_key(|face| {
            (
                !face.style.eq_ignore_ascii_case("normal"),
                face.weight.abs_diff(400),
            )
        })
    }
}

fn families_in(xml: &str) -> Vec<FontFamily> {
    let mut families = Vec::new();
    let mut rest = xml;
    while let Some(start) = tag_start(rest, "<family") {
        rest = &rest[start..];
        let Some(open_end) = rest.find('>') else { break };
        let attrs = &rest["<family".len()..open_end];
        let name = attribute(attrs, "name").unwrap_or_default();
        let lang = attribute(attrs, "lang").unwrap_or_default();
        let body = &rest[open_end + 1..];
        let Some(close) = body.find("</family") else { break };
        let faces = faces_in(&body[..close]);
        if !faces.is_empty() {
            families.push(FontFamily { name, lang, faces });
        }
        rest = &body[close..];
    }
    families
}

fn faces_in(body: &str) -> Vec<FontFace> {
    let mut faces = Vec::new();
    let mut rest = body;
    while let Some(start) = tag_start(rest, "<font") {
        rest = &rest[start..];
        let Some(open_end) = rest.find('>') else { break };
        let attrs = &rest["<font".len()..open_end];
        let after = &rest[open_end + 1..];
        let end = after.find('<').unwrap_or(after.len());
        let file = after[..end].trim();
        if !file.is_empty() {
            let weight = attribute(attrs, "weight")
                .and_then(|value| value.parse::<u16>().ok())
                .unwrap_or(400);
            let style = attribute(attrs, "style").unwrap_or_else(|| "normal".to_owned());
            faces.push(FontFace {
                file: file.to_owned(),
                weight,
                style,
            });
        }
        rest = &after[end..];
    }
    faces
}

/// Byte offset of `tag`, but only when it is a real tag start: `<familyset`
/// must not be mistaken for `<family`.
fn tag_start(haystack: &str, tag: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = haystack[from..].find(tag) {
        let index = from + found;
        let only_attrs_left = match haystack[index + tag.len()..].chars().next() {
            Some('>') => true,
            Some(c) => c.is_whitespace(),
            None => false,
        };
        if only_attrs_left {
            return Some(index);
        }
        from = index + tag.len();
    }
    None
}

fn attribute(attrs: &str, key: &str) -> Option<String> {
    let mut from = 0;
    while let Some(found) = attrs[from..].find(key) {
        let index = from + found;
        // Only a whole attribute name counts: `name` must not match `lang`.
        let boundary = attrs[..index]
            .chars()
            .next_back()
            .map_or(true, |c| !(c.is_alphanumeric() || c == '-' || c == '_'));
        if boundary {
            if let Some(rest) = attrs[index + key.len()..].trim_start().strip_prefix('=') {
                let rest = rest.trim_start();
                let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'');
                if let Some(quote) = quote {
                    let value = &rest[1..];
                    if let Some(end) = value.find(quote) {
                        return Some(value[..end].trim().to_owned());
                    }
                }
            }
        }
        from = index + key.len();
    }
    None
}

/// Unnamed families are the platform's own fallbacks; named ones have to be a
/// sans cut to stand in for body copy.
fn is_sans_fallback(family: &FontFamily) -> bool {
    let name = family.name.to_ascii_lowercase();
    name.is_empty() || name.contains("sans")
}

fn looks_like_cjk(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    CJK_NAME_HINTS.iter().any(|hint| lower.contains(hint))
        || ["misans", "harmony", "oppo", "vivo", "source-han", "sourcehan", "pingfang"]
            .iter()
            .any(|hint| lower.contains(hint))
}

/// Whether any of `families` actually has a glyph for Han, so the platform
/// chain is proven usable before it replaces the built-in preference list.
fn covers_han(collection: &mut fontique::Collection, families: &[fontique::FamilyId]) -> bool {
    let mut cache = fontique::SourceCache::default();
    let mut query = collection.query(&mut cache);
    query.set_families(families.iter().copied());
    let mut covers = false;
    query.matches_with(|font| {
        if font.charmap().is_some_and(|charmap| charmap.map('中').is_some()) {
            covers = true;
            fontique::QueryStatus::Stop
        } else {
            fontique::QueryStatus::Continue
        }
    });
    covers
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Stock Android: Roboto for Latin, an unnamed `zh-Hans` fallback for Han.
    const AOSP: &str = r#"<familyset version="23">
    <family name="sans-serif">
        <font weight="400" style="normal">Roboto-Regular.ttf</font>
        <font weight="700" style="normal">Roboto-Bold.ttf</font>
    </family>
    <family name="serif">
        <font weight="400" style="normal">NotoSerif-Regular.ttf</font>
    </family>
    <alias name="sans-serif" to="Roboto" />
    <family name="serif" lang="zh-Hans">
        <font weight="400" style="normal">NotoSerifCJK-Regular.ttc</font>
    </family>
    <family lang="zh-Hans">
        <font weight="400" style="normal">NotoSansCJK-Regular.ttc</font>
    </family>
</familyset>"#;

    /// A vendor that replaced the whole default family with its own face.
    const VENDOR: &str = r#"<familyset>
    <family name="sans-serif">
        <font weight="400" style="normal">MiSans-Regular.ttf</font>
        <font weight="500" style="normal">MiSans-Medium.ttf</font>
    </family>
</familyset>"#;

    const BOLD_ONLY: &str = r#"<familyset>
    <family name="sans-serif">
        <font weight="700" style="normal">Roboto-Bold.ttf</font>
        <font weight="400" style="normal">Roboto-Regular.ttf</font>
    </family>
</familyset>"#;

    #[test]
    fn aosp_picks_roboto_then_the_sans_han_fallback() {
        let plan = plan_from_xml(AOSP).expect("plan");
        assert_eq!(plan.latin, "Roboto-Regular.ttf");
        // Not the serif cut listed just above it.
        assert_eq!(plan.cjk, "NotoSansCJK-Regular.ttc");
    }

    #[test]
    fn a_single_vendor_face_serves_both_scripts() {
        let plan = plan_from_xml(VENDOR).expect("plan");
        assert_eq!(plan.latin, "MiSans-Regular.ttf");
        assert_eq!(plan.cjk, "MiSans-Regular.ttf");
    }

    #[test]
    fn weight_400_and_upright_wins_over_bold() {
        let plan = plan_from_xml(BOLD_ONLY).expect("plan");
        assert_eq!(plan.latin, "Roboto-Regular.ttf");
        assert_eq!(plan.cjk, "Roboto-Regular.ttf");
    }

    #[test]
    fn familyset_is_not_mistaken_for_a_family() {
        let families = families_in(AOSP);
        assert_eq!(families.len(), 4);
        assert_eq!(families[0].name, "sans-serif");
    }

    #[test]
    fn the_plan_reads_the_platform_config_next_to_the_font_directory() {
        let root = std::env::temp_dir().join(format!("wunder-fonts-{}", std::process::id()));
        let fonts = root.join("fonts");
        std::fs::create_dir_all(&fonts).expect("fonts dir");
        std::fs::create_dir_all(root.join("etc")).expect("etc dir");
        std::fs::write(root.join("etc").join("fonts.xml"), AOSP).expect("fonts.xml");
        let plan = platform_font_plan(&fonts).expect("plan from the fake system tree");
        assert_eq!(plan.latin, "Roboto-Regular.ttf");
        assert_eq!(plan.cjk, "NotoSansCJK-Regular.ttc");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn attributes_need_a_whole_name() {
        let families = families_in(
            r#"<family name="sans-serif" lang="zh-Hans"><font>X.ttf</font></family>"#,
        );
        assert_eq!(families[0].name, "sans-serif");
        assert_eq!(families[0].lang, "zh-Hans");
    }
}

