//! Portable, file-relative syntax records for generated library data.
//!
//! The builder and command loader use the same portable record format.
//! Rust object layouts, tail addresses, leases and hash buckets are never
//! written to the image. Restoration makes ordinary owned syntax objects.

use super::*;
use crate::{ExternalModuleIndicatorOptions, FileReference, JSDocParsingMode, SourceFile};
use std::fmt;
use tsc_types::{EscapedName, JsStr};

const MAGIC: &[u8; 8] = b"TSCLIBS2";
const VERSION: u32 = 2;

#[path = "library_image/runtime.rs"]
mod runtime;
pub use runtime::{lookup, register, FreshLibrarySource, LibraryMetadata, StaticLibraryImage};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageError(pub &'static str);

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for ImageError {}

type Result<T> = std::result::Result<T, ImageError>;

/// A deterministic checksum for derived data, not a content-address security
/// boundary. Exact source equality is checked separately by the consumer.
pub fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

fn schema() -> u64 {
    include!(concat!(env!("OUT_DIR"), "/library_image_schema.rs"))
}

/// Shared little-endian framing used by the syntax and binder builders.
#[derive(Default)]
pub struct Encoder(Vec<u8>);

impl Encoder {
    pub fn word(&mut self, value: u32) {
        self.0.extend(value.to_le_bytes());
    }
    pub fn wide(&mut self, value: u64) {
        self.0.extend(value.to_le_bytes());
    }
    pub fn count(&mut self, value: usize) -> Result<()> {
        self.word(u32::try_from(value).map_err(|_| ImageError("image count exceeds u32"))?);
        Ok(())
    }
    pub fn bytes(&mut self, value: &[u8]) -> Result<()> {
        self.count(value.len())?;
        self.0.extend(value);
        Ok(())
    }
    pub fn words(&mut self, values: &[u32]) -> Result<()> {
        self.count(values.len())?;
        for &value in values {
            self.word(value);
        }
        Ok(())
    }
    pub fn finish(self, magic: &[u8; 8], provenance: u64) -> Result<Vec<u8>> {
        let mut output = Vec::with_capacity(self.0.len() + 32);
        output.extend(magic);
        output.extend(VERSION.to_le_bytes());
        output.extend(
            u32::try_from(self.0.len())
                .map_err(|_| ImageError("image exceeds u32"))?
                .to_le_bytes(),
        );
        output.extend(provenance.to_le_bytes());
        output.extend(fingerprint(&self.0).to_le_bytes());
        output.extend(self.0);
        Ok(output)
    }
}

pub struct Decoder<'a>(&'a [u8]);

impl<'a> Decoder<'a> {
    pub fn open(bytes: &'a [u8], magic: &[u8; 8], provenance: u64) -> Result<Self> {
        let (input, checksum) = Self::header(bytes, magic, provenance)?;
        if fingerprint(input.0) != checksum {
            return Err(ImageError("invalid library image length or checksum"));
        }
        Ok(input)
    }

    /// Compiled immutable frames have their contents proved against the
    /// ordinary frontend. Keep structural checks without hashing them again.
    pub fn open_static(bytes: &'static [u8], magic: &[u8; 8], provenance: u64) -> Result<Self> {
        let (input, _) = Self::header(bytes, magic, provenance)?;
        Ok(input)
    }

    fn header(bytes: &'a [u8], magic: &[u8; 8], provenance: u64) -> Result<(Self, u64)> {
        let mut input = Self(bytes);
        if input.take(8)? != magic || input.word()? != VERSION {
            return Err(ImageError("unknown library image format"));
        }
        let length = input.word()? as usize;
        if input.wide()? != provenance {
            return Err(ImageError("stale library image provenance"));
        }
        let checksum = input.wide()?;
        if input.0.len() != length {
            return Err(ImageError("invalid library image length or checksum"));
        }
        Ok((input, checksum))
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let (head, tail) = self
            .0
            .split_at_checked(count)
            .ok_or(ImageError("truncated library image"))?;
        self.0 = tail;
        Ok(head)
    }
    pub fn word(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }
    pub fn wide(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }
    pub fn count(&mut self, minimum_bytes: usize) -> Result<usize> {
        let count = self.word()? as usize;
        if minimum_bytes == 0 || count > self.0.len() / minimum_bytes {
            return Err(ImageError("invalid library image count"));
        }
        Ok(count)
    }
    pub fn bytes(&mut self) -> Result<&'a [u8]> {
        let count = self.count(1)?;
        self.take(count)
    }
    pub fn words(&mut self) -> Result<Vec<u32>> {
        let count = self.count(4)?;
        (0..count).map(|_| self.word()).collect()
    }
    pub fn end(self) -> Result<()> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(ImageError("trailing library image data"))
        }
    }
}

