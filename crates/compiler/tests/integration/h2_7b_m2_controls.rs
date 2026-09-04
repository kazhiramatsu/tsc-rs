//! h2-7b-m-2 controls — the first-sweep STOP conditions frozen as controls.
//!
//! Fence amendment #4: the production declaration member drove the
//! NodeBuilder's annotation-reuse decision into `getOptionalType` for a
//! mapped-type optional property under `strictNullChecks: false`; upstream
//! reaches that decision through `addOptionality`, whose `strictNullChecks`
//! gate keeps the annotation as-is (_tsc.js:56029-56031, :50932-50955).
//! The control replays the exact band row that stopped the sweep through
//! the production emit entry the runner uses and pins the frozen `.d.ts`
//! bytes of the m-1 artifact.

use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::Value;
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_harness::upstream_suites::execution::{
    load_qualified_compiler_emit_with_option_floor, EmitOptionFloor,
};
use tsc_program::ProgramLoadLimits;

const QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-7b-qualification.v1.json";
const OPTIONAL_MAPPED_PROPERTY_CASE: &str =
    "typescript-6.0.3/compiler/declarationEmitOptionalMappedTypePropertyNoStrictNullChecks1.ts#default";
/// Fence amendment #4b: a module file directly under `node_modules`
/// (`/.src/node_modules/umd.d.ts`) whose package-root index sits at the end
/// of the path — the specifier builder's `indexOf(directorySeparator,
/// packageRootIndex + 1)` port must yield "no further separator", not slice
/// past the end.
const NODE_MODULES_FILE_SPECIFIER_CASE: &str =
    "typescript-6.0.3/compiler/importShouldNotBeElidedInDeclarationEmit.ts#default";
/// Fence amendment #4c: the recursive inaccessible alias row (TS7056 ×3).
const RECURSIVE_INACCESSIBLE_ALIAS_CASE: &str =
    "typescript-6.0.3/compiler/declarationEmitPrivatePromiseLikeInterface.ts#default";
/// Fence amendment #4d: literal-const declarations of enum members.
const COMPUTED_ENUM_WIDENING_CASE: &str =
    "typescript-6.0.3/compiler/computedEnumTypeWidening.ts#default";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn frozen_case(case_id: &str) -> Value {
    let artifact: Value = serde_json::from_slice(
        &std::fs::read(workspace_root().join(QUALIFICATION_RELATIVE_PATH))
            .expect("the frozen H2.7b qualification artifact"),
    )
    .expect("valid qualification JSON");
    artifact["cases"]
        .as_array()
        .expect("qualification cases")
        .iter()
        .find(|case| case["case_id"] == case_id)
        .cloned()
        .unwrap_or_else(|| panic!("{case_id} is not a frozen band row"))
}

fn decode(value: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(value.as_str().expect("base64 text"))
        .expect("valid base64")
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(128, 1_024, 32, 8 * 1_024 * 1_024, 64 * 1_024 * 1_024)
}

/// The runner's qualified-VFS route for one frozen row: the recorded files,
/// virtual config, roots, and directive settings under the declaration
/// family floor.
fn prepared_band_row(case: &Value) -> tsc_program::PreparedProgram {
    let input = &case["input"];
    let mut files = input["files"]
        .as_array()
        .expect("case input files")
        .iter()
        .map(|file| {
            (
                PathBuf::from(file["path"].as_str().expect("file path")),
                decode(&file["utf8_base64"]),
            )
        })
        .collect::<Vec<_>>();
    if !input["virtual_config"].is_null() {
        let config = &input["virtual_config"];
        files.push((
            PathBuf::from(config["path"].as_str().expect("config path")),
            decode(&config["utf8_base64"]),
        ));
    }
    let roots = input["roots"]
        .as_array()
        .expect("case roots")
        .iter()
        .map(|root| PathBuf::from(root.as_str().expect("root path")))
        .collect::<Vec<_>>();
    let settings = input["settings"]
        .as_array()
        .expect("case settings")
        .iter()
        .map(|setting| {
            (
                setting["name"].as_str().expect("setting name").to_owned(),
                setting["value"].as_str().expect("setting value").to_owned(),
            )
        })
        .collect::<Vec<_>>();
    load_qualified_compiler_emit_with_option_floor(
        &workspace_root(),
        input["current_directory"]
            .as_str()
            .expect("case current directory"),
        &files,
        &roots,
        &settings,
        limits(),
        EmitOptionFloor::DeclarationFamily,
    )
    .expect("the frozen band row loads through the declaration-family floor")
}

