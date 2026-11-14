//! Construction from the compiler's immutable, host-built catalog.

use crate::{JSDocParsingMode, ParseOptions};
use std::sync::OnceLock;
use tsc_diagnostics::{JsStr, SharedText, TextSnapshot};

/// The descriptor is compiled for the target; its sections are portable
/// little-endian bytes. Text is the same allocation as the source host uses.
pub struct StaticLibraryImage {
    pub name: &'static str,
    pub source: &'static SharedText,
    pub source_fingerprint: u64,
    pub syntax: &'static [u8],
    pub binding: &'static [u8],
    pub edits: &'static [u8],
}

static CATALOG: OnceLock<&'static [StaticLibraryImage]> = OnceLock::new();

/// The compiler registers its catalog when its host first reads a bundled file.
/// Repeated hosts share it; neither registration nor lookup owns a text copy.
pub fn register(catalog: &'static [StaticLibraryImage]) {
    let registered = CATALOG.get_or_init(|| {
        assert!(catalog.windows(2).all(|pair| pair[0].name < pair[1].name));
        catalog
    });
    assert!(
        std::ptr::eq(*registered, catalog),
        "different embedded library catalogs"
    );
}

/// Eligibility is a property of the selected input, not of the host's
/// filename alone. Ordinary parser APIs never consult this registry.
pub fn lookup(
    file_name: JsStr<'_>,
    snapshot: &TextSnapshot,
    options: &ParseOptions,
) -> Option<&'static StaticLibraryImage> {
    lookup_in(CATALOG.get().copied()?, file_name, snapshot, options)
}

fn lookup_in(
    catalog: &'static [StaticLibraryImage],
    file_name: JsStr<'_>,
    snapshot: &TextSnapshot,
    options: &ParseOptions,
) -> Option<&'static StaticLibraryImage> {
    let supported = ParseOptions {
        js_doc_parsing_mode: JSDocParsingMode::ParseForTypeErrors,
        node_id_base: options.node_id_base,
        node_array_id_base: options.node_array_id_base,
        ..ParseOptions::default()
    };
    if options != &supported {
        return None;
    }
    let name = file_name.as_bytes().strip_prefix(b"bundled:///libs/")?;
    let index = catalog
        .binary_search_by(|entry| entry.name.as_bytes().cmp(name))
        .ok()?;
    let image = &catalog[index];
    let text = snapshot.text();
    let original = image.source.as_str();
    (text.len() == original.len() && (text.as_ptr() == original.as_ptr() || text == original))
        .then_some(image)
}

use super::{mode, schema, Decoder, ImageError, Result, Sections, MAGIC, VERSION};
use crate::nodes::{NodeArrayRecord, MAX_IMAGE_PAYLOAD_WORDS};
use crate::{
    ExternalModuleIndicatorOptions, FileReference, JsString, LanguageVariant, Node, NodeArena,
    NodeId, ScriptKind, SourceFile,
};
use std::sync::Arc;
use tsc_store::texts::TextWord;
use tsc_types::EscapedName;

/// The complete request-bearing facts of the supported declaration image.
/// Only successful validation of a compiler-derived frame produces this.
#[derive(Clone, Debug)]
pub struct LibraryMetadata {
    pub is_external_module: bool,
    pub references: Vec<FileReference>,
    root: u32,
    external: u32,
    identifiers: u32,
    symbol_bound: u32,
    parse_node_count: Option<u32>,
    factory_adjustment: i32,
}

struct Opened {
    payload: &'static [u8],
    input: Decoder<'static>,
    sections: Sections,
    metadata: LibraryMetadata,
}

