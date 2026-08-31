// H2.7a m-1: declaration-owner inventory over the pinned TypeScript bundle.
//
// The signed packet fixes the syntax grammar used here: a function row is a
// FunctionDeclaration, while an owner row additionally represents its body.
// All other inventories (resolver use sites, factory/parenthesizer members,
// reached helpers, and option owners) are derived from the same parsed inputs.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

// A name belongs here only while its ownership cannot be defended from the
// source. Acceptance is fail-closed, so the delivered inventory keeps this
// list empty after every reached helper has been reviewed.
const UNRESOLVED_CANDIDATES = Object.freeze([]);

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const MODE = process.argv[2];

const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-7a-owner-inventory.mjs";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-7a-owner-inventory.schema.json";
const TARGET_RELATIVE_PATH = "ratchets/h2-7a-owner-inventory.v1.json";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const OPTION_OWNER_SOURCE = "crates/oracle/h2-transition.mjs";
const RUST_PRINTER_SOURCE = "crates/emitter/src/printer.rs";
const RUST_WRITER_SOURCE = "crates/emitter/src/writer.rs";
const RUST_FACTORY_SOURCE = "crates/emitter/src/factory.rs";
const RUST_RESOLVER_SOURCE = "crates/emitter/src/resolver.rs";

const EXPECTED_TYPESCRIPT_SHA256 =
  "1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3";
const EXPECTED_OPTION_OWNER_SPAN_SHA256 =
  "25632b51bf9ea161a1b472e97f66b8d46f8b9e92b980e4effd4c0b1472d6cdd2";

const EXPECTED_RESOLVER_CALLS = Object.freeze({
  createLateBoundIndexSignatures: 1,
  createLiteralConstValue: 1,
  createReturnTypeOfSignatureDeclaration: 1,
  createTypeOfDeclaration: 2,
  createTypeOfExpression: 1,
  getDeclarationStatementsForSourceFile: 1,
  getEnumMemberValue: 1,
  getPropertiesOfContainerFunction: 2,
  isDeclarationVisible: 6,
  isDefinitelyReferenceToGlobalSymbolObject: 1,
  isEntityNameVisible: 1,
  isExpandoFunctionDeclaration: 2,
  isImplementationOfOverload: 2,
  isImportRequiredByAugmentation: 1,
  isLateBound: 1,
  isLiteralConstDeclaration: 1,
  isOptionalParameter: 1,
  isSymbolAccessible: 1,
  requiresAddingImplicitUndefined: 1,
});

const NODE_BUILDER_DEPENDENT_MEMBERS = new Set([
  "createLateBoundIndexSignatures",
  "createLiteralConstValue",
  "createReturnTypeOfSignatureDeclaration",
  "createTypeOfDeclaration",
  "createTypeOfExpression",
  "getDeclarationStatementsForSourceFile",
  "symbolToDeclarations",
]);

// getEnumMemberValue is already one of Rust's 21 script-era EmitResolver
// methods. It remains in the measured 19-member declarations-module census,
// but it is not new work in the m-2/m-3-head amendment partition.
const EXISTING_RESOLVER_MEMBERS = new Set(["getEnumMemberValue"]);

const RESOLVER_MEMBER_SPECS = Object.freeze([
  ["isDeclarationVisible", 55589, "function"],
  ["createTypeOfDeclaration", 88359, "function"],
  ["isExpandoFunctionDeclaration", 88090, "function"],
  ["isImplementationOfOverload", 88055, "function"],
  ["getPropertiesOfContainerFunction", 88113, "function"],
  ["createTypeOfExpression", 88389, "function"],
  ["createReturnTypeOfSignatureDeclaration", 88382, "function"],
  ["createLiteralConstValue", 88506, "function"],
  ["createLateBoundIndexSignatures", 88624, "property"],
  ["getDeclarationStatementsForSourceFile", 88612, "property"],
  ["getEnumMemberValue", 88231, "function"],
  ["isDefinitelyReferenceToGlobalSymbolObject", 47469, "function"],
  ["isEntityNameVisible", 50606, "function"],
  ["isImportRequiredByAugmentation", 88696, "function"],
  ["isLateBound", 88600, "property"],
  ["isLiteralConstDeclaration", 88485, "function"],
  ["isOptionalParameter", 59509, "function"],
  ["isSymbolAccessible", 50499, "function"],
  ["requiresAddingImplicitUndefined", 88075, "function"],
]);

const PRINTER_SEED_NAMES = Object.freeze([
  "emitTypeParameter",
  "emitParameter",
  "emitDecorator",
  "emitPropertySignature",
  "emitPropertyDeclaration",
  "emitMethodSignature",
  "emitMethodDeclaration",
  "emitConstructor",
  "emitAccessorDeclaration",
  "emitCallSignature",
  "emitConstructSignature",
  "emitIndexSignature",
  "emitTemplateTypeSpan",
  "emitTypePredicate",
  "emitTypeReference",
  "emitFunctionType",
  "emitFunctionTypeHead",
  "emitFunctionTypeBody",
  "emitJSDocFunctionType",
  "emitJSDocNullableType",
  "emitJSDocNonNullableType",
  "emitJSDocOptionalType",
  "emitConstructorType",
  "emitTypeQuery",
  "emitTypeLiteral",
  "emitArrayType",
  "emitRestOrJSDocVariadicType",
  "emitTupleType",
  "emitNamedTupleMember",
  "emitOptionalType",
  "emitUnionType",
  "emitIntersectionType",
  "emitConditionalType",
  "emitInferType",
  "emitParenthesizedType",
  "emitThisType",
  "emitTypeOperator",
  "emitIndexedAccessType",
  "emitMappedType",
  "emitLiteralType",
  "emitTemplateType",
  "emitImportTypeNode",
  "emitFunctionDeclaration",
  "emitFunctionDeclarationOrExpression",
  "emitSignatureAndBody",
  "emitSignatureHead",
  "emitClassDeclaration",
  "emitClassDeclarationOrExpression",
  "emitInterfaceDeclaration",
  "emitTypeAliasDeclaration",
  "emitEnumDeclaration",
  "emitModuleDeclaration",
  "emitModuleBlock",
  "emitHeritageClause",
  "emitTypeAnnotation",
  "emitTypeArguments",
  "emitTypeParameters",
  "emitParameters",
]);

