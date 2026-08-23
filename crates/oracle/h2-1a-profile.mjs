import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-1a-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-1a-profile.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-1a-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-1a-qualification.v1.json";
const TRUSTED_BASE = "b22491e86da731e4657fb8ec2c31c19291099b4c";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile_transition", "ratchets/h2-profile-transition.v1.json", "d31e9f50e83d8a5f09fb1e90cbb7033fdc45ad58bb074183df2eec7bcf8a1cab"],
  ["runtime_baseline", "ratchets/h2-runtime-baseline.v1.json", "634492148d44c374c922ed6bd0545c43cdcabe913c78dbffd9d2f940c4ac7cd9"],
  ["transition_generator", "crates/oracle/h2-transition.mjs", "0f94817914e632a8cd106aed83878720df9fd560810ad9c90e9589795cf9d7cf"],
  ["baseline_generator", "crates/oracle/h2-baseline.mjs", "dd282acbf207980db2fcd540206957f980c4e05a1b41b68e1bc347d6b5487197"],
  ["transition_contract", ".github/ci/contracts/h2-profile-transition.schema.json", "fb899f795a4b07b38da88748117347b3f662d85e4be194b2334b48c05e519b23"],
  ["baseline_contract", ".github/ci/contracts/h2-runtime-baseline.schema.json", "a37cbec40a1773d3b4199739764f34882457aaebe0661280ede262246b769fae"],
]);

const RUNTIME_INPUTS = Object.freeze([
  "crates/compiler/src/cli.rs",
  "crates/compiler/src/lib.rs",
  "crates/compiler/tests/integration/emit_session_contract.rs",
  "crates/emitter/src/activity.rs",
  "crates/emitter/src/builtins.rs",
  "crates/emitter/src/execute.rs",
  "crates/emitter/src/host.rs",
  "crates/emitter/src/printer.rs",
  "crates/emitter/src/transform.rs",
  "crates/emitter/tests/integration/output_plan_contract.rs",
  "crates/harness/src/upstream_suites/execution.rs",
  "crates/xtask/src/h2_1a_acceptance.rs",
  "crates/xtask/src/main.rs",
  "crates/xtask/tests/unit/h2_1a_acceptance/tests.rs",
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
      qualification.phase === "H2.1a-implied-esm-source-and-emit" &&
      qualification.summary.candidates === 295 &&
      qualification.summary.admitted_cases === 241 &&
      qualification.summary.diagnostic_deferred_output_control_cases === 5 &&
      qualification.summary.source_deferred_cases === 49 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.1a qualification is not closed",
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
      phase: "H2.1a",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_0b_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.0a/H2.0b artifacts remain immutable lineage; current runtime ownership transfers to this H2.1a profile",
      },
      qualification: pathHash(QUALIFICATION_RELATIVE_PATH),
      runtime_inputs: RUNTIME_INPUTS.map(pathHash),
      admitted_profile: {
        execution: "single-project-one-shot-whole-program",
        target: "ESNext(99)",
        module_states: ["absent-effective-ESNext", "ESNext(99)"],
        source_kinds: [".ts"],
        products: ["javascript"],
        exact_cases: 241,
        exact_reported_diagnostics: 499,
        exact_writes: 251,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 49,
        candidate_denominator: 295,
      },
      transition: {
        completed_slice: "H2.1a",
        next_slice: "H2.1b",
        active_runtime_slices: ["H2.1a"],
        inactive_runtime_slice_count: 36,
        commonjs_owner: "H2.1b",
        commonjs_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_1a_two_worker_execution_is_exact_and_isolated",
        sink_fault_control:
          "h2_1a_filesystem_failure_preserves_partial_set_continuation_and_activity",
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_resource_baseline: historical.runtime_baseline,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 1,
        next_runtime_slices: 1,
        runtime_admissions: 241,
        executed_candidates: 295,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-1a-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.1a profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-1a-profile.mjs [--write|--check]");
}
