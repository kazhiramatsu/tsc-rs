import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-1b-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-1b-profile.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-1b-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-1b-qualification.v1.json";
const TRUSTED_BASE = "49a8a87c443972e3dc2a7a57d6f2e45b8581a601";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-1a-profile.v1.json", "16b373b85dcfdca79c181bb381e05937eb65cc39235246762403505500cd5a94"],
  ["qualification", "ratchets/h2-1a-qualification.v1.json", "166429fff483e17f0581269de207346124c08f37df8e5e0e2ab033cb980bae77"],
  ["profile_generator", "crates/oracle/h2-1a-profile.mjs", "ee1ca3e9de285411a6ffc208c776a65719590bbbaf67fe2dd62b1ab1fc958fec"],
  ["qualification_generator", "crates/oracle/h2-1a-qualification.mjs", "32579a1e1f5162220412a0cab93a9d342ce591736a0ed97cb02effe38ce6842f"],
  ["profile_contract", ".github/ci/contracts/h2-1a-profile.schema.json", "6eb004f407b94dbffa5e289dfbce343b63469373886da06b9d1b254a47fc0656"],
  ["qualification_contract", ".github/ci/contracts/h2-1a-qualification.schema.json", "d394d5991041b0b5a16dc5e02b48b93db5f7176aea07aa4fb3442d49db2a87cd"],
]);

const RUNTIME_INPUTS = Object.freeze([
  "crates/checker/src/emit.rs",
  "crates/checker/src/modules.rs",
  "crates/compiler/src/lib.rs",
  "crates/compiler/tests/integration/emit_session_contract.rs",
  "crates/diagnostics/src/gen.rs",
  "crates/emitter/src/activity.rs",
  "crates/emitter/src/builtins.rs",
  "crates/emitter/src/execute.rs",
  "crates/emitter/src/factory.rs",
  "crates/emitter/src/host.rs",
  "crates/emitter/src/printer.rs",
  "crates/emitter/src/resolver.rs",
  "crates/emitter/src/transform.rs",
  "crates/harness/src/upstream_suites/execution.rs",
  "crates/xtask/src/h2_1b_acceptance.rs",
  "crates/xtask/src/main.rs",
  "crates/xtask/tests/unit/h2_1b_acceptance/tests.rs",
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
      qualification.phase === "H2.1b-commonjs-source-and-emit" &&
      qualification.summary.candidates === 15 &&
      qualification.summary.admitted_cases === 10 &&
      qualification.summary.diagnostic_deferred_output_control_cases === 0 &&
      qualification.summary.source_deferred_cases === 5 &&
      qualification.summary.admitted_typescript_writes === 15 &&
      qualification.summary.admitted_typescript_diagnostics === 2 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.1b qualification is not closed",
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
      phase: "H2.1b",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_1a_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.1a artifacts remain immutable lineage; current runtime ownership transfers to this H2.1b profile",
      },
      qualification: pathHash(QUALIFICATION_RELATIVE_PATH),
      runtime_inputs: RUNTIME_INPUTS.map(pathHash),
      admitted_profile: {
        execution: "single-project-one-shot-whole-program",
        target: "ESNext(99)",
        module_states: ["absent-effective-ESNext", "ESNext(99)", "CommonJS(1)"],
        source_kinds: [".ts"],
        products: ["javascript"],
        exact_cases: 251,
        h2_1b_exact_cases: 10,
        exact_reported_diagnostics: 501,
        exact_writes: 266,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 39,
        candidate_denominator: 295,
        h2_1b_candidate_denominator: 15,
      },
      transition: {
        completed_slice: "H2.1b",
        next_slice: "H2.1c",
        active_runtime_slices: ["H2.1a", "H2.1b"],
        inactive_runtime_slice_count: 35,
        amd_umd_owner: "H2.1c",
        commonjs_deferred_slices: ["H2.2a", "H2.2b", "H2.2d"],
        commonjs_deferred_cases: 5,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_1b_two_worker_execution_is_exact_and_isolated",
        ordering_and_helper_control: "h2_1b_multifile_order_and_helper_dedup_are_exact",
        sink_fault_control:
          "h2_1b_commonjs_filesystem_failure_preserves_partial_set_continuation_and_activity",
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_1a_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 2,
        next_runtime_slices: 1,
        runtime_admissions: 251,
        executed_candidates: 295,
        h2_1b_executed_candidates: 15,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-1b-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.1b profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-1b-profile.mjs [--write|--check]");
}