/// Replays one frozen band row through the runner's production entry and
/// pins its frozen declaration writes byte-exact (paths, texts, count),
/// `emitSkipped` false, and the frozen reported-diagnostic codes.
fn assert_frozen_declaration_writes(case_id: &str) {
    let case = frozen_case(case_id);
    assert_eq!(case["disposition"], "admitted-for-execution", "{case_id}");
    let prepared = prepared_band_row(&case);
    let mut sink = MemoryOutputSink::new();
    let (outcome, reported) = ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .unwrap_or_else(|error| panic!("{case_id}: the production emit completes: {error}"));
    let expected_skipped = case["typescript_observation"]["emit_result"]["emit_skipped"]
        .as_bool()
        .expect("frozen emitSkipped");
    assert_eq!(
        outcome.emit_skipped(),
        expected_skipped,
        "{case_id}: emitSkipped"
    );
    let expected_codes = case["typescript_observation"]["reported_diagnostics"]
        .as_array()
        .expect("frozen reported diagnostics")
        .iter()
        .map(|diagnostic| diagnostic["code"].as_u64().expect("diagnostic code"))
        .collect::<Vec<_>>();
    assert_eq!(
        reported
            .iter()
            .map(|diagnostic| u64::from(diagnostic.code()))
            .collect::<Vec<_>>(),
        expected_codes,
        "{case_id}: reported diagnostic codes"
    );
    let expected_writes = case["typescript_observation"]["writes"]
        .as_array()
        .expect("frozen writes");
    assert_eq!(
        sink.writes().len(),
        expected_writes.len(),
        "{case_id}: exact write count"
    );
    for (write, expected) in sink.writes().iter().zip(expected_writes) {
        assert_eq!(
            write.path(),
            Path::new(expected["path"].as_str().expect("frozen write path")),
            "{case_id}: exact output path"
        );
        let expected_text =
            String::from_utf8(decode(&expected["callback_utf8_base64"])).expect("UTF-8 text");
        assert_eq!(
            write.callback_text(),
            expected_text,
            "{case_id}: exact frozen bytes for {}",
            write.path().display()
        );
    }
}

/// Fence amendment #4: `getOptionalType` reached without `strictNullChecks`
/// from the annotation-reuse decision (upstream gates it in `addOptionality`).
#[test]
fn optional_mapped_type_property_without_strict_null_checks_emits_the_frozen_declarations() {
    let case = frozen_case(OPTIONAL_MAPPED_PROPERTY_CASE);
    assert_eq!(
        case["effective_declaration_options"]["strictNullChecks"], false,
        "the control row runs without strictNullChecks"
    );
    assert_eq!(
        prepared_band_row(&case)
            .compiler_options()
            .strict_null_checks,
        Some(false),
        "the harness floor keeps the explicit strictNullChecks: false"
    );
    assert_frozen_declaration_writes(OPTIONAL_MAPPED_PROPERTY_CASE);
}

/// Fence amendment #4b: the node-module specifier walk for a file directly
/// under `node_modules` (`tryGetModuleNameAsNodeModule`'s `indexOf` from an
/// index at the end of the path).
#[test]
fn node_modules_file_specifier_emits_the_frozen_declarations() {
    let case = frozen_case(NODE_MODULES_FILE_SPECIFIER_CASE);
    assert!(
        case["input"]["files"]
            .as_array()
            .expect("case files")
            .iter()
            .any(|file| file["path"] == "/.src/node_modules/umd.d.ts"),
        "the control row imports a module file directly under node_modules"
    );
    assert_frozen_declaration_writes(NODE_MODULES_FILE_SPECIFIER_CASE);
}