const PRINTER_RUST_ANCHORS = Object.freeze({
  createPrinter: "crates/emitter/src/printer.rs:903",
  emitCaseBlock: "crates/emitter/src/printer.rs:6169",
  emitDecorator: "crates/emitter/src/printer.rs:8349",
  emitDecoratorsAndModifiers: "crates/emitter/src/printer.rs:8349",
  emitHelpers: "crates/emitter/src/printer.rs:6152",
  emitList: "crates/emitter/src/printer.rs:8646",
  emitParameters: "crates/emitter/src/printer.rs:7589",
  emitSignatureHead: "crates/emitter/src/printer.rs:7546",
  emitTypeArguments: "crates/emitter/src/printer.rs:7513",
  printFile: "crates/emitter/src/printer.rs:931",
  writeLineOrSpace: "crates/emitter/src/printer.rs:7305",
});

// Curated ownership overrides for one-hop top-level helpers reached directly
// from the declarations module, NodeBuilder, or syntactic builder. Any helper
// not named here is deliberately classified by the packet-authorized
// shared/core-utility fallback below, never as unknown.
const REACHED_DISPOSITIONS = Object.freeze({
  addRelatedInfo: ["shared/diagnostics", null],
  addSyntheticLeadingComment: ["shared/emit-factory", null],
  addSyntheticTrailingComment: ["shared/emit-factory", null],
  addEmitFlags: ["shared/emit-factory", null],
  addEmitHelper: ["shared/emit-factory", null],
  canProduceDiagnostics: ["shared/diagnostics", null],
  countPathComponents: ["shared/path", null],
  createDiagnosticForNode: ["shared/diagnostics", null],
  createDiagnosticForNodeFromMessageChain: ["shared/diagnostics", null],
  createFileDiagnostic: ["shared/diagnostics", null],
  createGetIsolatedDeclarationErrors: ["shared/diagnostics", null],
  createGetSymbolAccessibilityDiagnosticForNode: ["shared/diagnostics", null],
  createGetSymbolAccessibilityDiagnosticForNodeName: ["shared/diagnostics", null],
  createModuleNotFoundChain: ["shared/diagnostics", null],
  createNodeArray: ["shared/emit-factory", null],
  getCommentRange: ["shared/emit-factory", null],
  getBaseFileName: ["shared/path", null],
  getDirectoryPath: ["shared/path", null],
  getNormalizedAbsolutePath: ["shared/path", null],
  getOutputPathsFor: ["shared/path", "H2.7b"],
  getRelativePathFromDirectory: ["shared/path", null],
  getRelativePathToDirectoryOrUrl: ["shared/path", null],
  isRootedDiskPath: ["shared/path", null],
  normalizePath: ["shared/path", null],
  normalizeSlashes: ["shared/path", null],
  pathIsRelative: ["shared/path", null],
  removeAllComments: ["shared/emit-factory", null],
  setCommentRange: ["shared/emit-factory", null],
  setEmitFlags: ["shared/emit-factory", null],
  setOriginalNode: ["shared/emit-factory", null],
  setParent: ["shared/emit-factory", null],
  setSyntheticLeadingComments: ["shared/emit-factory", null],
  setTextRange: ["shared/emit-factory", null],
  setTextRangePosEnd: ["shared/emit-factory", null],
  toPath: ["shared/path", null],
  tryGetModuleSpecifierFromDeclaration: ["shared/path", null],
  append: ["shared/core", null],
  arrayFrom: ["shared/core", null],
  contains: ["shared/core", null],
  every: ["shared/core", null],
  filter: ["shared/core", null],
  find: ["shared/core", null],
  firstDefined: ["shared/core", null],
  flatMap: ["shared/core", null],
  forEach: ["shared/core", null],
  map: ["shared/core", null],
  mapDefined: ["shared/core", null],
  some: ["shared/core", null],
});

function fail(message) {
  throw new Error(message);
}

function requireCondition(condition, message) {
  if (!condition) fail(message);
}

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function compareStrings(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
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

function render(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

function readBytes(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE, relativePath));
}

function pathHash(relativePath) {
  return { path: relativePath, sha256: sha256(readBytes(relativePath)) };
}

function camelToSnake(name) {
  return name
    .replace(/([a-z0-9])([A-Z])/gu, "$1_$2")
    .replace(/([A-Z])([A-Z][a-z])/gu, "$1_$2")
    .toLowerCase();
}

function rustFunctionAnchors(relativePath) {
  const lines = splitLines(readBytes(relativePath).toString("utf8"));
  const anchors = new Map();
  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(
      /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?fn\s+([A-Za-z0-9_]+)\s*(?:<[^>]*>)?\s*\(/u,
    );
    if (match && !anchors.has(match[1])) {
      anchors.set(match[1], `${relativePath}:${index + 1}`);
    }
  }
  return anchors;
}

function splitLines(text) {
  requireCondition(!text.includes("\r"), "inventory inputs must use LF newlines");
  return text.match(/[^\n]*\n|[^\n]+$/gu) ?? [];
}

function lineSpanText(lines, startLine, endLine) {
  requireCondition(
    Number.isInteger(startLine) &&
      Number.isInteger(endLine) &&
      startLine >= 1 &&
      endLine >= startLine &&
      endLine <= lines.length,
    `invalid line span ${startLine}-${endLine}`,
  );
  return lines.slice(startLine - 1, endLine).join("");
}

const sourceBytes = readBytes(TYPESCRIPT_IMPLEMENTATION);
requireCondition(
  sha256(sourceBytes) === EXPECTED_TYPESCRIPT_SHA256,
  "vendored _tsc.js differs from the reviewed TypeScript 6.0.3 pin",
);
requireCondition(ts.version === "6.0.3", `unexpected TypeScript version ${ts.version}`);

