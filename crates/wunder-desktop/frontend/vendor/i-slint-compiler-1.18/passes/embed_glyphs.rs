// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

// cSpell: ignore fsdm msdf msdfgen
use crate::diagnostics::BuildDiagnostics;
#[cfg(not(target_arch = "wasm32"))]
use crate::embedded_resources::{BitmapFont, BitmapGlyph, BitmapGlyphs, CharacterMapEntry};
#[cfg(not(target_arch = "wasm32"))]
use crate::expression_tree::BuiltinFunction;
use crate::expression_tree::{Expression, Unit};
use crate::object_tree::*;
use crate::CompilerConfiguration;
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;

use i_slint_common::sharedfontique::{self, fontique, skrifa};
#[cfg(not(target_arch = "wasm32"))]
use skrifa::MetadataProvider;

#[derive(Clone)]
struct Font {
    font: fontique::QueryFont,
}

/// The fontique collection shared by `embed_glyphs` and `embed_images`, together
/// with the imported fonts' file paths (the collection only knows them as in-memory
/// blobs, so the paths are tracked separately for embedding).
#[cfg(feature = "renderer-software")]
pub struct FontCollection {
    pub collection: sharedfontique::Collection,
    pub custom_font_paths: HashMap<fontique::FamilyId, std::path::PathBuf>,
    pub custom_fonts: HashMap<std::path::PathBuf, fontique::QueryFont>,
}

/// Built once and shared (by reference) between the font and image passes. The
/// `LazyLock` defers the system-font scan to the first lookup, so a build with no
/// glyphs or text SVGs to embed never scans.
#[cfg(feature = "renderer-software")]
pub type SharedFontCollection = std::sync::Arc<
    std::sync::LazyLock<
        std::sync::Mutex<FontCollection>,
        Box<dyn FnOnce() -> std::sync::Mutex<FontCollection> + Send + Sync>,
    >,
>;

/// Reads every imported (`import "...ttf"`) font file, reporting load errors with
/// their import span. The bytes feed [`shared_font_collection`].
#[cfg(feature = "renderer-software")]
pub fn read_custom_fonts<'a>(
    all_docs: impl Iterator<Item = &'a Document>,
    diag: &mut BuildDiagnostics,
) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut fonts = Vec::new();
    for doc in all_docs {
        for (font_path, import_token) in doc.custom_fonts.iter() {
            match std::fs::read(font_path.as_str()) {
                Err(e) => diag.push_error(format!("Error loading font: {e}"), import_token),
                Ok(bytes) => fonts.push((font_path.as_str().into(), bytes)),
            }
        }
    }
    fonts
}

/// Wraps the system fonts plus the imported `custom_fonts` into a [`SharedFontCollection`].
#[cfg(feature = "renderer-software")]
pub fn shared_font_collection(
    custom_fonts: Vec<(std::path::PathBuf, Vec<u8>)>,
) -> SharedFontCollection {
    let init: Box<dyn FnOnce() -> std::sync::Mutex<FontCollection> + Send + Sync> =
        Box::new(move || {
            let mut collection = sharedfontique::create_collection(true);
            let mut custom_font_paths = HashMap::new();
            let mut custom_font_map = HashMap::new();
            for (path, bytes) in custom_fonts {
                if let Some(font) = collection
                    .register_fonts(bytes.into(), None)
                    .first()
                    .and_then(|(id, infos)| collection.get_font_for_info(*id, infos.first()?))
                {
                    custom_font_paths.insert(font.family.0, path.clone());
                    custom_font_map.insert(path, font);
                }
            }
            std::sync::Mutex::new(FontCollection {
                collection,
                custom_font_paths,
                custom_fonts: custom_font_map,
            })
        });
    std::sync::Arc::new(std::sync::LazyLock::new(init))
}

fn swash_font_ref(font: &Font) -> swash::FontRef<'_> {
    swash::FontRef::from_index(font.font.blob.data(), font.font.index as usize).unwrap()
}

