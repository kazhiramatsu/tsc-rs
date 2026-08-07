import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const generatorPath = fileURLToPath(import.meta.url);
const workspace = path.resolve(path.dirname(generatorPath), "../..");
const evidenceRelative = "ratchets/h1-noemit-performance.v1.json";
const evidencePath = path.join(workspace, evidenceRelative);
const contractRelative = ".github/ci/contracts/h1-noemit-performance.schema.json";
const contractPath = path.join(workspace, contractRelative);
const fixtureRelative = "ratchets/l0-fixtures.v1.json";
const fixtureManifestPath = path.join(workspace, fixtureRelative);
const fixtureRoot = path.join(workspace, "target/h1/noemit-qualification-fixtures");
const absoluteH0Relative = "ratchets/h0-qualification.v1.json";
const absoluteH0Path = path.join(workspace, absoluteH0Relative);
const runtimeParentRelative = "ratchets/l1-h0-performance.v1.json";
const runtimeParentPath = path.join(workspace, runtimeParentRelative);
const binaryPath = path.join(workspace, "target/release/examples/h0_qualification");

// H1.0a changed only evidence and documentation after the L0/L1 runtime was
// qualified. This merge is the exact reviewed pre-H1 runtime anchor against
// which every H1 semantic candidate remains comparable.
const trustedPreH1Commit = "c0951bf15cdec74223de29e06cd908b0899712f6";
const expectedRustc = "rustc 1.93.0 (254b59607 2026-01-19)";
const expectedNode = "v25.2.1";
const activityFields = [
  "emit_resolver_constructions",
  "transformer_initializations",
  "transform_context_constructions",
  "emit_side_table_allocations",
  "printer_writer_constructions",
  "output_plan_constructions",
  "emit_artifact_creations",
  "output_sink_writes",
];
const runtimePrefixes = [
  "crates/binder/",
  "crates/checker/",
  "crates/compiler/",
  "crates/diagnostics/",
  "crates/emitter/",
  "crates/host/",
  "crates/program/",
  "crates/syntax/",
  "crates/types/",
  "vendor/typescript-6.0.3/lib/",
];
const runtimeExact = new Set([
  "Cargo.lock",
  "Cargo.toml",
  "rust-toolchain.toml",
  ".node-version",
]);

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

function trackedRuntimePathsAt(commit = undefined) {
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
  const paths = trackedRuntimePathsAt();
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
  const paths = trackedRuntimePathsAt(commit);
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
  return sorted.length % 2 === 0
    ? (sorted[middle - 1] + sorted[middle]) / 2
    : sorted[middle];
}

function percentile(values, percentileValue) {
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.max(0, Math.ceil((percentileValue / 100) * sorted.length) - 1)];
}

function rounded(value) {
  return Number(value.toFixed(6));
}

function ratio(candidate, base) {
  if (base === 0) return candidate === 0 ? 1 : Number.POSITIVE_INFINITY;
  return rounded(candidate / base);
}

function relativeRange(values) {
  const center = median(values);
  return center === 0 ? 0 : rounded((Math.max(...values) - Math.min(...values)) / center);
}

function exactKeys(value, required) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return false;
  const actual = Object.keys(value).sort();
  const expected = [...required].sort();
  return JSON.stringify(actual) === JSON.stringify(expected);
}

