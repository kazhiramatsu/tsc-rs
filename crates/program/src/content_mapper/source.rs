//! A content-mapped file as the program holds it: tsgo transform.go
//! (`ParseResult` up to its parse) and `ast.ContentMapperSourceFileInfo`,
//! with the file loader's content mapper diagnostics (compiler/
//! fileloader.go).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use tsc_diagnostics::spanmap::{MappedDiagnosticDirective, MappingErrorKind, SpanMap};
use tsc_diagnostics::{
    gen, Diagnostic, DiagnosticCategory, DiagnosticMessage, DocumentVersion, JsStr, JsString,
    MessageChain, TextSnapshot,
};

use super::errors::{
    DiagnosticDirectiveErrorKind, InitializeError, InitializeErrorKind, ProjectErrorKind,
    ProjectFailure, ResponseFailure, TransformFailure,
};
use super::protocol::{MappedResult, MapperDiagnostic, TransformOutput};
use super::{is_supported_virtual_extension, Mapper};

/// tsgo `maxContentMapperFailures`: the transform failures a mapper may
/// have before it is no longer used.
pub const MAX_CONTENT_MAPPER_FAILURES: usize = 5;

/// tsgo `ast.ContentMapperSourceFileInfo`: what a content-mapped file (or
/// one of its supplemental outputs) carries besides its virtual text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentMappedSource {
    /// The identity of the mapper that produced the file.
    pub content_mapper: String,
    /// The mapper's transform identity in the program (the parse cache key).
    pub transform_identity: String,
    /// The name the virtual text is parsed as: the file with its virtual
    /// extension, or a supplemental output's own name.
    pub virtual_file_name: JsString,
    /// The file's untransformed text.
    pub original_text: Arc<str>,
    /// How the virtual text maps to the original; none for a file whose
    /// transform could not be used (a failure stub).
    pub span_map: Option<SpanMap>,
    pub diagnostic_directives: Vec<MappedDiagnosticDirective>,
    /// The supplemental outputs' file names (on the canonical file).
    pub supplemental: Vec<JsString>,
    /// The canonical file's name (on a supplemental output).
    pub canonical: Option<JsString>,
    /// Reported with the file's syntactic diagnostics: the mapper's own
    /// (at original ranges) or why the transform could not be used.
    pub diagnostics: Vec<Diagnostic>,
}

impl ContentMappedSource {
    /// tsgo `IsContentMapperFailureStub`.
    pub fn is_failure_stub(&self) -> bool {
        self.span_map.is_none()
    }

    /// tsgo `IsContentMapperSupplemental`.
    pub fn is_supplemental(&self) -> bool {
        self.canonical.is_some()
    }
}

/// One virtual output to load: its file name, its text and what it carries,
/// shared with the programs that hold it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualSource {
    pub file_name: JsString,
    pub text: Arc<TextSnapshot>,
    pub info: Arc<ContentMappedSource>,
}

impl VirtualSource {
    fn new(file_name: JsString, text: String, info: ContentMappedSource) -> Self {
        Self {
            file_name,
            text: TextSnapshot::new(text, DocumentVersion::default()),
            info: Arc::new(info),
        }
    }
}

/// tsgo `contentmapper.SourceFiles` before parsing: the canonical output
/// (under the mapped file's own name) and the supplemental ones.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualSources {
    pub canonical: VirtualSource,
    pub supplemental: Vec<VirtualSource>,
}

