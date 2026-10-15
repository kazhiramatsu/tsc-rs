//! tsgo `handleInitialize`'s result: the server's information and the
//! capabilities tsgo advertises, with the trigger characters and the
//! semantic token legend of tsgo's language service ([`tsc_ls::semantic_tokens_legend`]).

use tsc_lsproto::*;

/// tsgo `ls.CompletionTriggerCharacters`.
pub const COMPLETION_TRIGGER_CHARACTERS: [&str; 10] =
    [".", "\"", "'", "`", "/", "@", "<", "#", " ", "*"];

/// tsgo `ls.SignatureHelpTriggerCharacters`.
pub const SIGNATURE_HELP_TRIGGER_CHARACTERS: [&str; 3] = ["(", ",", "<"];

/// tsgo `ls.SignatureHelpRetriggerCharacters`.
pub const SIGNATURE_HELP_RETRIGGER_CHARACTERS: [&str; 1] = [")"];

/// tsgo `supportedCodeActionKinds`.
pub fn supported_code_action_kinds() -> Vec<CodeActionKind> {
    vec![
        CodeActionKind::QUICK_FIX,
        CodeActionKind::SOURCE_ORGANIZE_IMPORTS_TS,
        CodeActionKind::SOURCE_REMOVE_UNUSED_IMPORTS_TS,
        CodeActionKind::SOURCE_SORT_IMPORTS_TS,
        CodeActionKind::SOURCE_FIX_ALL_TS,
    ]
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// tsgo `handleInitialize`'s `InitializeResult`.
pub fn initialize_result(
    position_encoding: &PositionEncodingKind,
    client: &ResolvedClientCapabilities,
) -> InitializeResponse {
    InitializeResult {
        server_info: Some(ServerInfo {
            name: "typescript".to_owned(),
            version: Some(tsc_types::TYPESCRIPT_VERSION.to_owned()),
        }),
        capabilities: ServerCapabilities {
            position_encoding: Some(position_encoding.clone()),
            text_document_sync: Some(TextDocumentSyncOptionsOrKind::Options(
                TextDocumentSyncOptions {
                    open_close: Some(true),
                    change: Some(TextDocumentSyncKind::INCREMENTAL),
                    save: Some(BooleanOrSaveOptions::Boolean(true)),
                    ..TextDocumentSyncOptions::default()
                },
            )),
            hover_provider: Some(BooleanOrHoverOptions::Boolean(true)),
            definition_provider: Some(BooleanOrDefinitionOptions::Boolean(true)),
            type_definition_provider: Some(
                BooleanOrTypeDefinitionOptionsOrTypeDefinitionRegistrationOptions::Boolean(true),
            ),
            references_provider: Some(BooleanOrReferenceOptions::Boolean(true)),
            implementation_provider: Some(
                BooleanOrImplementationOptionsOrImplementationRegistrationOptions::Boolean(true),
            ),
            diagnostic_provider: Some(DiagnosticOptionsOrRegistrationOptions::Options(
                DiagnosticOptions {
                    identifier: Some("typescript".to_owned()),
                    inter_file_dependencies: true,
                    ..DiagnosticOptions::default()
                },
            )),
            completion_provider: Some(CompletionOptions {
                trigger_characters: Some(strings(&COMPLETION_TRIGGER_CHARACTERS)),
                resolve_provider: Some(true),
                completion_item: Some(ServerCompletionItemOptions {
                    label_details_support: Some(true),
                }),
                ..CompletionOptions::default()
            }),
            signature_help_provider: Some(SignatureHelpOptions {
                trigger_characters: Some(strings(&SIGNATURE_HELP_TRIGGER_CHARACTERS)),
                retrigger_characters: Some(strings(&SIGNATURE_HELP_RETRIGGER_CHARACTERS)),
                ..SignatureHelpOptions::default()
            }),
            document_formatting_provider: Some(BooleanOrDocumentFormattingOptions::Boolean(true)),
            document_range_formatting_provider: Some(
                BooleanOrDocumentRangeFormattingOptions::Boolean(true),
            ),
            document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
                first_trigger_character: "{".to_owned(),
                more_trigger_character: Some(strings(&["}", ";", "\n"])),
            }),
            workspace_symbol_provider: Some(BooleanOrWorkspaceSymbolOptions::Boolean(true)),
            document_symbol_provider: Some(BooleanOrDocumentSymbolOptions::Boolean(true)),
            folding_range_provider: Some(
                BooleanOrFoldingRangeOptionsOrFoldingRangeRegistrationOptions::Boolean(true),
            ),
            rename_provider: Some(BooleanOrRenameOptions::RenameOptions(RenameOptions {
                prepare_provider: Some(true),
                ..RenameOptions::default()
            })),
            document_highlight_provider: Some(BooleanOrDocumentHighlightOptions::Boolean(true)),
            selection_range_provider: Some(
                BooleanOrSelectionRangeOptionsOrSelectionRangeRegistrationOptions::Boolean(true),
            ),
            linked_editing_range_provider: Some(
                BooleanOrLinkedEditingRangeOptionsOrLinkedEditingRangeRegistrationOptions::Boolean(
                    true,
                ),
            ),
            inlay_hint_provider: Some(
                BooleanOrInlayHintOptionsOrInlayHintRegistrationOptions::Boolean(true),
            ),
            code_lens_provider: Some(CodeLensOptions {
                resolve_provider: Some(true),
                ..CodeLensOptions::default()
            }),
            code_action_provider: Some(BooleanOrCodeActionOptions::CodeActionOptions(
                CodeActionOptions {
                    code_action_kinds: Some(supported_code_action_kinds()),
                    ..CodeActionOptions::default()
                },
            )),
            call_hierarchy_provider: Some(
                BooleanOrCallHierarchyOptionsOrCallHierarchyRegistrationOptions::Boolean(true),
            ),
            experimental: Some(ExperimentalServerCapabilities {
                custom_source_definition_provider: Some(true),
                custom_multi_document_highlight_provider: Some(true),
            }),
            _vs_references_provider: Some(true),
            _vs_on_auto_insert_provider: Some(VSOnAutoInsertOptions {
                _vs_trigger_characters: strings(&[">"]),
            }),
            workspace: Some(WorkspaceOptions {
                file_operations: Some(FileOperationOptions {
                    will_rename: Some(FileOperationRegistrationOptions {
                        filters: vec![FileOperationFilter {
                            scheme: Some("file".to_owned()),
                            pattern: FileOperationPattern {
                                glob: "**/*.{ts,tsx,js,jsx,cts,cjs,mts,mjs,json}".to_owned(),
                                ..FileOperationPattern::default()
                            },
                        }],
                    }),
                    ..FileOperationOptions::default()
                }),
                ..WorkspaceOptions::default()
            }),
            semantic_tokens_provider: Some(SemanticTokensOptionsOrRegistrationOptions::Options(
                SemanticTokensOptions {
                    legend: tsc_ls::semantic_tokens_legend(&client.text_document.semantic_tokens),
                    full: Some(BooleanOrSemanticTokensFullDelta::Boolean(true)),
                    range: Some(BooleanOrEmptyObject::Boolean(true)),
                    ..SemanticTokensOptions::default()
                },
            )),
            ..ServerCapabilities::default()
        },
    }
}

