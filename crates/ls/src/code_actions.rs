//! tsgo `ls/codeactions.go`: the code actions of a document — the organize
//! imports actions of the requested kinds. tsgo's quick fixes and fix-all
//! action come from its code fix providers; with none ported they add no
//! action, as tsgo's loops over them add none for a diagnostic no provider
//! fixes.

use std::collections::BTreeMap;

use tsc_diagnostics::DiagnosticMessage;
use tsc_lsproto::{
    CodeAction, CodeActionKind, CodeActionParams, CodeActionResponse, CommandOrCodeAction,
    CommandOrCodeActionArrayOrNull, DocumentUri, TextEdit, WorkspaceEdit,
};
use tsc_program::uri::file_name_to_document_uri;

use crate::language_service::{LanguageService, LsError};

impl LanguageService<'_> {
    /// tsgo `ProvideCodeActions`.
    pub fn provide_code_actions(
        &mut self,
        params: &CodeActionParams,
    ) -> Result<CodeActionResponse, LsError> {
        let file = self.file(&params.text_document.uri);
        let mut actions = Vec::new();
        for kind in params.context.only.iter().flatten() {
            for matching_kind in get_organize_imports_actions_for_kind(kind) {
                actions.push(self.create_organize_imports_action(file, matching_kind)?);
            }
        }
        Ok(CommandOrCodeActionArrayOrNull::CommandOrCodeActionArray(
            actions,
        ))
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
        Ok(CommandOrCodeAction::CodeAction(CodeAction {
            title,
            kind: Some(kind),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..WorkspaceEdit::default()
            }),
            ..CodeAction::default()
        }))
    }
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
