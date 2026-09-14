// Complete pinned commands for the H2.5h parameter pass boundary.
// --write creates immutable observations; --check re-observes all cases twice.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const read = name => fs.readFileSync(path.join(root, name));
const selection = JSON.parse(read("docs/design/greenfield/slices/h2-5h-parameter-temporaries-selection.v1.json"));
const destination = "crates/compiler/tests/fixtures/h2-5h-parameter-temporaries.json";
assert.equal(ts.version, "6.0.3");
assert.equal(sha(read("vendor/typescript-6.0.3/lib/typescript.js")), selection.files["vendor/typescript-6.0.3/lib/typescript.js"]);
assert.ok(["--write", "--check"].includes(process.argv[2]));
const parents = new Map();
for (const pinned of selection.cases) if (!parents.has(pinned.artifact)) {
  const bytes = read(pinned.artifact);
  assert.equal(sha(bytes), selection.files[pinned.artifact]);
  parents.set(pinned.artifact, JSON.parse(bytes));
}
// Same effectiveCompilerOptions defaults as H2.5g/H2.5h. Only the explicitly
// inventoried settings below are supported; no unknown option is dropped.
function optionsFor(input) {
  const options = {noResolve:false, newLine:ts.NewLineKind.CarriageReturnLineFeed,
    noErrorTruncation:true, skipDefaultLibCheck:true};
  for (const {name, value} of input.settings) {
    if (name === "noTypesAndSymbols") { assert.equal(value, "true"); continue; }
    if (name === "target") {
      const targets = {es5:1,es2015:2,es2019:6,es2020:7,esnext:99};
      assert.ok(Object.hasOwn(targets,value)); options.target=targets[value];
    } else if (name === "newLine") {
      assert.ok(["lf","crlf"].includes(value)); options.newLine=value === "lf" ? 1 : 0;
    } else {
      assert.ok(["strict","removeComments","sourceMap","emitBOM","noEmit","noEmitOnError"].includes(name),name);
      assert.ok(["true","false"].includes(value)); options[name]=value === "true";
    }
  }
  return options;
}
const inputs=[];
for (const pinned of selection.cases) {
  const row=parents.get(pinned.artifact).cases.find(c=>c.case_id===pinned.case_id);
  assert.equal(sha(JSON.stringify(row)),pinned.sha256_json_stringify);
  assert.equal(row.execution_route,"qualified-vfs");
  inputs.push({case_id:row.case_id,group:"original",floor:"established",input:row.input,
    original:{artifact:pinned.artifact,case_id:row.case_id,row_sha256:pinned.sha256_json_stringify},frozen:row.typescript_observation});
  if(pinned.role === "frozen-adjacent-control") {
    const input=structuredClone(row.input);
    const setting=input.settings.find(s=>s.name === "target"); assert.equal(setting.value,"es2015"); setting.value="esnext";
    inputs.push({case_id:row.case_id.replace(/es2015$/u,"esnext"),group:"original",floor:"established",input,
      derived_from:{artifact:pinned.artifact,case_id:row.case_id,only_setting_change:{target:"esnext"}}});
  }
}
const declarations="declare function get(): any;\ndeclare let simple: any;\ndeclare let holder: any;\n";
function control(id,body,targets=["es5","es2015"],settings=[],lineEnding="lf") {
  for(const target of targets) {
    let text=declarations+body+"\n";
    if(lineEnding === "crlf")text=text.replaceAll("\n","\r\n");
    const bytes=Buffer.from(text),file="/.src/main.ts";
    inputs.push({case_id:`parameter-temporaries/${id}/${target}`,group:"focused",floor:"map-family",
      branch:id,input:{current_directory:"/.src",roots:[file],virtual_config:null,vfs_symlinks:[],
        settings:[{name:"strict",value:"false"},{name:"target",value:target},...settings.map(([name,value])=>({name,value}))],
        files:[{unit:0,path:file,utf8_base64:bytes.toString("base64"),utf8_bytes:bytes.length,utf8_sha256:sha(bytes)}]}});
  }
}
control("simple-nullish","function f(x = simple ?? 1) { return x; }");
control("call-nullish","function f(x = get() ?? 1) { return x; }");
control("simple-optional","function f(x = simple?.value) { return x; }");
control("optional-call-receiver","function f(x = get().method?.()) { return x; }");
control("optional-chain-receiver","function f(x = get()?.method()) { return x; }");
control("pattern-default-computed","function f({[get()?.key]: x = 1} = get() ?? {}) { return x; }");
control("multiple-rest","function f(x = get()?.value, y = get() ?? 1, ...rest: any[]) { return [x, y, rest]; }");
control("empty-pattern","function f({} = get()?.value) { return 1; }");
control("arrow-concise-body","const f = (x = get()?.value) => get() ?? x;");
control("arrow-block-body","const f = (x = get() ?? 1) => { return get()?.value || x; };");
control("nested-functions","function f(x = get()?.value) { function inner(y = get() ?? 1) { return get()?.value || y; } return inner(x); }");
control("function-expression-collision","const f = function(x = get() ?? 1) { const _a = 2; return x + _a; };");
control("object-method","const object = { f(x = get()?.value) { return x; } };");
control("class-method","class C { f(x = get() ?? 1) { return x; } }");
control("nested-parameter-function","function f(x = (y = get()?.value) => y) { return x; }");
control("exponentiation-assignment","function f(x = holder.value **= get()) { return x; }");
for(const [id,operator]of[["nullish","??="],["and","&&="],["or","||="]]) {
  control(`logical-${id}-assignment`,`function f(x = get().value ${operator} get()) { return x; }`,["es5","es2020"]);
}
control("optional-target-boundary","function f(x = get()?.value) { return x; }",["es2019","es2020"]);
control("nullish-target-boundary","function f(x = get() ?? 1) { return x; }",["es2019","es2020"]);
const comments='// header\n\n"custom";\nfunction f(/* parameter */ x = /* before */ get()?.value /* after */) { /* body */ return x; }';
control("comments-lf",comments,["es5","es2015"],[["newLine","lf"]]);
control("comments-crlf",comments,["es5","es2015"],[["newLine","crlf"]],"crlf");
control("comments-removed",comments,["es5","es2015"],[["removeComments","true"]]);
control("source-map",comments,["es5","es2015"],[["sourceMap","true"]]);
control("bom",comments,["es5","es2015"],[["emitBOM","true"]]);
control("no-emit",comments,["es5","es2015"],[["noEmit","true"]]);
control("no-emit-on-error",comments,["es5","es2015"],[["noEmitOnError","true"]]);
assert.equal(new Set(inputs.map(c=>c.case_id)).size,inputs.length);
function diagnostic(d) {
  return {code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null};
}
function write(args, index) {
  const [name, text, bom, onError, sources, data] = args;
  assert.ok(name.endsWith(".js") || name.endsWith(".js.map"));
  assert.ok(text.isWellFormed(), "scalar fixture must reject non-scalar callback text");
  if (data !== undefined) assert.deepEqual(Object.keys(data).sort(), ["diagnostics", "sourceMapUrlPos"]);
  const bytes = Buffer.from(text);
  const materialized = bom ? Buffer.concat([Buffer.from([239,187,191]), bytes]) : bytes;
  return {index, path: name, kind: name.endsWith(".map") ? "source-map" : "javascript",
    callback_utf8_base64: bytes.toString("base64"), callback_utf8_sha256: sha(bytes), callback_utf8_bytes: bytes.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"),
    materialized_utf8_sha256: sha(materialized), materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined, source_files: sources?.map(s => s.fileName) ?? null,
    data_present: data !== undefined, data_keys: data === undefined ? null : Object.keys(data),
    data_source_map_url_pos: data?.sourceMapUrlPos ?? null, data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null};
}
function observe(input, options) {
  const files = new Map(input.files.map(f => [f.path, Buffer.from(f.utf8_base64, "base64").toString("utf8")]));
  const base = ts.createCompilerHost(options, true);
  const dirs = createHermeticDirectoryOverlay(files.keys(), {currentDirectory: input.current_directory,
    useCaseSensitiveFileNames: true, fallbackHost: base});
  // Same createProgramCase host policy as h2-5h-qualification.mjs: exact
  // VFS spellings first, standard compiler host fallback for the libraries.
  const host = {...base, getCurrentDirectory: () => input.current_directory,
    useCaseSensitiveFileNames: () => true, getCanonicalFileName: name => name, trace() {},
    fileExists: name => files.has(ts.normalizePath(name)) || base.fileExists(ts.normalizePath(name)),
    readFile: name => files.get(ts.normalizePath(name)) ?? base.readFile(ts.normalizePath(name)),
    directoryExists: name => dirs.directoryExists(name), getDirectories: name => dirs.getDirectories(name),
    realpath: name => files.has(ts.normalizePath(name)) ? ts.normalizePath(name) : (base.realpath?.(ts.normalizePath(name)) ?? ts.normalizePath(name)),
    getSourceFile(name, languageVersion) {
      const normalized = ts.normalizePath(name), text = files.get(normalized);
      return text === undefined ? base.getSourceFile(name, languageVersion)
        : ts.createSourceFile(normalized, text, languageVersion, true, ts.getScriptKindFromFileName(normalized));
    }, writeFile: () => assert.fail("unexpected host write")};
  const program = ts.createProgram(input.roots, options, host);
  const writes = [], reported = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => reported.push(diagnostic(d)),
    s => status.push(s), undefined, (...args) => writes.push(write(args, writes.length)));
  assert.ok(result);
  const maps = result.sourceMaps?.map(entry => {
    assert.deepEqual(Object.keys(entry).sort(), ["inputSourceFileNames", "sourceMap"]);
    return {input_source_file_names: entry.inputSourceFileNames, source_map_json: JSON.stringify(entry.sourceMap)};
  }) ?? null;
  return {writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: {emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
      emitted_files: result.emittedFiles ?? null, source_maps: maps}, status_writes: status, exit_code: exit};
}
// Select only the original artifact's fields, with no value normalization.
// Every old observed value must agree; the new artifact adds evidence.
function oldShape(actual, expected) {
  if (Array.isArray(expected)) {
    assert.ok(Array.isArray(actual)); assert.equal(actual.length, expected.length);
    return expected.map((value, i) => oldShape(actual[i], value));
  }
  if (expected !== null && typeof expected === "object") {
    assert.ok(actual !== null && typeof actual === "object");
    return Object.fromEntries(Object.entries(expected).map(([key, value]) => {
      assert.ok(Object.hasOwn(actual, key), key); return [key, oldShape(actual[key], value)];
    }));
  }
  return actual;
}
const cases=inputs.map(({frozen,...entry})=>{
  const {input}=entry;
  assert.equal(input.virtual_config,null); assert.deepEqual(input.vfs_symlinks,[]);
  for(const file of input.files){const bytes=Buffer.from(file.utf8_base64,"base64");
    assert.equal(bytes.length,file.utf8_bytes);assert.equal(sha(bytes),file.utf8_sha256);
    assert.equal(Buffer.from(new TextDecoder("utf-8",{fatal:true}).decode(bytes)).toString("base64"),file.utf8_base64);}
  const options=optionsFor(input),first=observe(input,options),second=observe(input,options);
  assert.deepEqual(second,first,entry.case_id);
  if(frozen){const {run_fingerprint_sha256,...old}=frozen;assert.deepEqual(oldShape(first,old),old,entry.case_id);}
  console.log(`${entry.case_id}: full command x2${frozen ? "; frozen fields unchanged" : ""}`);
  return {...entry,options,typescript_observation:first,typescript_run_sha256:[sha(JSON.stringify(first)),sha(JSON.stringify(second))]};
});
const artifact={version:1,status:"Pinned upstream complete commands; native results recorded separately",
  typescript:ts.version,compiler_sha256:sha(read("vendor/typescript-6.0.3/lib/typescript.js")),
  selection_sha256:sha(read("docs/design/greenfield/slices/h2-5h-parameter-temporaries-selection.v1.json")),
  observer_sha256:sha(fs.readFileSync(import.meta.filename)),repetitions:2,
  parents:[...parents.keys()].map(name=>({path:name,sha256:sha(read(name))})),
  complete_program_executions:cases.length*2,cases};
const output=path.join(root,destination),rendered=JSON.stringify(artifact,null,2)+"\n";
if(process.argv[2] === "--write")fs.writeFileSync(output,rendered,{flag:"wx"});
else assert.equal(fs.readFileSync(output,"utf8"),rendered);
console.log(JSON.stringify({destination,cases:cases.length,complete_program_executions:cases.length*2,sha256:sha(read(destination))}));
