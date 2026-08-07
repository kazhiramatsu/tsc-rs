use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const RECORDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h1-owner-inventory.v1.json"
));
const MANIFEST_SHA256: &str = "6148160678bf0b34a8310551eac8c9ab3f2afb1cd9260fa8eaa59efadc71abb5";
const GENERATOR_PATH: &str = "crates/oracle/h1-owner-inventory.mjs";
const GENERATOR_SHA256: &str = "4765d3b27475a8a9389de96c6dcd89aacf2dbc7d6e63675c5a847d0fa6acacaf";
const CONTRACT_PATH: &str = ".github/ci/contracts/h1-owner-inventory.schema.json";
const CONTRACT_SHA256: &str = "433809f8489f12ec34c3de10dbecc8fad019bf0d7f777302298b10cfaf707cf2";
const TYPESCRIPT_SOURCE_PATH: &str = "vendor/typescript-6.0.3/lib/_tsc.js";
const TYPESCRIPT_SOURCE_SHA256: &str =
    "1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3";

const ROOTS: [(&str, &str, usize); 11] = [
    ("program-emit", "emit", 123_568),
    ("cli-emit", "emitFilesAndReportErrors", 129_412),
    ("transformer-selection", "getTransformers", 115_897),
    ("typescript-transform", "transformTypeScript", 94_036),
    ("class-fields-transform", "transformClassFields", 95_852),
    ("module-transform", "transformECMAScriptModule", 113_369),
    ("transform-runtime", "transformNodes", 115_977),
    ("printer", "createPrinter", 116_912),
    ("output-enumeration", "forEachEmittedFile", 116_312),
    ("output-paths", "getOutputPathsFor", 116_373),
    ("emit-orchestration", "emitFiles", 116_530),
];

const SEAMS: [(&str, &str, &str, usize); 8] = [
    (
        "declaration",
        "declaration output-path slot",
        "getDeclarationEmitOutputFilePath",
        16_577,
    ),
    (
        "declaration",
        "declaration transform root",
        "transformDeclarations",
        114_265,
    ),
    (
        "declaration",
        "declaration transformer ordering",
        "getDeclarationTransformers",
        115_950,
    ),
    (
        "source-map",
        "source-map generator contract",
        "createSourceMapGenerator",
        92_365,
    ),
    (
        "source-map",
        "source-map output-path slot",
        "getSourceMapFilePath",
        116_388,
    ),
    (
        "bundle",
        "bundle output shape",
        "getOutputPathsForBundle",
        116_365,
    ),
    (
        "build-info",
        "build-info output-path slot",
        "getTsBuildInfoEmitOutputFilePath",
        116_342,
    ),
    (
        "targeted-emit",
        "target SourceFile request parameter",
        "emit",
        123_568,
    ),
];

