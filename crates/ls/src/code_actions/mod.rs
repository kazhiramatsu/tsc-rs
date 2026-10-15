//! tsgo `ls/codeactions.go`: the code actions of a document — the organize
//! imports actions of the requested kinds, `source.fixAll`, and the quick
//! fixes of the code fix providers for the request's diagnostics with their
//! "fix all" variants.

mod implement_interface;
mod import_fixes;
mod missing_member;
mod missing_type_annotation;

pub(crate) use import_fixes::prepared_view;
pub(crate) use missing_member::{MissingMemberFixer, PRESERVE_OPTIONAL_PROPERTY};

use std::cmp::Ordering;
use std::collections::BTreeMap;

use tsc_diagnostics::DiagnosticMessage;
use tsc_lsproto::{
    self as lsproto, CodeActionKind, CodeActionParams, CodeActionResponse, CommandOrCodeAction,
    CommandOrCodeActionArrayOrNull, DocumentUri, IntegerOrString, TextEdit, WorkspaceEdit,
};
use tsc_program::uri::file_name_to_document_uri;

use crate::language_service::{LanguageService, LsError};
use crate::utilities::script_of;

/// tsgo `CodeFixProvider.GetCodeActions`.
type GetCodeActions =
    fn(&mut LanguageService<'_>, &CodeFixContext<'_>) -> Result<Vec<CodeAction>, LsError>;

/// tsgo `CodeFixProvider.GetAllCodeActions`.
type GetAllCodeActions = fn(
    &mut LanguageService<'_>,
    &CodeFixContext<'_>,
) -> Result<Option<CombinedCodeActions>, LsError>;

/// tsgo `CodeFixProvider`.
struct CodeFixProvider {
    error_codes: fn() -> Vec<u32>,
    get_code_actions: GetCodeActions,
    get_all_code_actions: Option<GetAllCodeActions>,
}

/// tsgo `codeFixProviders`: the registered code fix providers.
const CODE_FIX_PROVIDERS: &[CodeFixProvider] = &[
    import_fixes::IMPORT_FIX_PROVIDER,
    missing_type_annotation::PROVIDER,
    implement_interface::PROVIDER,
];

/// tsgo `CodeFixContext`.
pub(crate) struct CodeFixContext<'d> {
    pub(crate) file: usize,
    /// The diagnostic's range in the file.
    pub(crate) span: (usize, usize),
    pub(crate) error_code: u32,
    pub(crate) diagnostic: Option<&'d lsproto::Diagnostic>,
}

/// tsgo `CodeAction`: a single code action fix.
#[derive(Clone, Debug)]
pub(crate) struct CodeAction {
    pub(crate) description: String,
    pub(crate) changes: Vec<TextEdit>,
    pub(crate) fix_id: String,
}

impl CodeAction {
    /// tsgo `Compare`: by description, then the text edits.
    fn compare(&self, other: &Self) -> Ordering {
        self.description
            .cmp(&other.description)
            .then_with(|| self.changes.len().cmp(&other.changes.len()))
            .then_with(|| {
                self.changes
                    .iter()
                    .zip(&other.changes)
                    .map(|(a, b)| compare_text_edits(a, b))
                    .find(|ordering| ordering.is_ne())
                    .unwrap_or(Ordering::Equal)
            })
    }
}

/// tsgo `TextEdit.Compare`: by range, then text.
fn compare_text_edits(a: &TextEdit, b: &TextEdit) -> Ordering {
    let position = |position: &lsproto::Position| (position.line, position.character);
    position(&a.range.start)
        .cmp(&position(&b.range.start))
        .then_with(|| position(&a.range.end).cmp(&position(&b.range.end)))
        .then_with(|| a.new_text.cmp(&b.new_text))
}

/// tsgo `CombinedCodeActions`: the edits of a fix-all.
pub(crate) struct CombinedCodeActions {
    pub(crate) description: String,
    pub(crate) changes: Vec<TextEdit>,
}

