import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-4b-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-4b-profile.v1.json";
const CONTRACT_RELATIVE_PATH = ".github/ci/contracts/h2-4b-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-4b-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH = "ratchets/h2-4b-owner-controls.v1.json";
const PARENT_PROFILE_RELATIVE_PATH = "ratchets/h2-4a-profile.v1.json";
const TRUSTED_BASE = "650d1f4ef43a1ad6ef28b8adb55f1308403e5625";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-4a-profile.v1.json", "bbc2b10c9ed43cb19442d804d64aea8dd757ea70ca93e9f56e220989b6c22b57"],
  ["qualification", "ratchets/h2-4a-qualification.v1.json", "c9a9c7223e8d66928bee577ea7ae360c35f7f63bc0f35b6a05263248674d0d75"],
  ["owner_controls", "ratchets/h2-4a-owner-controls.v1.json", "03fa843083e4cc521f6876fd95c8852dd30682478ee17319bd9c61115f459f4a"],
  ["profile_generator", "crates/oracle/h2-4a-profile.mjs", "909aa7aec9dcb759f0a7b45d92e3b6223e127ec27f90b87b17af0854d109e397"],
  ["qualification_generator", "crates/oracle/h2-4a-qualification.mjs", "702c0a1a20a3f4f2955fb7dfff417339f7db095443b0b1c493915aa6fa3d8fa8"],
  ["owner_controls_generator", "crates/oracle/h2-4a-owner-controls.mjs", "5bc3e108cd33c4f0d4027162b42e667f9ea502cce0fcd598a6b7457494ac5645"],
  ["profile_contract", ".github/ci/contracts/h2-4a-profile.schema.json", "7e7dac61f1517d3e7f2269c381776afed88dcf7ab6f6fb7a373ff1c0214963bb"],
  ["qualification_contract", ".github/ci/contracts/h2-4a-qualification.schema.json", "a13f94baacbf3eb4cde79be778e34fdf1b0f4108d058e015d9e1f9e27af15cb3"],
  ["owner_controls_contract", ".github/ci/contracts/h2-4a-owner-controls.schema.json", "9e3a5e077e5220a11ae473bbef996419e2c3fcc93694276393f62d4f819e734d"],
]);

const NEW_RUNTIME_INPUTS = Object.freeze([
  "crates/checker/src/engine.rs",
  "crates/checker/src/resolve.rs",
  "crates/emitter/src/builtins/class_fields.rs",
  "crates/emitter/src/builtins/standard_decorators.rs",
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
      qualification.phase === "H2.4b-standard-decorators-class-fields" &&
      qualification.status === "qualified-typescript-oracle" &&
      qualification.selection_contract.global_h2_4b_rows === 104 &&
      qualification.selection_contract.global_candidate_denominator === 41 &&
      qualification.selection_contract.historical_promotion_candidates === 3 &&
      qualification.selection_contract.candidate_denominator === 44 &&
      qualification.selection_contract.future_deferred_rows === 63 &&
      qualification.summary.candidates === 44 &&
      qualification.summary.admitted_cases === 42 &&
      qualification.summary.deferred_cases === 2 &&
      qualification.summary.source_deferred_cases === 2 &&
      qualification.summary.admitted_typescript_writes === 56 &&
      qualification.summary.admitted_typescript_diagnostics === 150 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.4b qualification is not closed",
  );
  requireCondition(
    ownerControls.schema === 1 &&
      ownerControls.phase === "H2.4b-standard-decorator-class-fields-owner-controls" &&
      ownerControls.status === "qualified" &&
      ownerControls.summary.controls === 19 &&
      ownerControls.summary.exact_outputs === 18 &&
      ownerControls.summary.typescript_runs === 38 &&
      ownerControls.summary.reported_diagnostics === 3 &&
      ownerControls.summary.emit_diagnostics === 1 &&
      ownerControls.summary.no_emit_on_error_controls === 1 &&
      ownerControls.summary.define_fields_controls === 1 &&
      ownerControls.summary.assignment_fields_controls === 18,
    "H2.4b owner controls are not closed",
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
    new Set(runtimeInputPaths).size === 68,
    "H2.4b runtime input identity changed",
  );

  return withFingerprint(
    {
      schema: 1,
      kind: "h2-runtime-profile",
      status: "qualified",
      phase: "H2.4b",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_4a_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.4a artifacts remain immutable lineage; current runtime ownership transfers to this H2.4b profile",
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
        exact_cases: 360,
        h2_4b_exact_cases: 42,
        exact_reported_diagnostics: 838,
        exact_writes: 469,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 3,
        candidate_denominator: 356,
        h2_4b_candidate_denominator: 44,
        h2_4b_global_future_rows: 63,
        h2_4b_owner_controls: 19,
        h2_4b_owner_writes: 18,
      },
      transition: {
        completed_slice: "H2.4b",
        next_slice: "H2.5a",
        active_runtime_slices: [
          "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a",
          "H2.2b", "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c",
          "H2.3d", "H2.4a", "H2.4b",
        ],
        inactive_runtime_slice_count: 22,
        classic_jsx_tsx_owner: "complete",
        automatic_jsx_runtime_owner: "complete",
        json_output_owner: "complete",
        legacy_decorators_owner: "complete",
        standard_decorators_and_class_fields_owner: "complete",
        target_esnext_transform_owner: "H2.5a",
        general_output_matrix_owner: "H2.8a",
        h2_4b_candidate_cases: 44,
        h2_4b_global_future_rows: 63,
        h2_4b_source_deferred_cases: 2,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_4b_cases_and_owner_controls_run_twice_in_isolated_programs",
        denominator_control: "h2_4b_exact_denominator_is_44_including_three_h2_1a_preservation_promotions_with_63_future_deferred_rows",
        decorator_transform_control: "standard_class_member_private_computed_and_replacement_output_matches_typescript",
        class_fields_control: "assignment_and_define_modes_auto_accessors_static_blocks_and_constructor_order_are_exact",
        evaluation_order_control: "decorator_binding_computed_names_initializers_and_class_replacement_order_are_exact",
        module_interaction_control: "esnext_commonjs_and_system_standard_decorator_exports_are_exact",
        failure_control: "two_parser_owned_cases_are_h2_9_deferred_and_no_emit_on_error_writes_nothing",
        owner_controls: {
          artifact: pathHash(OWNER_CONTROLS_RELATIVE_PATH),
          generator: pathHash("crates/oracle/h2-4b-owner-controls.mjs"),
          contract: pathHash(".github/ci/contracts/h2-4b-owner-controls.schema.json"),
        },
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_4a_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 15,
        next_runtime_slices: 1,
        runtime_admissions: 360,
        executed_candidates: 356,
        h2_4b_executed_candidates: 44,
        h2_4b_global_future_rows: 63,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-4b-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.4b profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-4b-profile.mjs [--write|--check]");
}
