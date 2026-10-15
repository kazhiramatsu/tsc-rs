//! Find all references and document highlights as tsgo answers them, for
//! the cases the fixes of the references port reach. Each case is a fourslash test's input with
//! tsgo's answer, which the lsp suite's transcripts record
//! (`scripts/lsp_transcripts.py`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use tsc_compiler::LiveProgram;
use tsc_emitter::source_mapper::SourceMapperHost;
use tsc_host::MemoryCompilerHost;
use tsc_ls::lsconv::{Converters, LspLineMap};
use tsc_ls::lsutil::UserPreferences;
use tsc_ls::LanguageService;
use tsc_lsproto::{
    CallHierarchyIncomingCallsOrNull, CallHierarchyItem, CallHierarchyItemsOrNull,
    CallHierarchyOutgoingCallsOrNull, DocumentHighlightsOrNull, DocumentUri, LocationsOrNull,
    MarkupKind, Position, PositionEncodingKind, ReferenceContext, ReferenceParams, RenameParams,
    ResolvedClientCapabilities, SignatureHelpContext, SignatureHelpParams,
    SignatureHelpTriggerKind, TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile,
    TextDocumentIdentifier, TextEditOrAnnotatedTextEditOrSnippetTextEdit, WorkspaceEditOrNull,
};
use tsc_program::{load_program, LibraryCatalog, ProgramLoadLimits, ProgramOptions};
use tsc_types::CompilerOptions;

/// tsgo's `ScriptTargetLatestStandard` (ES2026), the fourslash default.
const LATEST_STANDARD: i32 = 13;
/// tsgo `JsxEmitPreserve`, the fourslash default.
const JSX_PRESERVE: i32 = 1;

/// A protocol range: (line, character) to (line, character).
type Range = ((u32, u32), (u32, u32));

/// A reference: its file and range.
type Reference = (&'static str, Range);

/// The fourslash options with `configure` applied.
fn options(configure: impl FnOnce(&mut CompilerOptions)) -> CompilerOptions {
    let mut options = CompilerOptions {
        target: Some(LATEST_STANDARD),
        jsx: Some(JSX_PRESERVE),
        skip_default_lib_check: Some(true),
        no_emit: Some(true),
        ..CompilerOptions::default()
    };
    configure(&mut options);
    options
}

fn libraries() -> &'static [(String, Vec<u8>)] {
    static FILES: OnceLock<Vec<(String, Vec<u8>)>> = OnceLock::new();
    FILES.get_or_init(|| {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../vendor/typescript-native/7.1.0-dev-aa814927/upstream/tsc/internal/bundled/libs",
        );
        let mut files = Vec::new();
        for entry in std::fs::read_dir(directory).expect("vendored TypeScript libraries") {
            let entry = entry.expect("library directory entry");
            let name = entry.file_name().into_string().expect("library filename");
            if name.starts_with("lib.") && name.ends_with(".d.ts") {
                files.push((
                    format!("/lib/{name}"),
                    std::fs::read(entry.path()).expect("library bytes"),
                ));
            }
        }
        files.sort();
        files
    })
}

/// The test's files as the language service's host.
struct Files(BTreeMap<String, String>);

impl SourceMapperHost for Files {
    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn read_file(&self, file_name: &str) -> Option<String> {
        self.0.get(file_name).cloned()
    }
}

/// Only module specifier completions read the file system.
impl tsc_ls::Host for Files {
    fn read_directory(
        &self,
        _: &str,
        _: &[&str],
        _: &[&str],
        _: &[&str],
        _: Option<usize>,
    ) -> Vec<String> {
        unreachable!("only module specifier completions read directories")
    }

    fn get_directories(&self, _: &str) -> Vec<String> {
        unreachable!("only module specifier completions read directories")
    }

    fn directory_exists(&self, _: &str) -> bool {
        unreachable!("only module specifier completions read directories")
    }

    fn file_exists(&self, _: &str) -> bool {
        unreachable!("only module specifier completions read directories")
    }
}