#[cfg(target_arch = "wasm32")]
pub fn embed_glyphs<'a>(
    _component: &Document,
    _compiler_config: &CompilerConfiguration,
    _scale_factor: f64,
    _pixel_sizes: Vec<i16>,
    _font_weights: Vec<u16>,
    _characters_seen: HashSet<char>,
    _all_docs: impl Iterator<Item = &'a crate::object_tree::Document> + 'a,
    _diag: &mut BuildDiagnostics,
) -> bool {
    false
}

#[cfg(not(target_arch = "wasm32"))]
pub fn embed_glyphs(
    doc: &Document,
    compiler_config: &CompilerConfiguration,
    mut pixel_sizes: Vec<i16>,
    font_weights: Vec<u16>,
    mut characters_seen: HashSet<char>,
    font_collection: &SharedFontCollection,
    diag: &mut BuildDiagnostics,
) {
    use crate::diagnostics::Spanned;

    let generic_diag_location = doc.node.as_ref().map(|n| n.to_source_location());
    let scale_factor = compiler_config.const_scale_factor.unwrap_or(1.);

    characters_seen.extend(
        ('a'..='z')
            .chain('A'..='Z')
            .chain('0'..='9')
            .chain(" '!\"#$%&()*+,-./:;<=>?@\\[]{}^_|~".chars())
            .chain(std::iter::once('●'))
            .chain(std::iter::once('…')),
    );

    if let Ok(sizes_str) = std::env::var("SLINT_FONT_SIZES") {
        for custom_size_str in sizes_str.split(',') {
            let custom_size = if let Ok(custom_size) = custom_size_str
                .parse::<f32>()
                .map(|size_as_float| (size_as_float * scale_factor) as i16)
            {
                custom_size
            } else {
                diag.push_error(
                    format!(
                        "Invalid font size '{custom_size_str}' specified in `SLINT_FONT_SIZES`"
                    ),
                    &generic_diag_location,
                );
                return;
            };

            if let Err(pos) = pixel_sizes.binary_search(&custom_size) {
                pixel_sizes.insert(pos, custom_size)
            }
        }
    }

    // The collection (system fonts + imported fonts) is built once and shared with
    // `embed_images`; the imported-font paths come with it.
    let mut shared = font_collection.lock().unwrap();
    let FontCollection {
        collection,
        custom_font_paths: font_paths,
        custom_fonts,
    } = &mut *shared;

    let mut custom_face_error = false;

    let default_fonts: Vec<(std::path::PathBuf, fontique::QueryFont)> = if !collection
        .default_fonts
        .is_empty()
    {
        collection.default_fonts.as_ref().clone()
    } else {
        let mut default_fonts: Vec<(std::path::PathBuf, fontique::QueryFont)> = Vec::new();

        for c in doc.exported_roots() {
            let (family, source_location) = c
                .root_element
                .borrow()
                .binding("default-font-family")
                .and_then(|binding| match binding.value_expression() {
                    Expression::StringLiteral(family) => {
                        Some((Some(family.clone()), binding.span.clone()))
                    }
                    _ => None,
                })
                .unwrap_or_default();

            let font = {
                let mut query = collection.query();

                query.set_families(
                    family
                        .as_ref()
                        .map(|family| fontique::QueryFamily::from(family.as_str()))
                        .into_iter()
                        .chain(
                            sharedfontique::FALLBACK_FAMILIES
                                .into_iter()
                                .map(fontique::QueryFamily::Generic),
                        ),
                );

                let mut font = None;

                query.matches_with(|queried_font| {
                    font = Some(queried_font.clone());
                    fontique::QueryStatus::Stop
                });
                font
            };

            match font {
                None => {
                    if let Some(source_location) = source_location {
                        diag.push_error_with_span("could not find font that provides specified family, falling back to Sans-Serif".to_string(), source_location);
                    } else {
                        diag.push_error(
                            "internal error: could not determine a default font for sans-serif"
                                .to_string(),
                            &generic_diag_location,
                        );
                    };
                }
                Some(query_font) => {
                    if let Some(font_info) = collection
                        .family(query_font.family.0)
                        .and_then(|family_info| family_info.fonts().first().cloned())
                    {
                        let path = if let Some(path) = font_paths.get(&query_font.family.0) {
                            path.clone()
                        } else {
                            match &font_info.source().kind {
                                fontique::SourceKind::Path(path) => path.to_path_buf(),
                                fontique::SourceKind::Memory(_) => {
                                    diag.push_error(
                                    "internal error: memory fonts are not supported in the compiler"
                                        .to_string(),
                                    &generic_diag_location,
                                );
                                    custom_face_error = true;
                                    continue;
                                }
                            }
                        };
                        font_paths.insert(query_font.family.0, path.clone());
                        // Several exported top-level components commonly use
                        // the same default face. They all register the shared
                        // resource below, so embedding identical bitmap data
                        // once per root only inflates generated code and the
                        // executable's read-only data section.
                        if !default_fonts.iter().any(|(existing_path, existing_font)| {
                            existing_path == &path && existing_font.index == query_font.index
                        }) {
                            default_fonts.push((path.clone(), query_font));
                        }
                    }
                }
            }
        }

        default_fonts
    };

    if custom_face_error {
        return;
    }

    // The software renderer selects one pre-rendered BitmapFont for an entire
    // text run. Build that face from every project-owned font: the first
    // default remains authoritative, while later imported fonts contribute
    // only glyphs the primary face lacks. This is intentionally compiled into
    // the font atlas;
    // the Win7 application still never discovers or loads host system fonts.
    let fallback_fonts = project_fallback_fonts(&default_fonts, custom_fonts);

    let register_embedded_font = |path: &std::path::Path, embedded_bitmap_font: BitmapFont| {
        let resource_id = doc.embedded_file_resources.borrow_mut().push_and_get_key(
            crate::embedded_resources::EmbeddedResources {
                path: Some(path.to_string_lossy().as_ref().into()),
                kind: crate::embedded_resources::EmbeddedResourcesKind::BitmapFontData(
                    embedded_bitmap_font,
                ),
            },
        );

        for c in doc.exported_roots() {
            c.init_code
                .borrow_mut()
                .font_registration_code
                .push(Expression::FunctionCall {
                    function: BuiltinFunction::RegisterBitmapFont.into(),
                    arguments: vec![Expression::NumberLiteral(resource_id.0 as _, Unit::None)],
                    source_location: None,
                });
        }
    };

    let mut embed_font_by_path = |path: &std::path::Path, font: &fontique::QueryFont| {
        let Some(family_name) = collection.family_name(font.family.0).to_owned() else {
            diag.push_error(
                format!(
                    "internal error: TrueType font without family name encountered: {}",
                    path.display()
                ),
                &generic_diag_location,
            );
            return;
        };

        let Some(font_ref) = skrifa::FontRef::from_index(font.blob.data(), font.index).ok() else {
            diag.push_error(
                format!("internal error: failed to parse font: {}", path.display()),
                &generic_diag_location,
            );
            return;
        };

        // TextInput values are runtime data and therefore absent from Slint's
        // string-literal scan. Include both the selected face and the
        // project-owned fallback faces: this emits one deterministic bitmap
        // face instead of relying on the target machine's font fallback.
        let mut font_characters = characters_seen.clone();
        for face in core::iter::once(&Font { font: font.clone() }).chain(fallback_fonts.iter()) {
            let face_ref = skrifa::FontRef::from_index(face.font.blob.data(), face.font.index)
                .expect("project font is parseable");
            font_characters.extend(
                face_ref
                    .charmap()
                    .mappings()
                    .filter_map(|(code_point, _)| char::from_u32(code_point)),
            );
        }
        let axes = font_ref.axes();
        let wght_axis = axes
            .iter()
            .find(|axis| axis.tag() == skrifa::Tag::new(b"wght"));

        if let Some(wght_axis) = wght_axis {
            // Variable font: embed one BitmapFont per requested weight
            let weights = if font_weights.is_empty() {
                vec![fontique::FontWeight::NORMAL.value() as u16]
            } else {
                font_weights.clone()
            };
            for &weight in &weights {
                let clamped = (weight as f32).clamp(wght_axis.min_value(), wght_axis.max_value());
                let location = axes.location([("wght", clamped)]);
                let variations = vec![(skrifa::Tag::new(b"wght"), clamped)];

                let embedded = embed_font(
                    family_name.to_owned(),
                    Font { font: font.clone() },
                    &pixel_sizes,
                    font_characters.iter().cloned(),
                    &characters_seen,
                    &fallback_fonts,
                    compiler_config,
                    location.coords(),
                    &variations,
                    Some(weight),
                );
                register_embedded_font(path, embedded);
            }
        } else {
            // Static font: embed once
            let embedded = embed_font(
                family_name.to_owned(),
                Font { font: font.clone() },
                &pixel_sizes,
                font_characters.iter().cloned(),
                &characters_seen,
                &fallback_fonts,
                compiler_config,
                &[],
                &[],
                None,
            );
            register_embedded_font(path, embedded);
        }
    };

    // The first default face is already composed with every project-owned
    // fallback above. Registering the fallback as a second full CJK atlas
    // would duplicate tens of thousands of bitmap glyphs in the executable.
    if let Some((path, font)) = default_fonts.first() {
        embed_font_by_path(path, font);
    }
}

