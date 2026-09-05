import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-1d-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-1d-profile.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-1d-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-1d-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH = "ratchets/h2-1d-owner-controls.v1.json";
const TRUSTED_BASE = "533caca4df1ebcf9e9f2ec5fd13b9c73a3ee2786";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-1c-profile.v1.json", "c13b8b63afaa95d7f1a22332aeb1735928ad3373beabdda90ddca03614795a1b"],
  ["qualification", "ratchets/h2-1c-qualification.v1.json", "bf7789b7b534296b3081c1356923f57fa31b108e2b151f07248f5ffe25fe6add"],
  ["owner_controls", "ratchets/h2-1c-owner-controls.v1.json", "6aa502734a38a0905d5e996dd3d1a61ce20617c9d1ffa4f7b72d689fd95596de"],
  ["profile_generator", "crates/oracle/h2-1c-profile.mjs", "194586085f6c5e1bf17317186be1c7d558ef430503754b0e5cd7133b51f2ebdd"],
  ["qualification_generator", "crates/oracle/h2-1c-qualification.mjs", "76fb79f90b480cd6d3eddb7c1c37b09e04eef0fabb7d7440d2649504a4365af1"],
  ["owner_controls_generator", "crates/oracle/h2-1c-owner-controls.mjs", "d6e7a70b8ec85a82b0f68c7884bdf9c2b15b0f41d0dfffa82cfca2f6a0ed0362"],
  ["profile_contract", ".github/ci/contracts/h2-1c-profile.schema.json", "23fad428ca467c2af1d821081dd58e26d3d637125e5d9a1295af3e7b9f6f4cdd"],
  ["qualification_contract", ".github/ci/contracts/h2-1c-qualification.schema.json", "982d4c268399746b9e04f98154c38dc687971999cddd321796d5645201dcd554"],
  ["owner_controls_contract", ".github/ci/contracts/h2-1c-owner-controls.schema.json", "9c25aade0699776dcf7d9d1d7ad42389e6c8df20d978a219247e6ebda01425bc"],
]);

const RUNTIME_INPUTS = Object.freeze([
  "crates/checker/src/emit.rs",
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
  "crates/emitter/src/printer.rs",
  "crates/emitter/src/resolver.rs",
  "crates/emitter/src/transform.rs",
  "crates/harness/src/upstream_suites/execution.rs",
  "crates/harness/src/upstream_suites/execution/project.rs",
  "crates/program/src/prepared.rs",
  "crates/syntax/src/incremental.rs",
  "crates/syntax/src/lib.rs",
  "crates/syntax/src/parser.rs",
  "crates/syntax/tests/unit/incremental/tests.rs",
  "crates/syntax/tests/unit/parser/tests.rs",
  "crates/xtask/src/h2_1c_acceptance.rs",
  "crates/xtask/src/h2_1d_acceptance.rs",
  "crates/xtask/src/main.rs",
  "crates/xtask/tests/unit/h2_1c_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_1d_acceptance/tests.rs",
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
  requireCondition(
    qualification.schema === 1 &&
      qualification.phase === "H2.1d-system-source-and-emit" &&
      qualification.summary.candidates === 6 &&
      qualification.summary.admitted_cases === 5 &&
      qualification.summary.diagnostic_deferred_output_control_cases === 0 &&
      qualification.summary.source_deferred_cases === 1 &&
      qualification.summary.admitted_typescript_writes === 11 &&
      qualification.summary.admitted_typescript_diagnostics === 5 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.1d qualification is not closed",
  );
  requireCondition(
    ownerControls.schema === 1 &&
      ownerControls.phase === "H2.1d-system-owner-controls" &&
      ownerControls.status === "qualified" &&
      ownerControls.summary.controls === 1 &&
      ownerControls.summary.exact_outputs === 1 &&
      ownerControls.summary.typescript_runs === 2 &&
      ownerControls.summary.diagnostics === 0,
    "H2.1d owner controls are not closed",
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
      phase: "H2.1d",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_1c_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.1c artifacts remain immutable lineage; current runtime ownership transfers to this H2.1d profile",
      },
      qualification: pathHash(QUALIFICATION_RELATIVE_PATH),
      runtime_inputs: RUNTIME_INPUTS.map(pathHash),
      admitted_profile: {
        execution: "single-project-one-shot-whole-program",
        target: "ESNext(99)",
        module_states: [
          "absent-effective-ESNext",
          "ESNext(99)",
          "CommonJS(1)",
          "AMD(2)",
          "UMD(3)",
          "System(4)",
        ],
        source_kinds: [".ts"],
        products: ["javascript"],
        exact_cases: 262,
        h2_1d_exact_cases: 5,
        exact_reported_diagnostics: 512,
        exact_writes: 289,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 28,
        candidate_denominator: 295,
        h2_1d_candidate_denominator: 6,
      },
      transition: {
        completed_slice: "H2.1d",
        next_slice: "H2.1e",
        active_runtime_slices: ["H2.1a", "H2.1b", "H2.1c", "H2.1d"],
        inactive_runtime_slice_count: 33,
        node_format_owner: "H2.1e",
        bundle_owner: "H2.7d",
        amd_umd_deferred_slices: ["H2.2d"],
        amd_umd_deferred_cases: 2,
        system_deferred_slices: ["H2.2a", "H2.2b"],
        system_deferred_cases: 1,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_1d_two_worker_execution_is_exact_and_isolated",
        ordering_and_dynamic_import_control:
          "h2_1d_multifile_order_and_dynamic_import_rewrite_are_exact",
        dependency_and_name_control:
          "h2_1d_system_owner_closure_matches_the_pinned_transform",
        incremental_source_fact_control:
          "dynamic_import_and_import_meta_edits_are_fresh_equivalent",
        deferred_format_control:
          "unsupported_options_and_extensions_fail_before_the_first_sink_call",
        sink_fault_control:
          "h2_1d_system_filesystem_failure_preserves_partial_set_continuation_and_activity",
        owner_controls: {
          artifact: pathHash(OWNER_CONTROLS_RELATIVE_PATH),
          generator: pathHash("crates/oracle/h2-1d-owner-controls.mjs"),
          contract: pathHash(
            ".github/ci/contracts/h2-1d-owner-controls.schema.json",
          ),
        },
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_1c_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 4,
        next_runtime_slices: 1,
        runtime_admissions: 262,
        executed_candidates: 295,
        h2_1d_executed_candidates: 6,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-1d-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.1d profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-1d-profile.mjs [--write|--check]");
}