/// Run `run` over a language service of the files.
fn with_service<T>(
    files: &[(&str, &str)],
    options: CompilerOptions,
    run: impl FnOnce(&mut LanguageService<'_>, &Files) -> T,
) -> T {
    with_service_preferences(files, options, UserPreferences::default(), run)
}

/// [`with_service`] with the user's preferences.
fn with_service_preferences<T>(
    files: &[(&str, &str)],
    options: CompilerOptions,
    preferences: UserPreferences,
    run: impl FnOnce(&mut LanguageService<'_>, &Files) -> T,
) -> T {
    with_service_capabilities(
        files,
        options,
        preferences,
        ResolvedClientCapabilities::default(),
        run,
    )
}

/// [`with_service_preferences`] for a client with these capabilities.
fn with_service_capabilities<T>(
    files: &[(&str, &str)],
    options: CompilerOptions,
    preferences: UserPreferences,
    capabilities: ResolvedClientCapabilities,
    run: impl FnOnce(&mut LanguageService<'_>, &Files) -> T,
) -> T {
    let mut builder = MemoryCompilerHost::builder("/").case_sensitive(true);
    for (name, text) in files {
        builder = builder.file(PathBuf::from(name), text.as_bytes());
    }
    for (path, bytes) in libraries() {
        builder = builder.file(path.as_str(), bytes.as_slice());
    }
    let host = builder.build().expect("memory host");
    let roots = files
        .iter()
        .filter(|(name, _)| !name.ends_with("tsconfig.json"))
        .map(|(name, _)| PathBuf::from(name))
        .collect::<Vec<_>>();
    let prepared = load_program(
        &host,
        &roots,
        options,
        ProgramOptions::default(),
        &LibraryCatalog::typescript_7_1("/lib"),
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
    )
    .expect("load the program");
    let mut live = LiveProgram::new(prepared).expect("the live program");

    let texts: BTreeMap<String, String> = files
        .iter()
        .map(|(name, text)| ((*name).to_owned(), (*text).to_owned()))
        .collect();
    let line_maps = texts.clone();
    let converters = Converters::new(PositionEncodingKind::UTF16, move |file_name| {
        Arc::new(LspLineMap::new(
            line_maps.get(file_name).map_or("", String::as_str),
        ))
    });
    let files = Files(texts);
    let mut service = LanguageService::new(
        &mut live,
        &files,
        &converters,
        &preferences,
        &capabilities,
        None,
    );
    run(&mut service, &files)
}

/// A protocol range as `((line, character), (line, character))`.
fn range_of(range: &tsc_lsproto::Range) -> Range {
    (
        (range.start.line, range.start.character),
        (range.end.line, range.end.character),
    )
}

/// tsgo's document highlights at `(line, character)` of `file`: each
/// range with its kind (2 read, 3 write).
fn document_highlights(
    files: &[(&str, &str)],
    options: CompilerOptions,
    file: &str,
    (line, character): (u32, u32),
) -> Vec<(Range, u32)> {
    with_service(files, options, |service, _| {
        let response = service
            .provide_document_highlights(
                &DocumentUri(format!("file://{file}")),
                &Position { line, character },
            )
            .expect("an answer");
        let DocumentHighlightsOrNull::DocumentHighlights(highlights) = response else {
            panic!("highlights");
        };
        highlights
            .iter()
            .map(|highlight| {
                (
                    range_of(&highlight.range),
                    highlight.kind.map_or(0, |kind| kind.0),
                )
            })
            .collect()
    })
}

/// tsgo's references at `(line, character)` of `file`.
fn references(
    files: &[(&str, &str)],
    options: CompilerOptions,
    file: &str,
    (line, character): (u32, u32),
) -> Vec<Reference> {
    with_service(files, options, |service, files| {
        let params = ReferenceParams {
            text_document: TextDocumentIdentifier {
                uri: DocumentUri(format!("file://{file}")),
            },
            position: Position { line, character },
            work_done_token: None,
            partial_result_token: None,
            context: ReferenceContext {
                include_declaration: true,
            },
        };
        let LocationsOrNull::Locations(locations) =
            service.provide_references(&params).expect("an answer")
        else {
            panic!("locations");
        };
        locations
            .iter()
            .map(|location| {
                let target = location
                    .uri
                    .as_str()
                    .strip_prefix("file://")
                    .expect("a file URI");
                let target = files
                    .0
                    .keys()
                    .find(|name| name.as_str() == target)
                    .map_or("<other>", |name| leak(name));
                (
                    target,
                    (
                        (location.range.start.line, location.range.start.character),
                        (location.range.end.line, location.range.end.character),
                    ),
                )
            })
            .collect()
    })
}

/// tsgo's prepare rename at `(line, character)` of `file`: the span and
/// placeholder, else the reason.
fn prepare_rename(
    files: &[(&str, &str)],
    file: &str,
    (line, character): (u32, u32),
) -> Result<(Range, String), String> {
    with_service(files, options(|_| {}), |service, _| {
        let info = service
            .get_rename_info(
                "",
                &DocumentUri(format!("file://{file}")),
                &Position { line, character },
            )
            .expect("an answer");
        if info.can_rename {
            Ok((range_of(&info.trigger_span), info.display_name))
        } else {
            Err(info.localized_error_message)
        }
    })
}

/// tsgo's rename edits at `(line, character)` of `file`: each edit's file,
/// range and text.
fn rename(
    files: &[(&str, &str)],
    file: &str,
    (line, character): (u32, u32),
    new_name: &str,
) -> Vec<(String, Range, String)> {
    with_service(files, options(|_| {}), |service, _| {
        let params = RenameParams {
            text_document: TextDocumentIdentifier {
                uri: DocumentUri(format!("file://{file}")),
            },
            position: Position { line, character },
            work_done_token: None,
            new_name: new_name.to_owned(),
        };
        let WorkspaceEditOrNull::WorkspaceEdit(edit) =
            service.provide_rename(&params).expect("an answer")
        else {
            panic!("an edit");
        };
        edit.changes
            .unwrap_or_default()
            .into_iter()
            .flat_map(|(uri, edits)| {
                edits.into_iter().map(move |edit| {
                    (
                        uri.as_str().trim_start_matches("file://").to_owned(),
                        range_of(&edit.range),
                        edit.new_text,
                    )
                })
            })
            .collect()
    })
}

/// tsgo's edits for renaming `old` to `new` (`workspace/willRenameFiles`):
/// each edit's file, range and text.
fn file_rename(files: &[(&str, &str)], old: &str, new: &str) -> Vec<(String, Range, String)> {
    with_service(files, options(|_| {}), |service, _| {
        service
            .get_edits_for_file_rename(
                &DocumentUri(format!("file://{old}")),
                &DocumentUri(format!("file://{new}")),
            )
            .expect("an answer")
            .into_iter()
            .flat_map(|change| {
                let TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile::TextDocumentEdit(edit) =
                    change
                else {
                    panic!("text edits only");
                };
                let file = edit
                    .text_document
                    .uri
                    .as_str()
                    .trim_start_matches("file://")
                    .to_owned();
                edit.edits.into_iter().map(move |edit| {
                    let TextEditOrAnnotatedTextEditOrSnippetTextEdit::TextEdit(edit) = edit else {
                        panic!("a text edit");
                    };
                    (file.clone(), range_of(&edit.range), edit.new_text)
                })
            })
            .collect()
    })
}

/// tsgo's call hierarchy items at `(line, character)` of `file`.
fn prepare_call_hierarchy(
    files: &[(&str, &str)],
    file: &str,
    (line, character): (u32, u32),
) -> Vec<CallHierarchyItem> {
    with_service(files, options(|_| {}), |service, _| {
        match service
            .provide_prepare_call_hierarchy(
                &DocumentUri(format!("file://{file}")),
                &Position { line, character },
            )
            .expect("an answer")
        {
            CallHierarchyItemsOrNull::CallHierarchyItems(items) => items,
            CallHierarchyItemsOrNull::Null => Vec::new(),
        }
    })
}

/// A call: the other item's name and name range, and the call's ranges.
type Call = (String, Range, Vec<Range>);

/// tsgo's calls into `item`.
fn incoming_calls(files: &[(&str, &str)], item: &CallHierarchyItem) -> Vec<Call> {
    with_service(files, options(|_| {}), |service, _| {
        match service
            .provide_call_hierarchy_incoming_calls(item)
            .expect("an answer")
        {
            CallHierarchyIncomingCallsOrNull::CallHierarchyIncomingCalls(calls) => calls
                .iter()
                .map(|call| {
                    (
                        call.from.name.clone(),
                        range_of(&call.from.selection_range),
                        call.from_ranges.iter().map(range_of).collect(),
                    )
                })
                .collect(),
            CallHierarchyIncomingCallsOrNull::Null => Vec::new(),
        }
    })
}

/// tsgo's calls out of `item`.
fn outgoing_calls(files: &[(&str, &str)], item: &CallHierarchyItem) -> Vec<Call> {
    with_service(files, options(|_| {}), |service, _| {
        match service
            .provide_call_hierarchy_outgoing_calls(item)
            .expect("an answer")
        {
            CallHierarchyOutgoingCallsOrNull::CallHierarchyOutgoingCalls(calls) => calls
                .iter()
                .map(|call| {
                    (
                        call.to.name.clone(),
                        range_of(&call.to.selection_range),
                        call.from_ranges.iter().map(range_of).collect(),
                    )
                })
                .collect(),
            CallHierarchyOutgoingCallsOrNull::Null => Vec::new(),
        }
    })
}

/// A workspace symbol: its name, kind, container and range.
type WorkspaceSymbol = (String, u32, Option<String>, Range);

/// tsgo's workspace symbols matching `query`, with tsgo's default
/// preferences (no library symbols).
fn workspace_symbols(files: &[(&str, &str)], query: &str) -> Vec<WorkspaceSymbol> {
    with_service_preferences(
        files,
        options(|_| {}),
        UserPreferences::new_default(),
        |service, _| {
            let matches = service.workspace_symbol_matches(query).expect("an answer");
            let tsc_lsproto::SymbolInformationsOrWorkspaceSymbolsOrNull::SymbolInformations(
                symbols,
            ) = tsc_ls::workspace_symbols(vec![matches])
            else {
                panic!("symbol informations");
            };
            symbols
                .into_iter()
                .map(|symbol| {
                    (
                        symbol.name,
                        symbol.kind.0,
                        symbol.container_name,
                        range_of(&symbol.location.range),
                    )
                })
                .collect()
        },
    )
}

/// tsgo's signature help at a position, as the protocol's JSON, for the
/// fourslash client (per-signature active parameters, markdown
/// documentation).
fn signature_help(
    files: &[(&str, &str)],
    options: CompilerOptions,
    (line, character): (u32, u32),
    context: Option<SignatureHelpContext>,
) -> serde_json::Value {
    let mut capabilities = ResolvedClientCapabilities::default();
    let information = &mut capabilities
        .text_document
        .signature_help
        .signature_information;
    information.active_parameter_support = true;
    information.documentation_format = vec![MarkupKind::MARKDOWN, MarkupKind::PLAIN_TEXT];
    let uri = DocumentUri(format!("file://{}", files[0].0));
    with_service_capabilities(
        files,
        options,
        UserPreferences::default(),
        capabilities,
        |service, _| {
            let params = SignatureHelpParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line, character },
                work_done_token: None,
                context,
            };
            serde_json::to_value(service.provide_signature_help(&params).expect("an answer"))
                .expect("JSON")
        },
    )
}

/// tsgo's document symbols of the first file, as the protocol's JSON, for a
/// client with hierarchical document symbols.
fn document_symbols(files: &[(&str, &str)]) -> serde_json::Value {
    let mut capabilities = ResolvedClientCapabilities::default();
    capabilities
        .text_document
        .document_symbol
        .hierarchical_document_symbol_support = true;
    let uri = DocumentUri(format!("file://{}", files[0].0));
    with_service_capabilities(
        files,
        options(|_| {}),
        UserPreferences::default(),
        capabilities,
        |service, _| {
            serde_json::to_value(service.provide_document_symbols(&uri).expect("an answer"))
                .expect("JSON")
        },
    )
}

/// tsgo's folding ranges of the first file, as the protocol's JSON, for a
/// client that shows collapsed text.
fn folding_ranges(files: &[(&str, &str)]) -> serde_json::Value {
    let mut capabilities = ResolvedClientCapabilities::default();
    capabilities
        .text_document
        .folding_range
        .folding_range
        .collapsed_text = true;
    let uri = DocumentUri(format!("file://{}", files[0].0));
    with_service_capabilities(
        files,
        options(|options| options.allow_js = true),
        UserPreferences::default(),
        capabilities,
        |service, _| {
            serde_json::to_value(service.provide_folding_ranges(&uri).expect("an answer"))
                .expect("JSON")
        },
    )
}

/// tsgo's semantic tokens of the first file (of a range of it), as the
/// protocol's JSON, for a client with every token type and modifier.
fn semantic_tokens(files: &[(&str, &str)], range: Option<tsc_lsproto::Range>) -> serde_json::Value {
    let mut capabilities = ResolvedClientCapabilities::default();
    let semantic_tokens = &mut capabilities.text_document.semantic_tokens;
    semantic_tokens.token_types = tsc_ls::TOKEN_TYPES
        .iter()
        .map(|kind| kind.0.to_string())
        .collect();
    semantic_tokens.token_modifiers = tsc_ls::TOKEN_MODIFIERS
        .iter()
        .map(|modifier| modifier.0.to_string())
        .collect();
    let uri = DocumentUri(format!("file://{}", files[0].0));
    with_service_capabilities(
        files,
        options(|options| options.allow_js = true),
        UserPreferences::default(),
        capabilities,
        |service, _| {
            let response = match &range {
                Some(range) => service.provide_semantic_tokens_range(&uri, range),
                None => service.provide_semantic_tokens(&uri),
            };
            serde_json::to_value(response.expect("an answer")).expect("JSON")
        },
    )
}

/// tsgo's inlay hints of the whole first file for a user who enables every
/// kind (parameter names of all arguments): each hint's position, label text
/// and kind, and the locations of its label's parts.
#[allow(clippy::type_complexity)]
fn inlay_hints(
    files: &[(&str, &str)],
) -> Vec<((u32, u32), String, Option<u32>, Vec<tsc_lsproto::Location>)> {
    let mut preferences = UserPreferences::default();
    let hints = &mut preferences.inlay_hints_preferences;
    hints.include_inlay_variable_type_hints = Some(true);
    hints.include_inlay_function_like_return_type_hints = Some(true);
    hints.include_inlay_function_parameter_type_hints = Some(true);
    hints.include_inlay_parameter_name_hints = tsc_ls::lsutil::IncludeInlayParameterNameHints::ALL;
    hints.include_inlay_enum_member_value_hints = Some(true);
    let uri = DocumentUri(format!("file://{}", files[0].0));
    let lines = files[0].1.matches('\n').count() as u32;
    with_service_preferences(
        files,
        options(|options| options.allow_js = true),
        preferences,
        |service, _| {
            let params = tsc_lsproto::InlayHintParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                range: tsc_lsproto::Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: lines,
                        character: 0,
                    },
                },
                ..Default::default()
            };
            let tsc_lsproto::InlayHintsOrNull::InlayHints(hints) =
                service.provide_inlay_hints(&params).expect("an answer")
            else {
                panic!("inlay hints");
            };
            hints
                .into_iter()
                .map(|hint| {
                    let (text, locations) = match hint.label {
                        tsc_lsproto::StringOrInlayHintLabelParts::String(text) => {
                            (text, Vec::new())
                        }
                        tsc_lsproto::StringOrInlayHintLabelParts::InlayHintLabelParts(parts) => (
                            parts.iter().map(|part| part.value.as_str()).collect(),
                            parts.into_iter().filter_map(|part| part.location).collect(),
                        ),
                    };
                    (
                        (hint.position.line, hint.position.character),
                        text,
                        hint.kind.map(|kind| kind.0),
                        locations,
                    )
                })
                .collect()
        },
    )
}