#[derive(Clone, Copy)]
struct Section {
    offset: usize,
    count: usize,
}

/// Offsets name the count words, relative to the framed payload. Counts
/// precede construction so every published reference can be range-checked.
struct Sections {
    nodes: Section,
    arrays: Section,
    items: Section,
    texts: Section,
    payload_len: usize,
}

impl Sections {
    const BYTES: usize = 8 * 4;

    fn read(payload: &[u8]) -> Result<Self> {
        let mut input = Decoder(payload);
        let mut read = || -> Result<Section> {
            Ok(Section {
                offset: input.word()? as usize,
                count: input.word()? as usize,
            })
        };
        let sections = Self {
            nodes: read()?,
            arrays: read()?,
            items: read()?,
            texts: read()?,
            payload_len: payload.len(),
        };
        if sections.nodes.offset < Self::BYTES {
            return Err(ImageError("invalid image metadata section"));
        }
        for section in [
            sections.nodes,
            sections.arrays,
            sections.items,
            sections.texts,
        ] {
            let mut field = Decoder(
                payload
                    .get(section.offset..)
                    .ok_or(ImageError("image section outside payload"))?,
            );
            if field.word()? as usize != section.count {
                return Err(ImageError("image section count differs from directory"));
            }
        }
        let end = |section: Section, width: usize| {
            section
                .count
                .checked_mul(width)?
                .checked_add(4)?
                .checked_add(section.offset)
        };
        if end(sections.nodes, 24).is_none_or(|n| n > sections.arrays.offset)
            || end(sections.arrays, 20) != Some(sections.items.offset)
            || end(sections.items, 4) != Some(sections.texts.offset)
            || end(sections.texts, 4) != Some(payload.len())
        {
            return Err(ImageError("invalid image section layout"));
        }
        Ok(sections)
    }