const sourceText = sourceBytes.toString("utf8");
const sourceLines = splitLines(sourceText);
const sourcePath = path.join(WORKSPACE, TYPESCRIPT_IMPLEMENTATION);
const sourceFile = ts.createSourceFile(
  sourcePath,
  sourceText,
  ts.ScriptTarget.Latest,
  /* setParentNodes */ true,
  ts.ScriptKind.JS,
);

// Reuse the explicitly created SourceFile in a no-lib/no-resolve program so
// identifier references can be bound to their actual declarations. This
// prevents a same-spelling local callback from being guessed as a reached
// top-level helper.
const compilerHost = {
  getSourceFile(fileName) {
    return path.resolve(fileName) === sourcePath ? sourceFile : undefined;
  },
  getDefaultLibFileName() {
    return "lib.d.ts";
  },
  writeFile() {},
  getCurrentDirectory() {
    return WORKSPACE;
  },
  getDirectories() {
    return [];
  },
  fileExists(fileName) {
    return path.resolve(fileName) === sourcePath;
  },
  readFile(fileName) {
    return path.resolve(fileName) === sourcePath ? sourceText : undefined;
  },
  useCaseSensitiveFileNames() {
    return true;
  },
  getCanonicalFileName(fileName) {
    return fileName;
  },
  getNewLine() {
    return "\n";
  },
};
const program = ts.createProgram({
  rootNames: [sourcePath],
  options: {
    allowJs: true,
    checkJs: false,
    noLib: true,
    noResolve: true,
    target: ts.ScriptTarget.Latest,
  },
  host: compilerHost,
});
requireCondition(program.getSourceFile(sourcePath) === sourceFile, "source parser identity drifted");
const checker = program.getTypeChecker();

const allNodes = [];
function collectNodes(node) {
  allNodes.push(node);
  ts.forEachChild(node, collectNodes);
}
collectNodes(sourceFile);

const functionDeclarations = allNodes.filter(ts.isFunctionDeclaration);
const propertyAssignments = allNodes.filter(ts.isPropertyAssignment);
const variableDeclarations = allNodes.filter(ts.isVariableDeclaration);
const callExpressions = allNodes.filter(ts.isCallExpression);

function lineAt(offset) {
  return sourceFile.getLineAndCharacterOfPosition(offset).line + 1;
}

function nodeSpan(node) {
  const start = node.getStart(sourceFile);
  const end = Math.max(start, node.end - 1);
  return { start_line: lineAt(start), end_line: lineAt(end) };
}

function spanSha256(lines, span) {
  return sha256(Buffer.from(lineSpanText(lines, span.start_line, span.end_line), "utf8"));
}

function functionName(node) {
  return node.name && ts.isIdentifier(node.name) ? node.name.text : null;
}

function propertyName(node) {
  const name = node.name;
  if (ts.isIdentifier(name) || ts.isStringLiteralLike(name) || ts.isNumericLiteral(name)) {
    return name.text;
  }
  return null;
}

function nearestFunctionName(node) {
  for (let current = node.parent; current; current = current.parent) {
    if (ts.isFunctionDeclaration(current)) return functionName(current);
  }
  return null;
}

function hasAncestor(node, ancestor) {
  for (let current = node.parent; current; current = current.parent) {
    if (current === ancestor) return true;
  }
  return false;
}

function isWithin(node, owner) {
  return node === owner || hasAncestor(node, owner);
}

function findFunction(name, expectedLine) {
  const matches = functionDeclarations.filter(
    (node) => functionName(node) === name && nodeSpan(node).start_line === expectedLine,
  );
  requireCondition(matches.length === 1, `${name} function anchor ${expectedLine} changed`);
  return matches[0];
}

function findProperty(name, expectedLine) {
  const matches = propertyAssignments.filter(
    (node) => propertyName(node) === name && nodeSpan(node).start_line === expectedLine,
  );
  requireCondition(matches.length === 1, `${name} property anchor ${expectedLine} changed`);
  return matches[0];
}

function rowForNode({
  surface,
  name,
  kind,
  node,
  reachability,
  consumers,
  rustAnchor = null,
  disposition,
  targetRung = null,
  partition = null,
  callCount,
}) {
  const span = nodeSpan(node);
  const row = {
    surface,
    name,
    kind,
    span,
    sha256: spanSha256(sourceLines, span),
    nesting_parent: nearestFunctionName(node),
    reachability,
    consumers: [...new Set(consumers)].sort(),
    rust_anchor: rustAnchor,
    disposition,
    target_rung: targetRung,
    partition,
  };
  if (callCount !== undefined) row.call_count = callCount;
  return row;
}

function resolvedFunction(identifier) {
  const symbol = checker.getSymbolAtLocation(identifier);
  const declarations = symbol?.declarations?.filter(ts.isFunctionDeclaration) ?? [];
  return declarations.length === 1 ? declarations[0] : null;
}

const matchingRustAnchors = new Map([
  ...rustFunctionAnchors(RUST_WRITER_SOURCE),
  ...rustFunctionAnchors(RUST_PRINTER_SOURCE),
]);

function printerRustAnchor(name) {
  return PRINTER_RUST_ANCHORS[name] ?? matchingRustAnchors.get(camelToSnake(name)) ?? null;
}

const rows = [];

