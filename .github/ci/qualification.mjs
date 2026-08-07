import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

const workspace = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const policyPath = path.join(workspace, ".github/ci/qualification-policy.v1.json");
const contractDirectory = path.join(workspace, ".github/ci/contracts");

export function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function exactKeys(value, required, optional = []) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const allowed = new Set([...required, ...optional]);
  return required.every((key) => Object.hasOwn(value, key)) && Object.keys(value).every((key) => allowed.has(key));
}

function isSha1(value) {
  return typeof value === "string" && /^[0-9a-f]{40}$/u.test(value);
}

function isSha256(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}

function isRelativeRepositoryPath(value) {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= 4096 &&
    !path.posix.isAbsolute(value) &&
    !value.split("/").includes("..") &&
    !value.includes("\\")
  );
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function sortedUnique(values) {
  return [...new Set(values)].sort((left, right) => left.localeCompare(right));
}

export function pathsDigest(paths) {
  return sha256(Buffer.from(paths.join("\0"), "utf8"));
}

export function validateLaneSelection(selection, limit = 4096) {
  const required = [
    "schema",
    "kind",
    "head_sha",
    "base_sha",
    "paths_sha256",
    "changed_paths",
    "docs_only",
    "selected",
  ];
  if (!exactKeys(selection, required)) throw new Error("lane selection has missing or unknown fields");
  if (selection.schema !== 1 || selection.kind !== "lane-selection") throw new Error("invalid lane selection discriminator");
  if (!isSha1(selection.head_sha) || !isSha1(selection.base_sha)) throw new Error("invalid lane selection commit");
  if (!Array.isArray(selection.changed_paths) || selection.changed_paths.length > limit) {
    throw new Error("lane selection path list exceeds its bound");
  }
  if (selection.changed_paths.some((entry) => !isRelativeRepositoryPath(entry))) {
    throw new Error("lane selection contains an invalid repository path");
  }
  if (JSON.stringify(selection.changed_paths) !== JSON.stringify(sortedUnique(selection.changed_paths))) {
    throw new Error("lane selection paths must be sorted and unique");
  }
  if (!isSha256(selection.paths_sha256) || selection.paths_sha256 !== pathsDigest(selection.changed_paths)) {
    throw new Error("lane selection path digest mismatch");
  }
  if (typeof selection.docs_only !== "boolean") throw new Error("lane selection docs_only is not boolean");
  if (!exactKeys(selection.selected, ["static", "host_platform", "program_path", "tracks"])) {
    throw new Error("lane selection selected object has missing or unknown fields");
  }
  for (const key of ["static", "host_platform", "program_path"]) {
    if (typeof selection.selected[key] !== "boolean") throw new Error(`lane selection ${key} is not boolean`);
  }
  const tracks = selection.selected.tracks;
  if (!Array.isArray(tracks) || JSON.stringify(tracks) !== JSON.stringify(sortedUnique(tracks))) {
    throw new Error("lane selection tracks must be sorted and unique");
  }
  if (tracks.some((track) => !["common", "h1", "l0-l1"].includes(track))) {
    throw new Error("lane selection contains an unknown track");
  }
  if (selection.docs_only) {
    if (selection.changed_paths.length === 0 || selection.selected.static || selection.selected.host_platform || selection.selected.program_path || tracks.length > 0) {
      throw new Error("documentation-only selection must select no execution lane");
    }
  } else if (!selection.selected.static || !tracks.includes("common")) {
    throw new Error("non-documentation selection must include static/common validation");
  }
  if (selection.selected.program_path && !selection.selected.host_platform) {
    throw new Error("program-path selection requires host-platform validation");
  }
  return selection;
}

