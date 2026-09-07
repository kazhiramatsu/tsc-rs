// Join the immutable D/E input and twice-observed TypeScript corpus.
// This generator reads no Rust results and does not run the TypeScript compiler.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "../..");
const generator = "crates/oracle/h2-7de-qualification.mjs";
const contract = ".github/ci/contracts/h2-7de-qualification.schema.json";
const target = "ratchets/h2-7de-qualification.v1.json";
const frozen = [
  ["ratchets/h2-7de-candidates.v1.json", "1af6d75acf8212135a0850c5ff09487a5589de4d0f825ff1f0e9bc8e3f0f141d"],
  ["ratchets/h2-7de-candidate-inputs.v1.json", "f2e078a6b6d10cd3c6df833584924c18e8f78fe98c1e41621c70e10e215a073a"],
  ["ratchets/h2-7de-observations.v1.json", "1a1681b2375d27d9012b06e29808aca72aa3e39d1dbc1536b80ba2aadf9e8ce2"],
];
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const bytes = relative => fs.readFileSync(path.join(root, relative));
const identity = relative => ({ path: relative, sha256: sha256(bytes(relative)) });
const rowHash = row => sha256(JSON.stringify(row));
const mode = process.argv[2];
assert.ok(mode === "--write" || mode === "--check", "use --write or --check");
assert.equal(process.versions.node, bytes(".node-version").toString().trim());
const [inventory, prepared, observations] = frozen.map(([name, hash]) => {
  assert.equal(sha256(bytes(name)), hash, `${name}: frozen payload changed`);
  return JSON.parse(bytes(name));
});
for (const artifact of [inventory, prepared, observations]) {
  assert.equal(artifact.typescript, "6.0.3");
  assert.equal(artifact.source_commit, "050880ce59e30b356b686bd3144efe24f875ebc8");
}
assert.equal(inventory.cases.length, 325);
assert.equal(prepared.cases.length, 325);
assert.equal(observations.cases.length, 323);
assert.equal(observations.repetitions, 2);
assert.equal(observations.summary.typescript_runs, 646);
assert.equal(observations.summary.deterministic_cases, 323);
assert.equal(observations.summary.runtime_admitted, 0);
// Verify only direct observation provenance, without a historical certificate walk.
for (const record of [inventory.generator, observations.generator, ...observations.inputs]) {
  assert.deepEqual(identity(record.path), record);
}
const index = rows => {
  const result = new Map(rows.map((row, index) => [row.case_id, { row, index }]));
  assert.equal(result.size, rows.length, "duplicate case identity");
  return result;
};
const inputById = index(prepared.cases), observationById = index(observations.cases);
assert.deepEqual(inventory.cases.map(row => row.case_id), prepared.cases.map(row => row.case_id));
assert.deepEqual(observations.cases.map(row => row.case_id), prepared.cases
  .filter(row => row.input.route === "whole-program").map(row => row.case_id));

function checkCompleteObservation(observation) {
  assert.deepEqual(Object.keys(observation).sort(), ["emit_refused", "emit_result", "exit_code",
    "program_source_order", "reported_diagnostics", "standard_libraries", "status_writes", "writes"]);
  assert.deepEqual(Object.keys(observation.emit_result).sort(),
    ["diagnostics", "emit_skipped", "emitted_files", "source_maps"]);
  for (const [index, write] of observation.writes.entries()) {
    assert.equal(write.index, index);
    assert.deepEqual(Object.keys(write).sort(), ["callback_utf8_base64", "callback_utf8_bytes",
      "data_build_info", "data_diagnostics", "data_keys", "data_present", "data_source_map_url_pos",
      "index", "kind", "materialized_utf8_base64", "materialized_utf8_bytes", "on_error_callback_present",
      "path", "source_files", "write_byte_order_mark"]);
    const callback = Buffer.from(write.callback_utf8_base64, "base64");
    assert.equal(callback.toString("base64"), write.callback_utf8_base64);
    assert.equal(callback.length, write.callback_utf8_bytes);
    const materialized = write.write_byte_order_mark ? Buffer.concat([Buffer.from([239, 187, 191]), callback]) : callback;
    assert.equal(materialized.toString("base64"), write.materialized_utf8_base64);
    assert.equal(materialized.length, write.materialized_utf8_bytes);
  }
  for (const map of observation.emit_result.source_maps ?? []) {
    assert.deepEqual(Object.keys(map).sort(), ["input_source_file_names", "source_map_json"]);
    assert.equal(JSON.parse(map.source_map_json).version, 3);
  }
}

