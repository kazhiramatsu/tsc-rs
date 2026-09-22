// Exercise production reuse checks without launching the qualification CLI.
// Mutated observations below are controls, never newly qualified evidence.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import ts from "file:///Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/vendor/typescript-6.0.3/lib/typescript.js";

const ROOT = "/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep";
const GENERATOR = path.join(ROOT, "crates/oracle/h2-5g-qualification.mjs");
const LIBRARY_MARKER = "/vendor/typescript-6.0.3/lib/";
const CURRENT_LIBRARY = ts.normalizePath(path.dirname(ts.getDefaultLibFilePath({})));
const FOREIGN_LIBRARY = `/different-checkout${LIBRARY_MARKER.slice(0, -1)}`;
const qualification = JSON.parse(fs.readFileSync(
  path.join(ROOT, "ratchets/h2-5g-qualification.v1.json"), "utf8"));

async function loadGenerator() {
  const url = pathToFileURL(GENERATOR).href;
  let source = fs.readFileSync(GENERATOR, "utf8");
  const dispatcher = "validateRuntime();\nif (MODE === INTERNAL_CHECK_SHARD_MODE)";
  assert.equal(source.split(dispatcher).length, 2);
  source = source.slice(0, source.lastIndexOf(dispatcher));
  const location = "const GENERATOR_PATH = fileURLToPath(import.meta.url);";
  assert.equal(source.split(location).length, 2);
  source = source.replace(location,
    `const GENERATOR_PATH = fileURLToPath(${JSON.stringify(url)});`);
  source = source.replace(/from "(\.\.?\/[^"\n]+)"/g,
    (_, relative) => `from ${JSON.stringify(new URL(relative, url).href)}`);
  source += `
export { storedLibraryDiagnosticPathsReusable, storedCaseReusable, withFingerprint,
  preflightSuiteFixtures, buildSuite, CheckReceiptMiss, sha256 };
export function withReceiptAttempt(callback) {
  const previous = checkReceiptAttempt;
  checkReceiptAttempt = true;
  try { return callback(); }
  finally { checkReceiptAttempt = previous; }
}
`;
  return import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
}
const generator = await loadGenerator();
const affectedIds = [
  "typescript-6.0.3/compiler/variableDeclarationInStrictMode1.ts#default",
  "typescript-6.0.3/conformance/types/members/duplicateNumericIndexers.ts#default",
  "typescript-6.0.3/conformance/types/members/objectTypeHidingMembersOfExtendedObject.ts#default",
  "typescript-6.0.3/conformance/types/members/objectTypeWithStringIndexerHidingObjectIndexer.ts#default",
];
const affected = affectedIds.map(id => {
  const row = qualification.cases.find(row => row.case_id === id);
  assert.ok(row, id);
  return row;
});
function seal(record) {
  delete record.typescript_observation.run_fingerprint_sha256;
  delete record.case_fingerprint_sha256;
  record.typescript_observation = generator.withFingerprint(
    record.typescript_observation, "run_fingerprint_sha256");
  record.typescript_run_fingerprints = Array(2).fill(
    record.typescript_observation.run_fingerprint_sha256);
  return generator.withFingerprint(record, "case_fingerprint_sha256");
}
function withLibrary(record, directory) {
  const copy = structuredClone(record);
  let changed = 0;
  for (const diagnostic of [
    ...copy.typescript_observation.reported_diagnostics,
    ...copy.typescript_observation.emit_result.diagnostics,
  ]) {
    if (diagnostic.file?.includes(LIBRARY_MARKER)) {
      diagnostic.file = `${directory}/${path.posix.basename(diagnostic.file)}`;
      changed++;
    }
  }
  assert.ok(changed > 0);
  return seal(copy);
}
function reuseArguments(record) {
  // The four regression cases have no config or symlink indirection. Their
  // recorded input bytes supply independent fixture content to the real guard.
  assert.equal(record.input.virtual_config, null);
  assert.deepEqual(record.input.vfs_symlinks, []);
  const units = [];
  for (const file of record.input.files) {
    units[file.unit] = {
      name: file.path,
      text: Buffer.from(file.utf8_base64, "base64").toString("utf8"),
      file_options: [],
    };
  }
  const loaded = { source: record.source, units, virtualConfig: null };
  const row = {
    selection_origin: record.selection_origin,
    expansion_case: record.expansion_case,
  };
  const settings = new Map(record.input.settings.map(({ name, value }) => [name, value]));
  const selection = {
    vfs_write_order: record.input.files.map(file => file.unit),
    program_root_unit_ids: record.input.roots.map(root => {
      const file = record.input.files.find(file => file.path === root);
      assert.ok(file, root);
      return file.unit;
    }),
  };
  return [record.suite, row, loaded, settings, selection];
}

const results = affected.map(row => ({ case_id: row.case_id, reusable: generator.storedCaseReusable(row, ...reuseArguments(row)) }));
assert.equal(results.length, 4);
assert.ok(results.every(row => row.reusable === false));
console.log(JSON.stringify({ qualified: false, artifact_sha256: generator.sha256(fs.readFileSync(path.join(ROOT, "ratchets/h2-5g-qualification.v1.json"))), meaning: "The four actual unchanged stored observations that caused r526 failure are rejected by the live repaired guard before the official writer runs.", results }, null, 2));
