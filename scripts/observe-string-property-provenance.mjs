import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';

const output = 'crates/emitter/tests/fixtures/string-property-provenance.json';
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const compiler = fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js');
assert.equal(sha256(compiler), '569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39');
assert.equal(ts.version, '6.0.3');
const units = text => Array.from({ length: text.length }, (_, i) => text.charCodeAt(i));
const recipes = [
  { name: 'value-and-double-quote', origin_single_quote: true, single_quote: false },
  { name: 'value-and-single-quote', origin_single_quote: false, single_quote: true },
  { name: 'origin-text-source', origin_single_quote: true, single_quote: false, origin_text_source: 'originName' },
  { name: 'target-text-source', origin_single_quote: false, single_quote: true, text_source: 'targetName' },
  { name: 'both-text-sources', origin_single_quote: true, single_quote: false, origin_text_source: 'originName', text_source: 'targetName' },
];
function observe(input) {
  const source = ts.createSourceFile('main.ts', '', ts.ScriptTarget.Latest, true);
  const origin = ts.factory.createStringLiteral(String.fromCharCode(...input.origin_units), input.origin_single_quote);
  const created = ts.factory.createStringLiteral(String.fromCharCode(...input.units), input.single_quote);
  const originText = input.origin_text_source ? ts.factory.createIdentifier(input.origin_text_source) : undefined;
  const targetText = input.text_source ? ts.factory.createIdentifier(input.text_source) : undefined;
  // These are ordinary node properties in getLiteralTextOfNode, not emitNode
  // fields. Assign them before testing cloneNode/setOriginalNode ownership.
  if (originText) origin.textSourceNode = originText;
  if (targetText) created.textSourceNode = targetText;
  if (input.policy === 'node-no-ascii') ts.setEmitFlags(created, ts.EmitFlags.NoAsciiEscaping);
  if (input.operation === 'set-original' || input.operation === 'set-original-then-clone') ts.setOriginalNode(created, origin);
  const selected = input.operation === 'clone' || input.operation === 'set-original-then-clone'
    ? ts.factory.cloneNode(created) : created;
  const label = node => node === origin ? 'origin' : node === created ? 'created' : null;
  const state = node => ({ kind: node.kind, pos: node.pos, end: node.end, flags: node.flags,
    transform_flags: node.transformFlags, emit_flags: ts.getEmitFlags(node), text_utf16: units(node.text),
    single_quote: node.singleQuote, text_source: node.textSourceNode?.text ?? null, original: label(node.original) });
  const tree_state = { origin: state(origin), created: state(created), selected: state(selected) };
  const printed = ts.createPrinter({ newLine: ts.NewLineKind.LineFeed, neverAsciiEscape: input.policy === 'printer-no-ascii' })
    .printNode(ts.EmitHint.Unspecified, selected, source);
  const bytes = Buffer.from(printed), starts = ts.computeLineStarts(printed);
  return { tree_state, text_utf16: units(printed), utf8_base64: bytes.toString('base64'), utf8_bytes: bytes.length,
    end_utf16: { position: printed.length, line: starts.length - 1, column: printed.length - starts.at(-1) } };
}
const cases = [];
for (const recipe of recipes) for (const operation of ['create', 'clone', 'set-original', 'set-original-then-clone']) {
  for (const policy of ['ascii', 'printer-no-ascii', 'node-no-ascii']) {
    const input = { case_id: `string-property-provenance/${recipe.name}/${operation}/${policy}`,
      ...recipe, operation, policy, units: [120, 0xd800], origin_units: [111, 0xdc00] };
    const observed = observe(input);
    assert.deepEqual(observe(input), observed, input.case_id);
    cases.push({ ...input, typescript_observation: observed });
  }
}
assert.equal(cases.length, 60);
const artifact = { version: 1, typescript: ts.version, repetitions: 2, route: 'direct-factory-and-printer',
  compiler_sha256: sha256(compiler), observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
assert.ok(['--write', '--check'].includes(process.argv[2]));
if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
console.log(JSON.stringify({ output, cases: cases.length, sha256: sha256(rendered) }));
