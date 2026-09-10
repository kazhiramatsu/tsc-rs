// Freeze the checker input needed by the existing retained-field contract.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const nativeFile = 'crates/emitter/tests/integration/active_transform_contract.rs';
const testName = 'es2022_auto_accessor_lowers_while_native_fields_remain_owned';
const body = fs.readFileSync(nativeFile, 'utf8').split(`fn ${testName}()`)[1].split('#[test]')[0];
const input = /parse_source_file\(\s*"([^"]+)",\s*("(?:\\.|[^"\\])*")/.exec(body);
assert.ok(input);
const fileName = input[1], text = JSON.parse(input[2]);
const options = { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.Preserve,
  useDefineForClassFields: true, alwaysStrict: false, noLib: true };
function observe() {
  const source = ts.createSourceFile(fileName, text, options.target, true);
  const host = { getSourceFile: name => name === fileName ? source : undefined,
    getDefaultLibFileName: () => 'lib.d.ts', getCurrentDirectory: () => '/project',
    getCanonicalFileName: name => name, useCaseSensitiveFileNames: () => true,
    getNewLine: () => '\n', fileExists: name => name === fileName,
    readFile: name => name === fileName ? text : undefined, writeFile: () => assert.fail('unexpected host write') };
  const program = ts.createProgram([fileName], options, host);
  const resolver = program.getTypeChecker().getEmitResolver(source);
  const flag = ts.NodeCheckFlags.ContainsConstructorReference;
  const privateMembers = source.statements[0].members.filter(member => member.name && ts.isPrivateIdentifier(member.name));
  assert.equal(privateMembers.length, 1);
  const facts = privateMembers.map(member => ({ kind: ts.SyntaxKind[member.kind], name: member.name.text,
    pos: member.pos, end: member.end, flag, has_node_check_flag: resolver.hasNodeCheckFlag(member, flag) }));
  const writes = [];
  const result = program.emit(undefined, (path, text, bom) => writes.push({ path, text, bom }));
  return { facts, writes, emit_skipped: result.emitSkipped,
    emit_diagnostics: result.diagnostics.map(d => ({ code: d.code, message: ts.flattenDiagnosticMessageText(d.messageText, '\n') })) };
}
const first = observe();
assert.deepEqual(observe(), first);
assert.equal(first.facts[0].has_node_check_flag, false);
const artifact = { version: 1, typescript: ts.version, repetitions: 2,
  route: 'checker-resolver-facts-and-emit; no native complete-command qualification',
  compiler_sha256: sha256(fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js')),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)),
  input: { native_file: nativeFile, test_name: testName, file_name: fileName, text, options },
  typescript_observation: first };
const output = 'ratchets/h2-8a-retained-accessor-contract-resolver.v1.json';
const rendered = JSON.stringify(artifact, null, 2) + '\n';
assert.ok(['--write', '--check'].includes(process.argv[2]));
if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
console.log(JSON.stringify({ output, facts: first.facts, sha256: sha256(rendered) }));
