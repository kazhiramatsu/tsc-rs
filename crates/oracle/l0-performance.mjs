import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const workspace = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const evidencePath = path.join(workspace, "ratchets/l0-evidence.v1.json");
const comparisonPath = path.join(workspace, "ratchets/l0-text-ownership-performance.v1.json");
const fixtureManifestPath = path.join(workspace, "ratchets/l0-fixtures.v1.json");
const fixtureRoot = path.join(workspace, "target/l0/qualification-fixtures");
const binaryPath = path.join(workspace, "target/release/examples/h0_qualification");
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

function trackedRuntimeFingerprint() {
  const paths = execFileSync("git", ["ls-files", "-z"], {
    cwd: workspace,
    maxBuffer: 64 * 1024 * 1024,
  })
    .toString("utf8")
    .split("\0")
    .filter(Boolean)
    .filter((entry) => runtimeExact.has(entry) || runtimePrefixes.some((prefix) => entry.startsWith(prefix)))
    .sort((left, right) => left.localeCompare(right));
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
  const paths = execFileSync("git", ["ls-tree", "-r", "--name-only", "-z", commit], {
    cwd: workspace,
    maxBuffer: 64 * 1024 * 1024,
  })
    .toString("utf8")
    .split("\0")
    .filter(Boolean)
    .filter((entry) => runtimeExact.has(entry) || runtimePrefixes.some((prefix) => entry.startsWith(prefix)))
    .sort((left, right) => left.localeCompare(right));
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
  return Number(value.toFixed(6));
}

function summary(samples) {
  const warm = samples.filter((sample) => sample.temperature === "warm");
  const field = (name) => warm.map((sample) => sample[name]);
  return {
    cold_wall_seconds: samples[0].wall_seconds,
    cold_max_rss_bytes: samples[0].max_rss_bytes,
    warm_median_wall_seconds: rounded(median(field("wall_seconds"))),
    warm_p95_wall_seconds: rounded(percentile(field("wall_seconds"), 95)),
    warm_max_rss_bytes: Math.max(...field("max_rss_bytes")),
    warm_median_allocations: median(field("allocations")),
    warm_p95_allocations: percentile(field("allocations"), 95),
    warm_median_bytes_allocated: median(field("bytes_allocated")),
    warm_p95_bytes_allocated: percentile(field("bytes_allocated"), 95),
  };
}

function parseObservation(result, elapsedSeconds) {
  if (result.status !== 0) {
    throw new Error(`qualification workload failed (${result.status}):\n${result.stdout}\n${result.stderr}`);
  }
  const output = JSON.parse(result.stdout.trim());
  const rss = /^\s*(\d+)\s+maximum resident set size$/mu.exec(result.stderr);
  if (!rss) throw new Error(`cannot read Darwin maximum RSS from /usr/bin/time:\n${result.stderr}`);
  if (output.schema !== 1 || output.exit_code !== 0) throw new Error("invalid allocation observation");
  return {
    wall_seconds: rounded(elapsedSeconds),
    max_rss_bytes: Number(rss[1]),
    allocations: output.allocations,
    deallocations: output.deallocations,
    reallocations: output.reallocations,
    bytes_allocated: output.bytes_allocated,
    bytes_deallocated: output.bytes_deallocated,
    bytes_reallocated: output.bytes_reallocated,
    work: output.work,
  };
}

function measureBinary(binary, workload) {
  const started = process.hrtime.bigint();
  const result = spawnSync("/usr/bin/time", ["-l", binary, ...workload.args], {
    cwd: path.join(fixtureRoot, workload.id),
    encoding: "utf8",
    env: { ...process.env, CARGO_BUILD_JOBS: "2" },
    maxBuffer: 64 * 1024 * 1024,
  });
  const elapsed = Number(process.hrtime.bigint() - started) / 1_000_000_000;
  return parseObservation(result, elapsed);
}

