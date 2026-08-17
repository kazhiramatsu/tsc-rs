import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-3d-qualification.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-3d-qualification.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-3d-qualification.schema.json";
const CANDIDATE_RELATIVE_PATH = "ratchets/h2-candidate-dispositions.v1.json";
const PARENT_QUALIFICATION_RELATIVE_PATH =
  "ratchets/h2-3c-qualification.v1.json";
const PARENT_PROFILE_RELATIVE_PATH = "ratchets/h2-3c-profile.v1.json";
const PARENT_OWNER_CONTROLS_RELATIVE_PATH =
  "ratchets/h2-3c-owner-controls.v1.json";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const TYPESCRIPT_SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const TRUSTED_BASE_COMMIT = "9bc22c84a6c09149f31b9daa100e302d3730e6b2";
const EXPECTED_NODE = "25.2.1";
const EXPECTED_INPUT_HASHES = Object.freeze({
  [CANDIDATE_RELATIVE_PATH]:
    "8dba7b685cd3b46abfb69d030aaf3a4a82523441e78e4824446e3de09ad13b09",
  [PARENT_QUALIFICATION_RELATIVE_PATH]:
    "4bcb1e6e6d8a977cfab9458e906a6e68ad57c23356f506724d1b1e36375ac2a0",
  [PARENT_PROFILE_RELATIVE_PATH]:
    "50423a666c77404d5636f2a1b522101ffcd3536e768ac01ea4ccc3bf0391fe2d",
  [PARENT_OWNER_CONTROLS_RELATIVE_PATH]:
    "7e158b0311f5c3a6b3fcb60dcfbbebb5cd418d603502022382473ac3ac7b916b",
});
const CLOSED_SLICES_BEFORE = Object.freeze([
  "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a", "H2.2b",
  "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c",
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

function validateInputs() {
  for (const [relativePath, expected] of Object.entries(EXPECTED_INPUT_HASHES)) {
    requireCondition(
      sha256(readBytes(relativePath)) === expected,
      `${relativePath} identity changed`,
    );
  }
  const qualification = readJson(PARENT_QUALIFICATION_RELATIVE_PATH);
  const profile = readJson(PARENT_PROFILE_RELATIVE_PATH);
  const controls = readJson(PARENT_OWNER_CONTROLS_RELATIVE_PATH);
  requireCondition(
    qualification.phase === "H2.3c-automatic-jsx-runtime" &&
      qualification.status === "qualified-typescript-oracle",
    "H2.3c qualification header changed",
  );
  requireCondition(
    profile.phase === "H2.3c" && profile.status === "qualified",
    "H2.3c profile header changed",
  );
  requireCondition(
    controls.phase === "H2.3c-automatic-jsx-owner-controls" &&
      controls.status === "qualified",
    "H2.3c owner-control header changed",
  );
}

function groupFutureDependencies(rows, closedThrough) {
  const groups = new Map();
  for (const row of rows) {
    const requiredSlices = row.required_slices.filter((slice) =>
      !closedThrough.has(slice)
    );
    requireCondition(
      requiredSlices.length > 0,
      `${row.id}: H2.3d row has no future dependency`,
    );
    const key = canonical(requiredSlices);
    const group = groups.get(key) ?? { required_slices: requiredSlices, cases: 0 };
    group.cases += 1;
    groups.set(key, group);
  }
  return [...groups.values()].sort((left, right) =>
    canonical(left.required_slices).localeCompare(canonical(right.required_slices))
  );
}

function buildArtifact() {
  validateRuntime();
  validateInputs();
  const candidateArtifact = readJson(CANDIDATE_RELATIVE_PATH);
  requireCondition(
    candidateArtifact.schema === 1 &&
      candidateArtifact.phase === "H2.0a-runner-candidate-dispositions" &&
      candidateArtifact.status === "frozen",
    "global candidate-disposition header changed",
  );
  const closedBefore = new Set(CLOSED_SLICES_BEFORE);
  const closedThrough = new Set([...CLOSED_SLICES_BEFORE, "H2.3d"]);
  const h2Rows = candidateArtifact.cases.filter((entry) =>
    entry.required_slices.includes("H2.3d")
  );
  const candidates = h2Rows.filter((entry) => {
    const remaining = entry.required_slices.filter((slice) =>
      !closedBefore.has(slice)
    );
    return canonical(remaining) === canonical(["H2.3d"]);
  });
  requireCondition(h2Rows.length === 695, "global H2.3d row count changed");
  requireCondition(
    candidates.length === 0,
    "H2.3d unexpectedly gained an executable candidate row",
  );
  const futureDependencyGroups = groupFutureDependencies(h2Rows, closedThrough);
  requireCondition(
    futureDependencyGroups.length === 30 &&
      futureDependencyGroups.reduce((sum, group) => sum + group.cases, 0) === 695,
    "H2.3d future-dependency partition changed",
  );

  return withFingerprint(
    {
      schema: 1,
      phase: "H2.3d-json-source-output",
      status: "qualified-typescript-oracle",
      typescript: {
        version: ts.version,
        source_commit: TYPESCRIPT_SOURCE_COMMIT,
        bundle: pathHash(TYPESCRIPT_BUNDLE),
        implementation: pathHash(TYPESCRIPT_IMPLEMENTATION),
      },
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: { trusted_base_commit: TRUSTED_BASE_COMMIT },
      inputs: {
        candidate_dispositions: pathHash(CANDIDATE_RELATIVE_PATH),
        h2_3c_qualification: pathHash(PARENT_QUALIFICATION_RELATIVE_PATH),
        h2_3c_profile: pathHash(PARENT_PROFILE_RELATIVE_PATH),
        h2_3c_owner_controls: pathHash(PARENT_OWNER_CONTROLS_RELATIVE_PATH),
      },
      selection_contract: {
        global_h2_3d_rows: h2Rows.length,
        closed_slices_before_h2_3d: CLOSED_SLICES_BEFORE,
        candidate_definition:
          "after removing H2.1a through H2.3c, required_slices is exactly [H2.3d]",
        candidate_denominator: candidates.length,
        future_deferred_definition:
          "a global H2.3d row that retains at least one post-H2.3d dependency",
        future_deferred_rows: h2Rows.length - candidates.length,
      },
      future_dependency_groups: futureDependencyGroups,
      cases: [],
      summary: {
        global_h2_3d_rows: h2Rows.length,
        future_deferred_rows: h2Rows.length,
        future_dependency_groups: futureDependencyGroups.length,
        candidates: 0,
        admitted_cases: 0,
        deferred_cases: 0,
        source_deferred_cases: 0,
        typescript_runs: 0,
        admitted_typescript_writes: 0,
        admitted_typescript_diagnostics: 0,
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
    `wrote ${TARGET_RELATIVE_PATH}: global_rows=${artifact.summary.global_h2_3d_rows} candidates=${artifact.summary.candidates} future_deferred=${artifact.summary.future_deferred_rows}\n`,
  );
} else if (mode === "--check") {
  requireCondition(
    fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
      fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") === rendered,
    `stale ${TARGET_RELATIVE_PATH}; run h2-3d-qualification.mjs --write and review`,
  );
  process.stdout.write(
    `H2.3d qualification is fresh: candidates=${artifact.summary.candidates} future_deferred=${artifact.summary.future_deferred_rows}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-3d-qualification.mjs [--write|--check]");
}
