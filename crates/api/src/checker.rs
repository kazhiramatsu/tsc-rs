//! The checker queries of tsgo `api/session.go` (19dadef8): the symbols,
//! types and signatures of a project's program, asked of the program's API
//! checker (P5-1d) and handed to the client as handles that the snapshot
//! registers (tsgo `snapshotData`'s registries and `checkerSetup`).
//!
//! tsgo numbers a symbol with a global counter the first time it is asked
//! for (`ast.GetSymbolId`), so a binder symbol of a source file that two
//! programs share has one number. tsc-rs numbers a binder symbol by its
//! place in its file's identities, which programs that share the file share,
//! and a checker's symbol within the checker: a snapshot numbers each symbol
//! the first time it is asked for, by the file that declared it or by the
//! program that created it ([`SymbolKey`]). Types and signatures are the
//! checker's, numbered by it, as tsgo's are.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;
use serde_json::value::RawValue;
use tsc_checker::exports::{ConstantValue, IntrinsicType, TYPE_FORMAT_DEFAULT};
use tsc_checker::state::{CheckAbort, CheckerState};
use tsc_project::ProjectProgram;
use tsc_syntax::{NodeId, SourceFile};
use tsc_types::{CheckFlags, LiteralValue, ObjectFlags, SymbolId, TypeData, TypeFlags, TypeId};

use crate::astnav::{Found, Navigator};
use crate::encoder::{build_node_index_table, tsgo_kind, NodeIndexTable, PositionMap};
use crate::ipc::Payload;
use crate::proto::{CheckerParams, DocumentIdentifier, SnapshotId};
use crate::session::{client_error, go_parse_uint32, json, Session};

/// A symbol's identity in a snapshot (tsgo's `*ast.Symbol`): a binder
/// symbol by the source file whose bind created it, the same in every
/// program that has the file; a checker's symbol by the program.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum SymbolKey {
    Bound { file: usize, symbol: SymbolId },
    Checker { program: usize, symbol: SymbolId },
}

/// tsgo `snapshotData`'s symbol registry and project registries, with the
/// symbol numbers (tsgo `ast.GetSymbolId`) and node index tables
/// (`encoder.GetNodeIndexTable`) they read.
#[derive(Default)]
pub(crate) struct Registry {
    symbol_ids: HashMap<SymbolKey, u64>,
    /// tsgo `symbolRegistry` and `symbolCanonicalProjects`: the symbol of
    /// a handle and the project it was first handed out in.
    symbols: HashMap<u64, (SymbolKey, String)>,
    /// tsgo `projectRegistries`: the type and signature handles each
    /// project handed out.
    projects: HashMap<String, ProjectRegistry>,
    node_tables: HashMap<usize, Arc<NodeIndexTable>>,
    /// tsgo `SourceFile.GetPositionMap`, by source file.
    position_maps: HashMap<usize, Arc<PositionMap>>,
    /// tsgo `Program.GetSourceFileByPath`: each program's files by path.
    file_paths: HashMap<usize, Arc<HashMap<String, usize>>>,
}

#[derive(Default)]
struct ProjectRegistry {
    types: HashSet<u32>,
}

/// tsgo `SymbolResponse`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SymbolResponse {
    id: u64,
    project: String,
    name: String,
    flags: u32,
    check_flags: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    declarations: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    value_declaration: String,
    #[serde(skip_serializing_if = "is_zero")]
    parent: u64,
    #[serde(skip_serializing_if = "is_zero")]
    export_symbol: u64,
}

/// tsgo `TypeResponse`. Go's `omitempty` keeps `false` and `0`.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TypeResponse {
    id: u32,
    flags: u32,
    object_flags: u32,
    is_tuple_type: bool,
    value: Option<Box<RawValue>>,
    #[serde(skip_serializing_if = "is_zero")]
    target: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    type_parameters: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    outer_type_parameters: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    local_type_parameters: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    element_flags: Vec<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fixed_length: Option<usize>,
    #[serde(rename = "readonly", skip_serializing_if = "Option::is_none")]
    tuple_readonly: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    labeled_element_declarations: Vec<String>,
    #[serde(skip_serializing_if = "is_zero")]
    object_type: u32,
    #[serde(skip_serializing_if = "is_zero")]
    index_type: u32,
    #[serde(skip_serializing_if = "is_zero")]
    check_type: u32,
    #[serde(skip_serializing_if = "is_zero")]
    extends_type: u32,
    #[serde(skip_serializing_if = "is_zero")]
    base_type: u32,
    #[serde(skip_serializing_if = "is_zero")]
    subst_constraint: u32,
    #[serde(skip_serializing_if = "is_zero")]
    type_parameter: u32,
    #[serde(skip_serializing_if = "is_zero")]
    constraint_type: u32,
    #[serde(skip_serializing_if = "is_zero")]
    name_type: u32,
    #[serde(skip_serializing_if = "is_zero")]
    template_type: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    texts: Vec<String>,
    #[serde(skip_serializing_if = "is_zero")]
    fresh_type: u32,
    #[serde(skip_serializing_if = "is_zero")]
    regular_type: u32,
    is_this_type: bool,
    #[serde(skip_serializing_if = "is_zero")]
    this_type: u32,
    #[serde(skip_serializing_if = "String::is_empty")]
    intrinsic_name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    alias_type_arguments: Vec<u32>,
    #[serde(skip_serializing_if = "is_zero")]
    alias_symbol: u64,
    #[serde(skip_serializing_if = "is_zero")]
    symbol: u64,
}

