import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-baseline.mjs";
const EVIDENCE_RELATIVE_PATH = "ratchets/h2-runtime-baseline.v1.json";
const EVIDENCE_PATH = path.join(WORKSPACE, EVIDENCE_RELATIVE_PATH);
const CONTRACT_RELATIVE_PATH = ".github/ci/contracts/h2-runtime-baseline.schema.json";
const QUALIFICATION_EXAMPLE_RELATIVE_PATH =
  "crates/compiler/examples/h2_baseline_qualification.rs";
const FIXTURE_MANIFEST_RELATIVE_PATH = "ratchets/l0-fixtures.v1.json";
const FIXTURE_ROOT = path.join(WORKSPACE, "target/h2/qualification-fixtures");
const L1_FIXTURE_PATH = path.join(FIXTURE_ROOT, "large-edit/large-edit.ts");
const H1_QUALIFICATION_RELATIVE_PATH = "ratchets/h1-emit-qualification.v1.json";
const H2_TRANSITION_RELATIVE_PATH = "ratchets/h2-profile-transition.v1.json";
const TRUSTED_H2_0A_COMMIT = "5d50819f39c8c36f9b8b3e420d5e96c779737578";
const EXPECTED_RUSTC = "rustc 1.93.0 (254b59607 2026-01-19)";
const EXPECTED_NODE = "v25.2.1";
const EXPECTED_EMIT_CASE =
  "typescript-6.0.3/compiler/esmNoSynthesizedDefault.ts#module%3Dpreserve";

const AUTHORITIES = Object.freeze([
  "ratchets/h2-owner-inventory.v1.json",
  "ratchets/h2-candidate-dispositions.v1.json",
  H2_TRANSITION_RELATIVE_PATH,
  H1_QUALIFICATION_RELATIVE_PATH,
  "ratchets/h1-noemit-performance.v1.json",
  "ratchets/h1-emit-performance.v1.json",
  "ratchets/l1-incremental-parser-performance.v1.json",
  FIXTURE_MANIFEST_RELATIVE_PATH,
  QUALIFICATION_EXAMPLE_RELATIVE_PATH,
]);

const RUNTIME_PREFIXES = Object.freeze([
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
]);
const RUNTIME_EXACT = new Set([
  "Cargo.lock",
  "Cargo.toml",
  "rust-toolchain.toml",
  ".node-version",
]);

const H2_RUNTIME_SLICES = Object.freeze([
  "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e",
  "H2.2a", "H2.2b", "H2.2c", "H2.2d",
  "H2.3a", "H2.3b", "H2.3c", "H2.3d",
  "H2.4a", "H2.4b",
  "H2.5a", "H2.5b", "H2.5c", "H2.5d", "H2.5e", "H2.5f", "H2.5g", "H2.5h",
  "H2.6a", "H2.6b", "H2.6c",
  "H2.7a", "H2.7b", "H2.7c", "H2.7d", "H2.7e",
  "H2.8a", "H2.8b", "H2.8c", "H2.8d", "H2.8e",
  "H2.9",
]);

