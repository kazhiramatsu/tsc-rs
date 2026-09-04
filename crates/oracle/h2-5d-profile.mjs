import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-5d-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-5d-profile.v1.json";
const CONTRACT_RELATIVE_PATH = ".github/ci/contracts/h2-5d-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-5d-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH = "ratchets/h2-5d-owner-controls.v1.json";
const PARENT_PROFILE_RELATIVE_PATH = "ratchets/h2-5c-profile.v1.json";
const TRUSTED_BASE = "375e32fe559988ce36ce584839127c55669e6b3e";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-5c-profile.v1.json", "ccbd784e957013f275b9b16811b324cd3f4dd8becba49a2ab059f0f2e8be916d"],
  ["qualification", "ratchets/h2-5c-qualification.v1.json", "a4bb1f01c890248981f11d7446669121ca8204469e5bdb9d49ad90a5f3e56721"],
  ["owner_controls", "ratchets/h2-5c-owner-controls.v1.json", "b358f50c8ed747bf7ecc94c764a72bbc92c8b9c04799d173d9657534745f9030"],
  ["profile_generator", "crates/oracle/h2-5c-profile.mjs", "f7678e3b14ea3e82650c26b15679a2ef5e202385e457b9c7334b74f4082d9658"],
  ["qualification_generator", "crates/oracle/h2-5c-qualification.mjs", "42eaa277bc82b5502f15e729118e44638283b3c6721113151eb6fbcda3ae4c13"],
  ["owner_controls_generator", "crates/oracle/h2-5c-owner-controls.mjs", "f6703a4532393bbd5b0ce85d5959c9af70ce02ce0b7c4cdbfb445aff90fdc086"],
  ["profile_contract", ".github/ci/contracts/h2-5c-profile.schema.json", "3032d716f2a33654c3f1b98f831ce248ac2ef820584f401b8c07ae380e5616ed"],
  ["qualification_contract", ".github/ci/contracts/h2-5c-qualification.schema.json", "b6eede5fa4cd8eb98c182cce68f896bb8e32f8bdaa441fc785067be3cdac6fc2"],
  ["owner_controls_contract", ".github/ci/contracts/h2-5c-owner-controls.schema.json", "49f933b3858c46e62d919a997430cb92d586eaafe27e282c3f7dfada5cae2828"],
]);