/// tsgo's `contentMapper*RegistrationID`s and their methods, in tsgo's
/// order.
pub const CONTENT_MAPPER_REGISTRATIONS: [(&str, &str); 26] = [
    ("content-mapper-did-open", "textDocument/didOpen"),
    ("content-mapper-did-change", "textDocument/didChange"),
    ("content-mapper-did-close", "textDocument/didClose"),
    ("content-mapper-diagnostic", "textDocument/diagnostic"),
    ("content-mapper-hover", "textDocument/hover"),
    (
        "content-mapper-signature-help",
        "textDocument/signatureHelp",
    ),
    ("content-mapper-definition", "textDocument/definition"),
    (
        "content-mapper-type-definition",
        "textDocument/typeDefinition",
    ),
    (
        "content-mapper-implementation",
        "textDocument/implementation",
    ),
    ("content-mapper-references", "textDocument/references"),
    (
        "content-mapper-document-highlight",
        "textDocument/documentHighlight",
    ),
    ("content-mapper-completion", "textDocument/completion"),
    ("content-mapper-rename", "textDocument/rename"),
    (
        "content-mapper-semantic-tokens",
        "textDocument/semanticTokens",
    ),
    (
        "content-mapper-document-symbol",
        "textDocument/documentSymbol",
    ),
    ("content-mapper-folding-range", "textDocument/foldingRange"),
    (
        "content-mapper-selection-range",
        "textDocument/selectionRange",
    ),
    ("content-mapper-inlay-hint", "textDocument/inlayHint"),
    ("content-mapper-code-lens", "textDocument/codeLens"),
    ("content-mapper-code-action", "textDocument/codeAction"),
    ("content-mapper-formatting", "textDocument/formatting"),
    (
        "content-mapper-range-formatting",
        "textDocument/rangeFormatting",
    ),
    (
        "content-mapper-on-type-formatting",
        "textDocument/onTypeFormatting",
    ),
    (
        "content-mapper-linked-editing",
        "textDocument/linkedEditingRange",
    ),
    (
        "content-mapper-call-hierarchy",
        "textDocument/prepareCallHierarchy",
    ),
    (
        "content-mapper-will-rename-files",
        "workspace/willRenameFiles",
    ),
];