/// Fence amendment #4c: the exponential re-serialization of a recursive,
/// inaccessible type alias (`TPromise`'s `then` / `catch` return types) —
/// upstream's per-enclosing-declaration `serializedTypes` reuse keeps it
/// linear until the truncation length reports TS7056 and blocks the `.d.ts`
/// (emitSkipped true, the JavaScript members still written).
#[test]
fn recursive_inaccessible_alias_truncates_with_ts7056_and_blocks_the_declaration() {
    let case = frozen_case(RECURSIVE_INACCESSIBLE_ALIAS_CASE);
    assert_eq!(
        case["typescript_observation"]["emit_result"]["emit_skipped"], true,
        "the frozen row is transform-blocked"
    );
    assert_frozen_declaration_writes(RECURSIVE_INACCESSIBLE_ALIAS_CASE);
}

/// Fence amendment #4d: `literalTypeToNode`'s EnumLike arm (`type.flags &
/// EnumLike`, either bit) — a literal-const declaration of a computed enum
/// member (`const c1 = E.B` with `B = computed(1)`) prints the member
/// expression `E.B`, never the value.
#[test]
fn computed_enum_member_literal_const_prints_the_member_expression() {
    assert_frozen_declaration_writes(COMPUTED_ENUM_WIDENING_CASE);
}

/// Fence amendment #4e: the five first-sweep rows whose declaration
/// serialization reuses an annotation that lives in ANOTHER source file (a
/// re-export serializing an imported declaration). TypeScript's single node
/// pool reuses it; the Rust arena keys node handles by source, so the reuse
/// walk is skipped for foreign-source annotations and the type is serialized
/// structurally — the production emit must COMPLETE (the byte difference is a
/// manifest row owned by the first closure wave, never an error).
#[test]
fn cross_file_annotation_reuse_rows_complete_the_production_emit() {
    for case_id in [
        "typescript-6.0.3/compiler/declarationEmitMappedTypeDistributivityPreservesConstraints.ts#default",
        "typescript-6.0.3/compiler/declarationEmitPartialReuseComputedProperty.ts#default",
        "typescript-6.0.3/compiler/declarationEmitResolveTypesIfNotReusable.ts#default",
        "typescript-6.0.3/compiler/mappedTypeWithAsClauseAndLateBoundProperty2.ts#default",
        "typescript-6.0.3/conformance/types/typeRelationships/typeInference/noInferRedeclaration.ts#default",
    ] {
        let case = frozen_case(case_id);
        let prepared = prepared_band_row(&case);
        let mut sink = MemoryOutputSink::new();
        let (outcome, _reported) = ProgramSession::new(prepared)
            .emit_with_reported_diagnostics_for_harness(&mut sink)
            .unwrap_or_else(|error| panic!("{case_id}: the production emit completes: {error}"));
        let expected_skipped = case["typescript_observation"]["emit_result"]["emit_skipped"]
            .as_bool()
            .expect("frozen emitSkipped");
        assert_eq!(outcome.emit_skipped(), expected_skipped, "{case_id}: emitSkipped");
        let expected_writes = case["typescript_observation"]["writes"]
            .as_array()
            .expect("frozen writes");
        assert_eq!(
            sink.writes().len(),
            expected_writes.len(),
            "{case_id}: every frozen member is written (bytes may diverge — a manifest row)"
        );
    }
}

/// Fence amendment #4e (b): a mapped type without a type clause
/// (`{ [K in keyof T] }`) prints `: ;` like upstream's `emit(undefined)`.
#[test]
fn mapped_type_without_a_type_clause_prints_the_frozen_declaration() {
    assert_frozen_declaration_writes(
        "typescript-6.0.3/compiler/mappedTypeNoTypeNoCrash.ts#default",
    );
}

/// Fence amendment #4e (revised): a reused type literal from another file
/// carries a computed property name whose symbol is inaccessible — the
/// syntactic walk's recovery boundary DEFERS the tracker's
/// `reportInaccessibleUniqueSymbolError` (upstream `createRecoveryBoundary`
/// wraps the six error reports with `markError`), the enclosing type node
/// recovers to the checker's serialization, and the declaration reports
/// TS4023 (the accessibility error) — never TS2527.
#[test]
fn foreign_computed_property_name_reuse_defers_the_tracker_reports_like_upstream() {
    assert_frozen_declaration_writes(
        "typescript-6.0.3/compiler/declarationEmitComputedPropertyNameSymbol2.ts#default",
    );
}
