import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const workspace = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const sourceRelativePath = "vendor/typescript-6.0.3/lib/_tsc.js";
const sourcePath = path.join(workspace, sourceRelativePath);
const targetRelativePath = "ratchets/h1-owner-inventory.v1.json";
const targetPath = path.join(workspace, targetRelativePath);
const contractRelativePath = ".github/ci/contracts/h1-owner-inventory.schema.json";
const expectedSourceSha256 = "1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3";

const activeRootSpecs = [
  { key: "program-emit", name: "emit", line: 123568, owner: "createProgram" },
  { key: "cli-emit", name: "emitFilesAndReportErrors", line: 129412 },
  { key: "transformer-selection", name: "getTransformers", line: 115897 },
  { key: "typescript-transform", name: "transformTypeScript", line: 94036 },
  { key: "class-fields-transform", name: "transformClassFields", line: 95852 },
  { key: "module-transform", name: "transformECMAScriptModule", line: 113369 },
  { key: "transform-runtime", name: "transformNodes", line: 115977 },
  { key: "printer", name: "createPrinter", line: 116912 },
  { key: "output-enumeration", name: "forEachEmittedFile", line: 116312 },
  { key: "output-paths", name: "getOutputPathsFor", line: 116373 },
  { key: "emit-orchestration", name: "emitFiles", line: 116530 },
];

const dormantSeamSpecs = [
  {
    axis: "declaration",
    name: "getDeclarationEmitOutputFilePath",
    line: 16577,
    role: "declaration output-path slot",
  },
  {
    axis: "declaration",
    name: "transformDeclarations",
    line: 114265,
    role: "declaration transform root",
  },
  {
    axis: "declaration",
    name: "getDeclarationTransformers",
    line: 115950,
    role: "declaration transformer ordering",
  },
  {
    axis: "source-map",
    name: "createSourceMapGenerator",
    line: 92365,
    role: "source-map generator contract",
  },
  {
    axis: "source-map",
    name: "getSourceMapFilePath",
    line: 116388,
    role: "source-map output-path slot",
  },
  {
    axis: "bundle",
    name: "getOutputPathsForBundle",
    line: 116365,
    role: "bundle output shape",
  },
  {
    axis: "build-info",
    name: "getTsBuildInfoEmitOutputFilePath",
    line: 116342,
    role: "build-info output-path slot",
  },
  {
    axis: "targeted-emit",
    name: "emit",
    line: 123568,
    owner: "createProgram",
    role: "target SourceFile request parameter",
  },
];

const EXPECTED_SUMMARY = {
  source_declarations: 10899,
  active_roots: 11,
  dormant_seams: 8,
  closure_declarations: 6193,
  static_edges: 24054,
  lexical_edges: 19033,
  immediate_edges: 0,
  dynamic_symbol_edges: 10,
  nested_function_edges: 4511,
  property_symbol_edges: 500,
  call_sites: 29310,
  reviewed_call_sites: 5202,
  identifier_runtime_library_calls: 23,
  identifier_parameter_callback_calls: 279,
  identifier_destructured_callback_calls: 114,
  identifier_source_value_calls: 301,
  identifier_external_global_calls: 1,
  property_source_symbol_calls: 704,
  property_runtime_library_calls: 749,
  property_structural_dispatch_calls: 3021,
  property_checker_stack_overflow_calls: 1,
  dynamic_expression_calls: 10,
  dynamic_source_expression_calls: 7,
  reviewed_exact_edge_calls: 711,
  reviewed_non_edge_calls: 4491,
  property_candidate_sets: 442,
  property_candidate_declarations: 652,
  undispositioned_calls: 0,
};

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function pathHash(relative) {
  return { path: relative, sha256: sha256(fs.readFileSync(path.join(workspace, relative))) };
}

function requireJsonEqual(actual, expected, description) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(
      `${description} mismatch\nactual=${JSON.stringify(actual)}\nexpected=${JSON.stringify(expected)}`,
    );
  }
}

const sourceText = fs.readFileSync(sourcePath, "utf8");
if (sha256(sourceText) !== expectedSourceSha256) {
  throw new Error("vendored _tsc.js differs from the reviewed TypeScript 6.0.3 pin");
}
if (ts.version !== "6.0.3") throw new Error(`unexpected TypeScript version ${ts.version}`);

const program = ts.createProgram({
  rootNames: [sourcePath],
  options: {
    allowJs: true,
    checkJs: false,
    noResolve: true,
    target: ts.ScriptTarget.Latest,
  },
});
const source = program.getSourceFile(sourcePath);
if (!source) throw new Error(`program did not load ${sourceRelativePath}`);
const checker = program.getTypeChecker();
const sourceLines = sourceText.split(/(?<=\n)/u);

function position(offset) {
  const location = source.getLineAndCharacterOfPosition(offset);
  return {
    offset,
    line: location.line + 1,
    character: location.character + 1,
  };
}

