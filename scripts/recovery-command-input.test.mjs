import assert from "node:assert/strict";
import {test} from "node:test";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import {compilerLayout, compilerOptions, createHost, decode, documentPool, prepare, sha256} from "./recovery-command-input.mjs";
import {canonicalInputText, complete, observeSelectedRow, optionSnapshot, value} from "./observe-recovery-selected-corpus.mjs";

const root = path.resolve(import.meta.dirname, "..");
const library = fs.realpathSync(path.join(root, "vendor/typescript-6.0.3/lib"));
const pool = new Map();
const doc = text => { const hash = sha256(text); pool.set(hash, text); return hash; };
const unit = (id, name, text) => ({id, name, content_sha256: text === null ? null : doc(text), document_symlinks: []});
function compiler(units, extra = {}) {
  return {route: "recorded-compiler", floor: "established", current_directory: "/.src", use_case_sensitive_file_names: true,
    units, vfs_write_order: units.map(u => u.id), program_root_units: units.filter(u => !u.name.endsWith(".json")).map(u => u.id),
    global_symlinks: [], vfs_symlinks: [], config_unit: null, settings: [],
    prepared: {roots: units.filter(u => !u.name.endsWith(".json")).map(u => ts.getNormalizedAbsolutePath(u.name, "/.src"))}, ...extra};
}
const row = (input, loader = "load_compiler_emit") => ({case_id: "witness", command_input: input, loader});

test("None root has no file while empty-content root exists", () => {
  const input = compiler([unit(0, "missing.ts", null), unit(1, "empty.ts", "")]);
  const layout = compilerLayout(input, pool);
  assert.deepEqual(layout.roots, ["/.src/missing.ts", "/.src/empty.ts"]);
  assert.equal(layout.files.has("/.src/missing.ts"), false);
  assert.equal(layout.files.get("/.src/empty.ts"), "");
});

test("write order controls the final bytes for repeated unit names", () => {
  const input = compiler([unit(0, "same.ts", "first"), unit(1, "same.ts", "last")], {vfs_write_order: [1, 0]});
  assert.equal(compilerLayout(input, pool).files.get("/.src/same.ts"), "first");
});

test("config defaults survive, while skipped sourceMap directive does not clear config", () => {
  const input = compiler([unit(0, "tsconfig.json", '{"compilerOptions":{"newLine":"lf","skipDefaultLibCheck":false,"sourceMap":true},"files":["main.ts"]}'), unit(1, "main.ts", "let x = 1;")],
    {config_unit: 0, settings: [["SourceMap", "false"]]});
  const actual = prepare(row(input), pool, library);
  assert.equal(actual.options.newLine, ts.NewLineKind.LineFeed);
  assert.equal(actual.options.skipDefaultLibCheck, false);
  assert.equal(actual.options.sourceMap, true);
  assert.equal(actual.options.noErrorTruncation, true);
  assert.equal(actual.decisions[0].decision, "floor-dropped");
  assert.ok(actual.options.configFile);
});

test("config host resolves raw unit names insensitively under /.src", () => {
  const input = compiler([unit(0, "tsconfig.json", '{"extends":"./BASE.json","files":["main.ts"]}'), unit(1, "/.src/base.json", '{"compilerOptions":{"strict":true}}'), unit(2, "main.ts", "")], {config_unit: 0});
  assert.equal(prepare(row(input), pool, library).options.strict, true);
});

test("config errors are retained for complete-command diagnostics", () => {
  const input = compiler([unit(0, "tsconfig.json", '{"compilerOptions":{"unknownOption":true},"files":["main.ts"]}'), unit(1, "main.ts", "")], {config_unit: 0});
  assert.ok(prepare(row(input), pool, library).errors.some(d => d.code === 5023));
});

test("directory aliases expand through other directory links", () => {
  const links = [{target: "/.src/linked", link: "/.src/nested"}, {target: "/.src/physical", link: "/.src/linked"}];
  const input = compiler([unit(0, "physical/index.ts", "export const x = 1;")], {global_symlinks: links, vfs_symlinks: links});
  const layout = compilerLayout(input, pool);
  assert.equal(layout.files.get("/.src/nested/index.ts"), "export const x = 1;");
  assert.equal(layout.aliases.get("/.src/nested/index.ts"), "/.src/physical/index.ts");
});

