import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-3d-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-3d-profile.v1.json";
const CONTRACT_RELATIVE_PATH = ".github/ci/contracts/h2-3d-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-3d-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH = "ratchets/h2-3d-owner-controls.v1.json";
const PARENT_PROFILE_RELATIVE_PATH = "ratchets/h2-3c-profile.v1.json";
const TRUSTED_BASE = "9bc22c84a6c09149f31b9daa100e302d3730e6b2";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-3c-profile.v1.json", "00f2596d0ee273de3fd8bdc26b79e3f338282b0a38c4cab00fd8c9f2c15121b4"],
  ["qualification", "ratchets/h2-3c-qualification.v1.json", "3ce3972d82086553681a68794dc31ffdf50707e6dabf00cd16082deb6508cdf5"],
  ["owner_controls", "ratchets/h2-3c-owner-controls.v1.json", "7e158b0311f5c3a6b3fcb60dcfbbebb5cd418d603502022382473ac3ac7b916b"],
  ["profile_generator", "crates/oracle/h2-3c-profile.mjs", "00b826ac00f5c210993a1c056011efb636f0c02b984dbecc83059d64df6aabd6"],
  ["qualification_generator", "crates/oracle/h2-3c-qualification.mjs", "c7591914719ea8664e71749fac2a6f0b727816f2c42b2f047d853e7960a530a7"],
  ["owner_controls_generator", "crates/oracle/h2-3c-owner-controls.mjs", "351b828df9d01025702d15cc7ea9e368bf6fd9218b6eedaccb7fcfbe7be49def"],
  ["profile_contract", ".github/ci/contracts/h2-3c-profile.schema.json", "aa9b14c064e5be1ee77c3fd43b81994b81bcce0f930582730e24e2a3fe70ef70"],
  ["qualification_contract", ".github/ci/contracts/h2-3c-qualification.schema.json", "be3e00a3cf132b4c8d991a7fbfd0c853247a96b6ca9c6d36e49f7092352ab49e"],
  ["owner_controls_contract", ".github/ci/contracts/h2-3c-owner-controls.schema.json", "a0a0d54df71862647cfd21d58ea960ddfdded1d8c4df8b552e943297e5579ed1"],
]);

const NEW_RUNTIME_INPUTS = Object.freeze([
  "crates/program/tests/integration/no_lib_program_loader_contract.rs",
  "crates/xtask/src/h2_3d_acceptance.rs",
  "crates/xtask/tests/unit/h2_3d_acceptance/tests.rs",
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
  const parentProfile = readJson(PARENT_PROFILE_RELATIVE_PATH);
  requireCondition(
    qualification.schema === 1 &&
      qualification.phase === "H2.3d-json-source-output" &&
      qualification.status === "qualified-typescript-oracle" &&
      qualification.summary.global_h2_3d_rows === 695 &&
      qualification.summary.future_deferred_rows === 695 &&
      qualification.summary.candidates === 0 &&
      qualification.summary.admitted_cases === 0 &&
      qualification.summary.deferred_cases === 0 &&
      qualification.summary.source_deferred_cases === 0 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0,
    "H2.3d qualification is not closed",
  );
  requireCondition(
    ownerControls.schema === 1 &&
      ownerControls.phase === "H2.3d-json-source-owner-controls" &&
      ownerControls.status === "qualified" &&
      ownerControls.summary.controls === 14 &&
      ownerControls.summary.exact_outputs === 13 &&
      ownerControls.summary.typescript_runs === 28 &&
      ownerControls.summary.reported_diagnostics === 2,
    "H2.3d owner controls are not closed",
  );

  const historical = Object.fromEntries(
    HISTORICAL_AUTHORITIES.map(([key, relativePath, expected]) => {
      const record = pathHash(relativePath);
      requireCondition(
        record.sha256 === expected,
        `${relativePath} historical bytes changed`,
      );
      return [key, record];
    }),
  );
  const runtimeInputPaths = [
    ...parentProfile.runtime_inputs.map((record) => record.path),
    ...NEW_RUNTIME_INPUTS,
  ];
  requireCondition(
    new Set(runtimeInputPaths).size === 63,
    "H2.3d runtime input identity changed",
  );

  return withFingerprint(
    {
      schema: 1,
      kind: "h2-runtime-profile",
      status: "qualified",
      phase: "H2.3d",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_3c_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.3c artifacts remain immutable lineage; current runtime ownership transfers to this H2.3d profile",
      },
      qualification: pathHash(QUALIFICATION_RELATIVE_PATH),
      runtime_inputs: runtimeInputPaths.map(pathHash),
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
        source_kinds: [
          ".ts", ".mts", ".cts", ".tsx", ".js", ".mjs", ".cjs", ".jsx", ".json",
        ],
        products: ["javascript", "mjs", "cjs", "jsx", "json"],
        exact_cases: 309,
        h2_3d_exact_cases: 0,
        exact_reported_diagnostics: 680,
        exact_writes: 404,
        diagnostic_deferred_output_controls: 5,
        diagnostic_control_writes: 5,
        source_deferred_cases: 0,
        candidate_denominator: 302,
        h2_3d_candidate_denominator: 0,
        h2_3d_global_future_rows: 695,
      },
      transition: {
        completed_slice: "H2.3d",
        next_slice: "H2.4a",
        active_runtime_slices: [
          "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a",
          "H2.2b", "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c",
          "H2.3d",
        ],
        inactive_runtime_slice_count: 24,
        classic_jsx_tsx_owner: "complete",
        automatic_jsx_runtime_owner: "complete",
        json_output_owner: "complete",
        decorators_owner: "H2.4a",
        general_output_matrix_owner: "H2.8a",
        h2_3d_candidate_cases: 0,
        h2_3d_global_future_rows: 695,
        h2_3d_future_dependency_groups: 30,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_3d_owner_controls_run_twice_in_isolated_programs",
        denominator_control:
          "h2_3d_denominator_is_zero_after_h2_3c_dependency_closure_and_all_695_global_rows_retain_future_dependencies",
        json_text_control:
          "h2_3d_json_whitespace_trailing_commas_escapes_bom_and_newlines_match_typescript",
        path_control:
          "h2_3d_json_outdir_same_location_empty_and_mixed_write_order_match_typescript",
        module_interaction_control:
          "h2_3d_json_output_is_module_invariant_and_resolve_json_module_5070_5071_match_typescript",
        owner_matrix_control:
          "h2_3d_owner_controls_cover_text_paths_bom_newlines_modules_diagnostics_and_mixed_sources",
        owner_controls: {
          artifact: pathHash(OWNER_CONTROLS_RELATIVE_PATH),
          generator: pathHash("crates/oracle/h2-3d-owner-controls.mjs"),
          contract: pathHash(
            ".github/ci/contracts/h2-3d-owner-controls.schema.json",
          ),
        },
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_3c_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 13,
        next_runtime_slices: 1,
        runtime_admissions: 309,
        executed_candidates: 302,
        h2_3d_executed_candidates: 0,
        h2_3d_global_future_rows: 695,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-3d-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.3d profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-3d-profile.mjs [--write|--check]");
}