/// tsgo `ConstantValueResponse`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConstantValueResponse {
    is_number: bool,
    value: Option<Box<RawValue>>,
}

fn is_zero<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// A failed query: the checker aborted (no tsgo counterpart), or the
/// client's error.
fn aborted(abort: CheckAbort) -> String {
    format!("checker aborted: {}", abort.description())
}

impl Session {
    /// The checker methods of tsgo `HandleRequest`; `None` for another
    /// method.
    pub(crate) fn handle_checker_request(
        &self,
        method: &str,
        params: &[u8],
    ) -> Option<Result<Payload, String>> {
        let type_name = match method {
            "getSymbolAtPosition" => "GetSymbolAtPositionParams",
            "getSymbolsAtPositions" => "GetSymbolsAtPositionsParams",
            "getSymbolAtLocation" => "GetSymbolAtLocationParams",
            "getSymbolsAtLocations" => "GetSymbolsAtLocationsParams",
            "getSymbolOfSourceFile" => "GetSymbolOfSourceFileParams",
            "getSymbolsOfSourceFiles" => "GetSymbolsOfSourceFilesParams",
            "getTypeOfSymbol" | "getDeclaredTypeOfSymbol" | "getNonMissingTypeOfSymbol" => {
                "GetTypeOfSymbolParams"
            }
            "getTypesOfSymbols" => "GetTypesOfSymbolsParams",
            "getTypeAtLocation" | "getShorthandAssignmentValueSymbol" => "GetTypeAtLocationParams",
            "getTypeAtLocations" => "GetTypeAtLocationsParams",
            "getTypeAtPosition" => "GetTypeAtPositionParams",
            "getTypesAtPositions" => "GetTypesAtPositionsParams",
            "getTypeOfSymbolAtLocation" => "GetTypeOfSymbolAtLocationParams",
            "getParentOfSymbol"
            | "getMembersOfSymbol"
            | "getExportsOfSymbol"
            | "getExportSymbolOfSymbol" => "GetSymbolPropertyParams",
            "getSymbolOfType" => "GetTypePropertyParams",
            "typeToString" => "TypeToTypeNodeParams",
            "getAnyType"
            | "getStringType"
            | "getNumberType"
            | "getBooleanType"
            | "getVoidType"
            | "getUndefinedType"
            | "getNullType"
            | "getNeverType"
            | "getUnknownType"
            | "getBigIntType"
            | "getESSymbolType"
            | "getNonPrimitiveType" => "GetIntrinsicTypeParams",
            "getAliasedSymbol"
            | "getImmediateAliasedSymbol"
            | "getTargetSymbol"
            | "getExportSymbolOfSymbolForChecker"
            | "getFullyQualifiedName"
            | "getExportsOfModule"
            | "isReadonlySymbol" => "CheckerSymbolParams",
            "getExportSpecifierLocalTargetSymbol" | "getConstantValue" => "CheckerNodeParams",
            "getMemberInModuleExports" => "GetMemberInModuleExportsParams",
            _ => return None,
        };
        let params = match crate::session::parse::<CheckerParams>(type_name, params) {
            Ok(params) => params,
            Err(error) => return Some(Err(error)),
        };
        Some(self.checker_request(method, &params))
    }

