// H2.8a Token comment phases. Expectations are complete TS commands.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/compiler/tests/fixtures/token-comment-phases.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const defaults={strict:true,allowJs:true,checkJs:true,sourceMap:true,
 module:ts.ModuleKind.ESNext,skipDefaultLibCheck:true,noErrorTruncation:true,
 newLine:ts.NewLineKind.CarriageReturnLineFeed,outDir:"/project/out"};
const inputs=[];
const targets=[["es2015",ts.ScriptTarget.ES2015],["esnext",ts.ScriptTarget.ESNext]];
const modules=[["commonjs",ts.ModuleKind.CommonJS],["esnext",ts.ModuleKind.ESNext]];
function add(id,text,options={},extension="ts"){
 const main=`/project/main.${extension}`;
 inputs.push({case_id:`token-comment-phases/${id}`,roots:[main],files:[{path:main,text}],options:{...defaults,target:ts.ScriptTarget.ESNext,...options}});
}
const modifiers=[
 ["export-variable","export /* export */ const value = 1;\n"],
 ["export-function","export /* export */ function read() { return 1; }\n"],
 ["async-function","export async /* async */ function read() { return 1; }\n"],
 ["async-method","export class Box { async /* async */ read() { return 1; } }\n"],
 ["static-method","export class Box { static /* static */ read() { return 1; } }\n"],
 ["static-getter","export class Box { static /* static */ get value() { return 1; } }\n"],
 ["export-class","export /* export */ class /* class */ Box {}\n"],
 ["default-class","export /* export */ default /* default */ class /* class */ {}\n"],
];
for(const [name,text] of modifiers)for(const [targetName,target] of targets)for(const [moduleName,module] of modules)
 add(`modifiers/${name}/${targetName}/${moduleName}`,text,{target,module});
for(const [name,text] of modifiers.filter(([name])=>["export-class","default-class","static-method","async-method"].includes(name)))for(const [targetName,target] of targets)
 add(`javascript/${name}/${targetName}`,text,{target},"js");
const declarations=[
 ["readonly-property","export class Box { readonly /* readonly */ value = 1; }\n"],
 ["abstract-class","export abstract /* abstract */ class Box { abstract /* method */ read(): number; }\n"],
 ["declare-variable","export declare /* declare */ const value: number;\n"],
 ["interface","export /* export */ interface Box { readonly /* readonly */ value: number; }\n"],
 ["type-alias","export /* export */ type Box = { readonly /* readonly */ value: number };\n"],
 ["private-constructor","export class Box { private /* private */ constructor() {} }\n"],
 ["variance","export interface Box<out /* out */ T> { readonly value: T; }\n"],
 ["const-type-parameter","export function read<const /* const */ T>(value: T): T { return value; }\n"],
 ["index-signature","export interface Box { readonly /* readonly */ [key: string]: number; }\n"],
 ["constructor-type","export type Box = abstract /* abstract */ new () => object;\n"],
 ["enum","export /* export */ enum Box { value = 1 }\n"],
 ["namespace","export /* export */ namespace Box { export const value = 1; }\n"],
];
for(const [name,text] of declarations)for(const emitDeclarationOnly of [false,true])
 add(`declarations/${name}/${emitDeclarationOnly?"only":"all"}`,text,{declaration:true,declarationMap:true,emitDeclarationOnly,sourceMap:!emitDeclarationOnly});
const erased=[
 ["private-field","export class Box { private /* private */ value = 1; read() { return this.value; } }\n"],
 ["readonly-field","export class Box { readonly /* readonly */ value = 1; }\n"],
 ["parameter-property","export class Box { constructor(public /* public */ readonly /* readonly */ value: number) {} }\n"],
 ["abstract-method","export abstract class Box { protected /* protected */ abstract /* abstract */ read(): number; }\n"],
];
for(const [name,text] of erased)for(const [targetName,target] of targets)add(`erased/${name}/${targetName}`,text,{target});
for(const [name,text] of [
 ["class","function decorate(value: any) { return value; }\n@decorate\nexport /* export */ class /* class */ Box {}\n"],
 ["method","function decorate(...args: any[]) {}\nexport class Box { @decorate static /* static */ read() { return 1; } }\n"],
])for(const [targetName,target] of targets)add(`decorators/${name}/${targetName}`,text,{target,experimentalDecorators:true});
for(const [name,text] of modifiers.slice(2))add(`removed/${name}`,text,{removeComments:true});
add("unicode/class","export /* 😀 export */ default /* 😀 default */ class /* 😀 class */ Box {}\r\n");
add("unicode/method","export class Box { static // 😀 static\r\nread() { return 1; } }\r\n");
for(const [name,text] of [
 ["import","export /* export */ import value from './dep'; export { value };\n"],
 ["export","declare /* declare */ export { value }; const value = 1;\n"],
 ["assignment","declare /* declare */ export default 1;\n"],
])add(`recovery/${name}`,text);
const spread=[
 ["array",c=>`const values = [1, 2]; const copy = [...${c}values];\n`],
 ["call",c=>`function use(...args: number[]) {} const values = [1, 2]; use(...${c}values);\n`],
 ["construct",c=>`class Box { constructor(...args: number[]) {} } const values = [1, 2]; new Box(...${c}values);\n`],
 ["object",c=>`const source = { value: 1 }; const copy = { ...${c}source };\n`],
 ["parenthesized",c=>`const values = [1, 2]; const copy = [...${c}(true ? values : [])];\n`],
 ["list-boundaries",c=>`const values = [1, 2]; const copy = [0, ...${c}values /* tail */, 3];\n`],
];
for(const [name,make] of spread)for(const [commentName,comment] of [["block","/* spread */ "],["line","// spread\n"],["newline","\n/* spread */\n"]])for(const [targetName,target] of targets)
 add(`spread/${name}/${commentName}/${targetName}`,make(comment),{target});
for(const [name,make] of spread)add(`spread/${name}/downlevel`,make("/* spread */ "),{target:ts.ScriptTarget.ES5});
assert.equal(inputs.length,129);
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
console.log(`Token comment phases: ${cases.length} cases, two identical complete observations each`);
