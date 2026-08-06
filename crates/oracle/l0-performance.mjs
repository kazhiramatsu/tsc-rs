import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const workspace = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const evidencePath = path.join(workspace, "ratchets/l0-evidence.v1.json");
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

function measureWorkload(workload, sampleCount) {
  const cwd = path.join(fixtureRoot, workload.id);
  const samples = [];
  for (let ordinal = 0; ordinal < sampleCount; ordinal += 1) {
    const started = process.hrtime.bigint();
    const result = spawnSync("/usr/bin/time", ["-l", binaryPath, ...workload.args], {
      cwd,
      encoding: "utf8",
      env: { ...process.env, CARGO_BUILD_JOBS: "2" },
      maxBuffer: 64 * 1024 * 1024,
    });
    const elapsed = Number(process.hrtime.bigint() - started) / 1_000_000_000;
    samples.push({
      ordinal,
      temperature: ordinal === 0 ? "cold" : "warm",
      ...parseObservation(result, elapsed),
    });
  }
  return {
    id: workload.id,
    args: workload.args,
    workload_sha256: workload.workload_sha256,
    samples,
    summary: summary(samples),
  };
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

function measure(sampleCount) {
  if (!Number.isInteger(sampleCount) || sampleCount < 8) throw new Error("measurement requires at least eight samples per workload");
  command("node", ["crates/oracle/l0-fixtures.mjs", "--check"]);
  command("cargo", ["build", "--release", "--locked", "-p", "tsc-rs-compiler", "--example", "h0_qualification"]);
  command("node", ["crates/oracle/l0-fixtures.mjs", "--materialize", fixtureRoot]);
  const fixtures = JSON.parse(fs.readFileSync(fixtureManifestPath, "utf8"));
  const workloads = fixtures.workloads.filter((workload) => workload.args !== null);
  const runtime = trackedRuntimeFingerprint();
  return {
    schema: 1,
    status: "frozen",
    typescript_version: "6.0.3",
    observed_runtime_commit: git("rev-parse", "HEAD"),
    runtime_tree: runtime,
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
    binary: {
      path: "target/release/examples/h0_qualification",
      bytes: fs.statSync(binaryPath).size,
      sha256: sha256(fs.readFileSync(binaryPath)),
    },
    workloads: workloads.map((workload) => measureWorkload(workload, sampleCount)),
    relative_regression_policy: policy(),
  };
}

function sameJson(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function validate(evidence) {
  if (evidence.schema !== 1 || evidence.status !== "frozen" || evidence.typescript_version !== "6.0.3") throw new Error("invalid L0 evidence header");
  if (!/^[0-9a-f]{40}$/u.test(evidence.observed_runtime_commit)) throw new Error("invalid observed runtime commit");
  command("git", ["cat-file", "-e", `${evidence.observed_runtime_commit}^{commit}`]);
  const currentRuntime = trackedRuntimeFingerprint();
  if (!sameJson(evidence.runtime_tree, currentRuntime)) throw new Error("L0 runtime source fingerprint changed after the evidence freeze");
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

const commandName = process.argv[2];
if (commandName === "--write") {
  const samplesArgument = process.argv.indexOf("--samples");
  const sampleCount = samplesArgument >= 0 ? Number(process.argv[samplesArgument + 1]) : 9;
  const evidence = measure(sampleCount);
  validate(evidence);
  fs.writeFileSync(evidencePath, `${JSON.stringify(evidence, null, 2)}\n`);
  process.stdout.write(`wrote ${path.relative(workspace, evidencePath)}\n`);
} else if (commandName === "--check") {
  if (!fs.existsSync(evidencePath)) throw new Error("missing ratchets/l0-evidence.v1.json; run --write on the approved runner");
  validate(JSON.parse(fs.readFileSync(evidencePath, "utf8")));
  process.stdout.write("L0 performance evidence is valid and current\n");
} else {
  throw new Error("usage: l0-performance.mjs --write [--samples N]|--check");
}