function policy() {
  return {
    comparison: "same-approved-runner-alternating-baseline-candidate",
    minimum_paired_samples: 7,
    order: "alternating-ab-ba",
    absolute_h0_ratchet: "ratchets/h0-qualification.v1.json",
    ceilings: {
      warm_median_wall_ratio: 1.1,
      warm_p95_wall_ratio: 1.15,
      peak_rss_ratio: 1.1,
      allocation_count_ratio: 1.02,
      allocated_bytes_ratio: 1.03,
      parsed_documents_ratio: 1.0,
      bound_documents_ratio: 1.0,
      full_text_copies_ratio: 1.0,
      full_text_bytes_copied_ratio: 1.0,
    },
    moving_hosted_runner_can_mint_or_relax: false,
  };
}

function runner() {
  if (process.platform !== "darwin" || process.arch !== "arm64") {
    throw new Error("L0 performance evidence may only be minted on the approved macOS arm64 profile");
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

function commandAt(cwd, program, args, options = {}) {
  return execFileSync(program, args, {
    cwd,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    ...options,
  }).trim();
}

function comparisonSummary(samples) {
  const field = (name) => samples.map((sample) => sample[name]);
  return {
    median_wall_seconds: rounded(median(field("wall_seconds"))),
    p95_wall_seconds: rounded(percentile(field("wall_seconds"), 95)),
    max_rss_bytes: Math.max(...field("max_rss_bytes")),
    median_allocations: median(field("allocations")),
    median_bytes_allocated: median(field("bytes_allocated")),
    work: samples[0].work,
  };
}

function ratio(candidate, base) {
  if (base === 0) return candidate === 0 ? 1 : Number.POSITIVE_INFINITY;
  return rounded(candidate / base);
}

function comparisonRatios(base, candidate) {
  return {
    warm_median_wall_ratio: ratio(candidate.median_wall_seconds, base.median_wall_seconds),
    warm_p95_wall_ratio: ratio(candidate.p95_wall_seconds, base.p95_wall_seconds),
    peak_rss_ratio: ratio(candidate.max_rss_bytes, base.max_rss_bytes),
    allocation_count_ratio: ratio(candidate.median_allocations, base.median_allocations),
    allocated_bytes_ratio: ratio(candidate.median_bytes_allocated, base.median_bytes_allocated),
    parsed_documents_ratio: ratio(candidate.work.parsed_documents, base.work.parsed_documents),
    bound_documents_ratio: ratio(candidate.work.bound_documents, base.work.bound_documents),
    full_text_copies_ratio: ratio(candidate.work.full_text_copies, base.work.full_text_copies),
    full_text_bytes_copied_ratio: ratio(
      candidate.work.full_text_bytes_copied,
      base.work.full_text_bytes_copied,
    ),
  };
}

function ratiosQualify(ratios) {
  const ceilings = policy().ceilings;
  return Object.entries(ceilings).every(([name, ceiling]) => ratios[name] <= ceiling);
}

function compareWorkload(workload, pairCount, binaries) {
  const pairs = [];
  for (let ordinal = 0; ordinal < pairCount; ordinal += 1) {
    const order = ordinal % 2 === 0 ? "ab" : "ba";
    const observations = {};
    for (const side of order === "ab" ? ["base", "candidate"] : ["candidate", "base"]) {
      observations[side] = measureBinary(binaries[side], workload);
    }
    pairs.push({ ordinal, order, ...observations });
  }
  const warmPairs = pairs.slice(1);
  const base = comparisonSummary(warmPairs.map((pair) => pair.base));
  const candidate = comparisonSummary(warmPairs.map((pair) => pair.candidate));
  const ratios = comparisonRatios(base, candidate);
  return {
    id: workload.id,
    args: workload.args,
    workload_sha256: workload.workload_sha256,
    cold_pair_ordinal: 0,
    pairs,
    base_summary: base,
    candidate_summary: candidate,
    ratios,
    qualified: ratiosQualify(ratios),
  };
}

function compare(baseRef, pairCount) {
  if (!Number.isInteger(pairCount) || pairCount < policy().minimum_paired_samples + 1) {
    throw new Error("comparison requires one cold pair plus at least seven warm paired samples");
  }
  if (git("status", "--porcelain").length !== 0) {
    throw new Error("performance comparison requires a clean candidate worktree");
  }
  const candidateCommit = git("rev-parse", "HEAD");
  const baseCommit = git("rev-parse", "--verify", `${baseRef}^{commit}`);
  if (candidateCommit === baseCommit) throw new Error("performance comparison requires distinct candidate/base commits");
  command("git", ["merge-base", "--is-ancestor", baseCommit, candidateCommit]);
  command("node", ["crates/oracle/l0-fixtures.mjs", "--check"]);
  command("node", ["crates/oracle/l0-fixtures.mjs", "--materialize", fixtureRoot]);

  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "tsc-rs-l0-performance-"));
  const baseWorkspace = path.join(temporary, "base");
  const baseTarget = path.join(temporary, "base-target");
  const candidateBinary = path.join(temporary, "candidate-h0-qualification");
  let worktreeAdded = false;
  try {
    command("cargo", ["build", "--release", "--locked", "-p", "tsc-rs-compiler", "--example", "h0_qualification"]);
    fs.copyFileSync(binaryPath, candidateBinary);
    fs.chmodSync(candidateBinary, 0o755);

    command("git", ["worktree", "add", "--detach", baseWorkspace, baseCommit]);
    worktreeAdded = true;
    commandAt(
      baseWorkspace,
      "cargo",
      ["build", "--release", "--locked", "-p", "tsc-rs-compiler", "--example", "h0_qualification"],
      { env: { ...process.env, CARGO_TARGET_DIR: baseTarget, CARGO_BUILD_JOBS: "2" } },
    );
    const baseBinary = path.join(baseTarget, "release/examples/h0_qualification");
    const fixtures = JSON.parse(fs.readFileSync(fixtureManifestPath, "utf8"));
    const workloads = fixtures.workloads.filter((workload) => workload.args !== null);
    const compared = workloads.map((workload) =>
      compareWorkload(workload, pairCount, { base: baseBinary, candidate: candidateBinary }),
    );
    return {
      schema: 1,
      kind: "l0-text-ownership-performance",
      status: compared.every((workload) => workload.qualified) ? "qualified" : "failed",
      typescript_version: "6.0.3",
      candidate: {
        commit: candidateCommit,
        runtime_tree: trackedRuntimeFingerprint(),
        binary_sha256: sha256(fs.readFileSync(candidateBinary)),
      },
      base: {
        commit: baseCommit,
        runtime_tree: trackedRuntimeFingerprintAt(baseCommit),
        binary_sha256: sha256(fs.readFileSync(baseBinary)),
      },
      fixture_manifest_sha256: sha256(fs.readFileSync(fixtureManifestPath)),
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
      workloads: compared,
      relative_regression_policy: policy(),
    };
  } finally {
    if (worktreeAdded) {
      try {
        command("git", ["worktree", "remove", "--force", baseWorkspace]);
      } catch {
        // Preserve the original comparison failure; `worktree prune` below
        // still removes a stale administrative entry after temp cleanup.
      }
    }
    fs.rmSync(temporary, { force: true, recursive: true });
    command("git", ["worktree", "prune"]);
  }
}

