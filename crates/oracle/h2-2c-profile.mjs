import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-2c-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-2c-profile.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-2c-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-2c-qualification.v1.json";
const TRUSTED_BASE = "cb450995d1d0e72f34b774dc3feb9a32384e6068";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-2b-profile.v1.json", "bf259e2c81ed69efccb920ea50143c5721ace5312b9ea49e16c9dfc3381ad6f3"],
  ["qualification", "ratchets/h2-2b-qualification.v1.json", "3e94c145ed48275b772bf2bd5a0824c8b35e49417757112fd75f6a4b6632889c"],
  ["profile_generator", "crates/oracle/h2-2b-profile.mjs", "4a94c68e4033a8f08b7d368431bc1cd552400ba760363708ae7b1d6b734f505e"],
  ["qualification_generator", "crates/oracle/h2-2b-qualification.mjs", "b121bafc571ec98825b7e213a5aa381c0f2fd7ef6d8fbffe2797997292796ae6"],
  ["profile_contract", ".github/ci/contracts/h2-2b-profile.schema.json", "44dbbd10e0ce2898f669b0929cac06a6728e59dd6bdc66134507a98f6e14aabc"],
  ["qualification_contract", ".github/ci/contracts/h2-2b-qualification.schema.json", "ad3e5a5d2c555530ecc0b5ced52be3b35916f4fd807b1252e65a69059503bd0b"],
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
  "crates/xtask/src/h2_1d_acceptance.rs",
  "crates/xtask/src/h2_1e_acceptance.rs",
  "crates/xtask/src/h2_2a_acceptance.rs",
  "crates/xtask/src/h2_2b_acceptance.rs",
  "crates/xtask/src/h2_2c_acceptance.rs",
  "crates/xtask/src/main.rs",
  "crates/xtask/tests/unit/h2_1d_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_1e_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2a_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2b_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2c_acceptance/tests.rs",
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
      qualification.phase === "H2.2c-parameter-properties" &&
      qualification.summary.candidates === 6 &&
      qualification.summary.admitted_cases === 6 &&
      qualification.summary.diagnostic_deferred_output_control_cases === 0 &&
      qualification.summary.source_deferred_cases === 0 &&
      qualification.summary.admitted_typescript_writes === 6 &&
      qualification.summary.admitted_typescript_diagnostics === 12 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.2c qualification is not closed",
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
      phase: "H2.2c",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_2b_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.2b artifacts remain immutable lineage; current runtime ownership transfers to this H2.2c profile",
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
        exact_cases: 293,
        h2_2c_exact_cases: 6,
        exact_reported_diagnostics: 597,
        exact_writes: 384,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 0,
        candidate_denominator: 295,
        h2_2c_candidate_denominator: 6,
      },
      transition: {
        completed_slice: "H2.2c",
        next_slice: "H2.2d",
        active_runtime_slices: [
          "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a", "H2.2b", "H2.2c",
        ],
        inactive_runtime_slice_count: 29,
        runtime_enum_owner: "complete",
        namespace_owner: "complete",
        parameter_property_owner: "complete",
        import_equals_owner: "H2.2d",
        h2_2c_deferred_slices: [],
        h2_2c_deferred_cases: 0,
        inherited_h2_2b_residual_slices: ["H2.2d"],
        inherited_h2_2b_residual_cases: 3,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_2c_two_worker_execution_is_exact_and_isolated",
        parameter_property_control:
          "h2_2c_parameter_property_outputs_are_exact",
        integration_shape_control:
          "h2_2c_parameter_property_emit_matches_typescript_shapes",
        historical_residual_control:
          "h2_2b_later_owner_controls_remain_source_deferred",
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_2b_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 8,
        next_runtime_slices: 1,
        runtime_admissions: 293,
        executed_candidates: 295,
        h2_2c_executed_candidates: 6,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-2c-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.2c profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-2c-profile.mjs [--write|--check]");
}
