import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import Module from 'node:module';
import path from 'node:path';
const output = 'crates/emitter/tests/fixtures/utf16-writer.json';
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const compilerPath = path.resolve('vendor/typescript-6.0.3/lib/typescript.js');
const compiler = fs.readFileSync(compilerPath, 'utf8');
assert.equal(sha256(compiler), '569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39');
// This private constructor is not exported by the package. Expose the actual
// lexical function without changing it or any writer worker.
const anchor = 'var stringWriter = createSingleLineStringWriter();';
assert.equal(compiler.split(anchor).length, 2);
const instrumented = compiler.replace(anchor, `${anchor}\nObject.defineProperty(module.exports, "createSingleLineStringWriter", { value: createSingleLineStringWriter });`);
const module = new Module(compilerPath);
module.filename = compilerPath;
module.paths = Module._nodeModulePaths(path.dirname(compilerPath));
module._compile(instrumented, compilerPath);
const ts = module.exports;
const units = text => Array.from({ length: text.length }, (_, i) => text.charCodeAt(i));
const write = (op, value) => ({ op, units: typeof value === 'string' ? units(value) : value });
const line = force => ({ op: 'writeLine', force });
const inc = { op: 'increaseIndent' }, dec = { op: 'decreaseIndent' }, clear = { op: 'clear' };
const hi = [0xd83d], lo = [0xde00];
const programs = {
  pair: [write('writeStringLiteral', [...hi, ...lo])],
  split: [write('writeStringLiteral', hi), write('writeStringLiteral', lo)],
  empty: [write('write', hi), write('write', []), write('writeLiteral', []), write('writeComment', []), write('rawWrite', []), write('write', lo)],
  reverse: [write('write', lo), write('write', hi), write('write', lo)],
  successive: [write('write', hi), write('write', hi), write('write', lo), write('write', lo)],
  separator: [write('write', hi), write('write', 'x'), write('write', lo)],
  raw: [write('rawWrite', hi), write('rawWrite', lo), write('rawWrite', [])],
  comments: [write('writeComment', hi), write('write', []), write('writeComment', []), write('writeStringLiteral', lo)],
  indentation: [inc, write('write', hi), line(false), inc, write('write', lo), dec, line(true), dec],
  empty_raw_indent: [inc, write('rawWrite', []), write('write', hi), write('write', lo), clear, write('write', 'ok')],
  clear_between: [write('write', hi), clear, write('write', lo), clear, write('write', '😀')],
  split_newline: [write('write', hi), write('rawWrite', '\r'), write('rawWrite', '\n'), write('write', lo)],
  lines: [write('write', [0xd800, 13, 10, 0xdc00, 0x2028, 0x2029, 0x85]), write('writeComment', 'c'), line(false)],
  aliases: ['writeKeyword', 'writeOperator', 'writeParameter', 'writeProperty', 'writePunctuation', 'writeSpace', 'writeStringLiteral', 'writeSymbol', 'writeTrailingSemicolon', 'writeLiteral'].flatMap(op => [write(op, hi), write(op, lo), write(op, [])]),
  forced: [line(false), line(true), inc, write('writeComment', hi), line(false), line(true), write('write', lo)],
  reuse: [write('write', hi), write('write', lo), clear, inc, write('writeComment', hi), write('write', lo), clear, write('write', ' ')],
};
function observe(input) {
  const writer = input.mode === 'single' ? ts.createSingleLineStringWriter() : ts.createTextWriter(input.mode === 'lf' ? '\n' : '\r\n');
  const capture = () => {
    const text = writer.getText(), bytes = Buffer.from(text);
    return { text_utf16: units(text), utf8_base64: bytes.toString('base64'), utf8_bytes: bytes.length,
      end_utf16: { position: writer.getTextPos(), line: writer.getLine(), column: writer.getColumn() },
      indent: writer.getIndent(), at_line_start: writer.isAtStartOfLine(),
      trailing_comment: writer.hasTrailingComment(), trailing_whitespace: writer.hasTrailingWhitespace() };
  };
  const steps = [capture()];
  for (const action of input.actions) {
    if (action.units) writer[action.op](String.fromCharCode(...action.units));
    else if (action.op === 'writeLine') writer.writeLine(action.force);
    else writer[action.op]();
    steps.push(capture());
  }
  return { steps };
}
assert.equal(ts.version, '6.0.3');
assert.ok(['--write', '--check'].includes(process.argv[2]));
const cases = [];
for (const mode of ['lf', 'crlf', 'single']) for (const [name, actions] of Object.entries(programs)) {
  const input = { case_id: `utf16-writer/${mode}/${name}`, mode, actions };
  const observed = observe(input); assert.deepEqual(observe(input), observed, input.case_id);
  cases.push({ ...input, typescript_observation: observed });
}
assert.equal(cases.length, 48);
const artifact = { version: 1, typescript: ts.version, repetitions: 2, route: 'direct-writer',
  compiler_sha256: sha256(compiler), instrumented_sha256: sha256(instrumented),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
console.log(JSON.stringify({ output, cases: cases.length, sha256: sha256(rendered) }));