function sourceSliceHash(startLine, endLine) {
  return sha256(sourceLines.slice(startLine - 1, endLine).join(""));
}

function propertyNameText(name) {
  if (!name) return null;
  if (ts.isIdentifier(name) || ts.isStringLiteralLike(name) || ts.isNumericLiteral(name)) {
    return name.text;
  }
  return null;
}

function declarationInfo(node) {
  const parent = node.parent;
  if (parent && ts.isVariableDeclaration(parent) && ts.isIdentifier(parent.name)) {
    return {
      name: parent.name.text,
      lexicalBinding: parent.name.text,
      selfBinding: node.name && ts.isIdentifier(node.name) ? node.name.text : null,
      propertyAlias: null,
    };
  }
  if (parent && ts.isPropertyAssignment(parent)) {
    const name = propertyNameText(parent.name);
    return {
      name: name ?? "<anonymous>",
      lexicalBinding: null,
      selfBinding: node.name && ts.isIdentifier(node.name) ? node.name.text : null,
      propertyAlias: name,
    };
  }
  if (
    ts.isMethodDeclaration(node) ||
    ts.isGetAccessorDeclaration(node) ||
    ts.isSetAccessorDeclaration(node)
  ) {
    const name = propertyNameText(node.name);
    return {
      name: name ?? "<anonymous>",
      lexicalBinding: null,
      selfBinding: null,
      propertyAlias: name,
    };
  }
  if (node.name && ts.isIdentifier(node.name)) {
    return {
      name: node.name.text,
      lexicalBinding: ts.isFunctionDeclaration(node) ? node.name.text : null,
      selfBinding: ts.isFunctionExpression(node) ? node.name.text : null,
      propertyAlias: null,
    };
  }
  return {
    name: "<anonymous>",
    lexicalBinding: null,
    selfBinding: null,
    propertyAlias: null,
  };
}

const records = [];
const recordByNode = new Map();

function createRecord(node, owner, info) {
  const startOffset = node.getStart(source);
  const endOffset = node.end;
  const start = position(startOffset);
  const end = position(endOffset);
  const kind = ts.SyntaxKind[node.kind];
  const lexicalPath = `${owner?.lexicalPath ?? "<top>"}/${info.name}@${start.line}:${start.character}`;
  const declarationSha256 = sha256(sourceText.slice(startOffset, endOffset));
  const canonicalIdentity = JSON.stringify({
    lexical_owner_path: owner?.lexicalPath ?? null,
    kind,
    name: info.name,
    start: startOffset,
    end: endOffset,
    declaration_sha256: declarationSha256,
  });
  const body = node.body;
  const bodyStart = body ? body.getStart(source) : null;
  const bodyEnd = body ? body.end : null;
  const record = {
    node,
    id: `h1:${sha256(canonicalIdentity)}`,
    name: info.name,
    kind,
    lexicalOwner: owner?.id ?? null,
    lexicalPath,
    sourceRange: { start, end },
    declarationSha256,
    bodyRange:
      bodyStart === null ? null : { start: position(bodyStart), end: position(bodyEnd) },
    bodySha256: bodyStart === null ? null : sha256(sourceText.slice(bodyStart, bodyEnd)),
    ledgerSliceSha256: sourceSliceHash(start.line, end.line),
    lexicalBinding: info.lexicalBinding,
    selfBinding: info.selfBinding,
    propertyAlias: info.propertyAlias,
    rawCalls: [],
    callDispositions: [],
    callSites: 0,
    classifiedCallSites: 0,
  };
  records.push(record);
  recordByNode.set(node, record);
  return record;
}

function collectDeclarations(node, owner) {
  let nextOwner = owner;
  if (node !== source && ts.isFunctionLike(node)) {
    nextOwner = createRecord(node, owner, declarationInfo(node));
  }
  ts.forEachChild(node, (child) => collectDeclarations(child, nextOwner));
}
collectDeclarations(source, null);

const recordById = new Map(records.map((record) => [record.id, record]));
const lexicalBindings = new Map();
const aliasCandidates = new Map();

function addBinding(scopeId, name, record) {
  if (!name) return;
  const scopeKey = scopeId ?? "<top>";
  let scope = lexicalBindings.get(scopeKey);
  if (!scope) {
    scope = new Map();
    lexicalBindings.set(scopeKey, scope);
  }
  let candidates = scope.get(name);
  if (!candidates) {
    candidates = [];
    scope.set(name, candidates);
  }
  candidates.push(record);
}

function addAlias(name, record) {
  if (!name || name === "<anonymous>") return;
  let candidates = aliasCandidates.get(name);
  if (!candidates) {
    candidates = [];
    aliasCandidates.set(name, candidates);
  }
  if (!candidates.some((candidate) => candidate.id === record.id)) candidates.push(record);
}

