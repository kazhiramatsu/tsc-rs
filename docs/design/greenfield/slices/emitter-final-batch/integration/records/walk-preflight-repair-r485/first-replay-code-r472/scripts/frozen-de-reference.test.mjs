// No TypeScript observations: exercise the refusal boundaries with original
// reference payloads and temporary copies. Canonical files are never mutated.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import {
  ROOT, PARENTS, CHECK_ONLY, sha256, checkProjection, validateRegistry,
  loadReference, stageReference, verifyStaged,
} from "./check-frozen-de-reference.mjs";

const read = name => JSON.parse(fs.readFileSync(path.join(ROOT, name)));
const frozen = {
  inputs: read("ratchets/h2-7de-candidate-inputs.v1.json"),
  inventory: read("ratchets/h2-7de-candidates.v1.json"),
};
const hashes = current => new Map(current.inventory.inputs.map(row => [row.path, row.sha256]));
const temporary = () => fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), "tsrs-de-test-"));

test("D/E projection accepts current provenance changes only at the four historical parents", () => {
  assert.deepEqual(checkProjection(frozen, frozen, hashes(frozen)), []);
  const current = structuredClone(frozen);
  for (const name of PARENTS) {
    current.inventory.inputs.find(row => row.path === name).sha256 = sha256(name);
    assert.equal(checkProjection(current, frozen, hashes(current)).length,
      current.inventory.inputs.filter(row => PARENTS.includes(row.path)
        && row.sha256 === sha256(row.path)).length);
  }
});

test("D/E projection refuses semantic, tuple, roster and provenance drift", () => {
  const mutations = {
    "option": value => { value.inputs.cases[0].effective_options.noCheck = true; },
    "root": value => { value.inputs.cases[0].input.roots.push("/changed.ts"); },
    "source": value => { value.inputs.cases[0].source.sha256 = "1".repeat(64); },
    "input row": value => { value.inputs.cases.pop(); },
    "owner": value => { value.inventory.cases[0].required_slices.push("H2.changed"); },
    "parent membership": value => { value.inventory.cases[0].parent_membership.push({ parent: "changed" }); },
    "source facts": value => { value.inventory.cases[0].source_facts.parse_diagnostic_units.push({ path: "changed" }); },
    "census row": value => { value.inventory.cases.pop(); },
    "summary": value => { value.inventory.summary.changed = 1; },
    "status": value => { value.inventory.status = "admitted"; },
    "contract": value => { value.inventory.selection_contract += "changed"; },
    "source commit": value => { value.inventory.source_commit = "1".repeat(40); },
    "fifth hash": value => { value.inventory.inputs[4].sha256 = "1".repeat(64); },
    "provenance removal": value => { value.inventory.inputs.shift(); },
    "provenance addition": value => { value.inventory.inputs.push({ path: "other", sha256: "1".repeat(64) }); },
    "provenance order": value => { value.inventory.inputs.reverse(); },
    "provenance duplicate": value => { value.inventory.inputs[1] = value.inventory.inputs[0]; },
    "provenance extra field": value => { value.inventory.inputs[0].accepted = true; },
  };
  for (const [name, mutate] of Object.entries(mutations)) {
    const current = structuredClone(frozen);
    mutate(current);
    assert.throws(() => checkProjection(current, frozen, hashes(current)), undefined, name);
  }
  const current = structuredClone(frozen);
  current.inventory.inputs[0].sha256 = "1".repeat(64);
  assert.throws(() => checkProjection(current, frozen, hashes(frozen)), /stale current provenance/);
});

