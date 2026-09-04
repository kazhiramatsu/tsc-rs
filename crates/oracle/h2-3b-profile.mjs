import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-3b-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-3b-profile.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-3b-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-3b-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH = "ratchets/h2-3b-owner-controls.v1.json";
const TRUSTED_BASE = "3ce56e1fdb1c3841bc27f37b33488d3dd25b65a0";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-3a-profile.v1.json", "810d7b705bb914ffbb90e14d8cfdf1d0d203bf0e93ded507fc56f353440bdbef"],
  ["qualification", "ratchets/h2-3a-qualification.v1.json", "84e09aae3b5e196e4e4d6b5a5a10920ed0ffec51ab430b9050ec691010d601ce"],
  ["owner_controls", "ratchets/h2-3a-owner-controls.v1.json", "76ac4421892d434f9b6ee5113994776644e506263d9fc94c2cca79670cadb622"],
  ["profile_generator", "crates/oracle/h2-3a-profile.mjs", "16ba9b4a8da1b692c5628e4abb22d6cd1808548f64171d5efd2184ca845d5952"],
  ["qualification_generator", "crates/oracle/h2-3a-qualification.mjs", "d0671b4cb7d9bc5d287e1fb4cbd9519fa92cc8dae0d5e7a20809f51c4984b68e"],
  ["owner_controls_generator", "crates/oracle/h2-3a-owner-controls.mjs", "535fc11cd834a061ccc28b914e423ec984d484dd3411a2d6d985ec61f6674beb"],
  ["profile_contract", ".github/ci/contracts/h2-3a-profile.schema.json", "da917ede3387e0a3c8ecf7d96f7b74358fd47c22c5ee6199169c672262e5c0df"],
  ["qualification_contract", ".github/ci/contracts/h2-3a-qualification.schema.json", "81644212fe112a9f721f8073aacdd3ab78938a2d893a0191c719b1601daeb48e"],
  ["owner_controls_contract", ".github/ci/contracts/h2-3a-owner-controls.schema.json", "ea587f45ae303e20dd2f6ca8e630bbe8c8f8250984e38861be36535b6b73e4d7"],
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
  "crates/xtask/src/main.rs",
  "crates/xtask/tests/unit/h2_1d_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_1e_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2a_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2b_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2c_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_2d_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_3a_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_3b_acceptance/tests.rs",
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
      qualification.phase === "H2.3b-classic-jsx-tsx" &&
      qualification.status === "qualified-typescript-oracle" &&
      qualification.summary.candidates === 6 &&
      qualification.summary.admitted_cases === 2 &&
      qualification.summary.deferred_cases === 4 &&
      qualification.summary.diagnostic_deferred_output_control_cases === 0 &&
      qualification.summary.source_deferred_cases === 4 &&
      qualification.summary.admitted_typescript_writes === 2 &&
      qualification.summary.admitted_typescript_diagnostics === 4 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.3b qualification is not closed",
  );
  requireCondition(
    ownerControls.schema === 1 &&
      ownerControls.phase === "H2.3b-classic-jsx-owner-controls" &&
      ownerControls.status === "qualified" &&
      ownerControls.summary.controls === 8 &&
      ownerControls.summary.exact_outputs === 8 &&
      ownerControls.summary.typescript_runs === 16 &&
      ownerControls.summary.reported_diagnostics === 0,
    "H2.3b owner controls are not closed",
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
      phase: "H2.3b",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_3a_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.3a artifacts remain immutable lineage; current runtime ownership transfers to this H2.3b profile",
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
        jsx_modes: ["Preserve(1)", "React(2)", "ReactNative(3)"],
        source_kinds: [".ts", ".mts", ".cts", ".tsx", ".js", ".mjs", ".cjs", ".jsx"],
        products: ["javascript", "mjs", "cjs", "jsx"],
        exact_cases: 305,
        h2_3b_exact_cases: 2,
        exact_reported_diagnostics: 638,
        exact_writes: 400,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 4,
        candidate_denominator: 302,
        h2_3b_candidate_denominator: 6,
      },
      transition: {
        completed_slice: "H2.3b",
        next_slice: "H2.3c",
        active_runtime_slices: [
          "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a",
          "H2.2b", "H2.2c", "H2.2d", "H2.3a", "H2.3b",
        ],
        inactive_runtime_slice_count: 26,
        classic_jsx_tsx_owner: "complete",
        automatic_jsx_runtime_owner: "H2.3c",
        json_output_owner: "H2.3d",
        general_output_matrix_owner: "H2.8a",
        h2_3b_deferred_slices: ["H2.3c"],
        h2_3b_deferred_cases: 4,
        inherited_h2_3a_residual_slices: [],
        inherited_h2_3a_residual_cases: 0,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_3b_two_worker_execution_is_exact_and_isolated",
        denominator_control:
          "h2_3b_denominator_separates_classic_from_automatic_runtime",
        classic_transform_control:
          "h2_3b_classic_jsx_factories_fragments_namespaces_and_ranges_match_typescript",
        commonjs_factory_control:
          "h2_3b_classic_factory_import_substitution_and_lexical_shadowing_match_typescript",
        preserve_native_control:
          "h2_3b_preserve_and_react_native_reconstruct_jsx_with_exact_extensions",
        owner_matrix_control:
          "h2_3b_owner_controls_cover_modes_factories_pragmas_and_extensions",
        owner_controls: {
          artifact: pathHash(OWNER_CONTROLS_RELATIVE_PATH),
          generator: pathHash("crates/oracle/h2-3b-owner-controls.mjs"),
          contract: pathHash(
            ".github/ci/contracts/h2-3b-owner-controls.schema.json",
          ),
        },
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_3a_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 11,
        next_runtime_slices: 1,
        runtime_admissions: 305,
        executed_candidates: 302,
        h2_3b_executed_candidates: 6,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-3b-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.3b profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-3b-profile.mjs [--write|--check]");
}
