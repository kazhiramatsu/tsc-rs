// H2.8a: complete ordinary commands through the pinned directory/retry worker.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/compiler/tests/fixtures/output-filesystem.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const inputs = [];
for (const outDir of ["out/deep", "./out/../gen/deep", "/project/new/deep", "C:/emit/deep", "//server/share/emit", ""]) {
  for (const fault of ["none", "first-write", "create-directory", "all-writes"]) {
    inputs.push({ case_id: `filesystem/${outDir}/${fault}`, current_directory: "/project", fault,
      initial_directories: ["/", "/project", "/project/src"],
      files: [{ path: "/project/src/a.ts", text: "// before\nexport const a: number = 1;\n" },
        { path: "/project/src/b.ts", text: "export const b: string = 'b';\n" }],
      options: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.CommonJS, strict: true,
        outDir, emitBOM: true, newLine: ts.NewLineKind.CarriageReturnLineFeed,
        listEmittedFiles: true, skipDefaultLibCheck: true, noErrorTruncation: true } });
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
  return { index, path: name, kind: "javascript", callback_utf8_base64: bytes.toString("base64"), callback_utf8_bytes: bytes.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"), materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined, source_files: sources?.map(source => source.fileName) ?? null,
    data_present: data !== undefined, data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
    data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null };
}
function observe(input) {
  const files = new Map(input.files.map(file => [file.path, file.text]));
  const libraryRoot = path.join(root, "vendor/typescript-6.0.3/lib");
  const library = name => /^\/lib\/lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(name)
    && fs.existsSync(path.join(libraryRoot, path.basename(name)));
  const read = name => files.get(ts.normalizePath(name))
    ?? (library(name) ? fs.readFileSync(path.join(libraryRoot, path.basename(name)), "utf8") : undefined);
  const overlay = createHermeticDirectoryOverlay(files.keys(), { currentDirectory: input.current_directory,
    useCaseSensitiveFileNames: true, fallbackHost: { directoryExists: name => name === "/lib", getDirectories: () => [] } });
  const host = { ...ts.createCompilerHost(input.options, true), ...overlay,
    getCurrentDirectory: () => input.current_directory, getDefaultLibFileName: options => "/lib/" + ts.getDefaultLibFileName(options),
    getDefaultLibLocation: () => "/lib", useCaseSensitiveFileNames: () => true, getCanonicalFileName: name => name,
    readFile: read, fileExists: name => files.has(ts.normalizePath(name)) || library(name),
    getSourceFile: (name, options) => { const text = read(name); return text === undefined ? undefined : ts.createSourceFile(name, text, options, true); },
    writeFile: () => assert.fail("command callback owns filesystem transport") };
  const program = ts.createProgram({ rootNames: input.files.map(file => file.path), options: input.options, host });
  const absolute = name => ts.getNormalizedAbsolutePath(name, input.current_directory);
  const directories = new Set(input.initial_directories), materialized = new Map(), attempts = new Map();
  const operations = [], writes = [], reported = [], status = [];
  function rawWrite(name, text, bom) {
    const attempt = (attempts.get(name) ?? 0) + 1; attempts.set(name, attempt);
    const bytes = Buffer.from((bom ? "\ufeff" : "") + text);
    const error = input.fault === "all-writes" ? "H2.8 controlled write failure"
      : input.fault === "first-write" && attempt === 1 ? "H2.8 discarded first write failure"
      : !directories.has(ts.getDirectoryPath(absolute(name))) ? "H2.8 parent directory is missing" : null;
    operations.push({ operation: "write-file", path: name, utf8_base64: bytes.toString("base64"), error });
    if (error !== null) throw new Error(error);
    materialized.set(absolute(name), bytes);
  }
  function directoryExists(name) {
    const exists = directories.has(absolute(name));
    operations.push({ operation: "directory-exists", path: name, exists });
    return exists;
  }
  function createDirectory(name) {
    const error = input.fault === "create-directory" ? "H2.8 controlled create failure" : null;
    operations.push({ operation: "create-directory", path: name, error });
    if (error !== null) throw new Error(error);
    directories.add(absolute(name));
  }
  const sink = (...args) => {
    writes.push(write(args, writes.length));
    try { ts.writeFileEnsuringDirectories(args[0], args[1], args[2], rawWrite, createDirectory, directoryExists); }
    catch (error) { assert.ok(args[3]); args[3](error.message); }
  };
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => reported.push(diagnostic(d)), s => status.push(s), undefined, sink);
  assert.ok(result);
  assert.equal(result.sourceMaps, undefined);
  return { writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic), emitted_files: result.emittedFiles ?? null, source_maps: null },
    status_writes: status, exit_code: exit, filesystem: { operations,
      materialized_files: [...materialized].map(([name, bytes]) => ({ path: name, utf8_base64: bytes.toString("base64") })),
      directories: [...directories] } };
}
const cases = inputs.map(input => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  return { ...input, typescript_observation: first };
});
const artifact = { version: 1, typescript: ts.version, source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), repetitions: 2, cases };
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered);
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(`output filesystem: ${cases.length} cases, two identical complete observations each`);
