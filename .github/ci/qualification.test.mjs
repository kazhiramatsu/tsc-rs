import assert from "node:assert/strict";
import test from "node:test";

import {
  classifyPaths,
  loadPolicy,
  pathsDigest,
  qualificationResultHash,
  receiptResultHash,
  sha256,
  validateFailureArtifact,
  validateLaneSelection,
  validateMergeReceipt,
  validatePolicy,
  validateQualificationResult,
  validateBoundReceipt,
} from "./qualification.mjs";

const HEAD = "1".repeat(40);
const BASE = "2".repeat(40);
const HASH = "a".repeat(64);

function clone(value) {
  return structuredClone(value);
}

test("policy and every qualification schema boundary are valid", () => {
  const policy = validatePolicy(loadPolicy());
  assert.equal(policy.status, "active");
  assert.equal(policy.aggregate_check, "gates");
  assert.equal(policy.exact_merge_qualification.authority_job, "exact_qualification");
  assert.equal(
    policy.exact_merge_qualification.node_setup_action,
    "actions/setup-node@249970729cb0ef3589644e2896645e5dc5ba9c38",
  );
  assert.equal(policy.exact_merge_qualification.m8_runner_profile, "github-ubuntu-x64-standard");
  assert.equal(policy.scheduled_stress.authority_job, "scheduled_stress");
  assert.equal(policy.approved_performance.authority_job, "qualify");
  assert.equal(policy.approved_performance.l1_authority_job, "qualify");
  assert.ok(policy.scheduled_stress.active_scope.includes("fresh-incremental-exactness"));

  const frozen = clone(policy);
  frozen.status = "frozen";
  assert.throws(() => validatePolicy(frozen), /policy header/u);
});

test("documentation-only changes select no execution lane", () => {
  const selection = classifyPaths({
    paths: ["docs/design/greenfield/lsp-and-incremental.md"],
    headSha: HEAD,
    baseSha: BASE,
    statusBlockEqual: true,
    policy: loadPolicy(),
  });
  assert.equal(selection.docs_only, true);
  assert.deepEqual(selection.selected, {
    static: false,
    host_platform: false,
    program_path: false,
    tracks: [],
  });
});

test("generated-status drift and unknown paths fail closed", () => {
  const statusDrift = classifyPaths({
    paths: ["README.md"],
    headSha: HEAD,
    baseSha: BASE,
    statusBlockEqual: false,
    policy: loadPolicy(),
  });
  assert.equal(statusDrift.docs_only, false);
  assert.deepEqual(statusDrift.selected.tracks, ["common", "h1", "l0-l1"]);
  assert.equal(statusDrift.selected.host_platform, true);
  assert.equal(statusDrift.selected.program_path, true);

  const unknown = classifyPaths({
    paths: ["new-runtime/owner.rs"],
    headSha: HEAD,
    baseSha: BASE,
    statusBlockEqual: true,
    policy: loadPolicy(),
  });
  assert.deepEqual(unknown.selected.tracks, ["common", "h1", "l0-l1"]);
  assert.equal(unknown.selected.host_platform, true);
});

test("known compiler and program owners select their focused tracks", () => {
  const compiler = classifyPaths({
    paths: ["crates/compiler/src/lib.rs"],
    headSha: HEAD,
    baseSha: BASE,
    statusBlockEqual: true,
    policy: loadPolicy(),
  });
  assert.deepEqual(compiler.selected.tracks, ["common", "h1", "l0-l1"]);
  assert.equal(compiler.selected.host_platform, false);

  const program = classifyPaths({
    paths: ["crates/program/src/lib.rs"],
    headSha: HEAD,
    baseSha: BASE,
    statusBlockEqual: true,
    policy: loadPolicy(),
  });
  assert.equal(program.selected.host_platform, true);
  assert.equal(program.selected.program_path, true);
  assert.ok(program.selected.tracks.includes("l0-l1"));
});

test("qualification authority inputs always select exact qualification tracks", () => {
  for (const changedPath of [
    ".github/workflows/ci.yml",
    ".github/workflows/l1-performance.yml",
    ".github/ci/qualification-policy.v1.json",
    ".github/ci/contracts/qualification-result.schema.json",
    "Cargo.lock",
    ".node-version",
  ]) {
    const selection = classifyPaths({
      paths: [changedPath],
      headSha: HEAD,
      baseSha: BASE,
      statusBlockEqual: true,
      policy: loadPolicy(),
    });
    assert.deepEqual(selection.selected.tracks, ["common", "h1", "l0-l1"], changedPath);
  }
});