function sameJson(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function validateBaseline(evidence) {
  if (evidence.schema !== 1 || evidence.status !== "frozen" || evidence.typescript_version !== "6.0.3") throw new Error("invalid L0 evidence header");
  if (!/^[0-9a-f]{40}$/u.test(evidence.observed_runtime_commit)) throw new Error("invalid observed runtime commit");
  command("git", ["cat-file", "-e", `${evidence.observed_runtime_commit}^{commit}`]);
  const observedRuntime = trackedRuntimeFingerprintAt(evidence.observed_runtime_commit);
  if (!sameJson(evidence.runtime_tree, observedRuntime)) throw new Error("L0.0 runtime commit no longer matches its frozen fingerprint");
  if (evidence.fixture_manifest_sha256 !== sha256(fs.readFileSync(fixtureManifestPath))) throw new Error("L0 fixture manifest changed after the evidence freeze");
  if (evidence.runner.id !== "macos-arm64-local-approved" || evidence.toolchain.node !== "v25.2.1" || evidence.toolchain.cargo_build_jobs !== 2 || evidence.toolchain.profile !== "release") throw new Error("L0 evidence used an unapproved runner/toolchain profile");
  if (!sameJson(evidence.relative_regression_policy, policy())) throw new Error("L0 relative regression policy drifted");
  const fixtures = JSON.parse(fs.readFileSync(fixtureManifestPath, "utf8"));
  const expected = fixtures.workloads.filter((workload) => workload.args !== null);
  if (!Array.isArray(evidence.workloads) || evidence.workloads.length !== expected.length) throw new Error("L0 evidence workload set is incomplete");
  for (const [index, workload] of evidence.workloads.entries()) {
    const fixture = expected[index];
    if (workload.id !== fixture.id || !sameJson(workload.args, fixture.args) || workload.workload_sha256 !== fixture.workload_sha256) throw new Error(`L0 workload ${workload.id} no longer matches its fixture`);
    if (!Array.isArray(workload.samples) || workload.samples.length < 8) throw new Error(`L0 workload ${workload.id} has too few samples`);
    const work = workload.samples[0].work;
    for (const [ordinal, sample] of workload.samples.entries()) {
      if (sample.ordinal !== ordinal || sample.temperature !== (ordinal === 0 ? "cold" : "warm")) throw new Error(`L0 workload ${workload.id} has invalid sample order`);
      for (const field of ["wall_seconds", "max_rss_bytes", "allocations", "bytes_allocated"]) {
        if (!(sample[field] > 0)) throw new Error(`L0 workload ${workload.id} has invalid ${field}`);
      }
      if (!sameJson(sample.work, work)) throw new Error(`L0 workload ${workload.id} work counters changed across samples`);
    }
    if (!(work.parsed_documents > 0) || work.parsed_documents !== work.bound_documents || work.full_text_copies < work.parsed_documents * 2 || !(work.full_text_bytes_copied > 0)) {
      throw new Error(`L0 workload ${workload.id} has invalid H0 parse/bind/text-copy counters`);
    }
    if (!sameJson(workload.summary, summary(workload.samples))) throw new Error(`L0 workload ${workload.id} summary does not recompute`);
    if (workload.summary.warm_p95_wall_seconds / workload.summary.warm_median_wall_seconds > policy().ceilings.warm_p95_wall_ratio) {
      throw new Error(`L0 workload ${workload.id} baseline variance exceeds the reviewed relative budget`);
    }
  }
  return evidence;
}

function validateComparison(evidence) {
  if (
    evidence.schema !== 1 ||
    evidence.kind !== "l0-text-ownership-performance" ||
    evidence.status !== "qualified" ||
    evidence.typescript_version !== "6.0.3"
  ) {
    throw new Error("invalid L0.1 performance evidence header");
  }
  for (const side of ["candidate", "base"]) {
    if (!/^[0-9a-f]{40}$/u.test(evidence[side].commit)) throw new Error(`invalid ${side} commit`);
    command("git", ["cat-file", "-e", `${evidence[side].commit}^{commit}`]);
    if (!sameJson(evidence[side].runtime_tree, trackedRuntimeFingerprintAt(evidence[side].commit))) {
      throw new Error(`L0.1 ${side} runtime fingerprint does not match its commit`);
    }
    if (!/^[0-9a-f]{64}$/u.test(evidence[side].binary_sha256)) throw new Error(`invalid ${side} binary hash`);
  }
  command("git", ["merge-base", "--is-ancestor", evidence.base.commit, evidence.candidate.commit]);
  if (!sameJson(evidence.candidate.runtime_tree, trackedRuntimeFingerprint())) {
    throw new Error("current runtime sources differ from the qualified L0.1 candidate");
  }
  if (evidence.fixture_manifest_sha256 !== sha256(fs.readFileSync(fixtureManifestPath))) {
    throw new Error("L0.1 performance fixture binding changed");
  }
  if (
    evidence.runner.id !== "macos-arm64-local-approved" ||
    evidence.runner.os !== "darwin" ||
    evidence.runner.architecture !== "arm64" ||
    evidence.toolchain.node !== "v25.2.1" ||
    evidence.toolchain.cargo_build_jobs !== 2 ||
    evidence.toolchain.profile !== "release"
  ) {
    throw new Error("L0.1 comparison used an unapproved runner/toolchain profile");
  }
  if (!sameJson(evidence.relative_regression_policy, policy())) {
    throw new Error("L0.1 relative regression policy drifted");
  }
  if (
    !Number.isInteger(evidence.pair_count) ||
    evidence.pair_count < policy().minimum_paired_samples + 1 ||
    evidence.warm_pair_count !== evidence.pair_count - 1 ||
    evidence.order !== "alternating-ab-ba"
  ) {
    throw new Error("L0.1 comparison has too few or incorrectly ordered pairs");
  }
  const fixtures = JSON.parse(fs.readFileSync(fixtureManifestPath, "utf8"));
  const expected = fixtures.workloads.filter((workload) => workload.args !== null);
  if (!Array.isArray(evidence.workloads) || evidence.workloads.length !== expected.length) {
    throw new Error("L0.1 comparison workload set is incomplete");
  }
  for (const [index, workload] of evidence.workloads.entries()) {
    const fixture = expected[index];
    if (
      workload.id !== fixture.id ||
      !sameJson(workload.args, fixture.args) ||
      workload.workload_sha256 !== fixture.workload_sha256 ||
      workload.cold_pair_ordinal !== 0 ||
      !Array.isArray(workload.pairs) ||
      workload.pairs.length !== evidence.pair_count
    ) {
      throw new Error(`L0.1 workload ${workload.id} does not match its frozen fixture/pair shape`);
    }
    let baseWork;
    let candidateWork;
    for (const [ordinal, pair] of workload.pairs.entries()) {
      if (pair.ordinal !== ordinal || pair.order !== (ordinal % 2 === 0 ? "ab" : "ba")) {
        throw new Error(`L0.1 workload ${workload.id} has invalid alternating order`);
      }
      for (const side of ["base", "candidate"]) {
        const sample = pair[side];
        for (const field of ["wall_seconds", "max_rss_bytes", "allocations", "bytes_allocated"]) {
          if (!(sample[field] > 0)) throw new Error(`L0.1 workload ${workload.id} has invalid ${side} ${field}`);
        }
        const expectedWork = side === "base" ? baseWork : candidateWork;
        if (expectedWork && !sameJson(sample.work, expectedWork)) {
          throw new Error(`L0.1 workload ${workload.id} ${side} work changed across pairs`);
        }
        if (side === "base") baseWork ??= sample.work;
        else candidateWork ??= sample.work;
      }
    }
    const warmPairs = workload.pairs.slice(1);
    const baseSummary = comparisonSummary(warmPairs.map((pair) => pair.base));
    const candidateSummary = comparisonSummary(warmPairs.map((pair) => pair.candidate));
    const ratios = comparisonRatios(baseSummary, candidateSummary);
    if (
      !sameJson(workload.base_summary, baseSummary) ||
      !sameJson(workload.candidate_summary, candidateSummary) ||
      !sameJson(workload.ratios, ratios) ||
      workload.qualified !== ratiosQualify(ratios) ||
      !workload.qualified
    ) {
      throw new Error(`L0.1 workload ${workload.id} exceeds or misstates its relative budget`);
    }
    if (
      !(candidateWork.parsed_documents > 0) ||
      candidateWork.parsed_documents !== candidateWork.bound_documents ||
      candidateWork.parsed_documents > baseWork.parsed_documents ||
      candidateWork.bound_documents > baseWork.bound_documents ||
      candidateWork.full_text_copies !== 0 ||
      candidateWork.full_text_bytes_copied !== 0
    ) {
      throw new Error(`L0.1 workload ${workload.id} does not prove zero-copy H0 ownership`);
    }
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
  validateComparison(evidence);
  fs.writeFileSync(comparisonPath, `${JSON.stringify(evidence, null, 2)}\n`);
  process.stdout.write(`wrote ${path.relative(workspace, comparisonPath)}\n`);
} else if (commandName === "--check") {
  if (!fs.existsSync(evidencePath)) throw new Error("missing frozen ratchets/l0-evidence.v1.json");
  validateBaseline(JSON.parse(fs.readFileSync(evidencePath, "utf8")));
  if (!fs.existsSync(comparisonPath)) {
    throw new Error("missing ratchets/l0-text-ownership-performance.v1.json; run --compare on the approved runner");
  }
  validateComparison(JSON.parse(fs.readFileSync(comparisonPath, "utf8")));
  process.stdout.write("L0.0 baseline and L0.1 relative performance evidence are valid and current\n");
} else {
  throw new Error("usage: l0-performance.mjs --compare --baseline <exact-commit> [--pairs N]|--check");
}
