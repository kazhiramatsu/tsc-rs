import crypto from "node:crypto";
import fs from "node:fs";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const bundlePath = process.argv[2];
const band = process.argv[3] ?? "all";
if (!bundlePath || !["all", "2xxx"].includes(band)) {
  throw new Error("usage: emitter-inventory.mjs <_tsc.js> <all|2xxx>");
}

const text = fs.readFileSync(bundlePath, "utf8");
const program = ts.createProgram({
  rootNames: [bundlePath],
  options: {
    allowJs: true,
    checkJs: false,
    noResolve: true,
    target: ts.ScriptTarget.Latest,
  },
});
const source = program.getSourceFile(bundlePath);
if (!source) throw new Error(`program did not load ${bundlePath}`);
const checker = program.getTypeChecker();
const sourceLines = text.split(/(?<=\n)/u);

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

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

function diagnosticAt(node) {
  if (!ts.isPropertyAccessExpression(node)) return null;
  if (!ts.isIdentifier(node.expression) || node.expression.text !== "Diagnostics") return null;
  const membershipRead =
    ts.isPropertyAccessExpression(node.parent) &&
    node.parent.expression === node &&
    node.parent.name.text === "code";
  const diagnostic = ts.Diagnostics[node.name.text];
  return {
    membershipRead,
    name: node.name.text,
    code: diagnostic?.code ?? null,
  };
}

