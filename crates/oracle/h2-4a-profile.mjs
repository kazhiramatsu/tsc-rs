import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-4a-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-4a-profile.v1.json";
const CONTRACT_RELATIVE_PATH = ".github/ci/contracts/h2-4a-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-4a-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH = "ratchets/h2-4a-owner-controls.v1.json";
const PARENT_PROFILE_RELATIVE_PATH = "ratchets/h2-3d-profile.v1.json";
const TRUSTED_BASE = "1e15c7bede0444499ed8b70a873c23d0e3a50c56";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-3d-profile.v1.json", "1177ab53f8389d9e75564f12ae27839f4c1c60f7ae139214b992a3aaa77e7a1d"],
  ["qualification", "ratchets/h2-3d-qualification.v1.json", "25b5a7add753c1daf0baebfc64f4ebf6779e6eb7434e4a2e83401a5a00550fad"],
  ["owner_controls", "ratchets/h2-3d-owner-controls.v1.json", "86605688403f487c164f19b42c73fe58b9e0da218aa3fe1393c74ef2df0380ab"],
  ["profile_generator", "crates/oracle/h2-3d-profile.mjs", "b04fa7d599ff272b8e8fdae77616f500cc90b8ee1995ca9d7853aae8b963dd91"],
  ["qualification_generator", "crates/oracle/h2-3d-qualification.mjs", "0ac5be295678d7f361d5ccbaa62992c4ad777480f9d8d9df51c139a4864ff398"],
  ["owner_controls_generator", "crates/oracle/h2-3d-owner-controls.mjs", "a0f873c6cc7827f83b16eaf3cb74b19e58c4900f57753cce6995e1eb53e59e5c"],
  ["profile_contract", ".github/ci/contracts/h2-3d-profile.schema.json", "72d6fd3ff0527613663d9245af6eb628b741b495c7e6a751bfed6511f96d649f"],
  ["qualification_contract", ".github/ci/contracts/h2-3d-qualification.schema.json", "c91f8d3333029375598b0a11063eba73ea7951112612624129b27cfebd9f5c27"],
  ["owner_controls_contract", ".github/ci/contracts/h2-3d-owner-controls.schema.json", "fbbd9cd3c906303315a828daad000c810818435267520b63847665eb28569a60"],
]);

const NEW_RUNTIME_INPUTS = Object.freeze([
  "crates/emitter/src/builtins/legacy_decorators.rs",
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
      qualification.phase === "H2.4a-legacy-decorators" &&
      qualification.status === "qualified-typescript-oracle" &&
      qualification.selection_contract.global_h2_4a_rows === 418 &&
      qualification.selection_contract.candidate_denominator === 10 &&
      qualification.selection_contract.future_deferred_rows === 408 &&
      qualification.summary.candidates === 10 &&
      qualification.summary.admitted_cases === 9 &&
      qualification.summary.deferred_cases === 1 &&
      qualification.summary.source_deferred_cases === 1 &&
      qualification.summary.admitted_typescript_writes === 9 &&
      qualification.summary.admitted_typescript_diagnostics === 8 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.4a qualification is not closed",
  );
  requireCondition(
    ownerControls.schema === 1 &&
      ownerControls.phase === "H2.4a-legacy-decorator-owner-controls" &&
      ownerControls.status === "qualified" &&
      ownerControls.summary.controls === 19 &&
      ownerControls.summary.exact_outputs === 18 &&
      ownerControls.summary.typescript_runs === 38 &&
      ownerControls.summary.reported_diagnostics === 2 &&
      ownerControls.summary.emit_diagnostics === 1 &&
      ownerControls.summary.no_emit_on_error_controls === 1,
    "H2.4a owner controls are not closed",
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
    new Set(runtimeInputPaths).size === 64,
    "H2.4a runtime input identity changed",
  );

  return withFingerprint(
    {
      schema: 1,
      kind: "h2-runtime-profile",
      status: "qualified",
      phase: "H2.4a",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_3d_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.3d artifacts remain immutable lineage; current runtime ownership transfers to this H2.4a profile",
      },
      qualification: pathHash(QUALIFICATION_RELATIVE_PATH),
      runtime_inputs: runtimeInputPaths.map(pathHash),
      admitted_profile: {
        execution: "single-project-one-shot-whole-program",
        target: "ESNext(99)",
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
        exact_cases: 318,
        h2_4a_exact_cases: 9,
        exact_reported_diagnostics: 688,
        exact_writes: 413,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 1,
        candidate_denominator: 312,
        h2_4a_candidate_denominator: 10,
        h2_4a_global_future_rows: 408,
        h2_4a_owner_controls: 19,
        h2_4a_owner_writes: 18,
      },
      transition: {
        completed_slice: "H2.4a",
        next_slice: "H2.4b",
        active_runtime_slices: [
          "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a",
          "H2.2b", "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c",
          "H2.3d", "H2.4a",
        ],
        inactive_runtime_slice_count: 23,
        classic_jsx_tsx_owner: "complete",
        automatic_jsx_runtime_owner: "complete",
        json_output_owner: "complete",
        legacy_decorators_owner: "complete",
        standard_decorators_and_class_fields_owner: "H2.4b",
        general_output_matrix_owner: "H2.8a",
        h2_4a_candidate_cases: 10,
        h2_4a_global_future_rows: 408,
        h2_4a_source_deferred_cases: 1,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_4a_cases_and_owner_controls_run_twice_in_isolated_programs",
        denominator_control: "h2_4a_exact_denominator_is_10_after_h2_3d_closure_with_408_future_deferred_rows",
        decorator_transform_control: "legacy_class_member_parameter_metadata_and_helper_output_matches_typescript",
        resolver_fact_control: "constructor_reference_check_flags_and_referenced_value_declarations_drive_exact_output",
        evaluation_order_control: "computed_names_private_expressions_and_member_before_class_order_are_exact",
        module_interaction_control: "named_default_commonjs_and_system_decorated_exports_are_exact",
        failure_control: "one_parser_owned_case_is_h2_9_deferred_and_no_emit_on_error_writes_nothing",
        owner_controls: {
          artifact: pathHash(OWNER_CONTROLS_RELATIVE_PATH),
          generator: pathHash("crates/oracle/h2-4a-owner-controls.mjs"),
          contract: pathHash(".github/ci/contracts/h2-4a-owner-controls.schema.json"),
        },
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_3d_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 14,
        next_runtime_slices: 1,
        runtime_admissions: 318,
        executed_candidates: 312,
        h2_4a_executed_candidates: 10,
        h2_4a_global_future_rows: 408,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-4a-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.4a profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-4a-profile.mjs [--write|--check]");
}
