import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-5a-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-5a-profile.v1.json";
const CONTRACT_RELATIVE_PATH = ".github/ci/contracts/h2-5a-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-5a-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH = "ratchets/h2-5a-owner-controls.v1.json";
const PARENT_PROFILE_RELATIVE_PATH = "ratchets/h2-4b-profile.v1.json";
const TRUSTED_BASE = "59a9a7c5768a11cb2de082bd82e92c09a37f469f";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-4b-profile.v1.json", "eaef896cb41f514d78a05b02e58c55617a898bf823134b65561cd5f4f6468142"],
  ["qualification", "ratchets/h2-4b-qualification.v1.json", "b16371b70028f85107cc446dfe0338ba957cb70cac0e589f656557ebf3859060"],
  ["owner_controls", "ratchets/h2-4b-owner-controls.v1.json", "dac084453698e17ca40bb90ba6e18f001f49311377b70f57cbc175045b8c6d3a"],
  ["profile_generator", "crates/oracle/h2-4b-profile.mjs", "cc6d3b0f75900048f1daf2f2e0ccc8444104ce62943e2ce0a1dc9e4c26115d49"],
  ["qualification_generator", "crates/oracle/h2-4b-qualification.mjs", "3fffa19a9c3c5e623c05603029457c6f7f309b816d52074c500436cc9baa8543"],
  ["owner_controls_generator", "crates/oracle/h2-4b-owner-controls.mjs", "a79656f4a87ec85622bb87cc7e3042f17929b9ad373e30c6ea367302b4c0ca75"],
  ["profile_contract", ".github/ci/contracts/h2-4b-profile.schema.json", "08175ba26964370f9fd2c6231c50aa71b26248dbd27c42ad1cdd6a4028edf2b0"],
  ["qualification_contract", ".github/ci/contracts/h2-4b-qualification.schema.json", "82f5e0d5f7e7f60da376dc9f3b41e0b0e75435229673bc14ad2c3754863a2148"],
  ["owner_controls_contract", ".github/ci/contracts/h2-4b-owner-controls.schema.json", "68b29b4a7e6641ebceaf4d96683f8ad669e94ee535be3d7ede2155cc7f0245e0"],
]);

