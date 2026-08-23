import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-transition.mjs";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE_VERSION = "25.2.1";

const OWNER_RELATIVE_PATH = "ratchets/h2-owner-inventory.v1.json";
const CANDIDATE_RELATIVE_PATH = "ratchets/h2-candidate-dispositions.v1.json";
const PROFILE_RELATIVE_PATH = "ratchets/h2-profile-transition.v1.json";

const CONTRACTS = Object.freeze({
  owner: ".github/ci/contracts/h2-owner-inventory.schema.json",
  candidates: ".github/ci/contracts/h2-candidate-dispositions.schema.json",
  profile: ".github/ci/contracts/h2-profile-transition.schema.json",
  sourceReachability: ".github/ci/contracts/h2-source-reachability.schema.json",
  emitObservation: ".github/ci/contracts/h2-emit-observation.schema.json",
  runtimeBaseline: ".github/ci/contracts/h2-runtime-baseline.schema.json",
});

const INPUT_HASHES = Object.freeze({
  "ratchets/h1-owner-inventory.v1.json":
    "6148160678bf0b34a8310551eac8c9ab3f2afb1cd9260fa8eaa59efadc71abb5",
  "ratchets/h1-rust-omissions.v1.json":
    "9d8568dc0978752ce14af11ca1cb6226b147b5cb0126a56c24ca9086cb9130ff",
  "ratchets/h1-emit-profile.v1.json":
    "d7a7d212780ef94cb9675c104ec8d2ca28af95764fa78f8aeb8c7c25885fa7db",
  "ratchets/h1-emit-oracle.v1.json":
    "c0c06a1472c2f49d9d90b733f3d594e737d62d350da9e4c8317d7e2331c0056d",
  "ratchets/h1-emit-qualification.v1.json":
    "910d1c4a4b24f7dc5feb96282a3d4d0ae7a8f017ff73cd243e7cfb92b45e1036",
  "vendor/typescript-6.0.3/compiler-profile-classification.v1.json":
    "7158d2e4fac5b6d43ee9382d5dadac7d27e358c86bd532e07b4d1f9ff85ad5b0",
  "vendor/typescript-6.0.3/conformance-profile-classification.v1.json":
    "43c3e4d6f6273f2264e7eed96348795bf58caecf30d81b365c4c0dc8d630a990",
  "vendor/typescript-6.0.3/project-profile-classification.v1.json":
    "b89589c1372a2c2bb4d8415f8f5b3168605fd11cb43d5b9b55828d834f54342a",
  "vendor/typescript-6.0.3/transpile-suite-inventory.v1.json":
    "c254834286cf54f23888ace6996d0e7729aec12313caa76d125772d9b58a79e0",
  "vendor/typescript-6.0.3/test-suite-expansion.v1.json":
    "9c6e991103b571f7a8800dc5e1ef66088017689f3769de8fcdb408b2dc125188",
  "vendor/typescript-6.0.3/conformance-suite-expansion.v1.json":
    "924d4007b3ac93a3ee57032ea6089b649bab2902e30ee64cff02f4c9404b7bbd",
  "vendor/typescript-6.0.3/lib/_tsc.js":
    "1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3",
  "vendor/typescript-6.0.3/lib/typescript.js":
    "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39",
});

const TSC_SOURCE = "vendor/typescript-6.0.3/lib/_tsc.js";
const TYPESCRIPT_SOURCE = "vendor/typescript-6.0.3/lib/typescript.js";
const H1_COMPATIBLE_CASE =
  "typescript-6.0.3/compiler/esmNoSynthesizedDefault.ts#module%3Dpreserve";

function requireCondition(condition, message) {
  if (!condition) throw new Error(message);
}

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function fileBytes(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE, relativePath));
}

function fileText(relativePath) {
  return fileBytes(relativePath).toString("utf8");
}

function readJson(relativePath) {
  return JSON.parse(fileText(relativePath));
}

function pathHash(relativePath) {
  return { path: relativePath, sha256: sha256(fileBytes(relativePath)) };
}

function expectedPathHash(relativePath) {
  const record = pathHash(relativePath);
  const expected = INPUT_HASHES[relativePath];
  requireCondition(expected !== undefined, `missing expected hash for ${relativePath}`);
  requireCondition(record.sha256 === expected, `${relativePath} differs from the reviewed H2.0a input`);
  return record;
}