const NEW_RUNTIME_INPUTS = Object.freeze([
  "crates/checker/src/merge.rs",
  "crates/checker/tests/unit/merge/tests.rs",
  "crates/emitter/src/builtins/helpers.rs",
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
      qualification.phase === "H2.5d-es2019-target" &&
      qualification.status === "qualified-typescript-oracle" &&
      qualification.selection_contract.global_h2_5d_rows === 45 &&
      qualification.selection_contract.global_candidate_denominator === 24 &&
      qualification.selection_contract.candidate_denominator === 24 &&
      qualification.selection_contract.future_deferred_rows === 21 &&
      qualification.summary.candidates === 24 &&
      qualification.summary.admitted_cases === 23 &&
      qualification.summary.deferred_cases === 1 &&
      qualification.summary.source_deferred_cases === 1 &&
      qualification.summary.admitted_typescript_writes === 57 &&
      qualification.summary.admitted_typescript_diagnostics === 47 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.5d qualification is not closed",
  );
  requireCondition(
    ownerControls.schema === 1 &&
      ownerControls.phase === "H2.5d-es2019-target-owner-controls" &&
      ownerControls.status === "qualified" &&
      ownerControls.summary.controls === 20 &&
      ownerControls.summary.exact_outputs === 19 &&
      ownerControls.summary.typescript_runs === 40 &&
      ownerControls.summary.reported_diagnostics === 2 &&
      ownerControls.summary.emit_diagnostics === 1 &&
      ownerControls.summary.no_emit_on_error_controls === 1 &&
      ownerControls.summary.es2018_controls === 19 &&
      ownerControls.summary.es2019_controls === 1 &&
      ownerControls.summary.optional_catch_binding_controls === 17 &&
      ownerControls.summary.explicit_catch_binding_controls === 1 &&
      ownerControls.summary.generated_name_composition_controls === 4 &&
      ownerControls.summary.nested_scope_controls === 4 &&
      ownerControls.summary.comment_controls === 1 &&
      ownerControls.summary.using_controls === 3 &&
      ownerControls.summary.standard_decorator_controls === 1 &&
      ownerControls.summary.class_composition_controls === 3 &&
      ownerControls.summary.commonjs_controls === 1 &&
      ownerControls.summary.h2_5a_active_controls === 19 &&
      ownerControls.summary.h2_5b_active_controls === 19 &&
      ownerControls.summary.h2_5c_active_controls === 19 &&
      ownerControls.summary.h2_5d_active_controls === 18,
    "H2.5d owner controls are not closed",
  );
  requireCondition(
    parentProfile.schema === 1 &&
      parentProfile.phase === "H2.5c" &&
      parentProfile.admitted_profile.exact_cases === 609 &&
      parentProfile.summary.completed_runtime_slices === 18,
    "H2.5c parent profile is not closed",
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
    new Set(runtimeInputPaths).size === 82,
    "H2.5d runtime input identity changed",
  );

  return withFingerprint(
    {
      schema: 1,
      kind: "h2-runtime-profile",
      status: "qualified",
      phase: "H2.5d",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_5c_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.5c artifacts remain immutable lineage; current runtime ownership transfers to this H2.5d profile",
      },
      qualification: pathHash(QUALIFICATION_RELATIVE_PATH),
      runtime_inputs: runtimeInputPaths.map(pathHash),
      admitted_profile: {
        execution: "single-project-one-shot-whole-program",
        target_states: [
          "ES2018(5)", "ES2019(6)", "ES2020(7)", "ES2021(8)", "ES2022(9)", "ES2023(10)",
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
        exact_cases: 632,
        h2_5d_exact_cases: 23,
        exact_reported_diagnostics: 1287,
        exact_writes: 920,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 14,
        candidate_denominator: 639,
        h2_5d_candidate_denominator: 24,
        h2_5d_global_future_rows: 21,
        h2_5d_owner_controls: 20,
        h2_5d_owner_writes: 19,
      },
      transition: {
        completed_slice: "H2.5d",
        next_slice: "H2.5e",
        active_runtime_slices: [
          "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a",
          "H2.2b", "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c",
          "H2.3d", "H2.4a", "H2.4b", "H2.5a", "H2.5b", "H2.5c", "H2.5d",
        ],
        inactive_runtime_slice_count: 18,
        classic_jsx_tsx_owner: "complete",
        automatic_jsx_runtime_owner: "complete",
        json_output_owner: "complete",
        legacy_decorators_owner: "complete",
        standard_decorators_and_class_fields_owner: "complete",
        target_esnext_transform_owner: "complete",
        target_es2021_transform_owner: "complete",
        target_es2020_transform_owner: "complete",
        target_es2019_transform_owner: "complete",
        target_es2018_transform_owner: "H2.5e",
        general_output_matrix_owner: "H2.8a",
        h2_5d_candidate_cases: 24,
        h2_5d_admitted_cases: 23,
        h2_5d_global_future_rows: 21,
        h2_5d_source_deferred_cases: 1,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_5d_cases_and_owner_controls_run_twice_in_isolated_programs",
        denominator_control: "h2_5d_exact_denominator_is_24_with_1_h2_9_source_deferred_case_and_21_global_future_rows",
        target_band_control: "es2018_lowers_optional_catch_bindings_while_es2019_preserves_them",
        optional_catch_binding_control: "missing_bindings_are_synthesized_without_rewriting_explicit_bindings",
        generated_binding_control: "source_function_nested_and_composed_temporaries_have_typed_scope_ownership",
        comment_boundary_control: "catch_keyword_synthetic_parenthesis_and_block_token_comments_match_tsc",
        composition_control: "using_decorators_named_evaluation_class_fields_and_super_paths_compose_in_transform_order",
        module_interaction_control: "es_module_and_commonjs_outputs_retain_exact_optional_catch_lowering",
        diagnostic_control: "umd_global_conflicts_and_derived_constructor_super_diagnostics_match_tsc",
        printer_control: "canonical_try_catch_layout_helper_priority_and_token_comments_are_exact",
        failure_control: "one_h2_9_source_fails_before_first_sink_write_and_no_emit_on_error_writes_nothing",
        owner_controls: {
          artifact: pathHash(OWNER_CONTROLS_RELATIVE_PATH),
          generator: pathHash("crates/oracle/h2-5d-owner-controls.mjs"),
          contract: pathHash(".github/ci/contracts/h2-5d-owner-controls.schema.json"),
        },
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_5c_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 19,
        next_runtime_slices: 1,
        runtime_admissions: 632,
        executed_candidates: 639,
        h2_5d_executed_candidates: 24,
        h2_5d_global_future_rows: 21,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-5d-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.5d profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-5d-profile.mjs [--write|--check]");
}