    fn at(&self, input: &Decoder<'_>, section: Section) -> Result<()> {
        if input.0.len() == self.payload_len - section.offset {
            Ok(())
        } else {
            Err(ImageError("image section boundary differs from directory"))
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ImageNode {
    kind: SyntaxKind,
    literal_flags: u16,
    flags: i32,
    pos: u32,
    end: u32,
    parent: u32,
    js_doc: u32,
    payload: bool,
    slots: Vec<u32>,
    comment: [u32; 4],
}

/// A supported bundled declaration's complete parsed tree. File metadata
/// outside this closed domain is rejected, rather than silently discarded.
#[derive(Clone, Debug, PartialEq)]
pub struct SyntaxImage {
    file_name: String,
    text: String,
    mode: JSDocParsingMode,
    references: Vec<FileReference>,
    root: u32,
    external_module: u32,
    names: Vec<Vec<u8>>,
    nodes: Vec<ImageNode>,
    arrays: Vec<[u32; 6]>,
    items: Vec<u32>,
    texts: Vec<u32>,
    identifier_count: u32,
    symbol_bound: u32,
    parse_node_count: Option<u32>,
    factory_adjustment: i32,
}

fn local(word: u32, base: u32, length: usize) -> Result<u32> {
    if word == 0 {
        return Ok(0);
    }
    let value = word
        .checked_sub(base)
        .ok_or(ImageError("image reference below file base"))?;
    if value == 0 || value as usize > length {
        return Err(ImageError("image reference outside file"));
    }
    Ok(value)
}

fn mode(word: u32) -> Result<JSDocParsingMode> {
    match word {
        0 => Ok(JSDocParsingMode::ParseAll),
        1 => Ok(JSDocParsingMode::ParseNone),
        2 => Ok(JSDocParsingMode::ParseForTypeErrors),
        3 => Ok(JSDocParsingMode::ParseForTypeInfo),
        _ => Err(ImageError("invalid JSDoc image mode")),
    }
}

fn utf8(bytes: &[u8]) -> Result<String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| ImageError("non-UTF-8 image metadata"))
}

impl SyntaxImage {
    pub fn capture(source: &SourceFile) -> Result<Self> {
        let file_name = source.file_name.to_string_lossy().into_owned();
        let mut supported = SourceFile::from_tree_snapshot(
            file_name.as_str().into(),
            std::sync::Arc::clone(source.snapshot()),
            source.arena.clone(),
            source.root,
            crate::LanguageVariant::Standard,
            crate::ScriptKind::Ts,
            true,
            ExternalModuleIndicatorOptions::default(),
        );
        supported.js_doc_parsing_mode = source.js_doc_parsing_mode;
        supported.lib_reference_directives = source.lib_reference_directives.clone();
        supported.external_module_indicator = source.external_module_indicator;
        if source != &supported || source.arena.copied_subtrees {
            return Err(ImageError("unsupported library source metadata"));
        }
        let arena = &source.arena;
        let mut names = Vec::new();
        let mut indices = rustc_hash::FxHashMap::default();
        let mut map = |space, word| -> Result<u32> {
            match space {
                1 => local(word, arena.node_base, arena.nodes.len()),
                2 => local(word, arena.array_base, arena.arrays.len()),
                3 if word == u32::MAX => Ok(word),
                3 => local(word, arena.text_base, arena.texts.len()),
                4 => Ok(*indices.entry(word).or_insert_with(|| {
                    let index = names.len() as u32;
                    names.push(EscapedName::from_index(word).as_js().as_bytes().to_vec());
                    index
                })),
                _ => Ok(word),
            }
        };
        let mut nodes = Vec::with_capacity(arena.nodes.len());
        for node in &arena.nodes {
            let (mut slots, mut comment, payload) = node.image_payload();
            for (index, word) in slots.iter_mut().enumerate() {
                *word = map(Node::image_slot_space(node.kind, index), *word)?;
            }
            if comment[0] != 0 {
                comment[0] = map(3, comment[0])?;
            } else {
                comment[1] = map(2, comment[1])?;
            }
            nodes.push(ImageNode {
                kind: node.kind,
                literal_flags: node.literal_flags(),
                flags: node.flags,
                pos: node.pos,
                end: node.end,
                parent: map(1, node.parent.map_or(0, NodeId::position))?,
                js_doc: map(2, node.js_doc().map_or(0, NodeArrayId::position))?,
                payload,
                slots,
                comment,
            });
        }
        let arrays = arena
            .arrays
            .iter()
            .map(|a| {
                [
                    a.items - arena.item_base,
                    a.len,
                    a.pos,
                    a.end,
                    u32::from(a.has_trailing_comma) | (u32::from(a.is_missing_list) << 1),
                    u32::from(a.modifier_flags),
                ]
            })
            .collect();
        let items = arena
            .array_items
            .iter()
            .map(|id| local(id.position(), arena.node_base, arena.nodes.len()))
            .collect::<Result<_>>()?;
        Ok(Self {
            file_name,
            text: source.text().to_owned(),
            mode: source.js_doc_parsing_mode,
            references: source.lib_reference_directives.clone(),
            root: local(source.root.position(), arena.node_base, arena.nodes.len())?,
            external_module: local(
                source.external_module_indicator.map_or(0, NodeId::position),
                arena.node_base,
                arena.nodes.len(),
            )?,
            names,
            nodes,
            arrays,
            items,
            texts: arena.texts.iter().map(|w| w.0).collect(),
            identifier_count: arena.identifier_count,
            symbol_bound: arena.symbol_bound,
            parse_node_count: arena
                .parse_node_count
                .map(u32::try_from)
                .transpose()
                .map_err(|_| ImageError("parse count exceeds u32"))?,
            factory_adjustment: i32::try_from(arena.factory_node_adjustment)
                .map_err(|_| ImageError("factory count exceeds i32"))?,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut out = Encoder::default();
        for _ in 0..8 {
            out.word(0);
        }
        out.bytes(self.file_name.as_bytes())?;
        out.count(self.text.len())?;
        out.wide(fingerprint(self.text.as_bytes()));
        out.word(self.mode as u32);
        out.word(self.root);
        out.word(self.external_module);
        out.word(self.identifier_count);
        out.word(self.symbol_bound);
        out.word(self.parse_node_count.unwrap_or(u32::MAX));
        out.word(self.factory_adjustment as u32);
        out.count(self.references.len())?;
        for r in &self.references {
            out.bytes(r.file_name.as_js().as_bytes())?;
            out.word(r.pos);
            out.word(r.end);
            out.word(u32::from(r.preserve));
        }
        out.count(self.names.len())?;
        for name in &self.names {
            out.bytes(name)?;
        }
        let node_offset = out.0.len();
        out.count(self.nodes.len())?;
        for n in &self.nodes {
            for word in [
                n.kind as u32 | (u32::from(n.literal_flags) << 16) | (u32::from(n.payload) << 31),
                n.flags as u32,
                n.pos,
                n.end,
                n.parent,
                n.js_doc,
            ] {
                out.word(word);
            }
            for &word in &n.slots {
                out.word(word);
            }
            if crate::nodes::payload_shape(n.kind).1 {
                for word in n.comment {
                    out.word(word);
                }
            }
        }
        let array_offset = out.0.len();
        out.count(self.arrays.len())?;
        for &[items, len, pos, end, flags, modifiers] in &self.arrays {
            for word in [items, len, pos, end, flags | (modifiers << 2)] {
                out.word(word);
            }
        }
        let item_offset = out.0.len();
        out.words(&self.items)?;
        let text_offset = out.0.len();
        out.words(&self.texts)?;
        for (index, value) in [
            node_offset,
            self.nodes.len(),
            array_offset,
            self.arrays.len(),
            item_offset,
            self.items.len(),
            text_offset,
            self.texts.len(),
        ]
        .into_iter()
        .enumerate()
        {
            out.0[index * 4..index * 4 + 4].copy_from_slice(
                &u32::try_from(value)
                    .map_err(|_| ImageError("image section exceeds u32"))?
                    .to_le_bytes(),
            );
        }
        out.finish(MAGIC, schema())
    }

    /// `text` is the catalog entry's original text, retained separately in
    /// the binary. Eligibility must compare the requested text to that exact
    /// entry; this checksum only detects mismatched derived data.
    pub fn decode(bytes: &[u8], text: &str) -> Result<Self> {
        let mut input = Decoder::open(bytes, MAGIC, schema())?;
        let sections = Sections::read(input.0)?;
        input.take(Sections::BYTES)?;
        let file_name = utf8(input.bytes()?)?;
        if input.word()? as usize != text.len() || input.wide()? != fingerprint(text.as_bytes()) {
            return Err(ImageError("library image source changed"));
        }
        let text = text.to_owned();
        let mode = mode(input.word()?)?;
        let root = input.word()?;
        let external_module = input.word()?;
        let identifier_count = input.word()?;
        let symbol_bound = input.word()?;
        let count = input.word()?;
        let parse_node_count = (count != u32::MAX).then_some(count);
        let factory_adjustment = input.word()? as i32;
        let mut references = Vec::new();
        for _ in 0..input.count(16)? {
            let file_name = utf8(input.bytes()?)?.into();
            let pos = input.word()?;
            let end = input.word()?;
            let preserve = input.word()?;
            if preserve > 1 {
                return Err(ImageError("invalid reference flag"));
            }
            references.push(FileReference {
                file_name,
                pos,
                end,
                preserve: preserve != 0,
            });
        }
        let mut names = Vec::new();
        for _ in 0..input.count(4)? {
            names.push(input.bytes()?.to_vec());
        }
        sections.at(&input, sections.nodes)?;
        let mut nodes = Vec::new();
        for _ in 0..input.count(24)? {
            let packed = input.word()?;
            let kind =
                SyntaxKind::from_u16(packed as u16).ok_or(ImageError("invalid image node kind"))?;
            let literal_flags = ((packed >> 16) & 0x7fff) as u16;
            let flags = input.word()? as i32;
            let pos = input.word()?;
            let end = input.word()?;
            let parent = input.word()?;
            let js_doc = input.word()?;
            let (count, has_comment) = crate::nodes::payload_shape(kind);
            let slots = (0..count).map(|_| input.word()).collect::<Result<_>>()?;
            let mut comment = [0; 4];
            if has_comment {
                for word in &mut comment {
                    *word = input.word()?;
                }
            }
            nodes.push(ImageNode {
                kind,
                literal_flags,
                flags,
                pos,
                end,
                parent,
                js_doc,
                payload: packed >> 31 != 0,
                slots,
                comment,
            });
        }
        sections.at(&input, sections.arrays)?;
        let mut arrays = Vec::new();
        for _ in 0..input.count(20)? {
            let mut array = [0; 6];
            for word in &mut array[..4] {
                *word = input.word()?;
            }
            let flags = input.word()?;
            array[4] = flags & 3;
            array[5] = flags >> 2;
            arrays.push(array);
        }
        sections.at(&input, sections.items)?;
        let items = input.words()?;
        sections.at(&input, sections.texts)?;
        let texts = input.words()?;
        input.end()?;
        let image = Self {
            file_name,
            text,
            mode,
            references,
            root,
            external_module,
            identifier_count,
            symbol_bound,
            parse_node_count,
            factory_adjustment,
            names,
            nodes,
            arrays,
            items,
            texts,
        };
        // Validate all references before exposing a normal tree to callers.
        image.restore()?;
        Ok(image)
    }

    /// Materialize ordinary private objects, without scanning or parsing.
    /// This proof path deliberately does not lease/publish runtime rooms.
    pub fn restore(&self) -> Result<SourceFile> {
        let root_index = self
            .root
            .checked_sub(1)
            .ok_or(ImageError("absent image source root"))? as usize;
        if self.nodes.get(root_index).map(|n| n.kind) != Some(SyntaxKind::SourceFile) {
            return Err(ImageError("invalid image source root"));
        }
        let names: Vec<_> = self
            .names
            .iter()
            .map(|name| {
                let name = JsStr::try_from_canonical_bytes(name)
                    .ok_or(ImageError("invalid image name text"))?;
                Ok(EscapedName::from_escaped_text(name).index())
            })
            .collect::<Result<_>>()?;
        let mut text_starts = vec![false; self.texts.len()];
        let mut offset = 0;
        while offset < self.texts.len() {
            text_starts[offset] = true;
            let header = self.texts[offset];
            let length = (header & 0x7fff_ffff) as usize;
            let words = length.div_ceil(4);
            let body = self
                .texts
                .get(offset + 1..offset + 1 + words)
                .ok_or(ImageError("invalid image literal length"))?;
            let bytes: Vec<_> = body
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .take(length)
                .collect();
            if JsStr::try_from_canonical_bytes(&bytes).is_none()
                || (header >> 31 == 0 && std::str::from_utf8(&bytes).is_err())
            {
                return Err(ImageError("invalid image literal text"));
            }
            offset += 1 + words;
        }
        let mut arena = NodeArena::default();
        let map = |space, word| -> Result<u32> {
            match space {
                1 => local(word, 0, self.nodes.len()),
                2 => local(word, 0, self.arrays.len()),
                3 if word == u32::MAX => Ok(word),
                3 if word == 0 => Ok(0),
                3 => {
                    let index = local(word, 0, self.texts.len())?;
                    if !text_starts[(index - 1) as usize] {
                        return Err(ImageError("image text reference is not a length word"));
                    }
                    Ok(index)
                }
                4 => names
                    .get(word as usize)
                    .copied()
                    .ok_or(ImageError("invalid image name reference")),
                _ => Ok(word),
            }
        };
        for n in &self.nodes {
            let count = crate::nodes::payload_shape(n.kind).0;
            if n.slots.len() != count || n.literal_flags & 0x8000 != 0 || (!n.payload && count != 0)
            {
                return Err(ImageError("invalid image node shape"));
            }
            let slots = n
                .slots
                .iter()
                .enumerate()
                .map(|(i, &w)| map(Node::image_slot_space(n.kind, i), w))
                .collect::<Result<Vec<_>>>()?;
            let mut comment = n.comment;
            if comment[0] != 0 {
                comment[0] = map(3, comment[0])?;
            } else {
                comment[1] = map(2, comment[1])?;
            }
            let mut node = Node::from_image_payload(
                n.kind,
                &slots,
                comment,
                map(2, n.js_doc)?,
                n.payload,
                &mut arena.tails,
            )
            .ok_or(ImageError("invalid image node payload"))?;
            node.pos = n.pos;
            node.end = n.end;
            node.flags = n.flags;
            node.parent = NodeId::from_position(map(1, n.parent)?);
            node.set_literal_flags(n.literal_flags);
            arena.nodes.push(node);
        }
        for &[items, len, pos, end, flags, modifiers] in &self.arrays {
            if flags > 3
                || modifiers > u16::MAX as u32
                || items as usize > self.items.len()
                || len as usize > self.items.len() - items as usize
            {
                return Err(ImageError("invalid image array"));
            }
            arena.arrays.push(NodeArrayRecord {
                items,
                len,
                pos,
                end,
                has_trailing_comma: flags & 1 != 0,
                is_missing_list: flags & 2 != 0,
                modifier_flags: modifiers as u16,
            });
        }
        for &word in &self.items {
            arena.array_items.push(
                NodeId::from_position(map(1, word)?)
                    .ok_or(ImageError("absent image array element"))?,
            );
        }
        for &word in &self.texts {
            arena.texts.push(TextWord(word));
        }
        arena.finish_record_installation();
        arena.identifier_count = self.identifier_count;
        arena.symbol_bound = self.symbol_bound;
        arena.parse_node_count = self.parse_node_count.map(|n| n as usize);
        arena.factory_node_adjustment = self.factory_adjustment as isize;
        let root = NodeId::from_position(map(1, self.root)?)
            .ok_or(ImageError("absent image source root"))?;
        let mut source = SourceFile::from_tree(
            self.file_name.as_str(),
            Some(self.text.clone()),
            arena,
            root,
            crate::LanguageVariant::Standard,
            crate::ScriptKind::Ts,
            true,
            ExternalModuleIndicatorOptions::default(),
        );
        source.js_doc_parsing_mode = self.mode;
        source.lib_reference_directives = self.references.clone();
        source.external_module_indicator = NodeId::from_position(map(1, self.external_module)?);
        Ok(source)
    }

    pub fn matches(&self, file_name: &str, text: &str, mode: JSDocParsingMode) -> bool {
        self.file_name == file_name && self.text == text && self.mode == mode
    }

    /// Prove that binding only changed node flags before representing its
    /// tree edits sparsely. Any newly mutated field fails this boundary.
    pub fn binding_flag_edits(&self, bound: &Self) -> Result<Vec<[u32; 2]>> {
        if self.nodes.len() != bound.nodes.len() {
            return Err(ImageError("binding added library syntax"));
        }
        let mut result = Vec::new();
        let mut expected = self.clone();
        for (index, (a, b)) in expected.nodes.iter_mut().zip(&bound.nodes).enumerate() {
            if a.flags != b.flags {
                result.push([index as u32 + 1, b.flags as u32]);
            }
            a.flags = b.flags;
        }
        if &expected != bound {
            return Err(ImageError("binding changed a non-flag library field"));
        }
        Ok(result)
    }

    pub fn apply_binding_flags(&mut self, edits: &[[u32; 2]]) -> Result<()> {
        let mut previous = 0;
        for &[id, flags] in edits {
            if id <= previous || id as usize > self.nodes.len() {
                return Err(ImageError("invalid library binding flag edit"));
            }
            self.nodes[id as usize - 1].flags = flags as i32;
            previous = id;
        }
        Ok(())
    }

    pub fn encode_binding_edits(&self, bound: &Self) -> Result<Vec<u8>> {
        let edits = self.binding_flag_edits(bound)?;
        let mut out = Encoder::default();
        out.bytes(self.file_name.as_bytes())?;
        out.word(self.mode as u32);
        out.wide(fingerprint(self.text.as_bytes()));
        out.count(edits.len())?;
        for [id, flags] in edits {
            out.word(id);
            out.word(flags);
        }
        out.finish(b"TSCLIBE1", schema())
    }

    pub fn apply_binding_edits(&mut self, bytes: &[u8]) -> Result<()> {
        let mut input = Decoder::open(bytes, b"TSCLIBE1", schema())?;
        if input.bytes()? != self.file_name.as_bytes()
            || input.word()? != self.mode as u32
            || input.wide()? != fingerprint(self.text.as_bytes())
        {
            return Err(ImageError("library binding edits belong to another source"));
        }
        let count = input.count(8)?;
        let edits = (0..count)
            .map(|_| Ok([input.word()?, input.word()?]))
            .collect::<Result<Vec<_>>>()?;
        input.end()?;
        self.apply_binding_flags(&edits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_provenance_equals_source_inputs() {
        let input = concat!(
            include_str!("../nodes.schema.json"),
            include_str!("../nodes.layout.json"),
            include_str!("nodes.rs"),
            include_str!("kind.rs"),
            include_str!("library_image.rs"),
            include_str!("library_image/runtime.rs"),
            include_str!("../build.rs"),
        );
        assert_eq!(schema(), fingerprint(input.as_bytes()));
    }

    #[test]
    fn syntax_image_restores_owned_records_and_rejects_corruption() {
        let source = crate::parse_source_file(
            "lib.test.d.ts",
            "/** A {@link Array}. */ interface X<T> { readonly x?: T; f(x: '\\ud800'): string[] }",
            crate::ParseOptions::default(),
            None,
        );
        let image = SyntaxImage::capture(&source).unwrap();
        let bytes = image.encode().unwrap();
        let decoded = SyntaxImage::decode(&bytes, source.text()).unwrap();
        assert_eq!(image, decoded);
        let restored = decoded.restore().unwrap();
        assert_eq!(source, restored);
        assert_eq!(restored.arena.node_end(), decoded.nodes.len() as u32);
        assert_eq!(restored.arena.array_end(), decoded.arrays.len() as u32);
        let following = crate::parse_source_file(
            "ordinary.ts",
            "export const next = [1];",
            crate::ParseOptions {
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
        assert_eq!(bytes, decoded.encode().unwrap());
        assert!(!decoded.matches("other.d.ts", source.text(), source.js_doc_parsing_mode));
        assert!(!decoded.matches("lib.test.d.ts", "", source.js_doc_parsing_mode));
        assert!(!decoded.matches("lib.test.d.ts", source.text(), JSDocParsingMode::ParseNone));
        for position in [0, 8, 12, 16, 24, 32, bytes.len() - 1] {
            let mut corrupt = bytes.clone();
            corrupt[position] ^= 1;
            assert!(
                SyntaxImage::decode(&corrupt, source.text()).is_err(),
                "byte {position}"
            );
        }
        for length in [0, 7, 31, bytes.len() - 1] {
            assert!(SyntaxImage::decode(&bytes[..length], source.text()).is_err());
        }
        let mut extra = bytes;
        extra.push(0);
        assert!(SyntaxImage::decode(&extra, source.text()).is_err());
    }

    #[test]
    fn image_validation_precedes_tail_and_literal_reads() {
        let source = crate::parse_source_file(
            "lib.test.d.ts",
            "interface A { x: 'hello' }",
            crate::ParseOptions::default(),
            None,
        );
        let image = SyntaxImage::capture(&source).unwrap();
        let mut malformed = image.clone();
        malformed.nodes[0].slots = vec![0; 100];
        assert!(malformed.restore().is_err());
        let mut malformed = image.clone();
        malformed.texts[0] = 0x7fff_ffff;
        assert!(malformed.restore().is_err());
        let mut malformed = image.clone();
        malformed.texts[1] = 0xffff_ffff;
        assert!(malformed.restore().is_err());
        let mut malformed = image;
        malformed.root = u32::MAX;
        assert!(malformed.restore().is_err());
    }
}