test("document link preserves occupied file bytes and records physical identity", () => {
  const units = [unit(0, "physical.ts", "physical"), unit(1, "occupied.ts", "occupied")];
  const link = {target: "/.src/physical.ts", link: "/.src/occupied.ts"};
  units[0].document_symlinks = [link];
  const layout = compilerLayout(compiler(units, {vfs_symlinks: [link]}), pool);
  assert.equal(layout.files.get("/.src/occupied.ts"), "occupied");
  assert.equal(layout.aliases.get("/.src/occupied.ts"), "/.src/physical.ts");
});

function project(descriptor, loader = "load_project_emit") {
  const input = {route: "recorded-project", floor: "established", current_directory: "/project",
    descriptor_utf8: {content_sha256: doc(JSON.stringify(descriptor))}, module_variant: "Amd",
    mount: {case_sensitive: true, read_only: true, files: [{path: "/project/main.ts", content_sha256: doc("export const x = 1;")}]},
    prepared: {roots: ["/project/main.ts"]}};
  return prepare(row(input, loader), pool, library);
}

test("project descriptor module overrides the matrix variant", () => {
  const actual = project({inputFiles: ["main.ts"], module: "commonjs", sourceMap: true});
  assert.equal(actual.options.module, ts.ModuleKind.CommonJS);
  assert.equal(actual.options.sourceMap, true);
  assert.equal(actual.options.skipDefaultLibCheck, false);
  assert.equal(actual.host.getDefaultLibFileName({target: ts.ScriptTarget.ESNext}), library + "/lib.es5.d.ts");
});

test("project resolve roots use the source mount independently of key order", () => {
  for (const reversed of [false, true]) {
    const entries = Object.entries({inputFiles: ["main.ts"], mapRoot: "tests/maps", sourceRoot: "tests/src",
      declarationDir: "declarations", emittedFiles: ["metadata.js"], resolveMapRoot: true, resolveSourceRoot: true});
    const actual = project(Object.fromEntries(reversed ? entries.reverse() : entries));
    assert.equal(actual.options.mapRoot, "/.src/tests/maps");
    assert.equal(actual.options.sourceRoot, "/.src/tests/src");
    assert.equal(actual.options.declarationDir, "declarations");
    assert.equal(actual.options.listEmittedFiles, undefined);
  }
  for (const flag of [false, undefined]) {
    const actual = project({inputFiles: ["main.ts"], mapRoot: "tests/maps", resolveMapRoot: flag});
    assert.equal(actual.options.mapRoot, "tests/maps");
  }
  assert.equal(project({inputFiles: ["main.ts"], resolveMapRoot: true}).options.mapRoot, undefined);
  assert.equal(project({inputFiles: ["main.ts"], resolveMapRoot: true, mapRoot: ""}).options.mapRoot, "");
});

test("project existing-options undefined roots override config roots", () => {
  const config = '{"compilerOptions":{"mapRoot":"config/maps","sourceRoot":"config/src"},"files":["main.ts"]}';
  for (const descriptor of [{}, {resolveMapRoot: true}, {resolveMapRoot: true, mapRoot: "tests/maps"}]) {
    const input = {route: "recorded-project", floor: "established", current_directory: "/project",
      descriptor_utf8: {content_sha256: doc(JSON.stringify(descriptor))}, module_variant: "Amd",
      mount: {case_sensitive: true, read_only: true, files: [
        {path: "/project/main.ts", content_sha256: doc("export const x=1;")},
        {path: "/project/tsconfig.json", content_sha256: doc(config)}]}, prepared: {roots: ["/project/main.ts"]}};
    const actual = prepare(row(input, "load_project_emit"), pool, library);
    assert.equal(actual.options.mapRoot, descriptor.mapRoot ? "/.src/tests/maps" : undefined);
    assert.equal(actual.options.sourceRoot, undefined);
    if (!Object.keys(descriptor).length) {
      const noEmit = prepare(row(input, "load_project_no_emit"), pool, library);
      assert.equal(noEmit.options.mapRoot, undefined);
      assert.equal(noEmit.options.sourceRoot, undefined);
    }
  }
});

