// Raw boundary ranges and distinct leading/closing parent predicates.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const output = 'crates/emitter/tests/fixtures/list-boundary-lines.json';
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
assert.equal(ts.version, '6.0.3');
assert.ok(['--write', '--check'].includes(process.argv[2]));
const texts = {
  inline: 'sink(a, b);\n',
  leading: 'sink(\n a, b);\n',
  closing: 'sink(a, b\n);\n',
  both: 'sink(\n a, b\n);\n',
};
const inputs = [];
for (const [layout, text] of Object.entries(texts)) {
  for (const parent_provenance of ['synthetic', 'range-only', 'original-only', 'range-and-original']) {
    for (const child_provenance of ['parsed', 'clone', 'clone-ranged', 'created-ranged']) {
      for (const ranged_list of [false, true]) for (const multi_line of [false, true]) {
        inputs.push({ case_id: `list-boundary/array/${layout}/${parent_provenance}/${child_provenance}/${ranged_list ? 'ranged' : 'synthetic'}/${multi_line ? 'prefer-line' : 'preserve-lines'}`,
          container: 'array', list_mode: 'line-boundaries', text, parent_provenance, child_provenance,
          ranged_list, multi_line });
      }
    }
  }
}
// Explicit synthesized-node policy overrides, including false overriding
// the fallback, are separate from the parsed node's original provenance.
for (const starts_on_new_line of [false, true]) for (const parent_provenance of ['synthetic', 'range-and-original']) {
  for (const ranged_list of [false, true]) {
    inputs.push({ case_id: `list-boundary/array/starts/${starts_on_new_line}/${parent_provenance}/${ranged_list}`,
      container: 'array', list_mode: 'line-boundaries', text: texts.both, parent_provenance,
      child_provenance: 'clone', starts_on_new_line, ranged_list, multi_line: false });
  }
}
function observe(input) {
  const source = ts.createSourceFile('main.ts', input.text, ts.ScriptTarget.ESNext, true);
  const statement = source.statements[0], call = statement.expression;
  const supplied = ts.factory.createNodeArray(call.arguments.map(parsed => {
    let child;
    switch (input.child_provenance) {
      case 'parsed': child = parsed; break;
      case 'clone': child = ts.factory.cloneNode(parsed); break;
      case 'clone-ranged': child = ts.setTextRange(ts.factory.cloneNode(parsed), parsed); break;
      case 'created-ranged': child = ts.setTextRange(ts.factory.createIdentifier(parsed.text), parsed); break;
      default: assert.fail(input.child_provenance);
    }
    if (input.starts_on_new_line !== undefined) ts.setStartsOnNewLine(child, input.starts_on_new_line);
    return child;
  }));
  if (input.ranged_list) ts.setTextRange(supplied, call.arguments);
  const expression = ts.factory.createArrayLiteralExpression(supplied, input.multi_line);
  if (['range-only', 'range-and-original'].includes(input.parent_provenance)) ts.setTextRange(expression, call);
  if (['original-only', 'range-and-original'].includes(input.parent_provenance)) ts.setOriginalNode(expression, call);
  const array = expression.elements;
  const array_state = { same_input: array === supplied, pos: array.pos, end: array.end,
    has_trailing_comma: array.hasTrailingComma,
    elements: array.map(node => ({ kind: ts.SyntaxKind[node.kind], pos: node.pos, end: node.end,
      flags: node.flags, emit_flags: ts.getEmitFlags(node), original_present: node.original !== undefined,
      inner_is_supplied: ts.isParenthesizedExpression(node) ? supplied.includes(node.expression) : null })) };
  const parent_state = { pos: expression.pos, end: expression.end, original_present: expression.original !== undefined,
    multi_line: expression.multiLine };
  const updated = ts.factory.updateSourceFile(source, [ts.factory.updateExpressionStatement(statement, expression)]);
  const text = ts.createPrinter({ newLine: ts.NewLineKind.CarriageReturnLineFeed }).printFile(updated);
  const bytes = Buffer.from(text), starts = ts.computeLineStarts(text);
  return { array_state, parent_state, text, utf8_base64: bytes.toString('base64'), utf8_bytes: bytes.length,
    end_utf16: { position: text.length, line: starts.length - 1, column: text.length - starts.at(-1) } };
}
const cases = inputs.map(input => {
  const observation = observe(input);
  assert.deepEqual(observe(input), observation, input.case_id);
  return { ...input, typescript_observation: observation };
});
assert.equal(cases.length, 264);
const artifact = { version: 1, typescript: ts.version, route: 'direct-factory-and-printer', repetitions: 2,
  compiler_sha256: sha256(fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js')),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
console.log(JSON.stringify({ output, cases: cases.length, sha256: sha256(rendered) }));