// Surface a: declarations.ts module, including the transform owner, all 60
// nested FunctionDeclarations, and the seven file-scope companions.
const transformDeclarations = findFunction("transformDeclarations", 114265);
requireCondition(
  nodeSpan(transformDeclarations).end_line === 115802,
  "transformDeclarations owner span changed",
);
const declarationFunctions = functionDeclarations.filter((node) => {
  const span = nodeSpan(node);
  return span.start_line >= 114249 && span.end_line <= 115873;
});
const declarationNested = declarationFunctions.filter(
  (node) => node !== transformDeclarations && hasAncestor(node, transformDeclarations),
);
requireCondition(declarationFunctions.length === 68, "declarations module must have 68 function rows");
requireCondition(declarationNested.length === 60, "transformDeclarations must have 60 nested functions");
requireCondition(
  declarationFunctions.some(
    (node) => functionName(node) === "isProcessedComponent" && nodeSpan(node).start_line === 115850,
  ),
  "declarations module terminal sibling changed",
);
for (const node of declarationFunctions) {
  const name = functionName(node);
  const isOwner = node === transformDeclarations;
  const isNested = declarationNested.includes(node);
  const isDiagnostics = name === "getDeclarationDiagnostics";
  rows.push(
    rowForNode({
      surface: "declarations-module",
      name,
      kind: isOwner ? "owner" : isNested ? "nested" : "sibling",
      node,
      reachability: isNested ? "reached" : "direct",
      consumers: isDiagnostics
        ? ["H2.7c"]
        : ["H2.7a", "H2.7b", "H2.7c", "H2.7d", "H2.7e", "H2.8c", "H2.8d", "BLD1"],
      disposition: isDiagnostics
        ? "declaration-diagnostics-owner"
        : "declaration-transformer-foundation",
      targetRung: isDiagnostics ? "H2.7c" : "h2-7a-m-4",
    }),
  );
}

// Surface b: declaration-transformer selection and the build-info-only empty
// transformer set. The latter has exactly one use, at emitBuildInfo:123542.
const getDeclarationTransformers = findFunction("getDeclarationTransformers", 115950);
rows.push(
  rowForNode({
    surface: "selection-seam",
    name: "getDeclarationTransformers",
    kind: "owner",
    node: getDeclarationTransformers,
    reachability: "direct",
    consumers: ["H2.7a", "H2.7b", "API1"],
    disposition: "declaration-selection-foundation",
    targetRung: "h2-7a-m-4",
  }),
);
const noTransformers = variableDeclarations.filter(
  (node) =>
    ts.isIdentifier(node.name) &&
    node.name.text === "noTransformers" &&
    nodeSpan(node).start_line === 115896,
);
requireCondition(noTransformers.length === 1, "noTransformers declaration anchor changed");
const noTransformersSymbol = checker.getSymbolAtLocation(noTransformers[0].name);
const noTransformerUses = allNodes.filter(
  (node) =>
    ts.isIdentifier(node) &&
    node.text === "noTransformers" &&
    node !== noTransformers[0].name &&
    checker.getSymbolAtLocation(node) === noTransformersSymbol,
);
requireCondition(
  noTransformerUses.length === 1 && lineAt(noTransformerUses[0].getStart(sourceFile)) === 123542,
  "noTransformers must retain its sole emitBuildInfo consumer at line 123542",
);
rows.push(
  rowForNode({
    surface: "selection-seam",
    name: "noTransformers",
    kind: "member",
    node: noTransformers[0],
    reachability: "context",
    consumers: ["shared/build-info"],
    disposition: "shared/build-info",
  }),
);

// Surface c: checker NodeBuilder.
const createNodeBuilder = findFunction("createNodeBuilder", 50777);
requireCondition(nodeSpan(createNodeBuilder).end_line === 55451, "createNodeBuilder span changed");
const nodeBuilderFunctions = functionDeclarations.filter((node) => isWithin(node, createNodeBuilder));
requireCondition(nodeBuilderFunctions.length === 149, "createNodeBuilder must have 149 function rows");
requireCondition(
  nodeBuilderFunctions.filter((node) => node !== createNodeBuilder).length === 148,
  "createNodeBuilder must have 148 nested function declarations",
);
for (const node of nodeBuilderFunctions) {
  const name = functionName(node);
  const symbolMember = name === "symbolToDeclarations" && nodeSpan(node).start_line === 51136;
  rows.push(
    rowForNode({
      surface: "node-builder",
      name,
      kind: node === createNodeBuilder ? "owner" : symbolMember ? "member" : "nested",
      node,
      reachability: node === createNodeBuilder ? "direct" : "reached",
      consumers: symbolMember ? ["H2.7a", "API1"] : ["H2.7a", "H2.7b", "H2.7d", "BLD1", "API1"],
      disposition: symbolMember
        ? "node-builder-internal-api-surface"
        : "node-builder-foundation",
      targetRung: "h2-7a-m-3",
    }),
  );
}
requireCondition(
  nodeBuilderFunctions.some(
    (node) => functionName(node) === "symbolToDeclarations" && nodeSpan(node).start_line === 51136,
  ),
  "NodeBuilder symbolToDeclarations member changed",
);

// Surface d: syntactic type-node builder.
const createSyntacticTypeNodeBuilder = findFunction("createSyntacticTypeNodeBuilder", 133276);
requireCondition(
  nodeSpan(createSyntacticTypeNodeBuilder).end_line === 134447,
  "createSyntacticTypeNodeBuilder span changed",
);
const syntacticBuilderFunctions = functionDeclarations.filter((node) =>
  isWithin(node, createSyntacticTypeNodeBuilder),
);
for (const node of syntacticBuilderFunctions) {
  rows.push(
    rowForNode({
      surface: "syntactic-builder",
      name: functionName(node),
      kind: node === createSyntacticTypeNodeBuilder ? "owner" : "nested",
      node,
      reachability: node === createSyntacticTypeNodeBuilder ? "direct" : "reached",
      consumers: ["H2.7a", "H2.7b", "API1"],
      disposition: "syntactic-builder-foundation",
      targetRung: "h2-7a-m-3",
    }),
  );
}

