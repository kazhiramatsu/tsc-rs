#![forbid(unsafe_code)]

pub mod arena;
mod chars;
pub mod for_each_child;
mod keywords;
pub mod kind;
pub mod nodes;
pub mod observable_fields;
mod parser;
pub mod regex;
mod regex_unicode;
mod relocate;
pub mod scanner;
pub mod tokens;

use std::sync::Arc;

use tsc_diagnostics::{DiagnosticList, DocumentVersion, PositionIndex, TextSnapshot};
use tsc_types::{
    IdentityAllocationPolicy, IdentityDomain, IdentityError, IdentityLease, IdentitySpace,
    ScriptTarget,
};

pub use arena::NodeArena;
pub use for_each_child::{for_each_child, NodeLookup};
pub use kind::SyntaxKind;
pub use nodes::{
    JSDocComment, Node, NodeArray, NodeArrayId, NodeData, NodeId, NodePayload, SourceFileData,
};
pub use observable_fields::{for_each_observable_field, ObservableField};
pub use parser::{
    is_identifier_text, is_identifier_text_for_target, JSDocParsingMode, ParseOptions, SyntaxCursor,
};
pub use scanner::{
    is_js_whitespace, is_line_break, is_whitespace_like, js_trim_start, scan_big_int_string,
    scan_token_kinds, scan_tokens, skip_trivia, template_text_utf16, BigIntStringScan,
    CommentDirective, CommentDirectiveKind, LanguageVariant, TokenRecord,
};

/// Resolution-mode override retained from a leading
/// `/// <reference types="...">` directive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TypeReferenceDirectiveResolutionMode {
    Import,
    Require,
}

/// Exact source-owned triple-slash `path` or `lib` reference observation.
///
/// `pos` and `end` are UTF-16 offsets covering only the selected attribute
/// value, matching the vendored `FileReference` contract. `preserve` records
/// only the exact `preserve="true"` pragma value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileReference {
    pub file_name: String,
    pub pos: u32,
    pub end: u32,
    pub preserve: bool,
}

/// Exact source-owned type-reference directive observation.
///
/// `pos` and `end` are UTF-16 offsets covering only the directive's `types`
/// value, matching the vendored `FileReference` contract and TS2688 span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeReferenceDirective {
    pub file_name: String,
    pub pos: u32,
    pub end: u32,
    pub resolution_mode: Option<TypeReferenceDirectiveResolutionMode>,
    /// Whether the directive explicitly requested `preserve="true"`.
    pub preserve: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceFile {
    pub file_name: String,
    snapshot: Arc<TextSnapshot>,
    /// tsc SourceFile.languageVersion: the effective target used by the
    /// parser and scanner for this file.
    pub language_version: ScriptTarget,
    pub language_variant: LanguageVariant,
    pub is_declaration_file: bool,
    /// tsc SourceFile.jsDocParsingMode.
    pub js_doc_parsing_mode: JSDocParsingMode,
    pub arena: NodeArena,
    pub root: NodeId,
    pub external_module_indicator: Option<NodeId>,
    pub parse_diagnostics: DiagnosticList,
    /// tsc SourceFile.jsDocDiagnostics: diagnostics produced while
    /// parsing attached JSDoc. They are merged into bind/check diagnostics
    /// only for checked JavaScript files, never into syntactic diagnostics.
    pub js_doc_diagnostics: DiagnosticList,
    /// tsc SourceFile.referencedFiles, in leading pragma order.
    pub referenced_files: Vec<FileReference>,
    /// tsc SourceFile.typeReferenceDirectives, in leading pragma order.
    pub type_reference_directives: Vec<TypeReferenceDirective>,
    /// tsc SourceFile.libReferenceDirectives, in leading pragma order.
    pub lib_reference_directives: Vec<FileReference>,
    /// Whether leading multiline comments contain a recognized
    /// `@jsxImportSource` pragma.
    pub has_jsx_import_source_pragma: bool,
    /// The final argument of the last recognized `@jsxImportSource` pragma,
    /// when it has a usable argument. TypeScript's pragma map is last-write
    /// wins; the boolean above is retained for malformed/argument-less
    /// pragma observations.
    pub jsx_import_source_pragma: Option<String>,
    /// Whether leading multiline comments contain a recognized `@jsxRuntime`
    /// pragma.
    pub has_jsx_runtime_pragma: bool,
    /// The final argument of the last recognized `@jsxRuntime` pragma, when
    /// it has a usable argument (for example `automatic` or `classic`).
    pub jsx_runtime_pragma: Option<String>,
    /// tsc SourceFile.commentDirectives: scanner-collected
    /// `@ts-expect-error`/`@ts-ignore` markers, in scan order (byte
    /// offsets; see CommentDirective).
    pub comment_directives: Vec<CommentDirective>,
}

impl SourceFile {
    pub fn snapshot(&self) -> &Arc<TextSnapshot> {
        &self.snapshot
    }

    pub fn text(&self) -> &str {
        self.snapshot.text()
    }

