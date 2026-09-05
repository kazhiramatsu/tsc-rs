import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-5f-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-5f-profile.v1.json";
const CONTRACT_RELATIVE_PATH = ".github/ci/contracts/h2-5f-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-5f-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH = "ratchets/h2-5f-owner-controls.v1.json";
const PARENT_PROFILE_RELATIVE_PATH = "ratchets/h2-5e-profile.v1.json";
const TRUSTED_BASE = "4d59ca19507219e91cf5ea1f9af02057d1d551ac";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-5e-profile.v1.json", "efab0a79605bc4433129fd749fe0e29f9ef37acfec4918a338430cebd7dc0164"],
  ["qualification", "ratchets/h2-5e-qualification.v1.json", "9af1dec31fd9987cc72b6665fab6695fe3fcb4fa957d171333bb0e487c8b8339"],
  ["owner_controls", "ratchets/h2-5e-owner-controls.v1.json", "7297b7ce90f419fca8605251db4018baa9bfb6675b4af20940296ec72f67c995"],
  ["profile_generator", "crates/oracle/h2-5e-profile.mjs", "7847deba92d2c9fbeb1b65005440fbb0eb553ca427503ba3c8ca6c7661a132ad"],
  ["qualification_generator", "crates/oracle/h2-5e-qualification.mjs", "fec4ce713106c044b09a04ae98b0870607ddcc9dae5f61ef10c31a76b8b8a35d"],
  ["owner_controls_generator", "crates/oracle/h2-5e-owner-controls.mjs", "93be61dd5ebd4fd9733af3e614b4d7a3debc7ce81d038111b0d16c000810ab9b"],
  ["profile_contract", ".github/ci/contracts/h2-5e-profile.schema.json", "5de73cee2616a6bf5d033995750bb29097b16aad3197f029e07cb47a53801f33"],
  ["qualification_contract", ".github/ci/contracts/h2-5e-qualification.schema.json", "659a7cfd447ba51ca5278f6f435b0d700fb37ac993b78579f1d75ae3e8750fb0"],
  ["owner_controls_contract", ".github/ci/contracts/h2-5e-owner-controls.schema.json", "87b11b989988c5d5079505bd9ba3f243d1c59ae6a5bb0f3857b55f102e8e5fe1"],
]);