// Surface e: declaration-consumed resolver members and all 28 module call
// sites. The worker span is used even when createResolver returns a shorthand
// member; inline wrappers use their exact property-assignment span.
requireCondition(RESOLVER_MEMBER_SPECS.length === 19, "resolver member roster must have 19 entries");
const resolverMemberRows = [];
const resolverMetadata = new Map();
for (const [name, line, nodeKind] of RESOLVER_MEMBER_SPECS) {
  const node = nodeKind === "function" ? findFunction(name, line) : findProperty(name, line);
  const existing = EXISTING_RESOLVER_MEMBERS.has(name);
  const nodeBuilderDependent = NODE_BUILDER_DEPENDENT_MEMBERS.has(name);
  const partition = existing ? null : nodeBuilderDependent ? "m-3-head" : "m-2";
  const disposition = existing
    ? "existing-resolver-api"
    : nodeBuilderDependent
      ? "node-builder-dependent-resolver"
      : "checker-native-resolver";
  const targetRung = existing ? null : nodeBuilderDependent ? "h2-7a-m-3" : "h2-7a-m-2";
  const rustAnchor = existing ? "crates/emitter/src/resolver.rs:60" : null;
  const row = rowForNode({
    surface: "resolver-declaration-subset",
    name,
    kind: "member",
    node,
    reachability: "direct",
    consumers: ["H2.7a", "H2.7b", "H2.7c", "H2.7d", "H2.8c", "H2.8d", "BLD1", "API1"],
    rustAnchor,
    disposition,
    targetRung,
    partition,
  });
  resolverMemberRows.push(row);
  resolverMetadata.set(name, { disposition, targetRung, partition });
  rows.push(row);
}

const symbolToDeclarationsProperty = findProperty("symbolToDeclarations", 88692);
const symbolResolverRow = rowForNode({
  surface: "resolver-declaration-subset",
  name: "symbolToDeclarations",
  kind: "member",
  node: symbolToDeclarationsProperty,
  reachability: "direct",
  consumers: ["H2.7a", "API1"],
  disposition: "node-builder-internal-api-surface",
  targetRung: "h2-7a-m-3",
  partition: "m-3-head",
});
resolverMemberRows.push(symbolResolverRow);
resolverMetadata.set("symbolToDeclarations", {
  disposition: symbolResolverRow.disposition,
  targetRung: symbolResolverRow.target_rung,
  partition: symbolResolverRow.partition,
});
rows.push(symbolResolverRow);

const declarationResolverCalls = callExpressions.filter((call) => {
  if (!declarationFunctions.some((owner) => isWithin(call, owner))) return false;
  return (
    ts.isPropertyAccessExpression(call.expression) &&
    ts.isIdentifier(call.expression.expression) &&
    call.expression.expression.text === "resolver"
  );
});
const resolverCallCounts = {};
for (const call of declarationResolverCalls) {
  const name = call.expression.name.text;
  resolverCallCounts[name] = (resolverCallCounts[name] ?? 0) + 1;
  const metadata = resolverMetadata.get(name);
  requireCondition(metadata !== undefined, `unreviewed declarations resolver member ${name}`);
  rows.push(
    rowForNode({
      surface: "resolver-declaration-subset",
      name,
      kind: "use_site",
      node: call,
      reachability: "context",
      consumers: ["H2.7a", "H2.7b"],
      disposition: metadata.disposition,
      targetRung: metadata.targetRung,
    }),
  );
}
requireCondition(
  canonical(resolverCallCounts) === canonical(EXPECTED_RESOLVER_CALLS),
  `resolver call multiset changed: ${canonical(resolverCallCounts)}`,
);
requireCondition(declarationResolverCalls.length === 28, "declarations module must have 28 resolver calls");
requireCondition(
  !declarationResolverCalls.some((call) => call.expression.name.text === "symbolToDeclarations"),
  "symbolToDeclarations gained a resolver.* declarations-module call",
);

const orchestrationUseSpecs = Object.freeze([
  ["collectLinkedAliases", 116719, "declaration-orchestration", "H2.7b", ["H2.7b", "H2.8c", "H2.8d"]],
  ["collectLinkedAliases", 116727, "declaration-orchestration", "H2.7b", ["H2.7b", "H2.8c", "H2.8d"]],
  ["markLinkedReferences", 116741, "declaration-orchestration", "H2.7b", ["H2.7b"]],
  ["markLinkedReferences", 116597, "script-orchestration", null, ["H2.7a"]],
  ["hasGlobalName", 116624, "shared/printer-hook", null, ["API1"]],
  ["hasGlobalName", 116688, "shared/printer-hook", "H2.7b", ["H2.7b", "API1"]],
]);
for (const [name, line, disposition, targetRung, consumers] of orchestrationUseSpecs) {
  const matches = callExpressions.filter((call) => {
    if (nodeSpan(call).start_line !== line) return false;
    if (ts.isIdentifier(call.expression)) return call.expression.text === name;
    return ts.isPropertyAccessExpression(call.expression) && call.expression.name.text === name;
  });
  // hasGlobalName is passed as a hook rather than called at both sites.
  const propertyReferences = allNodes.filter(
    (node) =>
      ts.isPropertyAccessExpression(node) &&
      node.name.text === name &&
      nodeSpan(node).start_line === line,
  );
  const candidates = matches.length > 0 ? matches : propertyReferences;
  requireCondition(candidates.length === 1, `${name} orchestration use at ${line} changed`);
  rows.push(
    rowForNode({
      surface: "resolver-declaration-subset",
      name,
      kind: "use_site",
      node: candidates[0],
      reachability: "context",
      consumers,
      disposition,
      targetRung,
    }),
  );
}

const partitionCounts = {
  m_3_head: resolverMemberRows.filter((row) => row.partition === "m-3-head").length,
  m_2: resolverMemberRows.filter((row) => row.partition === "m-2").length,
};
requireCondition(
  partitionCounts.m_3_head === 7 && partitionCounts.m_2 === 12,
  `resolver partition must be 7/12, got ${partitionCounts.m_3_head}/${partitionCounts.m_2}`,
);

