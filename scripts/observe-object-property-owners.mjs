// Complete TypeScript observations for object property modifier ownership.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/compiler/tests/fixtures/object-property-owners.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") assert.ok(!fs.existsSync(destination));
const defaults = {strict:true,allowJs:true,checkJs:true,sourceMap:true,
 module:ts.ModuleKind.ESNext,skipDefaultLibCheck:true,noErrorTruncation:true,
 newLine:ts.NewLineKind.CarriageReturnLineFeed,outDir:"/project/out"};
const shapes = [
 ["export-property", "const value = { export field: 4 };\n"],
 ["export-shorthand", "const field = 4; const value = { export field };\n"],
 ["property-comments", "const value = { /* member */ export /* modifier */ field: /* initializer */ 4 /* tail */ };\n"],
 ["shorthand-comments", "const field = 4; const value = { /* member */ export /* modifier */ field /* tail */ };\n"],
 ["shorthand-default", "let field = 1; ({ export field = (2, 3) } = {});\n"],
 ["static-property", "const value = { static field: 4 };\n"],
 ["keyword-property-name", "const value = { export: 4, static: 5, async: 6 };\n"],
 ["exported-variable", "export const value = { field: 4 };\n"],
];
const inputs = [];
function add(extension, band, target, shape, text, extra = {}) {
 const main = `/project/main.${extension}`;
 inputs.push({case_id:`object-property-owners/${extension}/${band}/${shape}`,
 roots:[main],files:[{path:main,text}],options:{...defaults,target,...extra}});
}
for (const extension of ["js", "ts"]) {
 for (const [band,target] of [["es2015",ts.ScriptTarget.ES2015],["esnext",ts.ScriptTarget.ESNext]])
  for (const [shape,text] of shapes) add(extension,band,target,shape,text);
 for (const [shape,text] of shapes.filter(([shape]) => ["property-comments","shorthand-comments"].includes(shape)))
  add(extension,"comments-removed",ts.ScriptTarget.ESNext,shape,text,{removeComments:true});
 for (const [shape,text] of shapes.filter(([shape]) => ["export-property","export-shorthand"].includes(shape)))
  add(extension,"blocked",ts.ScriptTarget.ESNext,shape,text,{noEmitOnError:true});
 for (const [shape,text] of shapes.filter(([shape]) => ["export-property","exported-variable"].includes(shape))) {
  add(extension,"commonjs",ts.ScriptTarget.ESNext,shape,text,{module:ts.ModuleKind.CommonJS});
  add(extension,"declaration",ts.ScriptTarget.ESNext,shape,text,{declaration:true,declarationMap:true});
 }
}
assert.equal(inputs.length,48);
assert.equal(new Set(inputs.map(row => row.case_id)).size,48);
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
 const first = observe(input); assert.deepEqual(observe(input),first,input.case_id);
 console.log(JSON.stringify({case_id:input.case_id,complete_observations:2,
  diagnostics:first.reported_diagnostics.length,writes:first.writes.length,exit_code:first.exit_code}));
 return {...input,typescript_observation:first};
});
const artifact = {version:1,typescript:ts.version,source_commit:"050880ce59e30b356b686bd3144efe24f875ebc8",
 compiler_sha256:sha256(fs.readFileSync(path.join(root,"vendor/typescript-6.0.3/lib/typescript.js"))),
 observer_sha256:sha256(fs.readFileSync(import.meta.filename)),repetitions:2,program_executions:96,cases};
const rendered=JSON.stringify(artifact,null,2)+"\n";
if(process.argv[2]==="--write")fs.writeFileSync(destination,rendered);
else assert.equal(fs.readFileSync(destination,"utf8"),rendered);
console.log(JSON.stringify({destination,cases:cases.length,complete_program_executions:96,sha256:sha256(fs.readFileSync(destination))}));
