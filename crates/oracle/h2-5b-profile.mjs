import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-5b-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-5b-profile.v1.json";
const CONTRACT_RELATIVE_PATH = ".github/ci/contracts/h2-5b-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-5b-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH = "ratchets/h2-5b-owner-controls.v1.json";
const PARENT_PROFILE_RELATIVE_PATH = "ratchets/h2-5a-profile.v1.json";
const TRUSTED_BASE = "1676080a3efa759464a17fe91c96bbc807f38fc5";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-5a-profile.v1.json", "5c01136145e8f97b11f678c6069c5b533214260bff9e87d9fdaa44f2d1cbc5e5"],
  ["qualification", "ratchets/h2-5a-qualification.v1.json", "e4d2af4403fb410ca00665de3805a4370de7ab8cc048660f2f7289c4decd71b6"],
  ["owner_controls", "ratchets/h2-5a-owner-controls.v1.json", "f008dee2485e79ca2868271afb5bd361893c454ed9ed215b3b30908524960eeb"],
  ["profile_generator", "crates/oracle/h2-5a-profile.mjs", "bc08c7a78d824657b2c7de40d58ea7b9b80991803dfb084cc8aabeb94e87f13c"],
  ["qualification_generator", "crates/oracle/h2-5a-qualification.mjs", "a8438cc66352026eb8de3d3a6fb6d40473c22b9d03941313f40bf9bde04ca46d"],
  ["owner_controls_generator", "crates/oracle/h2-5a-owner-controls.mjs", "2fe85aa628428bc043174305854fe851e9ae78f9be3fffc9f195065035c6a091"],
  ["profile_contract", ".github/ci/contracts/h2-5a-profile.schema.json", "c28518cd56acb805b319a3319765ac55578f2565f1b40c4e9b5c67f71af08607"],
  ["qualification_contract", ".github/ci/contracts/h2-5a-qualification.schema.json", "d47dbd5fa81860e3be9ac66be4454f8ccb7d205a5178ce1daa12e0aa324188c6"],
  ["owner_controls_contract", ".github/ci/contracts/h2-5a-owner-controls.schema.json", "9979927dc11ca75cf42739dfb77766165994ba9814c29177770c1de60aa411a3"],
]);

const NEW_RUNTIME_INPUTS = Object.freeze([
  "crates/checker/src/annotate.rs",
  "crates/checker/src/calls.rs",
  "crates/emitter/src/builtins/es2021.rs",
  "crates/emitter/src/builtins/generated_bindings.rs",
  "crates/emitter/tests/integration/factory_transform_contract.rs",
  "crates/emitter/tests/integration/printer_foundation_contract.rs",
  "crates/emitter/tests/integration/printer_oracle_contract.rs",
]);

function fail(message) {
  throw new Error(message);
}

function requireCondition(condition, message) {
  if (!condition) fail(message);
}

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value !== null && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function readBytes(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE, relativePath));
}

function readJson(relativePath) {
  return JSON.parse(readBytes(relativePath).toString("utf8"));
}

function pathHash(relativePath) {
  return { path: relativePath, sha256: sha256(readBytes(relativePath)) };
}

function withFingerprint(value, field) {
  return { ...value, [field]: sha256(Buffer.from(canonical(value), "utf8")) };
}