    pub fn positions(&self) -> &PositionIndex {
        self.snapshot.positions()
    }

    pub fn node_count(&self) -> usize {
        self.arena.len()
    }

    pub fn identifier_count(&self) -> usize {
        self.arena
            .nodes()
            .iter()
            .filter(|node| node.kind == SyntaxKind::Identifier)
            .count()
    }

    pub fn node_identity_lease(&self) -> Option<&IdentityLease> {
        self.arena.node_identity_lease()
    }

    pub fn array_identity_lease(&self) -> Option<&IdentityLease> {
        self.arena.array_identity_lease()
    }

    pub fn identity_owned_by(&self, domain: &IdentityDomain) -> bool {
        self.node_identity_lease()
            .is_some_and(|lease| lease.belongs_to(domain))
            && self
                .array_identity_lease()
                .is_some_and(|lease| lease.belongs_to(domain))
    }

    /// Publish a locally constructed syntax tree into an exact domain range.
    /// Every ID-bearing `NodeData` field is covered by generated relocation.
    pub fn relocate_into_identity_domain(
        &mut self,
        domain: &IdentityDomain,
    ) -> Result<(), IdentityError> {
        let (node_count, array_count) = self.identity_counts()?;
        let leases = domain.lease_batch(&[
            (IdentitySpace::Node, node_count),
            (IdentitySpace::NodeArray, array_count),
        ])?;
        let (node_lease, array_lease) = syntax_leases(leases)?;
        let relocation = self.arena.identity_relocation(&node_lease, &array_lease)?;
        relocation.node(&mut self.root)?;
        if let Some(indicator) = &mut self.external_module_indicator {
            relocation.node(indicator)?;
        }
        self.arena
            .apply_identity_relocation(relocation, node_lease, array_lease)
    }

    fn identity_counts(&self) -> Result<(u32, u32), IdentityError> {
        let node_count =
            u32::try_from(self.arena.nodes().len()).map_err(|_| IdentityError::Exhausted {
                space: IdentitySpace::Node,
                requested: u32::MAX,
                limit: u32::MAX,
            })?;
        let array_count = u32::try_from(self.arena.node_arrays().len()).map_err(|_| {
            IdentityError::Exhausted {
                space: IdentitySpace::NodeArray,
                requested: u32::MAX,
                limit: u32::MAX,
            }
        })?;
        Ok((node_count, array_count))
    }

    fn attach_identity_leases(&mut self, leases: Vec<IdentityLease>) -> Result<(), IdentityError> {
        let (node_lease, array_lease) = syntax_leases(leases)?;
        self.arena.attach_identity_leases(node_lease, array_lease)
    }
}

fn syntax_leases(
    leases: Vec<IdentityLease>,
) -> Result<(IdentityLease, IdentityLease), IdentityError> {
    let mut node = None;
    let mut array = None;
    for lease in leases {
        match lease.space() {
            IdentitySpace::Node => node = Some(lease),
            IdentitySpace::NodeArray => array = Some(lease),
            space => {
                return Err(IdentityError::InvalidLease {
                    space,
                    detail: "syntax publication received a non-syntax lease",
                });
            }
        }
    }
    Ok((
        node.ok_or(IdentityError::ReservationMismatch)?,
        array.ok_or(IdentityError::ReservationMismatch)?,
    ))
}

pub fn parse_source_file(
    file_name: impl Into<String>,
    text: impl Into<String>,
    options: ParseOptions,
    cursor: Option<&SyntaxCursor>,
) -> SourceFile {
    let snapshot = TextSnapshot::new(text.into(), DocumentVersion::default());
    parser::parse_source_file_from_snapshot(file_name.into(), snapshot, options, cursor)
}

pub fn parse_source_file_from_snapshot(
    file_name: impl Into<String>,
    snapshot: Arc<TextSnapshot>,
    options: ParseOptions,
    cursor: Option<&SyntaxCursor>,
) -> SourceFile {
    parser::parse_source_file_from_snapshot(file_name.into(), snapshot, options, cursor)
}

/// Parse and publish a source in one identity domain. Ephemeral domains parse
/// directly at a sealed tail; reclaiming domains relocate the completed local
/// tree after reserving exact counts.
pub fn parse_source_file_from_snapshot_in_identity_domain(
    file_name: impl Into<String>,
    snapshot: Arc<TextSnapshot>,
    mut options: ParseOptions,
    cursor: Option<&SyntaxCursor>,
    domain: &IdentityDomain,
) -> Result<SourceFile, IdentityError> {
    let file_name = file_name.into();
    match domain.policy() {
        IdentityAllocationPolicy::EphemeralBump => {
            let reservation =
                domain.reserve_provisional(&[IdentitySpace::Node, IdentitySpace::NodeArray])?;
            options.node_id_base = reservation.base(IdentitySpace::Node)?;
            options.node_array_id_base = reservation.base(IdentitySpace::NodeArray)?;
            let mut source =
                parser::parse_source_file_from_snapshot(file_name, snapshot, options, cursor);
            let (node_count, array_count) = source.identity_counts()?;
            let leases = reservation.seal(&[
                (IdentitySpace::Node, node_count),
                (IdentitySpace::NodeArray, array_count),
            ])?;
            source.attach_identity_leases(leases)?;
            Ok(source)
        }
        IdentityAllocationPolicy::Reclaiming => {
            options.node_id_base = 0;
            options.node_array_id_base = 0;
            let mut source =
                parser::parse_source_file_from_snapshot(file_name, snapshot, options, cursor);
            source.relocate_into_identity_domain(domain)?;
            Ok(source)
        }
    }
}

