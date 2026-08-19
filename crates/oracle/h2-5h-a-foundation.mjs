import crypto from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-5h-a-foundation.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-5h-a-foundation.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-5h-a-foundation.schema.json";
const PARENT_PROFILE_RELATIVE_PATH = "ratchets/h2-5g-profile.v1.json";
const DISPOSITIONS_RELATIVE_PATH =
  "ratchets/h2-candidate-dispositions.v1.json";
const OWNER_INVENTORY_RELATIVE_PATH = "ratchets/h2-owner-inventory.v1.json";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const TYPESCRIPT_LIB_DIRECTORY = "vendor/typescript-6.0.3/lib";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";
const SELECTED_SLICE = "H2.5h";
const FOUNDATION_SLICE = "H2.5h-a";
// The landed H2.5g profile defers the activation split to this slice's
// owner graph: implementation slices are H2.5h-b+ and their exact cut is
// an output of this foundation, not an input.
const RUNTIME_ACTIVATION_SLICE = "H2.5h-b+";
const INTERNAL_OBSERVE_MODE = "--internal-observe-direct-control";

const CLOSED_THROUGH_H2_5G = Object.freeze([
  "H2.1a",
  "H2.1b",
  "H2.1c",
  "H2.1d",
  "H2.1e",
  "H2.2a",
  "H2.2b",
  "H2.2c",
  "H2.2d",
  "H2.3a",
  "H2.3b",
  "H2.3c",
  "H2.3d",
  "H2.4a",
  "H2.4b",
  "H2.5a",
  "H2.5b",
  "H2.5c",
  "H2.5d",
  "H2.5e",
  "H2.5f",
  "H2.5g",
]);

const OWNER_KEYS = Object.freeze([
  "transform-es2015",
  "transform-generators",
]);

const TRANSFORM_FLAG_NAMES = Object.freeze([
  "ContainsES2018",
  "ContainsES2017",
  "ContainsES2016",
  "ContainsES2015",
  "ContainsGenerator",
  "ContainsDestructuringAssignment",
  "ContainsLexicalThis",
  "ContainsRestOrSpread",
  "ContainsObjectRestOrSpread",
  "ContainsComputedPropertyName",
  "ContainsBlockScopedBinding",
  "ContainsBindingPattern",
  "ContainsYield",
  "ContainsAwait",
  "ContainsHoistedDeclarationOrCompletion",
]);

const LOOP_CHECK_FLAG_NAMES = Object.freeze([
  "LoopWithCapturedBlockScopedBinding",
  "ContainsCapturedBlockScopeBinding",
  "CapturedBlockScopedBinding",
  "BlockScopedBindingInLoop",
  "NeedsLoopOutParameter",
]);

function compilerOptions(extra = {}) {
  return {
    target: ts.ScriptTarget.ES5,
    module: ts.ModuleKind.ESNext,
    alwaysStrict: true,
    downlevelIteration: true,
    importHelpers: false,
    noEmitHelpers: false,
    newLine: ts.NewLineKind.LineFeed,
    useDefineForClassFields: false,
    useUnknownInCatchVariables: false,
    ignoreDeprecations: "6.0",
    ...extra,
  };
}

function directControl(controlId, layers, source, extra = {}) {
  return {
    control_id: controlId,
    layers,
    files: [{ path: "/project/input.ts", text: source }],
    roots: ["/project/input.ts"],
    compiler_options: compilerOptions(extra.compiler_options),
  };
}

// These are inputs, not expected outputs. Every observation and fingerprint is
// derived twice from the pinned TypeScript runtime below. H2.5h-a deliberately
// does not run the Rust emitter or admit any candidate program.
const DIRECT_CONTROL_SPECS = Object.freeze([
  directControl(
    "syntax-transform-flag-matrix",
    ["syntax"],
    [
      "function* generator(seed = 1) {",
      "  const read = () => seed;",
      "  yield read();",
      "  yield* [seed];",
      "}",
      "const holder = { *method() { yield 1; } };",
      "",
    ].join("\n"),
  ),
  directControl(
    "checker-colliding-block-scope",
    ["checker", "resolver"],
    [
      "declare function use(value: unknown): void;",
      "function collisionScope() {",
      "  var collisionValue = 0;",
      "  {",
      "    let collisionValue = 1;",
      "    use(collisionValue);",
      "  }",
      "}",
      "",
    ].join("\n"),
  ),
  directControl(
    "checker-captured-loop-bindings",
    ["checker", "resolver"],
    [
      "declare function use(value: unknown): void;",
      "for (let capturedValue = 0; capturedValue < 2; capturedValue++) {",
      "  use(() => capturedValue);",
      "}",
      "",
    ].join("\n"),
  ),
  directControl(
    "checker-arguments-and-catch-reference",
    ["checker", "resolver"],
    [
      "function localArguments(arguments: number) { return arguments; }",
      "function lexicalArguments() { return () => arguments; }",
      "function* catches() {",
      "  try { yield 1; }",
      "  catch (caughtValue) { yield caughtValue; }",
      "}",
      "",
    ].join("\n"),
  ),
  directControl(
    "factory-generated-name-lexical-environment",
    ["factory"],
    [
      "let _a = 0, _b = 0, _super = 0, value_1 = 0;",
      "class Base {}",
      "class Derived extends Base {",
      "  method(value = (() => this)()) { return value; }",
      "}",
      "",
    ].join("\n"),
  ),
  directControl(
    "factory-es2015-generator-helper-graph",
    ["factory"],
    [
      "class Base {}",
      "class Derived extends Base {}",
      "declare const iterable: number[];",
      "for (const item of iterable) { void item; }",
      "const spread = [...iterable];",
      "function* sequence() { yield* iterable; }",
      "",
    ].join("\n"),
  ),
]);

function fail(message) {
  throw new Error(message);
}

function requireCondition(condition, message) {
  if (!condition) fail(message);
}

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
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
  return { ...value, [field]: sha256(Buffer.from(canonical(value), "utf8")) };
}

function fingerprintIsValid(record, field) {
  if (record === null || typeof record !== "object") return false;
  const { [field]: storedFingerprint, ...rest } = record;
  return (
    typeof storedFingerprint === "string" &&
    storedFingerprint === sha256(Buffer.from(canonical(rest), "utf8"))
  );
}