export function classifyPaths({ paths, headSha, baseSha, statusBlockEqual, policy }) {
  const changedPaths = sortedUnique(paths);
  if (changedPaths.length > policy.limits.changed_paths) throw new Error("changed-path inventory exceeds policy bound");
  const docsOnly =
    changedPaths.length > 0 &&
    changedPaths.every((entry) => entry.endsWith(".md")) &&
    statusBlockEqual;
  let hostPlatform = false;
  let programPath = false;
  const tracks = new Set();
  if (!docsOnly) {
    tracks.add("common");
    for (const changedPath of changedPaths) {
      const hostMatch = policy.classification.host_platform_prefixes.some((prefix) => changedPath.startsWith(prefix));
      const programMatch = policy.classification.program_path_prefixes.some((prefix) => changedPath.startsWith(prefix));
      const commonMatch =
        policy.classification.common_exact.includes(changedPath) ||
        policy.classification.common_prefixes.some((prefix) => changedPath.startsWith(prefix));
      hostPlatform ||= hostMatch;
      programPath ||= programMatch;
      let known = hostMatch || programMatch || commonMatch;
      if (commonMatch) {
        tracks.add("l0-l1");
        tracks.add("h1");
      }
      for (const [track, prefixes] of Object.entries(policy.classification.track_prefixes)) {
        if (prefixes.some((prefix) => changedPath.startsWith(prefix))) {
          tracks.add(track);
          known = true;
        }
      }
      if (!known) {
        tracks.add("l0-l1");
        tracks.add("h1");
        hostPlatform = true;
        programPath = true;
      }
    }
  }
  return validateLaneSelection(
    {
      schema: 1,
      kind: "lane-selection",
      head_sha: headSha,
      base_sha: baseSha,
      paths_sha256: pathsDigest(changedPaths),
      changed_paths: changedPaths,
      docs_only: docsOnly,
      selected: {
        static: !docsOnly,
        host_platform: hostPlatform,
        program_path: programPath,
        tracks: sortedUnique([...tracks]),
      },
    },
    policy.limits.changed_paths,
  );
}

export function receiptResultHash(receipt) {
  const semantic = { ...receipt };
  delete semantic.result_sha256;
  delete semantic.authentication;
  return sha256(canonical(semantic));
}

export function qualificationResultHash(result) {
  const semantic = { ...result };
  delete semantic.result_sha256;
  return sha256(canonical(semantic));
}

export function validateQualificationResult(result) {
  const required = ["schema", "kind", "head_sha", "base_sha", "inputs", "lanes", "commands", "result_sha256"];
  if (!exactKeys(result, required)) throw new Error("qualification result has missing or unknown fields");
  const receiptShape = {
    ...result,
    kind: "exact-merge-qualification",
    authentication: {
      kind: "trusted-runner-oidc",
      issuer: "pending",
      subject: "pending",
      attestation_sha256: "0".repeat(64),
    },
  };
  receiptShape.result_sha256 = receiptResultHash(receiptShape);
  validateMergeReceipt(receiptShape);
  if (result.schema !== 1 || result.kind !== "exact-merge-qualification-result") {
    throw new Error("invalid qualification result discriminator");
  }
  if (result.result_sha256 !== qualificationResultHash(result)) {
    throw new Error("qualification result digest mismatch");
  }
  return result;
}

export function validateMergeReceipt(receipt) {
  const required = ["schema", "kind", "head_sha", "base_sha", "inputs", "lanes", "commands", "result_sha256", "authentication"];
  if (!exactKeys(receipt, required)) throw new Error("merge receipt has missing or unknown fields");
  if (receipt.schema !== 1 || receipt.kind !== "exact-merge-qualification") throw new Error("invalid merge receipt discriminator");
  if (!isSha1(receipt.head_sha) || !isSha1(receipt.base_sha) || receipt.head_sha === receipt.base_sha) {
    throw new Error("merge receipt does not bind distinct exact commits");
  }
  const inputKeys = [
    "rust_toolchain_sha256",
    "node_version_sha256",
    "cargo_lock_sha256",
    "vendor_inventory_sha256",
    "suite_inventory_sha256",
    "qualification_profile_sha256",
    "lane_selection_sha256",
  ];
  if (!exactKeys(receipt.inputs, inputKeys) || inputKeys.some((key) => !isSha256(receipt.inputs[key]))) {
    throw new Error("merge receipt input binding is incomplete");
  }
  if (!Array.isArray(receipt.lanes) || receipt.lanes.length === 0) throw new Error("merge receipt has no lanes");
  for (const lane of receipt.lanes) {
    if (!exactKeys(lane, ["name", "status", "result_sha256"]) || typeof lane.name !== "string" || lane.name.length === 0 || lane.status !== "success" || !isSha256(lane.result_sha256)) {
      throw new Error("merge receipt contains an invalid lane result");
    }
  }
  if (new Set(receipt.lanes.map((lane) => lane.name)).size !== receipt.lanes.length) throw new Error("merge receipt repeats a lane");
  if (!Array.isArray(receipt.commands) || receipt.commands.length === 0) throw new Error("merge receipt has no commands");
  for (const command of receipt.commands) {
    if (!exactKeys(command, ["argv", "exit_code", "stdout_sha256", "stderr_sha256"]) || !Array.isArray(command.argv) || command.argv.length === 0 || command.argv.some((arg) => typeof arg !== "string") || command.exit_code !== 0 || !isSha256(command.stdout_sha256) || !isSha256(command.stderr_sha256)) {
      throw new Error("merge receipt contains an invalid command result");
    }
  }
  if (!isSha256(receipt.result_sha256) || receipt.result_sha256 !== receiptResultHash(receipt)) {
    throw new Error("merge receipt result digest mismatch");
  }
  const authentication = receipt.authentication;
  if (!exactKeys(authentication, ["kind", "issuer", "subject", "attestation_sha256"]) || !["trusted-runner-oidc", "registered-signer"].includes(authentication.kind) || typeof authentication.issuer !== "string" || authentication.issuer.length === 0 || typeof authentication.subject !== "string" || authentication.subject.length === 0 || !isSha256(authentication.attestation_sha256)) {
    throw new Error("merge receipt lacks accepted authentication");
  }
  return receipt;
}

