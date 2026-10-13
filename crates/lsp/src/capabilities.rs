//! tsgo `handleInitialize`'s result: the server's information and the
//! capabilities tsgo advertises, with the trigger characters and the
//! semantic token legend of tsgo's language service.

use tsc_lsproto::*;

/// tsgo `ls.CompletionTriggerCharacters`.
pub const COMPLETION_TRIGGER_CHARACTERS: [&str; 10] =
    [".", "\"", "'", "`", "/", "@", "<", "#", " ", "*"];

/// tsgo `ls.SignatureHelpTriggerCharacters`.
pub const SIGNATURE_HELP_TRIGGER_CHARACTERS: [&str; 3] = ["(", ",", "<"];

/// tsgo `ls.SignatureHelpRetriggerCharacters`.
pub const SIGNATURE_HELP_RETRIGGER_CHARACTERS: [&str; 1] = [")"];

/// tsgo `ls.tokenTypes`, in their encoding order.
pub const TOKEN_TYPES: [SemanticTokenType; 23] = [
    SemanticTokenType::NAMESPACE,
    SemanticTokenType::CLASS,
    SemanticTokenType::ENUM,
    SemanticTokenType::INTERFACE,
    SemanticTokenType::STRUCT,
    SemanticTokenType::TYPE_PARAMETER,
    SemanticTokenType::TYPE,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::ENUM_MEMBER,
    SemanticTokenType::DECORATOR,
    SemanticTokenType::EVENT,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::METHOD,
    SemanticTokenType::MACRO,
    SemanticTokenType::LABEL,
    SemanticTokenType::COMMENT,
    SemanticTokenType::STRING,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::NUMBER,
    SemanticTokenType::REGEXP,
    SemanticTokenType::OPERATOR,
];

/// tsgo `ls.tokenModifiers`, in their encoding order.
pub const TOKEN_MODIFIERS: [SemanticTokenModifier; 10] = [
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::DEFINITION,
    SemanticTokenModifier::READONLY,
    SemanticTokenModifier::STATIC,
    SemanticTokenModifier::DEPRECATED,
    SemanticTokenModifier::ABSTRACT,
    SemanticTokenModifier::ASYNC,
    SemanticTokenModifier::MODIFICATION,
    SemanticTokenModifier::DOCUMENTATION,
    SemanticTokenModifier::DEFAULT_LIBRARY,
];

/// tsgo `ls.SemanticTokensLegend`: the token types and modifiers the client
/// supports, in tsgo's order.
pub fn semantic_tokens_legend(
    client: &ResolvedSemanticTokensClientCapabilities,
) -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES
            .iter()
            .filter(|kind| {
                client
                    .token_types
                    .iter()
                    .any(|supported| *supported == *kind.0)
            })
            .map(|kind| kind.0.to_string())
            .collect(),
        token_modifiers: TOKEN_MODIFIERS
            .iter()
            .filter(|modifier| {
                client
                    .token_modifiers
                    .iter()
                    .any(|supported| *supported == *modifier.0)
            })
            .map(|modifier| modifier.0.to_string())
            .collect(),
    }
}

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
                    legend: semantic_tokens_legend(&client.text_document.semantic_tokens),
                    full: Some(BooleanOrSemanticTokensFullDelta::Boolean(true)),
                    range: Some(BooleanOrEmptyObject::Boolean(true)),
                    ..SemanticTokensOptions::default()
                },
            )),
            ..ServerCapabilities::default()
        },
    }
}