impl LanguageService<'_> {
    /// tsgo `ProvideCodeActions`.
    pub fn provide_code_actions(
        &mut self,
        params: &CodeActionParams,
    ) -> Result<CodeActionResponse, LsError> {
        let uri = &params.text_document.uri;
        let file = self.file(uri);
        let mut actions = Vec::new();
        for kind in params.context.only.iter().flatten() {
            for matching_kind in get_organize_imports_actions_for_kind(kind) {
                actions.push(self.create_organize_imports_action(file, matching_kind)?);
            }
            if is_fix_all_kind(kind) {
                if let Some(action) = self.create_fix_all_action(file, uri)? {
                    actions.push(action);
                }
            }
        }
        if wants_quick_fixes(params.context.only.as_deref()) {
            // The providers by their fixes' IDs.
            let mut fix_id_seen: BTreeMap<String, usize> = BTreeMap::new();
            // sorted for binary search dedup, dedup across all diagnostics and providers so if multiple diags produce the same codefix, only one is returned
            let mut seen: Vec<CodeAction> = Vec::new();
            for diagnostic in &params.context.diagnostics {
                let Some(IntegerOrString::Integer(error_code)) = diagnostic.code else {
                    continue;
                };
                for (index, provider) in CODE_FIX_PROVIDERS.iter().enumerate() {
                    if !code_fix_provider_matches_lsp_diagnostic(provider, diagnostic) {
                        continue;
                    }
                    let context = CodeFixContext {
                        file,
                        span: self.text_range(file, &diagnostic.range),
                        error_code: error_code as u32,
                        diagnostic: Some(diagnostic),
                    };
                    for action in (provider.get_code_actions)(self, &context)? {
                        let Err(position) = seen.binary_search_by(|probe| probe.compare(&action))
                        else {
                            continue;
                        };
                        actions.push(convert_to_lsp_code_action(&action, diagnostic, uri));
                        if !action.fix_id.is_empty() {
                            fix_id_seen.insert(action.fix_id.clone(), index);
                        }
                        seen.insert(position, action);
                    }
                }
            }
            actions.extend(self.get_fix_all_quick_fixes(file, uri, &fix_id_seen)?);
        }
        Ok(CommandOrCodeActionArrayOrNull::CommandOrCodeActionArray(
            actions,
        ))
    }

    /// tsgo `FromLSPRangeForSourceFile` of a file without mapped
    /// projections: the range's offsets.
    fn text_range(&self, file: usize, range: &lsproto::Range) -> (usize, usize) {
        let Some(document) = self.program.document(file) else {
            return (0, 0);
        };
        let script = script_of(document.source());
        (
            self.converters.from_lsp_position(&script, &range.start),
            self.converters.from_lsp_position(&script, &range.end),
        )
    }

    /// tsgo `getFixAllQuickFixes`: per provider, a "fix all in file" quick
    /// fix when it matched at least 2 diagnostics in the file.
    fn get_fix_all_quick_fixes(
        &mut self,
        file: usize,
        uri: &DocumentUri,
        fix_id_seen: &BTreeMap<String, usize>,
    ) -> Result<Vec<CommandOrCodeAction>, LsError> {
        let mut actions = Vec::new();
        // Deduplicate providers; multiple fixIds may map to the same provider.
        let mut seen = Vec::new();
        for &index in fix_id_seen.values() {
            if seen.contains(&index) {
                continue;
            }
            seen.push(index);
            let provider = &CODE_FIX_PROVIDERS[index];
            let Some(get_all_code_actions) = provider.get_all_code_actions else {
                continue;
            };
            if !self.has_multiple_fixable_diagnostics(file, &(provider.error_codes)())? {
                continue;
            }
            let context = CodeFixContext {
                file,
                span: (0, 0),
                error_code: 0,
                diagnostic: None,
            };
            if let Some(combined) = get_all_code_actions(self, &context)? {
                if !combined.changes.is_empty() {
                    actions.push(CommandOrCodeAction::CodeAction(lsproto::CodeAction {
                        title: combined.description,
                        kind: Some(CodeActionKind::QUICK_FIX),
                        edit: Some(workspace_edit(uri, combined.changes)),
                        ..lsproto::CodeAction::default()
                    }));
                }
            }
        }
        Ok(actions)
    }

    /// tsgo `hasMultipleFixableDiagnostics`: whether the file has at least
    /// 2 diagnostics of the error codes, of every kind.
    fn has_multiple_fixable_diagnostics(
        &mut self,
        file: usize,
        error_codes: &[u32],
    ) -> Result<bool, LsError> {
        let diagnostics = self.all_diagnostics(file)?;
        Ok(diagnostics
            .iter()
            .filter(|diagnostic| is_fixable_diagnostic(diagnostic, error_codes))
            .take(2)
            .count()
            >= 2)
    }

    /// tsgo `createFixAllAction`: a `source.fixAll` action of every
    /// provider's fix-all.
    fn create_fix_all_action(
        &mut self,
        file: usize,
        uri: &DocumentUri,
    ) -> Result<Option<CommandOrCodeAction>, LsError> {
        let mut changes = Vec::new();
        for provider in CODE_FIX_PROVIDERS {
            let Some(get_all_code_actions) = provider.get_all_code_actions else {
                continue;
            };
            let context = CodeFixContext {
                file,
                span: (0, 0),
                error_code: 0,
                diagnostic: None,
            };
            if let Some(combined) = get_all_code_actions(self, &context)? {
                changes.extend(combined.changes);
            }
        }
        if changes.is_empty() {
            return Ok(None);
        }
        Ok(Some(CommandOrCodeAction::CodeAction(lsproto::CodeAction {
            title: self.localize(&tsc_diagnostics::gen::Fix_All, &[]),
            kind: Some(CodeActionKind::SOURCE_FIX_ALL_TS),
            edit: Some(workspace_edit(uri, changes)),
            ..lsproto::CodeAction::default()
        })))
    }

    /// tsgo `createOrganizeImportsAction`.
    fn create_organize_imports_action(
        &mut self,
        file: usize,
        kind: CodeActionKind,
    ) -> Result<CommandOrCodeAction, LsError> {
        let title = get_organize_imports_action_title(&kind)
            .template_in(self.catalog)
            .to_owned();
        let changes: BTreeMap<DocumentUri, Vec<TextEdit>> = self
            .organize_imports(file, &kind)?
            .into_iter()
            .map(|(file_name, edits)| (DocumentUri(file_name_to_document_uri(&file_name)), edits))
            .collect();
        Ok(CommandOrCodeAction::CodeAction(lsproto::CodeAction {
            title,
            kind: Some(kind),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..WorkspaceEdit::default()
            }),
            ..lsproto::CodeAction::default()
        }))
    }

    /// A message with its arguments in the request's locale (tsgo
    /// `Localize`).
    pub(crate) fn localize(&self, message: &'static DiagnosticMessage, args: &[&str]) -> String {
        crate::lsutil::localize(self.catalog, message, args)
    }
}