for (const record of records) {
  addAlias(record.name, record);
  addAlias(record.propertyAlias, record);
  if (record.lexicalBinding) addBinding(record.lexicalOwner, record.lexicalBinding, record);
  if (record.selfBinding) addBinding(record.id, record.selfBinding, record);
}
for (const candidates of aliasCandidates.values()) {
  candidates.sort((left, right) => left.id.localeCompare(right.id));
}

function functionCandidatesForSymbol(symbol) {
  const candidates = [];
  const seenSymbols = new Set();
  const collectSymbol = (symbol) => {
    if (!symbol || seenSymbols.has(symbol)) return;
    seenSymbols.add(symbol);
    for (const declaration of symbol.declarations ?? []) {
      if (ts.isFunctionLike(declaration)) {
        const candidate = recordByNode.get(declaration);
        if (candidate) candidates.push(candidate);
        continue;
      }
      if (ts.isShorthandPropertyAssignment(declaration)) {
        collectSymbol(checker.getShorthandAssignmentValueSymbol(declaration));
        continue;
      }
      if (
        (ts.isVariableDeclaration(declaration) ||
          ts.isPropertyAssignment(declaration) ||
          ts.isPropertyDeclaration(declaration)) &&
        declaration.initializer
      ) {
        const initializer = declaration.initializer;
        if (ts.isFunctionLike(initializer)) {
          const candidate = recordByNode.get(initializer);
          if (candidate) candidates.push(candidate);
        } else if (ts.isIdentifier(initializer)) {
          collectSymbol(checker.getSymbolAtLocation(initializer));
        } else if (ts.isPropertyAccessExpression(initializer)) {
          collectSymbol(checker.getSymbolAtLocation(initializer.name));
        }
      }
    }
  };
  collectSymbol(symbol);
  return [...new Map(candidates.map((candidate) => [candidate.id, candidate])).values()].sort(
    (left, right) => left.id.localeCompare(right.id),
  );
}

function lexicalCandidates(record, expression, name) {
  const candidates = functionCandidatesForSymbol(checker.getSymbolAtLocation(expression));
  if (candidates.length > 0) {
    return candidates;
  }

  let scope = record;
  while (scope) {
    const candidatesAtScope = lexicalBindings.get(scope.id)?.get(name);
    if (candidatesAtScope?.length) return [...candidatesAtScope];
    scope = scope.lexicalOwner ? recordById.get(scope.lexicalOwner) : null;
  }
  return [...(lexicalBindings.get("<top>")?.get(name) ?? [])];
}

function propertyCallName(expression) {
  if (ts.isPropertyAccessExpression(expression)) return expression.name.text;
  if (
    ts.isElementAccessExpression(expression) &&
    expression.argumentExpression &&
    ts.isStringLiteralLike(expression.argumentExpression)
  ) {
    return expression.argumentExpression.text;
  }
  return null;
}

function sourcePathForDeclaration(declaration) {
  return path.relative(workspace, declaration.getSourceFile().fileName).replaceAll("\\", "/");
}

function symbolDeclarationKinds(symbol) {
  return [...new Set((symbol?.declarations ?? []).map((declaration) => ts.SyntaxKind[declaration.kind]))]
    .sort();
}

function externalDeclarationPaths(symbol) {
  return [
    ...new Set(
      (symbol?.declarations ?? [])
        .filter((declaration) => declaration.getSourceFile() !== source)
        .map(sourcePathForDeclaration),
    ),
  ].sort();
}

function callSite(expression) {
  const start = position(expression.getStart(source));
  const text = expression.getText(source);
  return {
    expression: text,
    expression_sha256: sha256(text),
    line: start.line,
    character: start.character,
  };
}

function identifierDisposition(expression) {
  const site = callSite(expression);
  const symbol = checker.getSymbolAtLocation(expression);
  const declarationKinds = symbolDeclarationKinds(symbol);
  const libraryPaths = externalDeclarationPaths(symbol);
  if (libraryPaths.length > 0) {
    return {
      ...site,
      kind: "identifier",
      resolution: "runtime-library",
      symbol_declaration_kinds: declarationKinds,
      library_paths: libraryPaths,
    };
  }
  if (declarationKinds.length === 1 && declarationKinds[0] === "Parameter") {
    return {
      ...site,
      kind: "identifier",
      resolution: "parameter-callback",
      symbol_declaration_kinds: declarationKinds,
      library_paths: [],
    };
  }
  if (declarationKinds.length === 1 && declarationKinds[0] === "BindingElement") {
    return {
      ...site,
      kind: "identifier",
      resolution: "destructured-callback",
      symbol_declaration_kinds: declarationKinds,
      library_paths: [],
    };
  }
  if (declarationKinds.length === 1 && declarationKinds[0] === "VariableDeclaration") {
    return {
      ...site,
      kind: "identifier",
      resolution: "source-value-call",
      symbol_declaration_kinds: declarationKinds,
      library_paths: [],
    };
  }
  if (declarationKinds.length === 0) {
    return {
      ...site,
      kind: "identifier",
      resolution: "external-global",
      symbol_declaration_kinds: [],
      library_paths: [],
    };
  }
  throw new Error(
    `unreviewed identifier call ${expression.text} at ${site.line}:${site.character} (${declarationKinds.join(",") || "no-symbol"})`,
  );
}