function sameJson(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function relativePolicy() {
  return {
    comparison: "same-approved-runner-alternating-baseline-candidate",
    trusted_pre_h1_commit: trustedPreH1Commit,
    minimum_warm_paired_samples: 7,
    order: "alternating-ab-ba",
    variance_review: "warm-nearest-rank-p95-over-median-plus-relative-range",
    ceilings: {
      warm_median_wall_ratio: 1.1,
      warm_p95_wall_ratio: 1.15,
      peak_rss_ratio: 1.1,
      allocation_count_ratio: 1.02,
      allocated_bytes_ratio: 1.03,
      parsed_documents_ratio: 1,
      bound_documents_ratio: 1,
      full_text_copies_ratio: 1,
      full_text_bytes_copied_ratio: 1,
      executable_size_ratio: 1.25,
    },
    exact_zero: {
      output_writes: 0,
      emit_resolver_constructions: 0,
      transformer_initializations: 0,
      transform_context_constructions: 0,
      emit_side_table_allocations: 0,
      printer_writer_constructions: 0,
      output_plan_constructions: 0,
      emit_artifact_creations: 0,
      output_sink_writes: 0,
    },
    moving_hosted_runner_can_mint_or_relax: false,
  };
}

function absoluteH0Policy() {
  const artifact = JSON.parse(fs.readFileSync(absoluteH0Path, "utf8"));
  const profile = artifact.resource_profiles.find(
    (entry) => entry.id === "cli-dev-macos-arm64",
  );
  if (!profile) throw new Error("H0 qualification has no explicit-root resource profile");
  return {
    artifact: { path: absoluteH0Relative, sha256: sha256(fs.readFileSync(absoluteH0Path)) },
    resource_profile_id: profile.id,
    workload_id: "explicit-root",
    ceilings: {
      cold_wall_seconds: profile.ceilings.cold_wall_seconds,
      warm_wall_seconds: profile.ceilings.warm_wall_seconds,
      max_rss_bytes: profile.ceilings.max_rss_bytes,
    },
  };
}

function trustedRuntimeParent() {
  const artifact = JSON.parse(fs.readFileSync(runtimeParentPath, "utf8"));
  if (
    artifact.kind !== "l1-h0-nonregression-performance" ||
    artifact.status !== "qualified"
  ) {
    throw new Error("L1 H0 runtime parent is not qualified");
  }
  return {
    artifact: {
      path: runtimeParentRelative,
      sha256: sha256(fs.readFileSync(runtimeParentPath)),
    },
    candidate_commit: artifact.candidate.commit,
    candidate_runtime_tree: artifact.candidate.runtime_tree,
  };
}

function runner() {
  if (process.platform !== "darwin" || process.arch !== "arm64") {
    throw new Error("H1 no-emit evidence may only be minted on approved macOS arm64");
  }
  if (process.version !== expectedNode || command("rustc", ["--version"]) !== expectedRustc) {
    throw new Error("H1 no-emit evidence requires the frozen Rust and Node toolchain");
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

function snapshotTree(root) {
  const snapshot = new Map();
  const visit = (directory) => {
    const entries = fs
      .readdirSync(directory, { withFileTypes: true })
      .sort((left, right) => left.name.localeCompare(right.name));
    for (const entry of entries) {
      const absolute = path.join(directory, entry.name);
      const relative = path.relative(root, absolute).split(path.sep).join("/");
      if (entry.isDirectory()) visit(absolute);
      else if (entry.isFile()) snapshot.set(relative, sha256(fs.readFileSync(absolute)));
      else snapshot.set(relative, `other:${entry.isSymbolicLink()}`);
    }
  };
  visit(root);
  return snapshot;
}

function changedFileCount(before, after) {
  const names = new Set([...before.keys(), ...after.keys()]);
  let changed = 0;
  for (const name of names) {
    if (before.get(name) !== after.get(name)) changed += 1;
  }
  return changed;
}

function validActivity(value) {
  return (
    exactKeys(value, activityFields) &&
    activityFields.every((field) => Number.isInteger(value[field]) && value[field] >= 0)
  );
}

function parseObservation(result, elapsedSeconds, outputWrites, side) {
  if (result.status !== 0) {
    throw new Error(
      `H1 no-emit workload failed (${result.status}):\n${result.stdout}\n${result.stderr}`,
    );
  }
  const output = JSON.parse(result.stdout.trim());
  const rss = /^\s*(\d+)\s+maximum resident set size$/mu.exec(result.stderr);
  if (!rss) throw new Error(`cannot read Darwin maximum RSS:\n${result.stderr}`);
  if (![1, 2].includes(output.schema) || output.exit_code !== 0) {
    throw new Error("invalid H0 qualification observation");
  }
  if (side === "candidate" && output.schema !== 2) {
    throw new Error("candidate does not expose the H1 no-emit activity contract");
  }
  const activity = output.schema === 2 ? output.h1_no_emit : null;
  if (activity !== null && !validActivity(activity)) {
    throw new Error("invalid H1 no-emit activity observation");
  }
  return {
    wall_seconds: rounded(elapsedSeconds),
    max_rss_bytes: Number(rss[1]),
    allocations: output.allocations,
    deallocations: output.deallocations,
    reallocations: output.reallocations,
    bytes_allocated: output.bytes_allocated,
    bytes_deallocated: output.bytes_deallocated,
    bytes_reallocated: output.bytes_reallocated,
    output_writes: outputWrites,
    work: output.work,
    h1_no_emit: activity,
  };
}

function measureBinary(binary, workload, side) {
  const directory = path.join(fixtureRoot, workload.id);
  const before = snapshotTree(directory);
  const started = process.hrtime.bigint();
  const result = spawnSync("/usr/bin/time", ["-l", binary, ...workload.args], {
    cwd: directory,
    encoding: "utf8",
    env: {
      ...process.env,
      CARGO_BUILD_JOBS: "2",
      CARGO_INCREMENTAL: "0",
      RUSTC_WRAPPER: "",
    },
    maxBuffer: 64 * 1024 * 1024,
  });
  const elapsed = Number(process.hrtime.bigint() - started) / 1_000_000_000;
  const after = snapshotTree(directory);
  return parseObservation(result, elapsed, changedFileCount(before, after), side);
}

function comparisonSummary(samples) {
  const warm = samples.slice(1);
  const warmField = (name) => warm.map((sample) => sample[name]);
  return {
    cold_wall_seconds: samples[0].wall_seconds,
    cold_max_rss_bytes: samples[0].max_rss_bytes,
    warm_median_wall_seconds: rounded(median(warmField("wall_seconds"))),
    warm_p95_wall_seconds: rounded(percentile(warmField("wall_seconds"), 95)),
    peak_rss_bytes: Math.max(...samples.map((sample) => sample.max_rss_bytes)),
    warm_median_allocations: median(warmField("allocations")),
    warm_median_bytes_allocated: median(warmField("bytes_allocated")),
    max_output_writes: Math.max(...samples.map((sample) => sample.output_writes)),
    work: samples[0].work,
    h1_no_emit: samples[0].h1_no_emit,
  };
}

function comparisonRatios(base, candidate) {
  return {
    warm_median_wall_ratio: ratio(
      candidate.warm_median_wall_seconds,
      base.warm_median_wall_seconds,
    ),
    warm_p95_wall_ratio: ratio(candidate.warm_p95_wall_seconds, base.warm_p95_wall_seconds),
    peak_rss_ratio: ratio(candidate.peak_rss_bytes, base.peak_rss_bytes),
    allocation_count_ratio: ratio(
      candidate.warm_median_allocations,
      base.warm_median_allocations,
    ),
    allocated_bytes_ratio: ratio(
      candidate.warm_median_bytes_allocated,
      base.warm_median_bytes_allocated,
    ),
    parsed_documents_ratio: ratio(
      candidate.work.parsed_documents,
      base.work.parsed_documents,
    ),
    bound_documents_ratio: ratio(candidate.work.bound_documents, base.work.bound_documents),
    full_text_copies_ratio: ratio(candidate.work.full_text_copies, base.work.full_text_copies),
    full_text_bytes_copied_ratio: ratio(
      candidate.work.full_text_bytes_copied,
      base.work.full_text_bytes_copied,
    ),
  };
}

function workloadVariance(pairs) {
  const warmPairs = pairs.slice(1);
  const base = warmPairs.map((pair) => pair.base.wall_seconds);
  const candidate = warmPairs.map((pair) => pair.candidate.wall_seconds);
  const pairedRatios = warmPairs.map((pair) => pair.candidate.wall_seconds / pair.base.wall_seconds);
  return {
    base_warm_p95_over_median: ratio(percentile(base, 95), median(base)),
    candidate_warm_p95_over_median: ratio(percentile(candidate, 95), median(candidate)),
    base_warm_relative_range: relativeRange(base),
    candidate_warm_relative_range: relativeRange(candidate),
    paired_wall_ratio_min: rounded(Math.min(...pairedRatios)),
    paired_wall_ratio_max: rounded(Math.max(...pairedRatios)),
  };
}

function ratiosQualify(ratios) {
  const ceilings = relativePolicy().ceilings;
  return Object.entries(ratios).every(([field, value]) => value <= ceilings[field]);
}

function activityIsZero(activity) {
  return activity !== null && activityFields.every((field) => activity[field] === 0);
}

function absoluteH0Qualifies(workloadId, summary) {
  if (workloadId !== "explicit-root") return true;
  const ceilings = absoluteH0Policy().ceilings;
  return (
    summary.cold_wall_seconds <= ceilings.cold_wall_seconds &&
    summary.warm_median_wall_seconds <= ceilings.warm_wall_seconds &&
    summary.peak_rss_bytes <= ceilings.max_rss_bytes
  );
}

function workloadQualifies(workload) {
  return (
    ratiosQualify(workload.ratios) &&
    workload.base_summary.max_output_writes === 0 &&
    workload.candidate_summary.max_output_writes === 0 &&
    activityIsZero(workload.candidate_summary.h1_no_emit) &&
    absoluteH0Qualifies(workload.id, workload.candidate_summary)
  );
}

function compareWorkload(workload, pairCount, binaries) {
  const pairs = [];
  for (let ordinal = 0; ordinal < pairCount; ordinal += 1) {
    const order = ordinal % 2 === 0 ? "ab" : "ba";
    const observations = {};
    for (const side of order === "ab" ? ["base", "candidate"] : ["candidate", "base"]) {
      observations[side] = measureBinary(binaries[side], workload, side);
    }
    pairs.push({ ordinal, order, ...observations });
  }
  const base = comparisonSummary(pairs.map((pair) => pair.base));
  const candidate = comparisonSummary(pairs.map((pair) => pair.candidate));
  const compared = {
    id: workload.id,
    args: workload.args,
    workload_sha256: workload.workload_sha256,
    absolute_h0_ceiling_applied: workload.id === "explicit-root",
    pairs,
    base_summary: base,
    candidate_summary: candidate,
    variance: workloadVariance(pairs),
    ratios: comparisonRatios(base, candidate),
    qualified: false,
  };
  compared.qualified = workloadQualifies(compared);
  return compared;
}

function observedVariance(workloads) {
  const values = workloads.map((workload) => workload.variance);
  return {
    method: "warm-nearest-rank-p95-over-median-plus-relative-range",
    max_base_warm_p95_over_median: Math.max(
      ...values.map((value) => value.base_warm_p95_over_median),
    ),
    max_candidate_warm_p95_over_median: Math.max(
      ...values.map((value) => value.candidate_warm_p95_over_median),
    ),
    max_base_warm_relative_range: Math.max(
      ...values.map((value) => value.base_warm_relative_range),
    ),
    max_candidate_warm_relative_range: Math.max(
      ...values.map((value) => value.candidate_warm_relative_range),
    ),
  };
}

function binarySizeEvidence(baseBytes, candidateBytes) {
  const evidence = {
    base_bytes: baseBytes,
    candidate_bytes: candidateBytes,
    ratio: ratio(candidateBytes, baseBytes),
    ceiling: relativePolicy().ceilings.executable_size_ratio,
    qualified: false,
  };
  evidence.qualified = evidence.ratio <= evidence.ceiling;
  return evidence;
}

function dirtyPaths() {
  return execFileSync("git", ["status", "--porcelain=v1", "-z"], {
    cwd: workspace,
    encoding: "utf8",
  })
    .split("\0")
    .filter(Boolean)
    .map((line) => line.slice(3));
}

function compare(baseRef, pairCount) {
  if (!Number.isInteger(pairCount) || pairCount < relativePolicy().minimum_warm_paired_samples + 1) {
    throw new Error("comparison requires one cold pair plus at least seven warm AB/BA pairs");
  }
  const dirty = dirtyPaths();
  if (dirty.length !== 0) {
    throw new Error(`performance comparison requires a clean candidate worktree: ${dirty.join(", ")}`);
  }
  const candidateCommit = git("rev-parse", "HEAD");
  const baseCommit = git("rev-parse", "--verify", `${baseRef}^{commit}`);
  if (baseCommit !== trustedPreH1Commit) {
    throw new Error(`baseline must be the frozen pre-H1 commit ${trustedPreH1Commit}`);
  }
  if (candidateCommit === baseCommit) throw new Error("candidate and baseline must differ");
  command("git", ["merge-base", "--is-ancestor", baseCommit, candidateCommit]);
  command("node", ["crates/oracle/l0-fixtures.mjs", "--check"]);
  command("node", ["crates/oracle/l0-fixtures.mjs", "--materialize", fixtureRoot]);

  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "tsc-rs-h1-noemit-performance-"));
  const baseWorkspace = path.join(temporary, "base");
  const baseTarget = path.join(temporary, "base-target");
  const candidateBinary = path.join(temporary, "candidate-h0-qualification");
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
      ["build", "--release", "--locked", "-p", "tsc-rs-compiler", "--example", "h0_qualification"],
      { env: buildEnvironment },
    );
    fs.copyFileSync(binaryPath, candidateBinary);
    fs.chmodSync(candidateBinary, 0o755);

    command("git", ["worktree", "add", "--detach", baseWorkspace, baseCommit]);
    worktreeAdded = true;
    commandAt(
      baseWorkspace,
      "cargo",
      ["build", "--release", "--locked", "-p", "tsc-rs-compiler", "--example", "h0_qualification"],
      {
        env: {
          ...buildEnvironment,
          CARGO_TARGET_DIR: baseTarget,
        },
      },
    );
    const baseBinary = path.join(baseTarget, "release/examples/h0_qualification");
    const fixtures = JSON.parse(fs.readFileSync(fixtureManifestPath, "utf8"));
    const workloads = fixtures.workloads.filter((workload) => workload.args !== null);
    const compared = workloads.map((workload) =>
      compareWorkload(workload, pairCount, { base: baseBinary, candidate: candidateBinary }),
    );
    const binarySize = binarySizeEvidence(
      fs.statSync(baseBinary).size,
      fs.statSync(candidateBinary).size,
    );
    const qualified = binarySize.qualified && compared.every((workload) => workload.qualified);
    return {
      schema: 1,
      kind: "h1-noemit-performance",
      status: qualified ? "qualified" : "failed",
      phase: "H1.0b",
      typescript_version: "6.0.3",
      base: {
        commit: baseCommit,
        runtime_tree: trackedRuntimeFingerprintAt(baseCommit),
        executable: {
          sha256: sha256(fs.readFileSync(baseBinary)),
          bytes: fs.statSync(baseBinary).size,
        },
      },
      candidate: {
        commit: candidateCommit,
        runtime_tree: trackedRuntimeFingerprint(),
        executable: {
          sha256: sha256(fs.readFileSync(candidateBinary)),
          bytes: fs.statSync(candidateBinary).size,
        },
      },
      generator: {
        path: path.relative(workspace, generatorPath),
        sha256: sha256(fs.readFileSync(generatorPath)),
      },
      contract: { path: contractRelative, sha256: sha256(fs.readFileSync(contractPath)) },
      fixture_manifest: {
        path: fixtureRelative,
        sha256: sha256(fs.readFileSync(fixtureManifestPath)),
      },
      measured_at_utc: new Date().toISOString(),
      runner: runner(),
      toolchain: {
        rustc: command("rustc", ["--version"]),
        node: process.version,
        cargo_build_jobs: 2,
        profile: "release",
        allocator_observer: "stats_alloc-0.1.10-example-only",
        rss_observer: "bsd-time-l",
      },
      sampling: {
        pair_count: pairCount,
        warm_pair_count: pairCount - 1,
        cold_pair_ordinal: 0,
        order: "alternating-ab-ba",
      },
      observed_variance: observedVariance(compared),
      trusted_runtime_parent: trustedRuntimeParent(),
      absolute_h0_policy: absoluteH0Policy(),
      relative_regression_policy: relativePolicy(),
      binary_size: binarySize,
      workloads: compared,
    };
  } finally {
    if (worktreeAdded) {
      try {
        command("git", ["worktree", "remove", "--force", baseWorkspace]);
      } catch {
        // Preserve the comparison failure and prune the stale entry below.
      }
    }
    fs.rmSync(temporary, { force: true, recursive: true });
    command("git", ["worktree", "prune"]);
  }
}