#[inline(never)] // workaround https://github.com/rust-lang/rust/issues/104099
fn project_fallback_fonts(
    default_fonts: &[(std::path::PathBuf, fontique::QueryFont)],
    custom_fonts: &HashMap<std::path::PathBuf, fontique::QueryFont>,
) -> Vec<Font> {
    let mut paths: Vec<_> = custom_fonts.iter().collect();
    paths.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));

    let mut seen = HashSet::new();
    let mut fallback_fonts = Vec::new();
    for font in default_fonts
        .iter()
        .map(|(_, font)| font)
        .chain(paths.into_iter().map(|(_, font)| font))
    {
        if seen.insert((font.blob.id(), font.index)) {
            fallback_fonts.push(Font { font: font.clone() });
        }
    }
    fallback_fonts
}

#[cfg(not(target_arch = "wasm32"))]
fn embed_font(
    family_name: String,
    font: Font,
    pixel_sizes: &[i16],
    character_coverage: impl Iterator<Item = char>,
    seen_characters: &HashSet<char>,
    fallback_fonts: &[Font],
    _compiler_config: &CompilerConfiguration,
    normalized_coords: &[skrifa::instance::NormalizedCoord],
    _variations: &[(skrifa::Tag, f32)],
    override_weight: Option<u16>,
) -> BitmapFont {
    let coords_i16: Vec<i16> = normalized_coords.iter().map(|c| c.to_bits()).collect();

    let mut character_map: Vec<CharacterMapEntry> = character_coverage
        .filter(|code_point| {
            core::iter::once(&font)
                .chain(fallback_fonts.iter())
                .any(|font| swash_font_ref(font).charmap().map(*code_point) != 0)
        })
        .enumerate()
        .map(|(glyph_index, code_point)| CharacterMapEntry {
            code_point,
            glyph_index: u16::try_from(glyph_index)
                .expect("more than 65535 glyphs are not supported"),
        })
        .collect();

    #[cfg(feature = "sdf-fonts")]
    let glyphs = if _compiler_config.use_sdf_fonts {
        embed_sdf_glyphs(
            pixel_sizes,
            &character_map,
            &font,
            fallback_fonts,
            _variations,
        )
    } else {
        embed_alpha_map_glyphs(
            pixel_sizes,
            &character_map,
            &font,
            fallback_fonts,
            &coords_i16,
            seen_characters,
        )
    };
    #[cfg(not(feature = "sdf-fonts"))]
    let glyphs = embed_alpha_map_glyphs(
        pixel_sizes,
        &character_map,
        &font,
        fallback_fonts,
        &coords_i16,
        seen_characters,
    );

    character_map.sort_by_key(|entry| entry.code_point);

    let font_ref = skrifa::FontRef::from_index(font.font.blob.data(), font.font.index).unwrap();
    let location = skrifa::instance::LocationRef::new(normalized_coords);
    let metrics =
        skrifa::metrics::Metrics::new(&font_ref, skrifa::instance::Size::unscaled(), location);
    let attrs = skrifa::attribute::Attributes::new(&font_ref);

    BitmapFont {
        family_name,
        character_map,
        units_per_em: metrics.units_per_em as f32,
        ascent: metrics.ascent,
        descent: metrics.descent,
        x_height: metrics.x_height.unwrap_or_default(),
        cap_height: metrics.cap_height.unwrap_or_default(),
        glyphs,
        weight: override_weight.unwrap_or(attrs.weight.value() as u16),
        italic: attrs.style != skrifa::attribute::Style::Normal,
        #[cfg(feature = "sdf-fonts")]
        sdf: _compiler_config.use_sdf_fonts,
        #[cfg(not(feature = "sdf-fonts"))]
        sdf: false,
    }
}