const bands = ["H2.7d", "H2.7e"];
const cases = inventory.cases.map((candidate, candidateIndex) => {
  const input = inputById.get(candidate.case_id), observation = observationById.get(candidate.case_id);
  assert.ok(input);
  assert.deepEqual(input.row.source, candidate.source);
  assert.equal(rowHash(input.row), candidate.input_sha256);
  const source = bytes(`ts-tests/tests/cases/${candidate.suite}/${candidate.source.path}`);
  assert.equal(source.length, candidate.source.bytes);
  assert.equal(sha256(source), candidate.source.sha256);
  assert.equal(crypto.createHash("sha1").update(`blob ${source.length}\0`).update(source).digest("hex"), candidate.source.git_blob_sha1);
  assert.equal(candidate.disposition, "candidate-only");
  assert.equal(candidate.runtime_admitted, false);
  const membership = bands.filter(band => candidate.required_slices.includes(band));
  assert.ok(membership.length);
  const remaining = candidate.required_slices.filter(owner => !bands.includes(owner));
  const wholeProgram = input.row.input.route === "whole-program";
  assert.equal(!!observation, wholeProgram);
  if (observation) {
    assert.equal(observation.row.input_sha256, candidate.input_sha256);
    assert.deepEqual(observation.row.required_slices, candidate.required_slices);
    assert.equal(observation.row.disposition, "typescript-reference-only");
    assert.equal(observation.row.repetitions, 2);
    checkCompleteObservation(observation.row.typescript_observation);
  } else {
    assert.equal(input.row.input.route, "transpile-api");
    assert.ok(remaining.includes("H2.8c"));
  }
  return { case_id: candidate.case_id, suite: candidate.suite, source: candidate.source,
    bands: membership, required_slices: candidate.required_slices, remaining_slices: remaining,
    input_route: input.row.input.route,
    disposition: wholeProgram && !remaining.length ? "eligible-for-rust-comparison" : "deferred",
    candidate: { index: candidateIndex, sha256: rowHash(candidate) },
    input: { index: input.index, sha256: rowHash(input.row) },
    observation: observation ? { index: observation.index, sha256: rowHash(observation.row),
      typescript_observation_sha256: rowHash(observation.row.typescript_observation), repetitions: 2 } : null };
});
const countBand = rows => ({ candidates: rows.length,
  eligible: rows.filter(row => row.disposition === "eligible-for-rust-comparison").length,
  deferred: rows.filter(row => row.disposition === "deferred").length });
const summary = { union: countBand(cases),
  bands: Object.fromEntries(bands.map(band => [band, countBand(cases.filter(row => row.bands.includes(band)))])),
  intersection: countBand(cases.filter(row => row.bands.length === 2)),
  exclusive_groups: {
    d_only: countBand(cases.filter(row => row.bands.join() === "H2.7d")),
    e_only: countBand(cases.filter(row => row.bands.join() === "H2.7e")),
    d_and_e: countBand(cases.filter(row => row.bands.length === 2)),
  },
  observed_whole_program: 323, observed_eligible: cases.filter(row => row.disposition === "eligible-for-rust-comparison" && row.observation).length,
  observed_deferred: cases.filter(row => row.disposition === "deferred" && row.observation).length,
  unobserved_transpile_references: cases.filter(row => !row.observation).length,
  later_by_owner: Object.fromEntries([...new Set(cases.flatMap(row => row.remaining_slices))].sort()
    .map(owner => [owner, cases.filter(row => row.remaining_slices.includes(owner)).length])) };
assert.deepEqual(summary, {
  union: { candidates: 325, eligible: 291, deferred: 34 },
  bands: { "H2.7d": { candidates: 315, eligible: 283, deferred: 32 }, "H2.7e": { candidates: 13, eligible: 11, deferred: 2 } },
  intersection: { candidates: 3, eligible: 3, deferred: 0 },
  exclusive_groups: { d_only: { candidates: 312, eligible: 280, deferred: 32 }, e_only: { candidates: 10, eligible: 8, deferred: 2 }, d_and_e: { candidates: 3, eligible: 3, deferred: 0 } },
  observed_whole_program: 323, observed_eligible: 291, observed_deferred: 32, unobserved_transpile_references: 2,
  later_by_owner: { "H2.8a": 23, "H2.8b": 5, "H2.8c": 2, "H2.9": 4 },
});
const artifact = { schema: 1, kind: "h2-7de-qualification", status: "qualified-typescript-oracle",
  typescript: "6.0.3", source_commit: inventory.source_commit, repetitions: 2,
  generator: identity(generator), contract: identity(contract),
  inputs: [...frozen.map(([name]) => name), inventory.generator.path, observations.generator.path,
    "crates/oracle/vfs-directory-overlay.mjs", "vendor/typescript-6.0.3/lib/typescript.js",
    "vendor/typescript-6.0.3/lib/_tsc.js", ".node-version"].map(identity),
  selection_contract: "One union by original case ID: D315=283 eligible+32 later; E13=11 eligible+2 later; overlap3; union325=291 eligible+34 later. Eligible full-Program groups are D-only280, E-only8, D/E3. All input/owner additions and parent memberships remain in the frozen candidate records.",
  execution_contract: "Identity-preserving join of 325 frozen inputs and 323 frozen complete whole-Program TypeScript observations, each previously compared twice. This generator performs no fresh TypeScript execution and reads no Rust results. Row references retain all original inputs/options/roots/config/files/libraries and complete tuple bytes, metadata, map/list/status/exit, including 32 later whole-Program references. The 2 transpile references have no whole-Program observation. Eligibility is not Rust success or runtime admission; focused controls and hosted activation are outside this artifact.",
  cases, summary };
artifact.qualification_fingerprint_sha256 = rowHash(artifact);
const rendered = JSON.stringify(artifact, null, 2) + "\n";
assert.ok(!rendered.includes(root), "local workspace path escaped into artifact");
if (mode === "--write") fs.writeFileSync(path.join(root, target), rendered);
else assert.equal(bytes(target).toString(), rendered, "H2.7d/e qualification join is stale");
console.log("H2.7d/e TS join: union325, eligible291, deferred34; D315=283+32; E13=11+2; overlap3; no Rust qualification");