    fn checker_request(&self, method: &str, params: &CheckerParams) -> Result<Payload, String> {
        // The symbol and type properties tsgo reads without a checker.
        match method {
            "getParentOfSymbol"
            | "getExportSymbolOfSymbol"
            | "getMembersOfSymbol"
            | "getExportsOfSymbol" => {
                return self.symbol_property(method, params);
            }
            "getSymbolOfType" => {
                let setup = self.setup_checker(params.snapshot, &params.project)?;
                let ty = u32::try_from(params.object_id).map_err(|_| {
                    client_error(format!(
                        "type handle {} not found in project registry",
                        params.object_id
                    ))
                })?;
                return setup.query(self, |query| {
                    let ty = query.resolve_type(ty)?;
                    let symbol = query.state.tables.type_of(ty).symbol;
                    Ok(json(&symbol.map(|symbol| query.symbol_response(symbol))))
                });
            }
            _ => {}
        }
        let setup = self.setup_checker(params.snapshot, &params.project)?;
        // The file of a request that names one, found before the checker
        // is taken.
        let file = match method {
            "getSymbolAtPosition"
            | "getSymbolsAtPositions"
            | "getTypeAtPosition"
            | "getTypesAtPositions"
            | "getSymbolOfSourceFile" => {
                Some(self.required_program_source(&setup.program, &params.file)?)
            }
            _ => None,
        };
        let files = match method {
            "getSymbolsOfSourceFiles" => params
                .files
                .iter()
                .map(|file| self.required_program_source(&setup.program, file))
                .collect::<Result<Vec<_>, _>>()?,
            _ => Vec::new(),
        };
        setup.query(self, |query| match method {
            "getSymbolAtPosition" => {
                let file = query.file_index(file.as_ref().unwrap())?;
                let found = query.touching_property_name(file, params.position);
                let symbol = query.symbol_at(found)?;
                Ok(json(&symbol.map(|symbol| query.symbol_response(symbol))))
            }
            "getSymbolsAtPositions" => {
                let file = query.file_index(file.as_ref().unwrap())?;
                let mut results = Vec::with_capacity(params.positions.len());
                for &position in &params.positions {
                    let found = query.touching_property_name(file, position);
                    let symbol = query.symbol_at(found)?;
                    results.push(symbol.map(|symbol| query.symbol_response(symbol)));
                }
                Ok(json(&results))
            }
            "getSymbolAtLocation" => {
                let node = query.resolve_node(&params.location)?;
                let symbol = query.state.get_symbol_at_location(node).map_err(aborted)?;
                Ok(json(&symbol.map(|symbol| query.symbol_response(symbol))))
            }
            "getSymbolsAtLocations" => {
                let mut results = Vec::with_capacity(params.locations.len());
                for location in &params.locations {
                    let node = query.resolve_node(location)?;
                    let symbol = query.state.get_symbol_at_location(node).map_err(aborted)?;
                    results.push(symbol.map(|symbol| query.symbol_response(symbol)));
                }
                Ok(json(&results))
            }
            "getSymbolOfSourceFile" => {
                let file = query.file_index(file.as_ref().unwrap())?;
                let root = query.state.binder.source(file).root;
                let symbol = query.state.get_symbol_at_location(root).map_err(aborted)?;
                Ok(json(&symbol.map(|symbol| query.symbol_response(symbol))))
            }
            "getSymbolsOfSourceFiles" => {
                let mut results = Vec::with_capacity(files.len());
                for file in &files {
                    let file = query.file_index(file)?;
                    let root = query.state.binder.source(file).root;
                    let symbol = query.state.get_symbol_at_location(root).map_err(aborted)?;
                    results.push(symbol.map(|symbol| query.symbol_response(symbol)));
                }
                Ok(json(&results))
            }
            "getTypeOfSymbol" | "getDeclaredTypeOfSymbol" | "getNonMissingTypeOfSymbol" => {
                let symbol = query.resolve_symbol(params.symbol)?;
                let state = &mut query.state;
                let ty = match method {
                    "getTypeOfSymbol" => state.get_type_of_symbol(symbol),
                    "getDeclaredTypeOfSymbol" => state.get_declared_type_of_symbol(symbol),
                    _ => state.get_non_missing_type_of_symbol(symbol),
                }
                .map_err(aborted)?;
                Ok(json(&query.type_response(ty)?))
            }
            "getTypesOfSymbols" => {
                let mut results = Vec::with_capacity(params.symbols.len());
                for &symbol in &params.symbols {
                    let symbol = query.resolve_symbol(symbol)?;
                    let ty = query.state.get_type_of_symbol(symbol).map_err(aborted)?;
                    results.push(query.type_response(ty)?);
                }
                Ok(json(&results))
            }
            "getTypeAtLocation" => {
                let node = query.resolve_node(&params.location)?;
                let ty = query.state.get_type_at_location(node).map_err(aborted)?;
                Ok(json(&query.type_response(ty)?))
            }
            "getTypeAtLocations" => {
                let mut results = Vec::with_capacity(params.locations.len());
                for location in &params.locations {
                    let node = query.resolve_node(location)?;
                    let ty = query.state.get_type_at_location(node).map_err(aborted)?;
                    results.push(query.type_response(ty)?);
                }
                Ok(json(&results))
            }
            "getTypeAtPosition" => {
                let file = query.file_index(file.as_ref().unwrap())?;
                let found = query.touching_property_name(file, params.position);
                let ty = query.type_at(found)?;
                Ok(json(&query.type_response(ty)?))
            }
            "getTypesAtPositions" => {
                let file = query.file_index(file.as_ref().unwrap())?;
                let mut results = Vec::with_capacity(params.positions.len());
                for &position in &params.positions {
                    let found = query.touching_property_name(file, position);
                    let ty = query.type_at(found)?;
                    results.push(query.type_response(ty)?);
                }
                Ok(json(&results))
            }
            "getTypeOfSymbolAtLocation" => {
                let symbol = query.resolve_symbol(params.symbol)?;
                let node = query.resolve_node(&params.location)?;
                let ty = query
                    .state
                    .get_type_of_symbol_at_location(symbol, Some(node))
                    .map_err(aborted)?;
                Ok(json(&query.type_response(ty)?))
            }
            "typeToString" => {
                let ty = query.resolve_type(params.type_id)?;
                let enclosing = match params.location.as_str() {
                    "" => None,
                    location => Some(query.resolve_node(location)?),
                };
                let flags = match params.flags {
                    0 => TYPE_FORMAT_DEFAULT,
                    flags => flags as u32,
                };
                let text = query
                    .state
                    .type_to_string_with_flags(ty, enclosing, flags)?;
                Ok(json(&text))
            }
            "getAnyType"
            | "getStringType"
            | "getNumberType"
            | "getBooleanType"
            | "getVoidType"
            | "getUndefinedType"
            | "getNullType"
            | "getNeverType"
            | "getUnknownType"
            | "getBigIntType"
            | "getESSymbolType"
            | "getNonPrimitiveType" => {
                let intrinsic = match method {
                    "getAnyType" => IntrinsicType::Any,
                    "getStringType" => IntrinsicType::String,
                    "getNumberType" => IntrinsicType::Number,
                    "getBooleanType" => IntrinsicType::Boolean,
                    "getVoidType" => IntrinsicType::Void,
                    "getUndefinedType" => IntrinsicType::Undefined,
                    "getNullType" => IntrinsicType::Null,
                    "getNeverType" => IntrinsicType::Never,
                    "getUnknownType" => IntrinsicType::Unknown,
                    "getBigIntType" => IntrinsicType::BigInt,
                    "getESSymbolType" => IntrinsicType::ESSymbol,
                    _ => IntrinsicType::NonPrimitive,
                };
                let ty = query.state.get_intrinsic_type(intrinsic);
                Ok(json(&query.type_response(ty)?))
            }
            "getShorthandAssignmentValueSymbol" => {
                let node = query.resolve_node(&params.location)?;
                let symbol = query
                    .state
                    .get_shorthand_assignment_value_symbol(node)
                    .map_err(aborted)?;
                Ok(json(&symbol.map(|symbol| query.symbol_response(symbol))))
            }
            "getExportSpecifierLocalTargetSymbol" => {
                let node = query.resolve_node(&params.location)?;
                let symbol = query
                    .state
                    .get_export_specifier_local_target_symbol(node)
                    .map_err(aborted)?;
                Ok(json(&symbol.map(|symbol| query.symbol_response(symbol))))
            }
            "getConstantValue" => {
                let node = query.resolve_node(&params.location)?;
                let value = query.state.get_constant_value(node).map_err(aborted)?;
                Ok(json(&ConstantValueResponse {
                    is_number: matches!(value, Some(ConstantValue::Number(_))),
                    value: value.map(|value| match value {
                        ConstantValue::String(text) => string_value(&text.to_string_lossy()),
                        ConstantValue::Number(number) => number_value(number),
                    }),
                }))
            }
            "getAliasedSymbol" => {
                let symbol = query.resolve_symbol(params.symbol)?;
                let aliased = query.state.get_aliased_symbol(symbol).map_err(aborted)?;
                Ok(json(&query.symbol_response(aliased)))
            }
            "getImmediateAliasedSymbol" => {
                let symbol = query.resolve_symbol(params.symbol)?;
                let aliased = query
                    .state
                    .get_immediate_aliased_symbol(symbol)
                    .map_err(aborted)?;
                Ok(json(&aliased.map(|symbol| query.symbol_response(symbol))))
            }
            "getTargetSymbol" => {
                let symbol = query.resolve_symbol(params.symbol)?;
                let target = query.state.get_target_symbol(symbol);
                Ok(json(&query.symbol_response(target)))
            }
            "getExportSymbolOfSymbolForChecker" => {
                let symbol = query.resolve_symbol(params.symbol)?;
                let export_symbol = query.state.get_export_symbol_of_symbol(symbol);
                Ok(json(&query.symbol_response(export_symbol)))
            }
            "getFullyQualifiedName" => {
                let symbol = query.resolve_symbol(params.symbol)?;
                let name = query.state.get_fully_qualified_name(symbol);
                Ok(json(&name.to_string_lossy()))
            }
            "getExportsOfModule" => {
                let symbol = query.resolve_symbol(params.symbol)?;
                let mut exports = query
                    .state
                    .get_exports_of_module_symbols(symbol)
                    .map_err(aborted)?;
                if exports.is_empty() {
                    // Go writes a nil slice as `[]`.
                    return Ok(json(&Vec::<SymbolResponse>::new()));
                }
                query.state.sort_symbols(&mut exports);
                let results = exports
                    .into_iter()
                    .map(|symbol| query.symbol_response(symbol))
                    .collect::<Vec<_>>();
                Ok(json(&results))
            }
            "getMemberInModuleExports" => {
                let symbol = query.resolve_symbol(params.symbol)?;
                let member = query
                    .state
                    .try_get_member_in_module_exports(&params.name, symbol)
                    .map_err(aborted)?;
                Ok(json(&member.map(|symbol| query.symbol_response(symbol))))
            }
            "isReadonlySymbol" => {
                let symbol = query.resolve_symbol(params.symbol)?;
                let readonly = query.state.is_readonly_symbol(symbol).map_err(aborted)?;
                Ok(json(&readonly))
            }
            _ => unreachable!("a checker method"),
        })
    }