impl StaticLibraryImage {
    fn open(
        &self,
        file_name: JsStr<'_>,
        snapshot: &TextSnapshot,
        options: &ParseOptions,
    ) -> Result<Opened> {
        let supported = ParseOptions {
            js_doc_parsing_mode: JSDocParsingMode::ParseForTypeErrors,
            node_id_base: options.node_id_base,
            node_array_id_base: options.node_array_id_base,
            ..ParseOptions::default()
        };
        if options != &supported
            || file_name.as_bytes().strip_prefix(b"bundled:///libs/") != Some(self.name.as_bytes())
            || snapshot.text().len() != self.source.len()
            || (snapshot.text().as_ptr() != self.source.as_str().as_ptr()
                && snapshot.text() != self.source.as_str())
        {
            return Err(ImageError("library image input differs"));
        }
        let mut input = Decoder(self.syntax);
        if input.take(8)? != MAGIC || input.word()? != VERSION {
            return Err(ImageError("unknown library image format"));
        }
        let length = input.word()? as usize;
        if input.wide()? != schema() {
            return Err(ImageError("stale library image provenance"));
        }
        // The compiler's ordinary host/target proof covers the checksum of
        // these exact static sections. Structural checks still precede use.
        let _checksum = input.wide()?;
        if input.0.len() != length {
            return Err(ImageError("invalid library image length"));
        }
        let payload = input.0;
        let sections = Sections::read(payload)?;
        input.take(Sections::BYTES)?;
        if input.bytes()? != file_name.as_bytes()
            || input.word()? as usize != snapshot.text().len()
            || input.wide()? != self.source_fingerprint
            || mode(input.word()?)? != JSDocParsingMode::ParseForTypeErrors
        {
            return Err(ImageError("library image metadata differs"));
        }
        let root = input.word()?;
        let external = input.word()?;
        if root == 0
            || root as usize > sections.nodes.count
            || external as usize > sections.nodes.count
        {
            return Err(ImageError("invalid image source identity"));
        }
        let identifiers = input.word()?;
        let symbol_bound = input.word()?;
        let count = input.word()?;
        let parse_node_count = (count != u32::MAX).then_some(count);
        let factory_adjustment = input.word()? as i32;
        let count = input.count(16)?;
        let mut references = Vec::with_capacity(count);
        for _ in 0..count {
            let name = JsStr::try_from_canonical_bytes(input.bytes()?)
                .ok_or(ImageError("invalid image reference text"))?;
            let pos = input.word()?;
            let end = input.word()?;
            let preserve = input.word()?;
            if preserve > 1 || pos > end || end as usize > snapshot.text().len() {
                return Err(ImageError("invalid image reference range"));
            }
            references.push(FileReference {
                file_name: name.to_owned(),
                pos,
                end,
                preserve: preserve != 0,
            });
        }
        Ok(Opened {
            payload,
            input,
            sections,
            metadata: LibraryMetadata {
                is_external_module: external != 0,
                references,
                root,
                external,
                identifiers,
                symbol_bound,
                parse_node_count,
                factory_adjustment,
            },
        })
    }

    pub fn metadata(
        &self,
        file_name: JsStr<'_>,
        snapshot: &TextSnapshot,
        options: &ParseOptions,
    ) -> Result<LibraryMetadata> {
        Ok(self.open(file_name, snapshot, options)?.metadata)
    }

