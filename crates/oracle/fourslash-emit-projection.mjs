import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { TextDecoder } from "node:util";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH =
  "crates/oracle/fourslash-emit-projection.mjs";
const TARGET_RELATIVE_PATH =
  "vendor/typescript-6.0.3/fourslash-emit-projection.v1.json";
const TARGET_PATH = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
const VENDORED_RELATIVE_PATH = "ts-tests/tests/cases/fourslash";
const VENDORED_PATH = path.join(WORKSPACE, VENDORED_RELATIVE_PATH);

const TYPESCRIPT_VERSION = "6.0.3";
const SOURCE_REPOSITORY = "https://github.com/microsoft/TypeScript.git";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const SOURCE_TREE = {
  path: "tests/cases/fourslash",
  git_tree_sha1: "775c30f57c0638a180e7ac2e38b2581976620ca5",
  blob_inventory_sha256:
    "5f6ada2902ef3f7ce41e0652111d5d0db144a107b19d4d58f2369c8bd0cf3461",
  files: 6568,
  bytes: 14198525,
  unique_blobs: 6547,
  executable_paths: [],
};

const OPERATION_METHODS = [
  "baselineGetEmitOutput",
  "getEmitOutput",
  "verifyGetEmitOutputContentsForCurrentFile",
  "verifyGetEmitOutputForCurrentFile",
];
const OPERATION_CALL_PATTERN =
  String.raw`^[ \t]*verify\.(baselineGetEmitOutput|getEmitOutput|verifyGetEmitOutputForCurrentFile|verifyGetEmitOutputContentsForCurrentFile)[ \t]*\(`;
const OPERATION_CALL_FLAGS = "gm";
const EMIT_THIS_FILE_PATTERN =
  String.raw`^[ \t]*//[ \t]*@emitThisFile[ \t]*:[ \t]*(true|false)[ \t]*\r?$`;
const EMIT_THIS_FILE_FLAGS = "gm";
const BROAD_MENTION_PATTERN =
  /baselineGetEmitOutput|getEmitOutput|verifyGetEmitOutputForCurrentFile|verifyGetEmitOutputContentsForCurrentFile/;

const FALSE_POSITIVE_CONTROLS = [
  {
    path: "fourslash.ts",
    git_blob_sha1: "c53af3e78a24efd8352825f050ecfbbb7f00fbd0",
    bytes: 44800,
    mention_lines: [330, 331, 360, 363],
    disposition: "interface-declarations-only",
  },
  {
    path: "incrementalParsing1.ts",
    git_blob_sha1: "e09e96c9e0d2d5d9a83d940d22c019d22c58cb7b",
    bytes: 540,
    mention_lines: [16],
    disposition: "comment-only",
  },
];

const EXPECTED_SUMMARY = {
  fixture_files: 38,
  fixture_bytes: 31051,
  unique_blobs: 38,
  operation_calls: 38,
  operation_counts: {
    baselineGetEmitOutput: 31,
    getEmitOutput: 5,
    verifyGetEmitOutputContentsForCurrentFile: 1,
    verifyGetEmitOutputForCurrentFile: 1,
  },
  fixtures_with_emit_this_file: 37,
  emit_this_file_directives: 49,
  emit_this_file_true: 47,
  emit_this_file_false: 2,
};

const UTF8 = new TextDecoder("utf-8", { fatal: true });

function fail(message) {
  throw new Error(message);
}

function requireCondition(condition, message) {
  if (!condition) fail(message);
}

