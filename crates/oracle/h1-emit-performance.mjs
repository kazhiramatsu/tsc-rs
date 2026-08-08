import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h1-emit-performance.mjs";
const EVIDENCE_RELATIVE_PATH = "ratchets/h1-emit-performance.v1.json";
const EVIDENCE_PATH = path.join(WORKSPACE, EVIDENCE_RELATIVE_PATH);
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h1-emit-performance.schema.json";
const QUALIFICATION_PATH = path.join(
  WORKSPACE,
  "ratchets/h1-emit-qualification.v1.json",
);
const EXPANSION_RELATIVE_PATH =
  "vendor/typescript-6.0.3/test-suite-expansion.v1.json";
const PROFILE_RELATIVE_PATH = "ratchets/h1-emit-profile.v1.json";
const BINARY_PATH = path.join(WORKSPACE, "target/release/tsc-rs");

const TRUSTED_H1_5_COMMIT = "6e4b4d95eb500ab4be612c94302eb08b9658a225";
const EXPECTED_RUSTC = "rustc 1.93.0 (254b59607 2026-01-19)";
const EXPECTED_NODE = "v25.2.1";
const EXPECTED_CASE_ID =
  "typescript-6.0.3/compiler/esmNoSynthesizedDefault.ts#module%3Dpreserve";
