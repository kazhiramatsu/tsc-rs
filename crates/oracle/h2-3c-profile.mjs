import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-3c-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-3c-profile.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-3c-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-3c-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH = "ratchets/h2-3c-owner-controls.v1.json";
const TRUSTED_BASE = "7aaaa414133d630180931dd79cd9169d43e54121";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-3b-profile.v1.json", "b3b58853309ffd5ed1bcf2727c296357b3b6c5e13523063801c871dbbdedb705"],
  ["qualification", "ratchets/h2-3b-qualification.v1.json", "4ae20ded498223f4bcfd9e3911753948bc850763899a9629f9b2d1760a3b5a36"],
  ["owner_controls", "ratchets/h2-3b-owner-controls.v1.json", "7cb7058d0b2130a196ba0352093c531fc7ea059a69bd7438585936d04e2e7e72"],
  ["profile_generator", "crates/oracle/h2-3b-profile.mjs", "af474d1a1489e1b67638daa242bb38eecd7645b3b0fb8308c7447daf4745e0bc"],
  ["qualification_generator", "crates/oracle/h2-3b-qualification.mjs", "6c2d181642a5afd16645dc3ca288a5233b090232dcd7d7b4e603b838678660b6"],
  ["owner_controls_generator", "crates/oracle/h2-3b-owner-controls.mjs", "d2bee6c116d2f98a31e4bf67b0601d80384236f9db8e851a82d72fc8bec283de"],
  ["profile_contract", ".github/ci/contracts/h2-3b-profile.schema.json", "d21a658c8fc4d004130074deb38b60b7d133e3ace6b7b497a1a54cc2beb48f04"],
  ["qualification_contract", ".github/ci/contracts/h2-3b-qualification.schema.json", "a3c531969af2f368933ad3b4dbdb56a0edf0605536d685fe9fb34beb3da5c232"],
  ["owner_controls_contract", ".github/ci/contracts/h2-3b-owner-controls.schema.json", "fb1f709a9c0b468b1c1849f2a6c4268e94729869d7bc5a91f742101c486b3f12"],
]);

const RUNTIME_INPUTS = Object.freeze([
  "crates/checker/src/emit.rs",
  "crates/checker/src/evaluate.rs",
  "crates/checker/src/lib.rs",
  "crates/checker/src/modules.rs",
  "crates/compiler/src/lib.rs",
  "crates/compiler/tests/integration/emit_session_contract.rs",
  "crates/compiler/tests/integration/h1_emit_qualification_contract.rs",
  "crates/compiler/tests/integration/program_session_contract.rs",
  "crates/compiler/tests/integration/upstream_no_emit_harness_contract.rs",
  "crates/diagnostics/src/gen.rs",
  "crates/emitter/src/activity.rs",
  "crates/emitter/src/builtins.rs",
  "crates/emitter/src/builtins/jsx.rs",
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
  "crates/emitter/tests/unit/activity/tests.rs",
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
  "crates/xtask/src/h2_3b_acceptance.rs",
  "crates/xtask/src/h2_3c_acceptance.rs",
  "crates/xtask/src/main.rs",
  "crates/xtask/tests/unit/h2_1d_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_1e_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2a_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2b_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2c_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2d_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_3a_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_3b_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_3c_acceptance/tests.rs",
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
      qualification.phase === "H2.3c-automatic-jsx-runtime" &&
      qualification.status === "qualified-typescript-oracle" &&
      qualification.summary.candidates === 4 &&
      qualification.summary.admitted_cases === 4 &&
      qualification.summary.deferred_cases === 0 &&
      qualification.summary.diagnostic_deferred_output_control_cases === 0 &&
      qualification.summary.source_deferred_cases === 0 &&
      qualification.summary.admitted_typescript_writes === 4 &&
      qualification.summary.admitted_typescript_diagnostics === 42 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.3c qualification is not closed",
  );
  requireCondition(
    ownerControls.schema === 1 &&
      ownerControls.phase === "H2.3c-automatic-jsx-owner-controls" &&
      ownerControls.status === "qualified" &&
      ownerControls.summary.controls === 9 &&
      ownerControls.summary.exact_outputs === 9 &&
      ownerControls.summary.typescript_runs === 18 &&
      ownerControls.summary.reported_diagnostics === 0,
    "H2.3c owner controls are not closed",
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
      phase: "H2.3c",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_3b_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.3b artifacts remain immutable lineage; current runtime ownership transfers to this H2.3c profile",
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
        jsx_modes: [
          "Preserve(1)", "React(2)", "ReactNative(3)",
          "ReactJSX(4)", "ReactJSXDev(5)",
        ],
        source_kinds: [".ts", ".mts", ".cts", ".tsx", ".js", ".mjs", ".cjs", ".jsx"],
        products: ["javascript", "mjs", "cjs", "jsx"],
        exact_cases: 309,
        h2_3c_exact_cases: 4,
        exact_reported_diagnostics: 680,
        exact_writes: 404,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 0,
        candidate_denominator: 302,
        h2_3c_candidate_denominator: 4,
      },
      transition: {
        completed_slice: "H2.3c",
        next_slice: "H2.3d",
        active_runtime_slices: [
          "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a",
          "H2.2b", "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c",
        ],
        inactive_runtime_slice_count: 25,
        classic_jsx_tsx_owner: "complete",
        automatic_jsx_runtime_owner: "complete",
        json_output_owner: "H2.3d",
        general_output_matrix_owner: "H2.8a",
        h2_3c_deferred_slices: [],
        h2_3c_deferred_cases: 0,
        inherited_h2_3b_residual_slices: [],
        inherited_h2_3b_residual_cases: 0,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_3c_two_worker_execution_is_exact_and_isolated",
        denominator_control:
          "h2_3c_denominator_is_the_four_h2_3b_source_deferred_rows",
        automatic_transform_control:
          "h2_3c_jsx_jsxs_fragments_keys_children_and_spread_fallback_match_typescript",
        development_runtime_control:
          "h2_3c_jsxdev_source_metadata_static_children_and_name_collisions_match_typescript",
        import_source_control:
          "h2_3c_option_and_pragma_import_source_and_runtime_precedence_match_typescript",
        module_interaction_control:
          "h2_3c_esm_commonjs_and_system_helper_import_projections_match_typescript",
        historical_residual_control:
          "h2_3c_h2_3b_source_deferred_rows_are_exactly_promoted",
        owner_matrix_control:
          "h2_3c_owner_controls_cover_runtimes_imports_pragmas_extensions_and_modules",
        owner_controls: {
          artifact: pathHash(OWNER_CONTROLS_RELATIVE_PATH),
          generator: pathHash("crates/oracle/h2-3c-owner-controls.mjs"),
          contract: pathHash(
            ".github/ci/contracts/h2-3c-owner-controls.schema.json",
          ),
        },
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_3b_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 12,
        next_runtime_slices: 1,
        runtime_admissions: 309,
        executed_candidates: 302,
        h2_3c_executed_candidates: 4,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-3c-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.3c profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-3c-profile.mjs [--write|--check]");
}