function libraryInventoryRecord() {
  // The fresh-process observations resolve default libraries from disk
  // through the base compiler host; those .d.ts bytes drive the type
  // check but are not covered by the bundle/implementation hashes, so
  // the record pins the whole vendored lib inventory (gate-tax 2 R3-2).
  const directory = path.join(WORKSPACE, TYPESCRIPT_LIB_DIRECTORY);
  const names = fs
    .readdirSync(directory)
    .filter((name) => name.startsWith("lib.") && name.endsWith(".d.ts"))
    .sort();
  requireCondition(
    names.length > 0,
    "vendored TypeScript lib inventory is empty",
  );
  const hash = crypto.createHash("sha256");
  for (const name of names) {
    hash.update(name);
    hash.update("\u0000");
    hash.update(fs.readFileSync(path.join(directory, name)));
    hash.update("\u0000");
  }
  return {
    path: TYPESCRIPT_LIB_DIRECTORY,
    default_libraries: names.length,
    sha256: hash.digest("hex"),
  };
}

function typescriptRecord() {
  return {
    version: ts.version,
    source_commit: SOURCE_COMMIT,
    bundle: pathHash(TYPESCRIPT_BUNDLE),
    implementation: pathHash(TYPESCRIPT_IMPLEMENTATION),
    lib: libraryInventoryRecord(),
  };
}

function writeFileAtomic(absolutePath, contents) {
  // Same-directory temp + rename: a kill mid-write can never truncate
  // the artifact, which doubles as the adoption store (gate-tax 2 R4-1).
  const temporary = path.join(
    path.dirname(absolutePath),
    `.${path.basename(absolutePath)}.${process.pid}.tmp`,
  );
  fs.writeFileSync(temporary, contents);
  fs.renameSync(temporary, absolutePath);
}

let adoptedCases = 0;

// Write-side observation adoption (gate-tax 2). The adoption key is
// all-or-nothing and deliberately stricter than the 5g per-case
// fallback: the stored generator sha must byte-match this file (the
// direct-control specs live here, and unlike 5g there is no per-gate
// --check backstop, only the once-per-slice packet checker), and the
// stored typescript record must byte-match the current one including
// the library inventory. Only the fresh-process oracle observations
// are adopted; the candidate/owner derivations, layer-evidence guards,
// and lineage pins re-execute against current upstream content on
// every write. --check never adopts: the packet checker's full
// re-observation remains the slice-boundary backstop.
function reusableStoredObservations(currentTypescriptRecord) {
  if (mode !== "--write") return null;
  const targetPath = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
  if (!fs.existsSync(targetPath)) return null;
  let stored;
  try {
    stored = JSON.parse(fs.readFileSync(targetPath, "utf8"));
  } catch {
    return null;
  }
  if (
    stored === null ||
    typeof stored !== "object" ||
    stored.schema !== 1 ||
    stored.kind !== "h2-dormant-semantic-foundation" ||
    !fingerprintIsValid(stored, "foundation_fingerprint_sha256") ||
    canonical(stored.generator) !==
      canonical(pathHash(GENERATOR_RELATIVE_PATH)) ||
    canonical(stored.typescript) !== canonical(currentTypescriptRecord)
  ) {
    return null;
  }
  const observations = new Map();
  for (const storedControl of stored.direct_controls ?? []) {
    if (
      typeof storedControl.control_id === "string" &&
      fingerprintIsValid(
        storedControl.observation,
        "observation_fingerprint_sha256",
      )
    ) {
      observations.set(storedControl.control_id, storedControl.observation);
    }
  }
  return observations;
}

function readBytes(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE, relativePath));
}

function readText(relativePath) {
  return readBytes(relativePath).toString("utf8");
}

function readJson(relativePath) {
  return JSON.parse(readText(relativePath));
}

function pathHash(relativePath) {
  return { path: relativePath, sha256: sha256(readBytes(relativePath)) };
}

function bytesRecord(text) {
  const bytes = Buffer.from(text, "utf8");
  return {
    utf8_base64: bytes.toString("base64"),
    utf8_sha256: sha256(bytes),
    utf8_bytes: bytes.length,
  };
}

function validateRuntime() {
  const node = readText(".node-version").trim();
  requireCondition(node === EXPECTED_NODE, "unexpected Node pin");
  requireCondition(process.version === `v${node}`, `requires Node ${node}`);
  requireCondition(ts.version === "6.0.3", "unexpected TypeScript runtime");
  for (const name of TRANSFORM_FLAG_NAMES) {
    requireCondition(
      typeof ts.TransformFlags?.[name] === "number",
      `pinned TypeScript does not expose TransformFlags.${name}`,
    );
  }
  for (const name of LOOP_CHECK_FLAG_NAMES) {
    requireCondition(
      typeof ts.NodeCheckFlags?.[name] === "number",
      `pinned TypeScript does not expose NodeCheckFlags.${name}`,
    );
  }
}

function serializeOptions(options) {
  return Object.fromEntries(
    Object.entries(options).sort(([left], [right]) => left.localeCompare(right)),
  );
}

function hasDirectory(files, directory) {
  const prefix = `${ts.normalizePath(directory).replace(/\/$/, "")}/`;
  return [...files.keys()].some((fileName) => fileName.startsWith(prefix));
}