/// tsgo `supportsContentMapperRegistration`: whether the client registers
/// the feature dynamically.
pub fn supports_content_mapper_registration(id: &str, client: &ResolvedClientCapabilities) -> bool {
    let text_document = &client.text_document;
    match id {
        "content-mapper-did-open" | "content-mapper-did-change" | "content-mapper-did-close" => {
            text_document.synchronization.dynamic_registration
        }
        "content-mapper-diagnostic" => text_document.diagnostic.dynamic_registration,
        "content-mapper-hover" => text_document.hover.dynamic_registration,
        "content-mapper-signature-help" => text_document.signature_help.dynamic_registration,
        "content-mapper-definition" => text_document.definition.dynamic_registration,
        "content-mapper-type-definition" => text_document.type_definition.dynamic_registration,
        "content-mapper-implementation" => text_document.implementation.dynamic_registration,
        "content-mapper-references" => text_document.references.dynamic_registration,
        "content-mapper-document-highlight" => {
            text_document.document_highlight.dynamic_registration
        }
        "content-mapper-completion" => text_document.completion.dynamic_registration,
        "content-mapper-rename" => text_document.rename.dynamic_registration,
        "content-mapper-semantic-tokens" => text_document.semantic_tokens.dynamic_registration,
        "content-mapper-document-symbol" => text_document.document_symbol.dynamic_registration,
        "content-mapper-folding-range" => text_document.folding_range.dynamic_registration,
        "content-mapper-selection-range" => text_document.selection_range.dynamic_registration,
        "content-mapper-inlay-hint" => text_document.inlay_hint.dynamic_registration,
        "content-mapper-code-lens" => text_document.code_lens.dynamic_registration,
        "content-mapper-code-action" => text_document.code_action.dynamic_registration,
        "content-mapper-formatting" => text_document.formatting.dynamic_registration,
        "content-mapper-range-formatting" => text_document.range_formatting.dynamic_registration,
        "content-mapper-on-type-formatting" => {
            text_document.on_type_formatting.dynamic_registration
        }
        "content-mapper-linked-editing" => text_document.linked_editing_range.dynamic_registration,
        "content-mapper-call-hierarchy" => text_document.call_hierarchy.dynamic_registration,
        "content-mapper-will-rename-files" => {
            client.workspace.file_operations.dynamic_registration
                && client.workspace.file_operations.will_rename
        }
        _ => false,
    }
}