export function validateFailureArtifact(artifact, payload = undefined, limit = 10_485_760) {
  const required = ["schema", "kind", "head_sha", "base_sha", "track", "payload_path", "content_type", "bytes", "payload_sha256", "truncated"];
  if (!exactKeys(artifact, required, ["reproducer"])) throw new Error("failure artifact has missing or unknown fields");
  if (artifact.schema !== 1 || artifact.kind !== "failure-artifact" || !isSha1(artifact.head_sha) || !isSha1(artifact.base_sha)) {
    throw new Error("invalid failure artifact discriminator or commit binding");
  }
  if (!["common", "l0-l1", "h1", "host-platform", "stress", "performance"].includes(artifact.track)) throw new Error("unknown failure artifact track");
  if (!isRelativeRepositoryPath(artifact.payload_path) || typeof artifact.content_type !== "string" || artifact.content_type.length === 0 || artifact.content_type.length > 128) throw new Error("invalid failure artifact payload metadata");
  if (!Number.isInteger(artifact.bytes) || artifact.bytes < 0 || artifact.bytes > limit || !isSha256(artifact.payload_sha256) || typeof artifact.truncated !== "boolean") throw new Error("failure artifact exceeds its bound or has an invalid digest");
  if (payload !== undefined) {
    const bytes = Buffer.isBuffer(payload) ? payload : Buffer.from(payload);
    if (bytes.length !== artifact.bytes || sha256(bytes) !== artifact.payload_sha256) throw new Error("failure artifact payload binding mismatch");
  }
  if (artifact.reproducer !== undefined) {
    if (!exactKeys(artifact.reproducer, [], ["seed", "fixture", "initial_text_sha256", "options_key_sha256"])) throw new Error("failure artifact reproducer has unknown fields");
    if (artifact.reproducer.fixture !== undefined && !isRelativeRepositoryPath(artifact.reproducer.fixture)) throw new Error("failure artifact reproducer fixture is invalid");
    for (const key of ["initial_text_sha256", "options_key_sha256"]) {
      if (artifact.reproducer[key] !== undefined && !isSha256(artifact.reproducer[key])) throw new Error(`failure artifact ${key} is invalid`);
    }
  }
  return artifact;
}

export function loadPolicy() {
  return JSON.parse(fs.readFileSync(policyPath, "utf8"));
}