function generatedPathHash(relativePath, rendered) {
  return { path: relativePath, sha256: sha256(Buffer.from(rendered, "utf8")) };
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value !== null && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function withFingerprint(value, field) {
  const semantic = { ...value };
  delete semantic[field];
  return { ...value, [field]: sha256(canonical(semantic)) };
}

function countBy(values, key) {
  const counts = new Map();
  for (const value of values) counts.set(value[key], (counts.get(value[key]) ?? 0) + 1);
  return [...counts.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([value, cases]) => ({ value, cases }));
}

function validateRuntime() {
  requireCondition(ts.version === "6.0.3", `unexpected TypeScript runtime ${ts.version}`);
  const nodeVersion = fileText(".node-version").trim();
  requireCondition(nodeVersion === EXPECTED_NODE_VERSION, "unexpected Node pin");
  requireCondition(
    process.version === `v${EXPECTED_NODE_VERSION}`,
    `H2.0a generator requires Node ${EXPECTED_NODE_VERSION}; running ${process.version}`,
  );
  for (const relativePath of Object.keys(INPUT_HASHES)) expectedPathHash(relativePath);
  for (const relativePath of Object.values(CONTRACTS)) {
    const schema = readJson(relativePath);
    requireCondition(
      schema.$schema === "https://json-schema.org/draft/2020-12/schema" &&
        schema.additionalProperties === false,
      `invalid strict schema boundary ${relativePath}`,
    );
  }
}

const ROOT_SPECS = Object.freeze([
  ["own-output-path", TSC_SOURCE, "getOwnEmitOutputFilePath", 16567, "H2.8a", "partial-h1-residual", "single-file output path worker", "ordinary JavaScript path planning"],
  ["declaration-output-path", TSC_SOURCE, "getDeclarationEmitOutputFilePath", 16577, "H2.7b", "deferred-h2", "declaration output path worker", "declaration output enabled"],
  ["source-files-to-emit", TSC_SOURCE, "getSourceFilesToEmit", 16600, "H2.8a", "partial-h1-residual", "emit source selection", "whole or targeted emit selection"],
  ["source-file-may-emit", TSC_SOURCE, "sourceFileMayBeEmitted", 16617, "H2.8a", "partial-h1-residual", "per-source emit eligibility", "source kind and output route"],
  ["write-file", TSC_SOURCE, "writeFile", 16644, "H2.8a", "partial-h1-residual", "write callback and diagnostic boundary", "an output product reaches its sink"],
  ["parse-command-line", TSC_SOURCE, "parseCommandLine", 38255, "H2.8e", "partial-h1-residual", "command-line option parser", "one-shot CLI invocation"],
  ["source-map-generator", TSC_SOURCE, "createSourceMapGenerator", 92365, "H2.6a", "deferred-h2", "source-map generator", "external or inline source maps enabled"],
  ["transform-typescript", TSC_SOURCE, "transformTypeScript", 94036, "H2.2a", "partial-h1-residual", "TypeScript syntax transformer", "TypeScript source file"],
  ["transform-class-fields", TSC_SOURCE, "transformClassFields", 95852, "H2.4b", "partial-h1-residual", "class fields transformer", "class feature transform flags"],
  ["transform-legacy-decorators", TSC_SOURCE, "transformLegacyDecorators", 98430, "H2.4a", "deferred-h2", "legacy decorator transformer", "experimentalDecorators"],
  ["transform-standard-decorators", TSC_SOURCE, "transformESDecorators", 98946, "H2.4b", "deferred-h2", "standard decorator transformer", "standard decorators or class lowering"],
  ["transform-es2017", TSC_SOURCE, "transformES2017", 100810, "H2.5f", "deferred-h2", "ES2017 transformer", "target below ES2017"],
  ["transform-es2018", TSC_SOURCE, "transformES2018", 101680, "H2.5e", "deferred-h2", "ES2018 transformer", "target below ES2018"],
  ["transform-es2019", TSC_SOURCE, "transformES2019", 102907, "H2.5d", "deferred-h2", "ES2019 transformer", "target below ES2019"],
  ["transform-es2020", TSC_SOURCE, "transformES2020", 102943, "H2.5c", "deferred-h2", "ES2020 transformer", "target below ES2020"],
  ["transform-es2021", TSC_SOURCE, "transformES2021", 103205, "H2.5b", "deferred-h2", "ES2021 transformer", "target below ES2021"],
  ["transform-esnext", TSC_SOURCE, "transformESNext", 103278, "H2.5a", "deferred-h2", "ESNext-to-standard transformer", "target below ESNext"],
  ["transform-jsx", TSC_SOURCE, "transformJsx", 103845, "H2.3b", "deferred-h2", "JSX transformer", "JSX emit mode enabled"],
  ["transform-es2016", TSC_SOURCE, "transformES2016", 104646, "H2.5g", "deferred-h2", "ES2016 transformer", "target below ES2016"],
  ["transform-es2015", TSC_SOURCE, "transformES2015", 104740, "H2.5h", "deferred-h2", "ES2015 transformer", "target below ES2015"],
  ["transform-generators", TSC_SOURCE, "transformGenerators", 108119, "H2.5h", "deferred-h2", "generator transformer", "target below ES2015"],
  ["transform-module", TSC_SOURCE, "transformModule", 110090, "H2.1b", "deferred-h2", "CommonJS/AMD/UMD module transformer", "non-System non-ESM module selection"],
  ["transform-system-module", TSC_SOURCE, "transformSystemModule", 112050, "H2.1d", "deferred-h2", "System module transformer", "module System"],
  ["transform-es-module", TSC_SOURCE, "transformECMAScriptModule", 113369, "H1", "closed-h1", "ECMAScript module transformer", "module Preserve H1 path"],
  ["transform-implied-module", TSC_SOURCE, "transformImpliedNodeFormatDependentModule", 113730, "H2.1a", "deferred-h2", "per-file ESM/CJS dispatcher", "ESM or Node module kind"],
  ["transform-declarations", TSC_SOURCE, "transformDeclarations", 114265, "H2.7a", "deferred-h2", "declaration transformer", "declaration output"],
  ["module-transformer-selection", TSC_SOURCE, "getModuleTransformer", 115876, "H2.1a", "partial-h1-residual", "module transformer dispatcher", "script transformer construction"],
  ["transformer-selection", TSC_SOURCE, "getTransformers", 115897, "H2.1a", "partial-h1-residual", "script/declaration transformer pair", "Program emit"],
  ["script-transformer-selection", TSC_SOURCE, "getScriptTransformers", 115903, "H2.1a", "partial-h1-residual", "ordered built-in script transformer selection", "non-declaration emit"],
  ["declaration-transformer-selection", TSC_SOURCE, "getDeclarationTransformers", 115950, "H2.7a", "deferred-h2", "ordered declaration transformer selection", "declaration emit"],
  ["transform-runtime", TSC_SOURCE, "transformNodes", 115977, "H2.1a", "partial-h1-residual", "transform lifecycle and hook composition", "one or more script transformers"],
  ["output-enumeration", TSC_SOURCE, "forEachEmittedFile", 116312, "H2.8a", "partial-h1-residual", "ordered output enumeration", "one-shot emit"],
  ["bundle-output-paths", TSC_SOURCE, "getOutputPathsForBundle", 116365, "H2.7d", "deferred-h2", "bundle output path planning", "outFile bundle"],
  ["source-output-paths", TSC_SOURCE, "getOutputPathsFor", 116373, "H2.8a", "partial-h1-residual", "source-file output path planning", "non-bundle output"],
  ["source-map-output-path", TSC_SOURCE, "getSourceMapFilePath", 116388, "H2.6a", "deferred-h2", "source-map output path planning", "external source map"],
  ["output-extension", TSC_SOURCE, "getOutputExtension", 116391, "H2.8a", "partial-h1-residual", "source-kind output extension", "emittable source file"],
  ["declaration-output-name", TSC_SOURCE, "getOutputDeclarationFileName", 116400, "H2.7b", "deferred-h2", "config declaration output name", "declaration output"],
  ["javascript-output-name", TSC_SOURCE, "getOutputJSFileName", 116409, "H2.8a", "partial-h1-residual", "config JavaScript output name", "JavaScript output"],
  ["emit-files", TSC_SOURCE, "emitFiles", 116530, "H2.8a", "partial-h1-residual", "emit orchestration", "Program emit worker"],
  ["printer", TSC_SOURCE, "createPrinter", 116912, "H2.1a", "partial-h1-residual", "printer and substitution/notification hooks", "any textual output"],
  ["program-emit", TSC_SOURCE, "emit", 123568, "H2.8d", "partial-h1-residual", "Program.emit request boundary", "whole or targeted Program emit"],
  ["emit-module-format", TSC_SOURCE, "getEmitModuleFormatOfFileWorker", 125493, "H2.1a", "deferred-h2", "per-file emitted module format", "implied-format dispatcher"],
  ["implied-node-format", TSC_SOURCE, "getImpliedNodeFormatForEmitWorker", 125496, "H2.1e", "deferred-h2", "Node package/extension implied format", "Node module modes"],
  ["emit-and-report", TSC_SOURCE, "emitFilesAndReportErrors", 129412, "H2.8e", "partial-h1-residual", "diagnostic/emit/report/exit ordering", "one-shot compiler reporting"],
  ["cli-worker", TSC_SOURCE, "executeCommandLineWorker", 132583, "H2.8e", "partial-h1-residual", "CLI mode dispatcher", "tsc command line"],
  ["cli-entry", TSC_SOURCE, "executeCommandLine", 132742, "H2.8e", "partial-h1-residual", "CLI entry and locale/config setup", "tsc command line"],
  ["perform-compilation", TSC_SOURCE, "performCompilation", 132859, "H2.8e", "partial-h1-residual", "one-shot CLI compilation", "non-build non-watch CLI"],
  ["transpile-module", TYPESCRIPT_SOURCE, "transpileModule", 145985, "H2.8c", "deferred-h2", "transpileModule API entry", "transpile module runner"],
  ["transpile-declaration", TYPESCRIPT_SOURCE, "transpileDeclaration", 145993, "H2.8c", "deferred-h2", "transpileDeclaration API entry", "transpile declaration runner"],
  ["transpile-worker", TYPESCRIPT_SOURCE, "transpileWorker", 146022, "H2.8c", "deferred-h2", "shared transpile pipeline", "transpile API call"],
]);

function sourcePosition(sourceFile, offset) {
  const position = sourceFile.getLineAndCharacterOfPosition(offset);
  return { offset, line: position.line + 1, character: position.character + 1 };
}

function sourceSliceHash(text, startLine, endLine) {
  return sha256(text.split(/(?<=\n)/u).slice(startLine - 1, endLine).join(""));
}

function bundleIndex(relativePath) {
  const absolutePath = path.join(WORKSPACE, relativePath);
  const text = fileText(relativePath);
  const program = ts.createProgram({
    rootNames: [absolutePath],
    options: { allowJs: true, checkJs: false, noResolve: true, target: ts.ScriptTarget.Latest },
  });
  const sourceFile = program.getSourceFile(absolutePath);
  requireCondition(sourceFile !== undefined, `TypeScript did not load ${relativePath}`);
  const checker = program.getTypeChecker();
  const declarations = [];
  function visit(node) {
    if (ts.isFunctionDeclaration(node) && node.name) declarations.push(node);
    ts.forEachChild(node, visit);
  }
  visit(sourceFile);
  return { relativePath, text, sourceFile, checker, declarations };
}

function selectDeclaration(index, name, line) {
  const matches = index.declarations.filter(
    (node) =>
      node.name.text === name && sourcePosition(index.sourceFile, node.getStart(index.sourceFile)).line === line,
  );
  requireCondition(matches.length === 1, `expected one ${name} at ${index.relativePath}:${line}, found ${matches.length}`);
  return matches[0];
}

function declarationRecord(index, node) {
  const startOffset = node.getStart(index.sourceFile);
  const endOffset = node.end;
  const start = sourcePosition(index.sourceFile, startOffset);
  const end = sourcePosition(index.sourceFile, endOffset);
  const bodyStart = node.body.getStart(index.sourceFile);
  const bodyEnd = node.body.end;
  const declarationSha256 = sha256(index.text.slice(startOffset, endOffset));
  const identity = canonical({
    source: index.relativePath,
    name: node.name.text,
    kind: "FunctionDeclaration",
    start: startOffset,
    end: endOffset,
    declaration_sha256: declarationSha256,
  });
  return {
    id: `h2:${sha256(identity)}`,
    source: index.relativePath,
    name: node.name.text,
    kind: "FunctionDeclaration",
    lexical_path: `<bundle>/${node.name.text}@${start.line}:${start.character}`,
    source_range: { start, end },
    declaration_sha256: declarationSha256,
    body_sha256: sha256(index.text.slice(bodyStart, bodyEnd)),
    ledger_slice_sha256: sourceSliceHash(index.text, start.line, end.line),
  };
}

function usageKind(identifier) {
  const parent = identifier.parent;
  if (ts.isCallExpression(parent)) {
    if (parent.expression === identifier) return "call";
    if (parent.arguments.includes(identifier)) return "argument";
  }
  if (ts.isReturnStatement(parent)) return "return";
  if (ts.isArrayLiteralExpression(parent)) return "array-element";
  return "reference";
}

function buildOwnerInventory() {
  const indices = new Map([
    [TSC_SOURCE, bundleIndex(TSC_SOURCE)],
    [TYPESCRIPT_SOURCE, bundleIndex(TYPESCRIPT_SOURCE)],
  ]);
  const selected = ROOT_SPECS.map(
    ([key, sourcePath, name, line, ownerSlice, disposition, role, activation]) => {
      const index = indices.get(sourcePath);
      const node = selectDeclaration(index, name, line);
      return {
        key,
        index,
        node,
        output: {
          key,
          role,
          owner_slice: ownerSlice,
          disposition,
          activation,
          declaration: declarationRecord(index, node),
        },
      };
    },
  );
  const keys = new Set(selected.map((entry) => entry.key));
  requireCondition(keys.size === selected.length, "duplicate H2 owner key");

  const selectedByDeclaration = new Map(selected.map((entry) => [entry.node, entry]));
  const dependencies = new Map();
  for (const owner of selected) {
    function visit(node) {
      if (ts.isIdentifier(node) && node !== owner.node.name) {
        const symbol = owner.index.checker.getSymbolAtLocation(node);
        for (const declaration of symbol?.declarations ?? []) {
          const target = selectedByDeclaration.get(declaration);
          if (!target || target.key === owner.key) continue;
          const dependencyKey = `${owner.key}\0${target.key}`;
          let dependency = dependencies.get(dependencyKey);
          if (!dependency) {
            dependency = { from: owner.key, to: target.key, usageKinds: new Set(), sites: new Map() };
            dependencies.set(dependencyKey, dependency);
          }
          dependency.usageKinds.add(usageKind(node));
          const position = sourcePosition(owner.index.sourceFile, node.getStart(owner.index.sourceFile));
          dependency.sites.set(`${position.line}:${position.character}`, {
            line: position.line,
            character: position.character,
          });
        }
      }
      ts.forEachChild(node, visit);
    }
    visit(owner.node.body);
  }
  const dependencyRows = [...dependencies.values()]
    .map((entry) => ({
      from: entry.from,
      to: entry.to,
      usage_kinds: [...entry.usageKinds].sort(),
      sites: [...entry.sites.values()].sort(
        (left, right) => left.line - right.line || left.character - right.character,
      ),
    }))
    .sort((left, right) => left.from.localeCompare(right.from) || left.to.localeCompare(right.to));

  const h1Rust = readJson("ratchets/h1-rust-omissions.v1.json");
  const anchors = new Map(h1Rust.evidence.anchors.map((anchor) => [anchor.id, anchor]));
  const converseSpecs = [
    ["emitter-script-transformer-selection", ["module-transformer-selection", "transformer-selection", "script-transformer-selection"], "implements-h1"],
    ["emitter-transform-typescript", ["transform-typescript"], "implements-h1"],
    ["emitter-transform-class-fields", ["transform-class-fields"], "implements-h1"],
    ["emitter-transform-ecmascript-module", ["transform-es-module"], "implements-h1"],
    ["emitter-transform-nodes", ["transform-runtime"], "shared-h1-seam"],
    ["emitter-printer-factory", ["printer"], "shared-h1-seam"],
    ["emitter-source-map-recorder", ["source-map-generator", "source-map-output-path"], "dormant-h2-seam"],
    ["emitter-output-plan-shape", ["own-output-path", "declaration-output-path", "bundle-output-paths", "source-output-paths"], "shared-h1-seam"],
    ["emitter-emit-files", ["emit-files", "output-enumeration"], "shared-h1-seam"],
    ["program-session-emit", ["program-emit"], "implements-h1"],
    ["cli-mode-dispatch", ["parse-command-line", "cli-worker", "cli-entry", "perform-compilation"], "shared-h1-seam"],
    ["cli-filesystem-sink-dispatch", ["emit-and-report", "write-file"], "shared-h1-seam"],
    ["checker-emit-resolver-adapter", ["transform-typescript", "transform-class-fields", "transform-es-module", "transform-implied-module"], "shared-h1-seam"],
    ["compiler-options-current-shape", ["script-transformer-selection", "output-extension", "javascript-output-name"], "shared-h1-seam"],
  ];
  const rustConverse = converseSpecs.map(([anchorId, upstreamOwners, disposition]) => {
    const anchor = anchors.get(anchorId);
    requireCondition(anchor !== undefined, `missing Rust converse anchor ${anchorId}`);
    for (const ownerKey of upstreamOwners) requireCondition(keys.has(ownerKey), `unknown converse owner ${ownerKey}`);
    return {
      anchor: {
        id: anchor.id,
        path: anchor.path,
        line: anchor.line,
        text_sha256: anchor.text_sha256,
        file_sha256: anchor.file_sha256,
      },
      upstream_owners: [...upstreamOwners].sort(),
      disposition,
    };
  });

  const owners = selected.map((entry) => entry.output).sort((left, right) => left.key.localeCompare(right.key));
  const summary = {
    owner_roots: owners.length,
    closed_h1_roots: owners.filter((owner) => owner.disposition === "closed-h1").length,
    partial_h1_roots: owners.filter((owner) => owner.disposition === "partial-h1-residual").length,
    deferred_h2_roots: owners.filter((owner) => owner.disposition === "deferred-h2").length,
    dependency_edges: dependencyRows.length,
    rust_converse_rows: rustConverse.length,
    undispositioned_owners: owners.filter((owner) => !owner.owner_slice || !owner.disposition).length,
    unmapped_rust_converse_rows: rustConverse.filter((row) => row.upstream_owners.length === 0).length,
  };
  requireCondition(summary.owner_roots === 50, `unexpected H2 owner root count ${summary.owner_roots}`);
  requireCondition(summary.undispositioned_owners === 0, "H2 owner inventory retained an undispositioned owner");
  requireCondition(summary.unmapped_rust_converse_rows === 0, "H2 Rust converse retained an unmapped row");

  const owner = {
    schema: 1,
    status: "frozen",
    phase: "H2.0a-owner-converse-inventory",
    typescript: {
      version: ts.version,
      source_commit: SOURCE_COMMIT,
      compiler: expectedPathHash(TSC_SOURCE),
      library: expectedPathHash(TYPESCRIPT_SOURCE),
    },
    generator: pathHash(GENERATOR_RELATIVE_PATH),
    contract: pathHash(CONTRACTS.owner),
    inputs: {
      h1_owner_inventory: expectedPathHash("ratchets/h1-owner-inventory.v1.json"),
      h1_rust_omissions: expectedPathHash("ratchets/h1-rust-omissions.v1.json"),
    },
    closure_model:
      "complete H2 one-shot compiler owner-root universe plus exact source-symbol references between roots; each runtime slice regenerates the transitive helper closure and may suffix-split independent SCCs before admission",
    slices: [...new Set(owners.map((entry) => entry.owner_slice))].sort(),
    owners,
    dependencies: dependencyRows,
    rust_converse: rustConverse,
    summary,
  };
  return withFingerprint(owner, "inventory_fingerprint_sha256");
}

const SLICE_ORDER = Object.freeze([
  "H2.0a", "H2.0b", "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e",
  "H2.2a", "H2.2b", "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c",
  "H2.3d", "H2.4a", "H2.4b", "H2.5a", "H2.5b", "H2.5c", "H2.5d",
  "H2.5e", "H2.5f", "H2.5g", "H2.5h", "H2.6a", "H2.6b", "H2.6c",
  "H2.7a", "H2.7b", "H2.7c", "H2.7d", "H2.7e", "H2.8a", "H2.8b",
  "H2.8c", "H2.8d", "H2.8e", "H2.9",
]);
const SLICE_RANK = new Map(SLICE_ORDER.map((slice, index) => [slice, index]));

function blockerSlice(blocker) {
  const module = blocker.match(/^required-option:module=([^()]+)(?:\([0-9]+\))?$/u)?.[1];
  if (module !== undefined) {
    const normalized = module.toLowerCase();
    if (["absent", "esnext", "es2015", "es2020", "es2022"].includes(normalized)) return "H2.1a";
    if (["commonjs", "none"].includes(normalized)) return "H2.1b";
    if (["amd", "umd"].includes(normalized)) return "H2.1c";
    if (normalized === "system") return "H2.1d";
    if (["node16", "node18", "node20", "nodenext"].includes(normalized)) return "H2.1e";
  }
  const target = blocker.match(/^required-option:target=([^()]+)(?:\([0-9]+\))?$/u)?.[1];
  if (target !== undefined) {
    const normalized = target.toLowerCase();
    if (["es2021", "es2022", "es2023", "es2024", "es2025"].includes(normalized)) return "H2.5a";
    if (normalized === "es2020") return "H2.5b";
    if (normalized === "es2019") return "H2.5c";
    if (normalized === "es2018") return "H2.5d";
    if (normalized === "es2017") return "H2.5e";
    if (normalized === "es2016") return "H2.5f";
    if (["es2015", "es6"].includes(normalized)) return "H2.5g";
    if (["absent", "es5", "es3"].includes(normalized)) return "H2.5h";
  }
  if (blocker.startsWith("required-option:useDefineForClassFields=")) return "H2.4b";
  if (blocker === "route:noEmit=true") return "H2.9";
  if (blocker.startsWith("api:component-only:")) return "H2.8c";
  if (blocker === "product:declaration") return "H2.7b";
  const option = blocker.match(/^rejected-option:(.+)$/u)?.[1];
  const optionSlices = {
    allowImportingTsExtensions: "H2.1e",
    allowJs: "H2.3a",
    composite: "H2.8b",
    declaration: "H2.7b",
    declarationDir: "H2.7c",
    declarationMap: "H2.7e",
    emitDeclarationOnly: "H2.7b",
    experimentalDecorators: "H2.4a",
    importHelpers: "H2.8b",
    incremental: "H2.8b",
    inlineSourceMap: "H2.6b",
    isolatedModules: "H2.8c",
    jsx: "H2.3b",
    noCheck: "H2.8c",
    noEmitHelpers: "H2.8b",
    outDir: "H2.8a",
    outFile: "H2.7d",
    rewriteRelativeImportExtensions: "H2.1e",
    resolveJsonModule: "H2.3d",
    sourceMap: "H2.6a",
    tsBuildInfoFile: "H2.8b",
    verbatimModuleSyntax: "H2.8c",
  };
  if (option !== undefined && optionSlices[option]) return optionSlices[option];
  const feature = blocker.match(/^rejected-feature:(.+)$/u)?.[1];
  const featureSlices = {
    decorators: "H2.4a",
    "export-equals": "H2.2d",
    "import-equals": "H2.2d",
    jsx: "H2.3b",
    "parameter-properties": "H2.2c",
    "runtime-enums": "H2.2a",
    "runtime-namespaces": "H2.2b",
  };
  if (feature !== undefined && featureSlices[feature]) return featureSlices[feature];
  throw new Error(`H2.0a has no owner slice for blocker ${blocker}`);
}

function sourceKindSlice(sourcePath) {
  const lower = sourcePath.toLowerCase();
  if (lower.endsWith(".tsx")) return "H2.3b";
  if (lower.endsWith(".jsx")) return "H2.3b";
  if (lower.endsWith(".js") || lower.endsWith(".mjs") || lower.endsWith(".cjs")) return "H2.3a";
  if (lower.endsWith(".json")) return "H2.3d";
  if (lower.endsWith(".mts") || lower.endsWith(".cts")) return "H2.1e";
  return null;
}

function requiredSlicesFor(suite, sourcePath, blockers, caseRecord) {
  const slices = new Set(blockers.map(blockerSlice));
  const sourceSlice = sourceKindSlice(sourcePath);
  if (sourceSlice) slices.add(sourceSlice);
  if (suite === "transpile") {
    slices.add("H2.8c");
    if (caseRecord.component_disposition === "deferred-source-map-control") slices.add("H2.6a");
    if (caseRecord.component_disposition === "deferred-declaration-control") slices.add("H2.7b");
    if (caseRecord.component_disposition === "deferred-declaration-map-control") slices.add("H2.7e");
  }
  if (suite !== "transpile" && caseRecord.id !== H1_COMPATIBLE_CASE && slices.size === 0) {
    slices.add("H2.9");
  }
  return [...slices].sort((left, right) => SLICE_RANK.get(left) - SLICE_RANK.get(right));
}

function isModuleOnlyCandidate(suite, blockers) {
  return (
    ["compiler", "conformance"].includes(suite) &&
    blockers.length === 1 &&
    ["required-option:module=absent", "required-option:module=ESNext(99)"].includes(blockers[0])
  );
}

function buildCandidateDispositions() {
  const compiler = readJson("vendor/typescript-6.0.3/compiler-profile-classification.v1.json");
  const conformance = readJson("vendor/typescript-6.0.3/conformance-profile-classification.v1.json");
  const project = readJson("vendor/typescript-6.0.3/project-profile-classification.v1.json");
  const transpile = readJson("vendor/typescript-6.0.3/transpile-suite-inventory.v1.json");
  const expansion = readJson("vendor/typescript-6.0.3/test-suite-expansion.v1.json");
  const conformanceExpansion = readJson(
    "vendor/typescript-6.0.3/conformance-suite-expansion.v1.json",
  );
  const suiteInputs = [
    ["compiler", compiler.cases, expansion.sources],
    ["conformance", conformance.cases, conformanceExpansion.sources],
    ["project", project.cases, expansion.sources],
    ["transpile", transpile.cases, transpile.sources],
  ];
  const cases = [];
  for (const [suite, sourceCases, sources] of suiteInputs) {
    sourceCases.forEach((caseRecord, index) => {
      const source = sources[caseRecord.source];
      requireCondition(source !== undefined, `${suite} case ${caseRecord.id} has no source`);
      const blockers = [...caseRecord.profile_blockers];
      requireCondition(new Set(blockers).size === blockers.length, `${caseRecord.id} has duplicate blockers`);
      const closedH1 = caseRecord.id === H1_COMPATIBLE_CASE;
      const moduleOnly = isModuleOnlyCandidate(suite, blockers);
      const requiredSlices = closedH1
        ? []
        : requiredSlicesFor(suite, source.path, blockers, caseRecord);
      requireCondition(closedH1 || requiredSlices.length > 0, `${caseRecord.id} has no H2 disposition`);
      let candidateClass = "deferred-profile";
      let disposition = "deferred-to-slices";
      let sourceAnalysisState = "not-yet-required";
      if (closedH1) {
        candidateClass = "h1-compatible";
        disposition = "closed-h1-exact";
        sourceAnalysisState = "closed-h1";
      } else if (moduleOnly) {
        candidateClass = "h2.1a-module-only";
        disposition = "pending-source-analysis";
        sourceAnalysisState = "pending-owning-slice";
      } else if (suite === "transpile") {
        candidateClass = "component-only";
        disposition = "deferred-component-route";
      }
      cases.push({
        suite,
        upstream_case: caseRecord.expansion_case ?? index,
        id: caseRecord.id,
        source: {
          path: source.path,
          bytes: source.bytes,
          sha256: source.sha256,
          git_blob_sha1: source.git_blob_sha1,
        },
        profile_blockers: blockers,
        candidate_class: candidateClass,
        disposition,
        source_analysis_state: sourceAnalysisState,
        execution_state: closedH1 ? "executed-h1-exact" : "not-run",
        reference_baseline_state: closedH1 ? "compared-h1-exact" : "not-compared",
        required_slices: requiredSlices,
        next_slice: requiredSlices[0] ?? null,
      });
    });
  }
  cases.sort((left, right) => left.suite.localeCompare(right.suite) || left.id.localeCompare(right.id));
  requireCondition(new Set(cases.map((entry) => `${entry.suite}\0${entry.id}`)).size === cases.length, "duplicate H2 candidate identity");

  const suiteRows = ["compiler", "conformance", "project", "transpile"].map((suite) => {
    const rows = cases.filter((entry) => entry.suite === suite);
    return {
      suite,
      cases: rows.length,
      closed_h1: rows.filter((entry) => entry.disposition === "closed-h1-exact").length,
      module_only: rows.filter((entry) => entry.candidate_class === "h2.1a-module-only").length,
      deferred: rows.filter((entry) => !["closed-h1-exact", "pending-source-analysis"].includes(entry.disposition)).length,
    };
  });
  const moduleOnlyCases = cases.filter((entry) => entry.candidate_class === "h2.1a-module-only");
  const moduleOnlyStates = countBy(
    moduleOnlyCases.map((entry) => ({
      state: entry.profile_blockers[0] === "required-option:module=absent" ? "absent" : "ESNext(99)",
    })),
    "state",
  );
  const summary = {
    cases: cases.length,
    suite_rows: suiteRows,
    closed_h1_cases: cases.filter((entry) => entry.disposition === "closed-h1-exact").length,
    module_only_candidates: moduleOnlyCases.length,
    module_only_compiler_candidates: moduleOnlyCases.filter((entry) => entry.suite === "compiler").length,
    module_only_conformance_candidates: moduleOnlyCases.filter((entry) => entry.suite === "conformance").length,
    module_only_states: moduleOnlyStates,
    not_run_cases: cases.filter((entry) => entry.execution_state === "not-run").length,
    undispositioned_cases: cases.filter((entry) => !entry.disposition || (!entry.next_slice && entry.disposition !== "closed-h1-exact")).length,
  };
  requireCondition(summary.cases === 15642, `unexpected H2 runner denominator ${summary.cases}`);
  requireCondition(summary.closed_h1_cases === 1, "H2 candidate inventory lost the exact H1 case");
  requireCondition(summary.module_only_candidates === 295, "unexpected module-only candidate count");
  requireCondition(summary.module_only_compiler_candidates === 94, "unexpected compiler module-only count");
  requireCondition(summary.module_only_conformance_candidates === 201, "unexpected conformance module-only count");
  requireCondition(
    JSON.stringify(summary.module_only_states) ===
      JSON.stringify([
        { value: "absent", cases: 221 },
        { value: "ESNext(99)", cases: 74 },
      ]),
    `unexpected module-only state split ${JSON.stringify(summary.module_only_states)}`,
  );
  requireCondition(summary.undispositioned_cases === 0, "H2 candidate inventory retained an undispositioned case");

  const output = {
    schema: 1,
    status: "frozen",
    phase: "H2.0a-runner-candidate-dispositions",
    typescript: { version: ts.version, source_commit: SOURCE_COMMIT },
    generator: pathHash(GENERATOR_RELATIVE_PATH),
    contract: pathHash(CONTRACTS.candidates),
    inputs: [
      expectedPathHash("vendor/typescript-6.0.3/compiler-profile-classification.v1.json"),
      expectedPathHash("vendor/typescript-6.0.3/conformance-profile-classification.v1.json"),
      expectedPathHash("vendor/typescript-6.0.3/project-profile-classification.v1.json"),
      expectedPathHash("vendor/typescript-6.0.3/transpile-suite-inventory.v1.json"),
      expectedPathHash("vendor/typescript-6.0.3/test-suite-expansion.v1.json"),
      expectedPathHash("vendor/typescript-6.0.3/conformance-suite-expansion.v1.json"),
      expectedPathHash("ratchets/h1-emit-qualification.v1.json"),
    ],
    disposition_contract: {
      module_only_definition:
        "compiler or conformance row at target=ESNext whose sole H1 blocker is effective module absent or ESNext; source reachability is deliberately not inferred",
      source_analysis: "pending-until-owning-runtime-slice",
      execution: "not-run-except-frozen-h1-compatible-case",
      reference_baselines: "not-compared-except-frozen-h1-compatible-case",
      no_implicit_success: true,
    },
    cases,
    summary,
  };
  return withFingerprint(output, "inventory_fingerprint_sha256");
}

const TRANSITIONS = Object.freeze([
  ["H2.0a", "complete-evidence-only", ["H1.6"], ["owner-converse", "profile-manifest", "oracle-schemas", "runner-dispositions"], "all owners and 15,642 runner rows dispositioned; zero runtime admissions"],
  ["H2.0b", "complete-evidence-only", ["H2.0a"], ["no-emit", "H1-emit", "L1-edit", "binary-startup", "fault-resource-baselines"], "eight alternating approved-runner pairs, positive H1 controls, two output-fault observations, and zero activity across all 37 H2 runtime slices"],
  ["H2.1a", "next", ["H2.0b"], ["implied-module-dispatch", "ESM", "hook-composition"], "source analysis and exact execution for 295 option-level candidates"],
  ["H2.1b", "planned", ["H2.1a"], ["CommonJS", "interop", "helpers"], "exact CJS output and adjacent ESM controls"],
  ["H2.1c", "planned", ["H2.1b"], ["AMD", "UMD"], "exact wrapper/dependency/name observations"],
  ["H2.1d", "planned", ["H2.1c"], ["System"], "exact setter/execute/export ordering"],
  ["H2.1e", "planned", ["H2.1a", "H2.1d"], ["Node16", "Node18", "Node20", "NodeNext", "mts-cts"], "mixed-format project and package-format evidence"],
  ["H2.2a", "planned", ["H2.1b"], ["runtime-enum", "const-enum"], "exact runtime/inlining/preserve behavior"],
  ["H2.2b", "planned", ["H2.2a"], ["namespace", "module-declaration"], "exact nested/merged/export-container behavior"],
  ["H2.2c", "planned", ["H2.2a"], ["parameter-properties", "class-typescript-syntax"], "exact constructor and class-field ordering"],
  ["H2.2d", "planned", ["H2.1b", "H2.2c"], ["import-equals", "export-equals", "import-elision"], "exact resolver alias/value and module interaction"],
  ["H2.3a", "planned", ["H2.1e"], ["js", "mjs", "cjs", "allowJs", "checkJs"], "production Program JavaScript emit"],
  ["H2.3b", "planned", ["H2.3a"], ["classic-jsx", "tsx", "jsx-output"], "classic/preserve/native JSX observations"],
  ["H2.3c", "planned", ["H2.1b", "H2.3b"], ["automatic-jsx", "development-jsx"], "runtime import ordering and diagnostics"],
  ["H2.3d", "planned", ["H2.3a"], ["json", "resolveJsonModule"], "exact JSON bytes, paths, and collisions"],
  ["H2.4a", "planned", ["H2.2c"], ["legacy-decorators", "metadata"], "evaluation order and helper evidence"],
  ["H2.4b", "planned", ["H2.4a"], ["standard-decorators", "class-fields"], "ESNext and first-downlevel class/decorator closure"],
  ["H2.5a", "planned", ["H2.4b"], ["transformESNext"], "newest target transform closure"],
  ["H2.5b", "planned", ["H2.5a"], ["transformES2021"], "ES2021 transform closure"],
  ["H2.5c", "planned", ["H2.5b"], ["transformES2020"], "ES2020 transform closure"],
  ["H2.5d", "planned", ["H2.5c"], ["transformES2019"], "ES2019 transform closure"],
  ["H2.5e", "planned", ["H2.5d"], ["transformES2018"], "ES2018 transform closure"],
  ["H2.5f", "planned", ["H2.5e"], ["transformES2017"], "ES2017 transform closure"],
  ["H2.5g", "planned", ["H2.5f"], ["transformES2016"], "ES2016 transform closure"],
  ["H2.5h", "planned", ["H2.5g"], ["transformES2015", "transformGenerators"], "ES2015/generator transform closure"],
  ["H2.6a", "planned", ["H2.0b"], ["external-source-map", "range-recorder"], "exact map JSON and callback metadata"],
  ["H2.6b", "planned", ["H2.6a"], ["inline-map", "sources", "mapRoot", "sourceRoot"], "exact transformed/multi-source mapping"],
  ["H2.6c", "planned", ["H2.6b"], ["runner-map-qualification"], "every applicable upstream map observation exact"],
  ["H2.7a", "planned", ["H2.0a"], ["declaration-owner-inventory", "NodeBuilder", "declaration-printer"], "zero unresolved declaration owners"],
  ["H2.7b", "planned", ["H2.7a"], ["dts", "declaration-only", "callback-metadata"], "exact non-bundle declaration output"],
  ["H2.7c", "planned", ["H2.7b"], ["declaration-diagnostics", "stripInternal", "isolatedDeclarations"], "exact diagnostic and partial-output behavior"],
  ["H2.7d", "planned", ["H2.1d", "H2.7b"], ["bundle", "outFile"], "exact JS/declaration bundle ordering and failures"],
  ["H2.7e", "planned", ["H2.6b", "H2.7d"], ["declaration-map"], "exact d.ts.map bytes and metadata"],
  ["H2.8a", "planned", ["H2.0b"], ["output-directory", "collision", "filesystem-fault"], "exact Memory/Fs sink equivalence"],
  ["H2.8b", "planned", ["H2.8a"], ["config", "host", "System", "library-replacement"], "exact host fallback and diagnostic precedence"],
  ["H2.8c", "planned", ["H2.8b"], ["noCheck", "transpileModule", "transpileDeclaration"], "distinct no-check/transpile route evidence"],
  ["H2.8d", "planned", ["H2.8c"], ["targeted-emit", "emit-only", "cancellation", "callbacks"], "exact targeted Program.emit and cancellation behavior"],
  ["H2.8e", "planned", ["H2.8b"], ["CLI", "locale", "trace", "exit", "terminal"], "exact remaining one-shot CLI observations"],
  ["H2.9", "planned", ["H2.5h", "H2.6c", "H2.7e", "H2.8e"], ["broad-runner-qualification", "resource-freeze"], "every applicable runner observation executed and dispositioned"],
]);

function buildProfile(ownerRendered, candidateRendered) {
  const transitions = TRANSITIONS.map(([slice, state, dependencies, axes, closeEvidence]) => ({
    slice,
    state,
    dependencies,
    axes,
    close_evidence: closeEvidence,
  }));
  requireCondition(
    JSON.stringify(transitions.map((entry) => entry.slice)) === JSON.stringify(SLICE_ORDER),
    "H2 transition rows differ from the authoritative slice order",
  );
  const output = {
    schema: 1,
    status: "frozen-pre-runtime-baseline",
    phase: "H2.0b-baseline-transition",
    typescript: { version: ts.version, source_commit: SOURCE_COMMIT },
    generator: pathHash(GENERATOR_RELATIVE_PATH),
    contract: pathHash(CONTRACTS.profile),
    h1_frozen_inputs: [
      expectedPathHash("ratchets/h1-emit-profile.v1.json"),
      expectedPathHash("ratchets/h1-owner-inventory.v1.json"),
      expectedPathHash("ratchets/h1-rust-omissions.v1.json"),
      expectedPathHash("ratchets/h1-emit-oracle.v1.json"),
      expectedPathHash("ratchets/h1-emit-qualification.v1.json"),
    ],
    h2_inputs: {
      owner_inventory: generatedPathHash(OWNER_RELATIVE_PATH, ownerRendered),
      candidate_dispositions: generatedPathHash(CANDIDATE_RELATIVE_PATH, candidateRendered),
    },
    oracle_contracts: {
      source_reachability: pathHash(CONTRACTS.sourceReachability),
      emit_observation: pathHash(CONTRACTS.emitObservation),
      runtime_baseline: pathHash(CONTRACTS.runtimeBaseline),
    },
    current_runtime_profile: {
      source: "ratchets/h1-emit-profile.v1.json",
      execution: "single-project-one-shot-whole-program",
      target: "ESNext(99)",
      module: "Preserve(200)",
      products: ["javascript"],
      status: "frozen-h1",
    },
    first_runtime_candidate: {
      slice: "H2.1a",
      status: "not-admitted",
      target: "ESNext(99)",
      module_states: ["absent", "ESNext(99)"],
      source_kinds: [".ts"],
      candidate_cases: 295,
      claim: "option-level candidates only; source compatibility and execution remain unproven",
    },
    transitions,
    admission_contract: {
      monotonic: true,
      case_gate:
        "all required owner slices complete, source reachability exact, TypeScript oracle repeated deterministically, Rust exact, and adjacent failures occur before the first sink write",
      failure_boundary: "before-first-sink-write",
      h1_evidence_reuse: "forbidden-outside-exact-h1-profile",
      execution_default: "not-run",
      reference_default: "not-compared",
    },
    summary: {
      transition_rows: transitions.length,
      completed_rows: transitions.filter((entry) => entry.state === "complete-evidence-only").length,
      next_rows: transitions.filter((entry) => entry.state === "next").length,
      planned_rows: transitions.filter((entry) => entry.state === "planned").length,
      runtime_admissions: 0,
    },
  };
  requireCondition(output.summary.transition_rows === 39, "unexpected H2 transition row count");
  return withFingerprint(output, "profile_fingerprint_sha256");
}

function render(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

function writeOrCheck(relativePath, rendered, mode) {
  const absolutePath = path.join(WORKSPACE, relativePath);
  if (mode === "--write") {
    fs.writeFileSync(absolutePath, rendered);
    process.stdout.write(`wrote ${relativePath}\n`);
    return;
  }
  if (mode === "--check") {
    requireCondition(
      fs.existsSync(absolutePath) && fs.readFileSync(absolutePath, "utf8") === rendered,
      `stale ${relativePath}; run h2-transition.mjs --write and review`,
    );
  }
}

validateRuntime();
const owner = buildOwnerInventory();
const candidates = buildCandidateDispositions();
const ownerRendered = render(owner);
const candidateRendered = render(candidates);
const profile = buildProfile(ownerRendered, candidateRendered);
const profileRendered = render(profile);

const mode = process.argv[2];
if (mode === undefined) {
  process.stdout.write(
    render({ owner: owner.summary, candidates: candidates.summary, profile: profile.summary }),
  );
} else if (["--write", "--check"].includes(mode)) {
  writeOrCheck(OWNER_RELATIVE_PATH, ownerRendered, mode);
  writeOrCheck(CANDIDATE_RELATIVE_PATH, candidateRendered, mode);
  writeOrCheck(PROFILE_RELATIVE_PATH, profileRendered, mode);
  if (mode === "--check") {
    process.stdout.write(
      `H2.0b transition is fresh: owners=${owner.summary.owner_roots} cases=${candidates.summary.cases} module-only=295 baselines=1 undispositioned=0 admissions=0\n`,
    );
  }
} else {
  throw new Error("usage: h2-transition.mjs [--write|--check]");
}
