// Complete commands for source-owned external helper imports.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/compiler/tests/fixtures/import-helpers.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const helperNames = ["__awaiter", "__generator", "__importDefault", "__importStar", "__exportStar", "__createBinding", "__setModuleDefault"];
const ambient = 'declare module "dep" { const value: number; export default value; export const item: number; }\n'
  + 'declare module "tslib" { const value: number; export default value; export const item: number; '
  + helperNames.map(name => `export const ${name}: any;`).join(' ') + ' }\n';
const defaults = { strict: true, skipDefaultLibCheck: true, noErrorTruncation: true,
  sourceMap: true, outDir: "/project/out", esModuleInterop: true, importHelpers: true };
const shapes = [
  ["async-helper", 'export async function read() { return 1; }\n'],
  ["default-import", 'import value from "dep"; export const result = value;\n'],
  ["namespace-import", 'import * as library from "dep"; export const result = library.item;\n'],
  ["mixed-default-named", 'import value, { item } from "dep"; export const result = value + item;\n'],
  ["named-default-import", 'import { default as value } from "dep"; export const result = value;\n'],
  ["export-star", 'export * from "dep";\n'],
  ["export-namespace", 'export * as library from "dep";\n'],
  ["default-reexport", 'export { default as result } from "dep";\n'],
  ["user-tslib", 'import value from "tslib"; export async function read() { return value; }\n'],
  ["occupied-tslib", 'import value from "tslib"; const tslib_1 = 1; export async function read() { return value + tslib_1; }\n'],
  ["source-helper-name", 'const __awaiter = 1; export async function read() { return __awaiter; }\n'],
  ["no-demand", 'export const result = 1;\n'],
];
const inputs = [];
function add(group, shape, text, options, extra = {}) {
  const extension = extra.extension ?? "ts";
  const main = `/project/main.${extension}`;
  const files = extra.files ?? [{ path: main, text }, { path: "/project/ambient.d.ts", text: ambient }];
  const roots = extra.roots ?? [main, "/project/ambient.d.ts"];
  // Both compilers enter their ordinary config-loading/Program command route.
  // Keep the real configuration intact, including options outside old test adapters.
  const compilerOptions = { ...defaults, ...options };
  if (extension === "js") Object.assign(compilerOptions, { allowJs: true, checkJs: true });
  inputs.push({ case_id: `import-helpers/${group}/${shape}`, roots, files, options: {},
    config: JSON.stringify({ compilerOptions, files: roots.map(name => name.slice('/project/'.length)) }),
    witness: { group, shape, compiler_options: compilerOptions } });
}
for (const module of ["commonjs", "amd", "umd"])
  for (const target of ["es5", "es2015"])
    for (const [shape, text] of shapes) add(`${module}/${target}`, shape, text, { module, target });
for (const module of ["commonjs", "amd", "umd"]) {
  add(`${module}/options`, "import-helpers-off", shapes[0][1], { module, target: "es2015", importHelpers: false });
  add(`${module}/options`, "no-emit-helpers", shapes[0][1], { module, target: "es2015", noEmitHelpers: true });
  add(`${module}/options`, "interop-off", shapes[1][1], { module, target: "es2015", esModuleInterop: false });
  add(`${module}/options`, "error-blocked", 'export async function read() { return missing; }\n', { module, target: "es2015", noEmitOnError: true });
}
for (const [shape, text] of [
  ["script", 'async function read() { return 1; }\n'],
  ["commonjs-exports", 'exports.read = async function read() { return 1; };\n'],
  ["commonjs-require", 'const library = require("dep"); async function read() { return library.item; }\n'],
]) for (const importHelpers of [true, false])
  add(`commonjs/javascript`, `${shape}/${importHelpers ? 'imported' : 'inline'}`, text,
    { module: "commonjs", target: "es2015", importHelpers }, { extension: "js" });
for (const module of ["amd", "umd"])
  for (const importHelpers of [true, false])
    add(`${module}/javascript`, `commonjs-script/${importHelpers ? 'imported-option' : 'inline'}`,
      'exports.read = async function read() { return 1; };\n',
      { module, target: "es2015", importHelpers }, { extension: "js" });