    /// Construct into the arena installed by the ordinary placement boundary.
    /// All references are validated while applying their final identity bases.
    pub fn restore(
        &self,
        file_name: JsString,
        snapshot: Arc<TextSnapshot>,
        options: &ParseOptions,
    ) -> Result<SourceFile> {
        let Opened {
            payload,
            mut input,
            sections,
            metadata,
        } = self.open(file_name.as_js(), &snapshot, options)?;
        let count = input.count(4)?;
        tsc_types::names::reserve_capacity_hint(count);
        let mut names = Vec::with_capacity(count);
        for _ in 0..count {
            let name = JsStr::try_from_canonical_bytes(input.bytes()?)
                .ok_or(ImageError("invalid image name text"))?;
            names.push(EscapedName::from_escaped_text(name).index());
        }
        sections.at(&input, sections.nodes)?;
        let literal_starts = literal_starts(payload, &sections)?;
        let mut arena = if options.node_id_base == 0 && options.node_array_id_base == 0 {
            NodeArena::new()
        } else {
            NodeArena::with_bases(options.node_id_base, options.node_array_id_base)
        };
        arena.nodes.reserve(sections.nodes.count);
        arena.arrays.reserve(sections.arrays.count);
        arena.array_items.reserve(sections.items.count);
        arena.texts.reserve(sections.texts.count);
        let node_base = arena.node_base;
        let array_base = arena.array_base;
        let text_base = arena.text_base;
        let item_base = arena.item_base;
        let map = |space, word: u32| -> Result<u32> {
            let (base, length) = match space {
                1 => (node_base, sections.nodes.count),
                2 => (array_base, sections.arrays.count),
                3 if word == u32::MAX => return Ok(word),
                3 => (text_base, sections.texts.count),
                4 => {
                    return names
                        .get(word as usize)
                        .copied()
                        .ok_or(ImageError("invalid image name reference"));
                }
                _ => return Ok(word),
            };
            if word == 0 {
                return Ok(0);
            }
            if word as usize > length
                || (space == 3
                    && literal_starts[(word - 1) as usize / 64] & (1 << ((word - 1) % 64)) == 0)
            {
                return Err(ImageError("image reference outside its record space"));
            }
            base.checked_add(word)
                .ok_or(ImageError("image identity exceeds u32"))
        };
        if metadata.root == 0 || metadata.root as usize > sections.nodes.count {
            return Err(ImageError("absent image source root"));
        }
        let mut nodes = Decoder(&payload[sections.nodes.offset + 4..sections.arrays.offset]);
        for index in 0..sections.nodes.count {
            let header = nodes.take(24)?;
            let packed = record_word(header, 0);
            let kind = crate::SyntaxKind::from_u16(packed as u16)
                .ok_or(ImageError("invalid image node kind"))?;
            let flags = record_word(header, 4) as i32;
            let pos = record_word(header, 8);
            let end = record_word(header, 12);
            let parent = map(1, record_word(header, 16))?;
            let js_doc = map(2, record_word(header, 20))?;
            let layout = Node::image_layout(kind);
            let count = layout.count();
            let has_comment = layout.has_comment();
            if count > MAX_IMAGE_PAYLOAD_WORDS || (packed >> 31 == 0 && count != 0) {
                return Err(ImageError("invalid image node shape"));
            }
            let mut slots = [0; MAX_IMAGE_PAYLOAD_WORDS];
            let payload = nodes.take(count * 4)?;
            for (slot, (word, bytes)) in slots[..count]
                .iter_mut()
                .zip(payload.as_chunks::<4>().0)
                .enumerate()
            {
                *word = map(layout.space(slot), record_word(bytes, 0))?;
            }
            let mut comment = [0; 4];
            if has_comment {
                let bytes = nodes.take(16)?;
                for (word, bytes) in comment.iter_mut().zip(bytes.as_chunks::<4>().0) {
                    *word = record_word(bytes, 0);
                }
                if comment[0] != 0 {
                    comment[0] = map(3, comment[0])?;
                } else {
                    comment[1] = map(2, comment[1])?;
                }
            }
            if index + 1 == metadata.root as usize && kind != crate::SyntaxKind::SourceFile {
                return Err(ImageError("invalid image source root"));
            }
            let mut node = Node::from_image_layout(
                layout,
                &slots[..count],
                comment,
                js_doc,
                packed >> 31 != 0,
                &mut arena.tails,
            )
            .ok_or(ImageError("invalid image node payload"))?;
            node.flags = flags;
            node.pos = pos;
            node.end = end;
            node.parent = NodeId::from_position(parent);
            node.set_literal_flags(((packed >> 16) & 0x7fff) as u16);
            arena.nodes.push(node);
        }
        nodes.end()?;
        let mut arrays = Decoder(&payload[sections.arrays.offset + 4..sections.items.offset]);
        for _ in 0..sections.arrays.count {
            let header = arrays.take(20)?;
            let offset = record_word(header, 0);
            let len = record_word(header, 4);
            let pos = record_word(header, 8);
            let end = record_word(header, 12);
            let packed = record_word(header, 16);
            if offset
                .checked_add(len)
                .is_none_or(|end| end as usize > sections.items.count)
                || packed >> 2 > u16::MAX as u32
            {
                return Err(ImageError("invalid image array"));
            }
            arena.arrays.push(NodeArrayRecord {
                items: item_base
                    .checked_add(offset)
                    .ok_or(ImageError("image item identity exceeds u32"))?,
                len,
                pos,
                end,
                has_trailing_comma: packed & 1 != 0,
                is_missing_list: packed & 2 != 0,
                modifier_flags: (packed >> 2) as u16,
            });
        }
        arrays.end()?;
        let mut items = Decoder(&payload[sections.items.offset + 4..sections.texts.offset]);
        for _ in 0..sections.items.count {
            arena.array_items.push(
                NodeId::from_position(map(1, items.word()?)?)
                    .ok_or(ImageError("absent image array item"))?,
            );
        }
        items.end()?;
        let mut texts = Decoder(&payload[sections.texts.offset + 4..]);
        for _ in 0..sections.texts.count {
            arena.texts.push(TextWord(texts.word()?));
        }
        texts.end()?;
        arena.finish_record_installation();
        arena.identifier_count = metadata.identifiers;
        arena.symbol_bound = metadata.symbol_bound;
        arena.parse_node_count = metadata.parse_node_count.map(|n| n as usize);
        arena.factory_node_adjustment = metadata.factory_adjustment as isize;
        let root = NodeId::from_position(map(1, metadata.root)?)
            .ok_or(ImageError("absent image source root"))?;
        let external = NodeId::from_position(map(1, metadata.external)?);
        let mut source = SourceFile::from_tree_snapshot(
            file_name,
            snapshot,
            arena,
            root,
            LanguageVariant::Standard,
            ScriptKind::Ts,
            true,
            ExternalModuleIndicatorOptions::default(),
        );
        source.js_doc_parsing_mode = JSDocParsingMode::ParseForTypeErrors;
        source.external_module_indicator = external;
        source.lib_reference_directives = metadata.references;
        Ok(source)
    }
}

