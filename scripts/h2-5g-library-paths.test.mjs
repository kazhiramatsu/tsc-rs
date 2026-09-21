// Exercise production reuse checks without launching the qualification CLI.
// Mutated observations below are controls, never newly qualified evidence.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
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

test("H2.5g reuse rejects the four foreign-library observations without rewriting them", () => {
  let diagnostics = 0;
  for (const original of affected) {
    const args = reuseArguments(original);
    const foreign = withLibrary(original, FOREIGN_LIBRARY);
    const unchanged = structuredClone(foreign);
    assert.equal(generator.storedCaseReusable(foreign, ...args), false, original.case_id);
    assert.deepEqual(foreign, unchanged);
    const current = withLibrary(original, CURRENT_LIBRARY);
    assert.equal(generator.storedCaseReusable(current, ...args), true, original.case_id);
    diagnostics += current.typescript_observation.reported_diagnostics
      .filter(row => row.file?.startsWith(`${CURRENT_LIBRARY}/`)).length;
  }
  assert.equal(diagnostics, 17);
});

test("H2.5g both diagnostic channels invalidate foreign-library reuse", () => {
  const record = withLibrary(affected[0], CURRENT_LIBRARY);
  const fixturePaths = new Set(record.input.files.map(file => file.path));
  record.typescript_observation.reported_diagnostics = [];
  record.typescript_observation.emit_result.diagnostics = [];
  assert.equal(generator.storedLibraryDiagnosticPathsReusable(record.typescript_observation, fixturePaths), true);
  for (const channel of [record.typescript_observation.reported_diagnostics,
    record.typescript_observation.emit_result.diagnostics]) {
    for (const file of [null, "/.src/a.ts", "/project/tsconfig.json", `${CURRENT_LIBRARY}/lib.es5.d.ts`]) {
      channel.push({ file });
      assert.equal(generator.storedLibraryDiagnosticPathsReusable(record.typescript_observation, fixturePaths), true);
      channel.pop();
    }
    for (const file of [`${FOREIGN_LIBRARY}/lib.es5.d.ts`,
      `${CURRENT_LIBRARY}-sibling/vendor/typescript-6.0.3/lib/lib.es5.d.ts`]) {
      channel.push({ file });
      assert.equal(generator.storedLibraryDiagnosticPathsReusable(record.typescript_observation, fixturePaths), false);
      channel.pop();
    }
  }
});

test("H2.5g fixture, symlink and config vendor names require matching fresh inputs", () => {
  const filename = `${FOREIGN_LIBRARY}/lib.es5.d.ts`;
  for (const kind of ["fixture", "symlink", "config"]) {
    const original = affected[0];
    const record = withLibrary(original, FOREIGN_LIBRARY);
    const args = reuseArguments(original);
    const loaded = args[2];
    const unit = record.input.files[0].unit;
    if (kind === "fixture") {
      record.input.files[0].path = filename;
      record.input.roots[0] = filename;
      loaded.units[unit].name = filename;
    } else if (kind === "symlink") {
      record.input.vfs_symlinks.push({ link_path: filename, target_path: record.input.files[0].path });
      loaded.units[unit].file_options.push({ name: "symlink", value: filename });
    } else {
      loaded.virtualConfig = { name: filename, text: "{}" };
      record.input.virtual_config = { path: filename, utf8_sha256: generator.sha256(Buffer.from("{}")) };
    }
    const sealed = seal(record);
    const unchanged = structuredClone(sealed);
    assert.equal(generator.storedCaseReusable(sealed, ...args), true, kind);
    assert.deepEqual(sealed, unchanged);
    assert.equal(generator.storedCaseReusable(sealed, ...reuseArguments(original)), false,
      `${kind} cannot self-exempt without matching fresh inputs`);
  }
});

test("H2.5g current library paths cannot bypass original input and fingerprint guards", () => {
  const original = affected[0];
  const args = reuseArguments(original);
  for (const [name, mutate] of Object.entries({
    suite: row => { row.suite = "different"; },
    origin: row => { row.selection_origin = "different"; },
    expansion: row => { row.expansion_case++; },
    source: row => { row.source.sha256 = "0".repeat(64); },
    cwd: row => { row.input.current_directory += "/changed"; },
    settings: row => { row.input.settings[0].value = "es5"; },
    roots: row => { row.input.roots[0] += "-changed"; },
    symlink: row => { row.input.vfs_symlinks.push({ link_path: "/link", target_path: "/other" }); },
    content: row => { row.input.files[0].utf8_sha256 = "0".repeat(64); },
    "forged fixture exemption": row => { row.input.files[0].path = `${FOREIGN_LIBRARY}/lib.es5.d.ts`; },
  })) {
    const record = withLibrary(original, CURRENT_LIBRARY);
    mutate(record);
    assert.equal(generator.storedCaseReusable(seal(record), ...args), false, name);
  }
  const badFingerprint = withLibrary(original, CURRENT_LIBRARY);
  badFingerprint.case_fingerprint_sha256 = "0".repeat(64);
  assert.equal(generator.storedCaseReusable(badFingerprint, ...args), false);
});


test("H2.5g a foreign-library receipt fails with the existing case miss term before observation", () => {
  const read = file => JSON.parse(fs.readFileSync(path.join(ROOT, file), "utf8"));
  const data = {
    compiler: {
      classification: read("vendor/typescript-6.0.3/compiler-profile-classification.v1.json"),
      expansion: read("vendor/typescript-6.0.3/test-suite-expansion.v1.json"),
    },
    conformance: {
      classification: read("vendor/typescript-6.0.3/conformance-profile-classification.v1.json"),
      expansion: read("vendor/typescript-6.0.3/conformance-suite-expansion.v1.json"),
    },
  };
  for (const original of affected) {
    const { classification, expansion } = data[original.suite];
    const selection = new Map([[original.case_id, original.selection_origin]]);
    const loaded = generator.preflightSuiteFixtures(original.suite,
      classification, expansion, selection, new Map());
    const args = [original.suite, classification, expansion, selection, loaded];
    const foreign = withLibrary(original, FOREIGN_LIBRARY);
    assert.throws(() => generator.withReceiptAttempt(() => generator.buildSuite(
      ...args, new Map([[original.case_id, foreign]]))), error =>
      error instanceof generator.CheckReceiptMiss && error.message === `case ${original.case_id}`);
    const current = withLibrary(original, CURRENT_LIBRARY);
    const rows = generator.withReceiptAttempt(() => generator.buildSuite(
      ...args, new Map([[original.case_id, current]])));
    assert.equal(rows.length, 1);
    assert.equal(rows[0], current);
  }
});


test("H2.5g malformed diagnostic arrays are not reusable", () => {
  for (const mutate of [
    observation => { delete observation.reported_diagnostics; },
    observation => { observation.reported_diagnostics = {}; },
    observation => { delete observation.emit_result.diagnostics; },
    observation => { observation.emit_result = null; },
  ]) {
    const record = withLibrary(affected[0], CURRENT_LIBRARY);
    mutate(record.typescript_observation);
    assert.equal(generator.storedCaseReusable(seal(record), ...reuseArguments(affected[0])), false);
  }
});