export function validatePolicy(policy) {
  if (policy.schema !== 1 || policy.status !== "active" || policy.aggregate_check !== "gates") throw new Error("invalid qualification policy header");
  if (policy.limits.changed_paths !== 4096 || policy.limits.failure_artifact_bytes !== 10_485_760) throw new Error("qualification bounds drifted");
  if (policy.classification.unknown_non_documentation !== "select-all") throw new Error("classification must fail closed");
  const exact = policy.exact_merge_qualification;
  if (exact.authority_workflow !== ".github/workflows/ci.yml" || exact.authority_job !== "exact_qualification" || exact.result_producer !== ".github/ci/qualification.mjs produce-result" || exact.result_contract !== ".github/ci/contracts/qualification-result.schema.json" || exact.receipt_contract !== ".github/ci/contracts/merge-receipt.schema.json" || exact.attestation_action !== "actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6" || exact.node_setup_action !== "actions/setup-node@249970729cb0ef3589644e2896645e5dc5ba9c38" || exact.m8_runner_profile !== "github-ubuntu-x64-standard") throw new Error("invalid exact qualification authority policy");
  if (policy.exact_merge_qualification.unsigned_receipts_qualify !== false) throw new Error("unsigned merge receipts must not qualify");
  if (policy.scheduled_stress.authority_workflow !== ".github/workflows/ci.yml" || policy.scheduled_stress.authority_job !== "scheduled_stress" || policy.scheduled_stress.event !== "schedule" || !policy.scheduled_stress.active_scope.includes("randomized-edits")) throw new Error("invalid scheduled stress authority policy");
  const workflow = fs.readFileSync(path.join(workspace, exact.authority_workflow), "utf8");
  const exactMarker = `\n  ${exact.authority_job}:\n`;
  const nextMarker = `\n  ${policy.scheduled_stress.authority_job}:\n`;
  const exactStart = workflow.indexOf(exactMarker);
  const exactEnd = workflow.indexOf(nextMarker, exactStart + exactMarker.length);
  if (exactStart < 0 || exactEnd < 0) throw new Error("exact qualification workflow job is missing");
  const exactJob = workflow.slice(exactStart, exactEnd);
  const setupIndex = exactJob.indexOf(`uses: ${exact.node_setup_action}`);
  const versionFileIndex = exactJob.indexOf("node-version-file: .node-version");
  const runnerProfileIndex = exactJob.indexOf(`TSRS_M8_RUNNER_PROFILE: ${exact.m8_runner_profile}`);
  const gateIndex = exactJob.indexOf("name: Run the unsplit full gate at the exact base");
  if (setupIndex < 0 || versionFileIndex < setupIndex || runnerProfileIndex < 0 || gateIndex < versionFileIndex) {
    throw new Error("exact qualification must pin its Node and M8 runner profiles before the full gate");
  }
  if (
    policy.approved_performance.authority_workflow !== ".github/workflows/l0-performance.yml" ||
    policy.approved_performance.authority_job !== "qualify" ||
    policy.approved_performance.environment !== "approved-performance" ||
    policy.approved_performance.evidence !==
      "ratchets/l0-one-shot-registry-performance.v1.json" ||
    policy.approved_performance.l1_authority_workflow !==
      ".github/workflows/l1-performance.yml" ||
    policy.approved_performance.l1_authority_job !== "qualify" ||
    policy.approved_performance.l1_evidence !==
      "ratchets/l1-incremental-parser-performance.v1.json"
  )
    throw new Error("invalid performance authority binding");
  if (!policy.approved_performance.alternating_baseline_candidate || policy.approved_performance.moving_hosted_images_may_mint_ratchets) throw new Error("invalid performance authority policy");
  for (const contract of ["lane-selection", "qualification-result", "merge-receipt", "failure-artifact"]) {
    const schema = JSON.parse(fs.readFileSync(path.join(contractDirectory, `${contract}.schema.json`), "utf8"));
    if (schema.$schema !== "https://json-schema.org/draft/2020-12/schema" || schema.additionalProperties !== false || !schema.$id.endsWith(`/${contract}.schema.json`)) {
      throw new Error(`invalid ${contract} JSON schema boundary`);
    }
  }
  return policy;
}

function fileSha256(filePath) {
  return sha256(fs.readFileSync(path.join(workspace, filePath)));
}

function trackedTreeDigest(prefixes, exact = []) {
  const exactSet = new Set(exact);
  const paths = execFileSync("git", ["ls-files", "-z"], {
    cwd: workspace,
    maxBuffer: 64 * 1024 * 1024,
  })
    .toString("utf8")
    .split("\0")
    .filter(Boolean)
    .filter((entry) => exactSet.has(entry) || prefixes.some((prefix) => entry.startsWith(prefix)))
    .sort((left, right) => left.localeCompare(right));
  const hash = crypto.createHash("sha256");
  for (const entry of paths) {
    hash.update(entry);
    hash.update("\0");
    hash.update(fileSha256(entry));
    hash.update("\0");
  }
  return hash.digest("hex");
}

