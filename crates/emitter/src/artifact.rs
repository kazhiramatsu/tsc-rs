use std::borrow::Cow;
use tsc_diagnostics::{JsStr, JsString};

use tsc_diagnostics::{Diagnostic, DiagnosticList};

use crate::writer::GeneratedText;
use crate::GeneratedUtf16Position;

/// The JavaScript string handed to TypeScript's write callback together with
/// its UTF-8 sink projection. tsc's `writeFile` callback receives the raw
/// string (`_tsc.js:16644-16650`); only `sys.writeFile` (`5164-5182`) turns an
/// unpaired unit into U+FFFD. Both faces are retained so a harness can compare
/// the callback value unit for unit while the sink still receives the
/// projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmitCallbackText(GeneratedText);

impl EmitCallbackText {
    pub(crate) fn from_generated(text: GeneratedText) -> Self {
        Self(text)
    }

    /// The UTF-8 sink projection.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// The raw UTF-16 value.
    pub fn units(&self) -> Cow<'_, [u16]> {
        self.0.units()
    }
}

impl From<String> for EmitCallbackText {
    fn from(text: String) -> Self {
        Self(GeneratedText::from(text))
    }
}

impl From<&str> for EmitCallbackText {
    fn from(text: &str) -> Self {
        Self(GeneratedText::from(text))
    }
}

impl From<Box<str>> for EmitCallbackText {
    fn from(text: Box<str>) -> Self {
        Self(GeneratedText::from(text))
    }
}

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
/// inconsistent struct literal. Product-specific constructors retain the exact
/// callback string without a BOM (raw units plus the UTF-8 sink projection),
/// while the BOM decision remains a separate observable value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmitArtifact {
    path: JsString,
    callback_text: EmitCallbackText,
    write_byte_order_mark: bool,
    kind: EmitArtifactKind,
    source_files: Option<Box<[JsString]>>,
    metadata: Option<EmitWriteMetadata>,
}

impl EmitArtifact {
    pub fn javascript(
        path: impl Into<JsString>,
        callback_text: impl Into<EmitCallbackText>,
        write_byte_order_mark: bool,
        source_files: Option<Vec<JsString>>,
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
        path: impl Into<JsString>,
        callback_text: impl Into<EmitCallbackText>,
        write_byte_order_mark: bool,
        source_files: Option<Vec<JsString>>,
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
        path: impl Into<JsString>,
        callback_text: impl Into<EmitCallbackText>,
        source_files: Option<Vec<JsString>>,
    ) -> Self {
        Self::map(
            path,
            callback_text,
            EmitArtifactKind::JavaScriptMap,
            source_files,
        )
    }

    pub fn declaration_map(
        path: impl Into<JsString>,
        callback_text: impl Into<EmitCallbackText>,
        source_files: Option<Vec<JsString>>,
    ) -> Self {
        Self::map(
            path,
            callback_text,
            EmitArtifactKind::DeclarationMap,
            source_files,
        )
    }

    pub fn build_info(
        path: impl Into<JsString>,
        callback_text: impl Into<EmitCallbackText>,
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
        path: impl Into<JsString>,
        callback_text: impl Into<EmitCallbackText>,
        write_byte_order_mark: bool,
        kind: EmitArtifactKind,
        source_files: Option<Vec<JsString>>,
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
        path: impl Into<JsString>,
        callback_text: impl Into<EmitCallbackText>,
        kind: EmitArtifactKind,
        source_files: Option<Vec<JsString>>,
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

    pub fn path(&self) -> JsStr<'_> {
        self.path.as_js()
    }

    /// The UTF-8 sink projection of the callback string: an unpaired unit is
    /// U+FFFD here, exactly as `sys.writeFile` materializes it.
    pub fn callback_text(&self) -> &str {
        self.callback_text.as_str()
    }

    pub fn callback_bytes(&self) -> &[u8] {
        self.callback_text.as_str().as_bytes()
    }

    /// The JavaScript string tsc hands to its `writeFile` callback, unit for
    /// unit (`_tsc.js:16644-16650`).
    pub fn callback_units(&self) -> Cow<'_, [u16]> {
        self.callback_text.units()
    }

    pub const fn write_byte_order_mark(&self) -> bool {
        self.write_byte_order_mark
    }

    pub const fn kind(&self) -> EmitArtifactKind {
        self.kind
    }

    /// Retains the distinction between an absent callback argument and an
    /// explicitly present empty source list.
    pub fn source_files(&self) -> Option<&[JsString]> {
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