function sha256(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

function sha1(bytes) {
  return crypto.createHash("sha1").update(bytes).digest();
}

function gitBlobSha1(bytes) {
  return crypto
    .createHash("sha1")
    .update(`blob ${bytes.length}\0`)
    .update(bytes)
    .digest("hex");
}

function compareNames(left, right) {
  return Buffer.compare(Buffer.from(left), Buffer.from(right));
}

function lineAt(text, offset) {
  let line = 1;
  for (let index = 0; index < offset; index += 1) {
    if (text.charCodeAt(index) === 0x0a) line += 1;
  }
  return line;
}

function normalizedRelativePath(root, candidate) {
  const relative = path.relative(root, candidate).split(path.sep).join("/");
  requireCondition(
    relative.length > 0 &&
      relative === path.posix.normalize(relative) &&
      !relative.startsWith("../") &&
      !relative.startsWith("/"),
    `unsafe FourSlash path ${JSON.stringify(relative)}`,
  );
  return relative;
}

function walkFiles(root, directory = root, output = []) {
  const entries = fs
    .readdirSync(directory, { withFileTypes: true })
    .sort((left, right) => compareNames(left.name, right.name));
  requireCondition(entries.length > 0, `empty directory ${directory}`);
  for (const entry of entries) {
    const candidate = path.join(directory, entry.name);
    requireCondition(!entry.isSymbolicLink(), `symlink is not allowed: ${candidate}`);
    if (entry.isDirectory()) {
      walkFiles(root, candidate, output);
      continue;
    }
    requireCondition(entry.isFile(), `unsupported filesystem entry: ${candidate}`);
    const mode = fs.statSync(candidate).mode;
    requireCondition(
      (mode & 0o111) === 0,
      `FourSlash source must not be executable: ${candidate}`,
    );
    const raw = fs.readFileSync(candidate);
    output.push({
      path: normalizedRelativePath(root, candidate),
      raw,
      bytes: raw.length,
      git_blob_sha1: gitBlobSha1(raw),
    });
  }
  return output;
}

function directoryNode() {
  return { directories: new Map(), files: new Map() };
}

function buildTree(files) {
  const root = directoryNode();
  for (const file of files) {
    const components = file.path.split("/");
    const name = components.pop();
    let current = root;
    for (const component of components) {
      requireCondition(
        !current.files.has(component),
        `file/directory collision at ${file.path}`,
      );
      if (!current.directories.has(component)) {
        current.directories.set(component, directoryNode());
      }
      current = current.directories.get(component);
    }
    requireCondition(
      !current.directories.has(name) && !current.files.has(name),
      `duplicate FourSlash path ${file.path}`,
    );
    current.files.set(name, file);
  }
  return root;
}

function treeEntries(node) {
  const entries = [
    ...[...node.files].map(([name, file]) => ({ name, kind: "file", file })),
    ...[...node.directories].map(([name, child]) => ({
      name,
      kind: "directory",
      child,
    })),
  ];
  entries.sort((left, right) =>
    compareNames(
      `${left.name}${left.kind === "directory" ? "/" : ""}`,
      `${right.name}${right.kind === "directory" ? "/" : ""}`,
    ),
  );
  return entries;
}

function hashTree(node) {
  const chunks = [];
  for (const entry of treeEntries(node)) {
    const mode = entry.kind === "directory" ? "40000" : "100644";
    const object =
      entry.kind === "directory"
        ? hashTree(entry.child)
        : Buffer.from(entry.file.git_blob_sha1, "hex");
    chunks.push(Buffer.from(`${mode} ${entry.name}\0`), object);
  }
  const body = Buffer.concat(chunks);
  return sha1(Buffer.concat([Buffer.from(`tree ${body.length}\0`), body]));
}

function flattenTree(node, prefix = "", output = []) {
  for (const entry of treeEntries(node)) {
    if (entry.kind === "directory") {
      flattenTree(entry.child, `${prefix}${entry.name}/`, output);
    } else {
      output.push({ path: `${prefix}${entry.name}`, file: entry.file });
    }
  }
  return output;
}

function treeIdentity(files) {
  const tree = buildTree(files);
  const inventory = Buffer.concat(
    flattenTree(tree).map(({ path: relative, file }) =>
      Buffer.from(
        `100644 blob ${file.git_blob_sha1} ${file.bytes}\t${relative}\0`,
      ),
    ),
  );
  return {
    git_tree_sha1: hashTree(tree).toString("hex"),
    blob_inventory_sha256: sha256(inventory),
    files: files.length,
    bytes: files.reduce((total, file) => total + file.bytes, 0),
    unique_blobs: new Set(files.map((file) => file.git_blob_sha1)).size,
    executable_paths: [],
  };
}

function extractFixture(file) {
  let text;
  try {
    text = UTF8.decode(file.raw);
  } catch (error) {
    fail(`FourSlash fixture ${file.path} is not UTF-8: ${error.message}`);
  }
  const operations = [...text.matchAll(new RegExp(OPERATION_CALL_PATTERN, OPERATION_CALL_FLAGS))];
  if (operations.length === 0) return undefined;
  requireCondition(
    operations.length === 1,
    `${file.path} contains ${operations.length} selected emit operations; expected one`,
  );
  const operation = operations[0];
  const directives = [
    ...text.matchAll(new RegExp(EMIT_THIS_FILE_PATTERN, EMIT_THIS_FILE_FLAGS)),
  ].map((match) => ({
    line: lineAt(text, match.index),
    value: match[1] === "true",
  }));
  return {
    path: file.path,
    git_blob_sha1: file.git_blob_sha1,
    bytes: file.bytes,
    operation: {
      method: operation[1],
      line: lineAt(text, operation.index),
    },
    emit_this_file_directives: directives,
  };
}

function mentionLines(file) {
  const text = UTF8.decode(file.raw);
  const lines = [];
  for (const match of text.matchAll(new RegExp(BROAD_MENTION_PATTERN.source, "g"))) {
    lines.push(lineAt(text, match.index));
  }
  return [...new Set(lines)];
}

function jsonEqual(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function requireJsonEqual(actual, expected, label) {
  requireCondition(
    jsonEqual(actual, expected),
    `${label} mismatch:\nactual=${JSON.stringify(actual)}\nexpected=${JSON.stringify(expected)}`,
  );
}

function validateFullSource(files) {
  requireJsonEqual(
    { path: SOURCE_TREE.path, ...treeIdentity(files) },
    SOURCE_TREE,
    "full FourSlash tree identity",
  );
  const selected = files.map(extractFixture).filter((value) => value !== undefined);
  const mentionedButNotSelected = files.filter(
    (file) => BROAD_MENTION_PATTERN.test(UTF8.decode(file.raw)) && !extractFixture(file),
  );
  const controls = mentionedButNotSelected.map((file) => ({
    path: file.path,
    git_blob_sha1: file.git_blob_sha1,
    bytes: file.bytes,
    mention_lines: mentionLines(file),
    disposition:
      file.path === "fourslash.ts"
        ? "interface-declarations-only"
        : "comment-only",
  }));
  requireJsonEqual(
    controls,
    FALSE_POSITIVE_CONTROLS,
    "FourSlash false-positive controls",
  );
  return selected;
}

function validateVendoredProjection(selected) {
  const vendoredFiles = walkFiles(VENDORED_PATH);
  const vendoredSelected = vendoredFiles.map(extractFixture);
  requireCondition(
    vendoredSelected.every((value) => value !== undefined),
    "the vendored projection contains a file without a selected emit operation",
  );
  requireJsonEqual(
    vendoredSelected,
    selected,
    "vendored FourSlash projection",
  );
  return vendoredFiles;
}

function projectionSummary(fixtures) {
  const operationCounts = Object.fromEntries(
    OPERATION_METHODS.map((method) => [
      method,
      fixtures.filter((fixture) => fixture.operation.method === method).length,
    ]),
  );
  const directives = fixtures.flatMap(
    (fixture) => fixture.emit_this_file_directives,
  );
  return {
    fixture_files: fixtures.length,
    fixture_bytes: fixtures.reduce((total, fixture) => total + fixture.bytes, 0),
    unique_blobs: new Set(fixtures.map((fixture) => fixture.git_blob_sha1)).size,
    operation_calls: fixtures.length,
    operation_counts: operationCounts,
    fixtures_with_emit_this_file: fixtures.filter(
      (fixture) => fixture.emit_this_file_directives.length > 0,
    ).length,
    emit_this_file_directives: directives.length,
    emit_this_file_true: directives.filter((directive) => directive.value).length,
    emit_this_file_false: directives.filter((directive) => !directive.value).length,
  };
}

function buildArtifact(sourceRoot) {
  let fixtures;
  if (sourceRoot === undefined) {
    const vendoredFiles = walkFiles(VENDORED_PATH);
    fixtures = vendoredFiles.map(extractFixture);
    requireCondition(
      fixtures.every((value) => value !== undefined),
      "the vendored projection contains a file without a selected emit operation",
    );
  } else {
    const sourceFiles = walkFiles(sourceRoot);
    fixtures = validateFullSource(sourceFiles);
    validateVendoredProjection(fixtures);
  }
  fixtures.sort((left, right) => compareNames(left.path, right.path));
  const summary = projectionSummary(fixtures);
  requireJsonEqual(summary, EXPECTED_SUMMARY, "FourSlash projection summary");

  const vendoredFiles = walkFiles(VENDORED_PATH);
  const projectionTree = treeIdentity(vendoredFiles);
  requireCondition(
    projectionTree.files === summary.fixture_files &&
      projectionTree.bytes === summary.fixture_bytes &&
      projectionTree.unique_blobs === summary.unique_blobs,
    "projection tree and fixture summary disagree",
  );

  return {
    schema: 1,
    kind: "typescript-fourslash-batch-emit-projection",
    status: "inventory-only-not-run",
    typescript_version: TYPESCRIPT_VERSION,
    source_repository: SOURCE_REPOSITORY,
    source_commit: SOURCE_COMMIT,
    generator: {
      path: GENERATOR_RELATIVE_PATH,
      sha256: sha256(fs.readFileSync(GENERATOR_PATH)),
    },
    source_tree: SOURCE_TREE,
    selector: {
      operation_call_pattern: OPERATION_CALL_PATTERN,
      operation_call_flags: OPERATION_CALL_FLAGS,
      operation_methods: OPERATION_METHODS,
      metadata_pattern: EMIT_THIS_FILE_PATTERN,
      metadata_flags: EMIT_THIS_FILE_FLAGS,
      broad_mentions: 40,
      selected_operation_files: 38,
      false_positive_controls: FALSE_POSITIVE_CONTROLS,
    },
    projection: {
      vendored_path: VENDORED_RELATIVE_PATH,
      ...projectionTree,
      summary,
    },
    fixtures,
    qualification: {
      expansion_rows: 0,
      executed_rows: 0,
      passing_rows: 0,
      source_inventory_integrity: true,
      fourslash_pass_rate: false,
      language_service_compatibility: false,
      whole_program_emit_equivalence: false,
    },
  };
}

function parseArguments(arguments_) {
  let mode;
  let sourceRoot;
  for (let index = 0; index < arguments_.length; index += 1) {
    const argument = arguments_[index];
    if (argument === "--write" || argument === "--check") {
      requireCondition(mode === undefined, "choose exactly one mode");
      mode = argument;
    } else if (argument === "--source-root") {
      requireCondition(sourceRoot === undefined, "--source-root was repeated");
      sourceRoot = arguments_[index + 1];
      requireCondition(sourceRoot !== undefined, "--source-root requires a path");
      index += 1;
    } else {
      fail(
        "usage: fourslash-emit-projection.mjs [--write|--check] [--source-root PATH]",
      );
    }
  }
  if (sourceRoot !== undefined) {
    sourceRoot = path.resolve(sourceRoot);
    requireCondition(fs.statSync(sourceRoot).isDirectory(), "--source-root is not a directory");
  }
  return { mode, sourceRoot };
}

const { mode, sourceRoot } = parseArguments(process.argv.slice(2));
const artifact = buildArtifact(sourceRoot);
const rendered = `${JSON.stringify(artifact, null, 2)}\n`;

if (mode === "--write") {
  fs.writeFileSync(TARGET_PATH, rendered);
  process.stdout.write(`wrote ${TARGET_RELATIVE_PATH}\n`);
} else if (mode === "--check") {
  requireCondition(fs.existsSync(TARGET_PATH), `missing ${TARGET_RELATIVE_PATH}`);
  requireCondition(
    fs.readFileSync(TARGET_PATH, "utf8") === rendered,
    `stale ${TARGET_RELATIVE_PATH}; run with --write and review`,
  );
  process.stdout.write(
    `FourSlash emit projection is fresh: fixtures=${EXPECTED_SUMMARY.fixture_files} operations=${EXPECTED_SUMMARY.operation_calls} status=not-run\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("unreachable mode");
}
