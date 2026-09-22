// Exercise the actual generator's private checks without its CLI dispatcher.
// Census rows here are synthetic controls, never native conformance evidence.
import assert from "node:assert/strict";
import childProcess from "node:child_process";
import fs from "node:fs";
import { syncBuiltinESMExports } from "node:module";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = "/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep";
const GENERATOR = path.join(ROOT, "crates/oracle/h2-7a-witnesses.mjs");
const read = relative => JSON.parse(fs.readFileSync(path.join(ROOT, relative)));
const artifact = read("ratchets/h2-7a-witnesses.v1.json");
const qualification = new Map(read("ratchets/h2-6c-qualification.v1.json")
  .cases.map(row => [row.case_id, row]));

async function loadGenerator(workspace = ROOT) {
  const url = pathToFileURL(GENERATOR).href;
  let source = fs.readFileSync("/tmp/emitter-w5-stratum-repair-r520/final-candidate-h2-7a-witnesses.mjs", "utf8");
  const dispatcher = "try {\n  if (MODE === INTERNAL_OBSERVE_MODE)";
  assert.equal(source.split(dispatcher).length, 2);
  source = source.slice(0, source.lastIndexOf(dispatcher));
  const location = "const GENERATOR_PATH = fileURLToPath(import.meta.url);";
  assert.equal(source.split(location).length, 2);
  source = source.replace(location,
    `const GENERATOR_PATH = fileURLToPath(${JSON.stringify(pathToFileURL(
      path.join(workspace, "crates/oracle/h2-7a-witnesses.mjs")).href)});`);
  source = source.replace(/from "(\.\.?\/[^"\n]+)"/g,
    (_, relative) => `from ${JSON.stringify(new URL(relative, url).href)}`);
  source += "\nexport { currentStratumCensusMatches, ensureStratumCensus, parseCensusJsonl, readDivergencePool, prepareStaticContext, verifyM1Projection, verifyS2Projection, verifyTrackedM1Projection };\n";
  return import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
}

const generator = await loadGenerator();
const pool = generator.readDivergencePool();
const censusRows = pool.map(case_id => {
  const expected = qualification.get(case_id).typescript_observation;
  return {
    case_id,
    writes_exact: expected.writes.length,
    writes_missing: [], writes_diverging: [], writes_rust_only: [],
    reported_diagnostics: {
      rust: expected.reported_diagnostics.length,
      expected: expected.reported_diagnostics.length,
    },
    emit_skipped: expected.emit_result.emit_skipped,
    source_maps_count: expected.emit_result.source_maps?.length ?? 0,
  };
});
const censusText = rows => rows.map(row => JSON.stringify(row)).join("\n") + "\n";
const temporary = () => fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), "tsrs-stratum-test-"));

// Builtin mocks are scoped to synchronous calls and always restored. Nothing
// invokes Cargo or writes canonical target/ or ratchets/ in this test module.
function mockCensusProducer(callback, run) {
  const original = childProcess.execFileSync;
  childProcess.execFileSync = callback;
  syncBuiltinESMExports();
  try { return run(); }
  finally {
    childProcess.execFileSync = original;
    syncBuiltinESMExports();
  }
}

test("W5 current census accepts the complete frozen roster and rejects output regressions", () => {
  for (const id of artifact.stratum.case_ids) {
    assert.equal(generator.currentStratumCensusMatches(
      censusRows.find(row => row.case_id === id), qualification.get(id)), true, id);
  }
  const id = artifact.stratum.case_ids[0];
  const original = censusRows.find(row => row.case_id === id);
  const mutations = {
    "missing declaration": row => { row.writes_missing.push({ kind: "declaration", path: "/x.d.ts" }); row.writes_exact--; },
    "missing JavaScript": row => { row.writes_missing.push({ kind: "javascript", path: "/x.js" }); row.writes_exact--; },
    "differing bytes": row => row.writes_diverging.push({ path: "/x.js", bytes_equal: false, bom_equal: true }),
    "differing BOM": row => row.writes_diverging.push({ path: "/x.js", bytes_equal: true, bom_equal: false }),
    "unexpected output": row => row.writes_rust_only.push("/extra.js"),
    "write count": row => row.writes_exact++,
    "diagnostic count": row => row.reported_diagnostics.rust++,
    "forged equal diagnostic counts": row => { row.reported_diagnostics.rust++; row.reported_diagnostics.expected++; },
    "absent diagnostic counts": row => { delete row.reported_diagnostics; },
    "map count": row => row.source_maps_count++,
    "emit skipped": row => { row.emit_skipped = !row.emit_skipped; },
  };
  for (const [name, mutate] of Object.entries(mutations)) {
    const row = structuredClone(original);
    mutate(row);
    assert.equal(generator.currentStratumCensusMatches(row, qualification.get(id)), false, name);
  }
});