// Surface f: establish the complete createPrinter denominator, then close the
// signed 58-name seed over exact in-printer function references. Core TypeNode
// switch arms are independently seeded so a new dispatch worker cannot hide
// behind a stale prose list.
const createPrinter = findFunction("createPrinter", 116912);
requireCondition(nodeSpan(createPrinter).end_line === 121378, "createPrinter span changed");
const printerFunctions = functionDeclarations.filter((node) => isWithin(node, createPrinter));
requireCondition(printerFunctions.length === 370, "createPrinter must have 370 function rows");
requireCondition(PRINTER_SEED_NAMES.length === 58, "printer seed must contain 58 names");
requireCondition(
  new Set(PRINTER_SEED_NAMES).size === PRINTER_SEED_NAMES.length,
  "printer seed names must be unique",
);
const printerFunctionByName = new Map();
for (const node of printerFunctions) {
  const name = functionName(node);
  requireCondition(name !== null, "createPrinter contains an anonymous FunctionDeclaration");
  requireCondition(!printerFunctionByName.has(name), `duplicate createPrinter worker ${name}`);
  printerFunctionByName.set(name, node);
}
for (const name of PRINTER_SEED_NAMES) {
  requireCondition(printerFunctionByName.has(name), `printer seed worker ${name} disappeared`);
}

const pipelineEmitWithHintWorker = printerFunctionByName.get("pipelineEmitWithHintWorker");
requireCondition(pipelineEmitWithHintWorker !== undefined, "printer dispatch worker disappeared");
const coreTypeNodeKinds = new Set([
  183, 184, 185, 186, 187, 188, 189, 190, 191, 192, 193, 194, 195, 196, 197,
  198, 199, 200, 201, 202, 203, 204, 205, 206, 234,
]);
const typeDispatchNames = new Set();
function collectTypeDispatch(node) {
  if (ts.isCaseClause(node)) {
    const kind = ts.isNumericLiteral(node.expression) ? Number(node.expression.text) : NaN;
    if (coreTypeNodeKinds.has(kind)) {
      const visit = (child) => {
        if (ts.isCallExpression(child) && ts.isIdentifier(child.expression)) {
          const target = resolvedFunction(child.expression);
          if (target && printerFunctions.includes(target)) typeDispatchNames.add(functionName(target));
        }
        ts.forEachChild(child, visit);
      };
      node.statements.forEach(visit);
    }
  }
  ts.forEachChild(node, collectTypeDispatch);
}
collectTypeDispatch(pipelineEmitWithHintWorker.body);
requireCondition(typeDispatchNames.size >= 20, "printer TypeNode dispatch scan narrowed unexpectedly");

const printerEdges = new Map();
for (const owner of printerFunctions) {
  const edges = new Set();
  const visit = (node) => {
    if (node !== owner && ts.isFunctionDeclaration(node)) {
      edges.add(node);
      return;
    }
    if (ts.isIdentifier(node)) {
      const target = resolvedFunction(node);
      if (target && target !== owner && printerFunctions.includes(target)) edges.add(target);
    }
    ts.forEachChild(node, visit);
  };
  if (owner.body) visit(owner.body);
  printerEdges.set(owner, edges);
}
// pipelineEmitWithHintWorker is a whole-printer switch. Following every arm
// from the generic emit() pipeline would falsely turn a TypeNode edge into all
// statement/expression workers. Its semantically reachable edge set for this
// inventory is the independently measured TypeNode arm set above.
printerEdges.set(
  pipelineEmitWithHintWorker,
  new Set([...typeDispatchNames].map((name) => printerFunctionByName.get(name))),
);

const printerSeedSet = new Set([
  ...PRINTER_SEED_NAMES.map((name) => printerFunctionByName.get(name)),
  ...[...typeDispatchNames].map((name) => printerFunctionByName.get(name)),
]);
const printerSubgraph = new Set(printerSeedSet);
const printerQueue = [...printerSeedSet];
while (printerQueue.length > 0) {
  const owner = printerQueue.shift();
  for (const target of printerEdges.get(owner) ?? []) {
    if (!printerSubgraph.has(target)) {
      printerSubgraph.add(target);
      printerQueue.push(target);
    }
  }
}
// The owner is contextual identity, not a closure seed: traversing its lexical
// children would collapse the selected numerator into all 370 workers.
rows.push(
  rowForNode({
    surface: "printer-subgraph",
    name: "createPrinter",
    kind: "owner",
    node: createPrinter,
    reachability: "context",
    consumers: ["H2.7a", "H2.7b", "H2.7d", "API1"],
    rustAnchor: printerRustAnchor("createPrinter"),
    disposition: "audit-pending",
    targetRung: "h2-7a-m-3.5",
  }),
);
for (const node of printerSubgraph) {
  const name = functionName(node);
  rows.push(
    rowForNode({
      surface: "printer-subgraph",
      name,
      kind: "nested",
      node,
      reachability: printerSeedSet.has(node) ? "direct" : "reached",
      consumers: ["H2.7a", "H2.7b", "H2.7d", "API1"],
      rustAnchor: printerRustAnchor(name),
      disposition: "audit-pending",
      targetRung: "h2-7a-m-3.5",
    }),
  );
}

// Surface g: factory calls from a/c/d and parenthesizer operations used by
// the selected declaration/type printer. Parenthesizer rules are passed as
// callbacks, so exact property references are their mechanically visible call
// sites even when invocation occurs inside emit()/emitList().
const factoryScanRoots = [
  ...declarationFunctions.filter((node) => !declarationFunctions.some(
    (other) => other !== node && isWithin(node, other),
  )),
  createNodeBuilder,
  createSyntacticTypeNodeBuilder,
];
const factoryCalls = callExpressions.filter(
  (call) =>
    factoryScanRoots.some((root) => isWithin(call, root)) &&
    ts.isPropertyAccessExpression(call.expression) &&
    ts.isIdentifier(call.expression.expression) &&
    call.expression.expression.text === "factory",
);

