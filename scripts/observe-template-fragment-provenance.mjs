import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';

const output = 'crates/emitter/tests/fixtures/template-fragment-provenance.json';
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const compiler = fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js');
assert.equal(sha256(compiler), '569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39');
assert.equal(ts.version, '6.0.3');
const units = text => Array.from({ length: text.length }, (_, i) => text.charCodeAt(i));
function capture(text) {
  const bytes = Buffer.from(text), starts = ts.computeLineStarts(text);
  return { text_utf16: units(text), utf8_base64: bytes.toString('base64'), utf8_bytes: bytes.length,
    end_utf16: { position: text.length, line: starts.length - 1, column: text.length - starts.at(-1) } };
}
function observe(input) {
  const empty = ts.createSourceFile('synthetic.ts', '', ts.ScriptTarget.Latest, true);
  const synthetic = ts.factory.createTemplateLiteralLikeNode(ts.SyntaxKind[input.kind], String.fromCharCode(...input.units), undefined);
  const printedSynthetic = ts.createPrinter({ newLine: ts.NewLineKind.LineFeed, neverAsciiEscape: input.never_ascii_escape })
    .printNode(ts.EmitHint.Unspecified, synthetic, empty);
  const source = ts.createSourceFile('parsed-template.ts', input.parsed_source, ts.ScriptTarget.Latest, true);
  assert.equal(source.parseDiagnostics.length, 0);
  let parsed;
  const visit = node => { if (node.kind === ts.SyntaxKind[input.kind]) parsed ??= node; ts.forEachChild(node, visit); };
  visit(source);
  assert.ok(parsed);
  assert.deepEqual(units(parsed.text), input.units);
  const printedParsed = ts.createPrinter({ newLine: ts.NewLineKind.LineFeed, neverAsciiEscape: false })
    .printNode(ts.EmitHint.Unspecified, parsed, source);
  return { synthetic: capture(printedSynthetic), parsed: capture(printedParsed), equal_utf16: printedSynthetic === printedParsed };
}
const cases = [];
for (const kind of ['NoSubstitutionTemplateLiteral', 'TemplateHead', 'TemplateMiddle', 'TemplateTail']) {
  for (const never_ascii_escape of [false, true]) {
    const body = never_ascii_escape ? '😀\\uD800' : '\\uD83D\\uDE00\\uD800';
    const token = kind === 'NoSubstitutionTemplateLiteral' ? `\`${body}\``
      : kind === 'TemplateHead' ? `\`${body}\${expression}\``
      : kind === 'TemplateMiddle' ? `\`\${left}${body}\${right}\`` : `\`\${left}${body}\``;
    const input = { case_id: `template-fragment-provenance/${kind}/${never_ascii_escape}`, kind,
      units: [0xd83d, 0xde00, 0xd800], never_ascii_escape, parsed_source: `const value = ${token};` };
    const observed = observe(input);
    assert.deepEqual(observe(input), observed, input.case_id);
    cases.push({ ...input, typescript_observation: observed });
  }
}
assert.equal(cases.length, 8);
const artifact = { version: 1, typescript: ts.version, repetitions: 2, route: 'direct-factory-and-printer',
  compiler_sha256: sha256(compiler), observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
assert.ok(['--write', '--check'].includes(process.argv[2]));
if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
console.log(JSON.stringify({ output, cases: cases.length, sha256: sha256(rendered),
  differing_parsed_and_synthetic: cases.filter(row => !row.typescript_observation.equal_utf16).length }));