test("lane selection rejects ambiguity, traversal, and missing lanes", () => {
  const selection = {
    schema: 1,
    kind: "lane-selection",
    head_sha: HEAD,
    base_sha: BASE,
    changed_paths: ["crates/compiler/src/lib.rs"],
    paths_sha256: pathsDigest(["crates/compiler/src/lib.rs"]),
    docs_only: false,
    selected: {
      static: true,
      host_platform: false,
      program_path: false,
      tracks: ["common", "h1", "l0-l1"],
    },
  };
  assert.equal(validateLaneSelection(selection), selection);
  const extra = clone(selection);
  extra.silent_skip = true;
  assert.throws(() => validateLaneSelection(extra), /unknown fields/u);
  const traversal = clone(selection);
  traversal.changed_paths = ["../outside"];
  traversal.paths_sha256 = pathsDigest(traversal.changed_paths);
  assert.throws(() => validateLaneSelection(traversal), /invalid repository path/u);
  const missing = clone(selection);
  missing.selected.tracks = ["h1", "l0-l1"];
  assert.throws(() => validateLaneSelection(missing), /static\/common/u);
  const programWithoutHost = clone(selection);
  programWithoutHost.selected.program_path = true;
  assert.throws(() => validateLaneSelection(programWithoutHost), /host-platform/u);
});

function validReceipt() {
  const receipt = {
    schema: 1,
    kind: "exact-merge-qualification",
    head_sha: HEAD,
    base_sha: BASE,
    inputs: {
      rust_toolchain_sha256: HASH,
      node_version_sha256: HASH,
      cargo_lock_sha256: HASH,
      vendor_inventory_sha256: HASH,
      suite_inventory_sha256: HASH,
      qualification_profile_sha256: HASH,
      lane_selection_sha256: HASH,
    },
    lanes: [{ name: "full", status: "success", result_sha256: HASH }],
    commands: [{ argv: ["cargo", "xtask", "ci"], exit_code: 0, stdout_sha256: HASH, stderr_sha256: HASH }],
    result_sha256: "0".repeat(64),
    authentication: {
      kind: "trusted-runner-oidc",
      issuer: "https://token.actions.githubusercontent.com",
      subject: "repo:kazhiramatsu/tsc-rs:ref:refs/heads/main",
      attestation_sha256: HASH,
    },
  };
  receipt.result_sha256 = receiptResultHash(receipt);
  return receipt;
}

test("exact receipt binds successful commands, immutable inputs, and authentication", () => {
  const receipt = validReceipt();
  assert.equal(validateMergeReceipt(receipt), receipt);
  const movedBase = clone(receipt);
  movedBase.base_sha = "3".repeat(40);
  assert.throws(() => validateMergeReceipt(movedBase), /digest mismatch/u);
  const failed = clone(receipt);
  failed.commands[0].exit_code = 1;
  assert.throws(() => validateMergeReceipt(failed), /command result/u);
  const unsigned = clone(receipt);
  unsigned.authentication.kind = "unsigned";
  assert.throws(() => validateMergeReceipt(unsigned), /authentication/u);
  const unknown = clone(receipt);
  unknown.inputs.extra = HASH;
  assert.throws(() => validateMergeReceipt(unknown), /input binding/u);
});

test("OIDC receipt remains bound to the exact attested qualification result", () => {
  const receipt = validReceipt();
  const result = clone(receipt);
  delete result.authentication;
  result.kind = "exact-merge-qualification-result";
  result.result_sha256 = qualificationResultHash(result);
  assert.equal(validateQualificationResult(result), result);

  const bundle = Buffer.from('{"mediaType":"application/vnd.dev.sigstore.bundle.v0.3+json"}\n');
  receipt.authentication.attestation_sha256 = sha256(bundle);
  receipt.result_sha256 = receiptResultHash(receipt);
  assert.equal(validateBoundReceipt(receipt, result, bundle, HEAD, BASE), receipt);

  const movedResult = clone(result);
  movedResult.base_sha = "3".repeat(40);
  movedResult.result_sha256 = qualificationResultHash(movedResult);
  assert.throws(
    () => validateBoundReceipt(receipt, movedResult, bundle, HEAD, BASE),
    /expected HEAD\/base/u,
  );
  assert.throws(
    () => validateBoundReceipt(receipt, result, Buffer.from("{}"), HEAD, BASE),
    /verified attestation bundle/u,
  );
});

test("failure artifacts are bounded, relative, and content addressed", () => {
  const payload = Buffer.from("bounded reproducer\n");
  const artifact = {
    schema: 1,
    kind: "failure-artifact",
    head_sha: HEAD,
    base_sha: BASE,
    track: "l0-l1",
    payload_path: "failure/reproducer.json",
    content_type: "application/json",
    bytes: payload.length,
    payload_sha256: sha256(payload),
    truncated: false,
    reproducer: { seed: "42", fixture: "ratchets/l0-fixtures.v1.json" },
  };
  assert.equal(validateFailureArtifact(artifact, payload), artifact);
  const oversized = clone(artifact);
  oversized.bytes = 10_485_761;
  assert.throws(() => validateFailureArtifact(oversized), /bound/u);
  const traversal = clone(artifact);
  traversal.payload_path = "../secrets";
  assert.throws(() => validateFailureArtifact(traversal), /payload metadata/u);
  const tampered = clone(artifact);
  tampered.payload_sha256 = HASH;
  assert.throws(() => validateFailureArtifact(tampered, payload), /binding mismatch/u);
});