fn leak(name: &str) -> &'static str {
    Box::leak(name.to_owned().into_boxed_str())
}

/// findAllReferencesLinkTag1: a member's references include the names of
/// `{@link}` and `@see` tags: unqualified (the class's members), qualified
/// and `C#m` (tsgo parses both as qualified names).
#[test]
fn link_tag_names_are_references() {
    let text = "class C {\n    m() { }\n    n = 1\n    static s() { }\n    /**\n     * {@link m}\n     * @see {m}\n     * {@link C.m}\n     * @see {C.m}\n     * {@link C#m}\n     * @see {C#m}\n     * {@link C.prototype.m}\n     * @see {C.prototype.m}\n     */\n    p() { }\n    /**\n     * {@link n}\n     * @see {n}\n     * {@link C.n}\n     * @see {C.n}\n     * {@link C#n}\n     * @see {C#n}\n     * {@link C.prototype.n}\n     * @see {C.prototype.n}\n     */\n    q() { }\n    /**\n     * {@link s}\n     * @see {s}\n     * {@link C.s}\n     * @see {C.s}\n     */\n    r() { }\n}\n\ninterface I {\n    a()\n    b: 1\n    /**\n     * {@link a}\n     * @see {a}\n     * {@link I.a}\n     * @see {I.a}\n     * {@link I#a}\n     * @see {I#a}\n     */\n    c()\n    /**\n     * {@link b}\n     * @see {b}\n     * {@link I.b}\n     * @see {I.b}\n     */\n    d()\n}\n\nfunction nestor() {\n    /** {@link r2} */\n    function ref() { }\n    /** @see {r2} */\n    function d3() { }\n    function r2() { }\n}";
    assert_eq!(
        references(
            &[("/findAllReferencesLinkTag1.ts", text)],
            options(|_| {}),
            "/findAllReferencesLinkTag1.ts",
            (1, 5)
        ),
        vec![
            ("/findAllReferencesLinkTag1.ts", ((1, 4), (1, 5))),
            ("/findAllReferencesLinkTag1.ts", ((5, 14), (5, 15))),
            ("/findAllReferencesLinkTag1.ts", ((6, 13), (6, 14))),
            ("/findAllReferencesLinkTag1.ts", ((7, 16), (7, 17))),
            ("/findAllReferencesLinkTag1.ts", ((8, 15), (8, 16))),
            ("/findAllReferencesLinkTag1.ts", ((9, 16), (9, 17))),
            ("/findAllReferencesLinkTag1.ts", ((10, 15), (10, 16))),
            ("/findAllReferencesLinkTag1.ts", ((11, 26), (11, 27))),
            ("/findAllReferencesLinkTag1.ts", ((12, 25), (12, 26))),
        ]
    );
}