function propertyDisposition(expression, property) {
  const site = callSite(expression);
  const location = ts.isPropertyAccessExpression(expression)
    ? expression.name
    : expression.argumentExpression;
  let symbol;
  let checkerState = "resolved";
  try {
    symbol = checker.getSymbolAtLocation(location);
  } catch (error) {
    if (!(error instanceof RangeError)) throw error;
    checkerState = "stack-overflow";
  }
  const candidates = functionCandidatesForSymbol(symbol);
  const libraryPaths = externalDeclarationPaths(symbol);
  let resolution;
  if (candidates.length > 0) {
    resolution = "source-symbol";
  } else if (libraryPaths.length > 0) {
    resolution = "runtime-library";
  } else {
    resolution = "structural-dispatch";
    if (checkerState === "resolved") checkerState = symbol ? "source-symbol-unfollowed" : "absent";
  }
  return {
    disposition: {
      ...site,
      kind: "property",
      property,
      receiver: expression.expression.getText(source),
      resolution,
      checker_state: checkerState,
      symbol_declaration_kinds: symbolDeclarationKinds(symbol),
      library_paths: libraryPaths,
      source_callees: candidates.map((candidate) => candidate.id),
      candidate_set: resolution === "structural-dispatch" ? property : null,
    },
    candidates,
  };
}

function unwrapParentheses(expression) {
  let current = expression;
  while (ts.isParenthesizedExpression(current)) current = current.expression;
  return current;
}

function dynamicExpressionCandidates(record, expression) {
  const current = unwrapParentheses(expression);
  if (ts.isFunctionLike(current)) {
    const candidate = recordByNode.get(current);
    return candidate ? [candidate] : [];
  }
  if (ts.isIdentifier(current)) {
    return lexicalCandidates(record, current, current.text);
  }
  if (ts.isConditionalExpression(current)) {
    return [
      ...dynamicExpressionCandidates(record, current.whenTrue),
      ...dynamicExpressionCandidates(record, current.whenFalse),
    ];
  }
  if (
    ts.isBinaryExpression(current) &&
    (current.operatorToken.kind === ts.SyntaxKind.BarBarToken ||
      current.operatorToken.kind === ts.SyntaxKind.QuestionQuestionToken)
  ) {
    return [
      ...dynamicExpressionCandidates(record, current.left),
      ...dynamicExpressionCandidates(record, current.right),
    ];
  }
  return [];
}

function dynamicDisposition(record, expression) {
  const site = callSite(expression);
  const current = unwrapParentheses(expression);
  const candidates = [
    ...new Map(
      dynamicExpressionCandidates(record, expression).map((candidate) => [candidate.id, candidate]),
    ).values(),
  ].sort((left, right) => left.id.localeCompare(right.id));
  let resolution = "computed-expression";
  if (candidates.length > 0) resolution = "source-expression";
  else if (ts.isElementAccessExpression(current)) resolution = "computed-element";
  else if (ts.isCallExpression(current)) resolution = "call-result";
  return {
    disposition: {
      ...site,
      kind: "dynamic",
      expression_kind: ts.SyntaxKind[current.kind],
      resolution,
      source_callees: candidates.map((candidate) => candidate.id),
    },
    candidates,
  };
}

function scanDeclaration(node, record, root = false) {
  if (!root && ts.isFunctionLike(node)) return;

  if (ts.isCallExpression(node)) {
    record.callSites += 1;
    const dispositionCount = record.callDispositions.length;
    const callStart = position(node.expression.getStart(source));
    const edges = [];
    if (ts.isIdentifier(node.expression)) {
      for (const candidate of lexicalCandidates(record, node.expression, node.expression.text)) {
        edges.push({ callee: candidate.id, kind: "lexical" });
      }
      if (edges.length === 0) {
        record.callDispositions.push(identifierDisposition(node.expression));
      }
    } else if (ts.isFunctionLike(node.expression)) {
      const candidate = recordByNode.get(node.expression);
      if (candidate) edges.push({ callee: candidate.id, kind: "immediate" });
    } else {
      const property = propertyCallName(node.expression);
      if (property !== null) {
        const resolved = propertyDisposition(node.expression, property);
        record.callDispositions.push(resolved.disposition);
        if (resolved.disposition.resolution === "source-symbol") {
          for (const candidate of resolved.candidates) {
            edges.push({ callee: candidate.id, kind: "property-symbol" });
          }
        }
      } else {
        const resolved = dynamicDisposition(record, node.expression);
        record.callDispositions.push(resolved.disposition);
        for (const candidate of resolved.candidates) {
          edges.push({ callee: candidate.id, kind: "dynamic-symbol" });
        }
      }
    }
    for (const edge of edges) {
      record.rawCalls.push({ ...edge, line: callStart.line, character: callStart.character });
    }
    if (edges.length === 0 && record.callDispositions.length === dispositionCount) {
      throw new Error(`unclassified call at ${callStart.line}:${callStart.character}`);
    }
    record.classifiedCallSites += 1;
  }

  ts.forEachChild(node, (child) => scanDeclaration(child, record));
}