function assertPositiveNumber(value, label) {
  if (!(typeof value === "number" && Number.isFinite(value) && value > 0)) {
    throw new Error(`${label} must be a positive finite number`);
  }
}

function validateWork(work, label) {
  const fields = [
    "parsed_documents",
    "bound_documents",
    "full_text_copies",
    "full_text_bytes_copied",
  ];
  if (!exactKeys(work, fields)) throw new Error(`${label} has an invalid work-counter shape`);
  for (const field of fields) {
    if (!Number.isInteger(work[field]) || work[field] < 0) {
      throw new Error(`${label}.${field} is invalid`);
    }
  }
}

function validateObservation(observation, side, label) {
  const fields = [
    "wall_seconds",
    "max_rss_bytes",
    "allocations",
    "deallocations",
    "reallocations",
    "bytes_allocated",
    "bytes_deallocated",
    "bytes_reallocated",
    "output_writes",
    "work",
    "h1_no_emit",
  ];
  if (!exactKeys(observation, fields)) throw new Error(`${label} observation shape drifted`);
  for (const field of ["wall_seconds", "max_rss_bytes", "allocations", "bytes_allocated"]) {
    assertPositiveNumber(observation[field], `${label}.${field}`);
  }
  for (const field of [
    "deallocations",
    "reallocations",
    "bytes_deallocated",
    "bytes_reallocated",
    "output_writes",
  ]) {
    if (!Number.isInteger(observation[field]) || observation[field] < 0) {
      throw new Error(`${label}.${field} is invalid`);
    }
  }
  validateWork(observation.work, label);
  if (side === "base") {
    if (observation.h1_no_emit !== null) {
      throw new Error(`${label} must preserve the pre-canary base observation as null`);
    }
  } else if (!validActivity(observation.h1_no_emit)) {
    throw new Error(`${label} has invalid H1 activity counters`);
  }
}