test("W5 census requires all 172 original pool identities exactly once", () => {
  const directory = temporary();
  try {
    const file = path.join(directory, "census.jsonl");
    fs.writeFileSync(file, censusText(censusRows));
    assert.equal(generator.parseCensusJsonl(file, pool).rows.length, 172);
    for (const [name, mutate] of Object.entries({
      "missing row": rows => rows.pop(),
      "duplicate row": rows => { rows[1] = rows[0]; },
      "different identity": rows => { rows[0].case_id += "-changed"; },
    })) {
      const rows = structuredClone(censusRows);
      mutate(rows);
      fs.writeFileSync(file, censusText(rows));
      assert.throws(() => generator.parseCensusJsonl(file, pool), /stratum census/, name);
    }
    fs.writeFileSync(file, "{invalid\n");
    assert.throws(() => generator.parseCensusJsonl(file, pool), /invalid census JSON/);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test("W5 producer refreshes on every call and refuses no-output, failure, and injection", async () => {
  const directory = temporary();
  const local = await loadGenerator(directory);
  const output = path.join(directory, "target/h2-7a/stratum-census/census.jsonl");
  const override = process.env.TSRS_H2_7A_STRATUM_CENSUS;
  delete process.env.TSRS_H2_7A_STRATUM_CENSUS;
  try {
    fs.mkdirSync(path.dirname(output), { recursive: true });
    let calls = 0;
    mockCensusProducer((command, args, options) => {
      calls++;
      assert.equal(command, "cargo");
      assert.deepEqual(args, ["test", "-p", "tsc-rs-compiler", "--test", "contracts", "probe_one_band_row", "--", "--ignored", "--nocapture"]);
      assert.equal(options.cwd, directory);
      assert.equal(fs.existsSync(output), false);
      assert.equal(fs.readFileSync(options.env.TSRS_H2_6C_PROBE_LIST, "utf8"), pool.join("\n") + "\n");
      assert.equal(options.env.TSRS_H2_6C_PROBE_OUT, path.dirname(output));
      fs.writeFileSync(output, censusText(censusRows));
    }, () => {
      fs.writeFileSync(output, "stale census\n");
      assert.equal(local.ensureStratumCensus(pool), output);
      assert.equal(local.ensureStratumCensus(pool), output);
      assert.equal(calls, 2);
    });
    mockCensusProducer(() => {}, () => {
      assert.throws(() => local.ensureStratumCensus(pool), /did not produce census/);
    });
    mockCensusProducer(() => { throw new Error("native producer failed"); }, () => {
      assert.throws(() => local.ensureStratumCensus(pool), /native producer failed/);
    });
    process.env.TSRS_H2_7A_STRATUM_CENSUS = output;
    mockCensusProducer(() => assert.fail("injected census must fail before Cargo"), () => {
      assert.throws(() => local.ensureStratumCensus(pool), /is not accepted/);
    });
  } finally {
    if (override === undefined) delete process.env.TSRS_H2_7A_STRATUM_CENSUS;
    else process.env.TSRS_H2_7A_STRATUM_CENSUS = override;
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test("W5 historical M1/S2 projections reject altered provenance and observations", () => {
  generator.verifyM1Projection(artifact.case_manifest.cases, artifact.observations, artifact.stratum, "test");
  generator.verifyS2Projection(artifact.case_manifest.cases, artifact.observations, artifact.m2_supplement, "test");
  for (const change of [
    value => { value.stratum.census_jsonl_sha256 = "0".repeat(64); },
    value => { value.stratum.case_ids[0] += "-changed"; },
    value => { value.stratum.selection_contract += " changed"; },
    value => { value.observations[0].observation_fingerprint_sha256 = "0".repeat(64); },
  ]) {
    const altered = structuredClone(artifact);
    change(altered);
    assert.throws(() => generator.verifyM1Projection(altered.case_manifest.cases,
      altered.observations, altered.stratum, "test"), /projection/);
  }
});

test("W5 freshly checked static context preserves signed history despite a different census hash", async () => {
  const directory = temporary();
  try {
    // Inputs are read through these links; the output target directory is
    // private. This is only static-context testing, not oracle observation.
    for (const name of [".node-version", "vendor", "docs", "crates", "ratchets", "ts-tests"]) {
      fs.symlinkSync(path.join(ROOT, name), path.join(directory, name));
    }
    const local = await loadGenerator(directory);
    const context = mockCensusProducer((command, args, options) => {
      assert.equal(command, "cargo");
      fs.writeFileSync(path.join(options.env.TSRS_H2_6C_PROBE_OUT, "census.jsonl"), censusText(censusRows));
    }, () => local.prepareStaticContext());
    assert.equal(context.caseSpecs.length, 120);
    assert.deepEqual(context.stratum, artifact.stratum);
    assert.notEqual(local.parseCensusJsonl(context.censusPath, pool).sha256,
      artifact.stratum.census_jsonl_sha256);
    local.verifyM1Projection(context.caseManifest.cases, artifact.observations, context.stratum, "fresh static context");
    local.verifyS2Projection(context.caseManifest.cases, artifact.observations, context.m2Supplement, "fresh static context");
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});
