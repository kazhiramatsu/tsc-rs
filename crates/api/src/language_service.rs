//! tsgo `api/session.go`'s language service requests: the completions at a
//! position, the referenced symbols of a node, a signature's usages, the
//! references to a symbol in a file, a symbol's JSDoc tags and
//! documentation comment, and the import adder's edits — over the language
//! service ([`tsc_ls`]) of a project's program.
//!
//! A request whose results or inputs are symbol handles runs on the
//! program's API checker (tsgo's `CheckerLifetimeAPI`): tsc-rs's handles
//! name a checker's symbols by that checker, where tsgo's global symbol ids
//! let any checker's symbol through.

use std::sync::Arc;

use serde::Serialize;
use tsc_ls::autoimport::ImportSymbolAction;
use tsc_ls::lsconv::LspLineMap;
use tsc_ls::{LanguageService, LsError, ResultNode};
use tsc_lsproto::{PositionEncodingKind, ResolvedClientCapabilities};
use tsc_project::{Project, ProjectId, ProjectProgram, Snapshot};

use crate::encoder::PositionMap;
use crate::ipc::Payload;
use crate::proto::{
    CheckerParams, DocumentIdentifier, GetCompletionsAtPositionParams, GetImportAdderEditsParams,
    GetReferencedSymbolsForNodeParams, GetReferencesToSymbolInFileParams, GetSignatureUsagesParams,
    JsDocTagInfo, SnapshotId, TextEdit, IMPORT_ADDER_ACTION_IMPORT_SYMBOL,
};
use crate::session::{client_error, document_file_name, json, parse, ProgramFile, Session};
use crate::symbols::SymbolResponse;

/// tsgo `CompletionEntryLabelDetailsResponse`.
#[derive(Serialize)]
struct CompletionEntryLabelDetailsResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

/// tsgo `CompletionEntryResponse`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompletionEntryResponse {
    name: String,
    #[serde(skip_serializing_if = "is_zero")]
    kind: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    sort_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    insert_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filter_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label_details: Option<CompletionEntryLabelDetailsResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol: Option<SymbolResponse>,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// tsgo `CompletionInfoResponse`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompletionInfoResponse {
    is_incomplete: bool,
    entries: Vec<CompletionEntryResponse>,
}

/// tsgo `ReferencedSymbolEntry`.
#[derive(Serialize)]
struct ReferencedSymbolEntry {
    definition: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol: Option<SymbolResponse>,
    references: Vec<String>,
}

/// tsgo `SignatureUsageResponse`.
#[derive(Serialize)]
struct SignatureUsageResponse {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    call: Option<String>,
}

impl Session {
    /// The language service methods of tsgo `HandleRequest`; `None` for
    /// another method.
    pub(crate) fn handle_language_service_request(
        &self,
        method: &str,
        params: &[u8],
    ) -> Option<Result<Payload, String>> {
        Some(match method {
            "getCompletionsAtPosition" => {
                parse::<GetCompletionsAtPositionParams>("GetCompletionsAtPositionParams", params)
                    .and_then(|params| self.handle_get_completions_at_position(&params))
            }
            "getReferencedSymbolsForNode" => parse::<GetReferencedSymbolsForNodeParams>(
                "GetReferencedSymbolsForNodeParams",
                params,
            )
            .and_then(|params| self.handle_get_referenced_symbols_for_node(&params)),
            "getSignatureUsages" => {
                parse::<GetSignatureUsagesParams>("GetSignatureUsagesParams", params)
                    .and_then(|params| self.handle_get_signature_usages(&params))
            }
            "getReferencesToSymbolInFile" => parse::<GetReferencesToSymbolInFileParams>(
                "GetReferencesToSymbolInFileParams",
                params,
            )
            .and_then(|params| self.handle_get_references_to_symbol_in_file(&params)),
            "getJsDocTags" => parse::<CheckerParams>("CheckerSymbolParams", params)
                .and_then(|params| self.handle_get_js_doc_tags(&params)),
            "getDocumentationComment" => parse::<CheckerParams>("CheckerSymbolParams", params)
                .and_then(|params| self.handle_get_documentation_comment(&params)),
            "getImportAdderEdits" => {
                parse::<GetImportAdderEditsParams>("GetImportAdderEditsParams", params)
                    .and_then(|params| self.handle_get_import_adder_edits(&params))
            }
            _ => return None,
        })
    }

