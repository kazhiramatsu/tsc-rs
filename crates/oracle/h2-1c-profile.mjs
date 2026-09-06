import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-1c-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-1c-profile.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-1c-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-1c-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH = "ratchets/h2-1c-owner-controls.v1.json";
const TRUSTED_BASE = "53a5509cc6a3f295744a7286a0bbc4b7c6096fcb";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-1b-profile.v1.json", "0d766e10405099e6ddcd6960d6f1b4c72a832aa4770b2147f49786a2d07d2fe1"],
  ["qualification", "ratchets/h2-1b-qualification.v1.json", "4fdab0ef83b0b32de53e251d0717a62c436e0f3963e7637187297104b9cefd2c"],
  ["profile_generator", "crates/oracle/h2-1b-profile.mjs", "5d930f92388b2e6c6b4abf038e52e6cf6a484d9c397da76e8244c8f553d3699d"],
  ["qualification_generator", "crates/oracle/h2-1b-qualification.mjs", "75982b69f18cc3ac26ac168a0339269cb01a80d5831c61c3570e324ebf2feaae"],
  ["profile_contract", ".github/ci/contracts/h2-1b-profile.schema.json", "4881d93dd418fd23cc0af5ad092fb979fb74211821dbebc29618f834d450792d"],
  ["qualification_contract", ".github/ci/contracts/h2-1b-qualification.schema.json", "bca2d5ff2d17d5444623e7cb58bfbb27bef5f0cb5b67f6da3a30566b66efb59e"],
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
  "crates/xtask/src/main.rs",
  "crates/xtask/tests/unit/h2_1c_acceptance/tests.rs",
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
      qualification.phase === "H2.1c-amd-umd-source-and-emit" &&
      qualification.summary.candidates === 8 &&
      qualification.summary.admitted_cases === 6 &&
      qualification.summary.diagnostic_deferred_output_control_cases === 0 &&
      qualification.summary.source_deferred_cases === 2 &&
      qualification.summary.admitted_typescript_writes === 12 &&
      qualification.summary.admitted_typescript_diagnostics === 6 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.1c qualification is not closed",
  );
  requireCondition(
    ownerControls.schema === 1 &&
      ownerControls.phase === "H2.1c-amd-umd-owner-controls" &&
      ownerControls.status === "qualified" &&
      ownerControls.summary.controls === 1 &&
      ownerControls.summary.exact_outputs === 2 &&
      ownerControls.summary.typescript_runs === 4 &&
      ownerControls.summary.diagnostics === 0,
    "H2.1c owner controls are not closed",
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
      phase: "H2.1c",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_1b_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.1b artifacts remain immutable lineage; current runtime ownership transfers to this H2.1c profile",
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
        ],
        source_kinds: [".ts"],
        products: ["javascript"],
        exact_cases: 257,
        h2_1c_exact_cases: 6,
        exact_reported_diagnostics: 507,
        exact_writes: 278,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 33,
        candidate_denominator: 295,
        h2_1c_candidate_denominator: 8,
      },
      transition: {
        completed_slice: "H2.1c",
        next_slice: "H2.1d",
        active_runtime_slices: ["H2.1a", "H2.1b", "H2.1c"],
        inactive_runtime_slice_count: 34,
        system_owner: "H2.1d",
        bundle_owner: "H2.7d",
        amd_umd_deferred_slices: ["H2.2d"],
        amd_umd_deferred_cases: 2,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_1c_two_worker_execution_is_exact_and_isolated",
        ordering_and_helper_control: "h2_1c_multifile_order_and_helper_dedup_are_exact",
        dependency_and_name_control:
          "h2_1c_amd_pragmas_and_static_dependency_order_match_the_pinned_transform",
        incremental_source_fact_control: "amd_pragma_edits_are_fresh_equivalent",
        deferred_format_control:
          "unsupported_options_and_extensions_fail_before_the_first_sink_call",
        sink_fault_control:
          "h2_1c_amd_umd_filesystem_failure_preserves_partial_set_continuation_and_activity",
        owner_controls: {
          artifact: pathHash(OWNER_CONTROLS_RELATIVE_PATH),
          generator: pathHash("crates/oracle/h2-1c-owner-controls.mjs"),
          contract: pathHash(
            ".github/ci/contracts/h2-1c-owner-controls.schema.json",
          ),
        },
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_1b_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 3,
        next_runtime_slices: 1,
        runtime_admissions: 257,
        executed_candidates: 295,
        h2_1c_executed_candidates: 8,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-1c-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.1c profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-1c-profile.mjs [--write|--check]");
}
