import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const workspace = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const driverPath = fileURLToPath(import.meta.url);
const evidencePath = path.join(workspace, "ratchets/l1-incremental-parser-performance.v1.json");
const l0EvidencePath = path.join(workspace, "ratchets/l0-one-shot-registry-performance.v1.json");
const fixtureManifestPath = path.join(workspace, "ratchets/l0-fixtures.v1.json");
const fixtureRoot = path.join(workspace, "target/l0/qualification-fixtures");
const fixturePath = path.join(fixtureRoot, "large-edit/large-edit.ts");
const binaryPath = path.join(workspace, "target/release/examples/l1_incremental_qualification");
const frozenQualifiedDriverSha256 =
  "0ca44ef5efb8d7e587db31474eda0cb4127553982730b407169cca5c439182e5";
const runtimePrefixes = [
  "crates/binder/",
  "crates/checker/",
  "crates/compiler/",
  "crates/diagnostics/",
  "crates/host/",
  "crates/program/",
  "crates/syntax/",
  "crates/types/",
  "vendor/typescript-6.0.3/lib/",
];
const runtimeExact = new Set(["Cargo.lock", "Cargo.toml", "rust-toolchain.toml", ".node-version"]);

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function command(program, args, options = {}) {
  return execFileSync(program, args, {
    cwd: workspace,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    ...options,
  }).trim();
}

function git(...args) {
  return command("git", args);
}

function runtimePathsAt(commit = undefined) {
  const args = commit
    ? ["ls-tree", "-r", "--name-only", "-z", commit]
    : ["ls-files", "-z"];
  return execFileSync("git", args, {
    cwd: workspace,
    maxBuffer: 64 * 1024 * 1024,
  })
    .toString("utf8")
    .split("\0")
    .filter(Boolean)
    .filter(
      (entry) =>
        runtimeExact.has(entry) || runtimePrefixes.some((prefix) => entry.startsWith(prefix)),
    )
    .sort((left, right) => left.localeCompare(right));
}

function trackedRuntimeFingerprint() {
  const paths = runtimePathsAt();
  const hash = crypto.createHash("sha256");
  for (const entry of paths) {
    hash.update(entry);
    hash.update("\0");
    hash.update(sha256(fs.readFileSync(path.join(workspace, entry))));
    hash.update("\0");
  }
  return { files: paths.length, sha256: hash.digest("hex") };
}

function trackedRuntimeFingerprintAt(commit) {
  const paths = runtimePathsAt(commit);
  const hash = crypto.createHash("sha256");
  for (const entry of paths) {
    const bytes = execFileSync("git", ["show", `${commit}:${entry}`], {
      cwd: workspace,
      maxBuffer: 64 * 1024 * 1024,
    });
    hash.update(entry);
    hash.update("\0");
    hash.update(sha256(bytes));
    hash.update("\0");
  }
  return { files: paths.length, sha256: hash.digest("hex") };
}