    /// tsgo `setupLanguageService`: the language service of the project's
    /// program in the snapshot, with the snapshot's converters (the API's
    /// UTF-8) and preferences and no client capabilities; on the API checker
    /// when `api_checker`.
    fn with_language_service<T>(
        &self,
        snapshot: &Arc<Snapshot>,
        project: &str,
        api_checker: bool,
        run: impl FnOnce(&mut LanguageService<'_>) -> T,
    ) -> Result<T, String> {
        let program = project_program(snapshot, project)?.1;
        let converters = snapshot.converters(PositionEncodingKind::UTF8);
        let preferences = snapshot.user_preferences().clone();
        let capabilities = ResolvedClientCapabilities::default();
        Ok(program.with_live(|live| {
            let mut service = LanguageService::new(
                live,
                snapshot.as_ref(),
                &converters,
                &preferences,
                &capabilities,
                None,
            )
            .with_project_id(project);
            if api_checker {
                service = service.with_api_checker();
            }
            run(&mut service)
        }))
    }

    /// tsgo `handleGetCompletionsAtPosition`: the completions at a UTF-16
    /// position of a file, a symbol's with its symbol when asked; through a
    /// snapshot with the file's auto-imports prepared when the snapshot's
    /// are not (an error when the symbols are asked).
    fn handle_get_completions_at_position(
        &self,
        params: &GetCompletionsAtPositionParams,
    ) -> Result<Payload, String> {
        let snapshot = self.snapshot(params.snapshot)?;
        let run = |snapshot: &Arc<Snapshot>,
                   program: &ProjectProgram|
         -> Result<Option<tsc_ls::ApiCompletionList>, Result<String, String>> {
            let Some(file) = self
                .program_source_file(program, &params.file)
                .map_err(Err)?
            else {
                return Ok(None);
            };
            let position = PositionMap::new(file.document.source().text()).utf8(params.position);
            self.with_language_service(
                snapshot,
                &params.project,
                params.include_symbol,
                |service| {
                    service.get_completions_at_position(
                        file.index,
                        position,
                        params.trigger_character.as_deref(),
                        params.include_symbol,
                    )
                },
            )
            .map_err(Err)?
            .map_err(|error| {
                if error.is_needs_auto_imports() {
                    Ok(file.file_name.clone())
                } else {
                    Err(error.0)
                }
            })
        };
        let program = project_program(&snapshot, &params.project)?.1;
        let list = match run(&snapshot, &program) {
            Ok(list) => list,
            Err(Err(error)) => return Err(error),
            Err(Ok(file_name)) => {
                if params.include_symbol {
                    return Err(client_error(format!(
                        "snapshot is not prepared for auto-imports for {}",
                        document_text(&params.file)
                    )));
                }
                let prepared =
                    self.clone_snapshot_with_auto_imports(params.snapshot, &snapshot, &file_name)?;
                let program = prepared_program(&prepared, &params.project)?;
                run(&prepared, &program).map_err(|error| match error {
                    Ok(_) => client_error(format!(
                        "snapshot is not prepared for auto-imports for {}",
                        document_text(&params.file)
                    )),
                    Err(error) => error,
                })?
            }
        };
        let Some(list) = list else {
            return Ok(json(&()));
        };
        let symbols = list
            .items
            .iter()
            .map(|item| item.symbol)
            .collect::<Vec<_>>();
        let symbol_responses = if symbols.iter().any(Option::is_some) {
            self.symbol_responses(params.snapshot, &params.project, &symbols)?
        } else {
            vec![None; symbols.len()]
        };
        let entries = list
            .items
            .into_iter()
            .zip(symbol_responses)
            .map(|(item, symbol)| {
                let item = item.item;
                CompletionEntryResponse {
                    name: item.label,
                    kind: item.kind.map_or(0, |kind| kind.0),
                    sort_text: item.sort_text,
                    insert_text: item.insert_text,
                    filter_text: item.filter_text,
                    detail: item.detail,
                    label_details: item.label_details.map(|details| {
                        CompletionEntryLabelDetailsResponse {
                            detail: details.detail,
                            description: details.description,
                        }
                    }),
                    symbol,
                }
            })
            .collect();
        Ok(json(&CompletionInfoResponse {
            is_incomplete: list.is_incomplete,
            entries,
        }))
    }

    /// tsgo `handleGetReferencedSymbolsForNode`: each referenced symbol's
    /// definition (one with a definition) and node references.
    fn handle_get_referenced_symbols_for_node(
        &self,
        params: &GetReferencedSymbolsForNodeParams,
    ) -> Result<Payload, String> {
        let snapshot = self.snapshot(params.snapshot)?;
        let program = project_program(&snapshot, &params.project)?.1;
        let (_, node) = self.resolve_node_handle(&program, &params.node)?;
        // tsgo hands the position to the service as it is.
        let position = params.position as u32;
        let entries = self
            .with_language_service(&snapshot, &params.project, true, |service| {
                service.get_referenced_symbols_for_node(position, node)
            })?
            .map_err(|error| error.0)?;
        let entries = entries
            .into_iter()
            .filter(|entry| entry.definition.is_some())
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return Ok(empty_list());
        }
        let setup = self.setup_checker(params.snapshot, &params.project)?;
        let results = setup.query(self, |query| {
            Ok(entries
                .iter()
                .map(|entry| ReferencedSymbolEntry {
                    definition: query.result_node_handle(entry.definition.expect("a definition")),
                    symbol: entry.symbol.map(|symbol| query.symbol_response(symbol)),
                    references: entry
                        .references
                        .iter()
                        .map(|&reference| query.result_node_handle(reference))
                        .collect(),
                })
                .collect::<Vec<_>>())
        })?;
        Ok(json(&results))
    }