test("NoEmit loaders retain their distinct option layers", () => {
  const input = compiler([unit(0, "main.ts", "")], {settings: [["noEmit", "false"]]});
  assert.equal(prepare(row(input, "load_compiler_no_emit"), pool, library).options.noEmit, true);
  const actual = project({inputFiles: ["main.ts"], sourceMap: false, declaration: false}, "load_project_no_emit");
  assert.equal(actual.options.noEmit, true);
  assert.equal(actual.options.newLine, undefined);
  assert.equal(actual.options.declaration, undefined);
  assert.equal(actual.options.sourceMap, undefined);
  assert.throws(() => project({inputFiles: ["main.ts"], sourceMap: true}, "load_project_no_emit"));
});

test("qualified config stays in VFS without changing options, host is sensitive", () => {
  const file = (path, text) => ({path, utf8_base64: Buffer.from(text).toString("base64"), utf8_bytes: Buffer.byteLength(text), utf8_sha256: sha256(text)});
  const input = {route: "qualified", floor: "established", use_case_sensitive_file_names: true,
    input: {current_directory: "/project", roots: ["/project/main.ts"], files: [file("/project/main.ts", "")],
      virtual_config: file("/project/tsconfig.json", '{"compilerOptions":{"strict":true,"sourceMap":true}}'), settings: []},
    prepared: {roots: ["/project/main.ts"]}};
  const actual = prepare(row(input, "load_qualified_compiler_emit_with_symlinks"), pool, library);
  assert.equal(actual.options.strict, undefined);
  assert.equal(actual.options.sourceMap, undefined);
  assert.equal(actual.host.fileExists("/project/MAIN.ts"), false);
  assert.equal(actual.host.fileExists("/project/tsconfig.json"), true);
  input.use_case_sensitive_file_names = false;
  assert.throws(() => prepare(row(input), pool, library));
});

test("case-insensitive compiler host preserves content lookup", () => {
  const input = compiler([unit(0, "Main.ts", "answer")], {use_case_sensitive_file_names: false});
  const actual = prepare(row(input), pool, library);
  assert.equal(actual.host.readFile("/.SRC/main.TS"), "answer");
  assert.equal(actual.host.useCaseSensitiveFileNames(), false);
});

test("pool decoder keeps BOM and rejects invalid UTF-8", () => {
  const text = "\uFEFFabc";
  assert.equal(decode(Buffer.from(text)), text);
  assert.deepEqual(documentPool({[sha256(text)]: Buffer.from(text).toString("base64")}).get(sha256(text)), Buffer.from(text));
  assert.throws(() => decode(Buffer.from([0xff])));
});

test("option parity distinguishes absent and false, preserves UTF-16 strings", () => {
  const absent = optionSnapshot({}), explicit = optionSnapshot({sourceMap: false, jsxFactory: "\uD800"});
  assert.equal(absent.sourceMap, null); assert.equal(explicit.sourceMap, false);
  assert.deepEqual(explicit.jsxFactory.utf16, [0xd800]);
  const projected = compilerOptions([["checkJs", "true"], ["allowJs", "false"], ["moduleSuffixes", ".ios,"]], {}, "/project");
  assert.equal(projected.options.allowJs, false);
  assert.deepEqual(projected.options.moduleSuffixes, [".ios", ""]);
});

test("complete observer retains raw surrogate units and metadata presence", () => {
  // A real program exercises the callback and diagnostic protocol. The direct
  // value witness covers a JS string that cannot survive UTF-8 round-tripping.
  assert.deepEqual(value("\uD800").utf16, [0xd800]);
  const layout = {files: new Map([["/project/main.ts", "export const x = 1;\n"]]), aliases: new Map(), roots: ["/project/main.ts"]};
  const options = {target: ts.ScriptTarget.ES2015, module: ts.ModuleKind.CommonJS, noLib: true};
  const host = createHost(layout, "/project", true, library);
  const actual = complete(ts.createProgram(layout.roots, options, host));
  assert.equal(actual.writes.length, 1);
  assert.deepEqual(actual.writes[0].data_keys, ["sourceMapUrlPos", "diagnostics"]);
  assert.equal(actual.writes[0].on_error_callback_present, true);
  assert.ok(actual.reported_diagnostics.length);
  assert.equal(actual.exit_code, 2);
});