/// A workspace edit of one document's edits.
fn workspace_edit(uri: &DocumentUri, edits: Vec<TextEdit>) -> WorkspaceEdit {
    WorkspaceEdit {
        changes: Some(BTreeMap::from([(uri.clone(), edits)])),
        ..WorkspaceEdit::default()
    }
}

/// tsgo `codeFixProviderMatchesLSPDiagnostic`.
fn code_fix_provider_matches_lsp_diagnostic(
    provider: &CodeFixProvider,
    diagnostic: &lsproto::Diagnostic,
) -> bool {
    if diagnostic
        .source
        .as_deref()
        .is_some_and(|source| source != "ts")
    {
        return false;
    }
    matches!(
        diagnostic.code,
        Some(IntegerOrString::Integer(code)) if (provider.error_codes)().contains(&(code as u32))
    )
}

/// tsgo `isFixableDiagnostic`.
fn is_fixable_diagnostic(diagnostic: &tsc_diagnostics::Diagnostic, error_codes: &[u32]) -> bool {
    diagnostic.source.as_deref().unwrap_or_default().is_empty()
        && error_codes.contains(&diagnostic.code())
}

/// tsgo `isFixAllKind`: the requested kind matches `source.fixAll`.
fn is_fix_all_kind(kind: &CodeActionKind) -> bool {
    kind.contains(&CodeActionKind::SOURCE_FIX_ALL_TS)
}

/// tsgo `wantsQuickFixes`: no `only` filter (all kinds wanted), or one
/// that includes the quick fix kind.
fn wants_quick_fixes(only: Option<&[CodeActionKind]>) -> bool {
    match only {
        None | Some([]) => true,
        Some(kinds) => kinds
            .iter()
            .any(|kind| kind.contains(&CodeActionKind::QUICK_FIX)),
    }
}

/// tsgo `convertToLSPCodeAction`.
fn convert_to_lsp_code_action(
    action: &CodeAction,
    diagnostic: &lsproto::Diagnostic,
    uri: &DocumentUri,
) -> CommandOrCodeAction {
    CommandOrCodeAction::CodeAction(lsproto::CodeAction {
        title: action.description.clone(),
        kind: Some(CodeActionKind::QUICK_FIX),
        edit: Some(workspace_edit(uri, action.changes.clone())),
        diagnostics: Some(vec![diagnostic.clone()]),
        ..lsproto::CodeAction::default()
    })
}

/// tsgo `getOrganizeImportsActionTitle`.
fn get_organize_imports_action_title(kind: &CodeActionKind) -> &'static DiagnosticMessage {
    if *kind == CodeActionKind::SOURCE_REMOVE_UNUSED_IMPORTS_TS {
        &tsc_diagnostics::gen::Remove_Unused_Imports
    } else if *kind == CodeActionKind::SOURCE_SORT_IMPORTS_TS {
        &tsc_diagnostics::gen::Sort_Imports
    } else {
        &tsc_diagnostics::gen::Organize_Imports
    }
}

/// tsgo `getOrganizeImportsActionsForKind`: the organize imports kinds the
/// requested kind contains, or the requested kind itself when it is one.
fn get_organize_imports_actions_for_kind(requested_kind: &CodeActionKind) -> Vec<CodeActionKind> {
    let result: Vec<CodeActionKind> = [
        CodeActionKind::SOURCE_ORGANIZE_IMPORTS_TS,
        CodeActionKind::SOURCE_REMOVE_UNUSED_IMPORTS_TS,
        CodeActionKind::SOURCE_SORT_IMPORTS_TS,
    ]
    .into_iter()
    .filter(|organize_kind| requested_kind.contains(organize_kind))
    .collect();
    if result.contains(requested_kind) {
        return vec![requested_kind.clone()];
    }
    result
}
