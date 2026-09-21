import assert from 'node:assert/strict';import fs from 'node:fs';import path from 'node:path';
import ts from '/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/vendor/typescript-6.0.3/lib/typescript.js';
import {createHermeticDirectoryOverlay} from '/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/crates/oracle/vfs-directory-overlay.mjs';
const root="/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep";const inputs=[{"case_id": "module-boundary/import-export", "roots": ["/project/main.ts"], "files": [{"path": "/project/main.ts", "text": "export /* a */ import /* b */ './dep';\n"}], "options": {"strict": true, "allowJs": true, "checkJs": true, "sourceMap": true, "module": 99, "skipDefaultLibCheck": true, "noErrorTruncation": true, "newLine": 0, "outDir": "/project/out", "target": 99, "declaration": true, "declarationMap": true, "emitDeclarationOnly": false, "removeComments": false}}, {"case_id": "module-boundary/import-declare", "roots": ["/project/main.ts"], "files": [{"path": "/project/main.ts", "text": "declare /* a */ import /* b */ './dep';\n"}], "options": {"strict": true, "allowJs": true, "checkJs": true, "sourceMap": true, "module": 99, "skipDefaultLibCheck": true, "noErrorTruncation": true, "newLine": 0, "outDir": "/project/out", "target": 99, "declaration": true, "declarationMap": true, "emitDeclarationOnly": false, "removeComments": false}}, {"case_id": "module-boundary/import-public", "roots": ["/project/main.ts"], "files": [{"path": "/project/main.ts", "text": "public /* a */ import /* b */ './dep';\n"}], "options": {"strict": true, "allowJs": true, "checkJs": true, "sourceMap": true, "module": 99, "skipDefaultLibCheck": true, "noErrorTruncation": true, "newLine": 0, "outDir": "/project/out", "target": 99, "declaration": true, "declarationMap": true, "emitDeclarationOnly": false, "removeComments": false}}, {"case_id": "module-boundary/import-async", "roots": ["/project/main.ts"], "files": [{"path": "/project/main.ts", "text": "async /* a */ import /* b */ './dep';\n"}], "options": {"strict": true, "allowJs": true, "checkJs": true, "sourceMap": true, "module": 99, "skipDefaultLibCheck": true, "noErrorTruncation": true, "newLine": 0, "outDir": "/project/out", "target": 99, "declaration": true, "declarationMap": true, "emitDeclarationOnly": false, "removeComments": false}}, {"case_id": "module-boundary/import-declare-export", "roots": ["/project/main.ts"], "files": [{"path": "/project/main.ts", "text": "declare export /* a */ import /* b */ './dep';\n"}], "options": {"strict": true, "allowJs": true, "checkJs": true, "sourceMap": true, "module": 99, "skipDefaultLibCheck": true, "noErrorTruncation": true, "newLine": 0, "outDir": "/project/out", "target": 99, "declaration": true, "declarationMap": true, "emitDeclarationOnly": false, "removeComments": false}}, {"case_id": "module-boundary/export-star-export", "roots": ["/project/main.ts"], "files": [{"path": "/project/main.ts", "text": "export /* a */ export /* b */ * /* c */ from /* d */ './dep';\n"}], "options": {"strict": true, "allowJs": true, "checkJs": true, "sourceMap": true, "module": 99, "skipDefaultLibCheck": true, "noErrorTruncation": true, "newLine": 0, "outDir": "/project/out", "target": 99, "declaration": true, "declarationMap": true, "emitDeclarationOnly": false, "removeComments": false}}, {"case_id": "module-boundary/export-star-declare", "roots": ["/project/main.ts"], "files": [{"path": "/project/main.ts", "text": "declare /* a */ export /* b */ * /* c */ from /* d */ './dep';\n"}], "options": {"strict": true, "allowJs": true, "checkJs": true, "sourceMap": true, "module": 99, "skipDefaultLibCheck": true, "noErrorTruncation": true, "newLine": 0, "outDir": "/project/out", "target": 99, "declaration": true, "declarationMap": true, "emitDeclarationOnly": false, "removeComments": false}}, {"case_id": "module-boundary/export-star-public", "roots": ["/project/main.ts"], "files": [{"path": "/project/main.ts", "text": "public /* a */ export /* b */ * /* c */ from /* d */ './dep';\n"}], "options": {"strict": true, "allowJs": true, "checkJs": true, "sourceMap": true, "module": 99, "skipDefaultLibCheck": true, "noErrorTruncation": true, "newLine": 0, "outDir": "/project/out", "target": 99, "declaration": true, "declarationMap": true, "emitDeclarationOnly": false, "removeComments": false}}, {"case_id": "module-boundary/export-star-async", "roots": ["/project/main.ts"], "files": [{"path": "/project/main.ts", "text": "async /* a */ export /* b */ * /* c */ from /* d */ './dep';\n"}], "options": {"strict": true, "allowJs": true, "checkJs": true, "sourceMap": true, "module": 99, "skipDefaultLibCheck": true, "noErrorTruncation": true, "newLine": 0, "outDir": "/project/out", "target": 99, "declaration": true, "declarationMap": true, "emitDeclarationOnly": false, "removeComments": false}}, {"case_id": "module-boundary/assignment-export", "roots": ["/project/main.ts"], "files": [{"path": "/project/main.ts", "text": "export /* a */ export /* b */ default /* c */ 1;\n"}], "options": {"strict": true, "allowJs": true, "checkJs": true, "sourceMap": true, "module": 99, "skipDefaultLibCheck": true, "noErrorTruncation": true, "newLine": 0, "outDir": "/project/out", "target": 99, "declaration": true, "declarationMap": true, "emitDeclarationOnly": false, "removeComments": false}}, {"case_id": "module-boundary/assignment-async", "roots": ["/project/main.ts"], "files": [{"path": "/project/main.ts", "text": "async /* a */ export /* b */ default /* c */ 1;\n"}], "options": {"strict": true, "allowJs": true, "checkJs": true, "sourceMap": true, "module": 99, "skipDefaultLibCheck": true, "noErrorTruncation": true, "newLine": 0, "outDir": "/project/out", "target": 99, "declaration": true, "declarationMap": true, "emitDeclarationOnly": false, "removeComments": false}}, {"case_id": "module-boundary/assignment-declare-export", "roots": ["/project/main.ts"], "files": [{"path": "/project/main.ts", "text": "declare export /* a */ export /* b */ default /* c */ 1;\n"}], "options": {"strict": true, "allowJs": true, "checkJs": true, "sourceMap": true, "module": 99, "skipDefaultLibCheck": true, "noErrorTruncation": true, "newLine": 0, "outDir": "/project/out", "target": 99, "declaration": true, "declarationMap": true, "emitDeclarationOnly": false, "removeComments": false}}];
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
const cases=inputs.map(input=>{const first=observe(input);assert.deepEqual(observe(input),first,input.case_id);console.log(input.case_id);return {...input,typescript_observation:first};});fs.writeFileSync("/tmp/emitter-module-boundary-observe-r408/oracle.json",JSON.stringify({typescript:ts.version,repetitions:2,cases},null,2)+'\n');