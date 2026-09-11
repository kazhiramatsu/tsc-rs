// Import-type attributes enter the ordinary pipeline with their own emit hint.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const output = 'crates/emitter/tests/fixtures/import-type-attributes.json';
const sha256 = value => crypto.createHash('sha256').update(value).digest('hex');
assert.equal(ts.version, '6.0.3');
assert.ok(['--write', '--check'].includes(process.argv[2]));
const texts = {
  inline: token => `type T = import("x", { ${token}: { type: "json", mode: "x" } }).A;\n`,
  lines: token => `type T = import("x", { ${token}: {\n type: "json",\n mode: "x"\n} }).A;\n`,
  comments: token => `type T = import("x", /* outer */ { ${token}: /* inner */ { /* first */ type: "json", // sibling\n /** second */ mode: "x" /* end */ } /* wrapper */ }).A;\n`,
  unicode: token => `/* 😀 */\ntype T = import("x", { ${token}: { /* é */ type: "json",\n mode: "x" } }).A;\n`,
};
const inputs = [];
for (const [layout, makeText] of Object.entries(texts)) for (const token of ['with', 'assert']) {
  for (const mode of ['parsed', 'clone', 'created', 'ranged-created']) for (const flags of [0, 2]) {
    inputs.push({ case_id: `import-type-attrs/${layout}/${token}/${mode}/${flags}`,
      text: makeText(token), mode, flags, replace: false });
  }
}
for (const mode of ['parsed', 'clone']) for (const flags of [4096, 3072]) {
  for (const only_print_js_doc_style of [false, true]) for (const remove_comments of [false, true]) {
    inputs.push({ case_id: `import-type-attrs/comments/policy/${mode}/${flags}/${only_print_js_doc_style}/${remove_comments}`,
      text: texts.comments('with'), mode, flags, replace: false, only_print_js_doc_style, remove_comments });
  }
}
for (const mode of ['parsed', 'clone']) for (const flags of [0, 2]) {
  inputs.push({ case_id: `import-type-attrs/substitution/${mode}/${flags}`, text: texts.comments('with'), mode, flags, replace: true });
}
function observe(input) {
  const source = ts.createSourceFile('main.ts', input.text, ts.ScriptTarget.ESNext, true);
  assert.equal(source.parseDiagnostics.length, 0);
  const parsed = source.statements[0].type, original = parsed.attributes;
  assert.equal(parsed.kind, ts.SyntaxKind.ImportType);
  let attributes;
  if (input.mode === 'parsed') attributes = original;
  else if (input.mode === 'clone') attributes = ts.factory.cloneNode(original);
  else {
    attributes = ts.factory.createImportAttributes(original.elements, original.multiLine, original.token);
    if (input.mode === 'ranged-created') ts.setTextRange(attributes, original);
  }
  if (input.flags) ts.setEmitFlags(attributes, input.flags);
  const node = ts.factory.updateImportTypeNode(parsed, parsed.argument, attributes,
    parsed.qualifier, parsed.typeArguments, parsed.isTypeOf);
  const replacement = input.replace
    ? ts.factory.createImportAttributes(ts.factory.createNodeArray([...attributes.elements].reverse()), false, attributes.token)
    : undefined;
  if (replacement && input.flags) ts.setEmitFlags(replacement, input.flags);
  const state = node => ({ kind: node.kind, pos: node.pos, end: node.end, flags: node.flags,
    emit_flags: ts.getEmitFlags(node), original_present: node.original !== undefined });
  const attributeState = node => node === undefined ? null : { ...state(node), multi_line: node.multiLine ?? null,
    token: node.token, members: { same_original: node.elements === original.elements,
      pos: node.elements.pos, end: node.elements.end, has_trailing_comma: node.elements.hasTrailingComma,
      elements: node.elements.map(state) } };
  const tree_state = { parent: state(node), attributes: attributeState(attributes), replacement: attributeState(replacement) };
  const events = [];
  const record = (phase, hint, node) => events.push({ phase, hint: ts.EmitHint[hint], node: state(node) });
  const text = ts.createPrinter({ newLine: ts.NewLineKind.CarriageReturnLineFeed,
    onlyPrintJsDocStyle: input.only_print_js_doc_style, removeComments: input.remove_comments }, {
    isEmitNotificationEnabled: node => ts.isImportAttributes(node),
    substituteNode(hint, node) {
      if (ts.isImportAttributes(node)) { record('substitute', hint, node); return replacement ?? node; }
      return node;
    },
    onEmitNode(hint, node, emit) {
      record('before', hint, node);
      emit(hint, node);
      record('after', hint, node);
    },
  }).printNode(ts.EmitHint.Unspecified, node, source);
  const bytes = Buffer.from(text), starts = ts.computeLineStarts(text);
  return { tree_state, events, text, utf8_base64: bytes.toString('base64'), utf8_bytes: bytes.length,
    end_utf16: { position: text.length, line: starts.length - 1, column: text.length - starts.at(-1) } };
}
const cases = inputs.map(input => {
  const observed = observe(input);
  assert.deepEqual(observe(input), observed, input.case_id);
  assert.deepEqual(observed.events.map(event => [event.phase, event.hint]),
    ['substitute', 'before', 'after'].map(phase => [phase, 'ImportTypeNodeAttributes']));
  return { ...input, typescript_observation: observed };
});
assert.equal(cases.length, 84);
const artifact = { version: 1, typescript: ts.version, repetitions: 2, route: 'direct-factory-and-printer',
  compiler_sha256: sha256(fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js')),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
console.log(JSON.stringify({ output, cases: cases.length, sha256: sha256(rendered) }));
