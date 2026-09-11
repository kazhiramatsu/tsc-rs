import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';

const output = 'crates/emitter/tests/fixtures/template-raw-provenance.json';
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const compiler = fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js');
assert.equal(sha256(compiler), '569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39');
assert.equal(ts.version, '6.0.3');
const units = text => Array.from({ length: text.length }, (_, i) => text.charCodeAt(i));
const text = value => value === null ? undefined : String.fromCharCode(...value);
const rawInputs = {
  absent: null, empty: [], ascii: units('raw'), high: [0xd800], low: [0xdc00],
  pair: [0xd83d, 0xde00], reverse: [0xdc00, 0xd800], escaped: units('\\uD800'),
  invalid_escape: units('\\u00G1${'), lines: [0xd800, 13, 10, 0xdc00, 0x2028],
};
function observe(input) {
  const source = ts.createSourceFile('main.ts', '', ts.ScriptTarget.Latest, true);
  const kind = ts.SyntaxKind[input.kind];
  const origin = ts.factory.createTemplateLiteralLikeNode(kind, text(input.origin_units), text(input.origin_raw_units));
  const created = ts.factory.createTemplateLiteralLikeNode(kind, text(input.units), text(input.raw_units));
  if (input.policy === 'node-no-ascii') ts.setEmitFlags(created, ts.EmitFlags.NoAsciiEscaping);
  if (input.operation === 'set-original' || input.operation === 'set-original-then-clone') ts.setOriginalNode(created, origin);
  const selected = input.operation === 'clone' || input.operation === 'set-original-then-clone'
    ? ts.factory.cloneNode(created) : created;
  const label = node => node === origin ? 'origin' : node === created ? 'created' : node === selected ? 'selected' : null;
  const state = node => ({ kind: node.kind, pos: node.pos, end: node.end, flags: node.flags,
    transform_flags: node.transformFlags, emit_flags: ts.getEmitFlags(node),
    text_utf16: units(node.text), raw_text_utf16: node.rawText === undefined ? null : units(node.rawText),
    original: label(node.original) });
  const tree_state = { origin: state(origin), created: state(created), selected: state(selected) };
  const printed = ts.createPrinter({ newLine: ts.NewLineKind.LineFeed, neverAsciiEscape: input.policy === 'printer-no-ascii' })
    .printNode(ts.EmitHint.Unspecified, selected, source);
  const bytes = Buffer.from(printed), starts = ts.computeLineStarts(printed);
  return { tree_state, text_utf16: units(printed), utf8_base64: bytes.toString('base64'), utf8_bytes: bytes.length,
    end_utf16: { position: printed.length, line: starts.length - 1, column: printed.length - starts.at(-1) } };
}
const cases = [];
for (const kind of ['NoSubstitutionTemplateLiteral', 'TemplateHead', 'TemplateMiddle', 'TemplateTail']) {
  for (const [raw_name, raw_units] of Object.entries(rawInputs)) {
    for (const operation of ['create', 'clone', 'set-original', 'set-original-then-clone']) {
      for (const policy of ['ascii', 'printer-no-ascii', 'node-no-ascii']) {
        const input = { case_id: `template-raw-provenance/${kind}/${raw_name}/${operation}/${policy}`,
          kind, raw_units, operation, policy, units: [120, 0xd800], origin_units: [111, 0xdc00], origin_raw_units: [111, 0xdc00] };
        const observed = observe(input);
        assert.deepEqual(observe(input), observed, input.case_id);
        cases.push({ ...input, typescript_observation: observed });
      }
    }
  }
}
assert.equal(cases.length, 480);
const artifact = { version: 1, typescript: ts.version, repetitions: 2, route: 'direct-factory-and-printer',
  compiler_sha256: sha256(compiler), observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
assert.ok(['--write', '--check'].includes(process.argv[2]));
if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
console.log(JSON.stringify({ output, cases: cases.length, sha256: sha256(rendered) }));