function qualificationInputs(selection) {
  return {
    rust_toolchain_sha256: fileSha256("rust-toolchain.toml"),
    node_version_sha256: fileSha256(".node-version"),
    cargo_lock_sha256: fileSha256("Cargo.lock"),
    vendor_inventory_sha256: trackedTreeDigest(["vendor/"]),
    suite_inventory_sha256: trackedTreeDigest([
      "baselines/",
      "tests/",
      "ratchets/",
      "crates/oracle/",
      "crates/conformance/tests/",
      "crates/harness/tests/",
    ]),
    qualification_profile_sha256: trackedTreeDigest(
      [".github/ci/"],
      [
        ".github/workflows/ci.yml",
        ".github/workflows/l0-performance.yml",
        ".github/workflows/l1-performance.yml",
      ],
    ),
    lane_selection_sha256: sha256(canonical(selection)),
  };
}

function produceQualificationResult({ baseSha, headSha, selectionPath, stdoutPath, stderrPath }) {
  const exactBase = git("rev-parse", "--verify", `${baseSha}^{commit}`);
  const exactHead = git("rev-parse", "--verify", `${headSha}^{commit}`);
  if (git("rev-parse", "HEAD") !== exactHead || exactBase === exactHead) {
    throw new Error("qualification result is not running at the declared distinct HEAD/base");
  }
  if (git("status", "--porcelain").length !== 0) {
    throw new Error("qualification result refuses a dirty candidate worktree");
  }
  const selection = validateLaneSelection(JSON.parse(fs.readFileSync(selectionPath, "utf8")), loadPolicy().limits.changed_paths);
  if (selection.head_sha !== exactHead || selection.base_sha !== exactBase || selection.docs_only) {
    throw new Error("qualification selection does not bind this executable HEAD/base");
  }
  const command = {
    argv: ["cargo", "xtask", "ci", "--baseline", exactBase],
    exit_code: 0,
    stdout_sha256: sha256(fs.readFileSync(stdoutPath)),
    stderr_sha256: sha256(fs.readFileSync(stderrPath)),
  };
  const result = {
    schema: 1,
    kind: "exact-merge-qualification-result",
    head_sha: exactHead,
    base_sha: exactBase,
    inputs: qualificationInputs(selection),
    lanes: [{ name: "full", status: "success", result_sha256: sha256(canonical(command)) }],
    commands: [command],
    result_sha256: "0".repeat(64),
  };
  result.result_sha256 = qualificationResultHash(result);
  return validateQualificationResult(result);
}

function finalizeReceipt(result, bundle, issuer, subject) {
  validateQualificationResult(result);
  if (!issuer || !subject) throw new Error("receipt finalization requires an OIDC issuer and subject");
  JSON.parse(bundle.toString("utf8"));
  const receipt = {
    ...result,
    kind: "exact-merge-qualification",
    authentication: {
      kind: "trusted-runner-oidc",
      issuer,
      subject,
      attestation_sha256: sha256(bundle),
    },
  };
  receipt.result_sha256 = receiptResultHash(receipt);
  return validateMergeReceipt(receipt);
}

export function validateBoundReceipt(receipt, result, bundle, headSha, baseSha) {
  validateMergeReceipt(receipt);
  validateQualificationResult(result);
  if (receipt.head_sha !== headSha || receipt.base_sha !== baseSha || result.head_sha !== headSha || result.base_sha !== baseSha) {
    throw new Error("authenticated receipt does not bind the expected HEAD/base");
  }
  for (const key of ["inputs", "lanes", "commands"]) {
    if (canonical(receipt[key]) !== canonical(result[key])) {
      throw new Error(`authenticated receipt ${key} differ from the attested result`);
    }
  }
  if (receipt.authentication.attestation_sha256 !== sha256(bundle)) {
    throw new Error("authenticated receipt does not bind the verified attestation bundle");
  }
  return receipt;
}

function writeJson(target, value) {
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, `${JSON.stringify(value, null, 2)}\n`);
}

