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

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
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
    unresolvedCalls: [],
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

function lexicalCandidates(record, expression, name) {
  const candidates = [];
  const seenSymbols = new Set();
  const collectSymbol = (symbol) => {
    if (!symbol || seenSymbols.has(symbol)) return;
    seenSymbols.add(symbol);
    for (const declaration of symbol.declarations ?? []) {
      if (ts.isFunctionLike(declaration)) {
        const candidate = recordByNode.get(declaration);
        if (candidate) candidates.push(candidate);
      } else if (ts.isVariableDeclaration(declaration) && declaration.initializer) {
        if (ts.isFunctionLike(declaration.initializer)) {
          const candidate = recordByNode.get(declaration.initializer);
          if (candidate) candidates.push(candidate);
        } else if (ts.isIdentifier(declaration.initializer)) {
          collectSymbol(checker.getSymbolAtLocation(declaration.initializer));
        }
      }
    }
  };
  collectSymbol(checker.getSymbolAtLocation(expression));
  if (candidates.length > 0) {
    return [...new Map(candidates.map((candidate) => [candidate.id, candidate])).values()].sort(
      (left, right) => left.id.localeCompare(right.id),
    );
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

function scanDeclaration(node, record, root = false) {
  if (!root && ts.isFunctionLike(node)) return;

  if (ts.isCallExpression(node)) {
    const callStart = position(node.expression.getStart(source));
    const edges = [];
    if (ts.isIdentifier(node.expression)) {
      for (const candidate of lexicalCandidates(record, node.expression, node.expression.text)) {
        edges.push({ callee: candidate.id, kind: "lexical" });
      }
      if (edges.length === 0) {
        record.unresolvedCalls.push({
          expression: node.expression.text,
          kind: "identifier",
          line: callStart.line,
          character: callStart.character,
        });
      }
    } else if (ts.isFunctionLike(node.expression)) {
      const candidate = recordByNode.get(node.expression);
      if (candidate) edges.push({ callee: candidate.id, kind: "immediate" });
    } else {
      const property = propertyCallName(node.expression);
      if (property !== null) {
        for (const candidate of aliasCandidates.get(property) ?? []) {
          edges.push({ callee: candidate.id, kind: "property-candidate" });
        }
        if (edges.length === 0) {
          record.unresolvedCalls.push({
            expression: property,
            kind: "property",
            line: callStart.line,
            character: callStart.character,
          });
        }
      }
    }
    for (const edge of edges) {
      record.rawCalls.push({ ...edge, line: callStart.line, character: callStart.character });
    }
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
const unresolvedCalls = closure
  .flatMap((record) =>
    record.unresolvedCalls.map((call) => ({ caller: record.id, ...call })),
  )
  .sort(
    (left, right) =>
      left.caller.localeCompare(right.caller) ||
      left.line - right.line ||
      left.character - right.character ||
      left.expression.localeCompare(right.expression),
  );

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

const output = {
  schema: 1,
  status: "draft/report-only",
  phase: "H1.0a-owner-graph",
  typescript: {
    version: ts.version,
    source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
    source: sourceRelativePath,
    source_sha256: expectedSourceSha256,
  },
  generator: "crates/oracle/h1-owner-inventory.mjs",
  identity:
    "sha256(lexical owner path + declaration kind + name-or-anonymous + UTF-16 start/end offsets + exact declaration SHA-256)",
  ledger_hash:
    "SHA-256 of inclusive complete source lines, compatible with tsc-span/tsc-hash ledger verification",
  closure_model:
    "exact lexical identifier calls plus conservative immediate nested-function and distinct property-dispatch declaration candidates; unresolved dynamic calls remain explicit",
  active_roots: activeRoots.map(({ spec, record }) => ({
    key: spec.key,
    declaration: declarationRecord(record),
  })),
  dormant_seams: dormantSeams.map(({ spec, record }) => ({
    axis: spec.axis,
    role: spec.role,
    declaration: declarationRecord(record),
  })),
  summary: {
    source_declarations: records.length,
    active_roots: activeRoots.length,
    dormant_seams: dormantSeams.length,
    closure_declarations: functions.length,
    static_edges: graphEdges.length,
    lexical_edges: graphEdges.filter((edge) => edge.kind === "lexical").length,
    immediate_edges: graphEdges.filter((edge) => edge.kind === "immediate").length,
    nested_function_edges: graphEdges.filter((edge) => edge.kind === "nested-function").length,
    property_dispatch_edges: graphEdges.filter((edge) => edge.kind === "property-candidate").length,
    unresolved_calls: unresolvedCalls.length,
  },
  pending_h1_0a: [
    "review and disposition every unresolved/dynamic call and property-dispatch over-approximation",
    "freeze the exact bootstrap option/syntax/output profile",
    "classify compiler/project/conformance/transpile and FourSlash emit inventories",
    "land callback-level in-memory oracle observations and schemas",
    "record every current emit-only Rust omission",
  ],
  functions,
  graph: { edges: graphEdges, unresolved_calls: unresolvedCalls },
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
for (const expected of [
  "emitWorker",
  "getScriptTransformers",
  "getSourceFilesToEmit",
  "sourceFileMayBeEmitted",
  "getOutputJSFileName",
  "getOutputExtension",
]) {
  if (!functions.some((record) => record.name === expected)) {
    throw new Error(`H1 active closure is missing required owner ${expected}`);
  }
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
    `H1 owner inventory is fresh: roots=${output.summary.active_roots} closure=${output.summary.closure_declarations} unresolved=${output.summary.unresolved_calls}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  throw new Error("usage: h1-owner-inventory.mjs [--write|--check]");
}
