// Complete commands for the remaining helper diagnostic call sites.
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
 ["for-of", "__values", "export function f(values: any) { for (const x of values) {} }"],
 ["for-await", "__asyncValues", "export async function f(values: any) { for await (const x of values) {} }"],
 ["yield-star", "__values", "export function* f(values: any) { yield* values; }"],
 ["async-yield-star-delegator", "__asyncDelegator", "export async function* f(values: any) { yield* values; }"],
 ["async-yield-star-values", "__asyncValues", "export async function* f(values: any) { yield* values; }"],
 ["tagged-template", "__makeTemplateObject", "declare const tag: any; export const x = tag`x`;"],
 ["private-in", "__classPrivateFieldIn", "export class C { #x = 0; f(value: object) { return #x in value; } }"],
 ["private-destructuring-set", "__classPrivateFieldSet", "export class C { #x = 0; f(value: any) { [this.#x] = value; } }"],
 ["legacy-param", "__param", "declare function dec(...args: any[]): any; export class C { constructor(@dec value: number) {} }", {experimentalDecorators: true}],
 ["legacy-metadata", "__metadata", "declare function dec(...args: any[]): any; @dec export class C { constructor(value: number) {} }", {experimentalDecorators: true, emitDecoratorMetadata: true}],
];
function add(shape, helper, text, target, control, extra = {}) {
 const missing = control !== "present";
 const ambient = 'declare module "tslib" { ' + helperNames.filter(name => !missing || name !== helper)
  .map(name => `export function ${name}(a: any, b?: any, c?: any, d?: any, e?: any): any;`).join(" ") + ' }\n';
 inputs.push({case_id: `emitter-helper-probes/${shape}/${target}/${control}`,
  roots: ["/project/main.ts", "/project/ambient.d.ts"],
  files: [{path: "/project/main.ts", text: text + "\n"}, {path: "/project/ambient.d.ts", text: ambient}], options: {},
  config: JSON.stringify({compilerOptions: {target, module: "commonjs", lib: ["esnext"], strict: false,
    importHelpers: true, downlevelIteration: true, skipDefaultLibCheck: true,
    noErrorTruncation: true, sourceMap: true, outDir: "/project/out", ignoreDeprecations: "6.0", ...extra},
    files: ["main.ts", "ambient.d.ts"]})});
}
for (const [shape, helper, text, options = {}] of scenarios) {
 for (const target of ["es5", "es2015", "es2022", "esnext"])
  for (const control of ["missing", "present"]) add(shape, helper, text, target, control, options);
 add(shape, helper, text, "es5", "import-helpers-off", {...options, importHelpers: false});
 add(shape, helper, text, "es5", "no-check", {...options, noCheck: true});
 add(shape, helper, text, "es5", "downlevel-iteration-off", {...options, downlevelIteration: false});
}
const param = scenarios.find(([shape]) => shape === "legacy-param");
const metadata = scenarios.find(([shape]) => shape === "legacy-metadata");
for (const target of ["es5", "esnext"]) {
 add(...param.slice(0, 3), target, "standard-decorators", {experimentalDecorators: false});
 add(...metadata.slice(0, 3), target, "metadata-off", {experimentalDecorators: true, emitDecoratorMetadata: false});
 add(...metadata.slice(0, 3), target, "verbatim-module-syntax", {...metadata[3], verbatimModuleSyntax: true});
 add(...metadata.slice(0, 3), target, "standard-decorators", {...metadata[3], experimentalDecorators: false});
 add("ambient-class-metadata", "__metadata", "declare function dec(...args: any[]): any; @dec export declare class C { constructor(value: number); }", target, "missing", metadata[3]);
 add("ambient-property-metadata", "__metadata", "declare function dec(...args: any[]): any; export declare class C { @dec value: number; }", target, "missing", metadata[3]);
}
assert.equal(inputs.length, 122);

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
const destination = path.join(root, "crates/compiler/tests/fixtures/emitter-helper-probes.json");
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered, {flag: "wx"});
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(`Helper diagnostic boundaries: ${cases.length} cases, two identical complete observations each`);