/// Pack an 8-bit alpha map that only contains 0/255 into 1-bit rows (MSB
/// first, each row padded to a whole byte). Returns None when the map holds
/// gray values or the dimensions do not match the data.
#[cfg(not(target_arch = "wasm32"))]
fn pack_mono_1bpp(data: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || data.len() < width * height {
        return None;
    }
    let data = &data[..width * height];
    if !data.iter().all(|&b| b == 0 || b == 255) {
        return None;
    }
    let stride = width.div_ceil(8);
    let mut packed = vec![0u8; stride * height];
    for y in 0..height {
        for x in 0..width {
            if data[y * width + x] == 255 {
                packed[y * stride + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    Some(packed)
}

#[cfg(not(target_arch = "wasm32"))]
fn embed_alpha_map_glyphs(
    pixel_sizes: &[i16],
    character_map: &Vec<CharacterMapEntry>,
    font: &Font,
    fallback_fonts: &[Font],
    normalized_coords: &[i16],
    seen_characters: &HashSet<char>,
) -> Vec<BitmapGlyphs> {
    use rayon::prelude::*;
    use std::cell::RefCell;

    thread_local! {
        static SCALE_CONTEXT: RefCell<swash::scale::ScaleContext> =
            RefCell::new(swash::scale::ScaleContext::new());
    }

    pixel_sizes
        .par_iter()
        .map(|pixel_size| {
            // SimSun's EBDT strikes exist at 12-16/18 ppem; keep the full cmap
            // at those sizes (monochrome bitmaps pack to 1 bit per pixel, so
            // full coverage is cheap there). Other sizes use hinted outline
            // rasterization and are bounded to GB2312 + UI literals + ASCII.
            let full_coverage = matches!(*pixel_size, 12 | 13 | 14 | 15 | 16 | 18);
            let glyph_data = character_map
                .par_iter()
                .map(|CharacterMapEntry { code_point, .. }| {
                    let font_to_use = core::iter::once(font)
                        .chain(fallback_fonts.iter())
                        .find(|f| swash_font_ref(f).charmap().map(*code_point) != 0)
                        .unwrap_or(font);

                    let font_ref = swash_font_ref(font_to_use);
                    let glyph_id = font_ref.charmap().map(*code_point);
                    let gm = font_ref.glyph_metrics(normalized_coords);
                    let fm = font_ref.metrics(normalized_coords);
                    let scale = *pixel_size as f32 / fm.units_per_em as f32;
                    let advance_width = gm.advance_width(glyph_id) * scale;

                    // Out-of-coverage glyphs keep their advance but stay blank,
                    // so rare characters never turn into tofu boxes at sizes
                    // with bounded coverage.
                    let covered = full_coverage
                        || seen_characters.contains(code_point)
                        || super::gb2312_table::GB2312_CODE_POINTS
                            .binary_search(&(*code_point as u32))
                            .is_ok();
                    if !covered {
                        return BitmapGlyph {
                            x_advance: i16::try_from((advance_width * 64.) as i64)
                                .expect("large advance width"),
                            ..Default::default()
                        };
                    }

                    SCALE_CONTEXT.with(|ctx| {
                        let font_ref = swash_font_ref(font_to_use);
                        let mut ctx = ctx.borrow_mut();
                        let mut scaler = ctx
                            .builder(font_ref)
                            .size(*pixel_size as f32)
                            .hint(true)
                            .normalized_coords(normalized_coords)
                            .build();
                        // SimSun's small EBDT strikes do not contain ASCII
                        // glyphs. Rendering those letters/digits as a normal
                        // alpha mask makes them visibly softer than native GDI
                        // ClearType on Win7. Keep CJK on the hand-tuned bitmap
                        // strikes, but render printable ASCII outlines into an
                        // RGB subpixel mask. The software renderer recognizes
                        // `data_packing == 2` and blends its three coverages
                        // independently. This stays fully embedded and avoids
                        // DirectWrite/system-font dependencies at runtime.
                        let subpixel = code_point.is_ascii_graphic() || *code_point == ' ';
                        let image = if subpixel {
                            swash::scale::Render::new(&[swash::scale::Source::Outline])
                                .format(swash::zeno::Format::Subpixel)
                                .render(&mut scaler, glyph_id)
                        } else {
                            // Prefer the font's embedded monochrome bitmaps
                            // (SimSun EBDT strikes at 12-18 ppem): they are
                            // hand-tuned pixel glyphs. Hinted outlines cover
                            // the sizes without a strike.
                            swash::scale::Render::new(&[
                                swash::scale::Source::Bitmap(
                                    swash::scale::StrikeWith::ExactSize,
                                ),
                                swash::scale::Source::Outline,
                            ])
                            .format(swash::zeno::Format::Alpha)
                            .render(&mut scaler, glyph_id)
                        };

                        match image {
                            Some(image) => {
                                let p = image.placement;
                                let (data, data_packing) = if subpixel {
                                    debug_assert_eq!(
                                        image.data.len(),
                                        p.width as usize * p.height as usize * 4
                                    );
                                    (image.data, 2u8)
                                } else {
                                    // EBDT strikes are pure black/white; pack
                                    // those to 1 bit per pixel (8x smaller in
                                    // the binary).
                                    pack_mono_1bpp(
                                        &image.data,
                                        p.width as usize,
                                        p.height as usize,
                                    )
                                    .map(|packed| (packed, 1u8))
                                    .unwrap_or_else(|| (image.data, 0u8))
                                };
                                BitmapGlyph {
                                    x: i16::try_from(p.left * 64)
                                        .expect("large glyph x coordinate"),
                                    y: i16::try_from((p.top - p.height as i32) * 64)
                                        .expect("large glyph y coordinate"),
                                    width: i16::try_from(p.width).expect("large width"),
                                    height: i16::try_from(p.height).expect("large height"),
                                    x_advance: i16::try_from((advance_width * 64.) as i64)
                                        .expect("large advance width"),
                                    data_packing,
                                    data,
                                }
                            }
                            None => BitmapGlyph {
                                x_advance: i16::try_from((advance_width * 64.) as i64)
                                    .expect("large advance width"),
                                ..Default::default()
                            },
                        }
                    })
                })
                .collect();

            BitmapGlyphs {
                pixel_size: *pixel_size,
                glyph_data,
            }
        })
        .collect()
}

#[cfg(all(not(target_arch = "wasm32"), feature = "sdf-fonts"))]
fn embed_sdf_glyphs(
    pixel_sizes: &[i16],
    character_map: &Vec<CharacterMapEntry>,
    font: &Font,
    fallback_fonts: &[Font],
    variations: &[(skrifa::Tag, f32)],
) -> Vec<BitmapGlyphs> {
    use rayon::prelude::*;

    const RANGE: f64 = 6.;

    let Some(max_size) = pixel_sizes.iter().max() else {
        return Vec::new();
    };
    let min_size = pixel_sizes
        .iter()
        .min()
        .expect("we have a 'max' so the vector is not empty");
    // Slint's upstream 16 px floor is sufficient for Latin UI text, but dense
    // SimSun strokes lose their structure when sampled down to compact 10–13
    // px controls. Use a 24 px source for the one scalable GBK atlas: this is
    // the smallest tested size that keeps the regular SimSun glyph structure
    // without renderer-side contour shifts that can make strokes stick.
    let target_pixel_size = (max_size * 2 / 3).max(24).min(RANGE as i16 * min_size);

    let glyph_data = character_map
        .par_iter()
        .map(|CharacterMapEntry { code_point, .. }| {
            core::iter::once(font)
                .chain(fallback_fonts.iter())
                .find_map(|font| {
                    (swash_font_ref(font).charmap().map(*code_point) != 0).then(|| {
                        generate_sdf_for_glyph(
                            font,
                            *code_point,
                            target_pixel_size,
                            RANGE,
                            variations,
                        )
                    })
                })
                .unwrap_or_else(|| {
                    generate_sdf_for_glyph(font, *code_point, target_pixel_size, RANGE, variations)
                })
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();

    vec![BitmapGlyphs {
        pixel_size: target_pixel_size,
        glyph_data,
    }]
}

#[cfg(all(not(target_arch = "wasm32"), feature = "sdf-fonts"))]
fn generate_sdf_for_glyph(
    font: &Font,
    code_point: char,
    target_pixel_size: i16,
    range: f64,
    variations: &[(skrifa::Tag, f32)],
) -> Option<BitmapGlyph> {
    use fdsm::transform::Transform;
    use nalgebra::{Affine2, Similarity2, Vector2};

    let mut face =
        fdsm_ttf_parser::ttf_parser::Face::parse(font.font.blob.data(), font.font.index).unwrap();
    for &(tag, value) in variations {
        face.set_variation(
            fdsm_ttf_parser::ttf_parser::Tag(u32::from_be_bytes(tag.to_be_bytes())),
            value,
        );
    }
    let glyph_id = face.glyph_index(code_point).unwrap_or_default();

    let font_ref = skrifa::FontRef::from_index(font.font.blob.data(), font.font.index).unwrap();
    let variation_settings: Vec<_> = variations
        .iter()
        .map(|&(tag, value)| (tag, value))
        .collect::<Vec<_>>();
    let location = font_ref.axes().location(variation_settings);
    let metrics = skrifa::metrics::Metrics::new(
        &font_ref,
        skrifa::instance::Size::unscaled(),
        skrifa::instance::LocationRef::from(&location),
    );
    let target_pixel_size = target_pixel_size as f64;
    let scale = target_pixel_size / metrics.units_per_em as f64;

    // TODO: handle bitmap glyphs (emojis)
    let Some(bbox) = face.glyph_bounding_box(glyph_id) else {
        // For example, for space
        return Some(BitmapGlyph {
            x_advance: (face.glyph_hor_advance(glyph_id).unwrap_or(0) as f64 * scale * 64.) as i16,
            ..Default::default()
        });
    };

    let mut shape = fdsm_ttf_parser::load_shape_from_face(&face, glyph_id)?;

    let width = ((bbox.x_max as f64 - bbox.x_min as f64) * scale + 2.).ceil() as u32;
    let height = ((bbox.y_max as f64 - bbox.y_min as f64) * scale + 2.).ceil() as u32;
    let transformation = nalgebra::convert::<_, Affine2<f64>>(Similarity2::new(
        Vector2::new(
            1. - bbox.x_min as f64 * scale,
            1. - bbox.y_min as f64 * scale,
        ),
        0.,
        scale,
    ));

    // Unlike msdfgen, the transformation is not passed into the
    // `generate_msdf` function – the coordinates of the control points
    // must be expressed in terms of pixels on the distance field. To get
    // the correct units, we pre-transform the shape:

    shape.transform(&transformation);

    let prepared_shape = shape.prepare();

    // Set up the resulting image and generate the distance field:

    let mut sdf = image::GrayImage::new(width, height);
    fdsm::generate::generate_sdf(&prepared_shape, range, &mut sdf);
    fdsm::render::correct_sign_sdf(
        &mut sdf,
        &prepared_shape,
        fdsm::bezier::scanline::FillRule::Nonzero,
    );

    let mut glyph_data = sdf.into_raw();

    // normalize around 0
    for x in &mut glyph_data {
        *x = x.wrapping_sub(128);
    }

    // invert the y coordinate (as the fsdm crate has the y axis inverted)
    let (w, h) = (width as usize, height as usize);
    for idx in 0..glyph_data.len() / 2 {
        glyph_data.swap(idx, (h - idx / w - 1) * w + idx % w);
    }

    // Add a "0" so that we can always access pos+1 without going out of bound
    // (so that the last row will look like `data[len-1]*1 + data[len]*0`)
    glyph_data.push(0);

    let bg = BitmapGlyph {
        x: i16::try_from((-(1. - bbox.x_min as f64 * scale) * 64.).ceil() as i32)
            .expect("large glyph x coordinate"),
        y: i16::try_from((-(1. - bbox.y_min as f64 * scale) * 64.).ceil() as i32)
            .expect("large glyph y coordinate"),
        width: i16::try_from(width).expect("large width"),
        height: i16::try_from(height).expect("large height"),
        x_advance: i16::try_from(
            (face.glyph_hor_advance(glyph_id).unwrap() as f64 * scale * 64.).round() as i32,
        )
        .expect("large advance width"),
        data_packing: 0,
        data: glyph_data,
    };

    Some(bg)
}

fn try_extract_literal_from_element(
    elem: &ElementRc,
    property_name: &str,
    unit: Unit,
) -> Option<f64> {
    elem.borrow().binding(property_name).and_then(|binding| match binding.value_expression() {
        Expression::NumberLiteral(value, u) if *u == unit => Some(*value),
        Expression::Cast { from, .. } => match from.as_ref() {
            Expression::NumberLiteral(value, u) if *u == unit => Some(*value),
            _ => None,
        },
        _ => None,
    })
}

pub fn collect_font_sizes_used(
    component: &Rc<Component>,
    scale_factor: f64,
    sizes_seen: &mut Vec<i16>,
) {
    let mut add_font_size = |logical_size: f64| {
        let pixel_size = (logical_size * scale_factor) as i16;
        // Zero is used by the widget interfaces as an "inherit the default"
        // sentinel (for example LineEdit's font-size). It is not a drawable
        // font size. Passing it to the alpha-map rasterizer selects an
        // effectively unscaled outline and embeds enormous bitmap glyphs.
        if pixel_size <= 0 {
            return;
        }
        match sizes_seen.binary_search(&pixel_size) {
            Ok(_) => {}
            Err(pos) => sizes_seen.insert(pos, pixel_size),
        }
    };

    recurse_elem_including_sub_components(component, &(), &mut |elem, _| match elem
        .borrow()
        .base_type
        .to_string()
        .as_str()
    {
        "TextInput" | "Text" | "SimpleText" | "ComplexText" | "StyledTextItem" => {
            if let Some(font_size) = try_extract_literal_from_element(elem, "font-size", Unit::Px) {
                add_font_size(font_size)
            }
        }
        "Dialog" | "Window" | "WindowItem" => {
            if let Some(font_size) =
                try_extract_literal_from_element(elem, "default-font-size", Unit::Px)
            {
                add_font_size(font_size)
            }
        }
        _ => {}
    });
}

pub fn collect_font_weights_used(component: &Rc<Component>, weights_seen: &mut Vec<u16>) {
    let mut add_weight = |weight: f64| {
        let weight = weight as u16;
        if let Err(pos) = weights_seen.binary_search(&weight) {
            weights_seen.insert(pos, weight);
        }
    };

    recurse_elem_including_sub_components(component, &(), &mut |elem, _| match elem
        .borrow()
        .base_type
        .to_string()
        .as_str()
    {
        "TextInput" | "Text" | "SimpleText" | "ComplexText" | "StyledTextItem" => {
            if let Some(weight) = try_extract_literal_from_element(elem, "font-weight", Unit::None)
            {
                add_weight(weight)
            }
        }
        "Dialog" | "Window" | "WindowItem" => {
            if let Some(weight) =
                try_extract_literal_from_element(elem, "default-font-weight", Unit::None)
            {
                add_weight(weight)
            }
        }
        _ => {}
    });
}

pub fn scan_string_literals(component: &Rc<Component>, characters_seen: &mut HashSet<char>) {
    visit_all_expressions(component, |expr, _| {
        expr.visit_recursive(&mut |expr| {
            if let Expression::StringLiteral(string) = expr {
                characters_seen.extend(string.chars());
            }
        })
    })
}
