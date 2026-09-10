// Fresh source observation of the existing declaration-printer regression.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const sha256 = data => crypto.createHash('sha256').update(data).digest('hex');
const inputPath = 'ratchets/h2-7a-printer-reprint.v1.json';
const inputBytes = fs.readFileSync(inputPath);
const row = JSON.parse(inputBytes).rows.find(row => row.id === 'h2-7a-p/P4/unfiltered-intervening-comments');
assert.equal(ts.version, '6.0.3');
assert.equal(sha256(row.input_utf8), row.input_sha256);
const options = { newLine: ts.NewLineKind.LineFeed, removeComments: false,
  noEmitHelpers: true, onlyPrintJsDocStyle: true, omitBraceSourceMapPositions: true };
const results = Array.from({ length: 2 }, () => {
  const source = ts.createSourceFile(row.file_name, row.input_utf8, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  assert.equal(source.parseDiagnostics.length, 0);
  const output = ts.createPrinter(options).printFile(source);
  assert.equal(output, row.expected_utf8);
  assert.equal(sha256(output), row.expected_sha256);
  return { text: output, utf8_base64: Buffer.from(output).toString('base64'), sha256: sha256(output) };
});
assert.deepEqual(results[0], results[1]);
const artifact = { version: 1, typescript: ts.version, route: 'existing-declaration-printer-control',
  source: { path: inputPath, sha256: sha256(inputBytes), row: row.id },
  compiler_sha256: sha256(fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js')),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), options, repetitions: 2, results };
const destination = 'ratchets/h2-8a-list-intervening-jsdoc.v1.json';
const rendered = JSON.stringify(artifact, null, 2) + '\n';
assert.ok(['--write', '--check'].includes(process.argv[2]));
if (process.argv[2] === '--write') fs.writeFileSync(destination, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(destination, 'utf8'), rendered);
console.log(JSON.stringify({ destination, sha256: sha256(rendered), exact: 2 }));
