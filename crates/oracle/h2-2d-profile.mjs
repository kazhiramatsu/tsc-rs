import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-2d-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-2d-profile.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-2d-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-2d-qualification.v1.json";
const TRUSTED_BASE = "8c61918365f186645d18724f50f125782754094c";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-2c-profile.v1.json", "ee5924d57669e9d65670247cd04172eb58cacdc8804438c298b3f25a926c8e3a"],
  ["qualification", "ratchets/h2-2c-qualification.v1.json", "807f2295c7f1b2f6dbba20fdfae89e7b7cc357eb9b06cd8e5f31366e4dd9dd46"],
  ["profile_generator", "crates/oracle/h2-2c-profile.mjs", "75d1ed767d6834703d1ca9972c2cbf0802f2a767d55619eb4fe1f9e896d64da3"],
  ["qualification_generator", "crates/oracle/h2-2c-qualification.mjs", "c49d516df972867d2d6f1303087640f075bbfcab1043f785cf34d93f59bff0c5"],
  ["profile_contract", ".github/ci/contracts/h2-2c-profile.schema.json", "e66208499e81d6004ff43232138a2989a72d55cc6284a5d2a795b475523dd333"],
  ["qualification_contract", ".github/ci/contracts/h2-2c-qualification.schema.json", "dcf9bf8f52afc62497aadd0b07041f2ed9f9d448a2cf1510546411fd3577aa65"],
]);

const RUNTIME_INPUTS = Object.freeze([
  "crates/checker/src/emit.rs",
  "crates/checker/src/evaluate.rs",
  "crates/checker/src/modules.rs",
  "crates/compiler/src/lib.rs",
  "crates/compiler/tests/integration/emit_session_contract.rs",
  "crates/compiler/tests/integration/program_session_contract.rs",
  "crates/compiler/tests/integration/upstream_no_emit_harness_contract.rs",
  "crates/diagnostics/src/gen.rs",
  "crates/emitter/src/activity.rs",
  "crates/emitter/src/builtins.rs",
  "crates/emitter/src/builtins/system.rs",
  "crates/emitter/src/execute.rs",
  "crates/emitter/src/factory.rs",
  "crates/emitter/src/host.rs",
  "crates/emitter/src/lib.rs",
  "crates/emitter/src/metadata.rs",
  "crates/emitter/src/printer.rs",
  "crates/emitter/src/resolver.rs",
  "crates/emitter/src/transform.rs",
  "crates/harness/src/upstream_suites/execution.rs",
  "crates/harness/src/upstream_suites/execution/project.rs",
  "crates/program/src/prepared.rs",
  "crates/program/src/module_requests.rs",
  "crates/program/tests/integration/module_request_contract.rs",
  "crates/syntax/src/incremental.rs",
  "crates/syntax/src/lib.rs",
  "crates/syntax/src/parser.rs",
  "crates/syntax/tests/unit/incremental/tests.rs",
  "crates/syntax/tests/unit/parser/tests.rs",
  "crates/xtask/src/h2_1a_acceptance.rs",
  "crates/xtask/src/h2_1b_acceptance.rs",
  "crates/xtask/src/h2_1c_acceptance.rs",
  "crates/xtask/src/h2_1d_acceptance.rs",
  "crates/xtask/src/h2_1e_acceptance.rs",
  "crates/xtask/src/h2_2a_acceptance.rs",
  "crates/xtask/src/h2_2b_acceptance.rs",
  "crates/xtask/src/h2_2c_acceptance.rs",
  "crates/xtask/src/h2_2d_acceptance.rs",
  "crates/xtask/src/main.rs",
  "crates/xtask/tests/unit/h2_1d_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_1e_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2a_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2b_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2c_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2d_acceptance/tests.rs",
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
  requireCondition(
    qualification.schema === 1 &&
      qualification.phase === "H2.2d-import-export-equals" &&
      qualification.summary.candidates === 9 &&
      qualification.summary.admitted_cases === 9 &&
      qualification.summary.diagnostic_deferred_output_control_cases === 0 &&
      qualification.summary.source_deferred_cases === 0 &&
      qualification.summary.admitted_typescript_writes === 13 &&
      qualification.summary.admitted_typescript_diagnostics === 36 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.2d qualification is not closed",
  );

  const historical = Object.fromEntries(
    HISTORICAL_AUTHORITIES.map(([key, relativePath, expected]) => {
      const record = pathHash(relativePath);
      requireCondition(record.sha256 === expected, `${relativePath} historical bytes changed`);
      return [key, record];
    }),
  );

  return withFingerprint(
    {
      schema: 1,
      kind: "h2-runtime-profile",
      status: "qualified",
      phase: "H2.2d",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_2c_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.2c artifacts remain immutable lineage; current runtime ownership transfers to this H2.2d profile",
      },
      qualification: pathHash(QUALIFICATION_RELATIVE_PATH),
      runtime_inputs: RUNTIME_INPUTS.map(pathHash),
      admitted_profile: {
        execution: "single-project-one-shot-whole-program",
        target: "ESNext(99)",
        module_states: [
          "absent-effective-ESNext", "ESNext(99)", "CommonJS(1)", "AMD(2)",
          "UMD(3)", "System(4)", "Node16(100)", "Node18(101)",
          "Node20(102)", "NodeNext(199)",
        ],
        source_kinds: [".ts", ".mts", ".cts"],
        products: ["javascript"],
        exact_cases: 302,
        h2_2d_exact_cases: 9,
        exact_reported_diagnostics: 633,
        exact_writes: 397,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 0,
        candidate_denominator: 295,
        h2_2d_candidate_denominator: 9,
      },
      transition: {
        completed_slice: "H2.2d",
        next_slice: "H2.3a",
        active_runtime_slices: [
          "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a", "H2.2b", "H2.2c", "H2.2d",
        ],
        inactive_runtime_slice_count: 28,
        runtime_enum_owner: "complete",
        namespace_owner: "complete",
        parameter_property_owner: "complete",
        import_equals_owner: "complete",
        export_equals_owner: "complete",
        h2_2d_deferred_slices: [],
        h2_2d_deferred_cases: 0,
        inherited_h2_2c_residual_slices: [],
        inherited_h2_2c_residual_cases: 0,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_2d_two_worker_execution_is_exact_and_isolated",
        import_export_equals_control:
          "h2_2d_import_export_equals_outputs_are_exact",
        integration_shape_control:
          "h2_2d_module_format_interactions_match_typescript_shapes",
        historical_residual_control:
          "h2_2d_historical_source_deferred_rows_are_exactly_promoted",
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_2c_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 9,
        next_runtime_slices: 1,
        runtime_admissions: 302,
        executed_candidates: 295,
        h2_2d_executed_candidates: 9,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-2d-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.2d profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-2d-profile.mjs [--write|--check]");
}
