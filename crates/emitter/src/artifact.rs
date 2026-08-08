use std::borrow::Cow;
use std::path::{Path, PathBuf};

use tsc_diagnostics::{Diagnostic, DiagnosticList};

use crate::GeneratedUtf16Position;

/// Normalized data passed with a JavaScript or declaration text callback.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EmitTextMetadata {
    diagnostics: DiagnosticList,
    source_map_url_position: Option<GeneratedUtf16Position>,
}

impl EmitTextMetadata {
    pub fn new(
        diagnostics: DiagnosticList,
        source_map_url_position: Option<GeneratedUtf16Position>,
    ) -> Self {
        Self {
            diagnostics,
            source_map_url_position,
        }
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub const fn source_map_url_position(&self) -> Option<GeneratedUtf16Position> {
        self.source_map_url_position
    }
}

/// Versioned, normalized build-info callback data reserved for a later track.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmitBuildInfoMetadata {
    schema_version: u32,
    canonical_json: Box<str>,
}

impl EmitBuildInfoMetadata {
    pub fn new(schema_version: u32, canonical_json: impl Into<Box<str>>) -> Self {
        Self {
            schema_version,
            canonical_json: canonical_json.into(),
        }
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn canonical_json(&self) -> &str {
        &self.canonical_json
    }
}

/// Typed form of the optional data argument on TypeScript's write callback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmitWriteMetadata {
    Text(EmitTextMetadata),
    BuildInfo(EmitBuildInfoMetadata),
}

/// Product identity for one output callback.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EmitArtifactKind {
    JavaScript,
    JavaScriptMap,
    Declaration,
    DeclarationMap,
    BuildInfo,
}

/// One immutable write-callback observation.
///
/// Fields are private so a caller cannot manufacture an internally
/// inconsistent struct literal. Product-specific constructors retain exact
/// UTF-8 callback text without a BOM, while the BOM decision remains a
/// separate observable value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmitArtifact {
    path: PathBuf,
    callback_text: Box<str>,
    write_byte_order_mark: bool,
    kind: EmitArtifactKind,
    source_files: Option<Box<[PathBuf]>>,
    metadata: Option<EmitWriteMetadata>,
}

impl EmitArtifact {
    pub fn javascript(
        path: impl Into<PathBuf>,
        callback_text: impl Into<Box<str>>,
        write_byte_order_mark: bool,
        source_files: Option<Vec<PathBuf>>,
        metadata: EmitTextMetadata,
    ) -> Self {
        Self::text(
            path,
            callback_text,
            write_byte_order_mark,
            EmitArtifactKind::JavaScript,
            source_files,
            metadata,
        )
    }

    pub fn declaration(
        path: impl Into<PathBuf>,
        callback_text: impl Into<Box<str>>,
        write_byte_order_mark: bool,
        source_files: Option<Vec<PathBuf>>,
        metadata: EmitTextMetadata,
    ) -> Self {
        Self::text(
            path,
            callback_text,
            write_byte_order_mark,
            EmitArtifactKind::Declaration,
            source_files,
            metadata,
        )
    }

    pub fn javascript_map(
        path: impl Into<PathBuf>,
        callback_text: impl Into<Box<str>>,
        source_files: Option<Vec<PathBuf>>,
    ) -> Self {
        Self::map(
            path,
            callback_text,
            EmitArtifactKind::JavaScriptMap,
            source_files,
        )
    }

    pub fn declaration_map(
        path: impl Into<PathBuf>,
        callback_text: impl Into<Box<str>>,
        source_files: Option<Vec<PathBuf>>,
    ) -> Self {
        Self::map(
            path,
            callback_text,
            EmitArtifactKind::DeclarationMap,
            source_files,
        )
    }

    pub fn build_info(
        path: impl Into<PathBuf>,
        callback_text: impl Into<Box<str>>,
        metadata: EmitBuildInfoMetadata,
    ) -> Self {
        Self {
            path: path.into(),
            callback_text: callback_text.into(),
            write_byte_order_mark: false,
            kind: EmitArtifactKind::BuildInfo,
            source_files: None,
            metadata: Some(EmitWriteMetadata::BuildInfo(metadata)),
        }
    }

    fn text(
        path: impl Into<PathBuf>,
        callback_text: impl Into<Box<str>>,
        write_byte_order_mark: bool,
        kind: EmitArtifactKind,
        source_files: Option<Vec<PathBuf>>,
        metadata: EmitTextMetadata,
    ) -> Self {
        debug_assert!(matches!(
            kind,
            EmitArtifactKind::JavaScript | EmitArtifactKind::Declaration
        ));
        Self {
            path: path.into(),
            callback_text: callback_text.into(),
            write_byte_order_mark,
            kind,
            source_files: source_files.map(Vec::into_boxed_slice),
            metadata: Some(EmitWriteMetadata::Text(metadata)),
        }
    }

    fn map(
        path: impl Into<PathBuf>,
        callback_text: impl Into<Box<str>>,
        kind: EmitArtifactKind,
        source_files: Option<Vec<PathBuf>>,
    ) -> Self {
        debug_assert!(matches!(
            kind,
            EmitArtifactKind::JavaScriptMap | EmitArtifactKind::DeclarationMap
        ));
        Self {
            path: path.into(),
            callback_text: callback_text.into(),
            write_byte_order_mark: false,
            kind,
            source_files: source_files.map(Vec::into_boxed_slice),
            metadata: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn callback_text(&self) -> &str {
        &self.callback_text
    }

    pub fn callback_bytes(&self) -> &[u8] {
        self.callback_text.as_bytes()
    }

    pub const fn write_byte_order_mark(&self) -> bool {
        self.write_byte_order_mark
    }

    pub const fn kind(&self) -> EmitArtifactKind {
        self.kind
    }

    /// Retains the distinction between an absent callback argument and an
    /// explicitly present empty source list.
    pub fn source_files(&self) -> Option<&[PathBuf]> {
        self.source_files.as_deref()
    }

    /// Retains the distinction between an absent callback argument and
    /// present typed metadata.
    pub const fn metadata(&self) -> Option<&EmitWriteMetadata> {
        self.metadata.as_ref()
    }

    /// Bytes a filesystem sink writes after applying the separate BOM flag.
    pub fn materialized_bytes(&self) -> Cow<'_, [u8]> {
        if !self.write_byte_order_mark {
            return Cow::Borrowed(self.callback_bytes());
        }
        let mut bytes = Vec::with_capacity(3 + self.callback_bytes().len());
        bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        bytes.extend_from_slice(self.callback_bytes());
        Cow::Owned(bytes)
    }
}