    /// tsgo `handleGetSignatureUsages`: each usage's name and call.
    fn handle_get_signature_usages(
        &self,
        params: &GetSignatureUsagesParams,
    ) -> Result<Payload, String> {
        let snapshot = self.snapshot(params.snapshot)?;
        let program = project_program(&snapshot, &params.project)?.1;
        let (_, declaration) = self.resolve_node_handle(&program, &params.signature_decl)?;
        let usages = self
            .with_language_service(&snapshot, &params.project, false, |service| {
                service.get_signature_usages(declaration)
            })?
            .map_err(|error| error.0)?;
        if usages.is_empty() {
            return Ok(empty_list());
        }
        let setup = self.setup_checker(params.snapshot, &params.project)?;
        let results = setup.query(self, |query| {
            Ok(usages
                .iter()
                .map(|usage| SignatureUsageResponse {
                    name: query.result_node_handle(usage.name),
                    call: usage
                        .call
                        .map(|call| query.result_node_handle(ResultNode::Node(call))),
                })
                .collect::<Vec<_>>())
        })?;
        Ok(json(&results))
    }

    /// tsgo `handleGetReferencesToSymbolInFile`: the identifiers of the
    /// file that name the symbol.
    fn handle_get_references_to_symbol_in_file(
        &self,
        params: &GetReferencesToSymbolInFileParams,
    ) -> Result<Payload, String> {
        let setup = self.setup_checker(params.snapshot, &params.project)?;
        let symbol = setup.query(self, |query| query.resolve_symbol(&params.symbol))?;
        let snapshot = self.snapshot(params.snapshot)?;
        let program = project_program(&snapshot, &params.project)?.1;
        let file = self.required_source_file(&program, &params.file)?;
        let nodes = self
            .with_language_service(&snapshot, &params.project, true, |service| {
                service.get_references_to_symbol_in_file(file.index, symbol)
            })?
            .map_err(|error| error.0)?;
        let handles = setup.query(self, |query| {
            Ok(nodes
                .iter()
                .map(|&node| query.result_node_handle(ResultNode::Node(node)))
                .collect::<Vec<_>>())
        })?;
        Ok(json(&handles))
    }