function validateEvidence(evidence, requireCurrent) {
  const topFields = [
    "schema",
    "kind",
    "status",
    "phase",
    "typescript_version",
    "base",
    "candidate",
    "generator",
    "contract",
    "fixture_manifest",
    "measured_at_utc",
    "runner",
    "toolchain",
    "sampling",
    "observed_variance",
    "trusted_runtime_parent",
    "absolute_h0_policy",
    "relative_regression_policy",
    "binary_size",
    "workloads",
  ];
  if (!exactKeys(evidence, topFields)) throw new Error("H1 no-emit artifact shape drifted");
  if (
    evidence.schema !== 1 ||
    evidence.kind !== "h1-noemit-performance" ||
    evidence.status !== "qualified" ||
    evidence.phase !== "H1.0b" ||
    evidence.typescript_version !== "6.0.3"
  ) {
    throw new Error("invalid H1 no-emit artifact header");
  }
  if (!/^\d{4}-\d{2}-\d{2}T/u.test(evidence.measured_at_utc)) {
    throw new Error("invalid H1 no-emit measurement timestamp");
  }

  for (const [side, identity] of [
    ["base", evidence.base],
    ["candidate", evidence.candidate],
  ]) {
    if (!exactKeys(identity, ["commit", "runtime_tree", "executable"])) {
      throw new Error(`${side} identity shape drifted`);
    }
    if (!/^[0-9a-f]{40}$/u.test(identity.commit)) throw new Error(`${side} commit is invalid`);
    command("git", ["cat-file", "-e", `${identity.commit}^{commit}`]);
    if (
      !exactKeys(identity.runtime_tree, ["files", "sha256"]) ||
      !Number.isInteger(identity.runtime_tree.files) ||
      identity.runtime_tree.files <= 0 ||
      !/^[0-9a-f]{64}$/u.test(identity.runtime_tree.sha256)
    ) {
      throw new Error(`${side} runtime fingerprint is invalid`);
    }
    if (
      !exactKeys(identity.executable, ["sha256", "bytes"]) ||
      !/^[0-9a-f]{64}$/u.test(identity.executable.sha256) ||
      !Number.isInteger(identity.executable.bytes) ||
      identity.executable.bytes <= 0
    ) {
      throw new Error(`${side} executable identity is invalid`);
    }
    if (!sameJson(identity.runtime_tree, trackedRuntimeFingerprintAt(identity.commit))) {
      throw new Error(`${side} commit no longer matches its runtime fingerprint`);
    }
  }
  if (evidence.base.commit !== trustedPreH1Commit) {
    throw new Error("H1 no-emit artifact does not use the trusted pre-H1 commit");
  }
  command("git", ["merge-base", "--is-ancestor", evidence.base.commit, evidence.candidate.commit]);
  command("git", ["merge-base", "--is-ancestor", evidence.candidate.commit, "HEAD"]);
  if (requireCurrent && !sameJson(evidence.candidate.runtime_tree, trackedRuntimeFingerprint())) {
    throw new Error("current H1 runtime has not been measured against the frozen no-emit base");
  }

  const expectedReferences = [
    [evidence.generator, "crates/oracle/h1-noemit-performance.mjs", generatorPath],
    [evidence.contract, contractRelative, contractPath],
    [evidence.fixture_manifest, fixtureRelative, fixtureManifestPath],
  ];
  for (const [reference, expectedPath, absolute] of expectedReferences) {
    if (
      !exactKeys(reference, ["path", "sha256"]) ||
      reference.path !== expectedPath ||
      reference.sha256 !== sha256(fs.readFileSync(absolute))
    ) {
      throw new Error(`${expectedPath} hash binding drifted`);
    }
  }

  if (
    !exactKeys(evidence.runner, [
      "id",
      "os",
      "architecture",
      "os_release",
      "product_version",
      "cpu",
      "logical_cpus",
    ]) ||
    evidence.runner.id !== "macos-arm64-local-approved" ||
    evidence.runner.os !== "darwin" ||
    evidence.runner.architecture !== "arm64" ||
    !Number.isInteger(evidence.runner.logical_cpus) ||
    evidence.runner.logical_cpus <= 0
  ) {
    throw new Error("H1 no-emit artifact used an unapproved runner");
  }
  if (
    !exactKeys(evidence.toolchain, [
      "rustc",
      "node",
      "cargo_build_jobs",
      "profile",
      "allocator_observer",
      "rss_observer",
    ]) ||
    evidence.toolchain.rustc !== expectedRustc ||
    evidence.toolchain.node !== expectedNode ||
    evidence.toolchain.cargo_build_jobs !== 2 ||
    evidence.toolchain.profile !== "release" ||
    evidence.toolchain.allocator_observer !== "stats_alloc-0.1.10-example-only" ||
    evidence.toolchain.rss_observer !== "bsd-time-l"
  ) {
    throw new Error("H1 no-emit artifact used an unapproved toolchain");
  }
  if (
    !exactKeys(evidence.sampling, [
      "pair_count",
      "warm_pair_count",
      "cold_pair_ordinal",
      "order",
    ]) ||
    evidence.sampling.pair_count < 8 ||
    evidence.sampling.warm_pair_count !== evidence.sampling.pair_count - 1 ||
    evidence.sampling.cold_pair_ordinal !== 0 ||
    evidence.sampling.order !== "alternating-ab-ba"
  ) {
    throw new Error("invalid H1 no-emit sampling contract");
  }
  if (!sameJson(evidence.relative_regression_policy, relativePolicy())) {
    throw new Error("frozen H1 relative-regression policy drifted");
  }
  if (!sameJson(evidence.absolute_h0_policy, absoluteH0Policy())) {
    throw new Error("absolute H0 resource policy drifted");
  }
  if (!sameJson(evidence.trusted_runtime_parent, trustedRuntimeParent())) {
    throw new Error("trusted L1 H0 runtime-parent binding drifted");
  }
  if (
    !sameJson(
      evidence.base.runtime_tree,
      evidence.trusted_runtime_parent.candidate_runtime_tree,
    )
  ) {
    throw new Error("pre-H1 base is not the exact L1 H0-qualified runtime tree");
  }

  const fixtures = JSON.parse(fs.readFileSync(fixtureManifestPath, "utf8"));
  const expectedWorkloads = fixtures.workloads.filter((workload) => workload.args !== null);
  if (!Array.isArray(evidence.workloads) || evidence.workloads.length !== expectedWorkloads.length) {
    throw new Error("H1 no-emit artifact does not contain the three frozen workloads");
  }
  for (const [index, workload] of evidence.workloads.entries()) {
    const fixture = expectedWorkloads[index];
    const workloadFields = [
      "id",
      "args",
      "workload_sha256",
      "absolute_h0_ceiling_applied",
      "pairs",
      "base_summary",
      "candidate_summary",
      "variance",
      "ratios",
      "qualified",
    ];
    if (
      !exactKeys(workload, workloadFields) ||
      workload.id !== fixture.id ||
      !sameJson(workload.args, fixture.args) ||
      workload.workload_sha256 !== fixture.workload_sha256 ||
      workload.absolute_h0_ceiling_applied !== (workload.id === "explicit-root") ||
      !Array.isArray(workload.pairs) ||
      workload.pairs.length !== evidence.sampling.pair_count
    ) {
      throw new Error(`H1 workload ${workload.id} shape or fixture binding drifted`);
    }
    let baseWork;
    let candidateWork;
    let candidateActivity;
    for (const [ordinal, pair] of workload.pairs.entries()) {
      if (
        !exactKeys(pair, ["ordinal", "order", "base", "candidate"]) ||
        pair.ordinal !== ordinal ||
        pair.order !== (ordinal % 2 === 0 ? "ab" : "ba")
      ) {
        throw new Error(`H1 workload ${workload.id} has invalid AB/BA ordering`);
      }
      for (const side of ["base", "candidate"]) {
        const sample = pair[side];
        validateObservation(sample, side, `${workload.id}.${ordinal}.${side}`);
        const expectedWork = side === "base" ? baseWork : candidateWork;
        if (expectedWork && !sameJson(sample.work, expectedWork)) {
          throw new Error(`H1 workload ${workload.id} ${side} work changed across samples`);
        }
        if (side === "base") baseWork ??= sample.work;
        else candidateWork ??= sample.work;
        if (side === "candidate") {
          if (candidateActivity && !sameJson(sample.h1_no_emit, candidateActivity)) {
            throw new Error(`H1 workload ${workload.id} activity changed across samples`);
          }
          candidateActivity ??= sample.h1_no_emit;
        }
      }
    }
    const baseSummary = comparisonSummary(workload.pairs.map((pair) => pair.base));
    const candidateSummary = comparisonSummary(workload.pairs.map((pair) => pair.candidate));
    const ratios = comparisonRatios(baseSummary, candidateSummary);
    const variance = workloadVariance(workload.pairs);
    if (
      !sameJson(workload.base_summary, baseSummary) ||
      !sameJson(workload.candidate_summary, candidateSummary) ||
      !sameJson(workload.ratios, ratios) ||
      !sameJson(workload.variance, variance) ||
      workload.qualified !== workloadQualifies(workload) ||
      !workload.qualified
    ) {
      throw new Error(`H1 workload ${workload.id} exceeds or misstates its frozen budgets`);
    }
    if (
      !(candidateWork.parsed_documents > 0) ||
      candidateWork.parsed_documents !== candidateWork.bound_documents ||
      candidateWork.parsed_documents > baseWork.parsed_documents ||
      candidateWork.bound_documents > baseWork.bound_documents ||
      candidateWork.full_text_copies !== 0 ||
      candidateWork.full_text_bytes_copied !== 0 ||
      workload.base_summary.max_output_writes !== 0 ||
      workload.candidate_summary.max_output_writes !== 0 ||
      !activityIsZero(candidateActivity)
    ) {
      throw new Error(`H1 workload ${workload.id} violates no-emit work/write canaries`);
    }
  }

  const expectedVariance = observedVariance(evidence.workloads);
  if (!sameJson(evidence.observed_variance, expectedVariance)) {
    throw new Error("H1 observed runner variance does not recompute");
  }
  const expectedBinarySize = binarySizeEvidence(
    evidence.base.executable.bytes,
    evidence.candidate.executable.bytes,
  );
  if (
    !sameJson(evidence.binary_size, expectedBinarySize) ||
    !evidence.binary_size.qualified
  ) {
    throw new Error("H1 executable-size evidence exceeds or misstates its budget");
  }
  return evidence;
}

const commandName = process.argv[2];
if (commandName === "--compare") {
  const baselineIndex = process.argv.indexOf("--baseline");
  const pairsIndex = process.argv.indexOf("--pairs");
  const baseline = baselineIndex >= 0 ? process.argv[baselineIndex + 1] : undefined;
  const pairCount = pairsIndex >= 0 ? Number(process.argv[pairsIndex + 1]) : 8;
  if (!baseline) throw new Error("--compare requires --baseline <exact-commit>");
  const evidence = compare(baseline, pairCount);
  validateEvidence(evidence, true);
  fs.writeFileSync(evidencePath, `${JSON.stringify(evidence, null, 2)}\n`);
  process.stdout.write(`wrote ${evidenceRelative}\n`);
} else if (commandName === "--check") {
  if (!fs.existsSync(evidencePath)) {
    throw new Error(`missing ${evidenceRelative}; run --compare on the approved runner`);
  }
  validateEvidence(JSON.parse(fs.readFileSync(evidencePath, "utf8")), true);
  process.stdout.write("H1 no-emit performance evidence is qualified and current\n");
} else {
  throw new Error(
    "usage: h1-noemit-performance.mjs --compare --baseline <exact-commit> [--pairs N]|--check",
  );
}