/// tsgo `RegisterContentMapperExtensions`' registrations: the document
/// synchronization, pull diagnostics and features of the server for the
/// files of `extensions`, those the client registers dynamically.
pub fn content_mapper_registrations(
    extensions: &[String],
    client: &ResolvedClientCapabilities,
) -> Vec<Registration> {
    let selector = || {
        DocumentSelectorOrNull::DocumentSelector(
            extensions
                .iter()
                .map(|extension| {
                    TextDocumentFilterLanguageOrSchemeOrPattern::Pattern(
                        TextDocumentFilterPattern {
                            pattern: PatternOrRelativePattern::Pattern(format!("**/*{extension}")),
                            ..TextDocumentFilterPattern::default()
                        },
                    )
                })
                .collect(),
        )
    };
    let file_rename_filters = extensions
        .iter()
        .map(|extension| FileOperationFilter {
            scheme: Some("file".to_owned()),
            pattern: FileOperationPattern {
                glob: format!("**/*{extension}"),
                ..FileOperationPattern::default()
            },
        })
        .collect();
    let options = [
        RegisterOptions::TextDocumentDidOpen(TextDocumentRegistrationOptions {
            document_selector: selector(),
        }),
        RegisterOptions::TextDocumentDidChange(TextDocumentChangeRegistrationOptions {
            document_selector: selector(),
            sync_kind: TextDocumentSyncKind::INCREMENTAL,
        }),
        RegisterOptions::TextDocumentDidClose(TextDocumentRegistrationOptions {
            document_selector: selector(),
        }),
        RegisterOptions::TextDocumentDiagnostic(DiagnosticRegistrationOptions {
            document_selector: selector(),
            identifier: Some("typescript".to_owned()),
            inter_file_dependencies: true,
            ..DiagnosticRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentHover(HoverRegistrationOptions {
            document_selector: selector(),
            ..HoverRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentSignatureHelp(SignatureHelpRegistrationOptions {
            document_selector: selector(),
            trigger_characters: Some(strings(&SIGNATURE_HELP_TRIGGER_CHARACTERS)),
            retrigger_characters: Some(strings(&SIGNATURE_HELP_RETRIGGER_CHARACTERS)),
            ..SignatureHelpRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentDefinition(DefinitionRegistrationOptions {
            document_selector: selector(),
            ..DefinitionRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentTypeDefinition(TypeDefinitionRegistrationOptions {
            document_selector: selector(),
            ..TypeDefinitionRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentImplementation(ImplementationRegistrationOptions {
            document_selector: selector(),
            ..ImplementationRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentReferences(ReferenceRegistrationOptions {
            document_selector: selector(),
            ..ReferenceRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentDocumentHighlight(DocumentHighlightRegistrationOptions {
            document_selector: selector(),
            ..DocumentHighlightRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentCompletion(CompletionRegistrationOptions {
            document_selector: selector(),
            trigger_characters: Some(strings(&COMPLETION_TRIGGER_CHARACTERS)),
            resolve_provider: Some(true),
            completion_item: Some(ServerCompletionItemOptions {
                label_details_support: Some(true),
            }),
            ..CompletionRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentRename(RenameRegistrationOptions {
            document_selector: selector(),
            prepare_provider: Some(true),
            ..RenameRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentSemanticTokens(SemanticTokensRegistrationOptions {
            document_selector: selector(),
            legend: tsc_ls::semantic_tokens_legend(&client.text_document.semantic_tokens),
            full: Some(BooleanOrSemanticTokensFullDelta::Boolean(true)),
            range: Some(BooleanOrEmptyObject::Boolean(true)),
            ..SemanticTokensRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentDocumentSymbol(DocumentSymbolRegistrationOptions {
            document_selector: selector(),
            ..DocumentSymbolRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentFoldingRange(FoldingRangeRegistrationOptions {
            document_selector: selector(),
            ..FoldingRangeRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentSelectionRange(SelectionRangeRegistrationOptions {
            document_selector: selector(),
            ..SelectionRangeRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentInlayHint(InlayHintRegistrationOptions {
            document_selector: selector(),
            ..InlayHintRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentCodeLens(CodeLensRegistrationOptions {
            document_selector: selector(),
            resolve_provider: Some(true),
            ..CodeLensRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentCodeAction(CodeActionRegistrationOptions {
            document_selector: selector(),
            code_action_kinds: Some(supported_code_action_kinds()),
            ..CodeActionRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentFormatting(DocumentFormattingRegistrationOptions {
            document_selector: selector(),
            ..DocumentFormattingRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentRangeFormatting(DocumentRangeFormattingRegistrationOptions {
            document_selector: selector(),
            ..DocumentRangeFormattingRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentOnTypeFormatting(
            DocumentOnTypeFormattingRegistrationOptions {
                document_selector: selector(),
                first_trigger_character: "{".to_owned(),
                more_trigger_character: Some(strings(&["}", ";", "\n"])),
            },
        ),
        RegisterOptions::TextDocumentLinkedEditingRange(LinkedEditingRangeRegistrationOptions {
            document_selector: selector(),
            ..LinkedEditingRangeRegistrationOptions::default()
        }),
        RegisterOptions::TextDocumentPrepareCallHierarchy(CallHierarchyRegistrationOptions {
            document_selector: selector(),
            ..CallHierarchyRegistrationOptions::default()
        }),
        RegisterOptions::WorkspaceWillRenameFiles(FileOperationRegistrationOptions {
            filters: file_rename_filters,
        }),
    ];
    CONTENT_MAPPER_REGISTRATIONS
        .iter()
        .zip(options)
        .filter(|((id, _), _)| supports_content_mapper_registration(id, client))
        .map(|((id, _), register_options)| Registration {
            id: (*id).to_owned(),
            register_options: Some(register_options),
        })
        .collect()
}