    /// tsgo `handleGetJSDocTags`: the symbol's JSDoc tags, none when it has
    /// none.
    fn handle_get_js_doc_tags(&self, params: &CheckerParams) -> Result<Payload, String> {
        let setup = self.setup_checker(params.snapshot, &params.project)?;
        let reference = params.symbol.clone().unwrap_or_default();
        let symbol = setup.query(self, |query| query.resolve_symbol(&reference))?;
        let snapshot = self.snapshot(params.snapshot)?;
        let tags = self
            .with_language_service(&snapshot, &params.project, true, |service| {
                service.get_symbol_js_doc_tags(symbol)
            })?
            .map_err(|error| error.0)?;
        if tags.is_empty() {
            return Ok(empty_list());
        }
        let tags = tags
            .into_iter()
            .map(|tag| JsDocTagInfo {
                name: tag.name,
                text: tag.text,
            })
            .collect::<Vec<_>>();
        Ok(json(&tags))
    }

    /// tsgo `handleGetDocumentationComment`: the symbol's documentation
    /// comment as plain text.
    fn handle_get_documentation_comment(&self, params: &CheckerParams) -> Result<Payload, String> {
        let setup = self.setup_checker(params.snapshot, &params.project)?;
        let reference = params.symbol.clone().unwrap_or_default();
        let symbol = setup.query(self, |query| query.resolve_symbol(&reference))?;
        let snapshot = self.snapshot(params.snapshot)?;
        let documentation = self
            .with_language_service(&snapshot, &params.project, true, |service| {
                service.get_symbol_documentation_comment(symbol)
            })?
            .map_err(|error| error.0)?;
        Ok(json(&documentation))
    }

    /// tsgo `handleGetImportAdderEdits`: the edits that import each action's
    /// symbol into the file, in the UTF-16 offsets of its original text;
    /// through a snapshot with the file's auto-imports prepared when the
    /// snapshot's are not.
    fn handle_get_import_adder_edits(
        &self,
        params: &GetImportAdderEditsParams,
    ) -> Result<Payload, String> {
        let snapshot = self.snapshot(params.snapshot)?;
        let program = project_program(&snapshot, &params.project)?.1;
        let file = self.required_source_file(&program, &params.file)?;
        // The actions' symbols, of the requested snapshot's handles.
        let setup = self.setup_checker(params.snapshot, &params.project)?;
        let mut actions = Vec::with_capacity(params.actions.len());
        for (index, action) in params.actions.iter().enumerate() {
            if action.kind != IMPORT_ADDER_ACTION_IMPORT_SYMBOL {
                return Err(client_error(format!(
                    "unknown import adder action kind {:?}",
                    action.kind
                )));
            }
            let Some(reference) = &action.symbol else {
                return Err(client_error(format!(
                    "import adder action {index} missing symbol"
                )));
            };
            let symbol = setup.query(self, |query| query.resolve_symbol(reference))?;
            actions.push(ImportSymbolAction {
                symbol,
                is_valid_type_only_use_site: action.is_valid_type_only_use_site.unwrap_or(true),
            });
        }
        let edits = self.with_language_service(&snapshot, &params.project, true, |service| {
            service.get_import_adder_edits(file.index, &actions)
        })?;
        let edits = match edits {
            Ok(edits) => edits,
            Err(error) if error.is_needs_auto_imports() => {
                let prepared = self.clone_snapshot_with_auto_imports(
                    params.snapshot,
                    &snapshot,
                    &file.file_name,
                )?;
                let program = prepared_program(&prepared, &params.project)?;
                let file = self.required_source_file(&program, &params.file)?;
                self.with_language_service(&prepared, &params.project, true, |service| {
                    service.get_import_adder_edits(file.index, &actions)
                })?
                .map_err(|error: LsError| error.0)?
            }
            Err(error) => return Err(error.0),
        };
        Ok(match to_api_text_edits(&file, &edits) {
            Some(edits) => json(&edits),
            None => empty_list(),
        })
    }