function buildArtifact() {
  const qualification = readJson(QUALIFICATION_RELATIVE_PATH);
  const ownerControls = readJson(OWNER_CONTROLS_RELATIVE_PATH);
  const parentProfile = readJson(PARENT_PROFILE_RELATIVE_PATH);
  requireCondition(
    qualification.schema === 1 &&
      qualification.phase === "H2.5b-es2021-target" &&
      qualification.status === "qualified-typescript-oracle" &&
      qualification.selection_contract.global_h2_5b_rows === 84 &&
      qualification.selection_contract.global_candidate_denominator === 72 &&
      qualification.selection_contract.candidate_denominator === 72 &&
      qualification.selection_contract.future_deferred_rows === 12 &&
      qualification.summary.candidates === 72 &&
      qualification.summary.admitted_cases === 68 &&
      qualification.summary.deferred_cases === 4 &&
      qualification.summary.source_deferred_cases === 4 &&
      qualification.summary.admitted_typescript_writes === 93 &&
      qualification.summary.admitted_typescript_diagnostics === 48 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.5b qualification is not closed",
  );
  requireCondition(
    ownerControls.schema === 1 &&
      ownerControls.phase === "H2.5b-es2021-target-owner-controls" &&
      ownerControls.status === "qualified" &&
      ownerControls.summary.controls === 20 &&
      ownerControls.summary.exact_outputs === 19 &&
      ownerControls.summary.typescript_runs === 40 &&
      ownerControls.summary.reported_diagnostics === 1 &&
      ownerControls.summary.emit_diagnostics === 1 &&
      ownerControls.summary.no_emit_on_error_controls === 1 &&
      ownerControls.summary.es2020_controls === 19 &&
      ownerControls.summary.es2021_controls === 1 &&
      ownerControls.summary.logical_assignment_controls === 19 &&
      ownerControls.summary.parameter_hoist_controls === 3 &&
      ownerControls.summary.standard_decorator_controls === 1 &&
      ownerControls.summary.legacy_decorator_controls === 1 &&
      ownerControls.summary.commonjs_controls === 1 &&
      ownerControls.summary.h2_5a_active_controls === 19 &&
      ownerControls.summary.h2_5b_active_controls === 18,
    "H2.5b owner controls are not closed",
  );
  requireCondition(
    parentProfile.schema === 1 &&
      parentProfile.phase === "H2.5a" &&
      parentProfile.admitted_profile.exact_cases === 527 &&
      parentProfile.summary.completed_runtime_slices === 16,
    "H2.5a parent profile is not closed",
  );

  const historical = Object.fromEntries(
    HISTORICAL_AUTHORITIES.map(([key, relativePath, expected]) => {
      const record = pathHash(relativePath);
      requireCondition(record.sha256 === expected, `${relativePath} historical bytes changed`);
      return [key, record];
    }),
  );
  const runtimeInputPaths = [
    ...parentProfile.runtime_inputs.map((record) => record.path),
    ...NEW_RUNTIME_INPUTS,
  ];
  requireCondition(
    new Set(runtimeInputPaths).size === 79,
    "H2.5b runtime input identity changed",
  );

  return withFingerprint(
    {
      schema: 1,
      kind: "h2-runtime-profile",
      status: "qualified",
      phase: "H2.5b",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_5a_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.5a artifacts remain immutable lineage; current runtime ownership transfers to this H2.5b profile",
      },
      qualification: pathHash(QUALIFICATION_RELATIVE_PATH),
      runtime_inputs: runtimeInputPaths.map(pathHash),
      admitted_profile: {
        execution: "single-project-one-shot-whole-program",
        target_states: [
          "ES2020(7)", "ES2021(8)", "ES2022(9)", "ES2023(10)",
          "ES2024(11)", "ES2025(12)", "ESNext(99)",
        ],
        module_states: [
          "absent-effective-ESNext", "ES2015(5)", "ES2020(6)", "ESNext(99)",
          "CommonJS(1)", "AMD(2)", "UMD(3)", "System(4)", "Node16(100)",
          "Node18(101)", "Node20(102)", "NodeNext(199)", "Preserve(200)",
        ],
        jsx_modes: [
          "Preserve(1)", "React(2)", "ReactNative(3)",
          "ReactJSX(4)", "ReactJSXDev(5)",
        ],
        source_kinds: [
          ".ts", ".mts", ".cts", ".tsx", ".js", ".mjs", ".cjs", ".jsx", ".json",
        ],
        products: ["javascript", "mjs", "cjs", "jsx", "json"],
        exact_cases: 595,
        h2_5b_exact_cases: 68,
        exact_reported_diagnostics: 1221,
        exact_writes: 849,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 12,
        candidate_denominator: 600,
        h2_5b_candidate_denominator: 72,
        h2_5b_global_future_rows: 12,
        h2_5b_owner_controls: 20,
        h2_5b_owner_writes: 19,
      },
      transition: {
        completed_slice: "H2.5b",
        next_slice: "H2.5c",
        active_runtime_slices: [
          "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a",
          "H2.2b", "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c",
          "H2.3d", "H2.4a", "H2.4b", "H2.5a", "H2.5b",
        ],
        inactive_runtime_slice_count: 20,
        classic_jsx_tsx_owner: "complete",
        automatic_jsx_runtime_owner: "complete",
        json_output_owner: "complete",
        legacy_decorators_owner: "complete",
        standard_decorators_and_class_fields_owner: "complete",
        target_esnext_transform_owner: "complete",
        target_es2021_transform_owner: "complete",
        target_es2020_transform_owner: "H2.5c",
        general_output_matrix_owner: "H2.8a",
        h2_5b_candidate_cases: 72,
        h2_5b_admitted_cases: 68,
        h2_5b_global_future_rows: 12,
        h2_5b_source_deferred_cases: 4,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_5b_cases_and_owner_controls_run_twice_in_isolated_programs",
        denominator_control: "h2_5b_exact_denominator_is_72_with_12_future_deferred_rows",
        target_band_control: "es2020_lowers_es2021_syntax_while_es2021_preserves_it",
        logical_assignment_control: "identifier_property_element_super_nested_and_parenthesized_logical_assignments_are_exact",
        evaluation_order_control: "effectful_receivers_keys_and_rhs_values_are_evaluated_once_in_tsc_order",
        generated_binding_control: "source_function_parameter_and_nested_default_temporaries_have_typed_scope_ownership",
        class_fields_control: "public_private_static_instance_and_decorator_field_composition_is_exact",
        module_interaction_control: "es_module_commonjs_node_and_preserve_outputs_including_import_defer_are_exact",
        printer_control: "canonical_compiler_print_numeric_spelling_deferred_import_phase_and_comments_are_exact",
        failure_control: "four_h2_9_sources_fail_before_first_sink_write_and_no_emit_on_error_writes_nothing",
        owner_controls: {
          artifact: pathHash(OWNER_CONTROLS_RELATIVE_PATH),
          generator: pathHash("crates/oracle/h2-5b-owner-controls.mjs"),
          contract: pathHash(".github/ci/contracts/h2-5b-owner-controls.schema.json"),
        },
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_5a_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 17,
        next_runtime_slices: 1,
        runtime_admissions: 595,
        executed_candidates: 600,
        h2_5b_executed_candidates: 72,
        h2_5b_global_future_rows: 12,
        unexecuted_candidates: 0,
        undispositioned_candidates: 0,
        historical_artifacts_reinterpreted: 0,
      },
    },
    "profile_fingerprint_sha256",
  );
}

function render(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

const artifact = buildArtifact();
const rendered = render(artifact);
const mode = process.argv[2];
if (mode === "--write") {
  fs.writeFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), rendered);
  process.stdout.write(
    `wrote ${TARGET_RELATIVE_PATH}: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === "--check") {
  requireCondition(
    fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
      fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") === rendered,
    `stale ${TARGET_RELATIVE_PATH}; run h2-5b-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.5b profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-5b-profile.mjs [--write|--check]");
}