for (const record of records) scanDeclaration(record.node, record, true);

// Inline callbacks and local function declarations are values before they are
// calls. A direct CallExpression-only graph would therefore lose, for example,
// Program.emit -> cancellation callback -> emitWorker. Treat every immediate
// nested declaration as a conservative owner edge; later H1.0a disposition can
// narrow a callback only with a reviewed call-site proof.
for (const record of records) {
  if (!record.lexicalOwner) continue;
  const owner = recordById.get(record.lexicalOwner);
  owner.rawCalls.push({
    callee: record.id,
    kind: "nested-function",
    line: record.sourceRange.start.line,
    character: record.sourceRange.start.character,
  });
}

for (const record of records) {
  const calls = new Map();
  for (const call of record.rawCalls) {
    const key = `${call.kind}\0${call.callee}`;
    let entry = calls.get(key);
    if (!entry) {
      entry = { callee: call.callee, kind: call.kind, sites: [] };
      calls.set(key, entry);
    }
    entry.sites.push({ line: call.line, character: call.character });
  }
  record.callees = [...calls.values()]
    .map((entry) => ({
      ...entry,
      sites: entry.sites.sort(
        (left, right) => left.line - right.line || left.character - right.character,
      ),
    }))
    .sort(
      (left, right) =>
        left.callee.localeCompare(right.callee) || left.kind.localeCompare(right.kind),
    );
  delete record.rawCalls;
}

function selectSpec(spec) {
  const matches = records.filter((record) => {
    if (record.name !== spec.name || record.sourceRange.start.line !== spec.line) return false;
    if (!spec.owner) return true;
    const owner = record.lexicalOwner ? recordById.get(record.lexicalOwner) : null;
    return owner?.name === spec.owner;
  });
  if (matches.length !== 1) {
    throw new Error(
      `expected one ${spec.name} declaration at line ${spec.line}, found ${matches.length}`,
    );
  }
  return matches[0];
}

const activeRoots = activeRootSpecs.map((spec) => ({ spec, record: selectSpec(spec) }));
const dormantSeams = dormantSeamSpecs.map((spec) => ({ spec, record: selectSpec(spec) }));

const closureIds = new Set(activeRoots.map(({ record }) => record.id));
const worklist = activeRoots.map(({ record }) => record);
while (worklist.length > 0) {
  const record = worklist.pop();
  for (const edge of record.callees) {
    if (closureIds.has(edge.callee)) continue;
    closureIds.add(edge.callee);
    worklist.push(recordById.get(edge.callee));
  }
}

const closure = records
  .filter((record) => closureIds.has(record.id))
  .sort((left, right) => left.id.localeCompare(right.id));

const shortestPaths = new Map();
const queue = activeRoots
  .map(({ spec, record }) => ({ record, path: [record.id], root: spec.key }))
  .sort((left, right) => left.root.localeCompare(right.root));
for (const entry of queue) {
  const previous = shortestPaths.get(entry.record.id);
  if (!previous || entry.root.localeCompare(previous.root) < 0) {
    shortestPaths.set(entry.record.id, { root: entry.root, path: entry.path });
  }
}
for (let cursor = 0; cursor < queue.length; cursor += 1) {
  const entry = queue[cursor];
  const selected = shortestPaths.get(entry.record.id);
  if (selected.root !== entry.root || selected.path.length !== entry.path.length) continue;
  for (const edge of entry.record.callees.filter((edge) => closureIds.has(edge.callee))) {
    const candidate = { root: entry.root, path: [...entry.path, edge.callee] };
    const previous = shortestPaths.get(edge.callee);
    const wins =
      !previous ||
      candidate.path.length < previous.path.length ||
      (candidate.path.length === previous.path.length && candidate.root < previous.root) ||
      (candidate.path.length === previous.path.length &&
        candidate.root === previous.root &&
        candidate.path.join("\0") < previous.path.join("\0"));
    if (wins) {
      shortestPaths.set(edge.callee, candidate);
      queue.push({ record: recordById.get(edge.callee), ...candidate });
    }
  }
}