/// findAllReferencesJsDocTypeLiteral: the last name of a nested `@param`
/// is the reparsed property's name (tsgo `GetReparsedNodeForNode`).
#[test]
fn a_nested_param_name_is_its_property() {
    assert_eq!(
        references(
            &[("/foo.js", "/**\n * @param {object} o - very important!\n * @param {string} o.x - a thing, its ok\n * @param {number} o.y - another thing\n * @param {Object} o.nested - very nested\n * @param {boolean} o.nested.great - much greatness\n * @param {number} o.nested.times - twice? probably!??\n */\n function f(o) { return o.nested.great; }")],
            options(|options| options.allow_js = true),
            "/foo.js",
            (5, 29)
        ),
        vec![
            ("/foo.js", ((5, 29), (5, 34))),
            ("/foo.js", ((8, 33), (8, 38))),
        ]
    );
}

/// findAllReferencesJsOverloadedFunctionParameter: an `@overload` signature's\n/// parameters are the reparsed overload's, not the implementation's.
#[test]
fn overload_parameters_are_their_own() {
    assert_eq!(
        references(
            &[
                ("/foo.js", "/**\n * @overload\n * @param {number} x\n * @returns {number}\n *\n * @overload\n * @param {string} x\n * @returns {string} \n *\n * @param {unknown} x\n * @returns {unknown} \n */\nfunction foo(x) {\n  return x;\n}")
            ],
            options(|options| options.allow_js = true),
            "/foo.js",
            (12, 14)
        ),
        vec![
            ("/foo.js", ((9, 20), (9, 21))),
            ("/foo.js", ((12, 13), (12, 14))),
            ("/foo.js", ((13, 9), (13, 10))),
        ]
    );
}

/// findAllRefsJSDocNamespacedTypedef: a namespaced typedef's name and the\n/// qualified names that refer to it.
#[test]
fn a_namespaced_typedef_is_referenced_by_its_qualified_name() {
    assert_eq!(
        references(
            &[
                ("/index.js", "// Namespaced typedef\n/** @typedef {string} NS.T */\n\n// Namespaced typedef aliased to qualified namespaced typedef.\n/** @typedef {NS.T} NS.U */\n\n// Namespaced typedef aliased to implicitly-resolved typedef.\n/** @typedef {U} NS.V */\n")
            ],
            options(|options| options.allow_js = true),
            "/index.js",
            (1, 25)
        ),
        vec![
            ("/index.js", ((1, 25), (1, 26))),
            ("/index.js", ((4, 17), (4, 18))),
        ]
    );
}

/// findAllRefsJsDocImportTag2: an `@import` of a default export and its uses.
#[test]
fn a_jsdoc_import_imports_a_default_export() {
    assert_eq!(
        references(
            &[
                ("/component.js", "export default class Component {\n  constructor() {\n    this.id_ = Math.random();\n  }\n  id() {\n    return this.id_;\n  }\n}"),
                ("/spatial-navigation.js", "/** @import Component from './component.js' */\n\nexport class SpatialNavigation {\n  /**\n   * @param {Component} component\n   */\n  add(component) {}\n}"),
                ("/player.js", "import Component from './component.js';\n\n/**\n * @extends Component\n */\nexport class Player extends Component {}")
            ],
            options(|options| options.allow_js = true),
            "/player.js",
            (3, 21)
        ),
        vec![
            ("/player.js", ((0, 7), (0, 16))),
            ("/player.js", ((3, 12), (3, 21))),
            ("/player.js", ((5, 28), (5, 37))),
            ("/component.js", ((0, 21), (0, 30))),
            ("/spatial-navigation.js", ((0, 12), (0, 21))),
            ("/spatial-navigation.js", ((4, 13), (4, 22))),
        ]
    );
}

/// findAllRefsTypedef_importType: an import type's qualifier names a\n/// CommonJS module's typedef.
#[test]
fn an_import_type_refers_to_a_commonjs_typedef() {
    assert_eq!(
        references(
            &[
                (
                    "/a.js",
                    "module.exports = 0;\n/** @typedef {number} Foo */\nconst dummy = 0;"
                ),
                ("/b.js", "/** @type {import('./a').Foo} */\nconst x = 0;")
            ],
            options(|options| options.allow_js = true),
            "/a.js",
            (1, 22)
        ),
        vec![("/a.js", ((1, 22), (1, 25))), ("/b.js", ((0, 25), (0, 28))),]
    );
}

/// findAllRefs_importType_js4.
#[test]
fn import_type_js4() {
    assert_eq!(
        references(
            &[
                ("/a.js", "/**\n * @callback A\n * @param {unknown} response\n */\n\nmodule.exports = {};"),
                ("/b.js", "/** @typedef {import(\"./a\").A} A */")
            ],
            options(|options| options.allow_js = true),
            "/a.js",
            (1, 13)
        ),
        vec![
            ("/a.js", ((1, 13), (1, 14))),
            ("/b.js", ((0, 28), (0, 29))),
        ]
    );
}

