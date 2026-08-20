import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-5g-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-5g-profile.v1.json";
const CONTRACT_RELATIVE_PATH = ".github/ci/contracts/h2-5g-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-5g-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH = "ratchets/h2-5g-owner-controls.v1.json";
const PARENT_PROFILE_RELATIVE_PATH = "ratchets/h2-5f-profile.v1.json";
const H2_1A_QUALIFICATION_RELATIVE_PATH = "ratchets/h2-1a-qualification.v1.json";
const H2_1A_QUALIFICATION_SHA256 =
  "80bfd75d48b3f3c76c46ddea711242ade8fb09d1fe6e5af2c26b79f39159411f";
const H2_1A_CURRENT_EXACT_PROMOTIONS = Object.freeze([
  Object.freeze({
    source_phase: "H2.1a",
    case_id: "typescript-6.0.3/compiler/arrayFromAsync.ts#default",
    historical_case_fingerprint_sha256:
      "9f63ee4777950bf7023052d1eef2c48a0fea492820217a9e4ff49cdc86da19aa",
    historical_disposition: "diagnostic-deferred-output-control",
    historical_diagnostic_state: "deferred-to-H2.9",
    current_disposition: "exact-required",
    exact_reported_diagnostics: 0,
    exact_writes: 1,
  }),
  Object.freeze({
    source_phase: "H2.1a",
    case_id:
      "typescript-6.0.3/compiler/arrayIterationLibES5TargetDifferent.ts#nolib%3Dtrue%2Ctarget%3Desnext",
    historical_case_fingerprint_sha256:
      "7d155578b5fa4353d81d2798cdc36d4d55bd5c892e2eec1b5580ad8d89f82292",
    historical_disposition: "diagnostic-deferred-output-control",
    historical_diagnostic_state: "deferred-to-H2.9",
    current_disposition: "exact-required",
    exact_reported_diagnostics: 11,
    exact_writes: 1,
  }),
  Object.freeze({
    source_phase: "H2.1a",
    case_id: "typescript-6.0.3/compiler/mapGroupBy.ts#default",
    historical_case_fingerprint_sha256:
      "f0bcdec1d79c70a608fcbbd5ae0629dc5637866d306c5fb1e5ba4a8e8fd371a5",
    historical_disposition: "diagnostic-deferred-output-control",
    historical_diagnostic_state: "deferred-to-H2.9",
    current_disposition: "exact-required",
    exact_reported_diagnostics: 0,
    exact_writes: 1,
  }),
  Object.freeze({
    source_phase: "H2.1a",
    case_id: "typescript-6.0.3/compiler/objectGroupBy.ts#default",
    historical_case_fingerprint_sha256:
      "5d4213b87de2c690084a684f590df4e2f4dd16f54789073916e347cd01f13d13",
    historical_disposition: "diagnostic-deferred-output-control",
    historical_diagnostic_state: "deferred-to-H2.9",
    current_disposition: "exact-required",
    exact_reported_diagnostics: 1,
    exact_writes: 1,
  }),
  Object.freeze({
    source_phase: "H2.1a",
    case_id:
      "typescript-6.0.3/compiler/regularExpressionScanning.ts#target%3Desnext",
    historical_case_fingerprint_sha256:
      "991c35eaee4cb7fd5a92b60fd696b3bc1046a36bab6bf25af873dae39ae6a428",
    historical_disposition: "diagnostic-deferred-output-control",
    historical_diagnostic_state: "deferred-to-H2.9",
    current_disposition: "exact-required",
    exact_reported_diagnostics: 193,
    exact_writes: 1,
  }),
]);
const TRUSTED_BASE = "11f5d0abb93fed4b109bdb1dc552721ceb05e707";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-5f-profile.v1.json", "68c3c6ed51afa36a668aaf6fa338df2da87d6cbaad4740be25e8733be1a45b73"],
  ["qualification", "ratchets/h2-5f-qualification.v1.json", "0292e5612ba878dd1b45726d1b2b8fa26e4ea59ae5be45ff8b32166426e2e2dd"],
  ["owner_controls", "ratchets/h2-5f-owner-controls.v1.json", "a4d9f500be900a0e3f759ba3231a3db20f789f5dcf4b888137ca886686ce9469"],
  ["profile_generator", "crates/oracle/h2-5f-profile.mjs", "d5593648869817dde318c26156d4b025298fbdf411fe67a5c89fc6df8e5a715d"],
  ["qualification_generator", "crates/oracle/h2-5f-qualification.mjs", "72773d747b0da690f7614dbd16755e5904aa617cc8e0b0f6573edbb84c342fad"],
  ["owner_controls_generator", "crates/oracle/h2-5f-owner-controls.mjs", "8b922d23867a697345be2ef173815feb85bc4543a47f636d3db08eaaf6dfb80e"],
  ["profile_contract", ".github/ci/contracts/h2-5f-profile.schema.json", "5e57df22fab8c62dee892564090681afd48bfa2ec72d582356cf9ec1b99488ee"],
  ["qualification_contract", ".github/ci/contracts/h2-5f-qualification.schema.json", "562a98c418e649440fe3aaf7ed6ef52af185099fb09f27b41254cc9606b1f362"],
  ["owner_controls_contract", ".github/ci/contracts/h2-5f-owner-controls.schema.json", "b11e39f35381e8b24218dc6d4dac68b73de346330c68726cac5519e31da0a947"],
]);