add('esnext/control', 'named-helper-import', shapes[0][1], { module: "esnext", target: "es2015" });
add('esnext/control', 'no-demand', shapes[11][1], { module: "esnext", target: "es2015" });
add('esnext/control', 'helper-name-collision', shapes[10][1], { module: "esnext", target: "es2015" });
add('esnext/control', 'import-helpers-off', shapes[0][1], { module: "esnext", target: "es2015", importHelpers: false });
for (const order of ["helper-first", "helper-last", "both-helpers"]) {
  const a = { path: '/project/a.ts', text: shapes[9][1] };
  const b = { path: '/project/b.ts', text: order === 'both-helpers' ? shapes[8][1] : shapes[11][1] };
  const ordered = order === 'helper-last' ? [b, a] : [a, b];
  add('commonjs/source-isolation', order, '', { module: "commonjs", target: "es2015" },
    { files: [...ordered, { path: '/project/ambient.d.ts', text: ambient }], roots: [...ordered.map(file => file.path), '/project/ambient.d.ts'] });
}
for (const module of ['node16', 'node18', 'node20', 'nodenext']) {
  add('implied-format', module, '', { module, target: 'es2015' }, {
    files: [
      { path: '/project/package.json', text: '{"type":"module"}' },
      { path: '/project/main.ts', text: shapes[0][1] },
      { path: '/project/sub/package.json', text: '{"type":"commonjs"}' },
      { path: '/project/sub/other.ts', text: shapes[8][1] },
      { path: '/project/ambient.d.ts', text: ambient },
    ], roots: ['/project/main.ts', '/project/sub/other.ts', '/project/ambient.d.ts'] });
}
for (const [shape, text, options] of [
  ['declaration-map', shapes[8][1], { declaration: true, declarationMap: true }],
  ['declaration-only', shapes[8][1], { declaration: true, declarationMap: true, emitDeclarationOnly: true }],
  ['import-equals-binding', 'import library = require("tslib"); export async function read() { return library.item; }\n', {}],
]) add('commonjs/adjacent', shape, text, { module: 'commonjs', target: 'es2015', ...options });
add('system/control', 'helper-demand', shapes[0][1], { module: 'system', target: 'es2015' });
add('system/control', 'no-demand', shapes[11][1], { module: 'system', target: 'es2015' });
add('bundle/control', 'import-helpers', shapes[0][1], { module: 'amd', target: 'es2015', outDir: undefined, outFile: '/project/out.js' });
assert.equal(inputs.length, 111);
function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
function write(args, index) {
  const [name, text, bom, onError, sources, data] = args;
  const bytes = Buffer.from(text), materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), bytes]) : bytes;
  return { index, path: name, kind: ts.isDeclarationFileName(name) ? "declaration" : name.endsWith(".map") && ts.isDeclarationFileName(name.slice(0, -4)) ? "declaration-map" : name.endsWith(".map") ? "source-map" : name.endsWith(".mjs") ? "mjs" : name.endsWith(".cjs") ? "cjs" : "javascript",
    callback_utf8_base64: bytes.toString("base64"), callback_utf8_bytes: bytes.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"), materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined, source_files: sources?.map(source => source.fileName) ?? null,
    data_present: data !== undefined, data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
    data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null };
}
function sourceMaps(maps) {
  return maps?.map(entry => {
    assert.deepEqual(Object.keys(entry).sort(), ["inputSourceFileNames", "sourceMap"]);
    return { input_source_file_names: entry.inputSourceFileNames, source_map_json: JSON.stringify(entry.sourceMap) };
  }) ?? null;
}
function observe(input) {
  const sensitive = input.use_case_sensitive_file_names ?? true;
  const canonical = ts.createGetCanonicalFileName(sensitive);
  const files = new Map(input.files.map(file => [canonical(file.path), file.text]));
  const libraryRoot = path.join(root, "vendor/typescript-6.0.3/lib");
  const library = name => /^\/lib\/lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(name) && fs.existsSync(path.join(libraryRoot, path.basename(name)));
  const read = name => files.get(canonical(ts.normalizePath(name))) ?? (library(name) ? fs.readFileSync(path.join(libraryRoot, path.basename(name)), "utf8") : undefined);
  const overlay = createHermeticDirectoryOverlay(files.keys(), { currentDirectory: "/project", useCaseSensitiveFileNames: sensitive,
    fallbackHost: { directoryExists: name => name === "/lib", getDirectories: () => [] } });
  const host = { ...ts.createCompilerHost(input.options, true), ...overlay,
    getCurrentDirectory: () => "/project", getDefaultLibFileName: options => "/lib/" + ts.getDefaultLibFileName(options),
    getDefaultLibLocation: () => "/lib", useCaseSensitiveFileNames: () => sensitive, getCanonicalFileName: canonical,
    readFile: read, fileExists: name => files.has(canonical(ts.normalizePath(name))) || library(name),
    getSourceFile: (name, options) => { const text = read(name); return text === undefined ? undefined : ts.createSourceFile(name, text, options, true); },
    writeFile: () => assert.fail("unexpected host write") };
  let options = input.options, roots = input.roots ?? input.files.map(file => file.path), errors = [];
  if (input.config) {
    const configPath = "/project/tsconfig.json";
    const parsed = ts.parseJsonSourceFileConfigFileContent(ts.parseJsonText(configPath, input.config),
      { ...host, readDirectory: () => roots }, "/project", undefined, configPath);
    options = parsed.options; roots = parsed.fileNames; errors = parsed.errors;
  }
  const program = ts.createProgram({ rootNames: roots, options, host, configFileParsingDiagnostics: errors });
  const writes = [], reported = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => reported.push(diagnostic(d)), s => status.push(s), undefined,
    (...args) => writes.push(write(args, writes.length)));
  assert.ok(result);
  return { writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic), emitted_files: result.emittedFiles ?? null, source_maps: sourceMaps(result.sourceMaps) },
    status_writes: status, exit_code: exit };
}
const cases = inputs.map(input => {
  const first = observe(input); assert.deepEqual(observe(input), first, input.case_id);
  return { ...input, typescript_observation: first };
});
const artifact = { version: 1, typescript: ts.version,
  source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), repetitions: 2, cases };
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered);
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(`External helper imports: ${cases.length} cases, two identical complete observations each`);