function createVirtualProgram(control) {
  const files = new Map(control.files.map((file) => [file.path, file.text]));
  const baseHost = ts.createCompilerHost(control.compiler_options, true);
  const host = {
    ...baseHost,
    getCurrentDirectory: () => "/project",
    useCaseSensitiveFileNames: () => true,
    getCanonicalFileName: (fileName) => fileName,
    trace() {},
    fileExists(fileName) {
      const normalized = ts.normalizePath(fileName);
      return files.has(normalized) || baseHost.fileExists(normalized);
    },
    readFile(fileName) {
      const normalized = ts.normalizePath(fileName);
      return files.get(normalized) ?? baseHost.readFile(normalized);
    },
    directoryExists(directory) {
      return (
        hasDirectory(files, directory) ||
        (baseHost.directoryExists?.(directory) ?? false)
      );
    },
    getDirectories(directory) {
      return baseHost.getDirectories?.(directory) ?? [];
    },
    realpath(fileName) {
      const normalized = ts.normalizePath(fileName);
      return files.has(normalized)
        ? normalized
        : (baseHost.realpath?.(normalized) ?? normalized);
    },
    getSourceFile(fileName, languageVersion) {
      const normalized = ts.normalizePath(fileName);
      const text = files.get(normalized);
      if (text === undefined) {
        return baseHost.getSourceFile(fileName, languageVersion);
      }
      return ts.createSourceFile(
        normalized,
        text,
        languageVersion,
        true,
        ts.getScriptKindFromFileName(normalized),
      );
    },
  };
  return ts.createProgram(control.roots, control.compiler_options, host);
}

function collectNodes(root, predicate = () => true) {
  const result = [];
  function visit(node) {
    if (predicate(node)) result.push(node);
    ts.forEachChild(node, visit);
  }
  visit(root);
  return result;
}

function nodeText(node) {
  if (ts.isIdentifier(node) || ts.isPrivateIdentifier(node)) {
    return String(node.escapedText);
  }
  if (
    ts.isStringLiteralLike(node) ||
    ts.isNumericLiteral(node) ||
    ts.isBigIntLiteral(node)
  ) {
    return node.text;
  }
  return null;
}

function nodeSelector(node) {
  const sourceFile = node.getSourceFile();
  return {
    file: ts.normalizePath(sourceFile.fileName),
    kind: ts.SyntaxKind[node.kind],
    start: node.getStart(sourceFile, false),
    end: node.end,
    text: nodeText(node),
  };
}

function activeTransformFlags(node) {
  const flags = node.transformFlags ?? 0;
  return TRANSFORM_FLAG_NAMES.filter(
    (name) => (flags & ts.TransformFlags[name]) !== 0,
  );
}

function syntaxFacts(sourceFile) {
  return collectNodes(sourceFile).map((node) => ({
    subject: nodeSelector(node),
    transform_flags: node.transformFlags ?? 0,
    active_transform_flags: activeTransformFlags(node),
  }));
}

function isDeclarationName(node) {
  return node.parent !== undefined && node.parent.name === node;
}

function checkerBindings(sourceFile, checker, names) {
  const selected = new Set(names);
  return collectNodes(
    sourceFile,
    (node) => ts.isIdentifier(node) && selected.has(node.text),
  ).map((identifier) => {
    const symbol = checker.getSymbolAtLocation(identifier);
    return {
      subject: nodeSelector(identifier),
      symbol_flags: symbol?.flags ?? null,
      declarations: (symbol?.declarations ?? []).map(nodeSelector),
      value_declaration: symbol?.valueDeclaration
        ? nodeSelector(symbol.valueDeclaration)
        : null,
    };
  });
}

function booleanResolverQuery(method, subject, value, secondarySubject = null, argument = null) {
  return {
    method,
    subject: nodeSelector(subject),
    secondary_subject: secondarySubject ? nodeSelector(secondarySubject) : null,
    argument,
    result: {
      kind: "boolean",
      boolean: Boolean(value),
      declaration: null,
    },
  };
}

function declarationResolverQuery(method, subject, declaration, argument = null) {
  return {
    method,
    subject: nodeSelector(subject),
    secondary_subject: null,
    argument,
    result: {
      kind: "declaration",
      boolean: null,
      declaration: declaration ? nodeSelector(declaration) : null,
    },
  };
}

function collisionResolverQueries(sourceFile, resolver) {
  const declarations = collectNodes(
    sourceFile,
    (node) =>
      ts.isVariableDeclaration(node) &&
      ts.isIdentifier(node.name) &&
      node.name.text === "collisionValue",
  );
  const references = collectNodes(
    sourceFile,
    (node) =>
      ts.isIdentifier(node) &&
      node.text === "collisionValue" &&
      !isDeclarationName(node),
  );
  return [
    ...declarations.map((declaration) =>
      booleanResolverQuery(
        "isDeclarationWithCollidingName",
        declaration,
        resolver.isDeclarationWithCollidingName(declaration),
      ),
    ),
    ...references.map((reference) =>
      declarationResolverQuery(
        "getReferencedDeclarationWithCollidingName",
        reference,
        resolver.getReferencedDeclarationWithCollidingName(reference),
      ),
    ),
  ];
}

function capturedLoopResolverQueries(sourceFile, resolver) {
  const declaration = collectNodes(
    sourceFile,
    (node) =>
      ts.isVariableDeclaration(node) &&
      ts.isIdentifier(node.name) &&
      node.name.text === "capturedValue",
  )[0];
  const loop = collectNodes(sourceFile, ts.isForStatement)[0];
  requireCondition(declaration !== undefined, "captured binding declaration disappeared");
  requireCondition(loop !== undefined, "captured binding loop disappeared");
  const subjects = [
    loop,
    loop.initializer,
    loop.condition,
    loop.incrementor,
    loop.statement,
    declaration,
  ].filter((node) => node !== undefined);
  const flagQueries = subjects.flatMap((subject) =>
    LOOP_CHECK_FLAG_NAMES.map((name) =>
      booleanResolverQuery(
        "hasNodeCheckFlag",
        subject,
        resolver.hasNodeCheckFlag(subject, ts.NodeCheckFlags[name]),
        null,
        name,
      ),
    ),
  );
  const captureQueries = subjects.map((subject) =>
    booleanResolverQuery(
      "isBindingCapturedByNode",
      subject,
      resolver.isBindingCapturedByNode(subject, declaration),
      declaration,
    ),
  );
  return [...flagQueries, ...captureQueries];
}

