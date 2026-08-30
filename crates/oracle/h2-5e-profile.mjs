import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-5e-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-5e-profile.v1.json";
const CONTRACT_RELATIVE_PATH = ".github/ci/contracts/h2-5e-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-5e-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH = "ratchets/h2-5e-owner-controls.v1.json";
const PARENT_PROFILE_RELATIVE_PATH = "ratchets/h2-5d-profile.v1.json";
const TRUSTED_BASE = "910e9f77fe89f3fb87fbdcb01340d308f6fdf7be";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-5d-profile.v1.json", "60b7b06b58a15885b27a29e12f9c6f85a77873840e21ed15c2063522f6091a3e"],
  ["qualification", "ratchets/h2-5d-qualification.v1.json", "0e08ab7a83d732c3d13d7399b3204b8d017bb2f29dc0c89aecb0fa517b6b4d03"],
  ["owner_controls", "ratchets/h2-5d-owner-controls.v1.json", "24a8dd9f39ccb92f9b9e3331c591248915f243bd0b5ff39d7fb04065ca040679"],
  ["profile_generator", "crates/oracle/h2-5d-profile.mjs", "71977f798bff68aa2bf9d872ef6c1716943586f93316096e4ec2a53eb3c746b1"],
  ["qualification_generator", "crates/oracle/h2-5d-qualification.mjs", "1c63d01da968b4a5b64c1f7d058ad9e99ebf2c0ed1be7482b3f20a7ae1d6cb7b"],
  ["owner_controls_generator", "crates/oracle/h2-5d-owner-controls.mjs", "492aee21efcd457ca8ce263023b296133a07ab1d5a947e2f21b288f1b22c85f6"],
  ["profile_contract", ".github/ci/contracts/h2-5d-profile.schema.json", "1c86c9c2ec78d60341b6eb04c309eb450b3817338237f58256568ed721b2e4a0"],
  ["qualification_contract", ".github/ci/contracts/h2-5d-qualification.schema.json", "3c0ba70ad066b2f0b4a67c2a49e81136f72a3d6cb8efa1d9f4e6e4fb874481f5"],
  ["owner_controls_contract", ".github/ci/contracts/h2-5d-owner-controls.schema.json", "a27154fd3641c5e1db64c138d5e5f46e06e1deca676524a155c3930e5e2627b6"],
]);