    /// tsgo `resolveSymbolPropertyOfSymbol` and
    /// `resolveSymbolTablePropertyOfSymbol`: a symbol's parent, export
    /// symbol, members or exports, the tables sorted by `CompareSymbols`.
    fn symbol_property(&self, method: &str, params: &CheckerParams) -> Result<Payload, String> {
        let setup = self.setup_checker(params.snapshot, &params.project)?;
        setup.query(self, |query| {
            let symbol = query.resolve_symbol(params.object_id)?;
            let data = query.state.binder.symbol(symbol);
            match method {
                "getParentOfSymbol" | "getExportSymbolOfSymbol" => {
                    let result = if method == "getParentOfSymbol" {
                        data.parent
                    } else {
                        data.export_symbol
                    };
                    Ok(json(&result.map(|symbol| query.symbol_response(symbol))))
                }
                _ => {
                    let table = if method == "getMembersOfSymbol" {
                        Arc::clone(data.members())
                    } else {
                        query.state.symbol_exports(symbol)
                    };
                    let mut symbols = table.values().copied().collect::<Vec<_>>();
                    if symbols.is_empty() {
                        // Go writes a nil slice as `[]`.
                        return Ok(json(&Vec::<SymbolResponse>::new()));
                    }
                    query.state.sort_symbols(&mut symbols);
                    let results = symbols
                        .into_iter()
                        .map(|symbol| query.symbol_response(symbol))
                        .collect::<Vec<_>>();
                    Ok(json(&results))
                }
            }
        })
    }

    /// tsgo `setupChecker`: the snapshot's registry and the project's
    /// program.
    fn setup_checker(&self, snapshot: SnapshotId, project: &str) -> Result<CheckerSetup, String> {
        let registry = self.snapshot_registry(snapshot)?;
        let (_, program) = self.program(snapshot, project)?;
        Ok(CheckerSetup {
            registry,
            project: project.to_owned(),
            program,
        })
    }

