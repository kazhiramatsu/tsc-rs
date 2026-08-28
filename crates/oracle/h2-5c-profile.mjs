import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-5c-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-5c-profile.v1.json";
const CONTRACT_RELATIVE_PATH = ".github/ci/contracts/h2-5c-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-5c-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH = "ratchets/h2-5c-owner-controls.v1.json";
const PARENT_PROFILE_RELATIVE_PATH = "ratchets/h2-5b-profile.v1.json";
const TRUSTED_BASE = "564fcafebdc855ec09104c07f1f6622b9ffab60e";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-5b-profile.v1.json", "001f6a844ab7f5222d601a54c2efdb6efe4e0077ea29b144c780e502b98299ee"],
  ["qualification", "ratchets/h2-5b-qualification.v1.json", "c091e9b16ad5065fdf62e19bdce26cc0359afbcc74af72e10d782ead51d3c9ab"],
  ["owner_controls", "ratchets/h2-5b-owner-controls.v1.json", "082bd8c88bea185616b77e9f687caeb62b36225cdb2c356b361fa1399749b3e1"],
  ["profile_generator", "crates/oracle/h2-5b-profile.mjs", "1ac6330c096c04358ed68a814111628e77de36c0abc0086ede3e36ae9f055481"],
  ["qualification_generator", "crates/oracle/h2-5b-qualification.mjs", "483b624fc39710a2b506b726fe78bb7dee3e801d0d176510a6c3e5d636f764dd"],
  ["owner_controls_generator", "crates/oracle/h2-5b-owner-controls.mjs", "219bf083f0205eb8f947317c923c5e6abc00099c8dc25ae63bdf5110b156d724"],
  ["profile_contract", ".github/ci/contracts/h2-5b-profile.schema.json", "a1dbc72f9df5f185dab12ffd2926b7bd5b25563cf9b668e5be468baa239768c7"],
  ["qualification_contract", ".github/ci/contracts/h2-5b-qualification.schema.json", "0bcaac2a7b5aa384e8bd45dd6c2f3205384cb3110361c08a4eecad8ca54067f1"],
  ["owner_controls_contract", ".github/ci/contracts/h2-5b-owner-controls.schema.json", "a87789ed2cea90c932736b7c71b7192e7271ea7f4fad2b3a3008b1c4ce619188"],
]);