function sameJson(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function median(values) {
  const sorted = [...values].sort((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0 ? (sorted[middle - 1] + sorted[middle]) / 2 : sorted[middle];
}

function percentile(values, percentileValue) {
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.max(0, Math.ceil((percentileValue / 100) * sorted.length) - 1)];
}

function rounded(value) {
  return Number(value.toFixed(9));
}

function policy() {
  return {
    comparison: "same-candidate-fresh-incremental-alternating",
    base_operation: "fresh",
    candidate_operation: "incremental",
    minimum_paired_samples: 7,
    order: "alternating-ab-ba",
    ceilings: {
      warm_median_operation_ratio: 0.9,
      warm_p95_operation_ratio: 1.0,
      peak_rss_ratio: 1.1,
      allocation_count_ratio: 0.9,
      allocated_bytes_ratio: 1.15,
      incremental_warm_median_operation_seconds: 0.05,
      incremental_warm_p95_operation_seconds: 0.075,
      incremental_peak_rss_bytes: 134217728,
      incremental_minimum_reused_nodes: 190000,
      incremental_maximum_freshly_parsed_nodes: 128,
    },
    moving_hosted_runner_can_mint_or_relax: false,
  };
}

function runner() {
  if (process.platform !== "darwin" || process.arch !== "arm64") {
    throw new Error("L1 performance evidence may only be minted on the approved macOS arm64 profile");
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

function largeEditWorkload() {
  const manifest = JSON.parse(fs.readFileSync(fixtureManifestPath, "utf8"));
  const workload = manifest.workloads.find((entry) => entry.id === "large-edit");
  if (
    manifest.schema !== 1 ||
    manifest.status !== "frozen" ||
    !workload ||
    workload.files?.length !== 1 ||
    workload.files[0].path !== "large-edit.ts" ||
    !workload.edit
  ) {
    throw new Error("frozen L1 large-edit workload is missing or malformed");
  }
  return workload;
}

function parseObservation(mode, result, elapsedSeconds) {
  if (result.status !== 0) {
    throw new Error(
      `L1 ${mode} workload failed (${result.status}):\n${result.stdout}\n${result.stderr}`,
    );
  }
  const output = JSON.parse(result.stdout.trim());
  const rss = /^\s*(\d+)\s+maximum resident set size$/mu.exec(result.stderr);
  if (!rss) throw new Error(`cannot read Darwin maximum RSS from /usr/bin/time:\n${result.stderr}`);
  if (
    output.schema !== 1 ||
    output.kind !== "l1-incremental-parser-operation" ||
    output.mode !== mode ||
    !(output.operation_nanoseconds > 0)
  ) {
    throw new Error(`invalid L1 ${mode} allocation observation`);
  }
  return {
    process_wall_seconds: rounded(elapsedSeconds),
    operation_seconds: rounded(output.operation_nanoseconds / 1_000_000_000),
    max_rss_bytes: Number(rss[1]),
    allocations: output.allocations,
    deallocations: output.deallocations,
    reallocations: output.reallocations,
    bytes_allocated: output.bytes_allocated,
    bytes_deallocated: output.bytes_deallocated,
    bytes_reallocated: output.bytes_reallocated,
    source: output.source,
    reuse: output.reuse,
  };
}

function measure(binary, mode) {
  const started = process.hrtime.bigint();
  const result = spawnSync("/usr/bin/time", ["-l", binary, mode, fixturePath], {
    cwd: workspace,
    encoding: "utf8",
    env: { ...process.env, CARGO_BUILD_JOBS: "2" },
    maxBuffer: 64 * 1024 * 1024,
  });
  const elapsed = Number(process.hrtime.bigint() - started) / 1_000_000_000;
  return parseObservation(mode, result, elapsed);
}

function observationSummary(samples) {
  const field = (name) => samples.map((sample) => sample[name]);
  return {
    median_process_wall_seconds: rounded(median(field("process_wall_seconds"))),
    median_operation_seconds: rounded(median(field("operation_seconds"))),
    p95_operation_seconds: rounded(percentile(field("operation_seconds"), 95)),
    max_rss_bytes: Math.max(...field("max_rss_bytes")),
    median_allocations: median(field("allocations")),
    p95_allocations: percentile(field("allocations"), 95),
    median_bytes_allocated: median(field("bytes_allocated")),
    p95_bytes_allocated: percentile(field("bytes_allocated"), 95),
    source: samples[0].source,
    reuse: samples[0].reuse,
  };
}

function ratio(candidate, base) {
  if (base === 0) return candidate === 0 ? 1 : Number.POSITIVE_INFINITY;
  return rounded(candidate / base);
}

function comparisonRatios(fresh, incremental) {
  return {
    warm_median_operation_ratio: ratio(
      incremental.median_operation_seconds,
      fresh.median_operation_seconds,
    ),
    warm_p95_operation_ratio: ratio(
      incremental.p95_operation_seconds,
      fresh.p95_operation_seconds,
    ),
    peak_rss_ratio: ratio(incremental.max_rss_bytes, fresh.max_rss_bytes),
    allocation_count_ratio: ratio(incremental.median_allocations, fresh.median_allocations),
    allocated_bytes_ratio: ratio(
      incremental.median_bytes_allocated,
      fresh.median_bytes_allocated,
    ),
  };
}

function qualifies(fresh, incremental, ratios) {
  const ceilings = policy().ceilings;
  return (
    ratios.warm_median_operation_ratio <= ceilings.warm_median_operation_ratio &&
    ratios.warm_p95_operation_ratio <= ceilings.warm_p95_operation_ratio &&
    ratios.peak_rss_ratio <= ceilings.peak_rss_ratio &&
    ratios.allocation_count_ratio <= ceilings.allocation_count_ratio &&
    ratios.allocated_bytes_ratio <= ceilings.allocated_bytes_ratio &&
    incremental.median_operation_seconds <=
      ceilings.incremental_warm_median_operation_seconds &&
    incremental.p95_operation_seconds <= ceilings.incremental_warm_p95_operation_seconds &&
    incremental.max_rss_bytes <= ceilings.incremental_peak_rss_bytes &&
    incremental.reuse.incremental === true &&
    incremental.reuse.full_parse_fallback === false &&
    incremental.reuse.nodes >= ceilings.incremental_minimum_reused_nodes &&
    incremental.reuse.freshly_parsed_nodes <=
      ceilings.incremental_maximum_freshly_parsed_nodes &&
    fresh.reuse.incremental === false &&
    fresh.reuse.nodes === 0 &&
    sameJson(fresh.source, incremental.source)
  );
}

function comparePairs(binary, pairCount, workload) {
  const pairs = [];
  for (let ordinal = 0; ordinal < pairCount; ordinal += 1) {
    const order = ordinal % 2 === 0 ? "ab" : "ba";
    const observations = {};
    for (const mode of order === "ab" ? ["fresh", "incremental"] : ["incremental", "fresh"]) {
      observations[mode] = measure(binary, mode);
    }
    pairs.push({ ordinal, order, ...observations });
  }
  const warmPairs = pairs.slice(1);
  const fresh = observationSummary(warmPairs.map((pair) => pair.fresh));
  const incremental = observationSummary(warmPairs.map((pair) => pair.incremental));
  const ratios = comparisonRatios(fresh, incremental);
  return {
    id: workload.id,
    workload_sha256: workload.workload_sha256,
    edit: workload.edit,
    cold_pair_ordinal: 0,
    pairs,
    fresh_summary: fresh,
    incremental_summary: incremental,
    ratios,
    qualified: qualifies(fresh, incremental, ratios),
  };
}

function validateL0Chain(baseCommit) {
  if (!fs.existsSync(l0EvidencePath)) throw new Error("missing L0.4 performance evidence");
  const l0 = JSON.parse(fs.readFileSync(l0EvidencePath, "utf8"));
  if (
    l0.schema !== 1 ||
    l0.kind !== "l0-one-shot-registry-performance" ||
    l0.status !== "qualified"
  ) {
    throw new Error("invalid L0.4 performance evidence header");
  }
  const baseRuntime = trackedRuntimeFingerprintAt(baseCommit);
  if (!sameJson(baseRuntime, l0.candidate.runtime_tree)) {
    throw new Error("L1 performance base is not the exact L0.4 qualified runtime tree");
  }
  return { l0, baseRuntime };
}

function compare(baseRef, pairCount) {
  if (!Number.isInteger(pairCount) || pairCount < policy().minimum_paired_samples + 1) {
    throw new Error("comparison requires one cold pair plus at least seven warm paired samples");
  }
  const allowedDirty = new Set([
    "ratchets/l1-h0-performance.v1.json",
    "ratchets/l1-incremental-parser-performance.v1.json",
  ]);
  const unexpectedDirty = execFileSync("git", ["status", "--porcelain=v1", "-z"], {
    cwd: workspace,
    encoding: "utf8",
  })
    .split("\0")
    .filter(Boolean)
    .map((line) => line.slice(3))
    .filter((entry) => !allowedDirty.has(entry));
  if (unexpectedDirty.length !== 0) {
    throw new Error("performance comparison requires a clean candidate worktree");
  }
  const candidateCommit = git("rev-parse", "HEAD");
  const baseCommit = git("rev-parse", "--verify", `${baseRef}^{commit}`);
  if (candidateCommit === baseCommit) throw new Error("L1 comparison requires a post-L0 candidate");
  command("git", ["merge-base", "--is-ancestor", baseCommit, candidateCommit]);
  const { baseRuntime } = validateL0Chain(baseCommit);
  command("node", ["crates/oracle/l0-fixtures.mjs", "--check"]);
  command("node", ["crates/oracle/l0-fixtures.mjs", "--materialize", fixtureRoot]);
  const workload = largeEditWorkload();
  if (sha256(fs.readFileSync(fixturePath)) !== workload.files[0].sha256) {
    throw new Error("materialized L1 fixture hash does not match its manifest");
  }

  command("cargo", [
    "build",
    "--release",
    "--locked",
    "-p",
    "tsc-rs-syntax",
    "--example",
    "l1_incremental_qualification",
  ]);
  const compared = comparePairs(binaryPath, pairCount, workload);
  return {
    schema: 1,
    kind: "l1-incremental-parser-performance",
    status: compared.qualified ? "qualified" : "failed",
    typescript_version: "6.0.3",
    candidate: {
      commit: candidateCommit,
      runtime_tree: trackedRuntimeFingerprint(),
      binary_sha256: sha256(fs.readFileSync(binaryPath)),
    },
    base: {
      commit: baseCommit,
      runtime_tree: baseRuntime,
      l0_evidence_sha256: sha256(fs.readFileSync(l0EvidencePath)),
    },
    fixture_manifest_sha256: sha256(fs.readFileSync(fixtureManifestPath)),
    fixture_sha256: sha256(fs.readFileSync(fixturePath)),
    driver_sha256: sha256(fs.readFileSync(driverPath)),
    measured_at_utc: new Date().toISOString(),
    runner: runner(),
    toolchain: {
      rustc: command("rustc", ["--version"]),
      node: process.version,
      cargo_build_jobs: 2,
      profile: "release",
      allocator_observer: "stats_alloc-0.1.10-example-only",
    },
    pair_count: pairCount,
    warm_pair_count: pairCount - 1,
    order: "alternating-ab-ba",
    workload: compared,
    performance_policy: policy(),
  };
}

function validateObservation(mode, observation, expectedSource, expectedReuse) {
  for (const field of [
    "process_wall_seconds",
    "operation_seconds",
    "max_rss_bytes",
    "allocations",
    "bytes_allocated",
  ]) {
    if (!(observation[field] > 0)) throw new Error(`L1 ${mode} observation has invalid ${field}`);
  }
  if (expectedSource && !sameJson(observation.source, expectedSource)) {
    throw new Error(`L1 ${mode} source facts changed across pairs`);
  }
  if (expectedReuse && !sameJson(observation.reuse, expectedReuse)) {
    throw new Error(`L1 ${mode} reuse facts changed across pairs`);
  }
}

function validateEvidence(evidence, requireCurrent) {
  if (
    evidence.schema !== 1 ||
    evidence.kind !== "l1-incremental-parser-performance" ||
    evidence.status !== "qualified" ||
    evidence.typescript_version !== "6.0.3"
  ) {
    throw new Error("invalid L1 performance evidence header");
  }
  for (const side of ["candidate", "base"]) {
    if (!/^[0-9a-f]{40}$/u.test(evidence[side].commit)) throw new Error(`invalid ${side} commit`);
    command("git", ["cat-file", "-e", `${evidence[side].commit}^{commit}`]);
    if (!sameJson(evidence[side].runtime_tree, trackedRuntimeFingerprintAt(evidence[side].commit))) {
      throw new Error(`L1 ${side} runtime fingerprint does not match its commit`);
    }
  }
  command("git", ["merge-base", "--is-ancestor", evidence.base.commit, evidence.candidate.commit]);
  const { l0, baseRuntime } = validateL0Chain(evidence.base.commit);
  if (
    !sameJson(evidence.base.runtime_tree, baseRuntime) ||
    evidence.base.l0_evidence_sha256 !== sha256(fs.readFileSync(l0EvidencePath)) ||
    !sameJson(evidence.base.runtime_tree, l0.candidate.runtime_tree)
  ) {
    throw new Error("L1 evidence is not chained to the frozen L0.4 runtime evidence");
  }
  if (requireCurrent && !sameJson(evidence.candidate.runtime_tree, trackedRuntimeFingerprint())) {
    throw new Error("current runtime sources differ from the qualified L1 candidate");
  }
  const workload = largeEditWorkload();
  const currentDriverSha256 = sha256(fs.readFileSync(driverPath));
  const driverMatches =
    evidence.driver_sha256 === currentDriverSha256 ||
    (!requireCurrent && evidence.driver_sha256 === frozenQualifiedDriverSha256);
  if (
    evidence.fixture_manifest_sha256 !== sha256(fs.readFileSync(fixtureManifestPath)) ||
    evidence.fixture_sha256 !== workload.files[0].sha256 ||
    !driverMatches
  ) {
    throw new Error("L1 performance fixture or driver binding changed");
  }
  if (
    evidence.runner.id !== "macos-arm64-local-approved" ||
    evidence.runner.os !== "darwin" ||
    evidence.runner.architecture !== "arm64" ||
    evidence.toolchain.node !== "v25.2.1" ||
    evidence.toolchain.cargo_build_jobs !== 2 ||
    evidence.toolchain.profile !== "release" ||
    evidence.toolchain.allocator_observer !== "stats_alloc-0.1.10-example-only"
  ) {
    throw new Error("L1 comparison used an unapproved runner/toolchain profile");
  }
  if (!sameJson(evidence.performance_policy, policy())) {
    throw new Error("L1 performance policy drifted");
  }
  if (
    !Number.isInteger(evidence.pair_count) ||
    evidence.pair_count < policy().minimum_paired_samples + 1 ||
    evidence.warm_pair_count !== evidence.pair_count - 1 ||
    evidence.order !== "alternating-ab-ba"
  ) {
    throw new Error("L1 comparison has too few or incorrectly ordered pairs");
  }
  const compared = evidence.workload;
  if (
    compared.id !== workload.id ||
    compared.workload_sha256 !== workload.workload_sha256 ||
    !sameJson(compared.edit, workload.edit) ||
    compared.cold_pair_ordinal !== 0 ||
    !Array.isArray(compared.pairs) ||
    compared.pairs.length !== evidence.pair_count
  ) {
    throw new Error("L1 comparison does not match its frozen workload/pair shape");
  }
  let freshSource;
  let incrementalSource;
  let freshReuse;
  let incrementalReuse;
  for (const [ordinal, pair] of compared.pairs.entries()) {
    if (pair.ordinal !== ordinal || pair.order !== (ordinal % 2 === 0 ? "ab" : "ba")) {
      throw new Error("L1 comparison has invalid alternating order");
    }
    validateObservation("fresh", pair.fresh, freshSource, freshReuse);
    validateObservation("incremental", pair.incremental, incrementalSource, incrementalReuse);
    freshSource ??= pair.fresh.source;
    incrementalSource ??= pair.incremental.source;
    freshReuse ??= pair.fresh.reuse;
    incrementalReuse ??= pair.incremental.reuse;
  }
  const warmPairs = compared.pairs.slice(1);
  const fresh = observationSummary(warmPairs.map((pair) => pair.fresh));
  const incremental = observationSummary(warmPairs.map((pair) => pair.incremental));
  const ratios = comparisonRatios(fresh, incremental);
  const qualified = qualifies(fresh, incremental, ratios);
  if (
    !sameJson(compared.fresh_summary, fresh) ||
    !sameJson(compared.incremental_summary, incremental) ||
    !sameJson(compared.ratios, ratios) ||
    compared.qualified !== qualified ||
    !qualified
  ) {
    throw new Error("L1 workload exceeds or misstates its latency/allocation/RSS budget");
  }
  return evidence;
}

const commandName = process.argv[2];
if (commandName === "--compare") {
  const baselineArgument = process.argv.indexOf("--baseline");
  const pairsArgument = process.argv.indexOf("--pairs");
  const baseline = baselineArgument >= 0 ? process.argv[baselineArgument + 1] : undefined;
  const pairCount = pairsArgument >= 0 ? Number(process.argv[pairsArgument + 1]) : 8;
  if (!baseline) throw new Error("--compare requires --baseline <exact-commit>");
  const evidence = compare(baseline, pairCount);
  validateEvidence(evidence, true);
  fs.writeFileSync(evidencePath, `${JSON.stringify(evidence, null, 2)}\n`);
  process.stdout.write(`wrote ${path.relative(workspace, evidencePath)}\n`);
} else if (commandName === "--check") {
  if (!fs.existsSync(evidencePath)) {
    throw new Error(
      "missing ratchets/l1-incremental-parser-performance.v1.json; run --compare on the approved runner",
    );
  }
  validateEvidence(JSON.parse(fs.readFileSync(evidencePath, "utf8")), false);
  process.stdout.write("L1 incremental-parser performance lineage is valid\n");
} else {
  throw new Error(
    "usage: l1-performance.mjs --compare --baseline <exact-L0.4-commit> [--pairs N]|--check",
  );
}