const NEW_RUNTIME_INPUTS = Object.freeze([
  "crates/emitter/src/builtins/es2018.rs",
  "crates/emitter/src/builtins/target_bindings.rs",
  "crates/emitter/tests/unit/generated_bindings/tests.rs",
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
      qualification.phase === "H2.5e-es2018-target" &&
      qualification.status === "qualified-typescript-oracle" &&
      qualification.selection_contract.global_h2_5e_rows === 163 &&
      qualification.selection_contract.global_candidate_denominator === 41 &&
      qualification.selection_contract.candidate_denominator === 41 &&
      qualification.selection_contract.future_deferred_rows === 122 &&
      qualification.summary.candidates === 41 &&
      qualification.summary.admitted_cases === 40 &&
      qualification.summary.deferred_cases === 1 &&
      qualification.summary.source_deferred_cases === 1 &&
      qualification.summary.admitted_typescript_writes === 46 &&
      qualification.summary.admitted_typescript_diagnostics === 88 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.5e qualification is not closed",
  );
  requireCondition(
    ownerControls.schema === 1 &&
      ownerControls.phase === "H2.5e-es2018-target-owner-controls" &&
      ownerControls.status === "qualified" &&
      ownerControls.summary.controls === 30 &&
      ownerControls.summary.exact_outputs === 29 &&
      ownerControls.summary.typescript_runs === 60 &&
      ownerControls.summary.reported_diagnostics === 1 &&
      ownerControls.summary.emit_diagnostics === 1 &&
      ownerControls.summary.no_emit_on_error_controls === 1 &&
      ownerControls.summary.es2017_controls === 29 &&
      ownerControls.summary.es2018_controls === 1 &&
      ownerControls.summary.object_spread_controls === 19 &&
      ownerControls.summary.object_rest_controls === 15 &&
      ownerControls.summary.for_await_controls === 4 &&
      ownerControls.summary.async_generator_controls === 9 &&
      ownerControls.summary.yield_star_controls === 2 &&
      ownerControls.summary.generated_name_composition_controls === 6 &&
      ownerControls.summary.parameter_controls === 5 &&
      ownerControls.summary.comment_controls === 1 &&
      ownerControls.summary.using_controls === 2 &&
      ownerControls.summary.standard_decorator_controls === 1 &&
      ownerControls.summary.class_composition_controls === 7 &&
      ownerControls.summary.commonjs_controls === 1 &&
      ownerControls.summary.h2_5a_active_controls === 29 &&
      ownerControls.summary.h2_5b_active_controls === 29 &&
      ownerControls.summary.h2_5c_active_controls === 29 &&
      ownerControls.summary.h2_5d_active_controls === 29 &&
      ownerControls.summary.h2_5e_active_controls === 28,
    "H2.5e owner controls are not closed",
  );
  requireCondition(
    parentProfile.schema === 1 &&
      parentProfile.phase === "H2.5d" &&
      parentProfile.admitted_profile.exact_cases === 632 &&
      parentProfile.summary.completed_runtime_slices === 19,
    "H2.5d parent profile is not closed",
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
    new Set(runtimeInputPaths).size === 85,
    "H2.5e runtime input identity changed",
  );

  return withFingerprint(
    {
      schema: 1,
      kind: "h2-runtime-profile",
      status: "qualified",
      phase: "H2.5e",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_5d_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.5d artifacts remain immutable lineage; current runtime ownership transfers to this H2.5e profile",
      },
      qualification: pathHash(QUALIFICATION_RELATIVE_PATH),
      runtime_inputs: runtimeInputPaths.map(pathHash),
      admitted_profile: {
        execution: "single-project-one-shot-whole-program",
        target_states: [
          "ES2017(4)", "ES2018(5)", "ES2019(6)", "ES2020(7)", "ES2021(8)",
          "ES2022(9)", "ES2023(10)", "ES2024(11)", "ES2025(12)", "ESNext(99)",
        ],
        module_states: [
          "absent-effective-ESNext", "ES2015(5)", "ES2020(6)", "ESNext(99)",
          "CommonJS(1)", "AMD(2)", "UMD(3)", "System(4)", "Node16(100)",
          "Node18(101)", "Node20(102)", "NodeNext(199)", "Preserve(200)",
        ],
        jsx_modes: [
          "Preserve(1)", "React(2)", "ReactNative(3)", "ReactJSX(4)", "ReactJSXDev(5)",
        ],
        source_kinds: [
          ".ts", ".mts", ".cts", ".tsx", ".js", ".mjs", ".cjs", ".jsx", ".json",
        ],
        products: ["javascript", "mjs", "cjs", "jsx", "json"],
        exact_cases: 672,
        h2_5e_exact_cases: 40,
        exact_reported_diagnostics: 1375,
        exact_writes: 966,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 15,
        candidate_denominator: 680,
        h2_5e_candidate_denominator: 41,
        h2_5e_global_future_rows: 122,
        h2_5e_owner_controls: 30,
        h2_5e_owner_writes: 29,
      },
      transition: {
        completed_slice: "H2.5e",
        next_slice: "H2.5f",
        active_runtime_slices: [
          "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a",
          "H2.2b", "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c",
          "H2.3d", "H2.4a", "H2.4b", "H2.5a", "H2.5b", "H2.5c",
          "H2.5d", "H2.5e",
        ],
        inactive_runtime_slice_count: 17,
        classic_jsx_tsx_owner: "complete",
        automatic_jsx_runtime_owner: "complete",
        json_output_owner: "complete",
        legacy_decorators_owner: "complete",
        standard_decorators_and_class_fields_owner: "complete",
        target_esnext_transform_owner: "complete",
        target_es2021_transform_owner: "complete",
        target_es2020_transform_owner: "complete",
        target_es2019_transform_owner: "complete",
        target_es2018_transform_owner: "complete",
        target_es2017_transform_owner: "H2.5f",
        general_output_matrix_owner: "H2.8a",
        h2_5e_candidate_cases: 41,
        h2_5e_admitted_cases: 40,
        h2_5e_global_future_rows: 122,
        h2_5e_source_deferred_cases: 1,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_5e_cases_and_owner_controls_run_twice_in_isolated_programs",
        denominator_control: "h2_5e_exact_denominator_is_41_with_1_h2_9_source_deferred_case_and_122_global_future_rows",
        target_band_control: "es2017_lowers_es2018_syntax_while_es2018_preserves_it",
        object_rest_spread_control: "spread_chunks_and_rest_destructuring_preserve_evaluation_and_comment_order",
        async_iteration_control: "for_await_and_async_generator_helpers_preserve_abrupt_completion_and_yield_order",
        generated_binding_control: "typed_binding_identity_reconciles_outer_inner_and_cross_transform_names",
        super_capture_control: "property_element_read_write_call_and_lexical_super_capture_match_tsc",
        composition_control: "using_decorators_class_fields_jsx_and_commonjs_compose_in_transform_order",
        diagnostic_control: "reported_emit_and_no_emit_on_error_diagnostics_match_tsc",
        printer_control: "helper_order_delimited_comments_and_synthetic_function_layout_are_exact",
        failure_control: "one_h2_9_source_fails_before_first_sink_write_and_no_emit_on_error_writes_nothing",
        owner_controls: {
          artifact: pathHash(OWNER_CONTROLS_RELATIVE_PATH),
          generator: pathHash("crates/oracle/h2-5e-owner-controls.mjs"),
          contract: pathHash(".github/ci/contracts/h2-5e-owner-controls.schema.json"),
        },
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_5d_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 20,
        next_runtime_slices: 1,
        runtime_admissions: 672,
        executed_candidates: 680,
        h2_5e_executed_candidates: 41,
        h2_5e_global_future_rows: 122,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-5e-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.5e profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-5e-profile.mjs [--write|--check]");
}