function argumentsAndCatchResolverQueries(sourceFile, resolver) {
  const argumentIdentifiers = collectNodes(
    sourceFile,
    (node) => ts.isIdentifier(node) && node.text === "arguments",
  );
  const catchReferences = collectNodes(
    sourceFile,
    (node) =>
      ts.isIdentifier(node) &&
      node.text === "caughtValue" &&
      !isDeclarationName(node),
  );
  return [
    ...argumentIdentifiers.map((identifier) =>
      booleanResolverQuery(
        "isArgumentsLocalBinding",
        identifier,
        resolver.isArgumentsLocalBinding(identifier),
      ),
    ),
    ...catchReferences.map((reference) =>
      declarationResolverQuery(
        "getReferencedValueDeclaration",
        reference,
        resolver.getReferencedValueDeclaration(reference),
      ),
    ),
  ];
}

function serializeGeneratedIdentity(node) {
  const generated = node.emitNode?.autoGenerate;
  if (generated === undefined) return null;
  return {
    flags: generated.flags ?? 0,
    prefix: generated.prefix ?? null,
    suffix: generated.suffix ?? null,
    source: generated.node ? nodeSelector(generated.node) : null,
  };
}

function serializeFactoryNode(node) {
  const children = [];
  ts.forEachChild(node, (child) => children.push(serializeFactoryNode(child)));
  return {
    kind: ts.SyntaxKind[node.kind],
    text: nodeText(node),
    transform_flags: node.transformFlags ?? 0,
    emit_flags:
      typeof ts.getEmitFlags === "function"
        ? ts.getEmitFlags(node)
        : (node.emitNode?.flags ?? 0),
    generated: serializeGeneratedIdentity(node),
    children,
  };
}

function factoryNode(label, node) {
  return { label, tree: serializeFactoryNode(node) };
}

function generatedNameFactoryControl(sourceFile, context) {
  const factory = context.factory;
  requireCondition(
    typeof factory.createUniqueName === "function" &&
      typeof factory.getGeneratedNameForNode === "function",
    "pinned TypeScript factory generated-name surface changed",
  );
  const derived = collectNodes(
    sourceFile,
    (node) => ts.isClassDeclaration(node) && node.name?.text === "Derived",
  )[0];
  requireCondition(derived !== undefined, "factory control declaration disappeared");

  context.startLexicalEnvironment();
  const uniqueA = factory.createUniqueName("value");
  const uniqueB = factory.createUniqueName("value");
  const generatedA = factory.getGeneratedNameForNode(derived);
  const generatedB = factory.getGeneratedNameForNode(derived);
  context.hoistVariableDeclaration(uniqueA);
  context.hoistVariableDeclaration(uniqueB);
  const hoisted = context.endLexicalEnvironment() ?? [];
  const observationExpression = factory.createExpressionStatement(
    factory.createArrayLiteralExpression(
      [uniqueA, uniqueB, generatedA, generatedB],
      false,
    ),
  );
  const transformed = factory.updateSourceFile(sourceFile, [
    ...hoisted,
    ...sourceFile.statements,
    observationExpression,
  ]);
  return {
    transformed,
    nodes: [
      factoryNode("unique-name-a", uniqueA),
      factoryNode("unique-name-b", uniqueB),
      factoryNode("generated-name-a", generatedA),
      factoryNode("generated-name-b", generatedB),
      ...hoisted.map((statement, index) =>
        factoryNode(
          index === 0
            ? "hoisted-lexical-environment"
            : `hoisted-lexical-environment-${index + 1}`,
          statement,
        ),
      ),
    ],
    relations: [
      {
        relation: "generated-name-node-object-identity",
        left_label: "generated-name-a",
        right_label: "generated-name-b",
        result: generatedA === generatedB,
      },
      {
        relation: "generated-name-source-identity",
        left_label: "generated-name-a",
        right_label: "generated-name-b",
        result:
          generatedA.emitNode?.autoGenerate?.node ===
          generatedB.emitNode?.autoGenerate?.node,
      },
      {
        relation: "unique-name-node-object-identity",
        left_label: "unique-name-a",
        right_label: "unique-name-b",
        result: uniqueA === uniqueB,
      },
    ],
  };
}

function helperGraphFactoryControl(sourceFile, context) {
  const factory = context.factory;
  const helpers = context.getEmitHelperFactory();
  for (const method of [
    "createExtendsHelper",
    "createValuesHelper",
    "createReadHelper",
    "createSpreadArrayHelper",
    "createGeneratorHelper",
  ]) {
    requireCondition(
      typeof helpers[method] === "function",
      `pinned TypeScript helper factory does not expose ${method}`,
    );
  }
  const calls = [
    [
      "extends-helper",
      helpers.createExtendsHelper(factory.createIdentifier("Derived")),
    ],
    [
      "values-helper",
      helpers.createValuesHelper(factory.createIdentifier("iterable")),
    ],
    [
      "read-helper",
      helpers.createReadHelper(factory.createIdentifier("iterator"), 2),
    ],
    [
      "spread-array-helper",
      helpers.createSpreadArrayHelper(
        factory.createArrayLiteralExpression([], false),
        factory.createIdentifier("iterable"),
        true,
      ),
    ],
    [
      "generator-helper",
      helpers.createGeneratorHelper(factory.createIdentifier("generatorBody")),
    ],
  ];
  const transformed = factory.updateSourceFile(sourceFile, [
    ...sourceFile.statements,
    ...calls.map(([, call]) => factory.createExpressionStatement(call)),
  ]);
  return {
    transformed,
    nodes: calls.map(([label, call]) => factoryNode(label, call)),
    relations: [],
  };
}

function emptyDirectObservation() {
  return {
    syntax_nodes: [],
    checker_bindings: [],
    resolver_queries: [],
    factory_nodes: [],
    factory_relations: [],
  };
}

