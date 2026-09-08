// H2.8a Static initializer source-map ranges. Expectations are complete TS commands.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/compiler/tests/fixtures/static-initializer-map-ranges.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const defaults = { strict:true, allowJs:true, checkJs:true, declaration:true, sourceMap:true,
  target:ts.ScriptTarget.ES2015, skipDefaultLibCheck:true, noErrorTruncation:true,
  newLine:ts.NewLineKind.CarriageReturnLineFeed, outDir:"/project/out" };
const shapes = [
 ["static-only", 'class Foo { static stat = 10; }'],
 ["instance-only", 'class Foo { member = 10; }'],
 ["public-initialized", 'class Foo { static stat = 10; member = 10; }'],
 ["public-uninitialized", 'class Foo { static stat = 10; member!: number; }'],
 ["abstract-property", 'abstract class Foo { static stat = 10; abstract member: number; }'],
 ["parameter-property", 'class Foo { static stat = 10; constructor(public member = 10) {} }'],
 ["private-field", 'class Foo { static stat = 10; #member = 10; read() { return this.#member; } }'],
 ["private-method", 'class Foo { static stat = 10; #method() {} read() { this.#method(); } }'],
 ["auto-accessor", 'class Foo { static stat = 10; accessor member = 10; }'],
 ["static-this", 'class Foo { static stat = 10; static value = this.stat; }'],
 ["static-super", 'class Base { static stat = 10; } class Foo extends Base { static value = super.stat; }'],
 ["class-expression", 'const Foo = class Named { static stat = 10; member = 10; };'],
 ["anonymous-expression", 'const Foo = class { static stat = 10; member = 10; };'],
 ["nested-reset", 'class Foo { member = 10; static value = class Inner { static stat = 20; }; static stat = 10; }'],
 ["nested-resume", 'class Foo { static value = class Inner { member = 10; static stat = 20; }; static stat = 10; }'],
 ["multiline-comments", 'class Foo {\n  member = 10;\n  /** field */\n  static stat = (\n    10 + 2\n  ); // trailing\n}'],
 ["computed-name", 'class Foo { static ["stat"] = 10; member = 10; }'],
 ["quoted-name", 'class Foo { static "stat" = 10; member = 10; }'],
 ["static-uninitialized", 'class Foo { static stat: number; member = 10; }'],
 ["private-static", 'class Foo { static #stat = 10; static value = 20; read() { return Foo.#stat; } }'],
];
const boundaryShapes = new Set(["static-only", "public-initialized", "public-uninitialized", "auto-accessor"]);
const moduleShapes = new Set(["static-only", "public-initialized", "static-this", "nested-reset"]);
const inputs=[];
function input(targetName,target,define,moduleName,module,shape,text) {
 const main="/project/main.ts";
 inputs.push({case_id:`${targetName}/${define ? "define" : "set"}/${moduleName}/${shape}`,
  roots:[main],files:[{path:main,text:text+"\nexport { Foo };\nexport const tail = 1;\n"}],
  options:{...defaults,allowJs:false,checkJs:false,target,module,useDefineForClassFields:define}});
}
for (const [targetName,target] of [["es5",ts.ScriptTarget.ES5],["es2015",ts.ScriptTarget.ES2015]]) {
 for (const define of [false,true]) for (const [shape,text] of shapes) {
  input(targetName,target,define,"commonjs",ts.ModuleKind.CommonJS,shape,text);
 }
 for (const [shape,text] of shapes.filter(([shape])=>moduleShapes.has(shape))) {
  input(targetName,target,false,"esnext",ts.ModuleKind.ESNext,shape,text);
 }
}
for (const [targetName,target] of [["es2022",ts.ScriptTarget.ES2022],["esnext",ts.ScriptTarget.ESNext]]) {
 for (const define of [false,true]) for (const [shape,text] of shapes.filter(([shape])=>boundaryShapes.has(shape))) {
  input(targetName,target,define,"commonjs",ts.ModuleKind.CommonJS,shape,text);
 }
}
assert.equal(inputs.length,104);
function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
function write(args, index) {
  const [name, text, bom, onError, sources, data] = args;
  const bytes = Buffer.from(text), materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), bytes]) : bytes;
  return { index, path: name, kind: ts.isDeclarationFileName(name) ? "declaration" : name.endsWith(".map") ? "source-map" : name.endsWith(".mjs") ? "mjs" : name.endsWith(".cjs") ? "cjs" : "javascript",
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
  return {...input, typescript_observation:first};
});
const artifact = {version:1,typescript:ts.version,source_commit:"050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256:sha256(fs.readFileSync(path.join(root,"vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256:sha256(fs.readFileSync(import.meta.filename)),repetitions:2,cases};
const rendered = JSON.stringify(artifact,null,2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination,rendered);
else assert.equal(fs.readFileSync(destination,"utf8"),rendered);
console.log(`Static initializer source-map ranges: ${cases.length} cases, two identical complete observations each`);
