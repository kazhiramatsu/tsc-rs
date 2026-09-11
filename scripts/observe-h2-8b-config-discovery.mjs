// Fresh config graphs through the pinned TypeScript source-file config API.
// node scripts/observe-h2-8b-config-discovery.mjs --write|--check
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";

const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const inputPath = "crates/program/tests/fixtures/h2-8b-config-discovery-inputs.json";
const outputPath = "crates/program/tests/fixtures/h2-8b-config-discovery.json";
const inputs = JSON.parse(fs.readFileSync(path.join(root, inputPath), "utf8"));
assert.equal(inputs.cases.length, 24);
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") assert.ok(!fs.existsSync(path.join(root, outputPath)), "retain frozen observations");

function diagnostic(d) {
  return {
    code: d.code,
    category: ts.DiagnosticCategory[d.category].toLowerCase(),
    file: d.file?.fileName ?? null,
    start: d.start ?? null,
    length: d.length ?? null,
    message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: (d.relatedInformation ?? []).map(diagnostic),
  };
}

function optionState(options, name) {
  if (!Object.hasOwn(options, name)) return {name, state: "absent"};
  const value = options[name];
  if (value === undefined) return {name, state: "undefined"};
  if (Array.isArray(value)) return {
    name, state: "list",
    elements: value.map(element => element === undefined
      ? {state: "undefined"} : {state: "value", value: element}),
  };
  return {name, state: "value", value};
}

function observe(input) {
  const base = ts.getDirectoryPath(input.config_path);
  const files = new Map(input.files.map(file => [file.path, file.text]));
  assert.equal(files.size, input.files.length);
  files.set(input.config_path, input.config);
  const directories = new Map();
  function directory(name) {
    if (!directories.has(name)) directories.set(name, {files: new Set(), directories: new Set()});
    return directories.get(name);
  }
  for (const name of files.keys()) {
    let current = ts.getDirectoryPath(name);
    directory(current).files.add(ts.getBaseFileName(name));
    while (true) {
      const parent = ts.getDirectoryPath(current);
      if (parent === current) break;
      directory(parent).directories.add(ts.getBaseFileName(current));
      current = parent;
    }
  }
  const normalize = name => ts.getNormalizedAbsolutePath(name, base);
  const host = {
    useCaseSensitiveFileNames: true,
    fileExists: name => files.has(normalize(name)),
    readFile: name => files.get(normalize(name)),
    readDirectory: (directoryName, extensions, excludes, includes, depth) =>
      ts.matchFiles(directoryName, extensions, excludes, includes, true, base, depth, name => {
        const entry = directories.get(normalize(name));
        return {
          files: [...(entry?.files ?? [])].sort(),
          directories: [...(entry?.directories ?? [])].sort(),
        };
      }, normalize),
  };
  const source = ts.parseJsonText(input.config_path, input.config);
  const parsed = ts.parseJsonSourceFileConfigFileContent(source, host, base, undefined, input.config_path);
  const extended = [...(parsed.options.configFile?.extendedSourceFiles ?? [])];
  return {
    raw: JSON.parse(JSON.stringify(parsed.raw)),
    file_names: parsed.fileNames,
    wildcard_directories: Object.entries(parsed.wildcardDirectories).map(([name, flags]) => ({path: name, recursive: flags === 1})),
    extended_source_files: extended,
    extended_sources: extended.filter(name => files.has(name)).map(name => ({file_name: name, text: files.get(name)})),
    option_probes: inputs.option_probe_keys.map(name => optionState(parsed.options, name)),
    root_parse_diagnostics: source.parseDiagnostics.map(diagnostic),
    parsed_errors: parsed.errors.map(diagnostic),
    config_diagnostics: ts.getConfigFileParsingDiagnostics(parsed).map(diagnostic),
  };
}

const cases = inputs.cases.map(input => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, `${input.case_id}: independent observation mismatch`);
  return {case_id: input.case_id, typescript_observation: first};
});
const sourcePath = "vendor/typescript-6.0.3/lib/_tsc.js";
const sourceBytes = fs.readFileSync(path.join(root, sourcePath));
const sourceLines = sourceBytes.toString("utf8").split("\n");
const anchors = [
  ["getFileMatcherPatterns", 18509, 18524],
  ["matchFiles", 18525, 18572],
  ["getBasePaths", 18573, 18589],
  ["getIncludeBasePath", 18590, 18596],
].map(([symbol, start_line, end_line]) => ({symbol, start_line, end_line,
  sha256: sha(sourceLines.slice(start_line - 1, end_line).join("\n") + "\n")}));
const artifact = {
  version: 1, slice: inputs.slice, typescript: ts.version,
  source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
  source: {path: sourcePath, sha256: sha(sourceBytes), anchors},
  compiler_sha256: sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha(fs.readFileSync(import.meta.filename)),
  inputs: {path: inputPath, sha256: sha(fs.readFileSync(path.join(root, inputPath)))},
  repetitions: 2, program_executions: 0, config_parse_executions: cases.length * 2, cases,
};
const destination = path.join(root, outputPath);
if (process.argv[2] === "--write") fs.writeFileSync(destination, JSON.stringify(artifact, null, 2) + "\n");
else assert.deepEqual(JSON.parse(fs.readFileSync(destination, "utf8")), artifact);
console.log(JSON.stringify({cases: cases.length, repetitions: 2, program_executions: 0,
  config_parse_executions: cases.length * 2, sha256: sha(fs.readFileSync(destination)),
  diagnostic_counts: cases.map(c => [c.case_id, c.typescript_observation.config_diagnostics.length])}));