test("walk registry rejects unknown, duplicate, missing and overlapping scripts", () => {
  const order = ["h1-owner-inventory", "h2-1a-qualification"];
  const available = [...order, ...Object.keys(CHECK_ONLY), "h2-6a-owner-controls", "h2-5g-check-resume", "h2-baseline"];
  validateRegistry(order, available);
  assert.throws(() => validateRegistry(order, [...available, "h2-9z-forgotten"]), /unregistered/);
  assert.throws(() => validateRegistry([...order, order[0]], available), /duplicate/);
  assert.throws(() => validateRegistry([...order, "h2-7de-candidates"], available), /duplicate/);
  for (const name of [...order, ...Object.keys(CHECK_ONLY)]) {
    assert.throws(() => validateRegistry(order, available.filter(entry => entry !== name)), /missing/);
  }
  assert.throws(() => validateRegistry(order, available, { ...CHECK_ONLY, "h2-7de-candidates": "canonical" }), /execution mode/);
  assert.throws(() => validateRegistry(order, available, { "h2-7de-candidates": "frozen-de-reference" }), /execution mode/);
  const source = fs.readFileSync(path.join(ROOT, "scripts/chain-walk.sh"), "utf8");
  const realOrder = source.match(/^ORDER=\(\n([\s\S]*?)\n\)/m)[1].trim().split(/\s+/);
  validateRegistry(realOrder, fs.readdirSync(path.join(ROOT, "crates/oracle"))
    .filter(name => name.endsWith(".mjs")).map(name => name.slice(0, -4)));
  assert.ok(source.includes('--registry "${ORDER[@]}" || exit 2'));
  assert.ok(source.includes('--walk-preflight || exit 2'));
  assert.ok(source.includes('--walk-checks >"$RUN_DIR/check-only.log"'));
});

test("reference copies reject links, mutation, deletion, extra files and path escape", () => {
  const root = temporary();
  const files = new Map([["crates/oracle/check.mjs", Buffer.from("original")], [".node-version", Buffer.from("24.8.0")]]);
  try {
    stageReference(root, files);
    const file = path.join(root, "crates/oracle/check.mjs");
    fs.writeFileSync(file, "changed");
    assert.throws(() => verifyStaged(root, files), /mutated/);
    fs.unlinkSync(file);
    assert.throws(() => verifyStaged(root, files), /file set/);
    fs.symlinkSync(path.join(root, ".node-version"), file);
    assert.throws(() => verifyStaged(root, files), /link/);
    fs.unlinkSync(file); fs.writeFileSync(file, files.get("crates/oracle/check.mjs"));
    fs.writeFileSync(path.join(root, "extra"), "unexpected");
    assert.throws(() => verifyStaged(root, files), /file set/);
    fs.unlinkSync(path.join(root, "extra"));
    verifyStaged(root, files);
    assert.throws(() => stageReference(root, files), /empty/);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
  const empty = temporary();
  try {
    assert.throws(() => stageReference(empty, new Map([["../escape", Buffer.from("x")]])));
  } finally { fs.rmSync(empty, { recursive: true, force: true }); }
});

test("original source, vendor, corpus, artifact, manifest and historical parent bytes are mandatory", () => {
  const root = temporary();
  try {
    const files = loadReference();
    const manifest = read("scripts/frozen-de-reference/manifest.json");
    for (const name of ["scripts/frozen-de-reference/manifest.json",
      ...manifest.files.filter(row => row.snapshot).map(row => row.snapshot.path)]) {
      files.set(name, fs.readFileSync(path.join(ROOT, name)));
    }
    stageReference(root, files);
    loadReference(root);
    const targets = [".node-version", "crates/oracle/h2-7de-candidates.mjs",
      "vendor/typescript-6.0.3/lib/typescript.js",
      "ratchets/h2-7de-observations.v1.json", "scripts/frozen-de-reference/manifest.json",
      manifest.files.find(row => row.path.startsWith("ts-tests/")).path,
      ...manifest.files.filter(row => row.snapshot).map(row => row.snapshot.path)];
    for (const name of targets) {
      const file = path.join(root, name), original = fs.readFileSync(file);
      fs.appendFileSync(file, "changed");
      assert.throws(() => loadReference(root), undefined, name);
      fs.writeFileSync(file, original);
    }
    fs.unlinkSync(path.join(root, ".node-version"));
    assert.throws(() => loadReference(root), /ENOENT/);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});