const parenthesizerReferences = [];
for (const owner of [createPrinter, ...printerSubgraph]) {
  const visit = (node) => {
    if (node !== owner && ts.isFunctionDeclaration(node)) return;
    if (
      ts.isPropertyAccessExpression(node) &&
      ts.isIdentifier(node.expression) &&
      node.expression.text === "parenthesizer"
    ) {
      parenthesizerReferences.push(node);
    }
    ts.forEachChild(node, visit);
  };
  if (owner.body) visit(owner.body);
}

function groupMemberSites(nodes, nameOf) {
  const grouped = new Map();
  for (const node of nodes) {
    const name = nameOf(node);
    const existing = grouped.get(name) ?? [];
    existing.push(node);
    grouped.set(name, existing);
  }
  return grouped;
}

const factoryMemberSites = groupMemberSites(factoryCalls, (call) => call.expression.name.text);
const parenthesizerMemberSites = groupMemberSites(
  parenthesizerReferences,
  (reference) => reference.name.text,
);

function memberRangeRow(receiver, name, sites) {
  const spans = sites.map(nodeSpan);
  const span = {
    start_line: Math.min(...spans.map((item) => item.start_line)),
    end_line: Math.max(...spans.map((item) => item.end_line)),
  };
  return {
    surface: "factory-parenthesizer",
    name: `${receiver}.${name}`,
    kind: "member",
    span,
    sha256: spanSha256(sourceLines, span),
    nesting_parent: null,
    reachability: "direct",
    consumers: ["H2.7a", "H2.7b", "API1"],
    rust_anchor:
      receiver === "factory"
        ? "crates/emitter/src/factory.rs:1182"
        : "crates/emitter/src/factory.rs:2004",
    disposition: "audit-pending",
    target_rung: "h2-7a-m-3.5",
    partition: null,
    call_count: sites.length,
  };
}

for (const [name, sites] of factoryMemberSites) rows.push(memberRangeRow("factory", name, sites));
for (const [name, sites] of parenthesizerMemberSites) {
  rows.push(memberRangeRow("parenthesizer", name, sites));
}
requireCondition(factoryMemberSites.size > 0, "factory member inventory is empty");
requireCondition(parenthesizerMemberSites.size > 0, "parenthesizer member inventory is empty");

// One-hop reached-helper closure. Only identifiers bound by TypeScript to a
// SourceFile-level FunctionDeclaration qualify; local callbacks and nested
// workers therefore cannot enter through spelling alone.
const sevenSurfaceFunctionNodes = new Set([
  ...declarationFunctions,
  getDeclarationTransformers,
  ...nodeBuilderFunctions,
  ...syntacticBuilderFunctions,
  ...RESOLVER_MEMBER_SPECS.flatMap(([name, line, kind]) =>
    kind === "function" ? [findFunction(name, line)] : [],
  ),
  createPrinter,
  ...printerFunctions,
]);
const reachedBySurface = new Map();
const reachedRoots = Object.freeze([
  ["declarations-module", declarationFunctions],
  ["node-builder", [createNodeBuilder]],
  ["syntactic-builder", [createSyntacticTypeNodeBuilder]],
]);
for (const [surface, roots] of reachedRoots) {
  const reached = new Set();
  for (const call of callExpressions) {
    if (!roots.some((root) => isWithin(call, root))) continue;
    if (!ts.isIdentifier(call.expression)) continue;
    const target = resolvedFunction(call.expression);
    if (
      target &&
      target.parent === sourceFile &&
      !sevenSurfaceFunctionNodes.has(target)
    ) {
      reached.add(target);
    }
  }
  reachedBySurface.set(surface, reached);
}
requireCondition(UNRESOLVED_CANDIDATES.length === 0, "UNRESOLVED_CANDIDATES must be empty");
const unresolvedSet = new Set(UNRESOLVED_CANDIDATES);
for (const [surface, reached] of reachedBySurface) {
  for (const node of reached) {
    const name = functionName(node);
    requireCondition(!unresolvedSet.has(name), `unresolved reached helper ${name}`);
    const [disposition, targetRung] = REACHED_DISPOSITIONS[name] ?? [
      "shared/core-utility",
      null,
    ];
    const consumers =
      surface === "declarations-module"
        ? ["H2.7a", "H2.7b", "H2.7c"]
        : ["H2.7a", "H2.7b", "API1"];
    rows.push(
      rowForNode({
        surface,
        name,
        kind: "reached",
        node,
        reachability: "reached",
        consumers,
        disposition,
        targetRung,
      }),
    );
  }
}

// Option-owner closure: parse the exact signed 448-467 text at runtime and
// retain both the enclosing span pin and a hash for each emitted map row.
const optionOwnerBytes = readBytes(OPTION_OWNER_SOURCE);
const optionOwnerText = optionOwnerBytes.toString("utf8");
const optionOwnerLines = splitLines(optionOwnerText);
const optionOwnerSpan = { start_line: 448, end_line: 467 };
const optionOwnerSpanText = lineSpanText(
  optionOwnerLines,
  optionOwnerSpan.start_line,
  optionOwnerSpan.end_line,
);
requireCondition(
  sha256(Buffer.from(optionOwnerSpanText, "utf8")) === EXPECTED_OPTION_OWNER_SPAN_SHA256,
  "h2-transition option-owner span 448-467 changed",
);
const optionEntries = [];
for (let line = optionOwnerSpan.start_line; line <= optionOwnerSpan.end_line; line += 1) {
  const text = optionOwnerLines[line - 1];
  const blocker = text.match(/blocker === "([^"]+)"\) return "([^"]+)";/u);
  if (blocker) {
    optionEntries.push({ name: blocker[1], slice: blocker[2], line });
    continue;
  }
  const option = text.match(/^\s+([A-Za-z][A-Za-z0-9]*): "([^"]+)",\s*$/u);
  if (option) optionEntries.push({ name: `rejected-option:${option[1]}`, slice: option[2], line });
}
requireCondition(optionEntries.length === 18, "option-owner closure must contain 18 rows");
for (const entry of optionEntries) {
  const span = { start_line: entry.line, end_line: entry.line };
  rows.push({
    surface: "option-owner-closure",
    name: entry.name,
    kind: "member",
    span,
    sha256: spanSha256(optionOwnerLines, span),
    nesting_parent: "classifyBlocker",
    reachability: "context",
    consumers: [entry.slice],
    rust_anchor: null,
    disposition: "option-owner",
    target_rung: entry.slice,
    partition: null,
  });
}

