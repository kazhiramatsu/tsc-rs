import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-2a-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-2a-profile.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-2a-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-2a-qualification.v1.json";
const TRUSTED_BASE = "ba45ad089d903e632676950be9fcea3ab56fbb37";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-1e-profile.v1.json", "5e7c743544a841b07e46ce4cdb4b02b21d171af00e3b19c7408a76aa44406d7a"],
  ["qualification", "ratchets/h2-1e-qualification.v1.json", "ad43c6b036d4699d3d2625c1d1bdb88768a34588b4f87cca2fa30632b7db761c"],
  ["owner_controls", "ratchets/h2-1e-owner-controls.v1.json", "259edadf2d0db814353b5de2060bb1ea9164e2c4f2a10a990ddc1594d7e9f6f4"],
  ["profile_generator", "crates/oracle/h2-1e-profile.mjs", "862c2b8bd0b41c34358b42121760e519722954d49e023daccf1aa67e39fdbbbc"],
  ["qualification_generator", "crates/oracle/h2-1e-qualification.mjs", "9f222b2301ea6d09e65e9e67708be575761a65c2d3b0bed0c38946c0d4c0fa6f"],
  ["owner_controls_generator", "crates/oracle/h2-1e-owner-controls.mjs", "e3feec8f379215a3beb47342e57cf8b3bd6e5481cc83070b732358fde3bb57f1"],
  ["profile_contract", ".github/ci/contracts/h2-1e-profile.schema.json", "1f174584838885f3e26edb5bb8425056194bc285289820201237c1fc5660a305"],
  ["qualification_contract", ".github/ci/contracts/h2-1e-qualification.schema.json", "a9e72293589f1915c6e33f00487fc1c9d84025a81731cce9f8377f61c6e84fcc"],
  ["owner_controls_contract", ".github/ci/contracts/h2-1e-owner-controls.schema.json", "3ffaff1f62f94a036225dfe1e9cdb382a42968ea5aa98f4828e3bce1f8ac7956"],
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
  "crates/xtask/src/main.rs",
  "crates/xtask/tests/unit/h2_1d_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_1e_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2a_acceptance/tests.rs",
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
      qualification.phase === "H2.2a-runtime-and-const-enums" &&
      qualification.summary.candidates === 11 &&
      qualification.summary.admitted_cases === 6 &&
      qualification.summary.diagnostic_deferred_output_control_cases === 0 &&
      qualification.summary.source_deferred_cases === 5 &&
      qualification.summary.admitted_typescript_writes === 9 &&
      qualification.summary.admitted_typescript_diagnostics === 8 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.2a qualification is not closed",
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
      phase: "H2.2a",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_1e_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.1e artifacts remain immutable lineage; current runtime ownership transfers to this H2.2a profile",
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
        exact_cases: 272,
        h2_2a_exact_cases: 6,
        exact_reported_diagnostics: 526,
        exact_writes: 306,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 18,
        candidate_denominator: 295,
        h2_2a_candidate_denominator: 11,
      },
      transition: {
        completed_slice: "H2.2a",
        next_slice: "H2.2b",
        active_runtime_slices: [
          "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a",
        ],
        inactive_runtime_slice_count: 31,
        runtime_enum_owner: "complete",
        namespace_owner: "H2.2b",
        h2_2a_deferred_slices: ["H2.2b", "H2.2d"],
        h2_2a_deferred_cases: 5,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_2a_two_worker_execution_is_exact_and_isolated",
        runtime_and_const_enum_control:
          "h2_2a_runtime_and_const_enum_outputs_are_exact",
        integration_shape_control:
          "h2_2a_runtime_and_const_enum_emit_matches_typescript_shapes",
        deferred_owner_control:
          "h2_2a_later_owner_controls_remain_source_deferred",
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_1e_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 6,
        next_runtime_slices: 1,
        runtime_admissions: 272,
        executed_candidates: 295,
        h2_2a_executed_candidates: 11,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-2a-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.2a profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-2a-profile.mjs [--write|--check]");
}