    /// tsgo `Program.GetSourceFile` of a request's file, or the client's
    /// error.
    fn required_program_source(
        &self,
        program: &ProjectProgram,
        file: &DocumentIdentifier,
    ) -> Result<usize, String> {
        let source = self.required_source_file(program, file)?;
        Ok(source.document.source() as *const SourceFile as usize)
    }
}

/// tsgo `checkerSetup`: a snapshot's registry, a project and its program.
struct CheckerSetup {
    registry: Arc<Mutex<Registry>>,
    project: String,
    program: Arc<ProjectProgram>,
}

impl CheckerSetup {
    /// Run `query` over the program's API checker.
    fn query<T>(
        &self,
        session: &Session,
        query: impl FnOnce(&mut Query<'_, '_>) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        let program = Arc::as_ptr(&self.program) as usize;
        self.program.with_live(|live| {
            live.with_checker(|state| {
                query(&mut Query {
                    state,
                    registry: &mut registry,
                    project: &self.project,
                    program,
                    session,
                })
            })
            .unwrap_or_else(|| Err(client_error("project has no checker")))
        })
    }
}

/// A query over a project's API checker and its snapshot's registry.
pub(crate) struct Query<'q, 'a> {
    state: &'q mut CheckerState<'a>,
    registry: &'q mut Registry,
    project: &'q str,
    /// The program's identity.
    program: usize,
    session: &'q Session,
}

impl Query<'_, '_> {
    fn symbol_key(&self, symbol: SymbolId) -> SymbolKey {
        match self.state.binder.file_index_of_symbol(symbol) {
            Some(file) => SymbolKey::Bound {
                file: self.state.binder.source(file) as *const SourceFile as usize,
                symbol,
            },
            None => SymbolKey::Checker {
                program: self.program,
                symbol,
            },
        }
    }