const NEW_RUNTIME_INPUTS = Object.freeze([
  "crates/compiler/src/cli.rs",
  "crates/compiler/tests/unit/cli/tests.rs",
  "crates/emitter/src/builtins/class_fields/downlevel.rs",
  "crates/emitter/src/builtins/es_next.rs",
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
      qualification.phase === "H2.5a-esnext-target" &&
      qualification.status === "qualified-typescript-oracle" &&
      qualification.selection_contract.global_h2_5a_rows === 634 &&
      qualification.selection_contract.global_candidate_denominator === 172 &&
      qualification.selection_contract.candidate_denominator === 172 &&
      qualification.selection_contract.future_deferred_rows === 462 &&
      qualification.summary.candidates === 172 &&
      qualification.summary.admitted_cases === 167 &&
      qualification.summary.deferred_cases === 5 &&
      qualification.summary.source_deferred_cases === 5 &&
      qualification.summary.admitted_typescript_writes === 287 &&
      qualification.summary.admitted_typescript_diagnostics === 335 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.5a qualification is not closed",
  );
  requireCondition(
    ownerControls.schema === 1 &&
      ownerControls.phase === "H2.5a-esnext-target-owner-controls" &&
      ownerControls.status === "qualified" &&
      ownerControls.summary.controls === 20 &&
      ownerControls.summary.exact_outputs === 19 &&
      ownerControls.summary.typescript_runs === 40 &&
      ownerControls.summary.reported_diagnostics === 1 &&
      ownerControls.summary.emit_diagnostics === 1 &&
      ownerControls.summary.no_emit_on_error_controls === 1 &&
      ownerControls.summary.no_emit_helpers_controls === 1 &&
      ownerControls.summary.es2021_controls === 4 &&
      ownerControls.summary.es2022_controls === 12 &&
      ownerControls.summary.later_standard_controls === 3 &&
      ownerControls.summary.esnext_controls === 1 &&
      ownerControls.summary.using_controls === 13 &&
      ownerControls.summary.await_using_controls === 2 &&
      ownerControls.summary.standard_decorator_controls === 2 &&
      ownerControls.summary.h2_5a_active_controls === 18,
    "H2.5a owner controls are not closed",
  );
  requireCondition(
    parentProfile.schema === 1 &&
      parentProfile.phase === "H2.4b" &&
      parentProfile.admitted_profile.exact_cases === 360 &&
      parentProfile.summary.completed_runtime_slices === 15,
    "H2.4b parent profile is not closed",
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
    new Set(runtimeInputPaths).size === 72,
    "H2.5a runtime input identity changed",
  );

  return withFingerprint(
    {
      schema: 1,
      kind: "h2-runtime-profile",
      status: "qualified",
      phase: "H2.5a",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_4b_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.4b artifacts remain immutable lineage; current runtime ownership transfers to this H2.5a profile",
      },
      qualification: pathHash(QUALIFICATION_RELATIVE_PATH),
      runtime_inputs: runtimeInputPaths.map(pathHash),
      admitted_profile: {
        execution: "single-project-one-shot-whole-program",
        target_states: [
          "ES2021(8)", "ES2022(9)", "ES2023(10)",
          "ES2024(11)", "ES2025(12)", "ESNext(99)",
        ],
        module_states: [
          "absent-effective-ESNext", "ESNext(99)", "CommonJS(1)", "AMD(2)",
          "UMD(3)", "System(4)", "Node16(100)", "Node18(101)",
          "Node20(102)", "NodeNext(199)",
        ],
        jsx_modes: [
          "Preserve(1)", "React(2)", "ReactNative(3)",
          "ReactJSX(4)", "ReactJSXDev(5)",
        ],
        source_kinds: [
          ".ts", ".mts", ".cts", ".tsx", ".js", ".mjs", ".cjs", ".jsx", ".json",
        ],
        products: ["javascript", "mjs", "cjs", "jsx", "json"],
        exact_cases: 527,
        h2_5a_exact_cases: 167,
        exact_reported_diagnostics: 1173,
        exact_writes: 756,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 8,
        candidate_denominator: 528,
        h2_5a_candidate_denominator: 172,
        h2_5a_global_future_rows: 462,
        h2_5a_owner_controls: 20,
        h2_5a_owner_writes: 19,
      },
      transition: {
        completed_slice: "H2.5a",
        next_slice: "H2.5b",
        active_runtime_slices: [
          "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a",
          "H2.2b", "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c",
          "H2.3d", "H2.4a", "H2.4b", "H2.5a",
        ],
        inactive_runtime_slice_count: 21,
        classic_jsx_tsx_owner: "complete",
        automatic_jsx_runtime_owner: "complete",
        json_output_owner: "complete",
        legacy_decorators_owner: "complete",
        standard_decorators_and_class_fields_owner: "complete",
        target_esnext_transform_owner: "complete",
        target_es2021_transform_owner: "H2.5b",
        general_output_matrix_owner: "H2.8a",
        h2_5a_candidate_cases: 172,
        h2_5a_admitted_cases: 167,
        h2_5a_global_future_rows: 462,
        h2_5a_source_deferred_cases: 5,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_5a_cases_and_owner_controls_run_twice_in_isolated_programs",
        denominator_control: "h2_5a_exact_denominator_is_172_with_462_future_deferred_rows",
        target_band_control: "es2021_through_es2025_lower_esnext_syntax_while_esnext_preserves_it",
        disposal_scope_control: "sync_async_source_block_function_loop_and_namespace_disposal_scopes_are_exact",
        generated_name_control: "generated_env_error_and_result_names_follow_output_scope_ownership_and_collision_rules",
        class_fields_control: "es2021_es2022_field_auto_accessor_static_and_parameter_property_boundaries_are_exact",
        decorator_binding_control: "computed_receiver_parenthesis_and_lexical_super_bindings_are_exact",
        module_interaction_control: "esnext_and_commonjs_disposal_placement_helper_policy_and_exports_are_exact",
        failure_control: "five_later_owned_sources_fail_before_first_sink_write_and_no_emit_on_error_writes_nothing",
        owner_controls: {
          artifact: pathHash(OWNER_CONTROLS_RELATIVE_PATH),
          generator: pathHash("crates/oracle/h2-5a-owner-controls.mjs"),
          contract: pathHash(".github/ci/contracts/h2-5a-owner-controls.schema.json"),
        },
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_4b_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 16,
        next_runtime_slices: 1,
        runtime_admissions: 527,
        executed_candidates: 528,
        h2_5a_executed_candidates: 172,
        h2_5a_global_future_rows: 462,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-5a-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.5a profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-5a-profile.mjs [--write|--check]");
}