function collectDirectObservation(control, sourceFile, checker, context, resolver) {
  const direct = emptyDirectObservation();
  let transformed = sourceFile;
  switch (control.control_id) {
    case "syntax-transform-flag-matrix":
      direct.syntax_nodes = syntaxFacts(sourceFile);
      break;
    case "checker-colliding-block-scope":
      direct.checker_bindings = checkerBindings(sourceFile, checker, [
        "collisionValue",
      ]);
      direct.resolver_queries = collisionResolverQueries(sourceFile, resolver);
      break;
    case "checker-captured-loop-bindings":
      direct.checker_bindings = checkerBindings(sourceFile, checker, [
        "capturedValue",
      ]);
      direct.resolver_queries = capturedLoopResolverQueries(sourceFile, resolver);
      break;
    case "checker-arguments-and-catch-reference":
      direct.checker_bindings = checkerBindings(sourceFile, checker, [
        "arguments",
        "caughtValue",
      ]);
      direct.resolver_queries = argumentsAndCatchResolverQueries(
        sourceFile,
        resolver,
      );
      break;
    case "factory-generated-name-lexical-environment": {
      const factory = generatedNameFactoryControl(sourceFile, context);
      direct.factory_nodes = factory.nodes;
      direct.factory_relations = factory.relations;
      transformed = factory.transformed;
      break;
    }
    case "factory-es2015-generator-helper-graph": {
      const factory = helperGraphFactoryControl(sourceFile, context);
      direct.factory_nodes = factory.nodes;
      direct.factory_relations = factory.relations;
      transformed = factory.transformed;
      break;
    }
    default:
      fail(`unknown direct control ${control.control_id}`);
  }
  return { direct, transformed };
}

function serializeDiagnostic(diagnostic, phase) {
  return {
    phase,
    code: diagnostic.code,
    category: ts.DiagnosticCategory[diagnostic.category],
    file: diagnostic.file ? ts.normalizePath(diagnostic.file.fileName) : null,
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
  };
}

function serializeWrite(arguments_, index) {
  const [fileName, text, writeByteOrderMark, onError, sourceFiles] = arguments_;
  const callback = Buffer.from(text, "utf8");
  const materialized = writeByteOrderMark
    ? Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), callback])
    : callback;
  return {
    index,
    path: ts.normalizePath(fileName),
    callback_utf8_base64: callback.toString("base64"),
    callback_utf8_sha256: sha256(callback),
    callback_utf8_bytes: callback.length,
    write_byte_order_mark: writeByteOrderMark,
    materialized_utf8_sha256: sha256(materialized),
    materialized_utf8_bytes: materialized.length,
    on_error_callback_present: typeof onError === "function",
    source_files: (sourceFiles ?? []).map((source) =>
      ts.normalizePath(source.fileName),
    ),
  };
}

function observeDirectControl(control) {
  const program = createVirtualProgram(control);
  const checker = program.getTypeChecker();
  const reportedDiagnostics = ts.getPreEmitDiagnostics(program);
  const writes = [];
  let direct = null;
  const before = (context) => {
    requireCondition(
      typeof context.getEmitResolver === "function" &&
        typeof context.getEmitHelperFactory === "function",
      "pinned TypeScript transformation context surface changed",
    );
    const resolver = context.getEmitResolver();
    for (const method of [
      "getReferencedDeclarationWithCollidingName",
      "isDeclarationWithCollidingName",
      "hasNodeCheckFlag",
      "getReferencedValueDeclaration",
      "isArgumentsLocalBinding",
      "isBindingCapturedByNode",
    ]) {
      requireCondition(
        typeof resolver[method] === "function",
        `pinned TypeScript emit resolver does not expose ${method}`,
      );
    }
    return (sourceFile) => {
      if (!control.roots.includes(sourceFile.fileName)) return sourceFile;
      requireCondition(direct === null, `${control.control_id} visited twice`);
      const observation = collectDirectObservation(
        control,
        sourceFile,
        checker,
        context,
        resolver,
      );
      direct = observation.direct;
      return observation.transformed;
    };
  };
  const emitResult = program.emit(
    undefined,
    (...arguments_) => writes.push(arguments_),
    undefined,
    false,
    { before: [before] },
  );
  requireCondition(direct !== null, `${control.control_id} was not transformed`);
  requireCondition(writes.length !== 0, `${control.control_id} produced no oracle write`);
  return withFingerprint(
    {
      ...direct,
      reported_diagnostics: reportedDiagnostics.map((diagnostic) =>
        serializeDiagnostic(diagnostic, "pre-emit"),
      ),
      emit_diagnostics: emitResult.diagnostics.map((diagnostic) =>
        serializeDiagnostic(diagnostic, "emit"),
      ),
      emit_skipped: emitResult.emitSkipped,
      writes: writes.map(serializeWrite),
    },
    "observation_fingerprint_sha256",
  );
}

function observeDirectControlInFreshProcess(control) {
  const stdout = execFileSync(
    process.execPath,
    [GENERATOR_PATH, INTERNAL_OBSERVE_MODE, control.control_id],
    {
      cwd: WORKSPACE,
      encoding: "utf8",
      maxBuffer: 128 * 1024 * 1024,
    },
  );
  const observation = JSON.parse(stdout);
  requireCondition(
    observation !== null &&
      typeof observation === "object" &&
      typeof observation.observation_fingerprint_sha256 === "string",
    `${control.control_id} fresh TypeScript observation is invalid`,
  );
  return observation;
}

function buildDirectControl(control, adoptedObservations) {
  // TypeScript's generated-name allocator has process-global state. Each
  // repetition therefore gets a fresh Node isolate, just like an independent
  // oracle worker, while the complete observation remains byte-accounted.
  const adopted = adoptedObservations?.get(control.control_id) ?? null;
  let first;
  if (adopted !== null) {
    // Adoption skips only the two fresh-process oracle runs; the stored
    // observation fingerprint stands for the repetitions=2 determinism
    // proof, exactly as the 5g reuse does. The layer-evidence guards
    // below still execute against the adopted record.
    adoptedCases += 1;
    first = adopted;
  } else {
    first = observeDirectControlInFreshProcess(control);
    const second = observeDirectControlInFreshProcess(control);
    requireCondition(
      first.observation_fingerprint_sha256 ===
        second.observation_fingerprint_sha256,
      `${control.control_id} TypeScript observation is nondeterministic`,
    );
  }
  for (const layer of control.layers) {
    const populated =
      layer === "syntax"
        ? first.syntax_nodes.length
        : layer === "checker"
          ? first.checker_bindings.length
          : layer === "resolver"
            ? first.resolver_queries.length
            : layer === "factory"
              ? first.factory_nodes.length
              : 0;
    requireCondition(populated !== 0, `${control.control_id} ${layer} evidence is empty`);
  }
  return withFingerprint(
    {
      control_id: control.control_id,
      layers: control.layers,
      input: {
        current_directory: "/project",
        roots: control.roots,
        files: control.files.map((file) => ({
          path: file.path,
          root: control.roots.includes(file.path),
          ...bytesRecord(file.text),
        })),
        compiler_options: serializeOptions(control.compiler_options),
      },
      repetitions: 2,
      observation: first,
    },
    "control_fingerprint_sha256",
  );
}

