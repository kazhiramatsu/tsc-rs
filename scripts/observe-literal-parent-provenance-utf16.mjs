// Lossless JSON representation of the freshly checked source observations.
// Rust's JSON strings cannot contain the unpaired UTF16 units returned by
// neverAsciiEscape. Keep those units explicitly; do not replace or drop them.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';

const output = 'crates/emitter/tests/fixtures/literal-parent-provenance-utf16.json';
const source = 'crates/emitter/tests/fixtures/literal-parent-provenance.json';
const sha256 = value => crypto.createHash('sha256').update(value).digest('hex');
assert.ok(['--write', '--check'].includes(process.argv[2]));
const checked = spawnSync(process.execPath, ['scripts/observe-literal-parent-provenance.mjs', '--check'], { encoding: 'utf8' });
assert.equal(checked.status, 0, checked.stderr);
const bytes = fs.readFileSync(source);
const artifact = JSON.parse(bytes);
assert.equal(JSON.parse(checked.stdout).sha256, sha256(bytes));
artifact.source_observations = { path: source, sha256: sha256(bytes), observer_sha256: artifact.observer_sha256 };
artifact.observer_sha256 = sha256(fs.readFileSync(import.meta.filename));
for (const row of artifact.cases) {
  const { text, ...rest } = row.typescript_observation;
  row.typescript_observation = { ...rest,
    text_utf16: Array.from({ length: text.length }, (_, index) => text.charCodeAt(index)) };
}
const rendered = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
console.log(JSON.stringify({ output, cases: artifact.cases.length, sha256: sha256(rendered) }));