const NEW_RUNTIME_INPUTS = Object.freeze([]);

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
      qualification.phase === "H2.5c-es2020-target" &&
      qualification.status === "qualified-typescript-oracle" &&
      qualification.selection_contract.global_h2_5c_rows === 16 &&
      qualification.selection_contract.global_candidate_denominator === 15 &&
      qualification.selection_contract.candidate_denominator === 15 &&
      qualification.selection_contract.future_deferred_rows === 1 &&
      qualification.summary.candidates === 15 &&
      qualification.summary.admitted_cases === 14 &&
      qualification.summary.deferred_cases === 1 &&
      qualification.summary.source_deferred_cases === 1 &&
      qualification.summary.admitted_typescript_writes === 14 &&
      qualification.summary.admitted_typescript_diagnostics === 19 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.5c qualification is not closed",
  );
  requireCondition(
    ownerControls.schema === 1 &&
      ownerControls.phase === "H2.5c-es2020-target-owner-controls" &&
      ownerControls.status === "qualified" &&
      ownerControls.summary.controls === 26 &&
      ownerControls.summary.exact_outputs === 25 &&
      ownerControls.summary.typescript_runs === 52 &&
      ownerControls.summary.reported_diagnostics === 1 &&
      ownerControls.summary.emit_diagnostics === 1 &&
      ownerControls.summary.no_emit_on_error_controls === 1 &&
      ownerControls.summary.es2019_controls === 25 &&
      ownerControls.summary.es2020_controls === 1 &&
      ownerControls.summary.optional_chain_controls === 24 &&
      ownerControls.summary.nullish_coalescing_controls === 13 &&
      ownerControls.summary.parameter_hoist_controls === 3 &&
      ownerControls.summary.standard_decorator_controls === 1 &&
      ownerControls.summary.legacy_decorator_controls === 1 &&
      ownerControls.summary.commonjs_controls === 1 &&
      ownerControls.summary.h2_5a_active_controls === 25 &&
      ownerControls.summary.h2_5b_active_controls === 25 &&
      ownerControls.summary.h2_5c_active_controls === 24,
    "H2.5c owner controls are not closed",
  );
  requireCondition(
    parentProfile.schema === 1 &&
      parentProfile.phase === "H2.5b" &&
      parentProfile.admitted_profile.exact_cases === 595 &&
      parentProfile.summary.completed_runtime_slices === 17,
    "H2.5b parent profile is not closed",
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
    "H2.5c runtime input identity changed",
  );

  return withFingerprint(
    {
      schema: 1,
      kind: "h2-runtime-profile",
      status: "qualified",
      phase: "H2.5c",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_5b_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.5b artifacts remain immutable lineage; current runtime ownership transfers to this H2.5c profile",
      },
      qualification: pathHash(QUALIFICATION_RELATIVE_PATH),
      runtime_inputs: runtimeInputPaths.map(pathHash),
      admitted_profile: {
        execution: "single-project-one-shot-whole-program",
        target_states: [
          "ES2019(6)", "ES2020(7)", "ES2021(8)", "ES2022(9)", "ES2023(10)",
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
        exact_cases: 609,
        h2_5c_exact_cases: 14,
        exact_reported_diagnostics: 1240,
        exact_writes: 863,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 13,
        candidate_denominator: 615,
        h2_5c_candidate_denominator: 15,
        h2_5c_global_future_rows: 1,
        h2_5c_owner_controls: 26,
        h2_5c_owner_writes: 25,
      },
      transition: {
        completed_slice: "H2.5c",
        next_slice: "H2.5d",
        active_runtime_slices: [
          "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a",
          "H2.2b", "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c",
          "H2.3d", "H2.4a", "H2.4b", "H2.5a", "H2.5b", "H2.5c",
        ],
        inactive_runtime_slice_count: 19,
        classic_jsx_tsx_owner: "complete",
        automatic_jsx_runtime_owner: "complete",
        json_output_owner: "complete",
        legacy_decorators_owner: "complete",
        standard_decorators_and_class_fields_owner: "complete",
        target_esnext_transform_owner: "complete",
        target_es2021_transform_owner: "complete",
        target_es2020_transform_owner: "complete",
        target_es2019_transform_owner: "H2.5d",
        general_output_matrix_owner: "H2.8a",
        h2_5c_candidate_cases: 15,
        h2_5c_admitted_cases: 14,
        h2_5c_global_future_rows: 1,
        h2_5c_source_deferred_cases: 1,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_5c_cases_and_owner_controls_run_twice_in_isolated_programs",
        denominator_control: "h2_5c_exact_denominator_is_15_with_1_future_deferred_row",
        target_band_control: "es2019_lowers_es2020_syntax_while_es2020_preserves_it",
        optional_chain_control: "property_element_call_delete_receiver_and_erased_outer_expression_chains_are_exact",
        nullish_coalescing_control: "simple_and_effectful_left_operands_are_evaluated_once_in_tsc_order",
        evaluation_order_control: "effectful_receivers_keys_and_rhs_values_are_evaluated_once_in_tsc_order",
        generated_binding_control: "source_function_parameter_and_nested_default_temporaries_have_typed_scope_ownership",
        class_fields_control: "public_private_static_instance_and_decorator_field_composition_is_exact",
        module_interaction_control: "es_module_commonjs_node_and_preserve_outputs_including_import_defer_are_exact",
        printer_control: "canonical_compiler_print_numeric_spelling_deferred_import_phase_and_comments_are_exact",
        failure_control: "one_h2_9_source_fails_before_first_sink_write_and_no_emit_on_error_writes_nothing",
        owner_controls: {
          artifact: pathHash(OWNER_CONTROLS_RELATIVE_PATH),
          generator: pathHash("crates/oracle/h2-5c-owner-controls.mjs"),
          contract: pathHash(".github/ci/contracts/h2-5c-owner-controls.schema.json"),
        },
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_5b_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 18,
        next_runtime_slices: 1,
        runtime_admissions: 609,
        executed_candidates: 615,
        h2_5c_executed_candidates: 15,
        h2_5c_global_future_rows: 1,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-5c-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.5c profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-5c-profile.mjs [--write|--check]");
}