function memberFingerprint(cases) {
  return sha256(
    Buffer.from(
      canonical(
        cases.map((item) => ({
          id: item.id,
          required_slices: item.required_slices,
        })),
      ),
      "utf8",
    ),
  );
}

function countBy(items, key) {
  const counts = new Map();
  for (const item of items) {
    const value = item[key];
    counts.set(value, (counts.get(value) ?? 0) + 1);
  }
  return counts;
}

function buildCandidateFoundation(dispositions) {
  requireCondition(
    dispositions.schema === 1 &&
      dispositions.phase === "H2.0a-runner-candidate-dispositions" &&
      dispositions.status === "frozen" &&
      dispositions.typescript.version === ts.version &&
      dispositions.typescript.source_commit === SOURCE_COMMIT &&
      Array.isArray(dispositions.cases) &&
      dispositions.cases.length === 15_642,
    "H2 candidate disposition authority is not closed",
  );
  const allIds = new Set(dispositions.cases.map((item) => item.id));
  requireCondition(
    allIds.size === dispositions.cases.length,
    "H2 candidate disposition ids are not unique",
  );
  const closed = new Set(CLOSED_THROUGH_H2_5G);
  const global = dispositions.cases.filter((item) =>
    item.required_slices.includes(SELECTED_SLICE),
  );
  const candidates = global.filter((item) =>
    item.required_slices.every(
      (slice) => slice === SELECTED_SLICE || closed.has(slice),
    ),
  );
  const candidateIds = new Set(candidates.map((item) => item.id));
  const future = global.filter((item) => !candidateIds.has(item.id));
  requireCondition(global.length === 2_012, "H2.5h global census changed");
  requireCondition(candidates.length === 932, "H2.5h candidate census changed");
  requireCondition(future.length === 1_080, "H2.5h future census changed");
  requireCondition(
    candidates.every(
      (item) =>
        item.execution_state === "not-run" &&
        item.required_slices.includes(SELECTED_SLICE),
    ),
    "H2.5h foundation contains an executed or unowned candidate",
  );
  const suiteCounts = countBy(candidates, "suite");
  requireCondition(
    suiteCounts.size === 3 &&
      suiteCounts.get("compiler") === 231 &&
      suiteCounts.get("conformance") === 619 &&
      suiteCounts.get("project") === 82,
    "H2.5h candidate suite partition changed",
  );
  const rows = candidates.map((item, index) =>
    withFingerprint(
      {
        index,
        suite: item.suite,
        upstream_case: item.upstream_case,
        id: item.id,
        source: item.source,
        required_slices: item.required_slices,
        foundation_route:
          item.suite === "project"
            ? "project-foundation-only"
            : "runner-executable",
        execution_state: "not-run",
        runtime_admission: false,
      },
      "candidate_fingerprint_sha256",
    ),
  );
  const runner = rows.filter(
    (item) => item.foundation_route === "runner-executable",
  );
  const projects = rows.filter(
    (item) => item.foundation_route === "project-foundation-only",
  );
  requireCondition(
    runner.length === 850 && projects.length === 82,
    "H2.5h execution capability partition changed",
  );
  return {
    rows,
    selection_contract: {
      selected_slice: SELECTED_SLICE,
      foundation_slice_id: FOUNDATION_SLICE,
      runtime_activation_slice_id: RUNTIME_ACTIVATION_SLICE,
      closed_through_slice: "H2.5g",
      closed_runtime_slices: CLOSED_THROUGH_H2_5G,
      selection_authority: "required_slices-only",
      candidate_rule:
        "required_slices contains H2.5h and every other required slice is closed through H2.5g",
      global_h2_5h_rows: global.length,
      candidate_denominator: candidates.length,
      runner_executable_cases: runner.length,
      project_foundation_cases: projects.length,
      future_deferred_rows: future.length,
      suite_partition: [
        { suite: "compiler", cases: suiteCounts.get("compiler") },
        { suite: "conformance", cases: suiteCounts.get("conformance") },
        { suite: "project", cases: suiteCounts.get("project") },
      ],
      global_membership_sha256: memberFingerprint(global),
      candidate_membership_sha256: memberFingerprint(candidates),
      future_membership_sha256: memberFingerprint(future),
    },
  };
}

function positionAt(text, offset) {
  const prefix = text.slice(0, offset);
  const lastNewline = prefix.lastIndexOf("\n");
  return {
    offset,
    line: prefix.split("\n").length,
    character: offset - lastNewline,
  };
}

function ledgerSliceHash(text, startLine, endLine) {
  return sha256(
    text
      .split(/(?<=\n)/u)
      .slice(startLine - 1, endLine)
      .join(""),
  );
}

function validateOwnerDeclaration(declaration, implementationText) {
  const { start, end } = declaration.source_range;
  requireCondition(
    declaration.source === TYPESCRIPT_IMPLEMENTATION &&
      canonical(positionAt(implementationText, start.offset)) ===
        canonical(start) &&
      canonical(positionAt(implementationText, end.offset)) === canonical(end),
    `${declaration.name} source range changed`,
  );
  const declarationText = implementationText.slice(start.offset, end.offset);
  requireCondition(
    declarationText.startsWith(`function ${declaration.name}(`) &&
      sha256(declarationText) === declaration.declaration_sha256 &&
      ledgerSliceHash(implementationText, start.line, end.line) ===
        declaration.ledger_slice_sha256,
    `${declaration.name} pinned declaration bytes changed`,
  );
}