/// jsdocSatisfiesTagFindAllReferences: a `@satisfies` tag's type refers to a\n/// typedef.
#[test]
fn a_satisfies_tag_refers_to_a_typedef() {
    assert_eq!(
        references(
            &[
                ("/a.js", "/**\n * @typedef {Object} T\n * @property {number} a\n */\n\n/** @satisfies {T} comment */\nconst foo = { a: 1 };")
            ],
            options(|options| options.allow_js = true),
            "/a.js",
            (5, 16)
        ),
        vec![
            ("/a.js", ((1, 21), (1, 22))),
            ("/a.js", ((5, 16), (5, 17))),
        ]
    );
}

/// jsdocTypedefTagSemanticMeaning0.
#[test]
fn typedef_semantic_meaning_0_2() {
    assert_eq!(
        references(
            &[(
                "/a.js",
                "/** @typedef {number} T */\nconst T = 1;\n/** @type {T} */\nconst n = T;"
            )],
            options(|options| options.allow_js = true),
            "/a.js",
            (0, 22)
        ),
        vec![("/a.js", ((0, 22), (0, 23))), ("/a.js", ((2, 11), (2, 12))),]
    );
}

/// jsdocTypedefTagServices.
#[test]
fn typedef_tag_services_2() {
    assert_eq!(
        references(
            &[
                ("/a.js", "/**\n * Doc comment\n * @typedef Product\n * @property {string} title\n */\n/**\n * @type {Product}\n */\nconst product = null;")
            ],
            options(|options| options.allow_js = true),
            "/a.js",
            (6, 10)
        ),
        vec![
            ("/a.js", ((2, 12), (2, 19))),
            ("/a.js", ((6, 10), (6, 17))),
        ]
    );
}

/// patternAmbientModuleWithImportAttributesLanguageService: a pattern\n/// module's export is searched in its file, then in the importers'.
#[test]
fn a_pattern_module_member_is_searched_in_its_file_first() {
    assert_eq!(
        references(
            &[
                ("/tsconfig.json", "{ \"compilerOptions\": { \"module\": \"preserve\", \"moduleResolution\": \"bundler\" } }\n"),
                ("/types.d.ts", "declare module \"*.asset\" with { type: \"css\" } {\n    export interface CssPayload { kind: \"css\" }\n    export const shared: CssPayload;\n}\ndeclare module \"*.asset\" with { type: \"text\" } {\n    export interface TextPayload { kind: \"text\" }\n    export const shared: TextPayload;\n}\n"),
                ("/index.ts", "import * as css from \"./style.asset\" with { type: \"css\" };\nimport * as text from \"./copy.asset\" with { type: \"text\" };\ncss.shared;\ntext.shared;")
            ],
            options(|_| {}),
            "/index.ts",
            (2, 4)
        ),
        vec![
            ("/types.d.ts", ((2, 17), (2, 23))),
            ("/index.ts", ((2, 4), (2, 10))),
        ]
    );
}

/// documentHighlightTypeofThis: `typeof this` in an interface with no other\n/// `this` (a thisless interface) has no this type, so no symbol and no\n/// highlights (the checker took the declared type for a generic one).
#[test]
fn typeof_this_in_a_thisless_interface_has_no_highlights() {
    assert_eq!(
        document_highlights(
            &[("/a.ts", "interface Foo {\n  bar(): typeof this;\n}\n")],
            options(|_| {}),
            "/a.ts",
            (1, 16)
        ),
        Vec::<(Range, u32)>::new()
    );
}

/// getOccurrencesTryCatchFinallyBroken: a missing (zero-width) keyword's\n/// highlight starts at its position, not at the next token.
#[test]
fn a_missing_keyword_is_highlighted_where_it_is_missing() {
    assert_eq!(
        document_highlights(
            &[
                ("/getOccurrencesTryCatchFinallyBroken.ts", "t ry {\n    try {\n    }\n    ctch (x) {\n    }\n\n    tr {\n    }\n    finally {\n    }\n}\ncatch (e) {\n}\nfinally {\n}\n\n// Missing catch variable\ntry {\n}\ncatch {\n}\nfinally {\n}\n\n// Missing try entirely\ncatch (x) {\n}\nfinally {\n}")
            ],
            options(|_| {}),
            "/getOccurrencesTryCatchFinallyBroken.ts",
            (8, 7)
        ),
        vec![
            (((7, 5), (7, 5)), 2),
        ]
    );
}

/// renameBuiltinTypes: a default library's type cannot be renamed.
#[test]
fn library_types_cannot_be_renamed() {
    let text = "\nconst arr: Array<number> = [];\nconst map1: Map<string, number> = new Map();\nconst prom: Promise<void> = Promise.resolve();\nconst str: string = \"hello\";\n";
    assert_eq!(
        prepare_rename(
            &[("/renameBuiltinTypes.ts", text)],
            "/renameBuiltinTypes.ts",
            (1, 11)
        ),
        Err(
            "You cannot rename elements that are defined in the standard TypeScript library."
                .to_owned()
        )
    );
}

/// renameModuleExportsProperties1: a shorthand property of `module.exports`
/// keeps its exported name.
#[test]
fn module_exports_shorthand_keeps_its_name() {
    let file = "/renameModuleExportsProperties1.ts";
    assert_eq!(
        rename(
            &[(file, "class A {}\nmodule.exports = { A }")],
            file,
            (0, 6),
            "?"
        ),
        [
            (file.to_owned(), ((0, 6), (0, 7)), "?".to_owned()),
            (file.to_owned(), ((1, 19), (1, 20)), "A: ?".to_owned()),
        ]
    );
}

/// renameNumericalIndex: a numeric index renamed to a name is quoted (with
/// the default quote preference, double quotes).
#[test]
fn numeric_index_is_quoted() {
    let file = "/renameNumericalIndex.ts";
    assert_eq!(
        rename(
            &[(file, "const foo = { 0: true };\nfoo[0];")],
            file,
            (0, 14),
            "?"
        ),
        [
            (file.to_owned(), ((0, 14), (0, 15)), "?".to_owned()),
            (file.to_owned(), ((1, 4), (1, 5)), "\"?\"".to_owned()),
        ]
    );
}

/// getEditsForFileRename_subDir: an unresolved relative import of a moved
/// file is written from its new directory.
#[test]
fn moved_file_keeps_its_unresolved_relative_import() {
    assert_eq!(
        file_rename(
            &[("/src/old.ts", "import a from \"./foo/a\";")],
            "/src/old.ts",
            "/src/dir/new.ts"
        ),
        [(
            "/src/old.ts".to_owned(),
            ((0, 15), (0, 22)),
            "../foo/a".to_owned()
        )]
    );
}