/// tsgo `ParseResult` up to its parse: each output's span map checked
/// against its text and the original content, its extension one a program
/// can hold, and the outputs named — the canonical one by the file with its
/// virtual extension, the supplemental ones `<file>.<index><extension>`.
pub fn virtual_sources(
    file_name: JsStr<'_>,
    content: &str,
    mapper: &Mapper,
    transform_identity: &str,
    output: TransformOutput,
) -> Result<VirtualSources, TransformFailure> {
    let original_units: Vec<u16> = content.encode_utf16().collect();
    let check = |result: &MappedResult| -> Result<(), TransformFailure> {
        let virtual_units: Vec<u16> = result.text.encode_utf16().collect();
        if let Some(problem) = result.mappings.validate(&virtual_units, &original_units) {
            return Err(TransformFailure::Mappings(problem));
        }
        if !is_supported_virtual_extension(&result.virtual_extension) {
            return Err(TransformFailure::Response(
                ResponseFailure::InvalidVirtualExtension(result.virtual_extension.clone()),
            ));
        }
        Ok(())
    };
    check(&output.canonical)?;
    for supplemental in &output.supplemental {
        check(supplemental)?;
    }
    let original_text: Arc<str> = Arc::from(content);
    let identity = mapper.identity();
    let supplemental_names: Vec<JsString> = output
        .supplemental
        .iter()
        .enumerate()
        .map(|(index, supplemental)| {
            let mut name = file_name.to_owned();
            name.push_str(&format!(".{index}{}", supplemental.virtual_extension));
            name
        })
        .collect();
    let info = |virtual_file_name: JsString, result: &MappedResult| ContentMappedSource {
        content_mapper: identity.clone(),
        transform_identity: transform_identity.to_owned(),
        virtual_file_name,
        original_text: Arc::clone(&original_text),
        span_map: Some(result.mappings.clone()),
        diagnostic_directives: result.diagnostic_directives.clone(),
        supplemental: Vec::new(),
        canonical: None,
        diagnostics: Vec::new(),
    };
    let mut virtual_file_name = file_name.to_owned();
    virtual_file_name.push_str(&output.canonical.virtual_extension);
    let mut canonical = info(virtual_file_name, &output.canonical);
    canonical.supplemental = supplemental_names.clone();
    canonical.diagnostics = output
        .diagnostics
        .iter()
        .map(|diagnostic| mapper_diagnostic(file_name, diagnostic))
        .collect();
    let supplemental = output
        .supplemental
        .iter()
        .zip(supplemental_names)
        .map(|(result, name)| {
            let mut info = info(name.clone(), result);
            info.canonical = Some(file_name.to_owned());
            VirtualSource::new(name, result.text.clone(), info)
        })
        .collect();
    Ok(VirtualSources {
        canonical: VirtualSource::new(file_name.to_owned(), output.canonical.text, canonical),
        supplemental,
    })
}

/// tsgo `ContentMappedParseCache` (the project system's): a file's
/// transform outputs, found again by the file, its original text and the
/// mapper's transform identity, so a program built again takes them without
/// running the mapper. An entry lives while a program holds its outputs:
/// tsgo counts the programs that acquired the bundle, and the `Weak`s here
/// follow the programs' `Arc`s.
#[derive(Debug, Default)]
pub struct ContentMappedCache {
    entries: Mutex<HashMap<(JsString, u64), Vec<CachedSources>>>,
}

#[derive(Debug)]
struct CachedSources {
    transform_identity: String,
    canonical: CachedSource,
    supplemental: Vec<CachedSource>,
}

#[derive(Debug)]
struct CachedSource {
    file_name: JsString,
    text: Weak<TextSnapshot>,
    info: Weak<ContentMappedSource>,
}

impl CachedSource {
    fn new(source: &VirtualSource) -> Self {
        Self {
            file_name: source.file_name.clone(),
            text: Arc::downgrade(&source.text),
            info: Arc::downgrade(&source.info),
        }
    }

    fn upgrade(&self) -> Option<VirtualSource> {
        Some(VirtualSource {
            file_name: self.file_name.clone(),
            text: self.text.upgrade()?,
            info: self.info.upgrade()?,
        })
    }

    fn is_held(&self) -> bool {
        self.text.strong_count() > 0 && self.info.strong_count() > 0
    }
}

impl CachedSources {
    fn is_held(&self) -> bool {
        self.canonical.is_held() && self.supplemental.iter().all(CachedSource::is_held)
    }

    fn upgrade(&self) -> Option<VirtualSources> {
        Some(VirtualSources {
            canonical: self.canonical.upgrade()?,
            supplemental: self
                .supplemental
                .iter()
                .map(CachedSource::upgrade)
                .collect::<Option<_>>()?,
        })
    }
}

impl ContentMappedCache {
    /// The outputs a program holds for `file_name` with `content` under
    /// `transform_identity` (tsgo `AcquireOrError` finding the key).
    pub fn get(
        &self,
        file_name: JsStr<'_>,
        content: &str,
        transform_identity: &str,
    ) -> Option<VirtualSources> {
        self.lock()
            .get(&(file_name.to_owned(), text_hash(content)))?
            .iter()
            .filter(|entry| entry.transform_identity == transform_identity)
            .find_map(CachedSources::upgrade)
            .filter(|sources| *sources.canonical.info.original_text == *content)
    }