// These files establish new runtime or direct acceptance ownership in H2.5g.
// Existing runtime inputs retain their parent order and receive fresh hashes;
// this append-only set is every non-oracle crate path changed from the trusted
// H2.5f merge that was not already part of the parent profile.
const NEW_RUNTIME_INPUTS = Object.freeze([
  "crates/checker/Cargo.toml",
  "crates/checker/src/access.rs",
  "crates/checker/src/check.rs",
  "crates/checker/src/constraints.rs",
  "crates/checker/src/contextual.rs",
  "crates/checker/src/elaboration.rs",
  "crates/checker/src/expr.rs",
  "crates/checker/src/facts.rs",
  "crates/checker/src/functions.rs",
  "crates/checker/src/globals.rs",
  "crates/checker/src/indexed.rs",
  "crates/checker/src/jsx.rs",
  "crates/checker/src/links.rs",
  "crates/checker/src/literals.rs",
  "crates/checker/src/operators.rs",
  "crates/checker/src/program.rs",
  "crates/checker/src/relate.rs",
  "crates/checker/src/speculate.rs",
  "crates/checker/src/spell.rs",
  "crates/checker/src/state.rs",
  "crates/checker/src/statements.rs",
  "crates/checker/src/structural.rs",
  "crates/checker/src/unions.rs",
  "crates/checker/tests/unit/access/tests.rs",
  "crates/checker/tests/unit/annotate/alias_and_typeof_tests.rs",
  "crates/checker/tests/unit/annotate/late_binding_tests.rs",
  "crates/checker/tests/unit/annotate/mapped_type_tests.rs",
  "crates/checker/tests/unit/annotate/unique_symbol_tests.rs",
  "crates/checker/tests/unit/calls/tests.rs",
  "crates/checker/tests/unit/check/tests.rs",
  "crates/checker/tests/unit/constraints/tests.rs",
  "crates/checker/tests/unit/elaboration/tests.rs",
  "crates/checker/tests/unit/emit/tests.rs",
  "crates/checker/tests/unit/functions/tests.rs",
  "crates/checker/tests/unit/indexed/tests.rs",
  "crates/checker/tests/unit/inference/tests.rs",
  "crates/checker/tests/unit/jsx/tests.rs",
  "crates/checker/tests/unit/lib/tests.rs",
  "crates/checker/tests/unit/literals/tests.rs",
  "crates/checker/tests/unit/mapped/tests.rs",
  "crates/checker/tests/unit/modules/tests.rs",
  "crates/checker/tests/unit/operators/tests.rs",
  "crates/checker/tests/unit/program/tests.rs",
  "crates/checker/tests/unit/relate/tests.rs",
  "crates/checker/tests/unit/resolve/tests.rs",
  "crates/checker/tests/unit/speculate/tests.rs",
  "crates/checker/tests/unit/statements/tests.rs",
  "crates/checker/tests/unit/structural/tests.rs",
  "crates/checker/tests/unit/unions/tests.rs",
  "crates/checker/tests/unit/variance/tests.rs",
  "crates/compiler/tests/integration/filesystem_loader_contract.rs",
  "crates/compiler/tests/unit/lib/tests.rs",
  "crates/emitter/src/comment_cursor.rs",
  "crates/emitter/src/position.rs",
  "crates/emitter/src/token_cursor.rs",
  "crates/emitter/src/writer.rs",
  "crates/emitter/tests/contracts.rs",
  "crates/emitter/tests/integration/token_cursor_contract.rs",
  "crates/emitter/tests/integration/writer_position_contract.rs",
  "crates/emitter/tests/source_comment_topology_contract.rs",
  "crates/emitter/tests/unit/builtins/tests.rs",
  "crates/emitter/tests/unit/comment_scope_predicate/tests.rs",
  "crates/emitter/tests/unit/lib/tests.rs",
  "crates/emitter/tests/unit/token_cursor/tests.rs",
  "crates/harness/src/lib.rs",
  "crates/harness/tests/integration/upstream_execution_plan.rs",
  "crates/harness/tests/unit/lib/tests.rs",
  "crates/program/src/config.rs",
  "crates/program/src/lib.rs",
  "crates/program/src/library.rs",
  "crates/program/src/module_resolution.rs",
  "crates/program/src/option_validation.rs",
  "crates/program/src/resolution.rs",
  "crates/program/tests/integration/config_paths_program_options_contract.rs",
  "crates/program/tests/integration/config_program_loader_contract.rs",
  "crates/program/tests/integration/config_root_plan_contract.rs",
  "crates/program/tests/integration/library_program_loader_contract.rs",
  "crates/program/tests/integration/module_resolution_contract.rs",
  "crates/program/tests/unit/library/tests.rs",
  "crates/syntax/nodes.schema.json",
  "crates/syntax/src/for_each_child.rs",
  "crates/syntax/src/nodes.rs",
  "crates/syntax/src/observable_fields.rs",
  "crates/syntax/src/regex.rs",
  "crates/syntax/src/relocate.rs",
  "crates/syntax/src/scanner.rs",
  "crates/syntax/tests/unit/regex/tests.rs",
  "crates/syntax/tests/unit/scanner/tests.rs",
  "crates/xtask/tests/unit/h2_1a_acceptance/tests.rs",
  "crates/types/src/flags.rs",
  // Pre-closure Functional-CI shadow packages are workspace inputs, but they
  // do not participate in the H2 semantic action graph. Binding their exact
  // bytes here keeps the legacy profile honest without granting them H2
  // qualification or authority.
  "crates/ci-adapter-tsc-rs-control/Cargo.toml",
  "crates/ci-adapter-tsc-rs-control/src/lib.rs",
  "crates/ci-adapter-tsc-rs-control/tests/plan.rs",
  "crates/ci-adapter-tsc-rs-protocol/Cargo.toml",
  "crates/ci-adapter-tsc-rs-protocol/src/lib.rs",
  "crates/ci-adapter-tsc-rs-protocol/tests/protocol.rs",
  "crates/ci-core/Cargo.toml",
  "crates/ci-core/src/adapter.rs",
  "crates/ci-core/src/canonical.rs",
  "crates/ci-core/src/digest.rs",
  "crates/ci-core/src/explain.rs",
  "crates/ci-core/src/graph.rs",
  "crates/ci-core/src/graph_schema.rs",
  "crates/ci-core/src/graph_validation.rs",
  "crates/ci-core/src/hash.rs",
  "crates/ci-core/src/identity.rs",
  "crates/ci-core/src/ids.rs",
  "crates/ci-core/src/impact.rs",
  "crates/ci-core/src/input.rs",
  "crates/ci-core/src/inventory.rs",
  "crates/ci-core/src/lib.rs",
  "crates/ci-core/src/membership.rs",
  "crates/ci-core/src/model.rs",
  "crates/ci-core/src/registry.rs",
  "crates/ci-harness-tsc-rs/Cargo.toml",
  "crates/ci-harness-tsc-rs/src/main.rs",
  "crates/ci-harness-tsc-rs/tests/process.rs",
  "crates/ci-runner/Cargo.toml",
  "crates/ci-runner/src/bounded.rs",
  "crates/ci-runner/src/error.rs",
  "crates/ci-runner/src/lib.rs",
  "crates/ci-runner/src/resource.rs",
  "crates/ci-runner/src/snapshot.rs",
  "crates/ci-testkit/Cargo.toml",
  "crates/ci-testkit/src/lib.rs",
  "crates/ci-testkit/tests/fixtures.rs",
  "crates/ci-core/tests/contracts.rs",
  "crates/ci-core/tests/contracts/canonical.rs",
  "crates/ci-core/tests/contracts/descriptors.rs",
  "crates/ci-core/tests/contracts/explain.rs",
  "crates/ci-core/tests/contracts/graph.rs",
  "crates/ci-core/tests/contracts/graph_schema.rs",
  "crates/ci-core/tests/contracts/graph_validation.rs",
  "crates/ci-core/tests/contracts/hashes.rs",
  "crates/ci-core/tests/contracts/identifiers.rs",
  "crates/ci-core/tests/contracts/identity.rs",
  "crates/ci-core/tests/contracts/impact.rs",
  "crates/ci-core/tests/contracts/inventory.rs",
  "crates/ci-core/tests/contracts/registry_membership.rs",
  "crates/ci-harness-tsc-rs/tests/unit/main_tests.rs",
  "crates/ci-runner/tests/contracts.rs",
  "crates/ci-runner/tests/contracts/bounded_effect.rs",
  "crates/ci-runner/tests/contracts/error_boundary.rs",
  "crates/ci-runner/tests/contracts/snapshot_resource.rs",
  "crates/ci-runner/tests/unit/bounded_tests.rs",
  "crates/ci-runner/tests/unit/snapshot_tests.rs",
  "crates/emitter/tests/integration/tsx_type_argument_transform_contract.rs",
  "crates/emitter/tests/unit/builtins_jsx_tests.rs",
  "crates/emitter/tests/unit/target_bindings_tests.rs",
  "crates/harness/tests/unit/upstream_suites/execution_tests.rs",
  "crates/program/tests/unit/option_validation_tests.rs",
  "crates/emitter/tests/integration/comment_scope_witness_contract.rs",
]);