    /// tsgo `SymbolHandle` (`ast.GetSymbolId`).
    fn symbol_id(&mut self, symbol: SymbolId) -> u64 {
        let key = self.symbol_key(symbol);
        let session = self.session;
        *self
            .registry
            .symbol_ids
            .entry(key)
            .or_insert_with(|| session.next_symbol_id.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// tsgo `registerSymbol`: the symbol's handle and the project it was
    /// first handed out in.
    fn register_symbol(&mut self, symbol: SymbolId) -> (u64, String) {
        let id = self.symbol_id(symbol);
        let key = self.symbol_key(symbol);
        let project = self.project;
        let (_, canonical) = self
            .registry
            .symbols
            .entry(id)
            .or_insert_with(|| (key, project.to_owned()));
        (id, canonical.clone())
    }

    /// tsgo `newSymbolResponse`.
    fn symbol_response(&mut self, symbol: SymbolId) -> SymbolResponse {
        let (id, project) = self.register_symbol(symbol);
        let data = self.state.binder.symbol(symbol);
        let declarations = data.declarations.to_vec();
        let value_declaration = data.value_declaration;
        let parent = data.parent;
        let export_symbol = data.export_symbol;
        let name = data.escaped_name.as_js().to_string_lossy().into_owned();
        let flags = symbol_flags(self.state, symbol);
        let check_flags = check_flags(self.state.get_check_flags(symbol));
        SymbolResponse {
            id,
            project,
            name,
            flags,
            check_flags,
            declarations: declarations
                .into_iter()
                .map(|declaration| self.node_handle(declaration))
                .collect(),
            value_declaration: value_declaration
                .map(|declaration| self.node_handle(declaration))
                .unwrap_or_default(),
            parent: parent.map_or(0, |parent| self.symbol_id(parent)),
            export_symbol: export_symbol.map_or(0, |symbol| self.symbol_id(symbol)),
        }
    }

    /// tsgo `resolveSymbolHandle`, of a symbol of this program: a binder
    /// symbol of one of its files, or a symbol of its checker.
    fn resolve_symbol(&self, handle: u64) -> Result<SymbolId, String> {
        if handle == 0 {
            return Err(client_error("empty symbol handle"));
        }
        let not_found = || {
            client_error(format!(
                "symbol handle {handle} not found in snapshot registry"
            ))
        };
        let (key, _) = self.registry.symbols.get(&handle).ok_or_else(not_found)?;
        match *key {
            SymbolKey::Bound { file, symbol } => self
                .state
                .binder
                .file_index_of_symbol(symbol)
                .filter(|&index| {
                    self.state.binder.source(index) as *const SourceFile as usize == file
                })
                .map(|_| symbol)
                .ok_or_else(not_found),
            SymbolKey::Checker { program, symbol } if program == self.program => Ok(symbol),
            SymbolKey::Checker { .. } => Err(not_found()),
        }
    }

    /// tsgo `TypeHandle`: the checker's number of the type.
    fn type_handle(ty: TypeId) -> u32 {
        ty.index() + 1
    }

    /// tsgo `registerType`.
    fn register_type(&mut self, ty: TypeId) -> u32 {
        let handle = Self::type_handle(ty);
        self.registry
            .projects
            .entry(self.project.to_owned())
            .or_default()
            .types
            .insert(handle);
        handle
    }

    /// tsgo `resolveTypeHandle`.
    fn resolve_type(&self, handle: u32) -> Result<TypeId, String> {
        if handle == 0 {
            return Err(client_error("empty type handle"));
        }
        if self.project.is_empty() {
            return Err(client_error(format!(
                "empty project ID for type handle {handle}"
            )));
        }
        let Some(registry) = self.registry.projects.get(self.project) else {
            return Err(client_error(format!(
                "type handle {handle} not found (no registry for project {})",
                self.project
            )));
        };
        if !registry.types.contains(&handle) {
            return Err(client_error(format!(
                "type handle {handle} not found in project registry"
            )));
        }
        Ok(TypeId::new(handle - 1))
    }

    /// tsgo `snapshotData.newTypeResponse`: [`new_type_response`] with the
    /// type registered, a mapped type's parts registered, and a tuple
    /// target's labeled element declarations.
    fn type_response(&mut self, ty: TypeId) -> Result<TypeResponse, String> {
        let id = self.register_type(ty);
        let mut response = self.new_type_response(ty, id);
        if self
            .state
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::MAPPED)
            && self.state.tables.flags_of(ty).intersects(TypeFlags::OBJECT)
        {
            let [type_parameter, constraint, name, template] =
                self.state.mapped_type_components(ty).map_err(aborted)?;
            let mut register = |ty: Option<TypeId>| ty.map_or(0, |ty| self.register_type(ty));
            response.type_parameter = register(type_parameter);
            response.constraint_type = register(constraint);
            response.name_type = register(name);
            response.template_type = register(template);
        }
        if let TypeData::TupleTarget(target) = &self.state.tables.type_of(ty).data {
            if let Some(labels) = target.labeled_element_declarations.clone() {
                response.labeled_element_declarations = labels
                    .iter()
                    .map(|label| {
                        label.map_or_else(String::new, |node| self.node_handle(NodeId::new(node)))
                    })
                    .collect();
            }
        }
        Ok(response)
    }

    /// tsgo `newTypeResponse`.
    fn new_type_response(&mut self, ty: TypeId, id: u32) -> TypeResponse {
        let tables = &self.state.tables;
        let data = tables.type_of(ty);
        let flags = data.flags;
        let symbol = data.symbol;
        let alias_symbol = data.alias_symbol;
        let alias_type_arguments = data.alias_type_arguments.clone();
        let mut response = TypeResponse {
            id,
            flags: flags.bits() as u32,
            alias_type_arguments: alias_type_arguments
                .iter()
                .flatten()
                .map(|&ty| Self::type_handle(ty))
                .collect(),
            ..TypeResponse::default()
        };
        if flags.intersects(TypeFlags::FRESHABLE) {
            if flags.intersects(TypeFlags::LITERAL) {
                response.value = literal_value(self.state, ty);
            }
            let data = self.state.tables.type_of(ty);
            response.fresh_type = data.fresh_type.map_or(0, Self::type_handle);
            response.regular_type = data.regular_type.map_or(0, Self::type_handle);
        } else if flags.intersects(TypeFlags::OBJECT) {
            let object_flags = self.state.tables.object_flags_of(ty);
            response.object_flags = object_flags_of(self.state, ty, object_flags);
            response.is_tuple_type = self.is_tuple_type(ty);
            if object_flags.intersects(ObjectFlags::REFERENCE) {
                if let TypeData::TupleTarget(target) = &self.state.tables.type_of(ty).data {
                    response.element_flags = target
                        .element_flags
                        .iter()
                        .map(|flags| flags.bits() as u32)
                        .collect();
                    response.fixed_length = Some(target.fixed_length);
                    response.tuple_readonly = Some(target.readonly);
                }
                response.target = Self::type_handle(self.state.tables.reference_target(ty));
            }
            if object_flags.intersects(ObjectFlags::CLASS_OR_INTERFACE) {
                if let TypeData::GenericType {
                    type_parameters,
                    outer_type_parameter_count,
                    this_type,
                } = &self.state.tables.type_of(ty).data
                {
                    let handles = type_parameters
                        .iter()
                        .map(|&ty| Self::type_handle(ty))
                        .collect::<Vec<_>>();
                    response.outer_type_parameters =
                        handles[..*outer_type_parameter_count].to_vec();
                    response.local_type_parameters =
                        handles[*outer_type_parameter_count..].to_vec();
                    response.type_parameters = handles;
                    response.this_type = Self::type_handle(*this_type);
                }
            }
        } else if flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
            // The types are fetched with their own request.
        } else {
            match &self.state.tables.type_of(ty).data {
                TypeData::Index { ty, .. } | TypeData::StringMapping { ty } => {
                    response.target = Self::type_handle(*ty);
                }
                TypeData::IndexedAccess {
                    object_type,
                    index_type,
                    ..
                } => {
                    response.object_type = Self::type_handle(*object_type);
                    response.index_type = Self::type_handle(*index_type);
                }
                TypeData::Conditional(conditional) => {
                    response.check_type = Self::type_handle(conditional.check_type);
                    response.extends_type = Self::type_handle(conditional.extends_type);
                }
                TypeData::Substitution(substitution) => {
                    response.base_type = Self::type_handle(substitution.base_type);
                    response.subst_constraint = Self::type_handle(substitution.constraint);
                }
                TypeData::TemplateLiteral { texts, .. } => {
                    // The types are fetched with their own request.
                    response.texts = texts
                        .iter()
                        .map(|text| text.to_js_string().to_string_lossy().into_owned())
                        .collect();
                }
                TypeData::TypeParameter { is_this_type, .. } => {
                    response.is_this_type = *is_this_type;
                }
                TypeData::Intrinsic { name, .. } if flags.intersects(TypeFlags::INTRINSIC) => {
                    response.intrinsic_name = (*name).to_owned();
                }
                _ => {}
            }
        }
        response.symbol = symbol.map_or(0, |symbol| self.symbol_id(symbol));
        response.alias_symbol = alias_symbol.map_or(0, |symbol| self.symbol_id(symbol));
        response
    }

