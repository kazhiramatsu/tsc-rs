import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const output = 'crates/emitter/tests/fixtures/utf16-literal-escaping.json';
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const units = text => Array.from({ length: text.length }, (_, i) => text.charCodeAt(i));
const inputs = { empty: [], quotes: units('"\'\\`${'), controls: [...Array(32).keys(), 0x85, 0x2028, 0x2029],
  nul_digit: [0, 55, 0, 97], bmp: [233, 38634], pair: [0xd83d, 0xde00], high: [0xd800], low: [0xdc00],
  successive: [0xd800, 0xd800, 0xdc00, 0xdc00], lf: [97, 10, 98], crlf: [97, 13, 10, 98], substitution: units('${x}') };
const kinds = ['double', 'single', 'NoSubstitutionTemplateLiteral', 'TemplateHead', 'TemplateMiddle', 'TemplateTail'];
function observe(input) {
  const text = String.fromCharCode(...input.units), source = ts.createSourceFile('main.ts', '', ts.ScriptTarget.ESNext, true);
  const node = input.kind === 'double' || input.kind === 'single'
    ? ts.factory.createStringLiteral(text, input.kind === 'single')
    : ts.factory.createTemplateLiteralLikeNode(ts.SyntaxKind[input.kind], text, undefined);
  ts.setEmitFlags(node, input.emit_flags);
  const tree_state = { kind: node.kind, pos: node.pos, end: node.end, flags: node.flags,
    emit_flags: ts.getEmitFlags(node), value_utf16: units(node.text) };
  const printed = ts.createPrinter({ newLine: ts.NewLineKind.CarriageReturnLineFeed, neverAsciiEscape: input.never_ascii_escape })
    .printNode(ts.EmitHint.Unspecified, node, source);
  const bytes = Buffer.from(printed), starts = ts.computeLineStarts(printed);
  return { tree_state, text_utf16: units(printed), utf8_base64: bytes.toString('base64'), utf8_bytes: bytes.length,
    end_utf16: { position: printed.length, line: starts.length - 1, column: printed.length - starts.at(-1) } };
}
assert.equal(ts.version, '6.0.3');
assert.ok(['--write', '--check'].includes(process.argv[2]));
const cases = [];
for (const [name, value] of Object.entries(inputs)) for (const kind of kinds)
for (const never_ascii_escape of [false, true]) for (const emit_flags of [0, ts.EmitFlags.NoAsciiEscaping]) {
  const input = { case_id: `utf16-escaping/${name}/${kind}/${never_ascii_escape}/${emit_flags}`, units: value, kind, never_ascii_escape, emit_flags };
  const observed = observe(input); assert.deepEqual(observe(input), observed, input.case_id);
  cases.push({ ...input, typescript_observation: observed });
}
assert.equal(cases.length, 288);
const artifact = { version: 1, typescript: ts.version, repetitions: 2, route: 'direct-factory-and-printer',
  compiler_sha256: sha256(fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js')),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
console.log(JSON.stringify({ output, cases: cases.length, sha256: sha256(rendered) }));
