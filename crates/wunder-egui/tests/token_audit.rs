//! V-01：视觉令牌审计。
//!
//! 组件绘制只能引用 `theme::Tokens` 中的语义令牌。本测试直接扫描生产代码
//! （`#[cfg(test)]` 之前的部分），发现任何 `Color32` 字面量就让构建失败，避免
//! 颜色重新散落到各页面。`theme.rs` 是唯一的合法性来源，不参与扫描。

use std::path::{Path, PathBuf};

/// 允许在绘制代码中出现的无色值：透明是"不着色"，不是颜色选择。
const ALLOWED_LITERALS: &[&str] = &["Color32::TRANSPARENT"];

/// 所有可能重新引入颜色字面量的写法。
const FORBIDDEN_PATTERNS: &[&str] = &[
    "Color32::from_rgb",
    "Color32::from_rgba",
    "Color32::from_rgba_premultiplied",
    "Color32::from_black_alpha",
    "Color32::from_white_alpha",
    "Color32::from_gray",
    "Color32::WHITE",
    "Color32::BLACK",
    "Color32::RED",
    "Color32::GREEN",
    "Color32::BLUE",
    "Color32::YELLOW",
];

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Read a source file and drop everything from the first test module onwards,
/// so fixtures and assertions may still use arbitrary colours.
fn production_source(path: &Path) -> String {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    match text.find("#[cfg(test)]") {
        Some(index) => text[..index].to_owned(),
        None => text,
    }
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot list {}: {error}", dir.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn ui_sources_only_use_semantic_tokens() {
    let root = source_root();
    let mut files = Vec::new();
    rust_sources(&root, &mut files);
    assert!(!files.is_empty(), "no sources found under {root:?}");

    let mut violations = Vec::new();
    for path in &files {
        // `theme.rs` owns the palette; it is the one place allowed to write
        // literal colours.
        if path.file_name().is_some_and(|name| name == "theme.rs") {
            continue;
        }
        let source = production_source(path);
        for (index, line) in source.lines().enumerate() {
            // Comments may quote CSS values without painting anything.
            let mut code = line.split("//").next().unwrap_or("").to_owned();
            for allowed in ALLOWED_LITERALS {
                code = code.replace(allowed, "");
            }
            if let Some(pattern) = FORBIDDEN_PATTERNS
                .iter()
                .find(|pattern| code.contains(*pattern))
            {
                violations.push(format!(
                    "{}:{}: {} — {}",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    index + 1,
                    pattern,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "绘制代码必须使用 theme::Tokens，不能散写颜色字面量：\n{}",
        violations.join("\n")
    );
}

#[test]
fn every_token_role_survives_the_light_palette() {
    // A token that equals its neighbour is not a role: it hides a real state
    // difference (hover vs selected, card vs canvas, ...).
    let tokens = wunder_egui::theme::tokens();
    let distinct = [
        ("canvas", tokens.canvas),
        ("surface", tokens.surface),
        ("surface_sunken", tokens.surface_sunken),
        ("hover", tokens.hover),
        ("selected", tokens.selected),
    ];
    for (index, (name, color)) in distinct.iter().enumerate() {
        for (other_name, other) in distinct.iter().skip(index + 1) {
            assert_ne!(
                color, other,
                "{name} 与 {other_name} 颜色相同，交互状态无法区分"
            );
        }
    }
    // `sidebar` and `timeline_hover` intentionally share one value: the
    // sidebar fill doubles as the light timeline hover. Both must still differ
    // from the canvas they are painted over, or hover feedback disappears.
    assert_ne!(tokens.sidebar, tokens.canvas);
    assert_ne!(tokens.timeline_hover, tokens.canvas);
    // Text and accent roles must remain readable against the canvas: the
    // light palette stays well above the 4.5:1 body-copy threshold.
    for (name, color) in [
        ("text", tokens.text),
        ("heading", tokens.heading),
        ("text_secondary", tokens.text_secondary),
    ] {
        let ratio = contrast_ratio(color, tokens.canvas);
        assert!(
            ratio >= 7.0,
            "{name} 与画布的对比度只有 {ratio:.2}:1，低于 AA 正文要求"
        );
    }
    let muted = contrast_ratio(tokens.text_muted, tokens.canvas);
    assert!(
        muted >= 3.0,
        "次要文字对比度只有 {muted:.2}:1，低于 AA 大字/边界要求"
    );
}

fn relative_luminance(color: egui::Color32) -> f32 {
    fn channel(value: u8) -> f32 {
        let value = f32::from(value) / 255.0;
        if value <= 0.039_28 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

fn contrast_ratio(a: egui::Color32, b: egui::Color32) -> f32 {
    let (light, dark) = {
        let a = relative_luminance(a);
        let b = relative_luminance(b);
        if a > b {
            (a, b)
        } else {
            (b, a)
        }
    };
    (light + 0.05) / (dark + 0.05)
}