function ownerRegistrationEvidence(implementationText) {
  const pattern =
    /if \(languageVersion < 2 \/\* ES2015 \*\/\) \{\s+transformers\.push\(transformES2015\);\s+transformers\.push\(transformGenerators\);\s+\}/gu;
  const matches = [...implementationText.matchAll(pattern)];
  requireCondition(
    matches.length === 1,
    "H2.5h joint transformer registration changed",
  );
  const match = matches[0];
  const startOffset = match.index;
  const endOffset = startOffset + match[0].length;
  return {
    source: TYPESCRIPT_IMPLEMENTATION,
    activation_test: "languageVersion < ES2015(2)",
    ordered_transformers: ["transformES2015", "transformGenerators"],
    source_range: {
      start: positionAt(implementationText, startOffset),
      end: positionAt(implementationText, endOffset),
    },
    registration_sha256: sha256(match[0]),
  };
}

function buildOwnerFoundation(ownerInventory) {
  const compiler = pathHash(TYPESCRIPT_IMPLEMENTATION);
  const library = pathHash(TYPESCRIPT_BUNDLE);
  requireCondition(
    ownerInventory.schema === 1 &&
      ownerInventory.phase === "H2.0a-owner-converse-inventory" &&
      ownerInventory.status === "frozen" &&
      ownerInventory.typescript.version === ts.version &&
      ownerInventory.typescript.source_commit === SOURCE_COMMIT &&
      canonical(ownerInventory.typescript.compiler) === canonical(compiler) &&
      canonical(ownerInventory.typescript.library) === canonical(library) &&
      Array.isArray(ownerInventory.owners),
    "H2 owner inventory authority is not closed",
  );
  const selected = ownerInventory.owners.filter(
    (owner) => owner.owner_slice === SELECTED_SLICE,
  );
  requireCondition(
    canonical(selected.map((owner) => owner.key)) === canonical(OWNER_KEYS) &&
      selected.every(
        (owner) =>
          owner.disposition === "deferred-h2" &&
          owner.activation === "target below ES2015",
      ),
    "H2.5h owner closure changed",
  );
  const implementationText = readText(TYPESCRIPT_IMPLEMENTATION);
  for (const owner of selected) {
    validateOwnerDeclaration(owner.declaration, implementationText);
  }
  requireCondition(
    selected[0].declaration.source_range.start.offset <
      selected[1].declaration.source_range.start.offset,
    "H2.5h owner source order changed",
  );
  return {
    activation_contract: {
      foundation_slice_id: FOUNDATION_SLICE,
      runtime_activation_slice_id: RUNTIME_ACTIVATION_SLICE,
      activation: selected[0].activation,
      activation_mode: "joint",
      ordered_owner_keys: selected.map((owner) => owner.key),
      order_basis: "pinned-typescript-source-order",
      production_registration: "dormant",
      upstream_registration: ownerRegistrationEvidence(implementationText),
    },
    owners: selected.map((owner, index) => ({
      index,
      key: owner.key,
      role: owner.role,
      owner_slice: owner.owner_slice,
      activation: owner.activation,
      disposition_before_foundation: owner.disposition,
      runtime_state: "dormant",
      declaration: owner.declaration,
    })),
  };
}

function loadParentProfile(requireParent) {
  const absolutePath = path.join(WORKSPACE, PARENT_PROFILE_RELATIVE_PATH);
  if (!fs.existsSync(absolutePath)) {
    requireCondition(
      !requireParent,
      `H2.5h-a final mode requires closed ${PARENT_PROFILE_RELATIVE_PATH}`,
    );
    return null;
  }
  const parent = readJson(PARENT_PROFILE_RELATIVE_PATH);
  requireCondition(
    parent.schema === 1 &&
      parent.kind === "h2-runtime-profile" &&
      parent.status === "qualified" &&
      parent.phase === "H2.5g" &&
      parent.transition.completed_slice === "H2.5g" &&
      parent.transition.next_slice === FOUNDATION_SLICE &&
      parent.transition.next_slice_scope ===
        "architecture-validation-owner-local-gap-rust-design-and-oracle-fixture-freeze" &&
      parent.transition.next_runtime_activation_slice ===
        "determined-by-H2.5h-a-owner-graph" &&
      canonical(parent.transition.active_runtime_slices) ===
        canonical(CLOSED_THROUGH_H2_5G) &&
      parent.transition.target_es2015_transform_owner ===
        RUNTIME_ACTIVATION_SLICE &&
      parent.transition.target_generators_transform_owner ===
        RUNTIME_ACTIVATION_SLICE &&
      parent.summary.completed_runtime_slices === 22 &&
      parent.summary.next_slice_runtime_slice_delta === 0 &&
      // 9,191 at the H2.5g candidate plus the five reviewed H2.1a exact
      // promotions recorded in the closed profile's
      // current_exact_promotions (arrayFromAsync, arrayIterationLibES5
      // TargetDifferent, mapGroupBy, objectGroupBy,
      // regularExpressionScanning).
      parent.summary.runtime_admissions === 9_196 &&
      parent.summary.executed_candidates === 9_715 &&
      parent.summary.unexecuted_candidates === 0 &&
      parent.summary.undispositioned_candidates === 0,
    "H2.5g parent profile is not closed",
  );
  return parent;
}

function buildControlCoverage(controls) {
  const layers = [...new Set(controls.flatMap((control) => control.layers))].sort();
  const resolverMethods = [
    ...new Set(
      controls.flatMap((control) =>
        control.observation.resolver_queries.map((query) => query.method),
      ),
    ),
  ].sort();
  const factoryOperations = [
    ...new Set(
      controls.flatMap((control) =>
        control.observation.factory_nodes.map((node) => node.label),
      ),
    ),
  ].sort();
  requireCondition(
    canonical(layers) ===
      canonical(["checker", "factory", "resolver", "syntax"]) &&
      canonical(resolverMethods) ===
        canonical([
          "getReferencedDeclarationWithCollidingName",
          "getReferencedValueDeclaration",
          "hasNodeCheckFlag",
          "isArgumentsLocalBinding",
          "isBindingCapturedByNode",
          "isDeclarationWithCollidingName",
        ]) &&
      factoryOperations.includes("extends-helper") &&
      factoryOperations.includes("generated-name-a") &&
      factoryOperations.includes("generator-helper") &&
      factoryOperations.includes("read-helper") &&
      factoryOperations.includes("spread-array-helper") &&
      factoryOperations.includes("values-helper"),
    "H2.5h-a direct control coverage changed",
  );
  return {
    layers,
    resolver_methods: resolverMethods,
    factory_operations: factoryOperations,
  };
}