const POSITIVE_ACTIVITY_FIELDS = Object.freeze([
  "emit_session_constructions",
  "output_plan_constructions",
  "emit_resolver_borrows",
  "script_transformer_list_constructions",
  "transform_typescript_constructions",
  "transform_class_fields_constructions",
  "transform_ecmascript_module_constructions",
  "transform_context_constructions",
  "printer_constructions",
  "javascript_artifact_creations",
  "output_sink_write_attempts",
  "output_sink_failures",
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

function exactKeys(value, keys) {
  return (
    value !== null &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    JSON.stringify(Object.keys(value).sort()) === JSON.stringify([...keys].sort())
  );
}

function rounded(value) {
  return Number(value.toFixed(9));
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
  return {
    path: relativePath,
    sha256: sha256(fs.readFileSync(path.join(WORKSPACE, relativePath))),
  };
}

function runtimePathsAt(commit = undefined) {
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

function runtimeFingerprintAt(commit = undefined) {
  const paths = runtimePathsAt(commit);
  const hash = crypto.createHash("sha256");
  let bytes = 0;
  for (const relativePath of paths) {
    const content = commit
      ? execFileSync("git", ["show", `${commit}:${relativePath}`], {
          cwd: WORKSPACE,
          maxBuffer: 64 * 1024 * 1024,
        })
      : fs.readFileSync(path.join(WORKSPACE, relativePath));
    bytes += content.length;
    hash.update(relativePath);
    hash.update("\0");
    hash.update(sha256(content));
    hash.update("\0");
  }
  return { files: paths.length, bytes, sha256: hash.digest("hex") };
}

function runner() {
  requireCondition(
    process.platform === "darwin" && process.arch === "arm64",
    "H2 baseline evidence may only be minted on approved macOS arm64",
  );
  requireCondition(
    process.version === EXPECTED_NODE && command("rustc", ["--version"]) === EXPECTED_RUSTC,
    "H2 baseline evidence requires the frozen Node and Rust toolchain",
  );
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

function policy() {
  return {
    comparison: "same-approved-runner-alternating-h2.0a-candidate",
    trusted_h2_0a_commit: TRUSTED_H2_0A_COMMIT,
    minimum_warm_pairs: 7,
    order: "alternating-ab-ba",
    no_emit: {
      warm_median_wall_ratio: 1.1,
      warm_p95_wall_ratio: 1.15,
      peak_rss_ratio: 1.1,
      allocation_count_ratio: 1.02,
      allocated_bytes_ratio: 1.03,
      parsed_documents_ratio: 1,
      bound_documents_ratio: 1,
      full_text_copies_ratio: 1,
      full_text_bytes_copied_ratio: 1,
    },
    h1_emit: {
      warm_median_wall_ratio: 1.1,
      warm_p95_wall_ratio: 1.2,
      peak_rss_ratio: 1.1,
      absolute_peak_rss_bytes: 268_435_456,
    },
    l1_edit: {
      fresh_candidate_base_operation_ratio: 1.1,
      incremental_candidate_base_operation_ratio: 1.1,
      candidate_peak_rss_ratio: 1.1,
      candidate_allocation_count_ratio: 1.05,
      candidate_allocated_bytes_ratio: 1.1,
      incremental_fresh_operation_ratio: 0.9,
      incremental_fresh_allocation_ratio: 0.9,
      incremental_fresh_allocated_bytes_ratio: 1.15,
      incremental_median_operation_seconds: 0.05,
      incremental_p95_operation_seconds: 0.075,
      incremental_peak_rss_bytes: 134_217_728,
      minimum_reused_nodes: 190_000,
      maximum_freshly_parsed_nodes: 128,
    },
    binary: {
      compiler_size_ratio: 1.05,
      h0_observer_size_ratio: 1.05,
      l1_observer_size_ratio: 1.05,
    },
    exact_outputs_required: true,
    h2_runtime_activity_before_admission: 0,
    moving_hosted_runner_can_mint_or_relax: false,
  };
}

function snapshotTree(root) {
  const snapshot = new Map();
  const visit = (directory) => {
    for (const entry of fs
      .readdirSync(directory, { withFileTypes: true })
      .sort((left, right) => left.name.localeCompare(right.name))) {
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
  return [...names].filter((name) => before.get(name) !== after.get(name)).length;
}

function parseRss(stderr, label) {
  const match = /^\s*(\d+)\s+maximum resident set size$/mu.exec(stderr);
  requireCondition(match !== null, `cannot parse maximum RSS for ${label}:\n${stderr}`);
  return Number(match[1]);
}

function spawnTimed(binary, args, cwd, label) {
  const started = process.hrtime.bigint();
  const result = spawnSync("/usr/bin/time", ["-l", binary, ...args], {
    cwd,
    encoding: "utf8",
    env: {
      ...process.env,
      CARGO_BUILD_JOBS: "2",
      CARGO_INCREMENTAL: "0",
      RUSTC_WRAPPER: "",
      NODE_PATH: "",
      NODE_OPTIONS: "",
    },
    maxBuffer: 64 * 1024 * 1024,
  });
  return {
    result,
    wall_seconds: rounded(Number(process.hrtime.bigint() - started) / 1_000_000_000),
    max_rss_bytes: parseRss(result.stderr, label),
  };
}

function noEmitWorkloads() {
  const manifest = JSON.parse(
    fs.readFileSync(path.join(WORKSPACE, FIXTURE_MANIFEST_RELATIVE_PATH), "utf8"),
  );
  requireCondition(
    manifest.schema === 1 && manifest.status === "frozen",
    "invalid L0 fixture authority",
  );
  const workloads = manifest.workloads.filter((entry) => entry.args !== null);
  requireCondition(
    JSON.stringify(workloads.map((entry) => entry.id)) ===
      JSON.stringify(["explicit-root", "project", "scale"]),
    "H2 no-emit workload set changed",
  );
  return workloads;
}

function parseH0Observation(output, label) {
  requireCondition(
    output.schema === 2 && output.exit_code === 0,
    `${label} did not produce the frozen H0 observation`,
  );
  return {
    allocations: output.allocations,
    deallocations: output.deallocations,
    reallocations: output.reallocations,
    bytes_allocated: output.bytes_allocated,
    bytes_deallocated: output.bytes_deallocated,
    bytes_reallocated: output.bytes_reallocated,
    work: output.work,
    h1_no_emit: output.h1_no_emit,
  };
}

function measureNoEmit(binary, workload, side) {
  const directory = path.join(FIXTURE_ROOT, workload.id);
  const before = snapshotTree(directory);
  const timed = spawnTimed(binary, workload.args, directory, `${workload.id}.${side}`);
  const after = snapshotTree(directory);
  requireCondition(
    timed.result.status === 0,
    `${workload.id}.${side} failed:\n${timed.result.stdout}\n${timed.result.stderr}`,
  );
  const observed = parseH0Observation(
    JSON.parse(timed.result.stdout.trim()),
    `${workload.id}.${side}`,
  );
  return {
    wall_seconds: timed.wall_seconds,
    max_rss_bytes: timed.max_rss_bytes,
    ...observed,
    output_writes: changedFileCount(before, after),
  };
}

function noEmitSummary(samples) {
  const warm = samples.slice(1);
  const values = (field) => warm.map((sample) => sample[field]);
  return {
    cold_wall_seconds: samples[0].wall_seconds,
    cold_max_rss_bytes: samples[0].max_rss_bytes,
    warm_median_wall_seconds: rounded(median(values("wall_seconds"))),
    warm_p95_wall_seconds: rounded(percentile(values("wall_seconds"), 95)),
    peak_rss_bytes: Math.max(...samples.map((sample) => sample.max_rss_bytes)),
    warm_median_allocations: median(values("allocations")),
    warm_median_bytes_allocated: median(values("bytes_allocated")),
    max_output_writes: Math.max(...samples.map((sample) => sample.output_writes)),
    work: samples[0].work,
    h1_no_emit: samples[0].h1_no_emit,
  };
}

function noEmitRatios(base, candidate) {
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
    bound_documents_ratio: ratio(
      candidate.work.bound_documents,
      base.work.bound_documents,
    ),
    full_text_copies_ratio: ratio(
      candidate.work.full_text_copies,
      base.work.full_text_copies,
    ),
    full_text_bytes_copied_ratio: ratio(
      candidate.work.full_text_bytes_copied,
      base.work.full_text_bytes_copied,
    ),
  };
}

function variance(pairs, projection) {
  const warm = pairs.slice(1);
  const base = warm.map((pair) => projection(pair.base));
  const candidate = warm.map((pair) => projection(pair.candidate));
  const paired = candidate.map((value, index) => value / base[index]);
  return {
    base_warm_p95_over_median: ratio(percentile(base, 95), median(base)),
    candidate_warm_p95_over_median: ratio(
      percentile(candidate, 95),
      median(candidate),
    ),
    base_warm_relative_range: relativeRange(base),
    candidate_warm_relative_range: relativeRange(candidate),
    paired_ratio_min: rounded(Math.min(...paired)),
    paired_ratio_max: rounded(Math.max(...paired)),
  };
}

function compareNoEmit(workload, binaries, pairCount) {
  const pairs = [];
  for (let ordinal = 0; ordinal < pairCount; ordinal += 1) {
    const order = ordinal % 2 === 0 ? "ab" : "ba";
    const observations = {};
    for (const side of order === "ab" ? ["base", "candidate"] : ["candidate", "base"]) {
      observations[side] = measureNoEmit(binaries[side], workload, side);
    }
    pairs.push({ ordinal, order, ...observations });
  }
  const baseSummary = noEmitSummary(pairs.map((pair) => pair.base));
  const candidateSummary = noEmitSummary(pairs.map((pair) => pair.candidate));
  const ratios = noEmitRatios(baseSummary, candidateSummary);
  const ceilings = policy().no_emit;
  const qualified =
    Object.entries(ratios).every(([name, value]) => value <= ceilings[name]) &&
    baseSummary.max_output_writes === 0 &&
    candidateSummary.max_output_writes === 0 &&
    Object.values(candidateSummary.h1_no_emit).every((value) => value === 0);
  return {
    id: workload.id,
    arguments: workload.args,
    workload_sha256: workload.workload_sha256,
    pairs,
    base_summary: baseSummary,
    candidate_summary: candidateSummary,
    variance: variance(pairs, (sample) => sample.wall_seconds),
    ratios,
    qualified,
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

function emitWorkload() {
  const artifact = JSON.parse(
    fs.readFileSync(path.join(WORKSPACE, H1_QUALIFICATION_RELATIVE_PATH), "utf8"),
  );
  requireCondition(
    artifact.kind === "h1-emit-qualification" && artifact.status === "qualified",
    "H1 emit qualification is not frozen",
  );
  const selected = artifact.compatible_cases.find((entry) => entry.id === EXPECTED_EMIT_CASE);
  requireCondition(selected !== undefined, "H1 compatible emit case is missing");
  const write = selected.observation.writes[0];
  return {
    case_id: selected.id,
    source: selected.source,
    files: selected.virtual_files,
    config_utf8_base64: selected.cli_projection.config_utf8_base64,
    config_utf8_sha256: selected.cli_projection.config_utf8_sha256,
    config_utf8_bytes: selected.cli_projection.config_utf8_bytes,
    arguments: ["--pretty", "false", "-p", "tsconfig.json"],
    expected_exit_code: selected.cli_projection.expected_exit_code,
    expected_diagnostic_codes: selected.cli_projection.expected_diagnostic_codes,
    expected_stdout: expectedCliStdout(selected),
    expected_output: {
      path: write.path,
      utf8_sha256: write.materialized_utf8_sha256,
      utf8_bytes: write.materialized_utf8_bytes,
    },
  };
}

function emitWorkloadRecord(workload) {
  return {
    case_id: workload.case_id,
    source: workload.source,
    source_files: workload.files.length,
    source_utf8_bytes: workload.files.reduce((sum, file) => sum + file.utf8_bytes, 0),
    config_utf8_sha256: workload.config_utf8_sha256,
    config_utf8_bytes: workload.config_utf8_bytes,
    arguments: workload.arguments,
    expected_exit_code: workload.expected_exit_code,
    expected_diagnostic_codes: workload.expected_diagnostic_codes,
    expected_output: workload.expected_output,
  };
}

function materializeEmit(workload, root) {
  fs.mkdirSync(root, { recursive: true });
  for (const file of workload.files) {
    const relative = file.path.replace(/^\//u, "");
    const output = path.join(root, ...relative.split("/"));
    fs.mkdirSync(path.dirname(output), { recursive: true });
    const bytes = Buffer.from(file.utf8_base64, "base64");
    requireCondition(
      bytes.length === file.utf8_bytes && sha256(bytes) === file.utf8_sha256,
      `invalid H1 source ${file.path}`,
    );
    fs.writeFileSync(output, bytes);
  }
  const config = Buffer.from(workload.config_utf8_base64, "base64");
  requireCondition(
    config.length === workload.config_utf8_bytes &&
      sha256(config) === workload.config_utf8_sha256,
    "invalid H1 config",
  );
  fs.writeFileSync(path.join(root, "tsconfig.json"), config);
}

function measureEmit(binary, workload, measurementRoot, label) {
  const root = path.join(measurementRoot, label);
  materializeEmit(workload, root);
  const canonicalRoot = fs.realpathSync(root).split(path.sep).join("/");
  const timed = spawnTimed(binary, workload.arguments, root, label);
  requireCondition(
    timed.result.status === workload.expected_exit_code,
    `${label} exit mismatch:\n${timed.result.stdout}\n${timed.result.stderr}`,
  );
  requireCondition(
    timed.result.stdout.replaceAll(canonicalRoot, "") === workload.expected_stdout,
    `${label} diagnostic output differs`,
  );
  const outputPath = path.join(root, workload.expected_output.path.replace(/^\//u, ""));
  const output = fs.readFileSync(outputPath);
  requireCondition(
    output.length === workload.expected_output.utf8_bytes &&
      sha256(output) === workload.expected_output.utf8_sha256,
    `${label} JavaScript differs`,
  );
  return {
    wall_seconds: timed.wall_seconds,
    max_rss_bytes: timed.max_rss_bytes,
    exit_code: timed.result.status,
    diagnostic_count: workload.expected_diagnostic_codes.length,
    output_files: 1,
    output_utf8_bytes: output.length,
    output_sha256: sha256(output),
  };
}

function emitSummary(samples) {
  const warm = samples.slice(1);
  return {
    cold_wall_seconds: samples[0].wall_seconds,
    cold_max_rss_bytes: samples[0].max_rss_bytes,
    warm_median_wall_seconds: rounded(median(warm.map((sample) => sample.wall_seconds))),
    warm_p95_wall_seconds: rounded(
      percentile(warm.map((sample) => sample.wall_seconds), 95),
    ),
    peak_rss_bytes: Math.max(...samples.map((sample) => sample.max_rss_bytes)),
    exit_code: samples[0].exit_code,
    diagnostic_count: samples[0].diagnostic_count,
    output_files: samples[0].output_files,
    output_utf8_bytes: samples[0].output_utf8_bytes,
    output_sha256: samples[0].output_sha256,
  };
}

function compareEmit(workload, binaries, pairCount, measurementRoot) {
  const pairs = [];
  for (let ordinal = 0; ordinal < pairCount; ordinal += 1) {
    const order = ordinal % 2 === 0 ? "ab" : "ba";
    const observations = {};
    for (const side of order === "ab" ? ["base", "candidate"] : ["candidate", "base"]) {
      observations[side] = measureEmit(
        binaries[side],
        workload,
        measurementRoot,
        `${ordinal}-${order}-${side}`,
      );
    }
    pairs.push({ ordinal, order, ...observations });
  }
  const baseSummary = emitSummary(pairs.map((pair) => pair.base));
  const candidateSummary = emitSummary(pairs.map((pair) => pair.candidate));
  const ratios = {
    warm_median_wall_ratio: ratio(
      candidateSummary.warm_median_wall_seconds,
      baseSummary.warm_median_wall_seconds,
    ),
    warm_p95_wall_ratio: ratio(
      candidateSummary.warm_p95_wall_seconds,
      baseSummary.warm_p95_wall_seconds,
    ),
    peak_rss_ratio: ratio(candidateSummary.peak_rss_bytes, baseSummary.peak_rss_bytes),
  };
  const ceilings = policy().h1_emit;
  const exact = (summary) =>
    summary.exit_code === workload.expected_exit_code &&
    summary.output_files === 1 &&
    summary.output_utf8_bytes === workload.expected_output.utf8_bytes &&
    summary.output_sha256 === workload.expected_output.utf8_sha256;
  const qualified =
    ratios.warm_median_wall_ratio <= ceilings.warm_median_wall_ratio &&
    ratios.warm_p95_wall_ratio <= ceilings.warm_p95_wall_ratio &&
    ratios.peak_rss_ratio <= ceilings.peak_rss_ratio &&
    candidateSummary.peak_rss_bytes <= ceilings.absolute_peak_rss_bytes &&
    exact(baseSummary) &&
    exact(candidateSummary);
  return {
    workload: emitWorkloadRecord(workload),
    pairs,
    base_summary: baseSummary,
    candidate_summary: candidateSummary,
    variance: variance(pairs, (sample) => sample.wall_seconds),
    ratios,
    qualified,
  };
}

function parseL1Observation(mode, timed, label) {
  requireCondition(
    timed.result.status === 0,
    `${label} failed:\n${timed.result.stdout}\n${timed.result.stderr}`,
  );
  const output = JSON.parse(timed.result.stdout.trim());
  requireCondition(
    output.schema === 1 &&
      output.kind === "l1-incremental-parser-operation" &&
      output.mode === mode &&
      output.operation_nanoseconds > 0,
    `${label} produced an invalid L1 observation`,
  );
  return {
    process_wall_seconds: timed.wall_seconds,
    operation_seconds: rounded(output.operation_nanoseconds / 1_000_000_000),
    max_rss_bytes: timed.max_rss_bytes,
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

function measureL1(binary, mode, label) {
  return parseL1Observation(
    mode,
    spawnTimed(binary, [mode, L1_FIXTURE_PATH], WORKSPACE, label),
    label,
  );
}

function l1Summary(samples) {
  return {
    median_process_wall_seconds: rounded(
      median(samples.map((sample) => sample.process_wall_seconds)),
    ),
    median_operation_seconds: rounded(
      median(samples.map((sample) => sample.operation_seconds)),
    ),
    p95_operation_seconds: rounded(
      percentile(samples.map((sample) => sample.operation_seconds), 95),
    ),
    max_rss_bytes: Math.max(...samples.map((sample) => sample.max_rss_bytes)),
    median_allocations: median(samples.map((sample) => sample.allocations)),
    median_bytes_allocated: median(samples.map((sample) => sample.bytes_allocated)),
    source: samples[0].source,
    reuse: samples[0].reuse,
  };
}

function compareL1(workload, binaries, pairCount) {
  const pairs = [];
  for (let ordinal = 0; ordinal < pairCount; ordinal += 1) {
    const order = ordinal % 2 === 0 ? "ab" : "ba";
    const observations = {};
    const sides = order === "ab" ? ["base", "candidate"] : ["candidate", "base"];
    const modes = order === "ab" ? ["fresh", "incremental"] : ["incremental", "fresh"];
    for (const side of sides) {
      observations[side] = {};
      for (const mode of modes) {
        observations[side][mode] = measureL1(
          binaries[side],
          mode,
          `l1.${ordinal}.${side}.${mode}`,
        );
      }
    }
    pairs.push({ ordinal, order, ...observations });
  }
  const warm = pairs.slice(1);
  const summaries = {};
  for (const side of ["base", "candidate"]) {
    summaries[side] = {};
    for (const mode of ["fresh", "incremental"]) {
      summaries[side][mode] = l1Summary(warm.map((pair) => pair[side][mode]));
    }
  }
  const baseFresh = summaries.base.fresh;
  const baseIncremental = summaries.base.incremental;
  const candidateFresh = summaries.candidate.fresh;
  const candidateIncremental = summaries.candidate.incremental;
  const ratios = {
    fresh_candidate_base_operation_ratio: ratio(
      candidateFresh.median_operation_seconds,
      baseFresh.median_operation_seconds,
    ),
    incremental_candidate_base_operation_ratio: ratio(
      candidateIncremental.median_operation_seconds,
      baseIncremental.median_operation_seconds,
    ),
    candidate_peak_rss_ratio: ratio(
      Math.max(candidateFresh.max_rss_bytes, candidateIncremental.max_rss_bytes),
      Math.max(baseFresh.max_rss_bytes, baseIncremental.max_rss_bytes),
    ),
    candidate_allocation_count_ratio: ratio(
      candidateIncremental.median_allocations,
      baseIncremental.median_allocations,
    ),
    candidate_allocated_bytes_ratio: ratio(
      candidateIncremental.median_bytes_allocated,
      baseIncremental.median_bytes_allocated,
    ),
    incremental_fresh_operation_ratio: ratio(
      candidateIncremental.median_operation_seconds,
      candidateFresh.median_operation_seconds,
    ),
    incremental_fresh_allocation_ratio: ratio(
      candidateIncremental.median_allocations,
      candidateFresh.median_allocations,
    ),
    incremental_fresh_allocated_bytes_ratio: ratio(
      candidateIncremental.median_bytes_allocated,
      candidateFresh.median_bytes_allocated,
    ),
  };
  const ceilings = policy().l1_edit;
  const qualified =
    Object.entries(ratios).every(([name, value]) => value <= ceilings[name]) &&
    candidateIncremental.median_operation_seconds <=
      ceilings.incremental_median_operation_seconds &&
    candidateIncremental.p95_operation_seconds <=
      ceilings.incremental_p95_operation_seconds &&
    candidateIncremental.max_rss_bytes <= ceilings.incremental_peak_rss_bytes &&
    candidateIncremental.reuse.incremental === true &&
    candidateIncremental.reuse.full_parse_fallback === false &&
    candidateIncremental.reuse.nodes >= ceilings.minimum_reused_nodes &&
    candidateIncremental.reuse.freshly_parsed_nodes <=
      ceilings.maximum_freshly_parsed_nodes &&
    candidateFresh.reuse.incremental === false &&
    candidateFresh.reuse.nodes === 0 &&
    canonical(baseFresh.source) === canonical(candidateFresh.source) &&
    canonical(candidateFresh.source) === canonical(candidateIncremental.source);
  return {
    workload: {
      id: workload.id,
      workload_sha256: workload.workload_sha256,
      edit: workload.edit,
      fixture: {
        path: "target/h2/qualification-fixtures/large-edit/large-edit.ts",
        sha256: sha256(fs.readFileSync(L1_FIXTURE_PATH)),
      },
    },
    pairs,
    summaries,
    ratios,
    qualified,
  };
}

function validateActivity(activity, label, expectedPositive) {
  requireCondition(
    exactKeys(activity, ["positive", "runtime_slices"]) &&
      exactKeys(activity.positive, POSITIVE_ACTIVITY_FIELDS) &&
      exactKeys(activity.runtime_slices, H2_RUNTIME_SLICES),
    `${label} activity shape changed`,
  );
  requireCondition(
    canonical(activity.positive) === canonical(expectedPositive),
    `${label} positive activity differs: ${JSON.stringify(activity.positive)}`,
  );
  requireCondition(
    H2_RUNTIME_SLICES.every((slice) => activity.runtime_slices[slice] === 0),
    `${label} reached an unadmitted H2 runtime slice`,
  );
}

function zeroPositive() {
  return Object.fromEntries(POSITIVE_ACTIVITY_FIELDS.map((field) => [field, 0]));
}

function oneFilePositive() {
  return {
    emit_session_constructions: 1,
    output_plan_constructions: 1,
    emit_resolver_borrows: 1,
    script_transformer_list_constructions: 1,
    transform_typescript_constructions: 1,
    transform_class_fields_constructions: 1,
    transform_ecmascript_module_constructions: 1,
    transform_context_constructions: 1,
    printer_constructions: 1,
    javascript_artifact_creations: 1,
    output_sink_write_attempts: 1,
    output_sink_failures: 0,
  };
}

function faultPositive() {
  return {
    emit_session_constructions: 1,
    output_plan_constructions: 1,
    emit_resolver_borrows: 1,
    script_transformer_list_constructions: 2,
    transform_typescript_constructions: 2,
    transform_class_fields_constructions: 2,
    transform_ecmascript_module_constructions: 2,
    transform_context_constructions: 2,
    printer_constructions: 1,
    javascript_artifact_creations: 2,
    output_sink_write_attempts: 2,
    output_sink_failures: 1,
  };
}

function runActivityCli(binary, args, cwd, label, expectedPositive, expectedExit, expectedStdout) {
  const before = snapshotTree(cwd);
  const timed = spawnTimed(binary, ["cli", ...args], cwd, label);
  const after = snapshotTree(cwd);
  requireCondition(timed.result.status === 0, `${label} observer failed: ${timed.result.stderr}`);
  const output = JSON.parse(timed.result.stdout.trim());
  requireCondition(
    output.schema === 1 &&
      output.kind === "h2-cli-baseline-observation" &&
      output.exit_code === expectedExit &&
      output.stderr === "",
    `${label} command observation changed`,
  );
  const canonicalRoot = fs.realpathSync(cwd).split(path.sep).join("/");
  requireCondition(
    output.stdout.replaceAll(canonicalRoot, "") === expectedStdout,
    `${label} stdout changed`,
  );
  validateActivity(output.h2_activity, label, expectedPositive);
  return {
    id: label,
    exit_code: output.exit_code,
    stdout_utf8_bytes: Buffer.byteLength(output.stdout.replaceAll(canonicalRoot, "")),
    stdout_sha256: sha256(Buffer.from(output.stdout.replaceAll(canonicalRoot, ""), "utf8")),
    stderr_utf8_bytes: 0,
    output_writes: changedFileCount(before, after),
    allocations: output.allocations,
    work: output.work,
    h1_no_emit: output.h1_no_emit,
    h2_activity: output.h2_activity,
  };
}

function collectCanaries(binary, noEmit, emit, measurementRoot) {
  const noEmitCanaries = noEmit.map((workload) =>
    runActivityCli(
      binary,
      workload.arguments,
      path.join(FIXTURE_ROOT, workload.id),
      `no-emit:${workload.id}`,
      zeroPositive(),
      0,
      "",
    ),
  );
  for (const canary of noEmitCanaries) {
    requireCondition(
      canary.output_writes === 0 &&
        Object.values(canary.h1_no_emit).every((value) => value === 0),
      `${canary.id} violated the H0/H1 zero boundary`,
    );
  }

  const emitRoot = path.join(measurementRoot, "emit-canary");
  materializeEmit(emit, emitRoot);
  const emitCanary = runActivityCli(
    binary,
    emit.arguments,
    emitRoot,
    "h1-emit:compatible-case",
    oneFilePositive(),
    emit.expected_exit_code,
    emit.expected_stdout,
  );
  const outputPath = path.join(emitRoot, emit.expected_output.path.replace(/^\//u, ""));
  const outputBytes = fs.readFileSync(outputPath);
  requireCondition(
    outputBytes.length === emit.expected_output.utf8_bytes &&
      sha256(outputBytes) === emit.expected_output.utf8_sha256,
    "H1 emit activity canary output changed",
  );
  emitCanary.output = {
    path: emit.expected_output.path,
    bytes: outputBytes.length,
    sha256: sha256(outputBytes),
  };

  const outputFaults = [];
  for (const index of [0, 1]) {
    const result = spawnSync(binary, ["fault", String(index)], {
      cwd: WORKSPACE,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    });
    requireCondition(result.status === 0, `fault ${index} observer failed: ${result.stderr}`);
    const observation = JSON.parse(result.stdout.trim());
    requireCondition(
      observation.schema === 1 &&
        observation.kind === "h2-output-fault-observation" &&
        observation.failed_index === index &&
        observation.diagnostics.length === 1 &&
        observation.diagnostics[0].code === 5033 &&
        observation.emit_skipped === false &&
        observation.source_maps_present === false &&
        observation.emitted_files.length === 2 &&
        observation.successful_files.length === 1 &&
        observation.filesystem_attempts.length === 3,
      `fault ${index} observation changed`,
    );
    validateActivity(observation.h2_activity, `fault:${index}`, faultPositive());
    outputFaults.push(observation);
  }
  return {
    runtime_slice_order: H2_RUNTIME_SLICES,
    no_emit: noEmitCanaries,
    h1_emit: emitCanary,
    output_faults: outputFaults,
    runtime_activity_sum: 0,
    positive_controls_observed: true,
  };
}

function binaryRecord(filePath) {
  const bytes = fs.readFileSync(filePath);
  return { sha256: sha256(bytes), bytes: bytes.length };
}

function binaryComparison(base, candidate) {
  return {
    base_bytes: base.bytes,
    candidate_bytes: candidate.bytes,
    ratio: ratio(candidate.bytes, base.bytes),
  };
}

function validateHistoricalAuthorities() {
  const h1NoEmit = JSON.parse(
    fs.readFileSync(path.join(WORKSPACE, "ratchets/h1-noemit-performance.v1.json"), "utf8"),
  );
  const h1Emit = JSON.parse(
    fs.readFileSync(path.join(WORKSPACE, "ratchets/h1-emit-performance.v1.json"), "utf8"),
  );
  const l1 = JSON.parse(
    fs.readFileSync(
      path.join(WORKSPACE, "ratchets/l1-incremental-parser-performance.v1.json"),
      "utf8",
    ),
  );
  requireCondition(
    h1NoEmit.kind === "h1-noemit-performance" && h1NoEmit.status === "qualified",
    "historical H1 no-emit evidence is invalid",
  );
  requireCondition(
    h1Emit.kind === "h1-emit-performance" && h1Emit.status === "qualified",
    "historical H1 emit evidence is invalid",
  );
  requireCondition(
    l1.kind === "l1-incremental-parser-performance" && l1.status === "qualified",
    "historical L1 edit evidence is invalid",
  );
  for (const commit of [
    h1NoEmit.base.commit,
    h1NoEmit.candidate.commit,
    h1Emit.base.commit,
    h1Emit.candidate.commit,
    l1.base.commit,
    l1.candidate.commit,
  ]) {
    command("git", ["cat-file", "-e", `${commit}^{commit}`]);
  }
  return {
    h1_no_emit: pathHash("ratchets/h1-noemit-performance.v1.json"),
    h1_emit: pathHash("ratchets/h1-emit-performance.v1.json"),
    l1_edit: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
    interpretation: "immutable-historical-lineage; current runtime ownership transfers to H2",
  };
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
    Number.isInteger(pairCount) && pairCount >= policy().minimum_warm_pairs + 1,
    "H2 comparison requires one cold plus seven warm pairs",
  );
  const dirty = dirtyPaths().filter((entry) => entry !== EVIDENCE_RELATIVE_PATH);
  requireCondition(
    dirty.length === 0,
    `H2 comparison requires a clean candidate worktree: ${dirty.join(", ")}`,
  );
  const baseCommit = git("rev-parse", "--verify", `${baseRef}^{commit}`);
  const candidateCommit = git("rev-parse", "HEAD");
  requireCondition(
    baseCommit === TRUSTED_H2_0A_COMMIT,
    `baseline must be H2.0a merge ${TRUSTED_H2_0A_COMMIT}`,
  );
  requireCondition(baseCommit !== candidateCommit, "H2.0b candidate must differ from H2.0a");
  command("git", ["merge-base", "--is-ancestor", baseCommit, candidateCommit]);
  command("node", ["crates/oracle/l0-fixtures.mjs", "--check"]);
  command("node", ["crates/oracle/l0-fixtures.mjs", "--materialize", FIXTURE_ROOT]);
  const fixtureManifest = JSON.parse(
    fs.readFileSync(path.join(WORKSPACE, FIXTURE_MANIFEST_RELATIVE_PATH), "utf8"),
  );
  const l1Workload = fixtureManifest.workloads.find((entry) => entry.id === "large-edit");
  requireCondition(
    l1Workload !== undefined &&
      sha256(fs.readFileSync(L1_FIXTURE_PATH)) === l1Workload.files[0].sha256,
    "H2 L1 fixture differs from its authority",
  );
  const noEmit = noEmitWorkloads();
  const emit = emitWorkload();
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "tsc-rs-h2-baseline-"));
  const baseWorkspace = path.join(temporary, "base");
  const baseTarget = path.join(temporary, "base-target");
  const candidateCopies = path.join(temporary, "candidate");
  const emitMeasurements = path.join(temporary, "emit-measurements");
  const canaryMeasurements = path.join(temporary, "canaries");
  let worktreeAdded = false;
  try {
    fs.mkdirSync(candidateCopies, { recursive: true });
    const buildEnvironment = {
      ...process.env,
      CARGO_BUILD_JOBS: "2",
      CARGO_INCREMENTAL: "0",
      RUSTC_WRAPPER: "",
    };
    command(
      "cargo",
      [
        "build", "--release", "--locked", "-p", "tsc-rs-compiler",
        "--bin", "tsc-rs", "--example", "h0_qualification",
        "--example", "h2_baseline_qualification",
      ],
      { env: buildEnvironment },
    );
    command(
      "cargo",
      [
        "build", "--release", "--locked", "-p", "tsc-rs-syntax",
        "--example", "l1_incremental_qualification",
      ],
      { env: buildEnvironment },
    );
    const candidateSources = {
      compiler: path.join(WORKSPACE, "target/release/tsc-rs"),
      h0: path.join(WORKSPACE, "target/release/examples/h0_qualification"),
      h2: path.join(WORKSPACE, "target/release/examples/h2_baseline_qualification"),
      l1: path.join(WORKSPACE, "target/release/examples/l1_incremental_qualification"),
    };
    const candidateBinaries = {};
    for (const [name, source] of Object.entries(candidateSources)) {
      const output = path.join(candidateCopies, name);
      fs.copyFileSync(source, output);
      fs.chmodSync(output, 0o755);
      candidateBinaries[name] = output;
    }

    command("git", ["worktree", "add", "--detach", baseWorkspace, baseCommit]);
    worktreeAdded = true;
    commandAt(
      baseWorkspace,
      "cargo",
      [
        "build", "--release", "--locked", "-p", "tsc-rs-compiler",
        "--bin", "tsc-rs", "--example", "h0_qualification",
      ],
      { env: { ...buildEnvironment, CARGO_TARGET_DIR: baseTarget } },
    );
    commandAt(
      baseWorkspace,
      "cargo",
      [
        "build", "--release", "--locked", "-p", "tsc-rs-syntax",
        "--example", "l1_incremental_qualification",
      ],
      { env: { ...buildEnvironment, CARGO_TARGET_DIR: baseTarget } },
    );
    const baseBinaries = {
      compiler: path.join(baseTarget, "release/tsc-rs"),
      h0: path.join(baseTarget, "release/examples/h0_qualification"),
      l1: path.join(baseTarget, "release/examples/l1_incremental_qualification"),
    };

    const noEmitEvidence = noEmit.map((workload) =>
      compareNoEmit(
        workload,
        { base: baseBinaries.h0, candidate: candidateBinaries.h0 },
        pairCount,
      ),
    );
    const emitEvidence = compareEmit(
      emit,
      { base: baseBinaries.compiler, candidate: candidateBinaries.compiler },
      pairCount,
      emitMeasurements,
    );
    const l1Evidence = compareL1(
      l1Workload,
      { base: baseBinaries.l1, candidate: candidateBinaries.l1 },
      pairCount,
    );
    const canaries = collectCanaries(
      candidateBinaries.h2,
      noEmitEvidence,
      emit,
      canaryMeasurements,
    );

    const baseBinaryRecords = {
      compiler: binaryRecord(baseBinaries.compiler),
      h0_observer: binaryRecord(baseBinaries.h0),
      l1_observer: binaryRecord(baseBinaries.l1),
    };
    const candidateBinaryRecords = {
      compiler: binaryRecord(candidateBinaries.compiler),
      h0_observer: binaryRecord(candidateBinaries.h0),
      l1_observer: binaryRecord(candidateBinaries.l1),
      h2_observer: binaryRecord(candidateBinaries.h2),
    };
    const binaryStartup = {
      compiler: binaryComparison(baseBinaryRecords.compiler, candidateBinaryRecords.compiler),
      h0_observer: binaryComparison(baseBinaryRecords.h0_observer, candidateBinaryRecords.h0_observer),
      l1_observer: binaryComparison(baseBinaryRecords.l1_observer, candidateBinaryRecords.l1_observer),
      no_emit_cold_wall_ratio: ratio(
        noEmitEvidence[0].candidate_summary.cold_wall_seconds,
        noEmitEvidence[0].base_summary.cold_wall_seconds,
      ),
      h1_emit_cold_wall_ratio: ratio(
        emitEvidence.candidate_summary.cold_wall_seconds,
        emitEvidence.base_summary.cold_wall_seconds,
      ),
    };
    binaryStartup.qualified =
      binaryStartup.compiler.ratio <= policy().binary.compiler_size_ratio &&
      binaryStartup.h0_observer.ratio <= policy().binary.h0_observer_size_ratio &&
      binaryStartup.l1_observer.ratio <= policy().binary.l1_observer_size_ratio;

    const allQualified =
      noEmitEvidence.every((workload) => workload.qualified) &&
      emitEvidence.qualified &&
      l1Evidence.qualified &&
      binaryStartup.qualified &&
      canaries.runtime_activity_sum === 0 &&
      canaries.positive_controls_observed;
    const evidence = {
      schema: 1,
      kind: "h2-pre-runtime-baseline",
      status: allQualified ? "qualified" : "failed",
      phase: "H2.0b",
      typescript: {
        version: "6.0.3",
        source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
      },
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      authorities: AUTHORITIES.map(pathHash),
      historical_lineage: validateHistoricalAuthorities(),
      base: {
        commit: baseCommit,
        runtime_tree: runtimeFingerprintAt(baseCommit),
        binaries: baseBinaryRecords,
      },
      candidate: {
        commit: candidateCommit,
        runtime_tree: runtimeFingerprintAt(),
        binaries: candidateBinaryRecords,
      },
      measured_at_utc: new Date().toISOString(),
      runner: runner(),
      toolchain: {
        rustc: command("rustc", ["--version"]),
        node: process.version,
        cargo_build_jobs: 2,
        profile: "release",
        wall_observer: "node-hrtime-bigint",
        rss_observer: "bsd-time-l",
        allocation_observer: "stats_alloc-0.1.10-qualification-only",
      },
      policy: policy(),
      sampling: {
        pair_count: pairCount,
        warm_pair_count: pairCount - 1,
        cold_pair_ordinal: 0,
        order: "alternating-ab-ba",
      },
      no_emit: noEmitEvidence,
      h1_emit: emitEvidence,
      l1_edit: l1Evidence,
      binary_startup: binaryStartup,
      canaries,
      summary: {
        no_emit_workloads: noEmitEvidence.length,
        h1_emit_cases: 1,
        l1_edit_workloads: 1,
        output_fault_observations: canaries.output_faults.length,
        h2_runtime_slices: H2_RUNTIME_SLICES.length,
        h2_runtime_activity: canaries.runtime_activity_sum,
        runtime_admissions: 0,
        all_qualified: allQualified,
      },
      qualified: allQualified,
    };
    const semantic = { ...evidence };
    delete semantic.evidence_fingerprint_sha256;
    evidence.evidence_fingerprint_sha256 = sha256(canonical(semantic));
    return evidence;
  } finally {
    if (worktreeAdded) {
      try {
        command("git", ["worktree", "remove", "--force", baseWorkspace]);
      } catch {
        // Preserve the primary comparison error and prune below.
      }
    }
    fs.rmSync(temporary, { recursive: true, force: true });
    command("git", ["worktree", "prune"]);
  }
}

function validatePairOrder(pairs, pairCount, label) {
  requireCondition(Array.isArray(pairs) && pairs.length === pairCount, `${label} pair count`);
  for (const [ordinal, pair] of pairs.entries()) {
    requireCondition(
      pair.ordinal === ordinal && pair.order === (ordinal % 2 === 0 ? "ab" : "ba"),
      `${label} pair order changed`,
    );
  }
}

function validateEvidence(evidence, requireCurrent) {
  requireCondition(
    evidence.schema === 1 &&
      evidence.kind === "h2-pre-runtime-baseline" &&
      evidence.status === "qualified" &&
      evidence.phase === "H2.0b" &&
      evidence.typescript.version === "6.0.3" &&
      evidence.qualified === true,
    "invalid H2 baseline header",
  );
  requireCondition(
    evidence.base.commit === TRUSTED_H2_0A_COMMIT &&
      evidence.candidate.commit !== evidence.base.commit,
    "invalid H2 baseline commits",
  );
  command("git", ["merge-base", "--is-ancestor", evidence.base.commit, evidence.candidate.commit]);
  requireCondition(
    canonical(evidence.base.runtime_tree) === canonical(runtimeFingerprintAt(evidence.base.commit)),
    "H2 base runtime fingerprint changed",
  );
  if (requireCurrent) {
    command("git", ["merge-base", "--is-ancestor", evidence.candidate.commit, "HEAD"]);
    requireCondition(
      canonical(evidence.candidate.runtime_tree) === canonical(runtimeFingerprintAt()),
      "current runtime differs from the H2-qualified candidate",
    );
  }
  requireCondition(
    canonical(evidence.generator) === canonical(pathHash(GENERATOR_RELATIVE_PATH)) &&
      canonical(evidence.contract) === canonical(pathHash(CONTRACT_RELATIVE_PATH)) &&
      canonical(evidence.authorities) === canonical(AUTHORITIES.map(pathHash)),
    "H2 baseline generator, schema, or authority changed",
  );
  requireCondition(
    canonical(evidence.historical_lineage) === canonical(validateHistoricalAuthorities()),
    "H1/L1 historical lineage changed",
  );
  requireCondition(canonical(evidence.policy) === canonical(policy()), "H2 baseline policy changed");
  const pairCount = evidence.sampling.pair_count;
  requireCondition(
    pairCount >= policy().minimum_warm_pairs + 1 &&
      evidence.sampling.warm_pair_count === pairCount - 1 &&
      evidence.sampling.cold_pair_ordinal === 0 &&
      evidence.sampling.order === "alternating-ab-ba",
    "invalid H2 sampling contract",
  );
  requireCondition(
    evidence.no_emit.length === 3 && evidence.no_emit.every((entry) => entry.qualified),
    "H2 no-emit baseline is incomplete",
  );
  for (const entry of evidence.no_emit) validatePairOrder(entry.pairs, pairCount, entry.id);
  validatePairOrder(evidence.h1_emit.pairs, pairCount, "H1 emit");
  validatePairOrder(evidence.l1_edit.pairs, pairCount, "L1 edit");
  requireCondition(
    evidence.h1_emit.qualified &&
      evidence.l1_edit.qualified &&
      evidence.binary_startup.qualified,
    "H2 emit, edit, or binary baseline is unqualified",
  );
  requireCondition(
    canonical(evidence.canaries.runtime_slice_order) === canonical(H2_RUNTIME_SLICES) &&
      evidence.canaries.no_emit.length === 3 &&
      evidence.canaries.output_faults.length === 2 &&
      evidence.canaries.runtime_activity_sum === 0 &&
      evidence.canaries.positive_controls_observed === true,
    "H2 constructor/activity canary set is incomplete",
  );
  for (const entry of evidence.canaries.no_emit) {
    validateActivity(entry.h2_activity, entry.id, zeroPositive());
  }
  validateActivity(evidence.canaries.h1_emit.h2_activity, "H1 emit", oneFilePositive());
  for (const [index, fault] of evidence.canaries.output_faults.entries()) {
    requireCondition(fault.failed_index === index, "H2 fault index order changed");
    validateActivity(fault.h2_activity, `fault:${index}`, faultPositive());
  }
  requireCondition(
    evidence.summary.no_emit_workloads === 3 &&
      evidence.summary.h1_emit_cases === 1 &&
      evidence.summary.l1_edit_workloads === 1 &&
      evidence.summary.output_fault_observations === 2 &&
      evidence.summary.h2_runtime_slices === 37 &&
      evidence.summary.h2_runtime_activity === 0 &&
      evidence.summary.runtime_admissions === 0 &&
      evidence.summary.all_qualified === true,
    "H2 baseline summary changed",
  );
  const semantic = { ...evidence };
  delete semantic.evidence_fingerprint_sha256;
  requireCondition(
    evidence.evidence_fingerprint_sha256 === sha256(canonical(semantic)),
    "H2 baseline fingerprint mismatch",
  );
  const transition = JSON.parse(
    fs.readFileSync(path.join(WORKSPACE, H2_TRANSITION_RELATIVE_PATH), "utf8"),
  );
  requireCondition(
    transition.phase === "H2.0b-baseline-transition" &&
      transition.transitions[1].slice === "H2.0b" &&
      transition.transitions[1].state === "complete-evidence-only" &&
      transition.transitions[2].slice === "H2.1a" &&
      transition.transitions[2].state === "next" &&
      transition.summary.runtime_admissions === 0,
    "H2 profile transition does not close H2.0b",
  );
}

const arguments_ = process.argv.slice(2);
if (arguments_[0] === "--compare") {
  let baseline;
  let pairs = 8;
  for (let index = 1; index < arguments_.length; index += 1) {
    const argument = arguments_[index];
    if (argument === "--baseline") baseline = arguments_[++index];
    else if (argument === "--pairs") pairs = Number.parseInt(arguments_[++index], 10);
    else fail(`unexpected H2 baseline argument ${argument}`);
  }
  requireCondition(typeof baseline === "string", "--compare requires --baseline <H2.0a commit>");
  const evidence = compare(baseline, pairs);
  validateEvidence(evidence, true);
  fs.writeFileSync(EVIDENCE_PATH, `${JSON.stringify(evidence, null, 2)}\n`);
  process.stdout.write(
    `wrote ${EVIDENCE_RELATIVE_PATH}: noemit=${evidence.no_emit.length} emit=${evidence.h1_emit.qualified} l1=${evidence.l1_edit.qualified} faults=${evidence.canaries.output_faults.length} h2-activity=${evidence.summary.h2_runtime_activity}\n`,
  );
} else if (arguments_[0] === "--check" && arguments_.length === 1) {
  requireCondition(fs.existsSync(EVIDENCE_PATH), `missing ${EVIDENCE_RELATIVE_PATH}`);
  const evidence = JSON.parse(fs.readFileSync(EVIDENCE_PATH, "utf8"));
  validateEvidence(evidence, true);
  process.stdout.write(
    `H2.0b baseline is qualified: noemit=3 emit=1 l1=1 faults=2 slices=37 activity=0\n`,
  );
} else {
  fail(
    "usage: h2-baseline.mjs --compare --baseline <H2.0a commit> [--pairs N] | --check",
  );
}