/// callHierarchyClassStaticBlock: a call in a class static block comes from
/// the block, named `static {}` (tsgo finds no symbol at a class node), and
/// a function's calls out are grouped by the function they call.
#[test]
fn calls_in_a_class_static_block() {
    let file = "/callHierarchyClassStaticBlock.ts";
    let text = "class C {\n    static {\n        function foo() {\n            bar();\n        }\n\n        function bar() {\n            baz();\n            quxx();\n            baz();\n        }\n\n        foo();\n    }\n}\n\nfunction baz() {\n}\n\nfunction quxx() {\n}";
    let files = [(file, text)];
    let foo = prepare_call_hierarchy(&files, file, (2, 17));
    assert_eq!(
        foo.iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["foo"]
    );
    assert_eq!(
        incoming_calls(&files, &foo[0]),
        [(
            "static {}".to_owned(),
            ((1, 4), (1, 10)),
            vec![((12, 8), (12, 11))]
        )]
    );
    let bar = prepare_call_hierarchy(&files, file, (6, 17));
    assert_eq!(
        outgoing_calls(&files, &bar[0]),
        [
            (
                "baz".to_owned(),
                ((16, 9), (16, 12)),
                vec![((7, 12), (7, 15)), ((9, 12), (9, 15))]
            ),
            (
                "quxx".to_owned(),
                ((19, 9), (19, 13)),
                vec![((8, 12), (8, 16))]
            ),
        ]
    );
}

/// navigationItemsInConstructorsExactMatch: a constructor's parameter
/// properties are symbols of its class (a plain parameter is not).
#[test]
fn parameter_properties_are_workspace_symbols() {
    let file = "/navigationItemsInConstructorsExactMatch.ts";
    let text = "class Test {\n    private search1: number;\n    constructor(public search2: boolean, readonly search3: string, search4: string) {\n    }\n}";
    let test = Some("Test".to_owned());
    assert_eq!(
        workspace_symbols(&[(file, text)], "search"),
        [
            ("search1".to_owned(), 7, test.clone(), ((1, 12), (1, 19))),
            ("search2".to_owned(), 7, test.clone(), ((2, 23), (2, 30))),
            ("search3".to_owned(), 7, test, ((2, 50), (2, 57))),
        ]
    );
}

/// navigateToSymbolIterator: a computed name's property access is named by
/// its property.
#[test]
fn a_computed_property_access_name_is_its_property() {
    let file = "/navigateToSymbolIterator.ts";
    assert_eq!(
        workspace_symbols(
            &[(file, "class C {\n    [Symbol.iterator]() {}\n}")],
            "iterator"
        ),
        [(
            "iterator".to_owned(),
            6,
            Some("C".to_owned()),
            ((1, 4), (1, 21))
        )]
    );
}

/// TestTrailingCommaSignatureHelp: a trailing comma counts an argument, so
/// signature help's resolution picks the overload with two parameters.
#[test]
fn a_trailing_comma_counts_an_argument_in_signature_help() {
    let text = "function str(n: number): string;\n/**\n * Stringifies a number with radix\n * @param radix The radix\n */\nfunction str(n: number, radix: number): string;\nfunction str(n: number, radix?: number): string { return \"\"; }\n\nstr(1, )\n\ndeclare function f<T>(a: T): T;\nf(2, );";
    let help = signature_help(
        &[("/trailingCommaSignatureHelp.ts", text)],
        options(|_| {}),
        (8, 7),
        None,
    );
    assert_eq!(
        help,
        serde_json::json!({"signatures": [{"label": "str(n: number): string", "parameters": [{"label": "n: number"}], "activeParameter": 1}, {"label": "str(n: number, radix: number): string", "documentation": {"kind": "markdown", "value": "Stringifies a number with radix"}, "parameters": [{"label": "n: number"}, {"label": "radix: number", "documentation": {"kind": "markdown", "value": "The radix"}}], "activeParameter": 1}], "activeSignature": 1})
    );
}

/// TestSignatureHelpNestedCallTrailingComma: an outer call that fails its
/// arity only by signature help's trailing comma reports the call (tsgo
/// 7.1's guard), not an excess argument past the arguments.
#[test]
fn a_nested_call_with_trailing_commas_has_signature_help() {
    let help = signature_help(
        &[(
            "/signatureHelpNestedCallTrailingComma.ts",
            "declare function outer<T>(range: T): T;\ndeclare function inner(a: any): any;\n\nouter(inner(undefined,),);",
        )],
        options(|_| {}),
        (3, 11),
        Some(SignatureHelpContext {
            trigger_kind: SignatureHelpTriggerKind::INVOKED,
            trigger_character: None,
            is_retrigger: false,
            active_signature_help: None,
        }),
    );
    assert_eq!(
        help,
        serde_json::json!({"signatures": [{"label": "outer(range: any): any", "parameters": [{"label": "range: any"}], "activeParameter": 0}], "activeSignature": 0})
    );
}

/// TestJsSignature_41059: in JavaScript, an unresolved method call finds a
/// declaration of the name with call signatures (the library's
/// `Generator.next`); its rest tuple union expands into a signature per
/// tuple, the element named by the rest parameter's binding name.
#[test]
fn an_unresolved_javascript_call_finds_a_declaration_of_its_name() {
    let help = signature_help(
        &[("/Foo.js", "a.next();")],
        options(|options| options.allow_js = true),
        (0, 7),
        None,
    );
    assert_eq!(
        help,
        serde_json::json!({"signatures": [{"label": "Generator.next(): IteratorResult<T, TReturn>", "parameters": []}, {"label": "Generator.next(value: TNext): IteratorResult<T, TReturn>", "parameters": [{"label": "value: TNext"}], "activeParameter": 0}], "activeSignature": 0})
    );
}