function buildArtifact(parentProfile) {
  validateRuntime();
  const dispositions = readJson(DISPOSITIONS_RELATIVE_PATH);
  const ownerInventory = readJson(OWNER_INVENTORY_RELATIVE_PATH);
  const candidates = buildCandidateFoundation(dispositions);
  const owner = buildOwnerFoundation(ownerInventory);
  const typescript = typescriptRecord();
  const adoptedObservations = reusableStoredObservations(typescript);
  const directControls = DIRECT_CONTROL_SPECS.map((control) =>
    buildDirectControl(control, adoptedObservations),
  );
  const coverage = buildControlCoverage(directControls);
  const frozen = parentProfile !== null;
  const parentAdmissions = parentProfile?.summary.runtime_admissions ?? null;
  const parentExecuted = parentProfile?.summary.executed_candidates ?? null;
  return withFingerprint(
    {
      schema: 1,
      kind: "h2-dormant-semantic-foundation",
      status: frozen
        ? "frozen-dormant-semantic-foundation"
        : "pre-freeze-parent-profile-absent",
      phase: FOUNDATION_SLICE,
      slice_id: FOUNDATION_SLICE,
      typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      lineage: {
        state: frozen
          ? "closed-h2-5g-parent"
          : "pre-freeze-h2-5g-parent-absent",
        parent_phase: "H2.5g",
        parent_profile: frozen ? pathHash(PARENT_PROFILE_RELATIVE_PATH) : null,
        interpretation:
          "H2.5h-a freezes candidate identity and semantic controls without activating either H2.5h production transformer",
      },
      inputs: {
        candidate_dispositions: pathHash(DISPOSITIONS_RELATIVE_PATH),
        owner_inventory: pathHash(OWNER_INVENTORY_RELATIVE_PATH),
      },
      selection_contract: candidates.selection_contract,
      owner_activation_contract: owner.activation_contract,
      owner_closure: owner.owners,
      runtime_contract: {
        foundation_slice_id: FOUNDATION_SLICE,
        runtime_activation_slice_id: RUNTIME_ACTIVATION_SLICE,
        production_state: "dormant",
        transformer_registration: "not-registered",
        active_runtime_slices: CLOSED_THROUGH_H2_5G,
        h2_5h_runtime_active: false,
        h2_5h_activity: 0,
        candidate_execution_state: "not-run",
        candidate_typescript_runs: 0,
        rust_runs: 0,
        parent_completed_runtime_slices:
          parentProfile?.summary.completed_runtime_slices ?? null,
        runtime_admissions_before: parentAdmissions,
        runtime_admissions_after: parentAdmissions,
        runtime_admissions_delta: 0,
        executed_candidates_before: parentExecuted,
        executed_candidates_after: parentExecuted,
      },
      direct_controls: directControls,
      control_coverage: coverage,
      candidates: candidates.rows,
      summary: {
        global_h2_5h_rows:
          candidates.selection_contract.global_h2_5h_rows,
        candidates: candidates.rows.length,
        compiler_candidates: candidates.rows.filter(
          (item) => item.suite === "compiler",
        ).length,
        conformance_candidates: candidates.rows.filter(
          (item) => item.suite === "conformance",
        ).length,
        project_candidates: candidates.rows.filter(
          (item) => item.suite === "project",
        ).length,
        runner_executable_cases: candidates.rows.filter(
          (item) => item.foundation_route === "runner-executable",
        ).length,
        project_foundation_cases: candidates.rows.filter(
          (item) => item.foundation_route === "project-foundation-only",
        ).length,
        future_deferred_rows:
          candidates.selection_contract.future_deferred_rows,
        owner_roots: owner.owners.length,
        direct_controls: directControls.length,
        typescript_oracle_runs: directControls.reduce(
          (sum, control) => sum + control.repetitions,
          0,
        ),
        candidate_typescript_runs: 0,
        rust_runs: 0,
        runtime_admissions_delta: 0,
        h2_5h_activity: 0,
        unexecuted_candidates: candidates.rows.filter(
          (item) => item.execution_state === "not-run",
        ).length,
        undispositioned_candidates: 0,
      },
    },
    "foundation_fingerprint_sha256",
  );
}

function render(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

const mode = process.argv[2];
if (mode === INTERNAL_OBSERVE_MODE) {
  requireCondition(
    process.argv.length === 4,
    "internal direct observation requires one control id",
  );
  validateRuntime();
  const control = DIRECT_CONTROL_SPECS.find(
    (item) => item.control_id === process.argv[3],
  );
  requireCondition(
    control !== undefined,
    `unknown internal direct control ${process.argv[3]}`,
  );
  process.stdout.write(render(observeDirectControl(control)));
} else {
  requireCondition(
    mode === undefined || mode === "--write" || mode === "--check",
    "usage: h2-5h-a-foundation.mjs [--write|--check]",
  );
  const finalMode = mode === "--write" || mode === "--check";
  const parentProfile = loadParentProfile(finalMode);
  const artifact = buildArtifact(parentProfile);
  const rendered = render(artifact);
  if (mode === "--write") {
    writeFileAtomic(path.join(WORKSPACE, TARGET_RELATIVE_PATH), rendered);
    process.stdout.write(
      `wrote ${TARGET_RELATIVE_PATH}: candidates=${artifact.summary.candidates} runtime_delta=${artifact.summary.runtime_admissions_delta} adopted_controls=${adoptedCases} oracle_runs_saved=${adoptedCases * 2}\n`,
    );
  } else if (mode === "--check") {
    requireCondition(
      fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
        fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") ===
          rendered,
      `stale ${TARGET_RELATIVE_PATH}; run h2-5h-a-foundation.mjs --write and review`,
    );
    process.stdout.write(
      `H2.5h-a foundation is fresh: candidates=${artifact.summary.candidates} runtime_delta=${artifact.summary.runtime_admissions_delta}\n`,
    );
  } else {
    process.stdout.write(rendered);
  }
}
