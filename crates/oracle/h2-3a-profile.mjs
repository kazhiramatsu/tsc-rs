import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-3a-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-3a-profile.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-3a-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-3a-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH = "ratchets/h2-3a-owner-controls.v1.json";
const TRUSTED_BASE = "03bbbe9dde5df1e5491a8a0568998fa2865600b5";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-2d-profile.v1.json", "645302111e07d23debffbd4df3eb2c6d5a6d51bdb39179d8924645e45dceafbe"],
  ["qualification", "ratchets/h2-2d-qualification.v1.json", "a9e10b645d4be3585576d37efcf8a2424f4ae2e01a66cc8dffef037d7372a9f5"],
  ["profile_generator", "crates/oracle/h2-2d-profile.mjs", "5fafadc48efe8d93e90c330672f6f403f6380849b2fbfd95c7f689e76e3549bb"],
  ["qualification_generator", "crates/oracle/h2-2d-qualification.mjs", "14b76a2e0f1ff5c18955741e0df73337e0b73a588ec5bcd9ccc8c47c6c84005d"],
  ["profile_contract", ".github/ci/contracts/h2-2d-profile.schema.json", "a935234328e34a87cd295e894410ca23e9cb3ba3736634d9aba786212ce58804"],
  ["qualification_contract", ".github/ci/contracts/h2-2d-qualification.schema.json", "33fc12f2fb96e4aa12620ffd9f68ca7acf4d09b17f38b89347fc2db61f405e60"],
]);

const RUNTIME_INPUTS = Object.freeze([
  "crates/checker/src/emit.rs",
  "crates/checker/src/evaluate.rs",
  "crates/checker/src/lib.rs",
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
  "crates/emitter/src/plan.rs",
  "crates/emitter/src/printer.rs",
  "crates/emitter/src/resolver.rs",
  "crates/emitter/src/transform.rs",
  "crates/emitter/tests/integration/active_transform_contract.rs",
  "crates/emitter/tests/integration/output_plan_contract.rs",
  "crates/harness/src/upstream_suites/execution.rs",
  "crates/harness/src/upstream_suites/execution/project.rs",
  "crates/program/src/prepared.rs",
  "crates/program/src/loader.rs",
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
  "crates/xtask/src/h2_3a_acceptance.rs",
  "crates/xtask/src/main.rs",
  "crates/xtask/tests/unit/h2_1d_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_1e_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2a_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2b_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2c_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2d_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_3a_acceptance/tests.rs",
  "crates/types/src/options.rs",
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
      qualification.phase === "H2.3a-javascript-source-output" &&
      qualification.summary.candidates === 1 &&
      qualification.summary.admitted_cases === 1 &&
      qualification.summary.diagnostic_deferred_output_control_cases === 0 &&
      qualification.summary.source_deferred_cases === 0 &&
      qualification.summary.admitted_typescript_writes === 1 &&
      qualification.summary.admitted_typescript_diagnostics === 1 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.3a qualification is not closed",
  );
  requireCondition(
    ownerControls.schema === 1 &&
      ownerControls.phase === "H2.3a-javascript-source-owner-controls" &&
      ownerControls.status === "qualified" &&
      ownerControls.summary.controls === 1 &&
      ownerControls.summary.variants === 3 &&
      ownerControls.summary.exact_outputs === 9 &&
      ownerControls.summary.typescript_runs === 6 &&
      ownerControls.summary.reported_diagnostics === 1,
    "H2.3a owner controls are not closed",
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
      phase: "H2.3a",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_2d_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.2d artifacts remain immutable lineage; current runtime ownership transfers to this H2.3a profile",
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
        source_kinds: [".ts", ".mts", ".cts", ".js", ".mjs", ".cjs"],
        products: ["javascript", "mjs", "cjs"],
        exact_cases: 303,
        h2_3a_exact_cases: 1,
        exact_reported_diagnostics: 634,
        exact_writes: 398,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 0,
        candidate_denominator: 296,
        h2_3a_candidate_denominator: 1,
      },
      transition: {
        completed_slice: "H2.3a",
        next_slice: "H2.3b",
        active_runtime_slices: [
          "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a", "H2.2b", "H2.2c", "H2.2d", "H2.3a",
        ],
        inactive_runtime_slice_count: 27,
        runtime_enum_owner: "complete",
        namespace_owner: "complete",
        parameter_property_owner: "complete",
        import_equals_owner: "complete",
        export_equals_owner: "complete",
        javascript_source_output_owner: "complete",
        general_output_matrix_owner: "H2.8a",
        h2_3a_deferred_slices: [],
        h2_3a_deferred_cases: 0,
        inherited_h2_2d_residual_slices: [],
        inherited_h2_2d_residual_cases: 0,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_3a_two_worker_execution_is_exact_and_isolated",
        javascript_collision_control:
          "h2_3a_javascript_collision_and_typescript_sibling_write_are_exact",
        checked_unchecked_control:
          "h2_3a_check_js_changes_diagnostics_without_changing_source_routing",
        source_family_control:
          "h2_3a_mjs_and_cjs_roots_materialize_the_planned_extension",
        printer_control:
          "h2_3a_javascript_print_preserves_shebang_directive_and_attached_comments",
        extension_planning_control:
          "h2_3a_javascript_families_keep_their_runtime_extensions_when_relocated",
        javascript_owner_control:
          "h2_3a_javascript_owner_controls_match_pinned_typescript",
        denominator_control:
          "h2_3a_denominator_is_the_single_dependency_closed_global_row",
        owner_controls: {
          artifact: pathHash(OWNER_CONTROLS_RELATIVE_PATH),
          generator: pathHash("crates/oracle/h2-3a-owner-controls.mjs"),
          contract: pathHash(
            ".github/ci/contracts/h2-3a-owner-controls.schema.json",
          ),
        },
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_2d_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 10,
        next_runtime_slices: 1,
        runtime_admissions: 303,
        executed_candidates: 296,
        h2_3a_executed_candidates: 1,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-3a-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.3a profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-3a-profile.mjs [--write|--check]");
}