const RUNTIME_PREFIXES = [
  "crates/binder/",
  "crates/checker/",
  "crates/compiler/",
  "crates/diagnostics/",
  "crates/emitter/",
  "crates/host/",
  "crates/program/",
  "crates/syntax/",
  "crates/types/",
];
const RUNTIME_EXACT = new Set([
  "Cargo.lock",
  "Cargo.toml",
  "rust-toolchain.toml",
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

function rounded(value) {
  return Number(value.toFixed(6));
}

function median(values) {
  const sorted = [...values].sort((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0
    ? (sorted[middle - 1] + sorted[middle]) / 2
    : sorted[middle];
}

function percentile(values, percentileValue) {
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.max(0, Math.ceil((percentileValue / 100) * sorted.length) - 1)];
}

function ratio(candidate, base) {
  if (base === 0) return candidate === 0 ? 1 : Number.POSITIVE_INFINITY;
  return rounded(candidate / base);
}

function relativeRange(values) {
  const center = median(values);
  return center === 0
    ? 0
    : rounded((Math.max(...values) - Math.min(...values)) / center);
}

function command(program, args, options = {}) {
  return execFileSync(program, args, {
    cwd: WORKSPACE,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    ...options,
  }).trim();
}

function commandAt(cwd, program, args, options = {}) {
  return execFileSync(program, args, {
    cwd,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    ...options,
  }).trim();
}

function git(...args) {
  return command("git", args);
}

function pathHash(relativePath) {
  const bytes = fs.readFileSync(path.join(WORKSPACE, relativePath));
  return { path: relativePath, sha256: sha256(bytes) };
}

function exactKeys(value, required) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const actual = Object.keys(value).sort();
  const expected = [...required].sort();
  return JSON.stringify(actual) === JSON.stringify(expected);
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

function policy() {
  return {
    comparison: "same-approved-runner-alternating-h1.5-candidate",
    trusted_h1_5_commit: TRUSTED_H1_5_COMMIT,
    minimum_warm_paired_samples: 7,
    order: "alternating-ab-ba",
    ceilings: {
      warm_median_wall_ratio: 1.1,
      warm_p95_wall_ratio: 1.2,
      peak_rss_ratio: 1.1,
      executable_size_ratio: 1.1,
      absolute_peak_rss_bytes: 268_435_456,
    },
    exact_output_required: true,
    moving_hosted_runner_can_mint_or_relax: false,
  };
}

function runner() {
  if (process.platform !== "darwin" || process.arch !== "arm64") {
    fail("H1 emit evidence may only be minted on approved macOS arm64");
  }
  if (process.version !== EXPECTED_NODE || command("rustc", ["--version"]) !== EXPECTED_RUSTC) {
    fail("H1 emit evidence requires the frozen Rust and Node toolchain");
  }
  return {
    id: "macos-arm64-local-approved",
    os: process.platform,
    architecture: process.arch,
    os_release: os.release(),
    product_version: command("sw_vers", ["-productVersion"]),
    cpu: command("sysctl", ["-n", "machdep.cpu.brand_string"]),
    logical_cpus: os.cpus().length,
  };
}

function trackedRuntimePathsAt(commit = undefined) {
  const args = commit
    ? ["ls-tree", "-r", "--name-only", "-z", commit]
    : ["ls-files", "-z"];
  return execFileSync("git", args, {
    cwd: WORKSPACE,
    maxBuffer: 64 * 1024 * 1024,
  })
    .toString("utf8")
    .split("\0")
    .filter(Boolean)
    .filter(
      (entry) =>
        RUNTIME_EXACT.has(entry) ||
        RUNTIME_PREFIXES.some((prefix) => entry.startsWith(prefix)),
    )
    .sort((left, right) => left.localeCompare(right));
}

function runtimeFingerprint() {
  const paths = trackedRuntimePathsAt();
  const hash = crypto.createHash("sha256");
  for (const entry of paths) {
    hash.update(entry);
    hash.update("\0");
    hash.update(sha256(fs.readFileSync(path.join(WORKSPACE, entry))));
    hash.update("\0");
  }
  return { files: paths.length, sha256: hash.digest("hex") };
}

function runtimeFingerprintAt(commit) {
  const paths = trackedRuntimePathsAt(commit);
  const hash = crypto.createHash("sha256");
  for (const entry of paths) {
    const bytes = execFileSync("git", ["show", `${commit}:${entry}`], {
      cwd: WORKSPACE,
      maxBuffer: 64 * 1024 * 1024,
    });
    hash.update(entry);
    hash.update("\0");
    hash.update(sha256(bytes));
    hash.update("\0");
  }
  return { files: paths.length, sha256: hash.digest("hex") };
}

function qualificationProjection() {
  const artifact = JSON.parse(fs.readFileSync(QUALIFICATION_PATH, "utf8"));
  requireCondition(
    artifact.kind === "h1-emit-qualification" && artifact.status === "qualified",
    "H1 qualification is not ready for resource measurement",
  );
  requireCondition(
    artifact.compatible_cases.length === 1 &&
      artifact.compatible_cases[0].id === EXPECTED_CASE_ID,
    "H1 performance workload case changed",
  );
  const selected = artifact.compatible_cases[0];
  const expectedWrite = selected.observation.writes[0];
  const files = selected.virtual_files.map((file) => ({
    path: file.path,
    utf8_base64: file.utf8_base64,
    utf8_sha256: file.utf8_sha256,
    utf8_bytes: file.utf8_bytes,
  }));
  const sourceTreeSha256 = sha256(
    Buffer.from(
      canonical(
        files.map((file) => ({
          path: file.path,
          sha256: file.utf8_sha256,
          bytes: file.utf8_bytes,
        })),
      ),
      "utf8",
    ),
  );
  return {
    case_id: selected.id,
    source: selected.source,
    files,
    source_tree_sha256: sourceTreeSha256,
    config_utf8_base64: selected.cli_projection.config_utf8_base64,
    config_utf8_sha256: selected.cli_projection.config_utf8_sha256,
    config_utf8_bytes: selected.cli_projection.config_utf8_bytes,
    arguments: ["--pretty", "false", "-p", "tsconfig.json"],
    expected_exit_code: selected.cli_projection.expected_exit_code,
    expected_diagnostic_codes: selected.cli_projection.expected_diagnostic_codes,
    expected_stdout: expectedCliStdout(selected),
    expected_output: {
      path: expectedWrite.path,
      utf8_sha256: expectedWrite.materialized_utf8_sha256,
      utf8_bytes: expectedWrite.materialized_utf8_bytes,
    },
  };
}

function expectedCliStdout(selected) {
  return selected.observation.reported_diagnostics
    .map((diagnostic) => {
      const file = diagnostic.file.value.replace(/^\//u, "");
      return `${file}(${diagnostic.line.value + 1},${diagnostic.column.value + 1}): ${diagnostic.category} TS${diagnostic.code}: ${diagnostic.chain.text}\n`;
    })
    .join("");
}

function workloadRecord(workload) {
  return {
    case_id: workload.case_id,
    source: workload.source,
    suite_expansion: pathHash(EXPANSION_RELATIVE_PATH),
    emit_profile: pathHash(PROFILE_RELATIVE_PATH),
    source_files: workload.files.length,
    source_utf8_bytes: workload.files.reduce((sum, file) => sum + file.utf8_bytes, 0),
    source_tree_sha256: workload.source_tree_sha256,
    config_utf8_sha256: workload.config_utf8_sha256,
    config_utf8_bytes: workload.config_utf8_bytes,
    arguments: workload.arguments,
    expected_exit_code: workload.expected_exit_code,
    expected_diagnostic_codes: workload.expected_diagnostic_codes,
    expected_output: workload.expected_output,
  };
}

function materialize(workload, root) {
  fs.mkdirSync(root, { recursive: true });
  for (const file of workload.files) {
    const relative = file.path.replace(/^\//u, "");
    const output = path.join(root, ...relative.split("/"));
    fs.mkdirSync(path.dirname(output), { recursive: true });
    const bytes = Buffer.from(file.utf8_base64, "base64");
    requireCondition(
      bytes.length === file.utf8_bytes && sha256(bytes) === file.utf8_sha256,
      `H1 workload source ${file.path} is invalid`,
    );
    fs.writeFileSync(output, bytes);
  }
  const config = Buffer.from(workload.config_utf8_base64, "base64");
  requireCondition(
    config.length === workload.config_utf8_bytes &&
      sha256(config) === workload.config_utf8_sha256,
    "H1 workload config is invalid",
  );
  fs.writeFileSync(path.join(root, "tsconfig.json"), config);
}

function measureBinary(binary, workload, measurementRoot, label) {
  const root = path.join(measurementRoot, label);
  materialize(workload, root);
  const canonicalRoot = fs.realpathSync(root).split(path.sep).join("/");
  const started = process.hrtime.bigint();
  const result = spawnSync("/usr/bin/time", ["-l", binary, ...workload.arguments], {
    cwd: root,
    encoding: "utf8",
    env: {
      ...process.env,
      PATH: "/usr/bin:/bin",
      NODE_PATH: "",
      NODE_OPTIONS: "",
    },
    maxBuffer: 64 * 1024 * 1024,
  });
  const elapsed = Number(process.hrtime.bigint() - started) / 1_000_000_000;
  const rss = /^\s*(\d+)\s+maximum resident set size$/mu.exec(result.stderr);
  requireCondition(rss !== null, `cannot parse maximum RSS for ${label}:\n${result.stderr}`);
  requireCondition(
    result.status === workload.expected_exit_code,
    `${label} exit ${result.status}, expected ${workload.expected_exit_code}:\n${result.stdout}\n${result.stderr}`,
  );
  const normalizedStdout = result.stdout.replaceAll(canonicalRoot, "");
  requireCondition(
    normalizedStdout === workload.expected_stdout,
    `${label} diagnostics differ:\n${normalizedStdout}`,
  );
  const outputRelative = workload.expected_output.path.replace(/^\//u, "");
  const outputBytes = fs.readFileSync(path.join(root, outputRelative));
  requireCondition(
    outputBytes.length === workload.expected_output.utf8_bytes &&
      sha256(outputBytes) === workload.expected_output.utf8_sha256,
    `${label} output differs`,
  );
  return {
    wall_seconds: rounded(elapsed),
    max_rss_bytes: Number(rss[1]),
    exit_code: result.status,
    diagnostic_count: workload.expected_diagnostic_codes.length,
    output_files: 1,
    output_utf8_bytes: outputBytes.length,
    output_sha256: sha256(outputBytes),
  };
}

function sampleSummary(samples) {
  const warm = samples.slice(1);
  return {
    cold_wall_seconds: samples[0].wall_seconds,
    cold_max_rss_bytes: samples[0].max_rss_bytes,
    warm_median_wall_seconds: rounded(median(warm.map((sample) => sample.wall_seconds))),
    warm_p95_wall_seconds: rounded(percentile(warm.map((sample) => sample.wall_seconds), 95)),
    peak_rss_bytes: Math.max(...samples.map((sample) => sample.max_rss_bytes)),
    exit_code: samples[0].exit_code,
    diagnostic_count: samples[0].diagnostic_count,
    output_files: samples[0].output_files,
    output_utf8_bytes: samples[0].output_utf8_bytes,
    output_sha256: samples[0].output_sha256,
  };
}

function comparisonRatios(base, candidate) {
  return {
    warm_median_wall_ratio: ratio(
      candidate.warm_median_wall_seconds,
      base.warm_median_wall_seconds,
    ),
    warm_p95_wall_ratio: ratio(
      candidate.warm_p95_wall_seconds,
      base.warm_p95_wall_seconds,
    ),
    peak_rss_ratio: ratio(candidate.peak_rss_bytes, base.peak_rss_bytes),
  };
}

function variance(pairs) {
  const warm = pairs.slice(1);
  const base = warm.map((pair) => pair.base.wall_seconds);
  const candidate = warm.map((pair) => pair.candidate.wall_seconds);
  const paired = warm.map(
    (pair) => pair.candidate.wall_seconds / pair.base.wall_seconds,
  );
  return {
    base_warm_p95_over_median: ratio(percentile(base, 95), median(base)),
    candidate_warm_p95_over_median: ratio(
      percentile(candidate, 95),
      median(candidate),
    ),
    base_warm_relative_range: relativeRange(base),
    candidate_warm_relative_range: relativeRange(candidate),
    paired_wall_ratio_min: rounded(Math.min(...paired)),
    paired_wall_ratio_max: rounded(Math.max(...paired)),
  };
}

function outputIsExact(summary, workload) {
  return (
    summary.exit_code === workload.expected_exit_code &&
    summary.diagnostic_count === workload.expected_diagnostic_codes.length &&
    summary.output_files === 1 &&
    summary.output_utf8_bytes === workload.expected_output.utf8_bytes &&
    summary.output_sha256 === workload.expected_output.utf8_sha256
  );
}

function qualifies(ratios, binarySize, base, candidate, workload) {
  const ceilings = policy().ceilings;
  return (
    ratios.warm_median_wall_ratio <= ceilings.warm_median_wall_ratio &&
    ratios.warm_p95_wall_ratio <= ceilings.warm_p95_wall_ratio &&
    ratios.peak_rss_ratio <= ceilings.peak_rss_ratio &&
    binarySize.ratio <= ceilings.executable_size_ratio &&
    candidate.peak_rss_bytes <= ceilings.absolute_peak_rss_bytes &&
    outputIsExact(base, workload) &&
    outputIsExact(candidate, workload)
  );
}

function dirtyPaths() {
  return execFileSync("git", ["status", "--porcelain=v1", "-z"], {
    cwd: WORKSPACE,
    encoding: "utf8",
  })
    .split("\0")
    .filter(Boolean)
    .map((line) => line.slice(3));
}

function compare(baseRef, pairCount) {
  requireCondition(
    Number.isInteger(pairCount) &&
      pairCount >= policy().minimum_warm_paired_samples + 1,
    "comparison requires one cold pair plus at least seven warm pairs",
  );
  const dirty = dirtyPaths();
  requireCondition(
    dirty.length === 0,
    `performance comparison requires a clean candidate worktree: ${dirty.join(", ")}`,
  );
  const candidateCommit = git("rev-parse", "HEAD");
  const baseCommit = git("rev-parse", "--verify", `${baseRef}^{commit}`);
  requireCondition(
    baseCommit === TRUSTED_H1_5_COMMIT,
    `baseline must be the frozen H1.5 commit ${TRUSTED_H1_5_COMMIT}`,
  );
  requireCondition(candidateCommit !== baseCommit, "candidate and baseline must differ");
  command("git", ["merge-base", "--is-ancestor", baseCommit, candidateCommit]);
  const workload = qualificationProjection();
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "tsc-rs-h1-emit-performance-"));
  const baseWorkspace = path.join(temporary, "base");
  const baseTarget = path.join(temporary, "base-target");
  const measurementRoot = path.join(temporary, "measurements");
  const candidateBinary = path.join(temporary, "candidate-tsc-rs");
  let worktreeAdded = false;
  try {
    const buildEnvironment = {
      ...process.env,
      CARGO_BUILD_JOBS: "2",
      CARGO_INCREMENTAL: "0",
      RUSTC_WRAPPER: "",
    };
    command(
      "cargo",
      ["build", "--release", "--locked", "-p", "tsc-rs-compiler", "--bin", "tsc-rs"],
      { env: buildEnvironment },
    );
    fs.copyFileSync(BINARY_PATH, candidateBinary);
    fs.chmodSync(candidateBinary, 0o755);

    command("git", ["worktree", "add", "--detach", baseWorkspace, baseCommit]);
    worktreeAdded = true;
    commandAt(
      baseWorkspace,
      "cargo",
      ["build", "--release", "--locked", "-p", "tsc-rs-compiler", "--bin", "tsc-rs"],
      { env: { ...buildEnvironment, CARGO_TARGET_DIR: baseTarget } },
    );
    const baseBinary = path.join(baseTarget, "release/tsc-rs");
    const pairs = [];
    for (let ordinal = 0; ordinal < pairCount; ordinal += 1) {
      const order = ordinal % 2 === 0 ? "ab" : "ba";
      const observations = {};
      for (const side of order === "ab"
        ? ["base", "candidate"]
        : ["candidate", "base"]) {
        observations[side] = measureBinary(
          side === "base" ? baseBinary : candidateBinary,
          workload,
          measurementRoot,
          `${ordinal}-${order}-${side}`,
        );
      }
      pairs.push({ ordinal, order, ...observations });
    }
    const baseSummary = sampleSummary(pairs.map((pair) => pair.base));
    const candidateSummary = sampleSummary(pairs.map((pair) => pair.candidate));
    const ratios = comparisonRatios(baseSummary, candidateSummary);
    const baseBytes = fs.statSync(baseBinary).size;
    const candidateBytes = fs.statSync(candidateBinary).size;
    const binarySize = {
      base_bytes: baseBytes,
      candidate_bytes: candidateBytes,
      ratio: ratio(candidateBytes, baseBytes),
      ceiling: policy().ceilings.executable_size_ratio,
      qualified: ratio(candidateBytes, baseBytes) <= policy().ceilings.executable_size_ratio,
    };
    const qualified = qualifies(
      ratios,
      binarySize,
      baseSummary,
      candidateSummary,
      workload,
    );
    return {
      schema: 1,
      kind: "h1-emit-performance",
      status: qualified ? "qualified" : "failed",
      phase: "H1.6",
      typescript_version: "6.0.3",
      base: {
        commit: baseCommit,
        runtime_tree: runtimeFingerprintAt(baseCommit),
        executable: { sha256: sha256(fs.readFileSync(baseBinary)), bytes: baseBytes },
      },
      candidate: {
        commit: candidateCommit,
        runtime_tree: runtimeFingerprint(),
        executable: {
          sha256: sha256(fs.readFileSync(candidateBinary)),
          bytes: candidateBytes,
        },
      },
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      measured_at_utc: new Date().toISOString(),
      runner: runner(),
      toolchain: {
        rustc: command("rustc", ["--version"]),
        node: process.version,
        cargo_build_jobs: 2,
        profile: "release",
        wall_observer: "node-hrtime-bigint",
        rss_observer: "bsd-time-l",
      },
      sampling: {
        pair_count: pairCount,
        warm_pair_count: pairCount - 1,
        cold_pair_ordinal: 0,
        order: "alternating-ab-ba",
      },
      policy: policy(),
      workload: workloadRecord(workload),
      pairs,
      base_summary: baseSummary,
      candidate_summary: candidateSummary,
      variance: variance(pairs),
      ratios,
      binary_size: binarySize,
      qualified,
    };
  } finally {
    if (worktreeAdded) {
      try {
        command("git", ["worktree", "remove", "--force", baseWorkspace]);
      } catch {
        // Preserve the primary comparison failure; prune the temporary entry below.
      }
    }
    fs.rmSync(temporary, { recursive: true, force: true });
    command("git", ["worktree", "prune"]);
  }
}

function validateObservation(observation, label, workload) {
  requireCondition(
    exactKeys(observation, [
      "wall_seconds",
      "max_rss_bytes",
      "exit_code",
      "diagnostic_count",
      "output_files",
      "output_utf8_bytes",
      "output_sha256",
    ]),
    `${label} has an invalid observation shape`,
  );
  requireCondition(
    Number.isFinite(observation.wall_seconds) &&
      observation.wall_seconds > 0 &&
      Number.isInteger(observation.max_rss_bytes) &&
      observation.max_rss_bytes > 0 &&
      observation.exit_code === workload.expected_exit_code &&
      observation.diagnostic_count === workload.expected_diagnostic_codes.length &&
      observation.output_files === 1 &&
      observation.output_utf8_bytes === workload.expected_output.utf8_bytes &&
      observation.output_sha256 === workload.expected_output.utf8_sha256,
    `${label} is invalid or output-inexact`,
  );
}

function validateEvidence(evidence) {
  requireCondition(
    evidence.schema === 1 &&
      evidence.kind === "h1-emit-performance" &&
      evidence.status === "qualified" &&
      evidence.phase === "H1.6" &&
      evidence.typescript_version === "6.0.3" &&
      evidence.qualified === true,
    "invalid H1 emit performance header",
  );
  requireCondition(
    evidence.base.commit === TRUSTED_H1_5_COMMIT &&
      evidence.candidate.commit !== evidence.base.commit,
    "invalid H1 emit performance commit binding",
  );
  requireCondition(
    canonical(evidence.policy) === canonical(policy()),
    "H1 emit performance policy changed",
  );
  const workload = qualificationProjection();
  requireCondition(
    canonical(evidence.workload) === canonical(workloadRecord(workload)),
    "H1 emit workload changed",
  );
  requireCondition(
    evidence.sampling.pair_count === evidence.pairs.length &&
      evidence.sampling.warm_pair_count === evidence.pairs.length - 1 &&
      evidence.pairs.length >= policy().minimum_warm_paired_samples + 1,
    "H1 emit sample count is invalid",
  );
  for (const [index, pair] of evidence.pairs.entries()) {
    requireCondition(
      pair.ordinal === index && pair.order === (index % 2 === 0 ? "ab" : "ba"),
      `H1 emit pair ${index} order changed`,
    );
    validateObservation(pair.base, `pair ${index} base`, workload);
    validateObservation(pair.candidate, `pair ${index} candidate`, workload);
  }
  const baseSummary = sampleSummary(evidence.pairs.map((pair) => pair.base));
  const candidateSummary = sampleSummary(evidence.pairs.map((pair) => pair.candidate));
  const ratios = comparisonRatios(baseSummary, candidateSummary);
  requireCondition(
    canonical(baseSummary) === canonical(evidence.base_summary) &&
      canonical(candidateSummary) === canonical(evidence.candidate_summary) &&
      canonical(ratios) === canonical(evidence.ratios) &&
      canonical(variance(evidence.pairs)) === canonical(evidence.variance),
    "H1 emit summaries, ratios, or variance are stale",
  );
  const binarySize = {
    base_bytes: evidence.base.executable.bytes,
    candidate_bytes: evidence.candidate.executable.bytes,
    ratio: ratio(evidence.candidate.executable.bytes, evidence.base.executable.bytes),
    ceiling: policy().ceilings.executable_size_ratio,
    qualified:
      ratio(evidence.candidate.executable.bytes, evidence.base.executable.bytes) <=
      policy().ceilings.executable_size_ratio,
  };
  requireCondition(
    canonical(binarySize) === canonical(evidence.binary_size) &&
      qualifies(ratios, binarySize, baseSummary, candidateSummary, workload),
    "H1 emit performance exceeds a frozen ceiling",
  );
  requireCondition(
    canonical(evidence.generator) === canonical(pathHash(GENERATOR_RELATIVE_PATH)) &&
      canonical(evidence.contract) === canonical(pathHash(CONTRACT_RELATIVE_PATH)),
    "H1 emit generator or schema changed",
  );
}

function validateCurrentTree(evidence) {
  const head = git("rev-parse", "HEAD");
  command("git", ["merge-base", "--is-ancestor", evidence.candidate.commit, head]);
  command("git", ["merge-base", "--is-ancestor", evidence.base.commit, evidence.candidate.commit]);
  requireCondition(
    canonical(runtimeFingerprint()) === canonical(evidence.candidate.runtime_tree),
    "current runtime tree differs from the measured H1 emit candidate",
  );
  requireCondition(
    canonical(runtimeFingerprintAt(evidence.base.commit)) ===
      canonical(evidence.base.runtime_tree),
    "trusted H1.5 runtime tree differs from the evidence",
  );
}

const arguments_ = process.argv.slice(2);
if (arguments_[0] === "--compare") {
  let baseline;
  let pairs;
  for (let index = 1; index < arguments_.length; index += 1) {
    const argument = arguments_[index];
    if (argument === "--baseline") baseline = arguments_[++index];
    else if (argument === "--pairs") pairs = Number.parseInt(arguments_[++index], 10);
    else fail(`unexpected H1 emit performance argument ${argument}`);
  }
  requireCondition(typeof baseline === "string" && Number.isInteger(pairs), "missing compare arguments");
  const evidence = compare(baseline, pairs);
  validateEvidence(evidence);
  fs.writeFileSync(EVIDENCE_PATH, `${JSON.stringify(evidence, null, 2)}\n`);
  process.stdout.write(
    `wrote ${EVIDENCE_RELATIVE_PATH}: status=${evidence.status} median=${evidence.ratios.warm_median_wall_ratio} p95=${evidence.ratios.warm_p95_wall_ratio} rss=${evidence.ratios.peak_rss_ratio}\n`,
  );
} else if (arguments_[0] === "--check" && arguments_.length === 1) {
  requireCondition(fs.existsSync(EVIDENCE_PATH), `missing ${EVIDENCE_RELATIVE_PATH}`);
  const evidence = JSON.parse(fs.readFileSync(EVIDENCE_PATH, "utf8"));
  validateEvidence(evidence);
  validateCurrentTree(evidence);
  process.stdout.write(
    `H1 emit performance is qualified: median=${evidence.ratios.warm_median_wall_ratio} p95=${evidence.ratios.warm_p95_wall_ratio} rss=${evidence.ratios.peak_rss_ratio} bytes=${evidence.candidate_summary.output_utf8_bytes}\n`,
  );
} else {
  fail(
    "usage: h1-emit-performance.mjs --compare --baseline <H1.5 commit> --pairs <n> | --check",
  );
}