    /// tsgo `Program.GetSourceFile` of a request's file: none when the
    /// program has no such file.
    fn program_source_file(
        &self,
        program: &ProjectProgram,
        file: &DocumentIdentifier,
    ) -> Result<Option<ProgramFile>, String> {
        Ok(self.source_file(program, &document_file_name(file)?))
    }

    /// The responses of symbols of the snapshot's project, on its API
    /// checker.
    fn symbol_responses(
        &self,
        snapshot: SnapshotId,
        project: &str,
        symbols: &[Option<tsc_types::SymbolId>],
    ) -> Result<Vec<Option<SymbolResponse>>, String> {
        let setup = self.setup_checker(snapshot, project)?;
        setup.query(self, |query| {
            Ok(symbols
                .iter()
                .map(|symbol| symbol.map(|symbol| query.symbol_response(symbol)))
                .collect())
        })
    }
}

/// A nil slice of tsgo's results, which its JSON (v2) encodes as an empty
/// array.
fn empty_list() -> Payload {
    json(&[(); 0])
}

/// tsgo `snapshotData.getProgram`: the project and its program, or the
/// client's error.
fn project_program(
    snapshot: &Snapshot,
    project: &str,
) -> Result<(Arc<Project>, Arc<ProjectProgram>), String> {
    let found = snapshot
        .project(&ProjectId::new(project))
        .ok_or_else(|| client_error(format!("project {project} not found")))?;
    let program = found
        .program()
        .cloned()
        .ok_or_else(|| client_error("project has no program"))?;
    Ok((Arc::clone(found), program))
}

/// The program of a project of a snapshot made with prepared auto-imports.
fn prepared_program(snapshot: &Snapshot, project: &str) -> Result<Arc<ProjectProgram>, String> {
    let found = snapshot
        .project(&ProjectId::new(project))
        .ok_or_else(|| client_error(format!("project {project} not found")))?;
    found
        .program()
        .cloned()
        .ok_or_else(|| client_error("project has no program"))
}

/// tsgo's `%s` of a `DocumentIdentifier`.
fn document_text(file: &DocumentIdentifier) -> &str {
    if file.uri.is_empty() {
        &file.file_name
    } else {
        &file.uri
    }
}

/// tsgo `toAPITextEdits`: each edit's LSP range (UTF-8 characters, the
/// API's converters) as UTF-16 offsets of the file's original text; none
/// when a range is outside it.
fn to_api_text_edits(file: &ProgramFile, edits: &[tsc_lsproto::TextEdit]) -> Option<Vec<TextEdit>> {
    let original = match &file.content_mapped {
        Some(mapped) => mapped.original_text.to_string(),
        None => file.document.source().text().to_owned(),
    };
    let line_map = LspLineMap::new(&original);
    let positions = PositionMap::new(&original);
    let offset = |position: &tsc_lsproto::Position| -> Option<u32> {
        let line_start = *line_map.line_starts.get(position.line as usize)?;
        let offset = line_start + position.character as usize;
        (offset <= original.len()).then_some(offset as u32)
    };
    edits
        .iter()
        .map(|edit| {
            Some(TextEdit {
                pos: positions.utf16(offset(&edit.range.start)?),
                end: positions.utf16(offset(&edit.range.end)?),
                new_text: edit.new_text.clone(),
            })
        })
        .collect()
}
