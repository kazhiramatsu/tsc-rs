// Complete commands for class helper target, decorator and named-evaluation gates.
// The host and tuple match the established import-helper command observer.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const inputs = [];
const helperNames = "__extends __assign __rest __decorate __metadata __param __esDecorate __runInitializers __awaiter __generator __values __read __spreadArray __await __asyncGenerator __asyncDelegator __asyncValues __exportStar __importStar __importDefault __makeTemplateObject __classPrivateFieldGet __classPrivateFieldSet __classPrivateFieldIn __setFunctionName __propKey __addDisposableResource __disposeResources".split(" ");
const scenarios = [
 ["decl-static-initializer", "@dec export class C { static x = 1 }"],
 ["decl-instance-method", "@dec export class C { @dec m() {} }"],
 ["decl-malformed-accessor", "@dec export abstract class C { @dec abstract get x(): number; @dec set x(v: number) {} }"],
 ["decl-accessor-pair", "@dec export class C { @dec get x() { return 1 } set x(v: number) {} }"],
 ["decl-dynamic-accessor", "@dec export class C { @dec get [key]() { return 1 } set [key](v: number) {} }"],
 ["decl-static-private", "@dec export class C { static #x = 1 }"],
 ["decl-anonymous-default", "export default @dec class {}"],
 ["expr-static-block", "export const C = class { static {} };"],
 ["expr-static-private", "export const C = class { static #p = 1 };"],
 ["expr-static-initializer", "export const C = class { static x = 1 };"],
 ["expr-computed", 'export const o = { ["k"]: class { static {} } };'],
 ["expr-proto-identifier", 'export const o = { __proto__: class { static {} } };'],
 ["expr-proto-string", 'export const o = { "__proto__": class { static {} } };'],
 ["expr-proto-computed", 'export const o = { ["__proto__"]: class { static {} } };'],
 ["expr-as", "export const C = (class { static {} } as any);"],
 ["expr-type-assertion", "export const C = <any>(class { static {} });"],
 ["expr-non-null", "export const C = (class { static {} })!;"],
 ["expr-satisfies", "export const C = (class { static {} }) satisfies any;"],
 ["expr-call-argument", "export const value = fn(class { static {} });"],
 ["expr-binding", "export let { C = class { static {} } } = source;"],
 ["expr-logical-assignment", "export let C: any; C ||= class { static {} };"],
 ["expr-decorated", "export const C = @dec class { static x = 1 };"],
 ["expr-named", "export const C = class Named { static {} };"],
 ["expr-first-request", "export const C = class { static {} }; export const D = class { static #p = 1 };"],
];
assert.equal(scenarios.length, 24);
function add(shape, text, target, module, control, extra = {}) {
 const missing = control === "present" ? [] : control === "set-only" ? ["__setFunctionName"]
   : control === "prop-key-only" ? ["__propKey"] : ["__setFunctionName", "__propKey"];
 const ambient = 'declare module "tslib" { ' + helperNames.filter(name => !missing.includes(name))
  .map(name => `export function ${name}(a: any, b?: any, c?: any, d?: any, e?: any): any;`).join(" ") + ' }\n';
 inputs.push({case_id: `emitter-class-helper-gates/${shape}/${target}/${module}/${control}`,
  roots: ["/project/main.ts", "/project/ambient.d.ts"],
  files: [{path: "/project/main.ts", text: "declare const dec: any, key: any, fn: any, source: any;\n" + text + "\n"}, {path: "/project/ambient.d.ts", text: ambient}], options: {},
  config: JSON.stringify({compilerOptions: {target, module, lib: ["esnext"], strict: false,
    importHelpers: true, skipDefaultLibCheck: true, noErrorTruncation: true, sourceMap: true,
    outDir: "/project/out", ignoreDeprecations: "6.0", ...extra}, files: ["main.ts", "ambient.d.ts"]})});
}
for (const [shape, text] of scenarios)
 for (const target of ["es5", "es2022", "esnext"])
  for (const module of ["commonjs", "esnext"])
   for (const control of ["missing", "present"]) add(shape, text, target, module, control);
for (const shape of ["decl-static-initializer", "expr-static-initializer"])
 for (const module of ["commonjs", "esnext"])
  for (const control of ["missing", "present"]) {
   const text = scenarios.find(([name]) => name === shape)[1];
   add(shape, text, "es2021", module, control);
   add(shape + "-assignment-fields", text, "es2022", module, control, {useDefineForClassFields: false});
  }
for (const shape of ["decl-static-private", "expr-static-block"])
 for (const module of ["commonjs", "esnext"])
  for (const control of ["missing", "present"])
   add(shape + "-legacy", scenarios.find(([name]) => name === shape)[1], "es2022", module, control, {experimentalDecorators: true});
for (const shape of ["expr-computed", "expr-proto-computed"])
 for (const module of ["commonjs", "esnext"])
  for (const control of ["set-only", "prop-key-only"])
   add(shape, scenarios.find(([name]) => name === shape)[1], "es2022", module, control);
assert.equal(inputs.length, 320);

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
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  return {...input, typescript_observation: first};
});
const artifact = {version: 1, typescript: ts.version, repetitions: 2,
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases};
const destination = path.join(root, "crates/compiler/tests/fixtures/emitter-class-helper-gates.json");
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered, {flag: "wx"});
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(`Class helper gates: ${cases.length} cases, two identical complete observations each`);