const reachableRoots = new Map(closure.map((record) => [record.id, []]));
for (const { spec, record: root } of activeRoots) {
  const seen = new Set([root.id]);
  const pending = [root];
  while (pending.length > 0) {
    const record = pending.pop();
    reachableRoots.get(record.id).push(spec.key);
    for (const edge of record.callees) {
      if (!closureIds.has(edge.callee) || seen.has(edge.callee)) continue;
      seen.add(edge.callee);
      pending.push(recordById.get(edge.callee));
    }
  }
}

const graphEdges = closure.flatMap((record) =>
  record.callees
    .filter((edge) => closureIds.has(edge.callee))
    .map((edge) => ({ caller: record.id, ...edge })),
);
const callDispositions = closure
  .flatMap((record) =>
    record.callDispositions.map((disposition) => ({ caller: record.id, ...disposition })),
  )
  .sort(
    (left, right) =>
      left.caller.localeCompare(right.caller) ||
      left.line - right.line ||
      left.character - right.character ||
      left.expression.localeCompare(right.expression),
  );

function propertyCandidateReference(record) {
  return {
    id: record.id,
    name: record.name,
    kind: record.kind,
    lexical_owner: record.lexicalOwner,
    line: record.sourceRange.start.line,
    character: record.sourceRange.start.character,
    declaration_sha256: record.declarationSha256,
  };
}

const structuralProperties = [
  ...new Set(
    callDispositions
      .filter(
        (disposition) =>
          disposition.kind === "property" &&
          disposition.resolution === "structural-dispatch",
      )
      .map((disposition) => disposition.property),
  ),
].sort();
const propertyCandidateSets = structuralProperties.map((property) => ({
  property,
  candidates: (aliasCandidates.get(property) ?? []).map(propertyCandidateReference),
}));

function dispositionCount(kind, resolution) {
  return callDispositions.filter(
    (disposition) =>
      (kind === undefined || disposition.kind === kind) &&
      (resolution === undefined || disposition.resolution === resolution),
  ).length;
}

function declarationRecord(record) {
  return {
    id: record.id,
    name: record.name,
    kind: record.kind,
    lexical_owner: record.lexicalOwner,
    lexical_path: record.lexicalPath,
    source_range: record.sourceRange,
    declaration_sha256: record.declarationSha256,
    body_range: record.bodyRange,
    body_sha256: record.bodySha256,
    ledger_slice_sha256: record.ledgerSliceSha256,
  };
}

const functions = closure.map((record) => {
  const shortest = shortestPaths.get(record.id);
  if (!shortest) throw new Error(`reachable declaration ${record.id} has no root path`);
  return {
    ...declarationRecord(record),
    reachable_from: reachableRoots.get(record.id).sort(),
    shortest_root_path: { root: shortest.root, declarations: shortest.path },
  };
});

const summary = {
  source_declarations: records.length,
  active_roots: activeRoots.length,
  dormant_seams: dormantSeams.length,
  closure_declarations: functions.length,
  static_edges: graphEdges.length,
  lexical_edges: graphEdges.filter((edge) => edge.kind === "lexical").length,
  immediate_edges: graphEdges.filter((edge) => edge.kind === "immediate").length,
  dynamic_symbol_edges: graphEdges.filter((edge) => edge.kind === "dynamic-symbol").length,
  nested_function_edges: graphEdges.filter((edge) => edge.kind === "nested-function").length,
  property_symbol_edges: graphEdges.filter((edge) => edge.kind === "property-symbol").length,
  call_sites: closure.reduce((total, record) => total + record.callSites, 0),
  reviewed_call_sites: callDispositions.length,
  identifier_runtime_library_calls: dispositionCount("identifier", "runtime-library"),
  identifier_parameter_callback_calls: dispositionCount("identifier", "parameter-callback"),
  identifier_destructured_callback_calls: dispositionCount(
    "identifier",
    "destructured-callback",
  ),
  identifier_source_value_calls: dispositionCount("identifier", "source-value-call"),
  identifier_external_global_calls: dispositionCount("identifier", "external-global"),
  property_source_symbol_calls: dispositionCount("property", "source-symbol"),
  property_runtime_library_calls: dispositionCount("property", "runtime-library"),
  property_structural_dispatch_calls: dispositionCount("property", "structural-dispatch"),
  property_checker_stack_overflow_calls: callDispositions.filter(
    (disposition) =>
      disposition.kind === "property" && disposition.checker_state === "stack-overflow",
  ).length,
  dynamic_expression_calls: dispositionCount("dynamic", undefined),
  dynamic_source_expression_calls: dispositionCount("dynamic", "source-expression"),
  reviewed_exact_edge_calls:
    dispositionCount("property", "source-symbol") +
    dispositionCount("dynamic", "source-expression"),
  reviewed_non_edge_calls:
    callDispositions.length -
    dispositionCount("property", "source-symbol") -
    dispositionCount("dynamic", "source-expression"),
  property_candidate_sets: propertyCandidateSets.length,
  property_candidate_declarations: propertyCandidateSets.reduce(
    (total, set) => total + set.candidates.length,
    0,
  ),
  undispositioned_calls: closure.reduce(
    (total, record) => total + record.callSites - record.classifiedCallSites,
    0,
  ),
};
requireJsonEqual(summary, EXPECTED_SUMMARY, "H1 reviewed owner summary");

