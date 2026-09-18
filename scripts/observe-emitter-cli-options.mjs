// Actual pinned TypeScript CLI observations for already implemented emit values.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';

const root = path.resolve(import.meta.dirname, '..');
const cli = path.join(root, 'vendor/typescript-6.0.3/lib/_tsc.js');
assert.ok(fs.existsSync(cli), 'pinned TypeScript CLI bundle must exist');
const destination = path.join(root, 'crates/compiler/tests/fixtures/emitter-cli-options.json');
assert.ok(['--write', '--check'].includes(process.argv[2]));
assert.equal(process.versions.node, fs.readFileSync(path.join(root, '.node-version'), 'utf8').trim());
const modules = ['none', 'commonjs', 'amd', 'umd', 'system', 'es6', 'es2015',
  'es2020', 'es2022', 'esnext', 'node16', 'node18', 'node20', 'nodenext', 'preserve'];
const inputs = [];
for (const module of modules)
  for (const target of ['es5', 'es2015', 'esnext']) inputs.push({ module, target, ignoreDeprecations: '6.0' });
inputs.push({ module: 'commonjs', target: 'es5' }, { module: 'commonjs', target: 'es3' },
  { module: 'CoMmOnJs', target: 'EsNeXt', ignoreDeprecations: '6.0' });
assert.equal(inputs.length, 48);
for (const noEmit of [true, false])
  for (const sourceMap of [undefined, false, true])
    inputs.push({ module: 'commonjs', target: 'es2015', ignoreDeprecations: '6.0', configOnly: true, noEmit, sourceMap });
assert.equal(inputs.length, 54);
for (const noEmit of [true, false])
  for (const ignoreDeprecations of [undefined, '6.0'])
    inputs.push({ module: 'commonjs', target: 'es2015', ignoreDeprecations, configOnly: true,
      noEmit, sourceMap: true, esModuleInterop: false, source: 'export const value: number = "wrong";\n' });
assert.equal(inputs.length, 58);
const source = 'export const value: number = 1;\nexport const read = () => value;\n';
function observe(input) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'emitter-cli-options-'));
  const config = { compilerOptions: { target: 'esnext', module: 'preserve', lib: ['es5'], types: [],
    sourceMap: true, outDir: 'out', skipDefaultLibCheck: true, ignoreDeprecations: input.ignoreDeprecations }, files: ['main.ts'] };
  if (input.configOnly) Object.assign(config.compilerOptions, {
    module: input.module, target: input.target, noEmit: input.noEmit, sourceMap: input.sourceMap,
    esModuleInterop: input.esModuleInterop,
  });
  const args = ['--pretty', 'false', '-p', 'tsconfig.json'];
  if (!input.configOnly) args.push('--module', input.module, '--target', input.target);
  const configText = JSON.stringify(config);
  try {
    fs.writeFileSync(path.join(directory, 'main.ts'), input.source ?? source);
    fs.writeFileSync(path.join(directory, 'tsconfig.json'), configText);
    const result = spawnSync(process.execPath, [cli, ...args], { cwd: directory, encoding: 'utf8' });
    assert.ifError(result.error);
    assert.ok(Number.isInteger(result.status));
    assert.equal(result.stderr, '', 'TypeScript must execute without a Node failure');
    assert.ok([0, 2].includes(result.status), 'this corpus emits even with option diagnostics');
    assert.ok(!result.stdout.includes(directory) && !result.stderr.includes(directory), 'non-hermetic output path');
    const outputs = {};
    const output = path.join(directory, 'out');
    if (fs.existsSync(output)) for (const name of fs.readdirSync(output).sort())
      outputs[`out/${name}`] = fs.readFileSync(path.join(output, name), 'base64');
    return { args, config, config_text: configText, observation: { exit: result.status, stdout: result.stdout, stderr: result.stderr, outputs } };
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}
const cases = inputs.map((input, index) => {
  const first = observe(input), second = observe(input);
  assert.deepEqual(first, second);
  return { case_id: `emitter-cli-options/${index}/${input.module}/${input.target}`,
    source: input.source ?? source, input, ...first };
});
const artifact = { schema: 1, typescript: '6.0.3', repetitions: 2,
  observer_sha256: crypto.createHash('sha256').update(fs.readFileSync(import.meta.filename)).digest('hex'),
  compiler_sha256: crypto.createHash('sha256').update(fs.readFileSync(path.join(root, 'vendor/typescript-6.0.3/lib/_tsc.js'))).digest('hex'), cases };
const bytes = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(destination, bytes);
else assert.equal(fs.readFileSync(destination, 'utf8'), bytes);
console.log(`emitter CLI options: ${cases.length} commands exact twice`);
