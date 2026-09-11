// Direct printer controls for the staged shared worker; no Program admission.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';

const root = path.resolve(import.meta.dirname, '..');
const destination = path.join(root, 'crates/emitter/tests/fixtures/comma-list-printer.json');
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
assert.equal(ts.version, '6.0.3');
assert.ok(['--write', '--check'].includes(process.argv[2]));
if (process.argv[2] === '--write') assert.ok(!fs.existsSync(destination));
const basic = 'sink(/* first */ a /* tail a */, /* between */ b, c /* tail c */);\n';
const shapes = [
  { name: 'parsed', text: basic },
  { name: 'empty', text: 'sink();\n' },
  { name: 'single', text: 'sink(/* first */ a /* last */);\n' },
  { name: 'trailing-comma', text: 'sink(a, b, c,);\n' },
  { name: 'end-leading', text: 'sink(a\n/* before delimiter */ , b\n/* after list */);\n' },
  { name: 'line-comments', text: 'sink(a, // first line\n b, // second line\n c);\n' },
  { name: 'nested-comma', text: 'sink((a, b), c, d);\n' },
  { name: 'grammar', text: 'sink({ x: 1 }, function () {}, a ? b : c);\n' },
  { name: 'synthetic', text: basic, synthetic: true },
  { name: 'synthetic-comment-range', text: basic, synthetic: true, comment_range: true },
  { name: 'next-line', text: basic, new_line: 1 },
  { name: 'first-line-ignored', text: basic, new_line: 0 },
  { name: 'synthetic-next-line', text: basic, synthetic: true, comment_range: true, new_line: 1 },
  { name: 'parent-multiline-ignored', text: basic, multi_line: true },
  { name: 'parent-range', text: basic, parent_range: true },
];
for (const [name, flags] of [['no-leading', 1024], ['no-trailing', 2048], ['no-own', 3072], ['no-nested', 4096], ['no-all', 7168]]) {
  shapes.push({ name: `child-${name}`, text: basic, child_flags: flags });
  shapes.push({ name: `parent-${name}`, text: basic, parent_flags: flags, parent_range: true });
}

function observe(input) {
  const source = ts.createSourceFile('main.ts', input.text, ts.ScriptTarget.ESNext, true, ts.ScriptKind.TS);
  const statement = source.statements[0];
  const call = statement.expression;
  assert.equal(call.kind, ts.SyntaxKind.CallExpression);
  const parsed = call.arguments;
  const elements = input.synthetic ? parsed.map(element => {
    assert.equal(element.kind, ts.SyntaxKind.Identifier);
    const result = ts.factory.createIdentifier(element.text);
    if (input.comment_range) ts.setCommentRange(result, { pos: element.pos, end: element.end });
    return result;
  }) : parsed;
  if (input.child_flags !== undefined) for (const element of elements) ts.setEmitFlags(element, input.child_flags);
  if (input.new_line !== undefined) ts.setStartsOnNewLine(elements[input.new_line], true);
  const comma = ts.factory.createCommaListExpression(elements);
  if (input.parent_range) ts.setTextRange(comma, { pos: parsed[0].pos, end: parsed.at(-1).end });
  if (input.parent_flags !== undefined) ts.setEmitFlags(comma, input.parent_flags);
  if (input.multi_line) ts.setEmitFlags(comma, ts.getEmitFlags(comma) | ts.EmitFlags.MultiLine);
  const updatedCall = ts.factory.updateCallExpression(call, call.expression, call.typeArguments, [comma]);
  const updatedStatement = ts.factory.updateExpressionStatement(statement, updatedCall);
  const updated = ts.factory.updateSourceFile(source, [updatedStatement]);
  const printer = ts.createPrinter({ newLine: ts.NewLineKind.CarriageReturnLineFeed, removeComments: input.remove_comments });
  const text = printer.printFile(updated);
  const bytes = Buffer.from(text);
  const starts = ts.computeLineStarts(text);
  return { text, utf8_base64: bytes.toString('base64'), utf8_bytes: bytes.length,
    end_utf16: { position: text.length, line: starts.length - 1, column: text.length - starts.at(-1) } };
}
const cases = [];
for (const shape of shapes) for (const remove_comments of [false, true]) {
  const { name, ...recipe } = shape;
  const input = { case_id: `comma-list-printer/${name}/${remove_comments ? 'remove' : 'retain'}`, ...recipe, remove_comments };
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  cases.push({ ...input, typescript_observation: first });
}
assert.equal(cases.length, 50);
const artifact = { version: 1, typescript: ts.version, route: 'direct-printer-metadata', repetitions: 2,
  source_commit: '050880ce59e30b356b686bd3144efe24f875ebc8',
  compiler_sha256: sha256(fs.readFileSync(path.join(root, 'vendor/typescript-6.0.3/lib/typescript.js'))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(destination, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(destination, 'utf8'), rendered);
console.log(JSON.stringify({ cases: cases.length, repetitions: 2, native_executions: 0, sha256: sha256(rendered) }));