#[inline(always)]
fn record_word(record: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        record[offset..offset + 4]
            .try_into()
            .expect("complete image word"),
    )
}

fn literal_starts(payload: &[u8], sections: &Sections) -> Result<Vec<u64>> {
    let bytes = &payload[sections.texts.offset + 4..];
    let mut starts = vec![0u64; sections.texts.count.div_ceil(64)];
    let mut offset = 0;
    while offset < sections.texts.count {
        let header =
            u32::from_le_bytes(bytes[offset * 4..offset * 4 + 4].try_into().expect("word"));
        let length = (header & 0x7fff_ffff) as usize;
        let words = length.div_ceil(4);
        let end = offset
            .checked_add(1)
            .and_then(|n| n.checked_add(words))
            .filter(|&end| end <= sections.texts.count)
            .ok_or(ImageError("invalid image literal length"))?;
        let value = &bytes[(offset + 1) * 4..(offset + 1) * 4 + length];
        if JsStr::try_from_canonical_bytes(value).is_none()
            || (header >> 31 == 0) != std::str::from_utf8(value).is_ok()
        {
            return Err(ImageError("invalid image literal text"));
        }
        starts[offset / 64] |= 1 << (offset % 64);
        offset = end;
    }
    Ok(starts)
}

impl StaticLibraryImage {
    pub fn counts(
        &self,
        file_name: JsStr<'_>,
        snapshot: &TextSnapshot,
        options: &ParseOptions,
    ) -> Result<[u32; 4]> {
        let opened = self.open(file_name, snapshot, options)?;
        Ok([
            opened.sections.nodes.count,
            opened.sections.arrays.count,
            opened.sections.items.count,
            opened.sections.texts.count,
        ]
        .map(|n| u32::try_from(n).expect("wire count")))
    }