function produceFailureArtifact({ headSha, baseSha, track, sourcePath, payloadPath, contentType, seed, fixture, initialTextSha256 }) {
  const policy = validatePolicy(loadPolicy());
  const exactHead = git("rev-parse", "--verify", `${headSha}^{commit}`);
  const exactBase = git("rev-parse", "--verify", `${baseSha}^{commit}`);
  if (exactHead === exactBase || !isRelativeRepositoryPath(payloadPath)) {
    throw new Error("failure artifact requires distinct commits and a repository-relative payload path");
  }
  const source = fs.readFileSync(sourcePath);
  const limit = policy.limits.failure_artifact_bytes;
  let payload = source;
  let truncated = false;
  if (source.length > limit) {
    const marker = Buffer.from("\n...[failure payload truncated to policy bound]...\n", "utf8");
    const side = Math.floor((limit - marker.length) / 2);
    payload = Buffer.concat([source.subarray(0, side), marker, source.subarray(source.length - side)]);
    truncated = true;
  }
  const absolutePayload = path.resolve(workspace, payloadPath);
  if (absolutePayload !== workspace && !absolutePayload.startsWith(`${workspace}${path.sep}`)) {
    throw new Error("failure payload escapes the repository workspace");
  }
  fs.mkdirSync(path.dirname(absolutePayload), { recursive: true });
  fs.writeFileSync(absolutePayload, payload);
  const reproducer = {};
  if (seed) reproducer.seed = seed;
  if (fixture) reproducer.fixture = fixture;
  if (initialTextSha256) reproducer.initial_text_sha256 = initialTextSha256;
  const artifact = {
    schema: 1,
    kind: "failure-artifact",
    head_sha: exactHead,
    base_sha: exactBase,
    track,
    payload_path: payloadPath,
    content_type: contentType,
    bytes: payload.length,
    payload_sha256: sha256(payload),
    truncated,
    ...(Object.keys(reproducer).length > 0 ? { reproducer } : {}),
  };
  return validateFailureArtifact(artifact, payload, limit);
}

function git(...args) {
  return execFileSync("git", args, { cwd: workspace, encoding: "utf8", maxBuffer: 16 * 1024 * 1024 }).trim();
}

function statusBlock(commit) {
  const readme = execFileSync("git", ["show", `${commit}:README.md`], { cwd: workspace, encoding: "utf8" });
  const begins = [...readme.matchAll(/<!-- STATUS:BEGIN /gu)];
  const ends = [...readme.matchAll(/<!-- STATUS:END -->/gu)];
  if (begins.length !== 1 || ends.length !== 1 || begins[0].index >= ends[0].index) throw new Error("invalid README status block");
  return readme.slice(begins[0].index, ends[0].index + ends[0][0].length);
}

function changedPaths(baseSha, headSha) {
  const output = execFileSync("git", ["diff", "--name-only", "--no-renames", "-z", baseSha, headSha], {
    cwd: workspace,
    maxBuffer: 16 * 1024 * 1024,
  });
  return output.toString("utf8").split("\0").filter(Boolean);
}

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function writeGithubOutput(selection, target) {
  const tracks = new Set(selection.selected.tracks);
  const lines = [
    `head_sha=${selection.head_sha}`,
    `base_sha=${selection.base_sha}`,
    `docs_only=${selection.docs_only}`,
    `static=${selection.selected.static}`,
    `host_platform=${selection.selected.host_platform}`,
    `program_path=${selection.selected.program_path}`,
    `l0_l1=${tracks.has("l0-l1")}`,
    `h1=${tracks.has("h1")}`,
    `selection_sha256=${sha256(canonical(selection))}`,
  ];
  fs.appendFileSync(target, `${lines.join("\n")}\n`);
}

