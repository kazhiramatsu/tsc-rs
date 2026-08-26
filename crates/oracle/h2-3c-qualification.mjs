import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-3c-qualification.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-3c-qualification.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-3c-qualification.schema.json";
const PARENT_RELATIVE_PATH = "ratchets/h2-3b-qualification.v1.json";
const PARENT_PROFILE_RELATIVE_PATH = "ratchets/h2-3b-profile.v1.json";
const CANDIDATE_RELATIVE_PATH =
  "ratchets/h2-candidate-dispositions.v1.json";
const EXPECTED_PARENT_SHA256 =
  "20ba4ffa78740c8e610ff83602f1320d6d203f5ac956cd51d637eecc5f311916";
const EXPECTED_PARENT_PROFILE_SHA256 =
  "b8f1c384635a3a882a9f8e346216dd321e82d6d54ad21f8a57681f158bef2995";
const EXPECTED_CANDIDATE_SHA256 =
  "4dc14f4c650a17e156adcde79054db642e1948339028e29f66ebb7c85b9d5866";
const TRUSTED_BASE_COMMIT = "7aaaa414133d630180931dd79cd9169d43e54121";
const EXPECTED_NODE = "25.2.1";
const CLOSED_SLICES = new Set([
  "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a", "H2.2b",
  "H2.2c", "H2.2d", "H2.3a", "H2.3b",
]);
const EXPECTED_CASE_IDS = Object.freeze([
  "typescript-6.0.3/compiler/jsxNamespacedNameNotComparedToNonMatchingIndexSignature.tsx#default",
  "typescript-6.0.3/conformance/jsx/tsxReactEmit8.tsx#jsx%3Dreact-jsx",
  "typescript-6.0.3/conformance/jsx/tsxReactEmit8.tsx#jsx%3Dreact-jsxdev",
  "typescript-6.0.3/conformance/jsx/tsxReactEmitSpreadAttribute.ts#target%3Desnext",
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

function withFingerprint(value, field) {
  return { ...value, [field]: sha256(Buffer.from(canonical(value), "utf8")) };
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

function validateRuntime() {
  const node = readBytes(".node-version").toString("utf8").trim();
  requireCondition(node === EXPECTED_NODE, "unexpected Node pin");
  requireCondition(process.version === `v${node}`, `requires Node ${node}`);
  requireCondition(ts.version === "6.0.3", "unexpected TypeScript runtime");
}

function withoutFingerprint(value, field) {
  const clone = structuredClone(value);
  delete clone[field];
  return clone;
}

function validateParent(parent) {
  requireCondition(
    sha256(readBytes(PARENT_RELATIVE_PATH)) === EXPECTED_PARENT_SHA256,
    "H2.3b qualification identity changed",
  );
  requireCondition(
    sha256(readBytes(PARENT_PROFILE_RELATIVE_PATH)) ===
      EXPECTED_PARENT_PROFILE_SHA256,
    "H2.3b profile identity changed",
  );
  requireCondition(
    parent.schema === 1 &&
      parent.phase === "H2.3b-classic-jsx-tsx" &&
      parent.status === "qualified-typescript-oracle",
    "H2.3b qualification header changed",
  );
  requireCondition(
    parent.qualification_fingerprint_sha256 ===
      sha256(Buffer.from(canonical(withoutFingerprint(
        parent,
        "qualification_fingerprint_sha256",
      )), "utf8")),
    "H2.3b qualification fingerprint changed",
  );
}

function validateGlobalDenominator(candidateArtifact) {
  requireCondition(
    sha256(readBytes(CANDIDATE_RELATIVE_PATH)) === EXPECTED_CANDIDATE_SHA256,
    "global H2 candidate identity changed",
  );
  const newCandidates = candidateArtifact.cases.filter((entry) => {
    const remaining = entry.required_slices.filter((slice) =>
      !CLOSED_SLICES.has(slice)
    );
    return remaining.length === 1 && remaining[0] === "H2.3c";
  });
  requireCondition(
    newCandidates.length === 0,
    "H2.3c unexpectedly gained a global candidate-disposition row",
  );
  return newCandidates.length;
}

function promoteCase(entry) {
  requireCondition(
    entry.disposition === "deferred-to-slices" &&
      canonical(entry.required_slices) === canonical(["H2.3c"]) &&
      entry.diagnostic_disposition.state === "not-observed-source-deferred",
    `${entry.case_id}: H2.3b carry-forward boundary changed`,
  );
  requireCondition(
    entry.typescript_runs.length === 2 &&
      canonical(entry.typescript_runs[0]) === canonical(entry.typescript_runs[1]),
    `${entry.case_id}: TypeScript two-run evidence changed`,
  );
  requireCondition(
    entry.typescript_runs[0].writes.length === 1 &&
      entry.typescript_runs[0].emit_result.emit_skipped === false &&
      entry.typescript_runs[0].emit_result.diagnostics.length === 0 &&
      entry.typescript_runs[0].status_writes.length === 0,
    `${entry.case_id}: TypeScript emit observation changed`,
  );
  const promoted = structuredClone(entry);
  promoted.disposition = "admitted-for-execution";
  promoted.required_slices = [];
  promoted.diagnostic_disposition = { state: "exact-required" };
  promoted.rust_expectation = "two-deterministic-exact-runs";
  delete promoted.case_fingerprint_sha256;
  return withFingerprint(promoted, "case_fingerprint_sha256");
}

function buildArtifact() {
  validateRuntime();
  const parent = readJson(PARENT_RELATIVE_PATH);
  validateParent(parent);
  const globalNewCandidates = validateGlobalDenominator(
    readJson(CANDIDATE_RELATIVE_PATH),
  );
  const carry = parent.cases.filter((entry) =>
    canonical(entry.required_slices) === canonical(["H2.3c"])
  );
  requireCondition(
    canonical(carry.map((entry) => entry.case_id)) === canonical(EXPECTED_CASE_IDS),
    "H2.3c carry-forward case identity changed",
  );
  const cases = carry.map(promoteCase);
  const exactDiagnostics = cases.reduce(
    (sum, entry) => sum + entry.typescript_runs[0].reported_diagnostics.length,
    0,
  );
  const exactWrites = cases.reduce(
    (sum, entry) => sum + entry.typescript_runs[0].writes.length,
    0,
  );
  requireCondition(exactDiagnostics === 42, "H2.3c diagnostic total changed");
  requireCondition(exactWrites === 4, "H2.3c write total changed");
  return withFingerprint(
    {
      schema: 1,
      phase: "H2.3c-automatic-jsx-runtime",
      status: "qualified-typescript-oracle",
      typescript: parent.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_base_commit: TRUSTED_BASE_COMMIT,
        h2_3b_profile: pathHash(PARENT_PROFILE_RELATIVE_PATH),
      },
      inputs: {
        ...parent.inputs,
        h2_3b_qualification: pathHash(PARENT_RELATIVE_PATH),
      },
      execution_contract: {
        source_reachability: "the four immutable H2.3b source-deferred automatic-runtime rows",
        module_selection: "the already recorded TypeScript Program.getEmitModuleFormatOfFile observation",
        admission: `all ${carry.length} H2.3b carry-forward rows are H2.3c-owned and exact; ${globalNewCandidates} new global candidate-disposition rows remain after H2.3b closure`,
        typescript_repetitions: 2,
        rust_repetitions: 2,
        normalization: "none",
        deferred_boundary: "none; every H2.3c candidate is admitted",
      },
      owner_closure: parent.owner_closure,
      cases,
      summary: {
        candidates: cases.length,
        compiler_candidates: cases.filter((entry) => entry.suite === "compiler").length,
        conformance_candidates: cases.filter((entry) => entry.suite === "conformance").length,
        admitted_cases: cases.length,
        deferred_cases: 0,
        diagnostic_deferred_output_control_cases: 0,
        source_deferred_cases: 0,
        no_emit_control_cases: 0,
        module_states: [{ value: "absent-effective-ESNext", cases: cases.length }],
        dispositions: [{ value: "admitted-for-execution", cases: cases.length }],
        first_deferred_slices: [],
        typescript_runs: cases.length * 2,
        deterministic_typescript_cases: cases.length,
        admitted_typescript_writes: exactWrites,
        diagnostic_control_typescript_writes: 0,
        admitted_typescript_diagnostics: exactDiagnostics,
        unexecuted_candidates: 0,
        undispositioned_candidates: 0,
      },
    },
    "qualification_fingerprint_sha256",
  );
}

function render(value) {
  return JSON.stringify(value, null, 2) + "\n";
}

const artifact = buildArtifact();
const rendered = render(artifact);
const mode = process.argv[2];
if (mode === "--write") {
  fs.writeFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), rendered);
  process.stdout.write(
    `wrote ${TARGET_RELATIVE_PATH}: candidates=${artifact.summary.candidates} admitted=${artifact.summary.admitted_cases} deferred=${artifact.summary.deferred_cases}\n`,
  );
} else if (mode === "--check") {
  requireCondition(
    fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
      fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") === rendered,
    `stale ${TARGET_RELATIVE_PATH}; run h2-3c-qualification.mjs --write and review`,
  );
  process.stdout.write(
    `H2.3c qualification is fresh: candidates=${artifact.summary.candidates} admitted=${artifact.summary.admitted_cases}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-3c-qualification.mjs [--write|--check]");
}
