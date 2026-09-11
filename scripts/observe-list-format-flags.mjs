// emitList and emitExpressionList apply different parent-flag policies.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const output = 'crates/emitter/tests/fixtures/list-format-flags.json';
const sha256 = value => crypto.createHash('sha256').update(value).digest('hex');
assert.equal(ts.version, '6.0.3');
assert.ok(['--write', '--check'].includes(process.argv[2]));
const cases = [];
function observe(input) {
  const source = ts.createSourceFile('main.ts', input.text, ts.ScriptTarget.ESNext, true);
  assert.equal(source.parseDiagnostics.length, 0);
  const statement = source.statements[0];
  const parsed = input.container === 'attributes' ? statement.attributes
    : statement.declarationList.declarations[0][input.container.endsWith('binding') ? 'name' : 'initializer'];
  const original = parsed.elements ?? parsed.properties;
  const children = ts.factory.createNodeArray([...original]);
  if (input.last_starts_on_new_line) ts.setStartsOnNewLine(children.at(-1), true);
  const node = input.container === 'array' ? ts.factory.createArrayLiteralExpression(children, input.multi_line)
    : input.container === 'object' ? ts.factory.createObjectLiteralExpression(children, input.multi_line)
    : input.container === 'attributes' ? ts.factory.createImportAttributes(children, input.multi_line)
    : input.container === 'array-binding' ? ts.factory.createArrayBindingPattern(children)
    : ts.factory.createObjectBindingPattern(children);
  // The native factory's public set_multi_line face accepts every node kind.
  // Binding emitters do not consume this field, even when it is present.
  if (input.container.endsWith('binding')) node.multiLine = input.multi_line;
  if (input.flags) ts.setEmitFlags(node, input.flags);
  const elements = node.elements ?? node.properties;
  const state = child => ({ kind: ts.SyntaxKind[child.kind], pos: child.pos, end: child.end,
    flags: child.flags, emit_flags: ts.getEmitFlags(child),
    original_present: child.original !== undefined, starts_on_new_line: child.emitNode?.startsOnNewLine ?? null });
  const tree_state = { parent: { ...state(node), multi_line: node.multiLine ?? null },
    members: { same_input: elements === children, pos: elements.pos, end: elements.end,
      has_trailing_comma: elements.hasTrailingComma, elements: elements.map(state) } };
  const text = ts.createPrinter({ newLine: ts.NewLineKind.CarriageReturnLineFeed })
    .printNode(ts.EmitHint.Unspecified, node, source);
  const bytes = Buffer.from(text), starts = ts.computeLineStarts(text);
  return { tree_state, text, utf8_base64: bytes.toString('base64'), utf8_bytes: bytes.length,
    end_utf16: { position: text.length, line: starts.length - 1, column: text.length - starts.at(-1) } };
}
for (const container of ['array', 'object', 'array-binding', 'object-binding', 'attributes']) {
  for (const layout of ['inline', 'lines']) {
    const gap = layout === 'inline' ? ' ' : '\n ';
    const body = container === 'array' ? `const x = [a,${gap}b];\n`
      : container === 'object' ? `const x = { a: x,${gap}b: y };\n`
      : container === 'array-binding' ? `const [a,${gap}b] = x;\n`
      : container === 'object-binding' ? `const { a,${gap}b } = x;\n`
      : `import x from "x" with { type: "json",${gap}mode: "x" };\n`;
    for (const multi_line of [false, true]) for (const flags of [0, 1, 2, 3]) {
      for (const last_starts_on_new_line of [false, true]) {
        const input = { case_id: `list-format/${container}/${layout}/${multi_line}/${flags}/${last_starts_on_new_line}`,
          container, layout, text: body, multi_line, flags, last_starts_on_new_line };
        const observed = observe(input);
        assert.deepEqual(observe(input), observed, input.case_id);
        cases.push({ ...input, typescript_observation: observed });
      }
    }
  }
}
assert.equal(cases.length, 160);
const artifact = { version: 1, typescript: ts.version, repetitions: 2, route: 'direct-factory-and-printer',
  compiler_sha256: sha256(fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js')),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
console.log(JSON.stringify({ output, cases: cases.length, sha256: sha256(rendered) }));