rows.sort(
  (left, right) =>
    compareStrings(left.surface, right.surface) ||
    left.span.start_line - right.span.start_line ||
    compareStrings(left.name, right.name) ||
    compareStrings(left.kind, right.kind) ||
    left.span.end_line - right.span.end_line,
);

const reachedRows = [...reachedBySurface.values()].reduce(
  (total, reached) => total + reached.size,
  0,
);
const countSurfaceRows = (surface) => rows.filter((row) => row.surface === surface).length;
const summary = {
  total_rows: rows.length,
  surface_rows: {
    declarations_module: countSurfaceRows("declarations-module"),
    selection_seam: countSurfaceRows("selection-seam"),
    node_builder: countSurfaceRows("node-builder"),
    syntactic_builder: countSurfaceRows("syntactic-builder"),
    resolver_declaration_subset: countSurfaceRows("resolver-declaration-subset"),
    printer_subgraph: countSurfaceRows("printer-subgraph"),
    factory_parenthesizer: countSurfaceRows("factory-parenthesizer"),
    option_owner_closure: countSurfaceRows("option-owner-closure"),
  },
  declarations_module: {
    function_rows: declarationFunctions.length,
    owner_rows: 1,
    nested_function_rows: declarationNested.length,
    sibling_function_rows: declarationFunctions.length - declarationNested.length - 1,
  },
  selection_seam: {
    rows: 2,
    no_transformers_consumers: noTransformerUses.length,
  },
  node_builder: {
    function_rows: nodeBuilderFunctions.length,
    nested_function_rows: nodeBuilderFunctions.length - 1,
  },
  syntactic_builder: {
    function_rows: syntacticBuilderFunctions.length,
  },
  resolver: {
    consumed_members: RESOLVER_MEMBER_SPECS.length,
    member_rows: resolverMemberRows.length,
    declarations_module_call_sites: declarationResolverCalls.length,
    orchestration_use_sites: orchestrationUseSpecs.length,
  },
  printer: {
    function_rows: printerFunctions.length,
    seed_workers: PRINTER_SEED_NAMES.length,
    type_dispatch_workers: typeDispatchNames.size,
    subgraph_rows: printerSubgraph.size + 1,
  },
  factory_parenthesizer: {
    factory_members: factoryMemberSites.size,
    factory_calls: factoryCalls.length,
    parenthesizer_members: parenthesizerMemberSites.size,
    parenthesizer_calls: parenthesizerReferences.length,
  },
  partition: partitionCounts,
  reached_rows: reachedRows,
  reached_by_surface: {
    declarations_module: reachedBySurface.get("declarations-module").size,
    node_builder: reachedBySurface.get("node-builder").size,
    syntactic_builder: reachedBySurface.get("syntactic-builder").size,
  },
  option_rows: optionEntries.length,
  unresolved_candidates: UNRESOLVED_CANDIDATES.length,
};

const artifact = withFingerprint(
  {
    schema: 1,
    status: "measured",
    phase: "H2.7a-owner-inventory",
    generator: pathHash(GENERATOR_RELATIVE_PATH),
    contract: pathHash(CONTRACT_RELATIVE_PATH),
    inputs: {
      typescript: {
        version: ts.version,
        implementation: pathHash(TYPESCRIPT_IMPLEMENTATION),
      },
      option_owner: {
        source: pathHash(OPTION_OWNER_SOURCE),
        span: {
          ...optionOwnerSpan,
          sha256: EXPECTED_OPTION_OWNER_SPAN_SHA256,
        },
      },
      rust_evidence: {
        printer: pathHash(RUST_PRINTER_SOURCE),
        writer: pathHash(RUST_WRITER_SOURCE),
        factory: pathHash(RUST_FACTORY_SOURCE),
        resolver: pathHash(RUST_RESOLVER_SOURCE),
      },
    },
    counting_grammar:
      "one row per FunctionDeclaration; an owner row additionally covers its own body; line hashes cover exact inclusive LF source lines",
    printer_closure:
      "the signed 58-name seed plus core TypeNode dispatch targets, transitively closed over bound in-printer FunctionDeclaration references and lexical nested workers",
    reached_helper_closure:
      "identifier call targets bound to SourceFile-level FunctionDeclarations outside the seven surfaces, measured independently for declarations-module, node-builder, and syntactic-builder",
    unresolved_candidates: [...UNRESOLVED_CANDIDATES],
    summary,
    rows,
  },
  "inventory_fingerprint_sha256",
);

const rendered = render(artifact);
const targetPath = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
const summaryLine =
  `decl=${summary.declarations_module.function_rows} ` +
  `node_builder=${summary.node_builder.function_rows} ` +
  `printer=${summary.printer.function_rows} ` +
  `resolver=${summary.resolver.consumed_members}/${summary.resolver.declarations_module_call_sites} ` +
  `partition=${summary.partition.m_3_head}/${summary.partition.m_2} ` +
  `reached=${summary.reached_rows} options=${summary.option_rows}`;

if (MODE === "--write") {
  fs.writeFileSync(targetPath, rendered);
  process.stdout.write(`wrote ${TARGET_RELATIVE_PATH}: ${summaryLine}\n`);
} else if (MODE === "--check") {
  requireCondition(
    fs.existsSync(targetPath) && fs.readFileSync(targetPath, "utf8") === rendered,
    `stale ${TARGET_RELATIVE_PATH}; run --write and review`,
  );
  process.stdout.write(`H2.7a owner inventory is fresh: ${summaryLine}\n`);
} else if (MODE === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-7a-owner-inventory.mjs [--write|--check]");
}
