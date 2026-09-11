// Internal printer controls, not admitted compiler inputs.
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
const hash = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
assert.equal(ts.version, '6.0.3');
assert.ok(['--write', '--check'].includes(process.argv[2]));
const modes = ['baseline', 'no-token-maps', 'token-override', 'absent-name'];
const shapes = [
  ['new-target', 'function Foo() { return 1_0, new.target; }\n'],
  ['import-meta', 'const Foo = [1_0, import.meta];\n'],
];
function observe(text, mode) {
  const options = { noLib: true, target: ts.ScriptTarget.ES2015,
    module: ts.ModuleKind.ESNext, sourceMap: true,
    newLine: ts.NewLineKind.CarriageReturnLineFeed };
  const host = ts.createCompilerHost(options, true);
  host.getCurrentDirectory = () => '/';
  host.getSourceFile = (name, language) => name === '/main.ts'
    ? ts.createSourceFile(name, text, language, true) : undefined;
  host.fileExists = name => name === '/main.ts';
  host.readFile = name => name === '/main.ts' ? text : undefined;
  const program = ts.createProgram(['/main.ts'], options, host);
  const writes = [];
  let touched = 0;
  const after = context => source => {
    const visit = node => {
      if (ts.isMetaProperty(node)) {
        touched++;
        if (mode === 'no-token-maps') {
          ts.setEmitFlags(node, ts.getEmitFlags(node) | ts.EmitFlags.NoTokenSourceMaps);
        } else if (mode === 'token-override') {
          ts.setTokenSourceMapRange(node, node.keywordToken, { pos: 0, end: 1 });
        } else if (mode === 'absent-name') {
          return ts.factory.updateMetaProperty(node, undefined);
        }
        return node;
      }
      return ts.visitEachChild(node, visit, context);
    };
    return ts.visitNode(source, visit);
  };
  const result = program.emit(undefined, (name, callbackText, bom, onError, sources, data) => {
    writes.push({ path: name, callback_text: callbackText, write_byte_order_mark: bom,
      on_error_callback_present: onError !== undefined,
      source_files: sources?.map(source => source.fileName) ?? null,
      data_present: data !== undefined,
      data_source_map_url_pos: data?.sourceMapUrlPos ?? null });
  }, undefined, false, { after: [after] });
  assert.equal(touched, 1);
  return { touched, emit_skipped: result.emitSkipped, writes,
    source_maps: result.sourceMaps?.map(map => ({ input_source_file_names: map.inputSourceFileNames,
      source_map_json: JSON.stringify(map.sourceMap) })) ?? null };
}
const rows = [];
for (const [shape, text] of shapes) for (const mode of modes) {
  const observation = observe(text, mode);
  assert.deepEqual(observe(text, mode), observation);
  rows.push({ case_id: `${shape}/${mode}`, shape, mode, source_text: text, observation });
}
const artifact = { version: 1, typescript: ts.version,
  source_commit: '050880ce59e30b356b686bd3144efe24f875ebc8', repetitions: 2,
  scope: 'internal printer metadata and optional-child invariants; not complete compiler commands or source admissions',
  compiler_sha256: hash(fs.readFileSync(new URL('../vendor/typescript-6.0.3/lib/typescript.js', import.meta.url))),
  observer_sha256: hash(fs.readFileSync(import.meta.filename)), rows };
const bytes = JSON.stringify(artifact, null, 2) + '\n';
const output = new URL('../crates/emitter/tests/fixtures/meta-property-token-map-invariants.json', import.meta.url);
if (process.argv[2] === '--write') fs.writeFileSync(output, bytes);
else assert.equal(fs.readFileSync(output, 'utf8'), bytes);
console.log('MetaProperty internal printer invariants: eight rows observed twice');
