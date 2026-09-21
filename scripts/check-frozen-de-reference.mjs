// Reproduce the original D/E reference without rewriting its frozen provenance.
// The mandatory current-tree comparison permits exactly four parent hash leaves.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { isDeepStrictEqual } from "node:util";
import { gunzipSync } from "node:zlib";

export const ROOT = path.resolve(import.meta.dirname, "..");
const MANIFEST = "scripts/frozen-de-reference/manifest.json";
const MANIFEST_SHA = "68d91a4a1d33cf8b17146e9b586c64dee7ccd8f758bbd7965b9d1248b0d85bfd";
export const PARENTS = Object.freeze([
  "ratchets/h2-candidate-dispositions.v1.json", "ratchets/h2-6c-qualification.v1.json",
  "ratchets/h2-7b-qualification.v1.json", "ratchets/h2-7c-qualification.v1.json",
]);
export const CHECK_ONLY = Object.freeze({
  "h2-6a-map-option-projection": "canonical",
  "h2-7de-candidates": "frozen-de-reference",
  "h2-7de-observations": "frozen-de-reference",
});
const TARGETS = Object.freeze({
  "h2-7de-candidates": "crates/oracle/h2-7de-candidates.mjs",
  "h2-7de-observations": "crates/oracle/h2-7de-observations.mjs",
  "bundle-plan": "scripts/observe-bundle-plan.mjs",
  "output-directory-corpus": "scripts/observe-output-directory-corpus.mjs",
});
export const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");

export function regularFile(root, name) {
  assert.ok(name && !path.isAbsolute(name) && !name.includes("\\")
    && name.split("/").every(part => part && part !== "." && part !== ".."), `invalid path: ${name}`);
  let file = root;
  for (const part of name.split("/")) {
    file = path.join(file, part);
    assert.ok(!fs.lstatSync(file).isSymbolicLink(), `symlink is not a reference copy: ${name}`);
  }
  assert.ok(fs.lstatSync(file).isFile(), `not a regular file: ${name}`);
  return fs.readFileSync(file);
}

export function checkProjection(current, frozen, actualHashes) {
  assert.ok(isDeepStrictEqual(current.inputs, frozen.inputs), "current D/E prepared inputs changed");
  const normalized = structuredClone(current.inventory);
  assert.deepEqual(normalized.inputs.map(row => row.path), frozen.inventory.inputs.map(row => row.path),
    "current D/E provenance path order changed");
  assert.equal(new Set(normalized.inputs.map(row => row.path)).size, normalized.inputs.length,
    "duplicate D/E provenance path");
  const changed = [];
  for (const [index, row] of normalized.inputs.entries()) {
    assert.equal(row.sha256, actualHashes.get(row.path), `stale current provenance: ${row.path}`);
    if (PARENTS.includes(row.path)) {
      const old = frozen.inventory.inputs[index].sha256;
      if (row.sha256 !== old) changed.push(row.path);
      row.sha256 = old;
    }
  }
  assert.ok(PARENTS.every(name => normalized.inputs.some(row => row.path === name)), "missing D/E parent");
  assert.ok(isDeepStrictEqual(normalized, frozen.inventory), "current D/E census changed beyond four parent hashes");
  return changed;
}

export function validateRegistry(order, available, checkOnly = CHECK_ONLY) {
  assert.deepEqual(checkOnly, CHECK_ONLY, "unknown check-only execution mode or entry");
  const names = [...order, ...Object.keys(checkOnly)];
  assert.equal(new Set(names).size, names.length, "duplicate ORDER/check-only entry");
  for (const name of names) assert.ok(available.includes(name), `ORDER DRIFT: missing ${name}.mjs`);
  for (const name of available) {
    if (!/^h2-[0-9]/.test(name) || /-(owner-controls|check-resume)$/.test(name)) continue;
    assert.ok(names.includes(name), `ORDER DRIFT: unregistered ${name}.mjs`);
  }
}

export function loadReference(root = ROOT) {
  const raw = regularFile(root, MANIFEST);
  assert.equal(sha256(raw), MANIFEST_SHA, "frozen D/E manifest changed");
  const manifest = JSON.parse(raw);
  const files = new Map();
  for (const row of manifest.files) {
    assert.ok(!files.has(row.path), `duplicate reference file: ${row.path}`);
    let bytes;
    if (row.snapshot) {
      assert.ok(PARENTS.includes(row.path), `unexpected historical file: ${row.path}`);
      const packed = regularFile(root, row.snapshot.path);
      assert.equal(sha256(packed), row.snapshot.sha256, `snapshot archive changed: ${row.path}`);
      bytes = gunzipSync(packed);
    } else {
      bytes = regularFile(root, row.path);
    }
    assert.equal(bytes.length, row.bytes, `reference length changed: ${row.path}`);
    assert.equal(sha256(bytes), row.sha256, `reference bytes changed: ${row.path}`);
    files.set(row.path, bytes);
  }
  assert.deepEqual(manifest.files.filter(row => row.snapshot).map(row => row.path).sort(), [...PARENTS].sort());
  return files;
}