static PARSED: OnceLock<Manifest> = OnceLock::new();

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    status: String,
    phase: String,
    typescript: TypeScriptIdentity,
    generator: String,
    contract: PathHash,
    identity: String,
    ledger_hash: String,
    closure_model: String,
    call_review_contract: CallReviewContract,
    active_roots: Vec<ActiveRoot>,
    dormant_seams: Vec<DormantSeam>,
    summary: Summary,
    completed_h1_0a: Vec<String>,
    pending_h1_0a: Vec<String>,
    functions: Vec<FunctionRecord>,
    graph: Graph,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TypeScriptIdentity {
    version: String,
    source_commit: String,
    source: String,
    source_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathHash {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CallReviewContract {
    exact_edges: String,
    runtime_library: String,
    callback_and_value_calls: String,
    structural_property_dispatch: String,
    dynamic_expressions: String,
    candidate_sets: String,
    unresolved_state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveRoot {
    key: String,
    declaration: Declaration,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DormantSeam {
    axis: String,
    role: String,
    declaration: Declaration,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Declaration {
    id: String,
    name: String,
    kind: String,
    lexical_owner: Option<String>,
    lexical_path: String,
    source_range: SourceRange,
    declaration_sha256: String,
    body_range: Option<SourceRange>,
    body_sha256: Option<String>,
    ledger_slice_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FunctionRecord {
    id: String,
    name: String,
    kind: String,
    lexical_owner: Option<String>,
    lexical_path: String,
    source_range: SourceRange,
    declaration_sha256: String,
    body_range: Option<SourceRange>,
    body_sha256: Option<String>,
    ledger_slice_sha256: String,
    reachable_from: Vec<String>,
    shortest_root_path: RootPath,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootPath {
    root: String,
    declarations: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRange {
    start: SourcePosition,
    end: SourcePosition,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourcePosition {
    offset: usize,
    line: usize,
    character: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    edges: Vec<Edge>,
    call_dispositions: Vec<CallDisposition>,
    property_candidate_sets: Vec<PropertyCandidateSet>,
    unresolved_calls: Vec<Value>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum EdgeKind {
    Lexical,
    Immediate,
    DynamicSymbol,
    NestedFunction,
    PropertySymbol,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Edge {
    caller: String,
    callee: String,
    kind: EdgeKind,
    sites: Vec<CallSite>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CallSite {
    line: usize,
    character: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum IdentifierResolution {
    RuntimeLibrary,
    ParameterCallback,
    DestructuredCallback,
    SourceValueCall,
    ExternalGlobal,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum PropertyResolution {
    SourceSymbol,
    RuntimeLibrary,
    StructuralDispatch,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum CheckerState {
    Resolved,
    Absent,
    SourceSymbolUnfollowed,
    StackOverflow,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum DynamicResolution {
    SourceExpression,
    ComputedElement,
    CallResult,
    ComputedExpression,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum CallDisposition {
    Identifier {
        caller: String,
        expression: String,
        expression_sha256: String,
        line: usize,
        character: usize,
        resolution: IdentifierResolution,
        symbol_declaration_kinds: Vec<String>,
        library_paths: Vec<String>,
    },
    Property {
        caller: String,
        expression: String,
        expression_sha256: String,
        line: usize,
        character: usize,
        property: String,
        receiver: String,
        resolution: PropertyResolution,
        checker_state: CheckerState,
        symbol_declaration_kinds: Vec<String>,
        library_paths: Vec<String>,
        source_callees: Vec<String>,
        candidate_set: Option<String>,
    },
    Dynamic {
        caller: String,
        expression: String,
        expression_sha256: String,
        line: usize,
        character: usize,
        expression_kind: String,
        resolution: DynamicResolution,
        source_callees: Vec<String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertyCandidateSet {
    property: String,
    candidates: Vec<PropertyCandidate>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertyCandidate {
    id: String,
    name: String,
    kind: String,
    lexical_owner: Option<String>,
    line: usize,
    character: usize,
    declaration_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Summary {
    source_declarations: usize,
    active_roots: usize,
    dormant_seams: usize,
    closure_declarations: usize,
    static_edges: usize,
    lexical_edges: usize,
    immediate_edges: usize,
    dynamic_symbol_edges: usize,
    nested_function_edges: usize,
    property_symbol_edges: usize,
    call_sites: usize,
    reviewed_call_sites: usize,
    identifier_runtime_library_calls: usize,
    identifier_parameter_callback_calls: usize,
    identifier_destructured_callback_calls: usize,
    identifier_source_value_calls: usize,
    identifier_external_global_calls: usize,
    property_source_symbol_calls: usize,
    property_runtime_library_calls: usize,
    property_structural_dispatch_calls: usize,
    property_checker_stack_overflow_calls: usize,
    dynamic_expression_calls: usize,
    dynamic_source_expression_calls: usize,
    reviewed_exact_edge_calls: usize,
    reviewed_non_edge_calls: usize,
    property_candidate_sets: usize,
    property_candidate_declarations: usize,
    undispositioned_calls: usize,
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("harness crate must be inside workspace")
        .to_path_buf()
}

fn parsed() -> &'static Manifest {
    PARSED.get_or_init(|| {
        serde_json::from_slice(RECORDED)
            .expect("H1 owner inventory must satisfy the strict Rust contract")
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn verify_file_hash(workspace: &Path, path: &str, expected: &str) {
    let bytes = fs::read(workspace.join(path))
        .unwrap_or_else(|error| panic!("failed to read {path}: {error}"));
    assert_eq!(sha256_hex(&bytes), expected, "{path} hash");
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_h1_id(value: &str) -> bool {
    value.strip_prefix("h1:").is_some_and(valid_sha256)
}

fn verify_range(range: &SourceRange) {
    assert!(range.start.offset < range.end.offset);
    assert!(range.start.line > 0);
    assert!(range.start.character > 0);
    assert!(range.end.line >= range.start.line);
    assert!(range.end.character > 0);
}

fn verify_declaration(declaration: &Declaration) {
    assert!(valid_h1_id(&declaration.id));
    assert!(!declaration.name.is_empty());
    assert!(!declaration.kind.is_empty());
    assert!(!declaration.lexical_path.is_empty());
    assert!(declaration.lexical_owner.as_deref().is_none_or(valid_h1_id));
    verify_range(&declaration.source_range);
    assert!(valid_sha256(&declaration.declaration_sha256));
    assert!(valid_sha256(&declaration.ledger_slice_sha256));
    match (&declaration.body_range, &declaration.body_sha256) {
        (Some(range), Some(hash)) => {
            verify_range(range);
            assert!(valid_sha256(hash));
        }
        (None, None) => {}
        _ => panic!("declaration body range and hash must appear together"),
    }
}

fn disposition_site(disposition: &CallDisposition) -> (&str, &str, &str, usize, usize) {
    match disposition {
        CallDisposition::Identifier {
            caller,
            expression,
            expression_sha256,
            line,
            character,
            ..
        }
        | CallDisposition::Property {
            caller,
            expression,
            expression_sha256,
            line,
            character,
            ..
        }
        | CallDisposition::Dynamic {
            caller,
            expression,
            expression_sha256,
            line,
            character,
            ..
        } => (caller, expression, expression_sha256, *line, *character),
    }
}

#[test]
fn reviewed_owner_graph_is_strict_and_bound_to_exact_inputs() {
    assert_eq!(sha256_hex(RECORDED), MANIFEST_SHA256, "manifest hash");
    let manifest = parsed();
    let workspace = workspace();

    assert_eq!(manifest.schema, 2);
    assert_eq!(manifest.status, "draft/report-only");
    assert_eq!(manifest.phase, "H1.0a-reviewed-owner-graph");
    assert_eq!(manifest.typescript.version, "6.0.3");
    assert_eq!(
        manifest.typescript.source_commit,
        "050880ce59e30b356b686bd3144efe24f875ebc8"
    );
    assert_eq!(manifest.typescript.source, TYPESCRIPT_SOURCE_PATH);
    assert_eq!(manifest.typescript.source_sha256, TYPESCRIPT_SOURCE_SHA256);
    assert_eq!(manifest.generator, GENERATOR_PATH);
    assert_eq!(manifest.contract.path, CONTRACT_PATH);
    assert_eq!(manifest.contract.sha256, CONTRACT_SHA256);
    verify_file_hash(&workspace, GENERATOR_PATH, GENERATOR_SHA256);
    verify_file_hash(&workspace, CONTRACT_PATH, CONTRACT_SHA256);
    verify_file_hash(&workspace, TYPESCRIPT_SOURCE_PATH, TYPESCRIPT_SOURCE_SHA256);

    assert!(manifest.identity.starts_with("sha256("));
    assert!(manifest.ledger_hash.starts_with("SHA-256"));
    assert!(manifest.closure_model.contains("source-symbol"));
    assert!(manifest
        .closure_model
        .contains("explicit non-edge dispositions"));
    let contract = &manifest.call_review_contract;
    assert!(contract.exact_edges.contains("become graph edges"));
    assert!(contract
        .runtime_library
        .contains("never mapped to same-name"));
    assert!(contract
        .callback_and_value_calls
        .contains("without guessed"));
    assert!(contract
        .structural_property_dispatch
        .contains("review candidates, not graph edges"));
    assert!(contract
        .dynamic_expressions
        .contains("explicit dynamic seams"));
    assert!(contract.candidate_sets.contains("explicit empty set"));
    assert_eq!(contract.unresolved_state, "none");

    let schema: Value = serde_json::from_slice(
        &fs::read(workspace.join(CONTRACT_PATH)).expect("failed to read H1 owner schema"),
    )
    .expect("H1 owner schema must be valid JSON");
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["additionalProperties"], false);
    for definition in [
        "declaration",
        "function",
        "edge",
        "property_candidate",
        "property_candidate_set",
        "summary",
    ] {
        assert_eq!(schema["$defs"][definition]["additionalProperties"], false);
    }
    assert_eq!(schema["properties"]["pending_h1_0a"]["maxItems"], 0);
    assert_eq!(
        schema["properties"]["graph"]["properties"]["unresolved_calls"]["maxItems"],
        0
    );
}

#[test]
fn roots_paths_and_exact_edges_form_a_closed_graph() {
    let manifest = parsed();
    assert_eq!(manifest.active_roots.len(), ROOTS.len());
    let mut root_ids = BTreeMap::new();
    for (actual, expected) in manifest.active_roots.iter().zip(ROOTS) {
        verify_declaration(&actual.declaration);
        assert_eq!(actual.key, expected.0);
        assert_eq!(actual.declaration.name, expected.1);
        assert_eq!(actual.declaration.source_range.start.line, expected.2);
        root_ids.insert(actual.key.as_str(), actual.declaration.id.as_str());
    }

    assert_eq!(manifest.dormant_seams.len(), SEAMS.len());
    for (actual, expected) in manifest.dormant_seams.iter().zip(SEAMS) {
        verify_declaration(&actual.declaration);
        assert_eq!(actual.axis, expected.0);
        assert_eq!(actual.role, expected.1);
        assert_eq!(actual.declaration.name, expected.2);
        assert_eq!(actual.declaration.source_range.start.line, expected.3);
    }

    assert_eq!(manifest.functions.len(), 6_193);
    let mut functions = BTreeMap::new();
    for function in &manifest.functions {
        assert!(valid_h1_id(&function.id));
        assert!(!function.name.is_empty());
        assert!(!function.kind.is_empty());
        assert!(!function.lexical_path.is_empty());
        assert!(function.lexical_owner.as_deref().is_none_or(valid_h1_id));
        verify_range(&function.source_range);
        assert!(valid_sha256(&function.declaration_sha256));
        assert!(valid_sha256(&function.ledger_slice_sha256));
        match (&function.body_range, &function.body_sha256) {
            (Some(range), Some(hash)) => {
                verify_range(range);
                assert!(valid_sha256(hash));
            }
            (None, None) => {}
            _ => panic!("function body range and hash must appear together"),
        }
        assert!(!function.reachable_from.is_empty());
        assert!(function
            .reachable_from
            .windows(2)
            .all(|pair| pair[0] < pair[1]));
        assert!(function
            .reachable_from
            .iter()
            .all(|root| root_ids.contains_key(root.as_str())));
        assert!(functions.insert(function.id.as_str(), function).is_none());
    }
    for (key, id) in &root_ids {
        assert!(functions.contains_key(id));
        assert!(functions[*id].reachable_from.iter().any(|root| root == key));
    }

    assert_eq!(manifest.graph.edges.len(), 24_054);
    let mut edge_keys = BTreeSet::new();
    let mut edge_pairs = BTreeSet::new();
    let mut edge_counts = BTreeMap::new();
    for edge in &manifest.graph.edges {
        assert!(functions.contains_key(edge.caller.as_str()));
        assert!(functions.contains_key(edge.callee.as_str()));
        assert!(!edge.sites.is_empty());
        let mut sites = BTreeSet::new();
        for site in &edge.sites {
            assert!(site.line > 0 && site.character > 0);
            assert!(sites.insert((site.line, site.character)));
        }
        assert!(edge_keys.insert((edge.caller.as_str(), edge.callee.as_str(), edge.kind)));
        edge_pairs.insert((edge.caller.as_str(), edge.callee.as_str()));
        *edge_counts.entry(edge.kind).or_insert(0usize) += 1;
    }
    assert_eq!(edge_counts.get(&EdgeKind::Lexical), Some(&19_033));
    assert_eq!(edge_counts.get(&EdgeKind::Immediate), None);
    assert_eq!(edge_counts.get(&EdgeKind::DynamicSymbol), Some(&10));
    assert_eq!(edge_counts.get(&EdgeKind::NestedFunction), Some(&4_511));
    assert_eq!(edge_counts.get(&EdgeKind::PropertySymbol), Some(&500));

    for function in &manifest.functions {
        let shortest = &function.shortest_root_path;
        assert_eq!(shortest.declarations.last(), Some(&function.id));
        assert_eq!(
            shortest.declarations.first().map(String::as_str),
            root_ids.get(shortest.root.as_str()).copied()
        );
        assert!(function.reachable_from.contains(&shortest.root));
        for pair in shortest.declarations.windows(2) {
            assert!(
                edge_pairs.contains(&(pair[0].as_str(), pair[1].as_str())),
                "shortest path for {} contains a missing edge",
                function.id
            );
        }
    }

    let names = manifest
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect::<BTreeSet<_>>();
    for required in [
        "emitWorker",
        "getScriptTransformers",
        "getSourceFilesToEmit",
        "sourceFileMayBeEmitted",
        "getOutputExtension",
    ] {
        assert!(
            names.contains(required),
            "missing exact H1 owner {required}"
        );
    }
    assert!(!names.contains("getOutputJSFileName"));
}

#[test]
fn every_reviewed_call_has_an_explicit_disposition() {
    let manifest = parsed();
    let function_ids = manifest
        .functions
        .iter()
        .map(|function| function.id.as_str())
        .collect::<BTreeSet<_>>();
    let candidate_sets = manifest
        .graph
        .property_candidate_sets
        .iter()
        .map(|set| (set.property.as_str(), set))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(candidate_sets.len(), 442);

    let mut sites = BTreeSet::new();
    let mut identifier_counts = BTreeMap::new();
    let mut property_counts = BTreeMap::new();
    let mut dynamic_counts = BTreeMap::new();
    let mut structural_properties = BTreeSet::new();
    let mut external_globals = Vec::new();
    let mut stack_overflows = Vec::new();

    assert_eq!(manifest.graph.call_dispositions.len(), 5_202);
    for disposition in &manifest.graph.call_dispositions {
        let (caller, expression, expression_hash, line, character) = disposition_site(disposition);
        assert!(function_ids.contains(caller));
        assert!(!expression.is_empty());
        assert_eq!(sha256_hex(expression.as_bytes()), expression_hash);
        assert!(line > 0 && character > 0);
        assert!(sites.insert((caller, line, character, expression_hash)));

        match disposition {
            CallDisposition::Identifier {
                resolution,
                symbol_declaration_kinds,
                library_paths,
                ..
            } => {
                *identifier_counts.entry(*resolution).or_insert(0usize) += 1;
                assert!(symbol_declaration_kinds
                    .windows(2)
                    .all(|pair| pair[0] < pair[1]));
                assert!(library_paths.windows(2).all(|pair| pair[0] < pair[1]));
                match resolution {
                    IdentifierResolution::RuntimeLibrary => assert!(!library_paths.is_empty()),
                    IdentifierResolution::ParameterCallback => {
                        assert_eq!(symbol_declaration_kinds, &["Parameter"]);
                        assert!(library_paths.is_empty());
                    }
                    IdentifierResolution::DestructuredCallback => {
                        assert_eq!(symbol_declaration_kinds, &["BindingElement"]);
                        assert!(library_paths.is_empty());
                    }
                    IdentifierResolution::SourceValueCall => {
                        assert_eq!(symbol_declaration_kinds, &["VariableDeclaration"]);
                        assert!(library_paths.is_empty());
                    }
                    IdentifierResolution::ExternalGlobal => {
                        assert!(symbol_declaration_kinds.is_empty());
                        assert!(library_paths.is_empty());
                        external_globals.push((expression, line, character));
                    }
                }
            }
            CallDisposition::Property {
                property,
                receiver,
                resolution,
                checker_state,
                symbol_declaration_kinds,
                library_paths,
                source_callees,
                candidate_set,
                ..
            } => {
                *property_counts.entry(*resolution).or_insert(0usize) += 1;
                assert!(!property.is_empty());
                assert!(!receiver.is_empty());
                assert!(symbol_declaration_kinds
                    .windows(2)
                    .all(|pair| pair[0] < pair[1]));
                assert!(library_paths.windows(2).all(|pair| pair[0] < pair[1]));
                assert!(source_callees.windows(2).all(|pair| pair[0] < pair[1]));
                assert!(source_callees
                    .iter()
                    .all(|callee| function_ids.contains(callee.as_str())));
                if *checker_state == CheckerState::StackOverflow {
                    stack_overflows.push((expression, line, character));
                }
                match resolution {
                    PropertyResolution::SourceSymbol => {
                        assert_eq!(*checker_state, CheckerState::Resolved);
                        assert!(!source_callees.is_empty());
                        assert!(library_paths.is_empty());
                        assert!(candidate_set.is_none());
                    }
                    PropertyResolution::RuntimeLibrary => {
                        assert_eq!(*checker_state, CheckerState::Resolved);
                        assert!(source_callees.is_empty());
                        assert!(!library_paths.is_empty());
                        assert!(candidate_set.is_none());
                    }
                    PropertyResolution::StructuralDispatch => {
                        assert!(source_callees.is_empty());
                        assert!(library_paths.is_empty());
                        assert_eq!(candidate_set.as_deref(), Some(property.as_str()));
                        assert!(candidate_sets.contains_key(property.as_str()));
                        structural_properties.insert(property.as_str());
                    }
                }
            }
            CallDisposition::Dynamic {
                expression_kind,
                resolution,
                source_callees,
                ..
            } => {
                *dynamic_counts.entry(*resolution).or_insert(0usize) += 1;
                assert!(!expression_kind.is_empty());
                assert!(source_callees.windows(2).all(|pair| pair[0] < pair[1]));
                assert!(source_callees
                    .iter()
                    .all(|callee| function_ids.contains(callee.as_str())));
                if *resolution == DynamicResolution::SourceExpression {
                    assert!(!source_callees.is_empty());
                } else {
                    assert!(source_callees.is_empty());
                }
            }
        }
    }

    assert_eq!(
        identifier_counts.get(&IdentifierResolution::RuntimeLibrary),
        Some(&23)
    );
    assert_eq!(
        identifier_counts.get(&IdentifierResolution::ParameterCallback),
        Some(&279)
    );
    assert_eq!(
        identifier_counts.get(&IdentifierResolution::DestructuredCallback),
        Some(&114)
    );
    assert_eq!(
        identifier_counts.get(&IdentifierResolution::SourceValueCall),
        Some(&301)
    );
    assert_eq!(
        identifier_counts.get(&IdentifierResolution::ExternalGlobal),
        Some(&1)
    );
    assert_eq!(
        property_counts.get(&PropertyResolution::SourceSymbol),
        Some(&704)
    );
    assert_eq!(
        property_counts.get(&PropertyResolution::RuntimeLibrary),
        Some(&749)
    );
    assert_eq!(
        property_counts.get(&PropertyResolution::StructuralDispatch),
        Some(&3_021)
    );
    assert_eq!(dynamic_counts.values().sum::<usize>(), 10);
    assert_eq!(
        dynamic_counts.get(&DynamicResolution::SourceExpression),
        Some(&7)
    );
    assert_eq!(
        dynamic_counts.get(&DynamicResolution::ComputedElement),
        Some(&2)
    );
    assert_eq!(
        dynamic_counts.get(&DynamicResolution::ComputedExpression),
        Some(&1)
    );
    assert_eq!(dynamic_counts.get(&DynamicResolution::CallResult), None);
    assert_eq!(
        structural_properties,
        candidate_sets.keys().copied().collect()
    );
    assert_eq!(external_globals, [("onProfilerEvent", 2_582, 7)]);
    assert_eq!(stack_overflows, [("typeElements.push", 52_156, 11)]);
    assert!(manifest.graph.unresolved_calls.is_empty());
    assert!(manifest.pending_h1_0a.is_empty());
}

#[test]
fn structural_dispatch_candidate_sets_are_complete_review_data_not_edges() {
    let manifest = parsed();
    let mut properties = BTreeSet::new();
    let mut candidate_count = 0usize;
    let mut empty_sets = 0usize;
    for set in &manifest.graph.property_candidate_sets {
        assert!(!set.property.is_empty());
        assert!(properties.insert(set.property.as_str()));
        if set.candidates.is_empty() {
            empty_sets += 1;
        }
        let mut candidate_ids = BTreeSet::new();
        for candidate in &set.candidates {
            assert!(valid_h1_id(&candidate.id));
            assert_eq!(candidate.name, set.property);
            assert!(!candidate.kind.is_empty());
            assert!(candidate.lexical_owner.as_deref().is_none_or(valid_h1_id));
            assert!(candidate.line > 0 && candidate.character > 0);
            assert!(valid_sha256(&candidate.declaration_sha256));
            assert!(candidate_ids.insert(candidate.id.as_str()));
            candidate_count += 1;
        }
    }
    assert_eq!(manifest.graph.property_candidate_sets.len(), 442);
    assert_eq!(candidate_count, 652);
    assert_eq!(empty_sets, 28);
    assert!(manifest
        .graph
        .edges
        .iter()
        .all(|edge| edge.kind != EdgeKind::PropertySymbol || !edge.sites.is_empty()));
}

#[test]
fn summary_is_derived_from_the_reviewed_graph_and_h1_0a_is_closed() {
    let manifest = parsed();
    let summary = &manifest.summary;
    assert_eq!(summary.source_declarations, 10_899);
    assert_eq!(summary.active_roots, manifest.active_roots.len());
    assert_eq!(summary.dormant_seams, manifest.dormant_seams.len());
    assert_eq!(summary.closure_declarations, manifest.functions.len());
    assert_eq!(summary.static_edges, manifest.graph.edges.len());
    assert_eq!(summary.lexical_edges, 19_033);
    assert_eq!(summary.immediate_edges, 0);
    assert_eq!(summary.dynamic_symbol_edges, 10);
    assert_eq!(summary.nested_function_edges, 4_511);
    assert_eq!(summary.property_symbol_edges, 500);
    assert_eq!(summary.call_sites, 29_310);
    assert_eq!(
        summary.reviewed_call_sites,
        manifest.graph.call_dispositions.len()
    );
    assert_eq!(summary.identifier_runtime_library_calls, 23);
    assert_eq!(summary.identifier_parameter_callback_calls, 279);
    assert_eq!(summary.identifier_destructured_callback_calls, 114);
    assert_eq!(summary.identifier_source_value_calls, 301);
    assert_eq!(summary.identifier_external_global_calls, 1);
    assert_eq!(summary.property_source_symbol_calls, 704);
    assert_eq!(summary.property_runtime_library_calls, 749);
    assert_eq!(summary.property_structural_dispatch_calls, 3_021);
    assert_eq!(summary.property_checker_stack_overflow_calls, 1);
    assert_eq!(summary.dynamic_expression_calls, 10);
    assert_eq!(summary.dynamic_source_expression_calls, 7);
    assert_eq!(summary.reviewed_exact_edge_calls, 711);
    assert_eq!(summary.reviewed_non_edge_calls, 4_491);
    assert_eq!(
        summary.reviewed_exact_edge_calls + summary.reviewed_non_edge_calls,
        summary.reviewed_call_sites
    );
    assert_eq!(
        summary.property_candidate_sets,
        manifest.graph.property_candidate_sets.len()
    );
    assert_eq!(summary.property_candidate_declarations, 652);
    assert_eq!(summary.undispositioned_calls, 0);
    assert_eq!(manifest.completed_h1_0a.len(), 13);
    assert_eq!(
        manifest.completed_h1_0a.last().map(String::as_str),
        Some(
            "replace property-name fan-out with exact source-symbol edges and disposition every runtime-library, callback/value, structural-property, and computed call without unresolved rows"
        )
    );
    assert!(manifest.pending_h1_0a.is_empty());
}

#[test]
fn owner_inventory_generator_is_fresh() {
    let workspace = workspace();
    let output = Command::new("node")
        .current_dir(&workspace)
        .arg(GENERATOR_PATH)
        .arg("--check")
        .output()
        .expect("failed to run H1 owner inventory generator");
    assert!(
        output.status.success(),
        "H1 owner inventory generator failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