test("pinned loader semantics and option schema remain unchanged", () => {
  const pins = JSON.parse(fs.readFileSync(path.join(root, "scripts/recovery-command-pins.json")));
  for (const [name, hash] of Object.entries(pins)) assert.equal(sha256(fs.readFileSync(path.join(root, name))), hash, name);
  const fields = JSON.parse(fs.readFileSync(path.join(root, "scripts/recovery-command-options.json")));
  const source = fs.readFileSync(path.join(root, "crates/types/src/options.rs"), "utf8").split("pub struct CompilerOptions {")[1].split("\n}")[0];
  assert.deepEqual([...source.matchAll(/^    pub (\w+): (.+),$/gm)].map(match => [match[1], match[2]]), fields.map(f => [f.field, f.type]));
});

test("new complete observer reproduces frozen EF7 commands on eight existing inputs", () => {
  const fixture = JSON.parse(fs.readFileSync(path.join(root, "crates/compiler/tests/fixtures/emitter-final-universe.json")));
  const eligible = fixture.cases.filter(c => c.input.route === "whole-program" && !c.input.config && !c.input.shared_mount && !c.input.vfs_symlinks.length);
  const cases = [...eligible.filter(c => c.typescript_observation.writes.length).slice(0, 5), ...eligible.filter(c => !c.typescript_observation.writes.length).slice(0, 3)];
  assert.equal(cases.length, 8);
  const text = v => String.fromCharCode(...v.utf16);
  const libText = v => text(v).split(library + "/").join("/lib/");
  const diag = d => ({...d, category: ts.DiagnosticCategory[d.category], file: d.file ? libText(d.file) : null,
    message: libText(d.message), related_information: d.related_information?.map(diag) ?? null});
  for (const c of cases) {
    const layout = {files: new Map(c.input.files.map(f => [f.path, f.text])), aliases: new Map(), roots: c.input.roots};
    const host = createHost(layout, c.input.current_directory, c.input.use_case_sensitive_file_names, library);
    const command = complete(ts.createProgram(layout.roots, c.effective_options, host));
    const writes = command.writes.map(w => {
      const name = text(w.path);
      return {index: w.index, path: name, kind: ts.isDeclarationFileName(name) ? "declaration" : name.endsWith(".map") ? "source-map" : "javascript",
        callback_utf8_base64: w.callback.utf8_base64, callback_utf8_bytes: Buffer.from(w.callback.utf8_base64, "base64").length,
        write_byte_order_mark: w.write_byte_order_mark, materialized_utf8_base64: w.materialized_utf8_base64,
        materialized_utf8_bytes: Buffer.from(w.materialized_utf8_base64, "base64").length,
        on_error_callback_present: w.on_error_callback_present, source_files: w.source_files?.map(text) ?? null,
        data_present: w.data_present, data_keys: w.data_keys, data_source_map_url_pos: w.data_source_map_url_pos,
        data_diagnostics: w.data_diagnostics?.map(diag) ?? null, data_build_info: null};
    });
    const emit = command.emit_result;
    const actual = {writes, reported_diagnostics: command.reported_diagnostics.map(diag), status_writes: command.status_writes.map(text), exit_code: command.exit_code,
      emit_result: {...emit, diagnostics: emit.diagnostics.map(diag), emitted_files: emit.emitted_files?.map(text) ?? null,
        source_maps: emit.source_maps?.map(m => ({input_source_file_names: m.input_source_file_names.map(text), source_map_json: text(m.source_map_json)})) ?? null}};
    const expected = Object.fromEntries(Object.keys(actual).map(key => [key, c.typescript_observation[key]]));
    assert.deepEqual(actual, expected, c.case_id);
  }
});


test("input hash uses scalar key order, including integer-like and supplementary keys", () => {
  const input = {z: [{y: 2, a: 1}], a: 0, "2": 2, "10": 10, "😀": 1, "\uE000": 2};
  assert.equal(canonicalInputText(input), '{"10":10,"2":2,"a":0,"z":[{"a":1,"y":2}],"\uE000":2,"😀":1}');
});

test("Established list directives trim and drop empty entries without dropping empty module suffix", () => {
  const actual = compilerOptions([["types","a, b,,"],["customConditions","a, b, "],["lib"," ES5, DOM, "],["moduleSuffixes",".ios,"]],{},"/.src").options;
  assert.deepEqual(actual.types,["a","b"]);
  assert.deepEqual(actual.customConditions,["a","b"]);
  assert.deepEqual(actual.lib,["lib.es5.d.ts","lib.dom.d.ts"]);
  assert.deepEqual(actual.moduleSuffixes,[".ios",""]);
});