async function currentProjection(files) {
  const { prepare, root } = await import("../crates/oracle/h2-7de-candidates.mjs");
  assert.equal(fs.realpathSync(root), fs.realpathSync(ROOT), "current generator root changed");
  const current = prepare();
  const frozen = {
    inventory: JSON.parse(files.get("ratchets/h2-7de-candidates.v1.json")),
    inputs: JSON.parse(files.get("ratchets/h2-7de-candidate-inputs.v1.json")),
  };
  const hashes = new Map(current.inventory.inputs.map(row => [row.path, sha256(regularFile(ROOT, row.path))]));
  const changed = checkProjection(current, frozen, hashes);
  console.log(`current D/E projection: 325 complete inputs and census unchanged; ${changed.length}/4 historical parent hashes differ`);
}

export function stageReference(directory, files) {
  assert.equal(fs.realpathSync(directory), directory, "reference root must resolve to itself");
  assert.deepEqual(fs.readdirSync(directory), [], "reference directory must be empty");
  for (const [name, bytes] of files) {
    const destination = path.join(directory, name);
    // Validate paths before creating files, even when a caller supplies a map.
    assert.ok(name && !path.isAbsolute(name) && !name.includes("\\")
      && name.split("/").every(part => part && part !== "." && part !== ".."));
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.writeFileSync(destination, bytes, { flag: "wx" });
  }
  verifyStaged(directory, files);
}

export function verifyStaged(directory, files) {
  assert.equal(fs.realpathSync(directory), directory, "reference root escaped");
  const actual = fs.readdirSync(directory, { recursive: true, withFileTypes: true });
  assert.ok(actual.every(entry => entry.isDirectory() || entry.isFile()), "reference tree contains a link or special file");
  assert.equal(actual.filter(entry => entry.isFile()).length, files.size, "reference file set changed");
  for (const [name, bytes] of files) {
    assert.equal(sha256(regularFile(directory, name)), sha256(bytes), `reference copy mutated: ${name}`);
  }
}

async function replay(targets) {
  for (const target of targets) assert.ok(Object.hasOwn(TARGETS, target), `unknown reference target: ${target}`);
  const files = loadReference();
  await currentProjection(files);
  if (!targets.length) return;
  // Copy vendor as well: Node realpath and TypeScript's default library root
  // make a vendor symlink observably different from an isolated reference.
  const directory = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), "tsrs-frozen-de-"));
  try {
    stageReference(directory, files);
    for (const target of targets) {
      console.log(`historical D/E reference: ${TARGETS[target]} --check (unchanged generator and frozen parents)`);
      const result = spawnSync(process.execPath, [TARGETS[target], "--check"], { cwd: directory, stdio: "inherit" });
      verifyStaged(directory, files);
      if (result.error) throw result.error;
      assert.equal(result.status, 0, `historical reference failed: ${target}; signal=${result.signal}`);
    }
    // Detect canonical changes during replay as well as copied-tree mutation.
    loadReference();
    await currentProjection(files);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}

async function main() {
  const [mode, ...args] = process.argv.slice(2);
  if (mode === "--walk-preflight" || mode === "--walk-checks") {
    assert.equal(args.length, 0);
    const frozen = [];
    for (const [name, execution] of Object.entries(CHECK_ONLY)) {
      if (execution === "canonical") {
        const result = spawnSync(process.execPath, [`crates/oracle/${name}.mjs`, "--check"], { cwd: ROOT, stdio: "inherit" });
        if (result.error) throw result.error;
        assert.equal(result.status, 0, `canonical check-only failed: ${name}`);
      } else {
        assert.equal(execution, "frozen-de-reference");
        frozen.push(name);
      }
    }
    await replay(mode === "--walk-checks" ? frozen : []);
    return;
  }
  if (mode === "--registry") {
    assert.ok(args.length, "ORDER list required");
    const available = fs.readdirSync(path.join(ROOT, "crates/oracle"), { withFileTypes: true })
      .filter(entry => entry.isFile() && entry.name.endsWith(".mjs")).map(entry => entry.name.slice(0, -4));
    validateRegistry(args, available);
    console.log(`coverage: ${args.length} ORDER scripts + ${Object.keys(CHECK_ONLY).length} explicit check-only scripts`);
    return;
  }
  if (mode === "--projection-check") {
    assert.equal(args.length, 0);
    await replay([]);
    return;
  }
  assert.equal(mode, "--check", "use --check <target>..., --projection-check, or --registry <ORDER>...");
  assert.ok(args.length, "explicit reference target required");
  await replay(args);
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  await main();
}