    /// Record a transform's outputs, and forget the entries of the same file
    /// and content no program holds any more.
    pub fn insert(&self, sources: &VirtualSources) {
        let info = &sources.canonical.info;
        let key = (
            sources.canonical.file_name.clone(),
            text_hash(&info.original_text),
        );
        let mut entries = self.lock();
        let slot = entries.entry(key).or_default();
        slot.retain(CachedSources::is_held);
        slot.push(CachedSources {
            transform_identity: info.transform_identity.clone(),
            canonical: CachedSource::new(&sources.canonical),
            supplemental: sources.supplemental.iter().map(CachedSource::new).collect(),
        });
    }

    /// Forget the entries no program holds (tsgo deletes an entry when its
    /// count reaches zero).
    pub fn purge(&self) {
        self.lock().retain(|_, slot| {
            slot.retain(CachedSources::is_held);
            !slot.is_empty()
        });
    }

    /// The number of entries a program still holds.
    pub fn len(&self) -> usize {
        self.lock()
            .values()
            .flatten()
            .filter(|entry| entry.is_held())
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(JsString, u64), Vec<CachedSources>>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The hash an original text is found by (tsgo hashes file contents with
/// xxh3); a hit still compares the text.
fn text_hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    text.hash(&mut hasher);
    hasher.finish()
}

/// tsgo `NewExternalDiagnostic` of a mapper's diagnostic: an error at its
/// original range, under the mapper's diagnostic source.
fn mapper_diagnostic(file_name: JsStr<'_>, diagnostic: &MapperDiagnostic) -> Diagnostic {
    let start = u32::try_from(diagnostic.start).unwrap_or_default();
    let end = u32::try_from(diagnostic.end).unwrap_or_default();
    Diagnostic::new_js(
        Some(file_name.to_owned()),
        Some(start),
        Some(end.saturating_sub(start)),
        MessageChain::external(
            diagnostic.code as u32,
            DiagnosticCategory::Error,
            diagnostic.message_text.as_str(),
        ),
    )
    .with_reports_unnecessary(None)
    .with_reports_deprecated(None)
    .with_source(diagnostic.source.clone())
}

/// tsgo `emptyContentMappedFile`'s information: a file whose transform could
/// not be used is empty TypeScript, keeps its original content and is
/// still a mapped file (not emitted).
pub fn failure_stub(
    file_name: JsStr<'_>,
    content: &str,
    mapper: &Mapper,
    transform_identity: &str,
    diagnostics: Vec<Diagnostic>,
) -> VirtualSource {
    let mut virtual_file_name = file_name.to_owned();
    virtual_file_name.push_str(".ts");
    VirtualSource::new(
        file_name.to_owned(),
        String::new(),
        ContentMappedSource {
            content_mapper: mapper.identity(),
            transform_identity: transform_identity.to_owned(),
            virtual_file_name,
            original_text: Arc::from(content),
            span_map: None,
            diagnostic_directives: Vec::new(),
            supplemental: Vec::new(),
            canonical: None,
            diagnostics,
        },
    )
}

fn chain(message: &'static DiagnosticMessage, args: &[JsString]) -> MessageChain {
    MessageChain::new_js(message, args).without_location()
}

fn number(value: impl ToString) -> JsString {
    JsString::from(value.to_string())
}

/// tsgo `contentMapperTransformDiagnostic` (and
/// `contentMapperMappingDiagnostic`): why a file's transform could not be
/// used, at the start of the file.
pub fn transform_diagnostic(
    file_name: JsStr<'_>,
    label: &str,
    failure: &TransformFailure,
) -> Diagnostic {
    let label = JsString::from(label);
    let at_start = |message: MessageChain| {
        Diagnostic::new_js(Some(file_name.to_owned()), Some(0), Some(0), message)
    };
    let failed = |detail: MessageChain| {
        at_start(
            MessageChain::new_js(
                &gen::The_content_mapper_0_failed_to_transform_this_file,
                std::slice::from_ref(&label),
            )
            .with_next(vec![detail]),
        )
    };
    let simple = |message: &'static DiagnosticMessage, args: &[JsString]| {
        let mut args_with_label = vec![label.clone()];
        args_with_label.extend_from_slice(args);
        at_start(MessageChain::new_js(message, &args_with_label))
    };
    match failure {
        TransformFailure::SupplementalFileCollision(name) => failed(chain(
            &gen::Content_mapper_supplemental_output_file_0_conflicts_with_an_existing_file,
            &[JsString::from(name.as_str())],
        )),
        TransformFailure::Initialize(error) => match &error.kind {
            InitializeErrorKind::PositionEncoding { encoding } => failed(chain(
                &gen::The_content_mapper_selected_unsupported_position_encoding_0,
                &[JsString::from(encoding.as_str())],
            )),
            InitializeErrorKind::EmptyDiagnosticSource => {
                failed(chain(&gen::The_content_mapper_diagnostic_source_must_not_be_empty, &[]))
            }
            InitializeErrorKind::ReservedDiagnosticSource { source } => failed(chain(
                &gen::The_content_mapper_diagnostic_source_0_is_reserved_by_TypeScript,
                &[JsString::from(source.as_str())],
            )),
            _ => failed(chain(
                &gen::The_content_mapper_process_could_not_be_started_or_initialized,
                &[],
            )),
        },
        TransformFailure::Project(failure) => failed(chain(project_error_message(failure), &[])),
        TransformFailure::Request(_) => failed(chain(
            &gen::The_content_mapper_process_failed_while_handling_the_transform_request,
            &[],
        )),
        TransformFailure::Response(ResponseFailure::InvalidVirtualExtension(extension)) => {
            failed(chain(
                &gen::The_content_mapper_returned_an_output_with_unsupported_virtual_extension_0,
                &[JsString::from(extension.as_str())],
            ))
        }
        TransformFailure::Response(ResponseFailure::DiagnosticDirective(error)) => {
            let index = number(error.index);
            let mut detail = match error.kind {
                DiagnosticDirectiveErrorKind::InvalidRange => chain(
                    &gen::Diagnostic_directive_0_returned_by_the_content_mapper_has_an_invalid_range,
                    &[index],
                ),
                DiagnosticDirectiveErrorKind::InvalidPolicy => chain(
                    &gen::The_content_mapper_returned_a_diagnostic_directive_with_invalid_policy_0,
                    &[number(error.policy)],
                ),
                DiagnosticDirectiveErrorKind::ExpectMissingUnusedDiagnostic => chain(
                    &gen::Diagnostic_directive_0_returned_by_the_content_mapper_must_specify_unusedExpectDirectiveIndex_when_there_is_not_exactly_one_unusedExpectDirectiveDiagnostics_entry,
                    &[index],
                ),
                DiagnosticDirectiveErrorKind::InvalidUnusedDiagnosticIndex => chain(
                    &gen::Diagnostic_directive_0_returned_by_the_content_mapper_has_an_invalid_unusedExpectDirectiveIndex,
                    &[index],
                ),
                DiagnosticDirectiveErrorKind::Overlap => chain(
                    &gen::The_content_mapper_returned_diagnostic_directives_with_overlapping_virtual_ranges,
                    &[],
                ),
            };
            if let Some(supplemental) = error.supplemental_index {
                // tsgo NewDiagnosticChain: the supplemental output's entry
                // over the directive's.
                detail = chain(
                    &gen::The_invalid_diagnostic_directive_is_in_supplemental_output_0_returned_by_the_content_mapper,
                    &[number(supplemental)],
                )
                .with_next(vec![detail]);
            }
            failed(detail)
        }
        TransformFailure::Response(ResponseFailure::Malformed(_)) => failed(chain(
            &gen::The_content_mapper_returned_an_invalid_transform_response,
            &[],
        )),
        TransformFailure::Mappings(problem) => match problem.kind {
            MappingErrorKind::Overlap => simple(
                &gen::The_content_mapper_0_produced_overlapping_or_out_of_order_position_mappings_near_virtual_offset_1,
                &[number(problem.virtual_pos)],
            ),
            MappingErrorKind::OutOfBounds => simple(
                &gen::The_content_mapper_0_produced_a_position_mapping_that_points_outside_the_original_content_original_offset_1,
                &[number(problem.original_pos)],
            ),
            MappingErrorKind::VerbatimMismatch => simple(
                &gen::The_content_mapper_0_produced_a_verbatim_mapping_that_does_not_match_the_original_content_virtual_offset_1_original_offset_2,
                &[number(problem.virtual_pos), number(problem.original_pos)],
            ),
            MappingErrorKind::Kind => simple(
                &gen::The_content_mapper_0_produced_a_position_mapping_with_an_invalid_kind_near_virtual_offset_1,
                &[number(problem.virtual_pos)],
            ),
            MappingErrorKind::Feature => simple(
                &gen::The_content_mapper_0_produced_invalid_mapping_features_near_original_offset_1,
                &[number(problem.original_pos)],
            ),
        },
        TransformFailure::Host(_) => simple(&gen::The_content_mapper_0_failed_to_transform_this_file, &[]),
    }
}

/// tsgo `ContentMapperProjectErrorDiagnostic`: the message of a project
/// setup failure.
pub fn project_error_message(failure: &ProjectFailure) -> &'static DiagnosticMessage {
    match failure {
        ProjectFailure::Project(kind) => match kind {
            ProjectErrorKind::MalformedResponse => {
                &gen::The_content_mapper_returned_a_project_response_that_could_not_be_decoded
            }
            ProjectErrorKind::MissingConfigIdentity => {
                &gen::The_content_mapper_did_not_return_configIdentity_which_is_required_when_the_content_mapper_has_dynamicConfig_true_in_its_package_json
            }
            ProjectErrorKind::NonAbsoluteWatchedFile => {
                &gen::The_content_mapper_returned_a_non_absolute_path_in_watchedFiles
            }
            ProjectErrorKind::UnexpectedConfigIdentity => {
                &gen::The_content_mapper_returned_configIdentity_which_is_only_allowed_when_it_declares_dynamicConfig_true_in_its_package_json
            }
            ProjectErrorKind::UnexpectedWatchedFiles => {
                &gen::The_content_mapper_returned_watchedFiles_which_is_only_allowed_when_it_declares_dynamicConfig_true_in_its_package_json
            }
        },
        ProjectFailure::Initialize(_) | ProjectFailure::Request(_) => {
            &gen::The_content_mapper_process_failed_while_handling_the_project_request
        }
    }
}