    pub fn restore_in_identity_domain(
        &self,
        file_name: JsString,
        snapshot: Arc<TextSnapshot>,
        options: &ParseOptions,
        domain: &tsc_types::IdentityDomain,
    ) -> std::result::Result<SourceFile, tsc_types::IdentityError> {
        let counts = self
            .counts(file_name.as_js(), &snapshot, options)
            .expect("compiler library image metadata must match its code and source");
        let leases = crate::placement::lease_counts(domain, counts)?;
        Ok(crate::placement::construct_at_leases(leases, || {
            self.restore(file_name, snapshot, options)
                .expect("compiler library image records must match the ordinary front end")
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library_image::{fingerprint, SyntaxImage};
    use tsc_diagnostics::DocumentVersion;

    fn fixture() -> (SyntaxImage, Arc<TextSnapshot>, ParseOptions) {
        let options = ParseOptions {
            js_doc_parsing_mode: JSDocParsingMode::ParseForTypeErrors,
            ..Default::default()
        };
        let snapshot = TextSnapshot::new(
            "export interface A { x: '\\ud800'; f<T>(x: T[]): T }",
            DocumentVersion::new("retained-version"),
        );
        let source = crate::parse_source_file_from_snapshot(
            "bundled:///libs/lib.fixture.d.ts",
            Arc::clone(&snapshot),
            options.clone(),
            None,
        );
        (SyntaxImage::capture(&source).unwrap(), snapshot, options)
    }

    fn descriptor(image: &SyntaxImage, snapshot: &TextSnapshot) -> StaticLibraryImage {
        StaticLibraryImage {
            name: "lib.fixture.d.ts",
            source: Box::leak(Box::new(snapshot.shared_text())),
            source_fingerprint: fingerprint(snapshot.text().as_bytes()),
            syntax: Box::leak(image.encode().unwrap().into_boxed_slice()),
            binding: &[],
            edits: &[],
        }
    }

    #[test]
    fn image_record_ends_follow_rows_before_an_ordinary_file() {
        let (image, snapshot, mut options) = fixture();
        options.node_id_base = 701;
        options.node_array_id_base = 911;
        let restored = descriptor(&image, &snapshot)
            .restore(
                "bundled:///libs/lib.fixture.d.ts".into(),
                snapshot,
                &options,
            )
            .unwrap();
        assert_eq!(restored.arena.node_base(), 701);
        assert_eq!(restored.arena.array_base(), 911);
        assert_eq!(restored.arena.node_end(), 701 + image.nodes.len() as u32);
        assert_eq!(restored.arena.array_end(), 911 + image.arrays.len() as u32);
        let following = crate::parse_source_file(
            "ordinary.ts",
            "export const next = [1];",
            ParseOptions {
                node_id_base: restored.arena.node_end(),
                node_array_id_base: restored.arena.array_end(),
                ..Default::default()
            },
            None,
        );
        assert_eq!(following.arena.node_base(), restored.arena.node_end());
        assert_eq!(following.arena.array_base(), restored.arena.array_end());
        assert!(following.arena.node_end() > restored.arena.node_end());
        assert!(following.arena.array_end() > restored.arena.array_end());
    }

    #[test]
    fn literal_boundaries_keep_their_bits_across_word_boundaries() {
        let (mut image, _, _) = fixture();
        image.texts.clear();
        for _ in 0..70 {
            image.texts.extend([
                5,
                u32::from_le_bytes(*b"aaaa"),
                u32::from_le_bytes(*b"a\0\0\0"),
            ]);
        }
        let bytes = image.encode().unwrap();
        let payload = &bytes[32..];
        let sections = Sections::read(payload).unwrap();
        let starts = literal_starts(payload, &sections).unwrap();
        for word in 0..image.texts.len() {
            assert_eq!(starts[word / 64] & (1 << (word % 64)) != 0, word % 3 == 0);
        }
    }

    #[test]
    fn unused_and_oversized_dictionaries_keep_the_tree() {
        let (expected, snapshot, options) = fixture();
        for count in [128, 129, 1024, 1025, 8192, 8193] {
            let mut image = expected.clone();
            for index in image.names.len()..count {
                image.names.push(format!("unused{index}").into_bytes());
            }
            let actual = descriptor(&image, &snapshot)
                .restore(
                    "bundled:///libs/lib.fixture.d.ts".into(),
                    Arc::clone(&snapshot),
                    &options,
                )
                .unwrap();
            assert_eq!(SyntaxImage::capture(&actual).unwrap(), expected);
        }
    }

    #[test]
    fn image_layout_keeps_every_kind_shape_and_reference_space() {
        for value in 0..=u16::MAX {
            let Some(kind) = crate::SyntaxKind::from_u16(value) else {
                continue;
            };
            let layout = Node::image_layout(kind);
            let (count, has_comment) = crate::nodes::payload_shape(kind);
            assert_eq!(layout.count(), count, "{kind:?}");
            assert_eq!(layout.has_comment(), has_comment, "{kind:?}");
            for slot in 0..count {
                assert_eq!(
                    layout.space(slot),
                    Node::image_slot_space(kind, slot),
                    "{kind:?} slot {slot}"
                );
            }
        }
    }

    #[test]
    fn direct_construction_validates_references_before_publishing_them() {
        let (image, snapshot, options) = fixture();
        let name: JsString = "bundled:///libs/lib.fixture.d.ts".into();
        let image_entry = descriptor(&image, &snapshot);
        let actual = image_entry
            .restore(name.clone(), Arc::clone(&snapshot), &options)
            .unwrap();
        assert!(Arc::ptr_eq(actual.snapshot(), &snapshot));
        assert_eq!(SyntaxImage::capture(&actual).unwrap(), image);

        let mut malformed = image.clone();
        malformed.nodes[0].parent = u32::MAX;
        assert!(descriptor(&malformed, &snapshot)
            .restore(name.clone(), Arc::clone(&snapshot), &options)
            .is_err());
        let mut malformed = image.clone();
        malformed.texts[0] = 0x7fff_ffff;
        assert!(descriptor(&malformed, &snapshot)
            .restore(name.clone(), Arc::clone(&snapshot), &options)
            .is_err());
        let mut offset = 0;
        while offset < image.texts.len() {
            let mut malformed = image.clone();
            malformed.texts[offset] ^= 1 << 31;
            assert!(descriptor(&malformed, &snapshot)
                .restore(name.clone(), Arc::clone(&snapshot), &options)
                .is_err());
            offset += 1 + ((image.texts[offset] & 0x7fff_ffff) as usize).div_ceil(4);
        }
        let mut malformed = image;
        malformed.root = u32::MAX;
        assert!(descriptor(&malformed, &snapshot)
            .metadata(name.as_js(), &snapshot, &options)
            .is_err());

        let mut bytes = image_entry.syntax.to_vec();
        bytes[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
        let malformed = StaticLibraryImage {
            syntax: Box::leak(bytes.into_boxed_slice()),
            ..image_entry
        };
        assert!(malformed.counts(name.as_js(), &snapshot, &options).is_err());
    }

    #[test]
    fn catalog_eligibility_requires_exact_text_name_and_parse_options() {
        let (image, snapshot, options) = fixture();
        let catalog = Box::leak(Box::new([descriptor(&image, &snapshot)]));
        let name: JsStr<'_> = "bundled:///libs/lib.fixture.d.ts".into();
        assert!(lookup_in(catalog, name, &snapshot, &options).is_some());
        let separately_owned =
            TextSnapshot::new(snapshot.text(), DocumentVersion::new("another-version"));
        assert!(lookup_in(catalog, name, &separately_owned, &options).is_some());
        for mode in [
            JSDocParsingMode::ParseAll,
            JSDocParsingMode::ParseNone,
            JSDocParsingMode::ParseForTypeInfo,
        ] {
            let changed = ParseOptions {
                js_doc_parsing_mode: mode,
                ..options.clone()
            };
            assert!(lookup_in(catalog, name, &snapshot, &changed).is_none());
        }
        let changed = TextSnapshot::new("export interface A {}", DocumentVersion::default());
        assert!(lookup_in(catalog, name, &changed, &options).is_none());
        assert!(lookup_in(catalog, "/p/lib.fixture.d.ts".into(), &snapshot, &options).is_none());
        let changed = ParseOptions {
            force_external_module: true,
            ..options
        };
        assert!(lookup_in(catalog, name, &snapshot, &changed).is_none());
    }
}
