// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

pub use fontique;
pub use skrifa;

#[cfg(feature = "svg-text")]
pub mod svg;

#[cfg(any(target_family = "wasm", target_os = "nto"))]
use fontique::ScriptExt;

use std::sync::Arc;

/// Create a new fontique Collection.
/// When `shared` is true, the collection uses `Arc`-based internal sharing,
/// so that clones share the underlying data and mutations are visible across clones.
pub fn create_collection(shared: bool) -> Collection {
    // Never enumerate host fonts here: on Windows that selects the DirectWrite
    // backend, which is not a valid dependency for the Win7 distribution.
    #[allow(unused_mut)]
    let mut collection =
        fontique::Collection::new(fontique::CollectionOptions { shared, system_fonts: false });
    let source_cache =
        if shared { fontique::SourceCache::new_shared() } else { fontique::SourceCache::default() };

    // Keep environment-supplied fonts out of the standalone build. The
    // application registers its two private TTFs from `include_bytes!` after
    // the Slint platform is initialized, so inherited environment variables
    // cannot inject a host file into the collection.
    let default_fonts: Vec<(std::path::PathBuf, fontique::QueryFont)> = Vec::new();

    #[cfg(any(target_family = "wasm", target_os = "nto"))]
    {
        let data = include_bytes!("sharedfontique/Inter-VariableFont.ttf");
        let fonts = collection.register_fonts(fontique::Blob::new(Arc::new(data)), None);
        for script in fontique::Script::all_samples().iter().map(|(script, _)| *script) {
            collection.append_fallbacks(
                fontique::FallbackKey::new(script, None),
                fonts.iter().map(|(family_id, _)| *family_id),
            );
        }
        for generic_family in [
            fontique::GenericFamily::SansSerif,
            fontique::GenericFamily::SystemUi,
            fontique::GenericFamily::UiSansSerif,
        ] {
            collection.append_generic_families(
                generic_family,
                fonts.iter().map(|(family_id, _)| *family_id),
            );
        }
    }

    Collection { inner: collection, source_cache, default_fonts: Arc::new(default_fonts) }
}

#[derive(Clone)]
pub struct Collection {
    pub inner: fontique::Collection,
    pub source_cache: fontique::SourceCache,
    pub default_fonts: Arc<Vec<(std::path::PathBuf, fontique::QueryFont)>>,
}

impl Collection {
    pub fn query<'a>(&'a mut self) -> fontique::Query<'a> {
        self.inner.query(&mut self.source_cache)
    }

    pub fn get_font_for_info(
        &mut self,
        family_id: fontique::FamilyId,
        info: &fontique::FontInfo,
    ) -> Option<fontique::QueryFont> {
        get_font_for_info(&mut self.inner, &mut self.source_cache, family_id, info)
    }
}

fn get_font_for_info(
    collection: &mut fontique::Collection,
    source_cache: &mut fontique::SourceCache,
    family_id: fontique::FamilyId,
    info: &fontique::FontInfo,
) -> Option<fontique::QueryFont> {
    let mut query = collection.query(source_cache);
    query.set_families(std::iter::once(fontique::QueryFamily::from(family_id)));
    query.set_attributes(fontique::Attributes {
        weight: info.weight(),
        style: info.style(),
        width: info.width(),
    });
    let mut font = None;
    query.matches_with(|queried_font| {
        font = Some(queried_font.clone());
        fontique::QueryStatus::Stop
    });
    font
}

impl std::ops::Deref for Collection {
    type Target = fontique::Collection;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::DerefMut for Collection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

pub const FALLBACK_FAMILIES: [fontique::GenericFamily; 2] = [
    // FemtoVG renderer needs SansSerif first, as it has difficulties rendering from SystemUi on macOS
    fontique::GenericFamily::SansSerif,
    fontique::GenericFamily::SystemUi,
];

/// Wrapper around fontique::Blob to permit use of the blob as a key in the cache in the different renderers,
/// to map the blob to the native type face representation (skia_safe::Typeface, femtovg::FontId, QRawFont, etc.).
/// The use as key also ensures the blob remains strongly referenced, so that it doesn't vanish from the
/// shared SourceCache (parley prunes it).
#[derive(Clone)]
pub struct HashedBlob(fontique::Blob<u8>);
impl core::hash::Hash for HashedBlob {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.0.id().hash(state);
    }
}

impl PartialEq for HashedBlob {
    fn eq(&self, other: &Self) -> bool {
        self.0.id() == other.0.id()
    }
}

impl Eq for HashedBlob {}

impl From<fontique::Blob<u8>> for HashedBlob {
    fn from(value: fontique::Blob<u8>) -> Self {
        Self(value)
    }
}

impl AsRef<fontique::Blob<u8>> for HashedBlob {
    fn as_ref(&self) -> &fontique::Blob<u8> {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    // cspell:ignore fonttools varLib instancer opsz pyftsubset unicodes
    use skrifa::MetadataProvider;

    // Keep the embedded font small. Regenerate it with:
    //   fonttools varLib.instancer -o pinned.ttf Inter-VariableFont.ttf opsz=14
    //   pyftsubset pinned.ttf --unicodes="U+0000-DFFF,U+F900-10FFFF" --output-file=Inter-VariableFont.ttf
    #[test]
    fn embedded_fallback_font_is_minimal() {
        let data = include_bytes!("sharedfontique/Inter-VariableFont.ttf");
        let font = skrifa::FontRef::new(data).unwrap();

        let has_pua = font
            .charmap()
            .mappings()
            .any(|(cp, _)| matches!(cp, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD));
        assert!(!has_pua, "the embedded font maps Private Use Area codepoints; regenerate it");

        assert!(
            font.axes().iter().all(|axis| axis.tag() != "opsz"),
            "the embedded font still has an optical-size axis; pin it"
        );
    }
}
