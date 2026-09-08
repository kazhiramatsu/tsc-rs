// H2.8a exported declaration name witnesses. Expectations are complete TS commands.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/compiler/tests/fixtures/export-name-syntax.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const defaults = { strict:true, allowJs:true, checkJs:true, declaration:true,
  skipDefaultLibCheck:true, noErrorTruncation:true, newLine:ts.NewLineKind.CarriageReturnLineFeed,
  outDir:"/project/out" };
const shapes = [
 ["local-default", 'const value = 1;\nexport { value as "default" };\n'],
 ["local-identifier", 'const value = 1;\nexport { value as "named" };\n'],
 ["local-unicode", 'const value = 1;\nexport { value as "名前" };\n'],
 ["local-nonidentifier", 'const value = 1;\nexport { value as "a-b" };\n'],
 ["function-default", 'function value() { return 1; }\nexport { value as "default" };\n'],
 ["class-default", 'class value {}\nexport { value as "default" };\n'],
 ["duplicate-quoted-first", 'const value = 1;\nexport { value as "default", value as default };\n'],
 ["duplicate-identifier-first", 'const value = 1;\nexport { value as default, value as "default" };\n'],
 ["external-default", 'export { "default" as named } from "./lib";\n'],
 ["external-named", 'export { "value" as named } from "./lib";\n'],
 ["external-unicode", 'export { "名前" as named } from "./lib";\n'],
 ["namespace-named", 'export * as "named" from "./lib";\n'],
 ["imported-default", 'import { value } from "./lib";\nexport { value as "default" };\n'],
 ["update-default", 'let value = 1;\nexport { value as "default" };\nvalue++;\n'],
 ["direct-then-quoted", 'export const value = 1;\nexport { value as "value" };\n'],
 ["quoted-then-direct", 'export { value as "value" };\nexport const value = 1;\n'],
];
const inputs = [];
for (const [modeName,module] of [["cjs",ts.ModuleKind.CommonJS],["amd",ts.ModuleKind.AMD],["umd",ts.ModuleKind.UMD],["system",ts.ModuleKind.System]]) {
 for (const [targetName,target] of [["es5",ts.ScriptTarget.ES5],...(modeName === "cjs" ? [["es2015",ts.ScriptTarget.ES2015]] : [])]) {
  for (const extension of ["js","ts"]) for (const [shape,text] of shapes) {
   const main = "/project/main."+extension;
   inputs.push({case_id:`${modeName}/${extension}/${targetName}/${shape}`,roots:[main],
    files:[{path:main,text},{path:"/project/lib."+extension,
     text:'export const value = 1;\nexport { value as 名前 };\nexport default value;\n'}],
    options:{...defaults,module,target}});
  }
 }
}
function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
function write(args, index) {
  const [name, text, bom, onError, sources, data] = args;
  const bytes = Buffer.from(text), materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), bytes]) : bytes;
  return { index, path: name, kind: ts.isDeclarationFileName(name) ? "declaration" : name.endsWith(".mjs") ? "mjs" : name.endsWith(".cjs") ? "cjs" : "javascript",
    callback_utf8_base64: bytes.toString("base64"), callback_utf8_bytes: bytes.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"), materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined, source_files: sources?.map(source => source.fileName) ?? null,
    data_present: data !== undefined, data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
    data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null };
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
  assert.equal(result.sourceMaps, undefined);
  return { writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic), emitted_files: result.emittedFiles ?? null, source_maps: null },
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
console.log(`Export name syntax: ${cases.length} cases, two identical complete observations each`);
