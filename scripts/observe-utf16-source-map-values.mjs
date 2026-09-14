// Pinned source-map API values and encodeURI observations, not full commands.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";

const root = path.resolve(import.meta.dirname, "..");
const hash = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = hash(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const units = value => Array.from({ length: value.length }, (_, index) => value.charCodeAt(index));
const inputs = [
  ["lone-names", "/project", "/project/src", true,
    ["/project/src/\ud800.ts", "/project/src/\ud801.ts", "/project/src/\ufffd.ts", "/project/src/\ud800.ts"]],
  ["paired-name-alias", "/project", "/project/src", true,
    ["/project/src/\ud800\udc00.ts", "/project/src/\u{10000}.ts", "/project/src/\ud800.ts"]],
  ["lone-cwd-and-directory", "/project/\udc00", "/project/\udc00/src/\ud800", true,
    ["/project/\udc00/src/\ud800/a.ts", "/project/\udc00/src/\ud801/a.ts"]],
  ["unc-root-identities", "/project", "//\ud800/share/src", true,
    ["//\ud800/share/src/a.ts", "//\ud801/share/src/a.ts", "//\ufffd/share/src/a.ts"]],
  ["unicode-unc-root-case", "/project", "//Σerver/share/src", true,
    ["//σERVER/share/src/a.ts", "//Σerver/share/src/\ud800.ts"]],
  ["case-folding-context", "/project", "/PROJECT/ΟΣ/\ud800", false,
    ["/project/ος/\ud800/a.ts", "/PROJECT/ΟΣ/\ud801/a.ts", "/project/ος/\ud800/A.ts"]],
  ["url-authority-identities", "/project", "https://\ud800.example/src", true,
    ["https://\ud800.example/src/a.ts", "https://\ud801.example/src/a.ts"]],
];
function observe([case_id, cwd, directory, case_sensitive, sources]) {
  const file = "output-\ud800.js";
  const sourceRoot = "root-\udc00/";
  const host = {
    getCurrentDirectory: () => cwd,
    getCanonicalFileName: name => case_sensitive ? name : ts.toFileNameLowerCase(name),
  };
  const generator = ts.createSourceMapGenerator(host, file, sourceRoot, directory, {});
  const contents = sources.map((_, index) => `text ${String.fromCharCode(0xd800 + index)}\n`);
  const source_indices = sources.map((source, index) => {
    const sourceIndex = generator.addSource(source);
    generator.setSourceContent(sourceIndex, contents[index]);
    generator.addMapping(index, 0, sourceIndex, index, 2);
    return sourceIndex;
  });
  return {
    case_id, cwd: units(cwd), directory: units(directory), case_sensitive,
    file: units(file), source_root: units(sourceRoot), sources: sources.map(units),
    contents: contents.map(units), source_indices,
    raw_sources: generator.getSources().map(units), json: generator.toString(),
  };
}
const maps = inputs.map(input => {
  const first = observe(input);
  assert.deepEqual(observe(input), first);
  return first;
});
const uriInputs = ["plain.js.map", "a b#?.js.map", "\ud800.js.map", "\ud801.js.map", "\udc00.js.map", "\ufffd.js.map", "\ud800\udc00.js.map"];
function observeUri(value) {
  try { return { input: units(value), url: encodeURI(value) }; }
  catch (error) { return { input: units(value), error: { name: error.name, message: error.message } }; }
}
const uris = uriInputs.map(value => {
  const first = observeUri(value);
  assert.deepEqual(observeUri(value), first);
  return first;
});
// Confirm that the compiler reaches this builtin boundary for an actual map URL.
function observeEmitFailure() {
  const fileName = "/project/\ud800.ts";
  const options = { target: ts.ScriptTarget.ES2015, sourceMap: true, noLib: true };
  const writes = [];
  const host = {
    getSourceFile: name => name === fileName ? ts.createSourceFile(name, "const value = 1;", options.target, true) : undefined,
    getDefaultLibFileName: () => "lib.d.ts", getCurrentDirectory: () => "/project",
    getDirectories: () => [], fileExists: name => name === fileName,
    readFile: name => name === fileName ? "const value = 1;" : undefined,
    getCanonicalFileName: name => name, useCaseSensitiveFileNames: () => true,
    getNewLine: () => "\n", writeFile: (name, text) => writes.push({ name: units(name), text }),
  };
  try { ts.createProgram([fileName], options, host).emit(); assert.fail("expected URIError"); }
  catch (error) {
    assert.equal(error.name, "URIError");
    return { error: { name: error.name, message: error.message }, writes };
  }
}
const emit_failure = observeEmitFailure();
assert.deepEqual(observeEmitFailure(), emit_failure);
const artifact = {
  version: 1, scope: "source-map API and URI builtin values; one compiler emit exception witness; no full-command comparison",
  typescript: ts.version, compiler_sha256: compilerSha,
  observer_sha256: hash(fs.readFileSync(import.meta.filename)), repetitions: 2, maps, uris, emit_failure,
};
const output = path.join(root, "crates/emitter/tests/fixtures/utf16-source-map-values.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", { flag: "wx" });
else assert.deepEqual(JSON.parse(fs.readFileSync(output, "utf8")), artifact);
console.log(JSON.stringify({ output, sha256: hash(fs.readFileSync(output)), maps: maps.length, uris: uris.length, repetitions: 2 }));
