// Read-only proof against the committed pre-walk fixture bytes.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { execFileSync } from 'node:child_process';
import { SPECS, SELECTION, ROOT, inspect, sha } from '/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/scripts/command-fixture-parents.mjs';
const before = name => execFileSync('git', ['show', `HEAD:${name}`], { cwd: ROOT, maxBuffer: 256 * 1024 * 1024 });
const decode = (name, bytes) => JSON.parse(name.endsWith('.zst') ? execFileSync('zstd', ['-q','-d','--stdout'], { input: bytes, maxBuffer: 256 * 1024 * 1024 }) : bytes);
const state = inspect(ROOT);
assert.deepStrictEqual(state.stale, []);
const oldSelection = before(SELECTION);
const expectedSelection = JSON.parse(oldSelection);
for (const parent of ['ratchets/h2-5h-qualification.v1.json', 'ratchets/h2-5g-qualification.v1.json']) {
  expectedSelection.files[parent] = sha(fs.readFileSync(`${ROOT}/${parent}`));
}
assert.deepStrictEqual(state.selection, expectedSelection);
const results = [];
for (const fixture of state.fixtures) {
  const { spec, document } = fixture;
  const original = before(spec.file);
  const expected = decode(spec.file, original);
  const payload = JSON.stringify(expected.cases);
  const leaves = [];
  for (const { pointer, parent } of spec.parents) {
    const container = pointer.slice(0,-1).reduce((x,k) => x[k], expected);
    const from = container[pointer.at(-1)], to = sha(fs.readFileSync(`${ROOT}/${parent}`));
    container[pointer.at(-1)] = to;
    leaves.push({ pointer, from, to });
  }
  if (spec.name === 'parameters') expected.selection_sha256 = sha(state.selectionBytes);
  assert.deepStrictEqual(document, expected, spec.file);
  assert.equal(JSON.stringify(document), JSON.stringify(expected), spec.file);
  assert.equal(JSON.stringify(document.cases), payload, spec.file);
  results.push({ path: spec.file, count: spec.count, before_sha256: sha(original), after_sha256: sha(fixture.bytes), complete_case_payloads_unchanged: true, allowed_parent_leaves: leaves });
}
console.log(JSON.stringify({ passed: true, scope: 'Read-only pre-walk versus post-walk metadata-only proof, not native qualification', fixture_count: SPECS.length, selection_two_parent_leaves_only: true, results }));