const NEW_RUNTIME_INPUTS = Object.freeze([
  "crates/emitter/src/builtins/es2017.rs",
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
      qualification.phase === "H2.5f-es2017-target" &&
      qualification.status === "qualified-typescript-oracle" &&
      qualification.selection_contract.global_h2_5f_rows === 9 &&
      qualification.selection_contract.global_candidate_denominator === 8 &&
      qualification.selection_contract.candidate_denominator === 8 &&
      qualification.selection_contract.future_deferred_rows === 1 &&
      qualification.summary.candidates === 8 &&
      qualification.summary.admitted_cases === 8 &&
      qualification.summary.deferred_cases === 0 &&
      qualification.summary.source_deferred_cases === 0 &&
      qualification.summary.admitted_typescript_writes === 8 &&
      qualification.summary.admitted_typescript_diagnostics === 20 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.5f qualification is not closed",
  );
  requireCondition(
    ownerControls.schema === 1 &&
      ownerControls.phase === "H2.5f-es2017-target-owner-controls" &&
      ownerControls.status === "qualified" &&
      ownerControls.summary.controls === 21 &&
      ownerControls.summary.exact_outputs === 20 &&
      ownerControls.summary.typescript_runs === 42 &&
      ownerControls.summary.reported_diagnostics === 2 &&
      ownerControls.summary.emit_diagnostics === 1 &&
      ownerControls.summary.no_emit_on_error_controls === 1 &&
      ownerControls.summary.es2016_controls === 20 &&
      ownerControls.summary.es2017_controls === 1 &&
      ownerControls.summary.async_function_controls === 21 &&
      ownerControls.summary.await_controls === 21 &&
      ownerControls.summary.parameter_controls === 4 &&
      ownerControls.summary.collision_controls === 3 &&
      ownerControls.summary.lexical_arguments_controls === 2 &&
      ownerControls.summary.super_controls === 3 &&
      ownerControls.summary.precedence_controls === 1 &&
      ownerControls.summary.comment_controls === 1 &&
      ownerControls.summary.class_composition_controls === 5 &&
      ownerControls.summary.commonjs_controls === 1 &&
      ownerControls.summary.h2_5a_active_controls === 20 &&
      ownerControls.summary.h2_5b_active_controls === 20 &&
      ownerControls.summary.h2_5c_active_controls === 20 &&
      ownerControls.summary.h2_5d_active_controls === 20 &&
      ownerControls.summary.h2_5e_active_controls === 20 &&
      ownerControls.summary.h2_5f_active_controls === 19,
    "H2.5f owner controls are not closed",
  );
  requireCondition(
    parentProfile.schema === 1 &&
      parentProfile.phase === "H2.5e" &&
      parentProfile.admitted_profile.exact_cases === 672 &&
      parentProfile.summary.completed_runtime_slices === 20,
    "H2.5e parent profile is not closed",
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
    new Set(runtimeInputPaths).size === 86,
    "H2.5f runtime input identity changed",
  );

  return withFingerprint(
    {
      schema: 1,
      kind: "h2-runtime-profile",
      status: "qualified",
      phase: "H2.5f",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_5e_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.5e artifacts remain immutable lineage; current runtime ownership transfers to this H2.5f profile",
      },
      qualification: pathHash(QUALIFICATION_RELATIVE_PATH),
      runtime_inputs: runtimeInputPaths.map(pathHash),
      admitted_profile: {
        execution: "single-project-one-shot-whole-program",
        target_states: [
          "ES2016(3)", "ES2017(4)", "ES2018(5)", "ES2019(6)", "ES2020(7)", "ES2021(8)",
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
        exact_cases: 680,
        h2_5f_exact_cases: 8,
        exact_reported_diagnostics: 1395,
        exact_writes: 974,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 15,
        candidate_denominator: 688,
        h2_5f_candidate_denominator: 8,
        h2_5f_global_future_rows: 1,
        h2_5f_owner_controls: 21,
        h2_5f_owner_writes: 20,
      },
      transition: {
        completed_slice: "H2.5f",
        next_slice: "H2.5g",
        active_runtime_slices: [
          "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a",
          "H2.2b", "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c",
          "H2.3d", "H2.4a", "H2.4b", "H2.5a", "H2.5b", "H2.5c",
          "H2.5d", "H2.5e", "H2.5f",
        ],
        inactive_runtime_slice_count: 16,
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
        target_es2017_transform_owner: "complete",
        target_es2016_transform_owner: "H2.5g",
        general_output_matrix_owner: "H2.8a",
        h2_5f_candidate_cases: 8,
        h2_5f_admitted_cases: 8,
        h2_5f_global_future_rows: 1,
        h2_5f_source_deferred_cases: 0,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_5f_cases_and_owner_controls_run_twice_in_isolated_programs",
        denominator_control: "h2_5f_exact_denominator_is_8_with_0_source_deferred_cases_and_1_global_future_row",
        target_band_control: "es2016_lowers_es2017_syntax_while_es2017_preserves_it",
        async_function_control: "declarations_expressions_arrows_methods_parameters_and_await_precedence_match_tsc",
        generated_binding_control: "typed_binding_identity_reconciles_parameter_arguments_and_var_collision_names",
        lexical_capture_control: "resolver_owned_arguments_and_super_capture_preserve_lexical_boundaries",
        composition_control: "object_rest_async_generators_decorators_class_fields_and_commonjs_compose_in_transform_order",
        diagnostic_control: "reported_emit_and_no_emit_on_error_diagnostics_match_tsc",
        printer_control: "helper_order_delimited_comments_and_synthetic_function_layout_are_exact",
        failure_control: "no_emit_on_error_writes_nothing_before_the_first_sink_write",
        owner_controls: {
          artifact: pathHash(OWNER_CONTROLS_RELATIVE_PATH),
          generator: pathHash("crates/oracle/h2-5f-owner-controls.mjs"),
          contract: pathHash(".github/ci/contracts/h2-5f-owner-controls.schema.json"),
        },
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_5e_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 21,
        next_runtime_slices: 1,
        runtime_admissions: 680,
        executed_candidates: 688,
        h2_5f_executed_candidates: 8,
        h2_5f_global_future_rows: 1,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-5f-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.5f profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-5f-profile.mjs [--write|--check]");
}