// These files implement the non-authoritative acceptance impact/restart
// shadow. They are not read by the fixed H2.5g acceptance command and must
// not silently become H2 runtime evidence inputs.
const NON_RUNTIME_SHADOW_INPUTS = new Set([
  "crates/xtask/src/acceptance_plan.rs",
  "crates/xtask/src/acceptance_slices.rs",
  "crates/xtask/tests/unit/acceptance_plan/tests.rs",
  "crates/xtask/tests/unit/acceptance_slices/tests.rs",
  "crates/xtask/src/local_ci_resume.rs",
  "crates/xtask/tests/unit/local_ci_resume/tests.rs",
  // Evidence/gate producers and their tests: B2-B4 artifact production,
  // the performance observation, and the CI lane/worker policy live
  // outside the H2 emit runtime, like the resume journal above.
  "crates/xtask/src/m8_evidence.rs",
  "crates/xtask/tests/unit/m8_evidence/tests.rs",
  "crates/xtask/tests/unit/main/ci_lane_tests.rs",
  "crates/harness/tests/integration/h1_compiler_profile_classification.rs",
  "crates/harness/tests/integration/h1_conformance_profile_classification.rs",
  "crates/harness/tests/integration/h1_fourslash_whole_program_equivalence.rs",
  "crates/harness/tests/integration/h1_project_profile_classification.rs",
  "crates/harness/tests/integration/h2_transition.rs",
  "crates/harness/tests/integration/h2_1a_profile.rs",
  "crates/harness/tests/integration/transpile_suite_inventory.rs",
  // Diagnostic conformance-runner orchestration: drives the T0 harness
  // over ProgramSession's no-emit surface and is outside the H2 emit
  // runtime (emit acceptance routes through the harness emit drivers,
  // not this runner).
  "crates/conformance/src/h0_memory.rs",
  "crates/conformance/src/bounded_pipeline.rs",
  "crates/conformance/src/families.rs",
  "crates/conformance/src/lib.rs",
  "crates/conformance/src/ratchet.rs",
  "crates/conformance/tests/unit/bounded_pipeline/tests.rs",
  "crates/conformance/tests/unit/lib/tests.rs",
  // Recovery-census gate infrastructure over the diagnostic corpus.
  "crates/xtask/src/recovery_census.rs",
  // Workspace-audit maintenance rules (the CS-6 permanent
  // zero-contextless audit): gate infrastructure, not read by the fixed
  // H2.5g acceptance command.
  "crates/xtask/src/workspace_maintenance.rs",
  "crates/xtask/tests/unit/workspace_maintenance/tests.rs",
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

function changedRuntimeInputPaths() {
  const runGit = (argv) =>
    execFileSync("git", argv, {
      cwd: WORKSPACE,
      maxBuffer: 16 * 1024 * 1024,
    })
      .toString("utf8")
      .split("\0")
      .filter(Boolean);
  return [
    ...runGit([
      "diff",
      "--name-only",
      "--diff-filter=ACMRTUXB",
      "-z",
      TRUSTED_BASE,
      "--",
      "crates",
    ]),
    ...runGit([
      "ls-files",
      "--others",
      "--exclude-standard",
      "-z",
      "--",
      "crates",
    ]),
  ]
    .filter((relativePath) => !relativePath.startsWith("crates/oracle/"))
    .filter((relativePath) => !NON_RUNTIME_SHADOW_INPUTS.has(relativePath))
    .filter((relativePath, index, paths) => paths.indexOf(relativePath) === index)
    .sort();
}

function withFingerprint(value, field) {
  return { ...value, [field]: sha256(Buffer.from(canonical(value), "utf8")) };
}

function buildArtifact() {
  const qualification = readJson(QUALIFICATION_RELATIVE_PATH);
  const ownerControls = readJson(OWNER_CONTROLS_RELATIVE_PATH);
  const parentProfile = readJson(PARENT_PROFILE_RELATIVE_PATH);
  const h2_1aQualification = readJson(H2_1A_QUALIFICATION_RELATIVE_PATH);
  requireCondition(
    qualification.schema === 1 &&
      qualification.phase === "H2.5g-es2016-target" &&
      qualification.status === "qualified-typescript-oracle" &&
      qualification.selection_contract.global_h2_5g_rows === 11_910 &&
      qualification.selection_contract.global_candidate_denominator === 9_027 &&
      qualification.selection_contract.candidate_denominator === 9_027 &&
      qualification.selection_contract.future_deferred_rows === 2_883 &&
      qualification.summary.candidates === 9_027 &&
      qualification.summary.compiler_candidates === 4_712 &&
      qualification.summary.conformance_candidates === 4_315 &&
      qualification.summary.recorded_compiler_plan_cases === 4_712 &&
      qualification.summary.qualified_vfs_cases === 4_315 &&
      qualification.summary.virtual_config_cases === 56 &&
      qualification.summary.vfs_symlink_cases === 3 &&
      qualification.summary.vfs_symlink_paths === 4 &&
      qualification.summary.admitted_cases === 8_511 &&
      qualification.summary.deferred_cases === 516 &&
      qualification.summary.diagnostic_deferred_output_control_cases === 0 &&
      qualification.summary.source_deferred_cases === 516 &&
      qualification.summary.no_emit_control_cases === 59 &&
      qualification.summary.typescript_runs === 18_054 &&
      qualification.summary.deterministic_typescript_cases === 9_027 &&
      qualification.summary.admitted_typescript_writes === 9_466 &&
      qualification.summary.diagnostic_control_typescript_writes === 0 &&
      qualification.summary.admitted_typescript_diagnostics === 26_815 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0 &&
      Array.isArray(qualification.cases) &&
      qualification.cases.length === 9_027 &&
      Array.isArray(qualification.owner_closure) &&
      qualification.owner_closure.length === 1 &&
      qualification.owner_closure[0].key === "transform-es2016",
    "H2.5g qualification is not closed",
  );
  requireCondition(
    ownerControls.schema === 1 &&
      ownerControls.phase === "H2.5g-es2016-target-owner-controls" &&
      ownerControls.status === "qualified" &&
      ownerControls.summary.controls === 22 &&
      ownerControls.summary.exact_outputs === 21 &&
      ownerControls.summary.typescript_runs === 44 &&
      ownerControls.summary.reported_diagnostics === 2 &&
      ownerControls.summary.emit_diagnostics === 1 &&
      ownerControls.summary.no_emit_on_error_controls === 1 &&
      ownerControls.summary.es2015_controls === 21 &&
      ownerControls.summary.es2016_controls === 1 &&
      ownerControls.summary.exponentiation_controls === 22 &&
      ownerControls.summary.exponentiation_assignment_controls === 15 &&
      ownerControls.summary.property_assignment_controls === 6 &&
      ownerControls.summary.element_assignment_controls === 5 &&
      ownerControls.summary.parameter_controls === 1 &&
      ownerControls.summary.collision_controls === 1 &&
      ownerControls.summary.super_controls === 1 &&
      ownerControls.summary.precedence_controls === 1 &&
      ownerControls.summary.comment_controls === 1 &&
      ownerControls.summary.class_composition_controls === 5 &&
      ownerControls.summary.commonjs_controls === 1 &&
      ownerControls.summary.async_composition_controls === 2 &&
      ownerControls.summary.using_controls === 1 &&
      ownerControls.summary.h2_5a_active_controls === 21 &&
      ownerControls.summary.h2_5b_active_controls === 21 &&
      ownerControls.summary.h2_5c_active_controls === 21 &&
      ownerControls.summary.h2_5d_active_controls === 21 &&
      ownerControls.summary.h2_5e_active_controls === 21 &&
      ownerControls.summary.h2_5f_active_controls === 21 &&
      ownerControls.summary.h2_5g_active_controls === 20 &&
      Array.isArray(ownerControls.controls) &&
      ownerControls.controls.length === 22,
    "H2.5g owner controls are not closed",
  );
  requireCondition(
    parentProfile.schema === 1 &&
      parentProfile.phase === "H2.5f" &&
      parentProfile.admitted_profile.exact_cases === 680 &&
      parentProfile.summary.completed_runtime_slices === 21 &&
      qualification.typescript.version === "6.0.3" &&
      qualification.typescript.source_commit ===
        "050880ce59e30b356b686bd3144efe24f875ebc8" &&
      canonical(ownerControls.typescript) === canonical(qualification.typescript) &&
      canonical(parentProfile.typescript) === canonical(qualification.typescript),
    "H2.5f parent profile is not closed",
  );

  const historical = Object.fromEntries(
    HISTORICAL_AUTHORITIES.map(([key, relativePath, expected]) => {
      const record = pathHash(relativePath);
      requireCondition(record.sha256 === expected, `${relativePath} historical bytes changed`);
      return [key, record];
    }),
  );
  const h2_1aQualificationRecord = pathHash(H2_1A_QUALIFICATION_RELATIVE_PATH);
  requireCondition(
    h2_1aQualificationRecord.sha256 === H2_1A_QUALIFICATION_SHA256,
    `${H2_1A_QUALIFICATION_RELATIVE_PATH} historical bytes changed`,
  );
  const h2_1aHistoricalControls = h2_1aQualification.cases?.filter(
    (candidate) => candidate.disposition === "diagnostic-deferred-output-control",
  );
  requireCondition(
    h2_1aHistoricalControls?.length === H2_1A_CURRENT_EXACT_PROMOTIONS.length &&
      h2_1aHistoricalControls.every((candidate) =>
        H2_1A_CURRENT_EXACT_PROMOTIONS.some(
          (promotion) => promotion.case_id === candidate.case_id,
        ),
      ),
    "H2.1a current exact promotion denominator changed",
  );
  const h2_1aCurrentExactPromotions = H2_1A_CURRENT_EXACT_PROMOTIONS.map(
    (promotion) => {
      const candidate = h2_1aHistoricalControls.find(
        (candidate) => candidate.case_id === promotion.case_id,
      );
      requireCondition(
        candidate?.case_fingerprint_sha256 ===
            promotion.historical_case_fingerprint_sha256 &&
          candidate.disposition === promotion.historical_disposition &&
          candidate.diagnostic_disposition?.state ===
            promotion.historical_diagnostic_state &&
          candidate.typescript_runs?.length === 2 &&
          candidate.typescript_runs.every(
            (run) =>
              run.reported_diagnostics?.length ===
                promotion.exact_reported_diagnostics &&
              run.writes?.length === promotion.exact_writes,
          ),
        `${promotion.case_id} current exact promotion evidence changed`,
      );
      return {
        ...promotion,
        historical_qualification: h2_1aQualificationRecord,
      };
    },
  );
  const runtimeInputPaths = [
    ...parentProfile.runtime_inputs.map((record) => record.path),
    ...NEW_RUNTIME_INPUTS,
  ];
  const runtimeInputSet = new Set(runtimeInputPaths);
  const parentRuntimeInputSet = new Set(
    parentProfile.runtime_inputs.map((record) => record.path),
  );
  const changedRuntimeInputs = changedRuntimeInputPaths();
  const missingRuntimeInputs = changedRuntimeInputs.filter(
    (relativePath) => !runtimeInputSet.has(relativePath),
  );
  const staleNewRuntimeInputs = NEW_RUNTIME_INPUTS.filter(
    (relativePath) =>
      parentRuntimeInputSet.has(relativePath) ||
      !changedRuntimeInputs.includes(relativePath),
  );
  requireCondition(
    missingRuntimeInputs.length === 0,
    `H2.5g runtime input closure is missing ${missingRuntimeInputs.join(", ")}`,
  );
  requireCondition(
    staleNewRuntimeInputs.length === 0,
    `H2.5g new runtime inputs are stale ${staleNewRuntimeInputs.join(", ")}`,
  );
  requireCondition(
    runtimeInputSet.size === 238,
    "H2.5g runtime input identity changed",
  );

  return withFingerprint(
    {
      schema: 1,
      kind: "h2-runtime-profile",
      status: "qualified",
      phase: "H2.5g",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_5f_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.5f artifacts remain immutable lineage; current runtime ownership transfers to this H2.5g profile",
      },
      qualification: pathHash(QUALIFICATION_RELATIVE_PATH),
      current_exact_promotions: h2_1aCurrentExactPromotions,
      runtime_inputs: runtimeInputPaths.map(pathHash),
      admitted_profile: {
        execution: "single-project-one-shot-whole-program",
        target_states: [
          "ES2015(2)", "ES2016(3)", "ES2017(4)", "ES2018(5)", "ES2019(6)",
          "ES2020(7)", "ES2021(8)", "ES2022(9)", "ES2023(10)",
          "ES2024(11)", "ES2025(12)", "ESNext(99)",
        ],
        module_states: [
          "absent-effective-ESNext", "None(0)", "ES2015(5)", "ES2020(6)",
          "ES2022(7)", "ESNext(99)", "CommonJS(1)", "AMD(2)", "UMD(3)",
          "System(4)", "Node16(100)", "Node18(101)", "Node20(102)",
          "NodeNext(199)", "Preserve(200)",
        ],
        jsx_modes: [
          "Preserve(1)", "React(2)", "ReactNative(3)", "ReactJSX(4)", "ReactJSXDev(5)",
        ],
        source_kinds: [
          ".ts", ".mts", ".cts", ".tsx", ".js", ".mjs", ".cjs", ".jsx", ".json",
        ],
        products: ["javascript", "mjs", "cjs", "jsx", "json"],
        exact_cases: 9_196,
        h2_5g_exact_cases: 8_511,
        exact_reported_diagnostics: 28_415,
        exact_writes: 10_445,
        diagnostic_deferred_output_controls: 0,
        diagnostic_control_writes: 0,
        source_deferred_cases: 531,
        candidate_denominator: 9_715,
        h2_5g_candidate_denominator: 9_027,
        h2_5g_global_future_rows: 2_883,
        h2_5g_owner_controls: 22,
        h2_5g_owner_writes: 21,
      },
      transition: {
        completed_slice: "H2.5g",
        next_slice: "H2.5h-a",
        next_slice_scope:
          "architecture-validation-owner-local-gap-rust-design-and-oracle-fixture-freeze",
        next_runtime_activation_slice: "determined-by-H2.5h-a-owner-graph",
        active_runtime_slices: [
          "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a",
          "H2.2b", "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c",
          "H2.3d", "H2.4a", "H2.4b", "H2.5a", "H2.5b", "H2.5c",
          "H2.5d", "H2.5e", "H2.5f", "H2.5g",
        ],
        inactive_runtime_slice_count: 15,
        classic_jsx_tsx_owner: "complete",
        automatic_jsx_runtime_owner: "complete",
        json_output_owner: "complete",
        legacy_decorators_owner: "complete",
        standard_decorators_and_class_fields_owner: "complete",
        target_esnext_transform_owner: "complete",
        target_es2021_transform_owner: "complete",
        target_es2020_transform_owner: "complete",
        target_es2019_transform_owner: "complete",
        target_es2018_transform_owner: "complete",
        target_es2017_transform_owner: "complete",
        target_es2016_transform_owner: "complete",
        target_es2015_transform_owner: "H2.5h-b+",
        target_generators_transform_owner: "H2.5h-b+",
        general_output_matrix_owner: "H2.8a",
        h2_5g_candidate_cases: 9_027,
        h2_5g_admitted_cases: 8_511,
        h2_5g_global_future_rows: 2_883,
        h2_5g_source_deferred_cases: 516,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_5g_cases_and_owner_controls_run_twice_in_isolated_programs",
        denominator_control: "h2_5g_exact_denominator_is_9027_with_516_source_deferred_cases_and_2883_global_future_rows",
        target_band_control: "es2015_lowers_es2016_syntax_while_es2016_preserves_it",
        exponentiation_control: "binary_and_assignment_exponentiation_preserve_associativity_evaluation_and_temp_ownership",
        generated_binding_control: "typed_scope_hoists_and_binding_identity_preserve_property_element_parameter_and_collision_names",
        composition_control: "async_object_rest_decorators_class_fields_using_and_commonjs_compose_in_transform_order",
        diagnostic_control: "reported_emit_and_no_emit_on_error_diagnostics_match_tsc",
        printer_control: "comments_precedence_and_final_tree_layout_are_exact",
        failure_control: "516_later_owned_sources_fail_before_first_sink_write_and_no_emit_on_error_writes_nothing",
        owner_controls: {
          artifact: pathHash(OWNER_CONTROLS_RELATIVE_PATH),
          generator: pathHash("crates/oracle/h2-5g-owner-controls.mjs"),
          contract: pathHash(".github/ci/contracts/h2-5g-owner-controls.schema.json"),
        },
        qualification_vfs_overlay_test: pathHash(
          "crates/oracle/vfs-directory-overlay.test.mjs",
        ),
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_5f_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 22,
        next_slice_runtime_slice_delta: 0,
        runtime_admissions: 9_196,
        executed_candidates: 9_715,
        h2_5g_executed_candidates: 9_027,
        h2_5g_global_future_rows: 2_883,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-5g-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.5g profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-5g-profile.mjs [--write|--check]");
}