/// tsc parseJsonText: .json inputs parse as a single JSON value expression.
pub fn parse_json_text(file_name: impl Into<String>, text: impl Into<String>) -> SourceFile {
    let snapshot = TextSnapshot::new(text.into(), DocumentVersion::default());
    parser::parse_json_text_from_snapshot(file_name.into(), snapshot)
}

pub fn parse_json_text_from_snapshot(
    file_name: impl Into<String>,
    snapshot: Arc<TextSnapshot>,
) -> SourceFile {
    parser::parse_json_text_from_snapshot(file_name.into(), snapshot)
}

pub fn parse_json_text_from_snapshot_in_identity_domain(
    file_name: impl Into<String>,
    snapshot: Arc<TextSnapshot>,
    domain: &IdentityDomain,
) -> Result<SourceFile, IdentityError> {
    let file_name = file_name.into();
    match domain.policy() {
        IdentityAllocationPolicy::EphemeralBump => {
            let reservation =
                domain.reserve_provisional(&[IdentitySpace::Node, IdentitySpace::NodeArray])?;
            let mut source = parser::parse_json_text_from_snapshot_with_bases(
                file_name,
                snapshot,
                reservation.base(IdentitySpace::Node)?,
                reservation.base(IdentitySpace::NodeArray)?,
            );
            let (node_count, array_count) = source.identity_counts()?;
            let leases = reservation.seal(&[
                (IdentitySpace::Node, node_count),
                (IdentitySpace::NodeArray, array_count),
            ])?;
            source.attach_identity_leases(leases)?;
            Ok(source)
        }
        IdentityAllocationPolicy::Reclaiming => {
            let mut source = parser::parse_json_text_from_snapshot(file_name, snapshot);
            source.relocate_into_identity_domain(domain)?;
            Ok(source)
        }
    }
}

/// `parse_json_text` with explicit arena bases for a multi-file program.
pub fn parse_json_text_with_bases(
    file_name: impl Into<String>,
    text: impl Into<String>,
    node_id_base: u32,
    node_array_id_base: u32,
) -> SourceFile {
    let snapshot = TextSnapshot::new(text.into(), DocumentVersion::default());
    parser::parse_json_text_from_snapshot_with_bases(
        file_name.into(),
        snapshot,
        node_id_base,
        node_array_id_base,
    )
}

pub fn parse_json_text_from_snapshot_with_bases(
    file_name: impl Into<String>,
    snapshot: Arc<TextSnapshot>,
    node_id_base: u32,
    node_array_id_base: u32,
) -> SourceFile {
    parser::parse_json_text_from_snapshot_with_bases(
        file_name.into(),
        snapshot,
        node_id_base,
        node_array_id_base,
    )
}

/// tsc stringToToken for keyword lookup (identifierToKeywordKind path):
/// Some only for keyword kinds.
pub fn keyword_kind(text: &str) -> Option<SyntaxKind> {
    keywords::keyword_kind(text)
}

/// tsc-port: escapeLeadingUnderscores @6.0.3
/// tsc-hash: 86d7f97e898c96c6de2e47109d4583e4446ba8a518842f34d0d3cd4aa1b0b3c4
/// tsc-span: _tsc.js:11438-11440
///
/// A name beginning with two underscores gains ONE more, so user
/// `__proto__` cannot collide with internal symbol names (`__call`
/// etc. are stored unescaped). The factory applies this to every
/// Identifier/PrivateIdentifier escapedText. The charCodeAt checks are
/// byte checks: `_` is ASCII, so a multi-byte first char never matches.
pub fn escape_leading_underscores(name: &str) -> String {
    let bytes = name.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'_' && bytes[1] == b'_' {
        format!("_{name}")
    } else {
        name.to_owned()
    }
}

/// tsc-port: unescapeLeadingUnderscores @6.0.3
/// tsc-hash: e8294a1e4ef10b8ca2bcce06045e22adab6689e46b655acf51bacc3810ef5271
/// tsc-span: _tsc.js:11441-11444
///
/// Display-time inverse: exactly three leading underscores drop one.
pub fn unescape_leading_underscores(name: &str) -> &str {
    let bytes = name.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'_' && bytes[1] == b'_' && bytes[2] == b'_' {
        &name[1..]
    } else {
        name
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib/tests.rs"]
mod tests;