    /// tsgo `IsTupleType`: a reference to a tuple target.
    fn is_tuple_type(&self, ty: TypeId) -> bool {
        let tables = &self.state.tables;
        tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::REFERENCE)
            && tables
                .object_flags_of(tables.reference_target(ty))
                .intersects(ObjectFlags::TUPLE)
    }

    /// tsgo `nodeHandleFrom`: `index.kind.path`, the node's index in the
    /// encoding of its file.
    fn node_handle(&mut self, node: NodeId) -> String {
        let file = self.state.binder.file_index_of_node(node);
        let source = self.state.binder.source(file);
        let path = self
            .session
            .host()
            .to_path(&source.file_name.to_string_lossy());
        let table = self.node_table(source);
        let index = table.get_index(node);
        let kind = match index {
            0 => tsgo_kind(source.arena.node(node).kind).unwrap_or_default(),
            index => table.kind(index),
        };
        format!("{index}.{kind}.{path}")
    }

    fn node_table(&mut self, source: &SourceFile) -> Arc<NodeIndexTable> {
        let key = source as *const SourceFile as usize;
        Arc::clone(
            self.registry
                .node_tables
                .entry(key)
                .or_insert_with(|| Arc::new(build_node_index_table(source))),
        )
    }

    /// tsgo `resolveNodeHandle`: the node of a handle `index.kind.path`, by
    /// its index in the encoding of the program's file at `path`.
    fn resolve_node(&mut self, handle: &str) -> Result<NodeId, String> {
        let invalid = || client_error(format!("invalid node handle {handle:?}"));
        let (index, rest) = handle.split_once('.').ok_or_else(invalid)?;
        let (_, path) = rest.split_once('.').ok_or_else(invalid)?;
        let index = go_parse_uint32(index)
            .map_err(|error| client_error(format!("invalid node handle {handle:?}: {error}")))?;
        let stale = || {
            client_error(format!(
                "node handle {handle:?} could not be resolved (file may not be loaded or handle may be stale)"
            ))
        };
        let file = self.file_paths().get(path).copied().ok_or_else(stale)?;
        let source = self.state.binder.source(file);
        let table = self.node_table(source);
        table
            .nodes()
            .get(index as usize)
            .copied()
            .flatten()
            .ok_or_else(stale)
    }

    /// The program's files by path.
    fn file_paths(&mut self) -> Arc<HashMap<String, usize>> {
        let state = &self.state;
        let host = self.session.host();
        Arc::clone(
            self.registry
                .file_paths
                .entry(self.program)
                .or_insert_with(|| {
                    Arc::new(
                        (0..state.binder.file_count())
                            .map(|file| {
                                let name = state.binder.source(file).file_name.to_string_lossy();
                                (host.to_path(&name), file)
                            })
                            .collect(),
                    )
                }),
        )
    }

    /// The checker's index of the program's file whose source is at
    /// `source` ([`Session::required_program_source`]).
    fn file_index(&self, source: &usize) -> Result<usize, String> {
        (0..self.state.binder.file_count())
            .find(|&file| self.state.binder.source(file) as *const SourceFile as usize == *source)
            .ok_or_else(|| client_error("source file not found"))
    }

    /// tsgo `astnav.GetTouchingPropertyName` at a UTF-16 position of a
    /// file.
    fn touching_property_name(&mut self, file: usize, position: u32) -> Found {
        let source = self.state.binder.source(file);
        let positions = Arc::clone(
            self.registry
                .position_maps
                .entry(source as *const SourceFile as usize)
                .or_insert_with(|| Arc::new(PositionMap::new(source.text()))),
        );
        Navigator::new(source).touching_property_name(positions.utf8(position))
    }

    /// tsgo `GetSymbolAtLocation` of what astnav found: a node's symbol,
    /// or the symbol at a token the tree does not keep.
    fn symbol_at(&mut self, found: Found) -> Result<Option<SymbolId>, String> {
        match found {
            Found::Node(view) => match view.node() {
                Some(node) => self.state.get_symbol_at_location(node).map_err(aborted),
                None => Ok(None),
            },
            Found::Token(token) => match token.parent.node() {
                Some(parent) => self
                    .state
                    .get_symbol_at_token(token.kind, parent)
                    .map_err(aborted),
                None => Ok(None),
            },
        }
    }

    /// tsgo `GetTypeAtLocation` of what astnav found: a node's type, or the
    /// type at a token the tree does not keep.
    fn type_at(&mut self, found: Found) -> Result<TypeId, String> {
        match found {
            Found::Node(view) => match view.node() {
                Some(node) => self.state.get_type_at_location(node).map_err(aborted),
                None => Ok(self.state.get_error_type()),
            },
            Found::Token(token) => match token.parent.node() {
                Some(parent) => self
                    .state
                    .get_type_at_token(token.kind, parent)
                    .map_err(aborted),
                None => Ok(self.state.get_error_type()),
            },
        }
    }
}