/// TestNavigationBarWellKnownSymbolExpando: a function expression assigned
/// to an element access of a computed key has no assigned name (tsgo 7.1
/// `GetAssignedName`), so it is `<function>` under the expando's symbol.
#[test]
fn an_expando_with_a_computed_key_holds_an_unnamed_function() {
    let symbols = document_symbols(&[(
        "/navigationBarWellKnownSymbolExpando.ts",
        "function f() {}\nf[Symbol.iterator] = function() {}",
    )]);
    assert_eq!(
        symbols,
        serde_json::json!([{"name": "f", "kind": 12, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 15}}, "selectionRange": {"start": {"line": 0, "character": 9}, "end": {"line": 0, "character": 10}}, "children": [{"name": "Symbol.iterator", "kind": 13, "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 18}}, "selectionRange": {"start": {"line": 1, "character": 2}, "end": {"line": 1, "character": 17}}, "children": [{"name": "<function>", "kind": 12, "range": {"start": {"line": 1, "character": 21}, "end": {"line": 1, "character": 34}}, "selectionRange": {"start": {"line": 1, "character": 21}, "end": {"line": 1, "character": 21}}, "children": []}]}]}])
    );
}

/// tsgo lists the declarations it reparses from a statement's JSDoc in a
/// JavaScript file (a `@typedef` or `@callback` with a type, an `@import`)
/// before the statement, and they are not imports: one before an import ends
/// a run of imports. The next run starts where tsgo's `FindChildOfKind`
/// leaves its stale `import` token, at the end of the JSDoc comment. A
/// `@typedef` without a type, the end of the file's JSDoc and a TypeScript
/// file's JSDoc reparse nothing. (Expected: tsgo's answers.)
#[test]
fn a_reparsed_js_doc_declaration_ends_a_run_of_imports() {
    let x = ("/x.js", "export default 1;\nexport const X = 1;\n");
    let range = |start: (u32, u32), end: (u32, u32), kind: &str| serde_json::json!({"startLine": start.0, "startCharacter": start.1, "endLine": end.0, "endCharacter": end.1, "kind": kind});
    let cases = [
        (
            "/a.js",
            "import a from \"./x\";\n/** @typedef {string} T */\nimport b from \"./x\";\nimport c from \"./x\";\n",
            vec![
                range((1, 0), (1, 26), "comment"),
                range((1, 26), (3, 20), "imports"),
            ],
        ),
        (
            "/b.js",
            "import a from \"./x\";\n/** @import { X } from \"./x\" */\nimport b from \"./x\";\nimport c from \"./x\";\n",
            vec![
                range((1, 0), (1, 31), "comment"),
                range((1, 31), (3, 20), "imports"),
            ],
        ),
        (
            "/c.js",
            "import a from \"./x\";\nimport b from \"./x\";\n/** @import { X } from \"./x\" */\n",
            vec![
                range((0, 0), (1, 20), "imports"),
                range((2, 0), (2, 31), "comment"),
            ],
        ),
        (
            "/d.js",
            "import a from \"./x\";\n/** @typedef T */\nimport b from \"./x\";\n",
            vec![
                range((0, 0), (2, 20), "imports"),
                range((1, 0), (1, 17), "comment"),
            ],
        ),
        (
            "/e.ts",
            "import a from \"./x\";\n/** @typedef {string} T */\nimport b from \"./x\";\n",
            vec![
                range((0, 0), (2, 20), "imports"),
                range((1, 0), (1, 26), "comment"),
            ],
        ),
    ];
    for (name, text, expected) in cases {
        assert_eq!(
            folding_ranges(&[(name, text), x]),
            serde_json::Value::Array(expected),
            "{name}"
        );
    }
}

/// In a JavaScript file tsgo's parser turns a member's `@readonly` into a
/// modifier of a class's property, method or accessor — not of one parsed
/// in an object literal, even in a class there — while a variable's tag is no
/// modifier: the tokens of the reparsed members are readonly. (Expected:
/// tsgo's answer.)
#[test]
fn reparsed_js_doc_modifiers_make_readonly_tokens() {
    let text = "class C {\n  /** @readonly */\n  x = 1;\n  /** @readonly */\n  get g() { return 1; }\n  m() { return this.x + this.g; }\n}\nconst o = {\n  /** @readonly */\n  get y() { return 1; },\n  k: class {\n    /** @readonly */\n    get z() { return 1; }\n  },\n};\n/** @readonly */\nlet w = o.y;\nw;\n";
    assert_eq!(
        semantic_tokens(&[("/a.js", text)], None),
        serde_json::json!({"data": [0, 6, 1, 1, 1, 2, 2, 1, 9, 5, 2, 6, 1, 9, 5, 1, 2, 1, 14, 1, 0, 18, 1, 9, 4, 0, 9, 1, 9, 4, 2, 6, 1, 8, 5, 2, 6, 1, 9, 1, 1, 2, 1, 1, 1, 2, 8, 1, 9, 1, 4, 4, 1, 8, 1, 0, 4, 1, 8, 4, 0, 2, 1, 9, 0, 1, 0, 1, 8, 0]})
    );
}

/// A range's tokens are the identifiers that overlap it (not a parameter
/// that ends at its start, nor a name that starts at its end); `Math.max`
/// is from the default library. (Expected: tsgo's answers.)
#[test]
fn semantic_tokens_of_a_range_and_of_the_default_library() {
    let text = "const a = 1;\nfunction f(p: number) { return p + a; }\nlet b = f(a);\nclass D { static s = Math.max(b, 2); }\n";
    let position = |line, character| Position { line, character };
    assert_eq!(
        semantic_tokens(
            &[("/b.ts", text)],
            Some(tsc_lsproto::Range {
                start: position(1, 20),
                end: position(3, 12),
            })
        ),
        serde_json::json!({"data": [1, 31, 1, 7, 0, 0, 4, 1, 8, 4, 1, 4, 1, 8, 1, 0, 4, 1, 13, 0, 0, 2, 1, 8, 4, 1, 6, 1, 1, 1]})
    );
    assert_eq!(
        semantic_tokens(&[("/b.ts", text)], None),
        serde_json::json!({"data": [0, 6, 1, 8, 5, 1, 9, 1, 13, 1, 0, 2, 1, 7, 1, 0, 20, 1, 7, 0, 0, 4, 1, 8, 4, 1, 4, 1, 8, 1, 0, 4, 1, 13, 0, 0, 2, 1, 8, 4, 1, 6, 1, 1, 1, 0, 11, 1, 9, 9, 0, 4, 4, 8, 512, 0, 5, 3, 14, 512, 0, 4, 1, 8, 0]})
    );
}

/// In a JavaScript file tsgo's parser gives a declaration the type of its
/// JSDoc's `@type`, `@returns` or `@param`: a declaration with one has no
/// type hint. (Expected: tsgo's answer.)
#[test]
fn js_doc_types_leave_no_type_hints() {
    let text = "function f() { return 1; }\n/** @type {number} */\nlet x = f();\nlet y = f();\n/** @returns {number} */\nfunction g() { return 1; }\nfunction h() { return 1; }\n/**\n * @param {number} p\n */\nfunction k(p, q = 1) { return p + q; }\n[1].map(function (n) { return n; });\n";
    let hints: Vec<_> = inlay_hints(&[("/a.js", text)])
        .into_iter()
        .map(|(position, text, kind, _)| (position, text, kind))
        .collect();
    let hint =
        |position: (u32, u32), text: &str, kind: u32| (position, text.to_owned(), Some(kind));
    assert_eq!(
        hints,
        vec![
            hint((0, 12), ": number", 1),
            hint((3, 5), ": number", 1),
            hint((6, 12), ": number", 1),
            hint((10, 20), ": number", 1),
            hint((11, 8), "callbackfn:", 2),
            hint((11, 19), ": number", 1),
            hint((11, 20), ": number", 1),
        ]
    );
}

/// A variable is not hintable only when a `const` (tsgo `IsVarConst`: not
/// `using` or `await using`) has a literal initializer; a rest parameter's
/// labeled tuple names its arguments, at the labels; a rest parameter names
/// only its first argument; an enum member without an initializer shows its
/// value. (Expected: tsgo's answer.)
#[test]
fn inlay_hints_of_variables_arguments_and_enum_members() {
    let text = "declare const d: Disposable;\nusing u = d;\nconst c = 1 + 1;\nlet l = 1;\nawait using w = null;\nfunction r(...args: [first: number, second?: string]) {}\nr(1, \"s\");\nfunction s(a: number, ...rest: number[]) {}\ns(1, 2, 3);\nenum E { A, B = 5, C }\nexport {};\n";
    let hints = inlay_hints(&[("/b.ts", text)]);
    let location = |start: (u32, u32), end: (u32, u32)| tsc_lsproto::Location {
        uri: DocumentUri("file:///b.ts".to_owned()),
        range: tsc_lsproto::Range {
            start: Position {
                line: start.0,
                character: start.1,
            },
            end: Position {
                line: end.0,
                character: end.1,
            },
        },
    };
    let hint = |position: (u32, u32), text: &str, kind: Option<u32>, locations| {
        (position, text.to_owned(), kind, locations)
    };
    assert_eq!(
        hints,
        vec![
            hint((1, 7), ": Disposable", Some(1), Vec::new()),
            hint((2, 7), ": number", Some(1), Vec::new()),
            hint((3, 5), ": number", Some(1), Vec::new()),
            hint((4, 13), ": null", Some(1), Vec::new()),
            hint((5, 53), ": void", Some(1), Vec::new()),
            hint((6, 2), "first:", Some(2), vec![location((5, 21), (5, 26))]),
            hint((6, 5), "second:", Some(2), vec![location((5, 36), (5, 42))]),
            hint((7, 40), ": void", Some(1), Vec::new()),
            hint((8, 2), "a:", Some(2), vec![location((7, 11), (7, 12))]),
            hint(
                (8, 5),
                "...rest:",
                Some(2),
                vec![location((7, 25), (7, 29))]
            ),
            hint((9, 10), "= 0", None, Vec::new()),
            hint((9, 20), "= 6", None, Vec::new()),
        ]
    );
}

/// tsgo builds signature help's return type in the checker's own emit
/// context (`TypeToTypeNode`), which signature help's printer never reads: a
/// tuple loses its single-line flag and prints in its multi-line form, on
/// one line `[ string, number ]` (found by the LSP test on hono). Pinned to
/// tsgo's answers at aa814927.
#[test]
fn signature_help_prints_a_return_tuple_in_its_multi_line_form() {
    let text = "declare function pair(): [string, number];\npair();\ndeclare function entries<T>(o: T): [string, T][];\nentries(1);\n";
    let label = |position| {
        signature_help(&[("/a.ts", text)], options(|_| {}), position, None)["signatures"][0]
            ["label"]
            .clone()
    };
    assert_eq!(label((1, 5)), "pair(): [ string, number ]");
    assert_eq!(label((3, 8)), "entries(o: number): [ string, number ][]");
}

/// The return type's node, built in the checker's own emit context, loses
/// the comment range a property signature takes from its declaration too:
/// the parameter keeps the property's comment, the return type does not
/// (found by the LSP test on hono, in a `c.json({ // @ts-expect-error … })`
/// call). Pinned to tsgo's answer at aa814927.
#[test]
fn signature_help_prints_a_return_type_without_property_comments() {
    let text = "declare function json<T>(object: T): { value: T };\njson({\n  // a comment\n  clientId: 1,\n});\n";
    let help = signature_help(&[("/a.ts", text)], options(|_| {}), (1, 5), None);
    assert_eq!(
        help["signatures"][0]["label"],
        "json(object: {  // a comment clientId: number; }): { value: { clientId: number; }; }"
    );
}

/// tsgo reparses JSDoc into the function it annotates in JavaScript files
/// only: in a TypeScript file a `@returns` type of a variable statement's
/// JSDoc resolves from the statement, so its `I` is no reference to the
/// arrow function's type parameter (found by the LSP test on hono, in
/// `useRequestContext`). Pinned to tsgo's answer at aa814927.
#[test]
fn a_typescript_jsdoc_type_above_an_arrow_function_is_no_reference() {
    let text = "interface Box<T> { x: T }\n/**\n * @returns {Box<I>} An instance.\n */\nexport const g = <I,>(): Box<I> => ({ x: undefined as I });\n";
    assert_eq!(
        document_highlights(&[("/a.ts", text)], options(|_| {}), "/a.ts", (4, 29)),
        vec![
            (((4, 18), (4, 19)), 3),
            (((4, 29), (4, 30)), 2),
            (((4, 54), (4, 55)), 2),
        ]
    );
}

/// tsgo's `isThis` takes the `this` token of a `this` type, so its
/// references are `this` references (`getReferencesForThisKeyword`): none
/// in an interface, where tsc-rs answered the interface merged with a
/// `const` (found by the LSP test on zod), and in a class both the `this`
/// types and the `this` expressions of its instance members. Pinned to
/// tsgo's answers at aa814927.
#[test]
fn a_this_type_has_this_references() {
    let text = "interface Box<T> { t: T }\nexport interface Foo { m(): Box<this>; }\nexport const Foo = 1;\nclass C {\n  m(): this { return this; }\n  n(x: this) { this.m(); }\n}\n";
    let file = [("/a.ts", text)];
    assert_eq!(
        document_highlights(&file, options(|_| {}), "/a.ts", (1, 32)),
        Vec::<(Range, u32)>::new()
    );
    let class = vec![
        (((4, 7), (4, 11)), 2),
        (((4, 21), (4, 25)), 2),
        (((5, 7), (5, 11)), 2),
        (((5, 15), (5, 19)), 2),
    ];
    assert_eq!(
        document_highlights(&file, options(|_| {}), "/a.ts", (4, 7)),
        class
    );
    assert_eq!(
        document_highlights(&file, options(|_| {}), "/a.ts", (5, 15)),
        class
    );
}

/// tsgo prints a signature help item's parameters with a node builder of
/// the item's own (`itemInfoForParameters`), whose cache lets a later
/// parameter reuse the whole node an earlier one built, and each return
/// type with a fresh one (`TypeToTypeNode`), so no signature reuses a node
/// another signature cut (found by the LSP test on hono). Pinned to tsgo's
/// answers at aa814927.
#[test]
fn signature_help_items_and_return_types_keep_their_own_built_nodes() {
    let a = (0..12)
        .map(|index| format!("a{index}: string"))
        .collect::<Vec<_>>()
        .join("; ");
    let b = (0..12)
        .map(|index| format!("b{index}: number"))
        .collect::<Vec<_>>()
        .join("; ");
    let text = format!("declare const t: {{ {a} }};\ninterface Box<T> {{ get(): T }}\ninterface Pair<U, T> {{ get(): T }}\ndeclare const pair: Pair<{{ {b} }}, typeof t>;\ndeclare const box: Box<typeof t>;\nfunction f(a = t, b = pair) {{}}\nf();\ndeclare function g(x: 1): typeof pair;\ndeclare function g(x: 2): typeof box;\ng();\n");
    let labels = |position| {
        let help = signature_help(&[("/a.ts", text.as_str())], options(|_| {}), position, None);
        help["signatures"]
            .as_array()
            .expect("signatures")
            .iter()
            .map(|signature| signature["label"].as_str().expect("a label").to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        labels((6, 2)),
        vec![format!(
            "f(a?: {{ {a}; }}, b?: Pair<{{ {b}; }}, {{ {a}; }}>): void"
        )]
    );
    assert_eq!(
        labels((9, 2)),
        vec![
            format!("g(x: 1): Pair<{{ {b}; }}, {{ a0: string; a1: string; a2: string; a3: string; ... 7 more ...; a11: string; }}>"),
            format!("g(x: 2): Box<{{ {a}; }}>"),
        ]
    );
}
