// Verify the frozen preparation inputs; this is not a runtime readiness gate.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const read = relative => fs.readFileSync(path.join(root, relative));
const sha = value => crypto.createHash('sha256').update(value).digest('hex');
const packet = JSON.parse(read('docs/design/greenfield/slices/h2-8a-jsdoc-return-selection.v1.json'));
assert.equal(packet.status, 'selection-and-source-inventory-not-runtime-ready');
for (const [relative, expected] of Object.entries(packet.files)) {
  assert.equal(sha(read(relative)), expected, `stale preparation source: ${relative}`);
}
const require = createRequire(import.meta.url);
const ts = require(path.join(root, 'vendor/typescript-6.0.3/lib/typescript.js'));
assert.equal(ts.version, packet.typescript.version);
const upstream = read(packet.typescript.path);
assert.equal(sha(upstream), packet.typescript.sha256);
const source = ts.createSourceFile(packet.typescript.path, upstream.toString('utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
const names = new Set(packet.typescript.owners.map(owner => owner.name));
const actual = [];
function visit(node) {
  if (ts.isFunctionDeclaration(node) && node.name && names.has(node.name.text)) {
    const start = node.getStart(source);
    actual.push({
      name: node.name.text,
      start_line: source.getLineAndCharacterOfPosition(start).line + 1,
      end_line: source.getLineAndCharacterOfPosition(node.end).line + 1,
      sha256_utf8_no_leading_trivia: sha(source.text.slice(start, node.end)),
    });
  }
  ts.forEachChild(node, visit);
}
visit(source);
assert.deepEqual(actual, packet.typescript.owners);
for (const [relative, expected] of Object.entries(packet.original_artifact_rows)) {
  const artifact = JSON.parse(read(relative));
  const rows = artifact.cases.filter(row => row.case_id === packet.case_id);
  assert.equal(rows.length, 1, `original case identity: ${relative}`);
  assert.equal(sha(JSON.stringify(rows[0])), expected.sha256_json_stringify);
  assert.deepEqual(rows[0], expected.row);
}
const beforeBytes = read(packet.new_native_baseline.artifact);
assert.equal(sha(beforeBytes), packet.new_native_baseline.sha256);
const before = JSON.parse(beforeBytes);
assert.equal(before.head, packet.base_head);
assert.equal(before.status, 'observed-strict-failure-not-a-pass');
assert.equal(before.receipt.steps[0].exit_code, 101);
assert.equal(before.receipt.steps[0].inputs_unchanged, true);
assert.equal(before.captures.length, 2);
assert.deepEqual(before.captures[0].record.actual, before.captures[1].record.actual);
const expected = packet.original_artifact_rows['ratchets/h2-8a-observations.v1.json'].row.typescript_observation;
for (const capture of before.captures) {
  assert.equal(capture.record.case_id, packet.case_id);
  assert.deepEqual(capture.record.expected, expected);
}
console.log(`G5c preparation verified: ${Object.keys(packet.files).length} file pins, ${actual.length} upstream owners, one unchanged original case, strict before failure twice. Runtime readiness remains pending.`);