const output = {
  schema: 2,
  status: "draft/report-only",
  phase: "H1.0a-reviewed-owner-graph",
  typescript: {
    version: ts.version,
    source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
    source: sourceRelativePath,
    source_sha256: expectedSourceSha256,
  },
  generator: "crates/oracle/h1-owner-inventory.mjs",
  contract: pathHash(contractRelativePath),
  identity:
    "sha256(lexical owner path + declaration kind + name-or-anonymous + UTF-16 start/end offsets + exact declaration SHA-256)",
  ledger_hash:
    "SHA-256 of inclusive complete source lines, compatible with tsc-span/tsc-hash ledger verification",
  closure_model:
    "exact lexical and source-symbol calls plus conservative immediate nested-function ownership; runtime-library, callback/value, structural-property, and computed calls have explicit non-edge dispositions",
  call_review_contract: {
    exact_edges:
      "lexical declarations, followed shorthand/property aliases, and statically selected callable expressions become graph edges",
    runtime_library:
      "symbols declared by vendored default-library files are recorded as runtime-library calls and never mapped to same-name _tsc.js declarations",
    callback_and_value_calls:
      "parameter, binding-element, and source variable callees remain explicit callback/value seams without guessed concrete graph edges",
    structural_property_dispatch:
      "a property without one followed source symbol remains a receiver-qualified structural dispatch; same-name source declarations are review candidates, not graph edges",
    dynamic_expressions:
      "computed elements and call results remain explicit dynamic seams; conditional/logical/function expressions add only statically found source callees",
    candidate_sets:
      "each structural property names its complete distinct same-name _tsc.js declaration set, including an explicit empty set",
    unresolved_state: "none",
  },
  active_roots: activeRoots.map(({ spec, record }) => ({
    key: spec.key,
    declaration: declarationRecord(record),
  })),
  dormant_seams: dormantSeams.map(({ spec, record }) => ({
    axis: spec.axis,
    role: spec.role,
    declaration: declarationRecord(record),
  })),
  summary,
  completed_h1_0a: [
    "freeze the exact bootstrap option/syntax/output profile",
    "land callback-level in-memory oracle observations and schemas",
    "record every current emit-only Rust omission",
    "pin the complete upstream transpile source tree and runner identity without execution results",
    "pin the complete upstream FourSlash tree identity and classify the 38 direct emit-operation witnesses without execution results",
    "reproduce and classify all 37 transpile runner rows without execution or reference-baseline results",
    "pin the complete upstream conformance source tree without adding expansion or execution results",
    "reconstruct all 7,697 conformance runner rows and 46,182 observation states without execution or reference-baseline results",
    "classify all 7,697 conformance runner rows by exact effective bootstrap options, proving zero admissions while retaining every row not-run",
    "classify all 7,276 compiler runner rows by exact effective bootstrap options and reached fixture sources, retaining one bootstrap candidate and every row not-run",
    "classify all 632 project runner rows by exact roots and effective bootstrap options, proving zero admissions while retaining every row not-run",
    "classify all 38 projected FourSlash emit-operation witnesses by exact targeted Language Service route and effective bootstrap options, proving zero promotions while retaining every row not-run",
    "replace property-name fan-out with exact source-symbol edges and disposition every runtime-library, callback/value, structural-property, and computed call without unresolved rows",
  ],
  pending_h1_0a: [],
  functions,
  graph: {
    edges: graphEdges,
    call_dispositions: callDispositions,
    property_candidate_sets: propertyCandidateSets,
    unresolved_calls: [],
  },
};