/// tsgo's `SymbolFlags` of a symbol: tsc-rs's flags are tsgo's first 28;
/// tsgo's `ConstEnumOnlyModule` and `ReplaceableByMethod` tsc-rs keeps
/// beside them.
fn symbol_flags(state: &CheckerState<'_>, symbol: SymbolId) -> u32 {
    let data = state.binder.symbol(symbol);
    let mut flags = data.flags.bits() as u32 & ((1 << 28) - 1);
    if data.extras().const_enum_only_module == Some(true) {
        flags |= 1 << 28;
    }
    if data.extras().is_replaceable_by_method {
        flags |= 1 << 29;
    }
    flags
}

/// tsgo's `CheckFlags` of tsc-rs's, bit by name (tsgo's
/// `IsDiscriminantComputed`, `IsDiscriminant` and `IndexSymbol` tsc-rs
/// keeps elsewhere).
fn check_flags(flags: CheckFlags) -> u32 {
    const BITS: [(CheckFlags, u32); 24] = [
        (CheckFlags::INSTANTIATED, 0),
        (CheckFlags::SYNTHETIC_PROPERTY, 1),
        (CheckFlags::SYNTHETIC_METHOD, 2),
        (CheckFlags::READONLY, 3),
        (CheckFlags::READ_PARTIAL, 4),
        (CheckFlags::WRITE_PARTIAL, 5),
        (CheckFlags::HAS_NON_UNIFORM_TYPE, 6),
        (CheckFlags::HAS_LITERAL_TYPE, 7),
        (CheckFlags::CONTAINS_PUBLIC, 8),
        (CheckFlags::CONTAINS_PROTECTED, 9),
        (CheckFlags::CONTAINS_PRIVATE, 10),
        (CheckFlags::CONTAINS_WRITE_PUBLIC, 11),
        (CheckFlags::CONTAINS_WRITE_PROTECTED, 12),
        (CheckFlags::CONTAINS_WRITE_PRIVATE, 13),
        (CheckFlags::CONTAINS_STATIC, 14),
        (CheckFlags::LATE, 15),
        (CheckFlags::REVERSE_MAPPED, 16),
        (CheckFlags::OPTIONAL_PARAMETER, 17),
        (CheckFlags::REST_PARAMETER, 18),
        (CheckFlags::DEFERRED_TYPE, 19),
        (CheckFlags::HAS_NEVER_TYPE, 20),
        (CheckFlags::MAPPED, 21),
        (CheckFlags::STRIP_OPTIONAL, 22),
        (CheckFlags::UNRESOLVED, 23),
    ];
    BITS.iter()
        .filter(|(flag, _)| flags.intersects(*flag))
        .fold(0, |bits, (_, bit)| bits | 1 << bit)
}

/// tsgo's `ObjectFlags` of an object type: tsc-rs's first 21 flags are
/// tsgo's, `MembersResolved` is whether the checker resolved the type's
/// members, and the object type flags after it are tsgo's by name.
fn object_flags_of(state: &CheckerState<'_>, ty: TypeId, flags: ObjectFlags) -> u32 {
    const MOVED: [(ObjectFlags, u32); 8] = [
        (ObjectFlags::CONTAINS_SPREAD, 22),
        (ObjectFlags::OBJECT_REST_TYPE, 23),
        (ObjectFlags::INSTANTIATION_EXPRESSION_TYPE, 24),
        (ObjectFlags::SINGLE_SIGNATURE_TYPE, 25),
        (ObjectFlags::IS_CLASS_INSTANCE_CLONE, 26),
        (ObjectFlags::IDENTICAL_BASE_TYPE_CALCULATED, 27),
        (ObjectFlags::IDENTICAL_BASE_TYPE_EXISTS, 28),
        (ObjectFlags::FROM_TYPE_NODE, 29),
    ];
    let mut bits = flags.bits() as u32 & ((1 << 21) - 1);
    if state.members_resolved(ty) {
        bits |= 1 << 21;
    }
    MOVED
        .iter()
        .filter(|(flag, _)| flags.intersects(*flag))
        .fold(bits, |bits, (_, bit)| bits | 1 << bit)
}

/// tsgo `literalValueToJSON` of a literal type's value: a string, a number
/// (`"+Infinity"`, `"-Infinity"` or `"NaN"` as strings), a boolean, or a
/// bigint as its signed decimal string.
fn literal_value(state: &CheckerState<'_>, ty: TypeId) -> Option<Box<RawValue>> {
    match &state.tables.type_of(ty).data {
        TypeData::Literal { value } => Some(match value {
            LiteralValue::String(text) => string_value(&text.to_js_string().to_string_lossy()),
            LiteralValue::Number(number) => number_value(*number),
            LiteralValue::BigInt(bigint) => string_value(&bigint.to_base10_string()),
        }),
        TypeData::Intrinsic { name, .. } => match *name {
            "true" => Some(raw("true".to_owned())),
            "false" => Some(raw("false".to_owned())),
            _ => None,
        },
        _ => None,
    }
}

fn raw(text: String) -> Box<RawValue> {
    RawValue::from_string(text).expect("valid JSON")
}

fn string_value(text: &str) -> Box<RawValue> {
    raw(serde_json::to_string(text).expect("a string serializes"))
}

/// A number as Go writes a `float64` (JavaScript's form; `-0` keeps its
/// sign), the infinities and NaN as tsgo's strings.
fn number_value(number: f64) -> Box<RawValue> {
    if number.is_nan() {
        return string_value("NaN");
    }
    if number.is_infinite() {
        return string_value(if number > 0.0 {
            "+Infinity"
        } else {
            "-Infinity"
        });
    }
    if number == 0.0 && number.is_sign_negative() {
        return raw("-0".to_owned());
    }
    raw(tsc_types::js_number_to_string(number))
}