/// tsgo `ContentMapperInitializationDiagnostic`: a mapper that could not be
/// started or initialized, without a file (by the error's mapper name when
/// no label is given).
pub fn initialization_diagnostic(label: &str, error: &InitializeError) -> Diagnostic {
    let label = if label.is_empty() {
        error.mapper_name.as_str()
    } else {
        label
    };
    let detail = match &error.kind {
        InitializeErrorKind::ProcessStart { command, detail } => chain(
            &gen::The_content_mapper_command_0_could_not_be_started_1,
            &[JsString::from(command.as_str()), JsString::from(detail.as_str())],
        ),
        InitializeErrorKind::ProcessExit { exit_code } => chain(
            &gen::The_content_mapper_process_exited_before_responding_to_the_initialize_request_exit_code_0,
            &[number(exit_code)],
        ),
        InitializeErrorKind::NoResponse { timeout_seconds } => chain(
            &gen::The_content_mapper_did_not_respond_to_the_initialize_request_within_0_seconds,
            &[number(timeout_seconds)],
        ),
        InitializeErrorKind::InvalidResponse { detail } => chain(
            &gen::The_content_mapper_returned_an_initialize_response_that_could_not_be_decoded_0,
            &[JsString::from(detail.as_str())],
        ),
        InitializeErrorKind::Request { detail } => chain(
            &gen::The_content_mapper_s_initialize_request_failed_0,
            &[JsString::from(detail.as_str())],
        ),
        InitializeErrorKind::PositionEncoding { encoding } => chain(
            &gen::The_content_mapper_selected_unsupported_position_encoding_0,
            &[JsString::from(encoding.as_str())],
        ),
        InitializeErrorKind::EmptyDiagnosticSource => {
            chain(&gen::The_content_mapper_diagnostic_source_must_not_be_empty, &[])
        }
        InitializeErrorKind::ReservedDiagnosticSource { source } => chain(
            &gen::The_content_mapper_diagnostic_source_0_is_reserved_by_TypeScript,
            &[JsString::from(source.as_str())],
        ),
    };
    Diagnostic::new_js(
        None,
        None,
        None,
        MessageChain::new_js(
            &gen::The_content_mapper_0_could_not_be_initialized,
            &[JsString::from(label)],
        )
        .with_next(vec![detail]),
    )
}

/// tsgo `ContentMapperProjectDiagnostic`: a project setup failure without a
/// file.
pub fn project_diagnostic(failure: &ProjectFailure) -> Diagnostic {
    match failure {
        ProjectFailure::Initialize(error) => initialization_diagnostic("", error),
        failure => Diagnostic::new_js(
            None,
            None,
            None,
            MessageChain::new_js(project_error_message(failure), &[]),
        ),
    }
}

/// tsgo `recordContentMapperFailure`'s diagnostic: the mapper is no longer
/// used after its failures.
pub fn disabled_diagnostic(label: &str) -> Diagnostic {
    Diagnostic::new_js(
        None,
        None,
        None,
        MessageChain::new_js(
            &gen::The_content_mapper_0_failed_1_times_and_will_not_be_used,
            &[JsString::from(label), number(MAX_CONTENT_MAPPER_FAILURES)],
        ),
    )
}

#[cfg(test)]
#[path = "../../tests/unit/content_mapper/source_tests.rs"]
mod source_tests;