test("raw pool entries are retained without being decoded as document text", () => {
  const raw = Buffer.from([0xff,0xfe,0x41,0]);
  const encoded = documentPool({[sha256(raw)]:raw.toString("base64")});
  assert.deepEqual(encoded.get(sha256(raw)),raw);
});

test("compiler VFS BOM is decoded once; project pool is already host-decoded", () => {
  const input = compiler([unit(0,"main.ts","\uFEFFlet x = 1;")]);
  const actual = prepare(row(input),pool,library);
  assert.equal(actual.host.getSourceFile("/.src/main.ts",ts.ScriptTarget.ES5).text,"let x = 1;");
  const layout = {files:new Map([["/project/main.ts","\uFEFFlet x = 1;"]]),aliases:new Map()};
  const projectHost = createHost(layout,"/project",true,library,true);
  assert.equal(projectHost.getSourceFile("/project/main.ts",ts.ScriptTarget.ES5).text,"\uFEFFlet x = 1;");
});

test("project config conversion retains located errors but no option SourceFile", () => {
  const input = {route:"recorded-project",floor:"established",current_directory:"/project",
    descriptor_utf8:{content_sha256:doc('{}')},module_variant:"Amd",
    mount:{case_sensitive:true,read_only:true,files:[
      {path:"/project/main.ts",content_sha256:doc("export const x = 1;")},
      {path:"/project/tsconfig.json",content_sha256:doc('{"compilerOptions":{"unknownOption":true},"files":["main.ts"]}')} ]},
    prepared:{roots:["/project/main.ts"]}};
  const actual = prepare(row(input,"load_project_emit"),pool,library);
  assert.equal(actual.options.configFile,undefined);
  assert.equal(actual.options.configFilePath,"/project/tsconfig.json");
  assert.equal(actual.externalConfigOptionDiagnostics,true);
  const diagnostic = actual.errors.find(d => d.code === 5023);
  assert.equal(diagnostic.file.fileName,"/project/tsconfig.json");
  assert.equal(typeof diagnostic.start,"number");
  const options = optionSnapshot(actual.options,actual);
  assert.equal(options.configFile,null);
  assert.equal(options.defaultLibraryFileName,"lib.es5.d.ts");
  assert.equal(options.configParsingDiagnostics[0].code,5023);
});

test("input reconstruction errors are per-row non-qualifying observations", () => {
  const input = compiler([unit(0,"main.ts","")]);
  input.prepared.roots = ["/.src/other.ts"];
  const first = observeSelectedRow(row(input),pool,library);
  assert.equal(first.disposition,"input-reconstruction-mismatch; emit-not-qualified");
  assert.deepEqual(first.complete_command_runs,[]);
  assert.match(first.input_reconstruction_error,/root order/);
  input.prepared.roots = ["/.src/main.ts"];
  input.prepared.source_files = [{path:"/.src/main.ts",sha256:sha256("")}];
  input.settings = [["noLib","true"]];
  const second = observeSelectedRow(row(input),pool,library);
  assert.equal(second.disposition,"observed-twice; pending-native-comparison");
  assert.equal(second.complete_command_runs.length,2);
});


test("config syntax diagnostics precede conversion diagnostics on both loader routes", () => {
  const text = '{"compilerOptions":{"unknownOption":true} "files":["main.ts"]}';
  const compilerInput = compiler([unit(0,"tsconfig.json",text),unit(1,"main.ts","")],{config_unit:0});
  const projectInput = {route:"recorded-project",floor:"established",current_directory:"/project",
    descriptor_utf8:{content_sha256:doc('{}')},module_variant:"Amd",
    mount:{case_sensitive:true,read_only:true,files:[
      {path:"/project/main.ts",content_sha256:doc("")},
      {path:"/project/tsconfig.json",content_sha256:doc(text)}]}, prepared:{roots:["/project/main.ts"]}};
  for (const [input,loader] of [[compilerInput,"load_compiler_emit"],[projectInput,"load_project_emit"]]) {
    const actual = prepare(row(input,loader),pool,library);
    assert.equal(actual.errors[0].code,1005);
    assert.equal(typeof actual.errors[0].start,"number");
    assert.ok(actual.errors.slice(1).some(d => d.code === 5023));
  }
});

test("canonical input hashing rejects fractional and unsafe numbers", () => {
  for (const value of [1.5,9007199254740992,-9007199254740992])
    assert.throws(() => canonicalInputText({value}),/safe integers/);
});
