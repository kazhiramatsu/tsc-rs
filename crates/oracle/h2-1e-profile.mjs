import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-1e-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-1e-profile.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-1e-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-1e-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH = "ratchets/h2-1e-owner-controls.v1.json";
const TRUSTED_BASE = "3cfa24fd7ef3bdd8dab97d4adf860306fac75782";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-1d-profile.v1.json", "b7518fa72bf021b2bdbd9dfba13d9f0f316180a22711fd585a5558f708890874"],
  ["qualification", "ratchets/h2-1d-qualification.v1.json", "8823422b4b648775c53f14bada27646fbd4eb63ae43a38b9e8e159f08f68379f"],
  ["owner_controls", "ratchets/h2-1d-owner-controls.v1.json", "aba2b26cbd92fab4bd6525274b5d4561bf168e45353a3c3ab6f8dea070054916"],
  ["profile_generator", "crates/oracle/h2-1d-profile.mjs", "15aad36782cc22563b3ef1357a52d1e3cc61b4244d3638b7fbee2b8dbafee9c8"],
  ["qualification_generator", "crates/oracle/h2-1d-qualification.mjs", "9709e49b8a1cd0d1bb28c1ec9ce08bea4e81e2a2e92fa429721d8e4497a93403"],
  ["owner_controls_generator", "crates/oracle/h2-1d-owner-controls.mjs", "1b2e2abdd44dc4cf394a55a3b90c28deae27d801576bf5597de71485fe31437b"],
  ["profile_contract", ".github/ci/contracts/h2-1d-profile.schema.json", "bb34e87b872b5f35f223ec424f4dcd621f63d1b8cd224d5ac5230eac7d966310"],
  ["qualification_contract", ".github/ci/contracts/h2-1d-qualification.schema.json", "6a7ca902431fa0149eb852e66badf21e2fb0ef9183c1e663310325547968539f"],
  ["owner_controls_contract", ".github/ci/contracts/h2-1d-owner-controls.schema.json", "cc203640d70cc4427cd3dc2a9491a91b53a5c898308b61c4101d87d32d8c095d"],
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
  "crates/program/src/module_requests.rs",
  "crates/program/tests/integration/module_request_contract.rs",
  "crates/syntax/src/incremental.rs",
  "crates/syntax/src/lib.rs",
  "crates/syntax/src/parser.rs",
  "crates/syntax/tests/unit/incremental/tests.rs",
  "crates/syntax/tests/unit/parser/tests.rs",
  "crates/xtask/src/h2_1d_acceptance.rs",
  "crates/xtask/src/h2_1e_acceptance.rs",
  "crates/xtask/src/main.rs",
  "crates/xtask/tests/unit/h2_1d_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_1e_acceptance/tests.rs",
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
      qualification.phase === "H2.1e-node-formats-source-and-emit" &&
      qualification.summary.candidates === 6 &&
      qualification.summary.admitted_cases === 4 &&
      qualification.summary.diagnostic_deferred_output_control_cases === 0 &&
      qualification.summary.source_deferred_cases === 2 &&
      qualification.summary.admitted_typescript_writes === 8 &&
      qualification.summary.admitted_typescript_diagnostics === 6 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.1e qualification is not closed",
  );
  requireCondition(
    ownerControls.schema === 1 &&
      ownerControls.phase === "H2.1e-node-format-owner-controls" &&
      ownerControls.status === "qualified" &&
      ownerControls.summary.controls === 4 &&
      ownerControls.summary.exact_outputs === 56 &&
      ownerControls.summary.typescript_runs === 30 &&
      ownerControls.summary.diagnostics === 0,
    "H2.1e owner controls are not closed",
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
      phase: "H2.1e",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_1d_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.1d artifacts remain immutable lineage; current runtime ownership transfers to this H2.1e profile",
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
          "Node16(100)",
          "Node18(101)",
          "Node20(102)",
          "NodeNext(199)",
        ],
        source_kinds: [".ts", ".mts", ".cts"],
        products: ["javascript"],
        exact_cases: 266,
        h2_1e_exact_cases: 4,
        exact_reported_diagnostics: 518,
        exact_writes: 297,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 24,
        candidate_denominator: 295,
        h2_1e_candidate_denominator: 6,
      },
      transition: {
        completed_slice: "H2.1e",
        next_slice: "H2.2a",
        active_runtime_slices: ["H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e"],
        inactive_runtime_slice_count: 32,
        node_format_owner: "complete",
        typescript_transform_owner: "H2.2a",
        bundle_owner: "H2.7d",
        h2_1e_deferred_slices: ["H2.2d", "H2.9"],
        h2_1e_deferred_cases: 2,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_1e_two_worker_execution_is_exact_and_isolated",
        import_attribute_control:
          "h2_1e_import_attribute_order_and_bytes_are_exact",
        node_format_owner_control:
          "h2_1e_node_format_owner_controls_match_pinned_typescript",
        fresh_package_and_casing_control:
          "h2_1e_fresh_package_type_and_path_casing_are_isolated",
        incremental_source_fact_control:
          "import_attribute_and_typescript_extension_edits_are_fresh_equivalent",
        deferred_format_control:
          "unsupported_options_and_unadmitted_extensions_fail_before_the_first_sink_call",
        sink_fault_control:
          "h2_1e_node_format_filesystem_failure_preserves_partial_set_continuation_and_activity",
        owner_controls: {
          artifact: pathHash(OWNER_CONTROLS_RELATIVE_PATH),
          generator: pathHash("crates/oracle/h2-1e-owner-controls.mjs"),
          contract: pathHash(
            ".github/ci/contracts/h2-1e-owner-controls.schema.json",
          ),
        },
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_1d_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 5,
        next_runtime_slices: 1,
        runtime_admissions: 266,
        executed_candidates: 295,
        h2_1e_executed_candidates: 6,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-1e-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.1e profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-1e-profile.mjs [--write|--check]");
}