function main() {
  const command = process.argv[2];
  if (command === "check") {
    validatePolicy(loadPolicy());
    process.stdout.write("CI qualification policy and schemas are valid\n");
    return;
  }
  if (command === "verify-selection") {
    const input = argument("--path");
    if (!input) throw new Error("verify-selection requires --path");
    validateLaneSelection(JSON.parse(fs.readFileSync(input, "utf8")), loadPolicy().limits.changed_paths);
    process.stdout.write("lane selection is valid\n");
    return;
  }
  if (command === "verify-receipt") {
    const input = argument("--path");
    if (!input) throw new Error("verify-receipt requires --path");
    validateMergeReceipt(JSON.parse(fs.readFileSync(input, "utf8")));
    process.stdout.write("authenticated exact merge receipt is valid\n");
    return;
  }
  if (command === "produce-result") {
    const baseSha = argument("--base");
    const headSha = argument("--head");
    const selectionPath = argument("--selection");
    const stdoutPath = argument("--stdout");
    const stderrPath = argument("--stderr");
    const output = argument("--out");
    if (!baseSha || !headSha || !selectionPath || !stdoutPath || !stderrPath || !output) {
      throw new Error("produce-result requires --base, --head, --selection, --stdout, --stderr, and --out");
    }
    const result = produceQualificationResult({ baseSha, headSha, selectionPath, stdoutPath, stderrPath });
    writeJson(path.resolve(output), result);
    process.stdout.write(`wrote exact qualification result ${output}\n`);
    return;
  }
  if (command === "finalize-receipt") {
    const resultPath = argument("--result");
    const bundlePath = argument("--bundle");
    const issuer = argument("--issuer");
    const subject = argument("--subject");
    const output = argument("--out");
    if (!resultPath || !bundlePath || !issuer || !subject || !output) {
      throw new Error("finalize-receipt requires --result, --bundle, --issuer, --subject, and --out");
    }
    const receipt = finalizeReceipt(
      JSON.parse(fs.readFileSync(resultPath, "utf8")),
      fs.readFileSync(bundlePath),
      issuer,
      subject,
    );
    writeJson(path.resolve(output), receipt);
    process.stdout.write(`wrote authenticated exact merge receipt ${output}\n`);
    return;
  }
  if (command === "verify-bound-receipt") {
    const receiptPath = argument("--receipt");
    const resultPath = argument("--result");
    const bundlePath = argument("--bundle");
    const headSha = argument("--head");
    const baseSha = argument("--base");
    if (!receiptPath || !resultPath || !bundlePath || !headSha || !baseSha) {
      throw new Error("verify-bound-receipt requires --receipt, --result, --bundle, --head, and --base");
    }
    validateBoundReceipt(
      JSON.parse(fs.readFileSync(receiptPath, "utf8")),
      JSON.parse(fs.readFileSync(resultPath, "utf8")),
      fs.readFileSync(bundlePath),
      headSha,
      baseSha,
    );
    process.stdout.write("authenticated receipt is bound to the verified result and exact HEAD/base\n");
    return;
  }
  if (command === "verify-failure") {
    const input = argument("--path");
    const payloadPath = argument("--payload");
    if (!input || !payloadPath) throw new Error("verify-failure requires --path and --payload");
    validateFailureArtifact(
      JSON.parse(fs.readFileSync(input, "utf8")),
      fs.readFileSync(payloadPath),
      loadPolicy().limits.failure_artifact_bytes,
    );
    process.stdout.write("bounded failure artifact is valid\n");
    return;
  }
  if (command === "write-failure") {
    const headSha = argument("--head");
    const baseSha = argument("--base");
    const track = argument("--track");
    const sourcePath = argument("--source");
    const payloadPath = argument("--payload-path");
    const contentType = argument("--content-type") ?? "text/plain";
    const output = argument("--out");
    if (!headSha || !baseSha || !track || !sourcePath || !payloadPath || !output) {
      throw new Error("write-failure requires --head, --base, --track, --source, --payload-path, and --out");
    }
    const artifact = produceFailureArtifact({
      headSha,
      baseSha,
      track,
      sourcePath,
      payloadPath,
      contentType,
      seed: argument("--seed"),
      fixture: argument("--fixture"),
      initialTextSha256: argument("--initial-text-sha256"),
    });
    writeJson(path.resolve(output), artifact);
    process.stdout.write(`wrote bounded failure artifact ${output}\n`);
    return;
  }
  if (command === "classify") {
    const policy = validatePolicy(loadPolicy());
    const baseRef = argument("--base");
    const headRef = argument("--head");
    if (!baseRef || !headRef) throw new Error("classify requires --base and --head");
    const baseSha = git("rev-parse", "--verify", `${baseRef}^{commit}`);
    const headSha = git("rev-parse", "--verify", `${headRef}^{commit}`);
    let equal = false;
    try {
      equal = statusBlock(baseSha) === statusBlock(headSha);
    } catch {
      equal = false;
    }
    const selection = classifyPaths({
      paths: changedPaths(baseSha, headSha),
      headSha,
      baseSha,
      statusBlockEqual: equal,
      policy,
    });
    const rendered = `${JSON.stringify(selection, null, 2)}\n`;
    const output = argument("--out");
    const githubOutput = argument("--github-output");
    if (output) fs.writeFileSync(output, rendered);
    if (githubOutput) writeGithubOutput(selection, githubOutput);
    process.stdout.write(rendered);
    return;
  }
  throw new Error("usage: qualification.mjs check|classify|produce-result|finalize-receipt|verify-bound-receipt|write-failure|verify-selection|verify-receipt|verify-failure ...");
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`${error.stack ?? error}\n`);
    process.exitCode = 1;
  }
}