const functionIds = new Set(functions.map((record) => record.id));
if (functionIds.size !== functions.length) throw new Error("H1 declaration identities are not unique");
for (const edge of graphEdges) {
  if (!functionIds.has(edge.caller) || !functionIds.has(edge.callee)) {
    throw new Error("H1 closure contains an edge outside the generated function set");
  }
}
for (const { record } of activeRoots) {
  if (!functionIds.has(record.id)) throw new Error(`active root ${record.name} left the closure`);
}
for (const record of closure) {
  if (record.callSites !== record.classifiedCallSites) {
    throw new Error(`owner ${record.id} has an undispositioned call site`);
  }
}
const dispositionSites = new Set();
const candidateSetByProperty = new Map(
  propertyCandidateSets.map((set) => [set.property, set]),
);
for (const disposition of callDispositions) {
  if (!functionIds.has(disposition.caller)) {
    throw new Error(`call disposition caller ${disposition.caller} left the closure`);
  }
  // Nested calls can share an expression start (for example
  // `emitHelpers().createHelper(...)`). Include the exact callee expression
  // identity so both reviewed call sites remain independently pinned.
  const site = `${disposition.caller}\0${disposition.line}\0${disposition.character}\0${disposition.expression_sha256}`;
  if (dispositionSites.has(site)) throw new Error(`duplicate call disposition site ${site}`);
  dispositionSites.add(site);
  if (sha256(disposition.expression) !== disposition.expression_sha256) {
    throw new Error(`call disposition expression hash changed at ${site}`);
  }
  for (const callee of disposition.source_callees ?? []) {
    if (!functionIds.has(callee)) {
      throw new Error(`call disposition callee ${callee} left the closure`);
    }
  }
  if (disposition.kind === "identifier") {
    if (
      disposition.resolution === "runtime-library" &&
      disposition.library_paths.length === 0
    ) {
      throw new Error(`runtime identifier ${site} lost its library declaration`);
    }
    if (
      disposition.resolution !== "runtime-library" &&
      disposition.library_paths.length !== 0
    ) {
      throw new Error(`source identifier ${site} gained a library declaration`);
    }
  } else if (disposition.kind === "property") {
    if (
      disposition.resolution === "source-symbol" &&
      disposition.source_callees.length === 0
    ) {
      throw new Error(`source property ${site} lost its exact callee`);
    }
    if (
      disposition.resolution === "runtime-library" &&
      disposition.library_paths.length === 0
    ) {
      throw new Error(`runtime property ${site} lost its library declaration`);
    }
    if (disposition.resolution === "structural-dispatch") {
      if (
        disposition.candidate_set !== disposition.property ||
        !candidateSetByProperty.has(disposition.property)
      ) {
        throw new Error(`structural property ${site} lost its candidate set`);
      }
    } else if (disposition.candidate_set !== null) {
      throw new Error(`non-structural property ${site} gained a candidate set`);
    }
  } else if (disposition.kind === "dynamic") {
    if (
      disposition.resolution === "source-expression" &&
      disposition.source_callees.length === 0
    ) {
      throw new Error(`source dynamic call ${site} lost its exact callee`);
    }
  } else {
    throw new Error(`unknown call disposition kind ${disposition.kind}`);
  }
}
for (const set of propertyCandidateSets) {
  const ids = new Set();
  for (const candidate of set.candidates) {
    if (!recordById.has(candidate.id) || ids.has(candidate.id)) {
      throw new Error(`invalid ${set.property} property candidate ${candidate.id}`);
    }
    ids.add(candidate.id);
  }
}
if (output.graph.unresolved_calls.length !== 0 || summary.undispositioned_calls !== 0) {
  throw new Error("H1 reviewed owner graph retained an unresolved call");
}
requireJsonEqual(
  callDispositions
    .filter((disposition) => disposition.resolution === "external-global")
    .map((disposition) => [disposition.expression, disposition.line, disposition.character]),
  [["onProfilerEvent", 2582, 7]],
  "external global call canary",
);
requireJsonEqual(
  callDispositions
    .filter((disposition) => disposition.checker_state === "stack-overflow")
    .map((disposition) => [disposition.expression, disposition.line, disposition.character]),
  [["typeElements.push", 52156, 11]],
  "checker stack-overflow call canary",
);
for (const expected of [
  "emitWorker",
  "getScriptTransformers",
  "getSourceFilesToEmit",
  "sourceFileMayBeEmitted",
  "getOutputExtension",
]) {
  if (!functions.some((record) => record.name === expected)) {
    throw new Error(`H1 active closure is missing required owner ${expected}`);
  }
}
if (functions.some((record) => record.name === "getOutputJSFileName")) {
  throw new Error("same-name property fan-out reintroduced inactive getOutputJSFileName");
}

const rendered = `${JSON.stringify(output, null, 2)}\n`;
const mode = process.argv[2];
if (mode === "--write") {
  fs.writeFileSync(targetPath, rendered);
  process.stdout.write(`wrote ${targetRelativePath}\n`);
} else if (mode === "--check") {
  if (!fs.existsSync(targetPath) || fs.readFileSync(targetPath, "utf8") !== rendered) {
    throw new Error(`stale ${targetRelativePath}; run with --write and review`);
  }
  process.stdout.write(
    `H1 owner inventory is fresh: roots=${output.summary.active_roots} closure=${output.summary.closure_declarations} reviewed=${output.summary.reviewed_call_sites} undispositioned=0\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  throw new Error("usage: h1-owner-inventory.mjs [--write|--check]");
}