function inBand(code) {
  return band === "all" || (code !== null && code >= 2000 && code < 3000);
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

const records = [];
const recordByNode = new Map();

function createRecord(node, owner, info) {
  const start = position(node.getStart(source));
  const end = position(node.end);
  const kind = ts.SyntaxKind[node.kind];
  const lexicalPath =
    owner === null
      ? "<top>"
      : `${owner.lexical_path}/${info.name}@${start.line}:${start.character}`;
  const sliceHash = sourceSliceHash(start.line, end.line);
  const canonicalIdentity = JSON.stringify({
    lexical_owner_path: owner?.lexical_path ?? null,
    kind,
    name: info.name,
    start: start.offset,
    end: end.offset,
    source_slice_sha256: sliceHash,
  });
  const record = {
    node,
    id: `d2:${sha256(canonicalIdentity)}`,
    name: info.name,
    kind,
    lexical_owner: owner?.id ?? null,
    lexical_path: lexicalPath,
    source_range: { start, end },
    source_slice_sha256: sliceHash,
    lexical_binding: info.lexicalBinding,
    self_binding: info.selfBinding,
    property_alias: info.propertyAlias,
    sites: [],
    rawCalls: [],
    unresolvedCalls: [],
  };
  records.push(record);
  if (node !== source) recordByNode.set(node, record);
  return record;
}

const top = createRecord(source, null, {
  name: "<top>",
  lexicalBinding: null,
  selfBinding: null,
  propertyAlias: null,
});

function collectDeclarations(node, owner) {
  let nextOwner = owner;
  if (node !== source && ts.isFunctionLike(node)) {
    nextOwner = createRecord(node, owner, declarationInfo(node));
  }
  ts.forEachChild(node, (child) => collectDeclarations(child, nextOwner));
}
collectDeclarations(source, top);

const recordById = new Map(records.map((record) => [record.id, record]));
const lexicalBindings = new Map();
const aliasCandidates = new Map();

function addBinding(scopeId, name, record) {
  if (!name) return;
  let scope = lexicalBindings.get(scopeId);
  if (!scope) {
    scope = new Map();
    lexicalBindings.set(scopeId, scope);
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
  candidates.push(record);
}

for (const record of records) {
  addAlias(record.name, record);
  addAlias(record.property_alias, record);
  if (record.lexical_owner && record.lexical_binding) {
    addBinding(record.lexical_owner, record.lexical_binding, record);
  }
  if (record.self_binding) {
    addBinding(record.id, record.self_binding, record);
  }
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
      } else if (
        ts.isVariableDeclaration(declaration) &&
        declaration.initializer
      ) {
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

  // The checker deliberately leaves some dynamically initialized
  // JavaScript bindings unresolved. Preserve a conservative lexical
  // fallback inside the nearest declaration owner for those calls.
  let scope = record;
  while (scope) {
    const candidates = lexicalBindings.get(scope.id)?.get(name);
    if (candidates?.length) {
      return [...candidates].sort((left, right) => left.id.localeCompare(right.id));
    }
    scope = scope.lexical_owner ? recordById.get(scope.lexical_owner) : null;
  }
  return [];
}

let membershipReads = 0;

function scanDeclaration(node, record, root = false) {
  if (!root && ts.isFunctionLike(node)) return;

  const diagnostic = diagnosticAt(node);
  if (diagnostic) {
    if (diagnostic.membershipRead) {
      membershipReads += 1;
    } else if (inBand(diagnostic.code)) {
      const start = position(node.getStart(source));
      record.sites.push({
        id: `diagnostic:${sha256(
          `${record.id}\0${diagnostic.code}\0${diagnostic.name}\0${start.offset}`,
        )}`,
        line: start.line,
        character: start.character,
        offset: start.offset,
        name: diagnostic.name,
        code: diagnostic.code,
      });
    }
  }

  if (ts.isCallExpression(node)) {
    const callStart = position(node.expression.getStart(source));
    const edges = [];
    if (ts.isIdentifier(node.expression)) {
      for (const candidate of lexicalCandidates(
        record,
        node.expression,
        node.expression.text,
      )) {
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
      record.rawCalls.push({
        ...edge,
        line: callStart.line,
        character: callStart.character,
      });
    }
  }

  ts.forEachChild(node, (child) => scanDeclaration(child, record));
}

scanDeclaration(source, top, true);
for (const record of records) {
  if (record !== top) scanDeclaration(record.node, record, true);
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

const directEmitters = records.filter((record) => record.sites.length > 0);
const closureIds = new Set(directEmitters.map((record) => record.id));
const worklist = [...directEmitters];
while (worklist.length > 0) {
  const record = worklist.pop();
  for (const edge of record.callees) {
    if (!closureIds.has(edge.callee)) {
      closureIds.add(edge.callee);
      worklist.push(recordById.get(edge.callee));
    }
  }
}
const closure = records
  .filter((record) => closureIds.has(record.id))
  .sort((left, right) => left.id.localeCompare(right.id));

// Tarjan SCC over exact declaration identities. Property-dispatch
// candidates remain distinct edges, so same-named declarations never
// collapse into one review node.
let nextIndex = 0;
const stack = [];
const onStack = new Set();
const indexes = new Map();
const lowlinks = new Map();
const components = [];

function strongConnect(record) {
  indexes.set(record.id, nextIndex);
  lowlinks.set(record.id, nextIndex);
  nextIndex += 1;
  stack.push(record);
  onStack.add(record.id);

  for (const edge of record.callees) {
    if (!closureIds.has(edge.callee)) continue;
    const callee = recordById.get(edge.callee);
    if (!indexes.has(callee.id)) {
      strongConnect(callee);
      lowlinks.set(record.id, Math.min(lowlinks.get(record.id), lowlinks.get(callee.id)));
    } else if (onStack.has(callee.id)) {
      lowlinks.set(record.id, Math.min(lowlinks.get(record.id), indexes.get(callee.id)));
    }
  }

  if (lowlinks.get(record.id) === indexes.get(record.id)) {
    const component = [];
    while (true) {
      const member = stack.pop();
      onStack.delete(member.id);
      component.push(member.id);
      if (member.id === record.id) break;
    }
    component.sort();
    components.push(component);
  }
}
for (const record of closure) {
  if (!indexes.has(record.id)) strongConnect(record);
}
components.sort((left, right) => left[0].localeCompare(right[0]));
const sccById = new Map();
components.forEach((members, index) => {
  const scc = `scc:${String(index).padStart(5, "0")}`;
  for (const member of members) sccById.set(member, { id: scc, members });
});

// One deterministic shortest path from any direct emitter. Ties are
// resolved by the exact declaration id, making the generated view
// byte-stable while still exposing the full graph for alternate paths.
const shortestPaths = new Map();
const queue = [...directEmitters].sort((left, right) => left.id.localeCompare(right.id));
for (const emitter of queue) shortestPaths.set(emitter.id, [emitter.id]);
for (let cursor = 0; cursor < queue.length; cursor += 1) {
  const record = queue[cursor];
  const path = shortestPaths.get(record.id);
  const edges = record.callees
    .filter((edge) => closureIds.has(edge.callee))
    .sort((left, right) => left.callee.localeCompare(right.callee));
  for (const edge of edges) {
    if (!shortestPaths.has(edge.callee)) {
      shortestPaths.set(edge.callee, [...path, edge.callee]);
      queue.push(recordById.get(edge.callee));
    }
  }
}

const graphEdges = closure.flatMap((record) =>
  record.callees
    .filter((edge) => closureIds.has(edge.callee))
    .map((edge) => ({
      caller: record.id,
      callee: edge.callee,
      kind: edge.kind,
      sites: edge.sites,
    })),
);
const unresolvedCalls = closure.flatMap((record) =>
  record.unresolvedCalls.map((call) => ({ caller: record.id, ...call })),
);
const functions = closure.map((record) => ({
  id: record.id,
  name: record.name,
  kind: record.kind,
  lexical_owner: record.lexical_owner,
  lexical_path: record.lexical_path,
  source_range: record.source_range,
  source_slice_sha256: record.source_slice_sha256,
  direct_emitter: record.sites.length > 0,
  sites: record.sites.sort(
    (left, right) => left.offset - right.offset || left.code - right.code,
  ),
  scc: sccById.get(record.id).id,
  shortest_emitter_path: shortestPaths.get(record.id),
}));

const output = {
  schema: 2,
  status: "draft/report-only",
  typescript_version: "6.0.3",
  source: "vendor/typescript-6.0.3/lib/_tsc.js",
  source_sha256: sha256(text),
  band,
  identity:
    "sha256(lexical owner path + declaration kind + name-or-anonymous + UTF-16 start/end offsets + inclusive source-line-slice SHA-256)",
  closure_model:
    "exact lexical identifier calls plus conservative distinct property-dispatch declaration candidates",
  unavailable: {
    automated_probe_synthesis: "unavailable until B2",
    runtime_trace_coverage: "unavailable until B2",
    document_slice_assignment: "unavailable by contract",
  },
  summary: {
    source_declarations: records.length,
    emitter_declarations: directEmitters.length,
    diagnostic_references: directEmitters.reduce(
      (total, record) => total + record.sites.length,
      0,
    ),
    membership_reads: membershipReads,
    closure_declarations: functions.length,
    sccs: components.length,
    nontrivial_sccs: components.filter((component) => component.length > 1).length,
    static_edges: graphEdges.length,
    property_dispatch_edges: graphEdges.filter(
      (edge) => edge.kind === "property-candidate",
    ).length,
    unresolved_calls: unresolvedCalls.length,
  },
  functions,
  graph: {
    edges: graphEdges,
    sccs: components.map((members, index) => ({
      id: `scc:${String(index).padStart(5, "0")}`,
      members,
    })),
    unresolved_calls: unresolvedCalls.sort(
      (left, right) =>
        left.caller.localeCompare(right.caller) ||
        left.line - right.line ||
        left.character - right.character ||
        left.expression.localeCompare(right.expression),
    ),
  },
};

process.stdout.write(`${JSON.stringify(output, null, 2)}\n`);
