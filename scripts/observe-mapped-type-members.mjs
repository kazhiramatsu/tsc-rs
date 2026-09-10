// MappedType.members is an unbracketed PreserveLines list with no delimiter.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const output = 'crates/emitter/tests/fixtures/mapped-type-members.json';
const sha256 = value => crypto.createHash('sha256').update(value).digest('hex');
assert.equal(ts.version, '6.0.3');
assert.ok(['--write', '--check'].includes(process.argv[2]));
const texts = {
  inline: 'type M<T> = { [K in keyof T]: T[K]; extra: number; method(x: T): string; };\n',
  lines: 'type M<T> = {\n [K in keyof T]: T[K];\n extra: number;\n method(x: T): string;\n};\n',
  comments: 'type M<T> = {\n [K in keyof T]: T[K];\n /* before */ extra: number; /* after */\n /** method */ method(x: T): string; // tail\n};\n',
  unicode: '/* 😀 */\ntype M<T> = { [K in keyof T]: T[K]; /* é */ extra: number;\n method(x: T): string; };\n',
};
const inputs = [];
for (const [layout, text] of Object.entries(texts)) for (const flags of [0, 1, 2, 3]) {
  inputs.push({ case_id: `mapped-members/${layout}/parsed/${flags}`, text, parent_mode: 'parsed',
    recipe: 'keep', ranged_list: true, flags });
  for (const parent_mode of ['updated', 'created', 'ranged-created']) {
    for (const recipe of ['keep', 'reverse', 'clones']) for (const ranged_list of [false, true]) {
      inputs.push({ case_id: `mapped-members/${layout}/${parent_mode}/${recipe}/${ranged_list}/${flags}`,
        text, parent_mode, recipe, ranged_list, flags });
    }
  }
}
for (const parent_mode of ['created', 'ranged-created']) for (const recipe of ['absent', 'empty']) {
  for (const flags of [0, 1, 2, 3]) inputs.push({ case_id: `mapped-members/empty/${parent_mode}/${recipe}/${flags}`,
    text: 'type M<T> = { [K in keyof T]: T[K]; };\n', parent_mode, recipe, ranged_list: false, flags });
}
for (const parent_flags of [ts.EmitFlags.NoNestedComments, ts.EmitFlags.NoComments]) {
  for (const only_print_js_doc_style of [false, true]) for (const remove_comments of [false, true]) {
    inputs.push({ case_id: `mapped-members/comments/flags/${parent_flags}/${only_print_js_doc_style}/${remove_comments}`,
      text: texts.comments, parent_mode: 'updated', recipe: 'keep', ranged_list: true,
      flags: parent_flags, only_print_js_doc_style, remove_comments });
  }
}
function observe(input) {
  const source = ts.createSourceFile('main.ts', input.text, ts.ScriptTarget.ESNext, true);
  assert.equal(source.parseDiagnostics.length, 0);
  const parsed = source.statements[0].type;
  assert.equal(parsed.kind, ts.SyntaxKind.MappedType);
  const original = parsed.members;
  let node = parsed;
  if (input.parent_mode !== 'parsed') {
    let members;
    if (input.recipe === 'absent') members = undefined;
    else {
      const children = input.recipe === 'empty' ? [] : input.recipe === 'reverse' ? [...original].reverse()
        : input.recipe === 'clones' ? original.map(child => ts.factory.cloneNode(child)) : [...original];
      members = ts.factory.createNodeArray(children);
      if (input.ranged_list) ts.setTextRange(members, original);
    }
    if (input.parent_mode === 'updated') {
      node = ts.factory.updateMappedTypeNode(parsed, parsed.readonlyToken, parsed.typeParameter,
        parsed.nameType, parsed.questionToken, parsed.type, members);
    } else {
      node = ts.factory.createMappedTypeNode(parsed.readonlyToken, parsed.typeParameter,
        parsed.nameType, parsed.questionToken, parsed.type, members);
      if (input.parent_mode === 'ranged-created') ts.setTextRange(node, parsed);
    }
  }
  if (input.flags) ts.setEmitFlags(node, input.flags);
  const state = value => ({ kind: ts.SyntaxKind[value.kind], pos: value.pos, end: value.end,
    flags: value.flags, emit_flags: ts.getEmitFlags(value), original_present: value.original !== undefined });
  const tree_state = { parent: state(node), members: node.members === undefined ? null : {
    same_original: node.members === original, pos: node.members.pos, end: node.members.end,
    has_trailing_comma: node.members.hasTrailingComma, elements: node.members.map(state) } };
  const text = ts.createPrinter({ newLine: ts.NewLineKind.CarriageReturnLineFeed,
    onlyPrintJsDocStyle: input.only_print_js_doc_style, removeComments: input.remove_comments })
    .printNode(ts.EmitHint.Unspecified, node, source);
  const bytes = Buffer.from(text), starts = ts.computeLineStarts(text);
  return { tree_state, text, utf8_base64: bytes.toString('base64'), utf8_bytes: bytes.length,
    end_utf16: { position: text.length, line: starts.length - 1, column: text.length - starts.at(-1) } };
}
const cases = inputs.map(input => {
  const observed = observe(input);
  assert.deepEqual(observe(input), observed, input.case_id);
  return { ...input, typescript_observation: observed };
});
assert.equal(cases.length, 328);
const artifact = { version: 1, typescript: ts.version, repetitions: 2, route: 'direct-factory-and-printer',
  compiler_sha256: sha256(fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js')),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
console.log(JSON.stringify({ output, cases: cases.length, sha256: sha256(rendered) }));
