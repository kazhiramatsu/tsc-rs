// H2.8a Ellipsis comment owners. Expectations are complete TS commands.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/compiler/tests/fixtures/ellipsis-comment-owners.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const defaults={strict:true,allowJs:true,checkJs:true,sourceMap:true,
 module:ts.ModuleKind.ESNext,skipDefaultLibCheck:true,noErrorTruncation:true,
 newLine:ts.NewLineKind.CarriageReturnLineFeed,outDir:"/project/out"};
const inputs=[];
const targets=[["es5",ts.ScriptTarget.ES5],["es2015",ts.ScriptTarget.ES2015],["esnext",ts.ScriptTarget.ESNext]];
const comments=[["block","/* rest */ "],["line","// rest\n"],["newline","\n/* rest */\n"]];
function add(id,text,options={},extension="ts"){
 const main=`/project/main.${extension}`;
 inputs.push({case_id:`ellipsis-comment-owners/${id}`,roots:[main],files:[{path:main,text}],options:{...defaults,target:ts.ScriptTarget.ESNext,...options}});
}
const parameters=[
 ["function",c=>`export function collect(...${c}values: number[]) { return values; }\n`],
 ["arrow",c=>`export const collect = (...${c}values: number[]) => values;\n`],
 ["method",c=>`export class Box { collect(...${c}values: number[]) { return values; } }\n`],
 ["constructor",c=>`export class Box { constructor(...${c}values: number[]) { values.length; } }\n`],
];
for(const [name,make] of parameters)for(const [commentName,comment] of comments)for(const [targetName,target] of targets)
 add(`parameters/${name}/${commentName}/${targetName}`,make(comment),{target,declaration:true,declarationMap:true});
const bindings=[
 ["array",c=>`const values = [1, 2, 3]; export const [first, ...${c}rest] = values;\n`],
 ["object",c=>`const value = {first: 1, second: 2}; export const {first, ...${c}rest} = value;\n`],
];
for(const [name,make] of bindings)for(const [commentName,comment] of comments)for(const [targetName,target] of targets)
 add(`bindings/${name}/${commentName}/${targetName}`,make(comment),{target});
const types=[
 ["named-tuple","export type Values = [... /* rest */ values: number[]];\n"],
 ["rest-type","export type Values = [... /* rest */ number[]];\n"],
 ["call-signature","export interface Collect { (... /* rest */ values: number[]): number[]; }\n"],
 ["construct-signature","export type Box = new (... /* rest */ values: number[]) => object;\n"],
];
for(const [name,text] of types)for(const emitDeclarationOnly of [false,true])
 add(`types/${name}/${emitDeclarationOnly?"only":"all"}`,text,{declaration:true,declarationMap:true,emitDeclarationOnly,sourceMap:!emitDeclarationOnly});
const jsxPrefix="declare namespace JSX { interface IntrinsicElements { div: any; } }\ndeclare const React: any;\nconst props = {id: 'box'}; const values = [1, 2];\n";
const jsxShapes=[
 ["attribute",c=>jsxPrefix+`export const node = <div {...${c}props} />;\n`],
 ["expression",c=>jsxPrefix+`export const node = <div>{...${c}values}</div>;\n`],
];
for(const [name,make] of jsxShapes)for(const [commentName,comment] of comments)for(const [jsxName,jsx] of [["preserve",ts.JsxEmit.Preserve],["react",ts.JsxEmit.React]])
 add(`jsx/${name}/${commentName}/${jsxName}`,make(comment),{jsx},"tsx");
// Keep the original local-namespace diagnostic controls and add clean JSX controls.
const globalJsxPrefix="declare global { namespace JSX { interface IntrinsicElements { div: any; } } }\ndeclare const React: any;\nconst props = {id: 'box'}; const values = [1, 2];\n";
const globalJsxShapes=[
 ["attribute",c=>globalJsxPrefix+`export const node = <div {...${c}props} />;\n`],
 ["expression",c=>globalJsxPrefix+`export const node = <div>{...${c}values}</div>;\n`],
];
for(const [name,make] of globalJsxShapes)for(const [commentName,comment] of comments)for(const [jsxName,jsx] of [["preserve",ts.JsxEmit.Preserve],["react",ts.JsxEmit.React]])
 add(`jsx-global/${name}/${commentName}/${jsxName}`,make(comment),{jsx},"tsx");
const literals=[
 ["string","export const value = '... /* text */';\n"],
 ["template","export const value = `... /* text */`;\n"],
 ["template-substitution","export const value = `... /* ${1} */`;\n"],
 ["block-comment","/* ... // text */\nexport const value = 1;\n"],
 ["line-comment","// ... /* text\nexport const value = 1;\n"],
];
for(const [name,text] of literals)for(const removeComments of [false,true])add(`non-code/${name}/${removeComments?"remove":"retain"}`,text,{removeComments});
for(const [name,make] of parameters)add(`removed/parameter-${name}`,make("/* rest */ "),{removeComments:true});
for(const [name,make] of bindings)add(`removed/binding-${name}`,make("/* rest */ "),{removeComments:true});
add("unicode/parameter",parameters[0][1]("\u2003/* 😀 rest */ "));
add("unicode/binding",bindings[0][1]("\u2003/* 😀 rest */ "));
add("unicode/spread","const values = [1, 2]; export const copy = [...\u2003/* 😀 spread */ values];\n");
assert.equal(inputs.length,105);
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
  return {...input, typescript_observation:first};
});
const artifact = {version:1,typescript:ts.version,source_commit:"050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256:sha256(fs.readFileSync(path.join(root,"vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256:sha256(fs.readFileSync(import.meta.filename)),repetitions:2,cases};
const rendered = JSON.stringify(artifact,null,2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination,rendered);
else assert.equal(fs.readFileSync(destination,"utf8"),rendered);
console.log(`Ellipsis comment owners: ${cases.length} cases, two identical complete observations each`);
