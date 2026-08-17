// H2.5h-a prerequisite-transition step 2: the complete pinned owner graph
// of transformES2015/transformGenerators.
//
// Design decisions (validated by scratchpad prototypes, 2026-08-17):
// - owner spans are DERIVED from ratchets/h2-owner-inventory.v1.json and
//   re-validated (declaration hash + position), never duplicated here;
// - reference extraction is parser-exact (pinned ts.createSourceFile over
//   each span; validated prototype owner-graph-parse-proto.mjs);
// - fail-closed receiver guard: create*/update* member calls may only use
//   the factory2 binding or a tracked emitHelpers() alias;
// - shared sub-transform modules (destructuring family, taggedTemplate) are
//   separate graph nodes pinned as transitive bare-call closures;
// - enum numeric-comment references are frozen as exact (value, name) pairs;
// - cross-artifact consistency guard: resolver/helper unions must equal the
//   foundation's direct-control coverage.
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-5h-a-owner-graph.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-5h-a-owner-graph.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-5h-a-owner-graph.schema.json";
const FOUNDATION_RELATIVE_PATH = "ratchets/h2-5h-a-foundation.v1.json";
const WITNESSES_RELATIVE_PATH =
  "ratchets/h2-5h-a-comment-scope-witnesses.v1.json";
const OWNER_INVENTORY_RELATIVE_PATH = "ratchets/h2-owner-inventory.v1.json";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";
const SLICE = "H2.5h-a";

// Expected surface census (validated by prototype 2026-08-17). The
// generator asserts these counts so the artifact cannot silently drift;
// a bundle change fails the mint loudly.
const EXPECTED = Object.freeze({
  transformES2015: Object.freeze({
    local_functions: 171,
    factory_methods: 92,
    resolver_methods: Object.freeze([
      "getReferencedDeclarationWithCollidingName",
      "hasNodeCheckFlag",
      "isArgumentsLocalBinding",
      "isBindingCapturedByNode",
      "isDeclarationWithCollidingName",
    ]),
    helper_calls: Object.freeze([
      "createExtendsHelper",
      "createReadHelper",
      "createSpreadArrayHelper",
      "createValuesHelper",
    ]),
    hooks: Object.freeze(["onEmitNode", "onSubstituteNode"]),
    enum_pairs: 167,
  }),
  transformGenerators: Object.freeze({
    local_functions: 129,
    factory_methods: 41,
    resolver_methods: Object.freeze(["getReferencedValueDeclaration"]),
    helper_calls: Object.freeze([
      "createGeneratorHelper",
      "createValuesHelper",
    ]),
    hooks: Object.freeze(["onSubstituteNode"]),
    enum_pairs: 75,
  }),
});

// Shared sub-transform entry points (module-scope in the bundle). The
// generator locates the definitions, closes over their top-level support
// family, and pins each member.
const SHARED_MODULE_ENTRIES = Object.freeze([
  Object.freeze({
    module_id: "destructuring-flattener",
    entries: Object.freeze([
      "flattenDestructuringAssignment",
      "flattenDestructuringBinding",
    ]),
    reached_from: Object.freeze(["transformES2015"]),
  }),
  Object.freeze({
    module_id: "tagged-template",
    entries: Object.freeze(["processTaggedTemplateExpression"]),
    reached_from: Object.freeze(["transformES2015"]),
  }),
]);

// Census-surface -> architecture-row assignment. Reviewed data verified
// mechanically: every member must appear in the corresponding census set,
// and every row id must exist in the current emitter architecture map.
// Assignment only - applicability dispositions belong to the readiness
// manifest (prerequisite step 5), not this graph.
const ARCHITECTURE_DOC_RELATIVE_PATH =
  "docs/design/greenfield/emitter-architecture.md";
const SURFACE_ROW_ASSIGNMENTS = Object.freeze([
  Object.freeze({
    surface_id: "resolver-collision-capture-queries",
    rows: Object.freeze(["E-RESOLVER-CAPTURE-H", "E-RESOLVER-BASE", "EA-GAP-CAPTURE"]),
    members: Object.freeze({
      resolver: Object.freeze([
        "getReferencedDeclarationWithCollidingName",
        "isDeclarationWithCollidingName",
        "isArgumentsLocalBinding",
        "isBindingCapturedByNode",
        "getReferencedValueDeclaration",
      ]),
    }),
  }),
  Object.freeze({
    surface_id: "resolver-node-check-flags",
    rows: Object.freeze(["E-CHECKER-FACTS-H", "E-CHECKER-FACTS-BASE"]),
    members: Object.freeze({ resolver: Object.freeze(["hasNodeCheckFlag"]) }),
  }),
  Object.freeze({
    surface_id: "factory-construction",
    rows: Object.freeze(["E-ARENA", "EA-GAP-FLAGS"]),
    members: Object.freeze({
      factory: Object.freeze(["createNodeArray", "cloneNode", "createTempVariable"]),
    }),
  }),
  Object.freeze({
    surface_id: "syntax-guards",
    rows: Object.freeze(["E-SYNTAX-FACTS"]),
    members: Object.freeze({
      utilities: Object.freeze(["isIdentifier", "isBindingPattern", "isIterationStatement"]),
    }),
  }),
  Object.freeze({
    surface_id: "helper-factory",
    rows: Object.freeze(["E-HELPERS-BASE", "E-HELPERS-H", "E-HELPERS-PROVENANCE-G"]),
    members: Object.freeze({
      helpers: Object.freeze([
        "createExtendsHelper",
        "createReadHelper",
        "createSpreadArrayHelper",
        "createValuesHelper",
        "createGeneratorHelper",
      ]),
    }),
  }),
  Object.freeze({
    surface_id: "name-generation",
    rows: Object.freeze(["E-NAMES-BASE", "E-NAMES-H"]),
    members: Object.freeze({
      factory: Object.freeze(["createUniqueName", "createLoopVariable", "getGeneratedNameForNode", "getInternalName", "getLocalName"]),
      utilities: Object.freeze(["isGeneratedIdentifier", "isInternalName"]),
    }),
  }),
  Object.freeze({
    surface_id: "lexical-environment",
    rows: Object.freeze(["E-CONTEXT", "EA-GAP-CAPTURE"]),
    members: Object.freeze({
      context: Object.freeze([
        "startLexicalEnvironment",
        "resumeLexicalEnvironment",
        "endLexicalEnvironment",
        "hoistVariableDeclaration",
        "hoistFunctionDeclaration",
      ]),
      factory: Object.freeze(["mergeLexicalEnvironment", "copyStandardPrologue", "copyCustomPrologue"]),
    }),
  }),
  Object.freeze({
    surface_id: "hook-composition",
    rows: Object.freeze(["E-ORDER-H", "EA-GAP-COMPOSITION"]),
    members: Object.freeze({
      context: Object.freeze(["enableSubstitution", "enableEmitNotification"]),
      utilities: Object.freeze(["chainBundle"]),
    }),
  }),
  Object.freeze({
    surface_id: "comment-apis",
    rows: Object.freeze(["E-COMMENTS-H", "E-COMMENT-SCOPE-H"]),
    members: Object.freeze({
      utilities: Object.freeze(["setCommentRange", "moveSyntheticComments", "addSyntheticLeadingComment", "addSyntheticTrailingComment", "getCommentRange"]),
    }),
  }),
  Object.freeze({
    surface_id: "source-map-apis",
    rows: Object.freeze(["EA-GAP-MAPS-DECLS"]),
    members: Object.freeze({
      utilities: Object.freeze(["setSourceMapRange", "setTokenSourceMapRange", "getSourceMapRange"]),
    }),
  }),
  Object.freeze({
    surface_id: "destructuring-module",
    rows: Object.freeze(["EA-GAP-COMPOSITION", "E-CAPTURE-BASE"]),
    members: Object.freeze({
      utilities: Object.freeze(["flattenDestructuringAssignment", "flattenDestructuringBinding"]),
    }),
  }),
  Object.freeze({
    surface_id: "tagged-template-module",
    rows: Object.freeze(["EA-GAP-COMPOSITION", "E-STRINGS"]),
    members: Object.freeze({
      utilities: Object.freeze(["processTaggedTemplateExpression"]),
    }),
  }),
  Object.freeze({
    surface_id: "loop-partition-machinery",
    rows: Object.freeze(["EA-GAP-CAPTURE", "E-CAPTURE-BASE"]),
    members: Object.freeze({
      utilities: Object.freeze(["visitIterationBody", "unwrapInnermostStatementOfLabel", "spanMap"]),
      factory: Object.freeze(["createLoopVariable", "restoreEnclosingLabel"]),
    }),
  }),
  Object.freeze({
    surface_id: "outer-expression-wrappers",
    rows: Object.freeze(["E-POSITIONS", "E-COMMENTS-H"]),
    members: Object.freeze({
      utilities: Object.freeze(["skipOuterExpressions"]),
      factory: Object.freeze(["createPartiallyEmittedExpression", "restoreOuterExpressions"]),
    }),
  }),
  Object.freeze({
    surface_id: "class-lowering-reach",
    rows: Object.freeze(["E-CAPTURE-CLASS-G", "E-NAMES-CLASS-G", "E-METADATA-G-CLASS"]),
    members: Object.freeze({
      utilities: Object.freeze(["getAllAccessorDeclarations", "getFirstConstructorWithBody", "isStatic", "hasStaticModifier"]),
    }),
  }),
  Object.freeze({
    surface_id: "transform-flag-recomputation",
    rows: Object.freeze(["EA-GAP-FLAGS", "E-METADATA-BASE"]),
    members: Object.freeze({
      utilities: Object.freeze(["setEmitFlags", "getEmitFlags", "setOriginalNode", "setTextRange", "nodeIsSynthesized"]),
    }),
  }),
  Object.freeze({
    surface_id: "yield-star-synthesis",
    rows: Object.freeze(["EA-GAP-COMPOSITION", "E-ORDER-H"]),
    members: Object.freeze({
      factory: Object.freeze(["createYieldExpression"]),
    }),
  }),
]);

function verifySurfaceAssignments(analyses, sharedModules) {
  const architectureText = readText(ARCHITECTURE_DOC_RELATIVE_PATH);
  const unionOf = (selector) =>
    new Set(analyses.flatMap(({ analysis }) => selector(analysis)));
  const factoryUnion = unionOf((analysis) => analysis.factory_methods);
  const resolverUnion = unionOf((analysis) => analysis.resolver_methods);
  const helperUnion = unionOf((analysis) => analysis.helper_calls);
  const contextUnion = unionOf((analysis) => analysis.context_apis);
  const utilityUnion = unionOf((analysis) => analysis.external_utilities);
  for (const module of sharedModules) {
    for (const member of module.family) utilityUnion.add(member.name);
  }
  for (const surface of SURFACE_ROW_ASSIGNMENTS) {
    for (const row of surface.rows) {
      requireCondition(
        architectureText.includes("`" + row + "`"),
        `${surface.surface_id} names unknown architecture row ${row}`,
      );
    }
    const buckets = [
      ["factory", factoryUnion],
      ["resolver", resolverUnion],
      ["helpers", helperUnion],
      ["context", contextUnion],
      ["utilities", utilityUnion],
    ];
    for (const [bucket, union] of buckets) {
      for (const member of surface.members[bucket] ?? []) {
        requireCondition(
          union.has(member),
          `${surface.surface_id} member ${member} is not in the observed ${bucket} census`,
        );
      }
    }
  }
  return SURFACE_ROW_ASSIGNMENTS.map((surface) => ({
    surface_id: surface.surface_id,
    rows: [...surface.rows],
    members: Object.fromEntries(
      Object.entries(surface.members).map(([bucket, names]) => [bucket, [...names]]),
    ),
  }));
}

function helperLabel(methodName) {
  return methodName
    .replace(/^create/u, "")
    .replace(/Helper$/u, "")
    .replace(/([a-z0-9])([A-Z])/gu, "$1-$2")
    .toLowerCase()
    .concat("-helper");
}

function validateLineage(analyses) {
  const foundation = readJson(FOUNDATION_RELATIVE_PATH);
  requireCondition(
    foundation.schema === 1 &&
      foundation.kind === "h2-dormant-semantic-foundation" &&
      foundation.status === "frozen-dormant-semantic-foundation" &&
      foundation.phase === SLICE &&
      typeof foundation.foundation_fingerprint_sha256 === "string",
    "H2.5h-a foundation lineage is not closed",
  );
  const witnesses = readJson(WITNESSES_RELATIVE_PATH);
  requireCondition(
    witnesses.schema === 1 &&
      witnesses.kind === "h2-comment-scope-witnesses" &&
      witnesses.status === "frozen-comment-scope-witnesses" &&
      witnesses.phase === SLICE &&
      typeof witnesses.witnesses_fingerprint_sha256 === "string",
    "comment-scope witness lineage is not closed",
  );
  const resolverUnion = [
    ...new Set(analyses.flatMap(({ analysis }) => analysis.resolver_methods)),
  ].sort();
  requireCondition(
    canonical(resolverUnion) ===
      canonical(foundation.control_coverage.resolver_methods),
    "owner resolver union no longer matches the foundation direct-control coverage",
  );
  const helperLabels = [
    ...new Set(
      analyses.flatMap(({ analysis }) => analysis.helper_calls.map(helperLabel)),
    ),
  ].sort();
  for (const label of helperLabels) {
    requireCondition(
      foundation.control_coverage.factory_operations.includes(label),
      `owner helper ${label} is outside the foundation factory-operation coverage`,
    );
  }
  return {
    foundation: {
      artifact: pathHash(FOUNDATION_RELATIVE_PATH),
      foundation_fingerprint_sha256: foundation.foundation_fingerprint_sha256,
    },
    witnesses: {
      artifact: pathHash(WITNESSES_RELATIVE_PATH),
      witnesses_fingerprint_sha256: witnesses.witnesses_fingerprint_sha256,
    },
    upstream_registration:
      foundation.owner_activation_contract.upstream_registration,
    activation_contract: {
      runtime_activation_slice_id:
        foundation.owner_activation_contract.runtime_activation_slice_id,
      activation: foundation.owner_activation_contract.activation,
      activation_mode: foundation.owner_activation_contract.activation_mode,
      ordered_owner_keys: foundation.owner_activation_contract.ordered_owner_keys,
    },
  };
}

function buildCompositionEdges(implementationText, owners, analyses) {
  const es2015 = owners.find((owner) => owner.name === "transformES2015");
  const yieldSites = [];
  let cursor = implementationText.indexOf(
    "factory2.createYieldExpression(",
    es2015.start,
  );
  while (cursor !== -1 && cursor < es2015.end) {
    yieldSites.push(cursor - es2015.start);
    cursor = implementationText.indexOf(
      "factory2.createYieldExpression(",
      cursor + 1,
    );
  }
  requireCondition(
    yieldSites.length === 2,
    `yield* synthesis site census changed: ${yieldSites.length}`,
  );
  const es2015Analysis = analyses.find(
    ({ owner }) => owner.name === "transformES2015",
  ).analysis;
  const generatorsAnalysis = analyses.find(
    ({ owner }) => owner.name === "transformGenerators",
  ).analysis;
  requireCondition(
    canonical(es2015Analysis.hooks) ===
      canonical(["onEmitNode", "onSubstituteNode"]) &&
      canonical(generatorsAnalysis.hooks) === canonical(["onSubstituteNode"]),
    "hook registration census changed",
  );
  return [
    {
      edge_id: "pass-order",
      kind: "registration-order",
      from: "transformES2015",
      to: "transformGenerators",
      evidence:
        "joint upstream registration pushes transformES2015 then transformGenerators for languageVersion < ES2015; ES2015 output is Generators input",
    },
    {
      edge_id: "yield-star-synthesis",
      kind: "synthesized-consumer",
      from: "transformES2015",
      to: "transformGenerators",
      evidence:
        "converted loop bodies containing yield are re-emitted as factory2.createYieldExpression(AsteriskToken, loopCall) and the synthesized yield* is lowered by the Generators state machine",
      owner_relative_offsets: yieldSites,
    },
    {
      edge_id: "substitution-chain",
      kind: "hook-chain",
      from: "transformGenerators",
      to: "transformES2015",
      evidence:
        "both owners save previousOnSubstituteNode and delegate; ES2015 additionally chains previousOnEmitNode (Generators registers substitution only)",
    },
    {
      edge_id: "destructuring-shared-module",
      kind: "shared-module",
      from: "transformES2015",
      to: "destructuring-flattener",
      evidence: "flattenDestructuringAssignment/Binding called from ES2015 only",
    },
    {
      edge_id: "tagged-template-shared-module",
      kind: "shared-module",
      from: "transformES2015",
      to: "tagged-template",
      evidence: "processTaggedTemplateExpression called from ES2015 only",
    },
  ];
}


// --- shared utilities (identical to the witness generator) ---
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

function validateRuntime() {
  const node = readText(".node-version").trim();
  requireCondition(node === EXPECTED_NODE, "unexpected Node pin");
  requireCondition(process.version === `v${node}`, `requires Node ${node}`);
  requireCondition(ts.version === "6.0.3", "unexpected TypeScript runtime");
}

// Derive + revalidate owner spans from the frozen owner inventory.
function loadOwnerSpans(implementationText) {
  const inventory = readJson(OWNER_INVENTORY_RELATIVE_PATH);
  requireCondition(
    inventory.schema === 1 &&
      inventory.phase === "H2.0a-owner-converse-inventory" &&
      inventory.status === "frozen",
    "H2 owner inventory authority is not closed",
  );
  const selected = inventory.owners.filter(
    (owner) => owner.owner_slice === "H2.5h",
  );
  requireCondition(selected.length === 2, "H2.5h owner closure changed");
  return selected.map((owner) => {
    const { start, end } = owner.declaration.source_range;
    const declarationText = implementationText.slice(start.offset, end.offset);
    requireCondition(
      declarationText.startsWith(`function ${owner.declaration.name}(`) &&
        sha256(declarationText) === owner.declaration.declaration_sha256,
      `${owner.declaration.name} pinned declaration bytes changed`,
    );
    return {
      key: owner.key,
      name: owner.declaration.name,
      start: start.offset,
      end: end.offset,
      declaration: owner.declaration,
    };
  });
}

// Parser-exact extraction (validated prototype). Returns the classified
// external reference closure of one owner span.
function analyzeOwnerSpan(name, sliceText) {
  const sourceFile = ts.createSourceFile(
    `${name}.js`,
    sliceText,
    ts.ScriptTarget.ES2022,
    true,
    ts.ScriptKind.JS,
  );
  requireCondition(
    !sourceFile.parseDiagnostics?.length,
    `${name} span does not parse clean`,
  );
  requireCondition(
    sourceFile.statements.length === 1 &&
      ts.isFunctionDeclaration(sourceFile.statements[0]),
    `${name} span is not one function declaration`,
  );
  const owner = sourceFile.statements[0];
  requireCondition(owner.name?.text === name, `${name} owner name changed`);

  const declared = new Set([name]);
  const localFunctions = [];
  (function collect(node) {
    if (ts.isFunctionDeclaration(node) && node.name) {
      declared.add(node.name.text);
      if (node !== owner) {
        localFunctions.push({
          name: node.name.text,
          start: node.getStart(sourceFile, false),
          end: node.end,
          sha256: sha256(sliceText.slice(node.getStart(sourceFile, false), node.end)),
        });
      }
    } else if (ts.isFunctionExpression(node) && node.name) {
      declared.add(node.name.text);
    } else if (
      ts.isVariableDeclaration(node) ||
      ts.isParameter(node) ||
      ts.isBindingElement(node)
    ) {
      if (ts.isIdentifier(node.name)) declared.add(node.name.text);
    } else if (ts.isCatchClause(node) && node.variableDeclaration) {
      if (ts.isIdentifier(node.variableDeclaration.name)) {
        declared.add(node.variableDeclaration.name.text);
      }
    } else if (ts.isClassDeclaration(node) && node.name) {
      declared.add(node.name.text);
    }
    ts.forEachChild(node, collect);
  })(owner);

  const contextDestructure = new Map();
  (function findDestructure(node) {
    if (
      ts.isVariableDeclaration(node) &&
      ts.isObjectBindingPattern(node.name) &&
      node.initializer &&
      ts.isIdentifier(node.initializer) &&
      node.initializer.text === "context"
    ) {
      for (const element of node.name.elements) {
        const local = ts.isIdentifier(element.name) ? element.name.text : null;
        const member =
          element.propertyName && ts.isIdentifier(element.propertyName)
            ? element.propertyName.text
            : local;
        if (local) contextDestructure.set(local, member);
      }
    }
    ts.forEachChild(node, findDestructure);
  })(owner);

  const helperAliases = new Set();
  (function findAliases(node) {
    if (
      ts.isVariableDeclaration(node) &&
      ts.isIdentifier(node.name) &&
      node.initializer &&
      ts.isCallExpression(node.initializer) &&
      ts.isIdentifier(node.initializer.expression) &&
      node.initializer.expression.text === "emitHelpers"
    ) {
      helperAliases.add(node.name.text);
    }
    ts.forEachChild(node, findAliases);
  })(owner);

  const factoryMethods = new Set();
  const resolverMethods = new Set();
  const contextApis = new Set();
  const helperCalls = new Set();
  const bareCalls = new Set();
  const badReceivers = new Set();
  (function walk(node) {
    if (ts.isCallExpression(node)) {
      const callee = node.expression;
      if (
        ts.isPropertyAccessExpression(callee) &&
        ts.isIdentifier(callee.expression)
      ) {
        const recv = callee.expression.text;
        const member = callee.name.text;
        if (recv === "factory2") factoryMethods.add(member);
        else if (recv === "resolver") resolverMethods.add(member);
        else if (recv === "context") contextApis.add(member);
        else if (helperAliases.has(recv)) helperCalls.add(member);
        else if (/^(create|update)[A-Z]/u.test(member)) badReceivers.add(recv);
      } else if (ts.isIdentifier(callee)) {
        const called = callee.text;
        if (contextDestructure.has(called)) {
          contextApis.add(contextDestructure.get(called));
        } else if (!declared.has(called)) {
          bareCalls.add(called);
        }
      }
      if (
        ts.isPropertyAccessExpression(callee) &&
        ts.isCallExpression(callee.expression) &&
        ts.isIdentifier(callee.expression.expression) &&
        callee.expression.expression.text === "emitHelpers"
      ) {
        helperCalls.add(callee.name.text);
      }
    }
    ts.forEachChild(node, walk);
  })(owner);

  // Fail-closed alias guard: no other receiver may perform factory-shaped
  // construction. (Method-name heuristic keeps e.g. Map.set out of scope.)
  requireCondition(
    badReceivers.size === 0,
    `${name} constructs through unknown receivers: ${[...badReceivers].join(", ")}`,
  );

  const enumPairs = [
    ...new Set(
      [...sliceText.matchAll(/(\d+)\s*\/\* ([A-Za-z][A-Za-z0-9]*) \*\//gu)].map(
        (match) => `${match[1]}:${match[2]}`,
      ),
    ),
  ]
    .map((pair) => {
      const [value, enumName] = pair.split(":");
      return { value: Number(value), name: enumName };
    })
    .sort((a, b) => a.value - b.value || a.name.localeCompare(b.name));

  const hooks = [];
  if (contextDestructure.size >= 0) {
    // Hook registrations appear as `context.onEmitNode = onEmitNode;` etc.
    for (const hook of ["onEmitNode", "onSubstituteNode"]) {
      if (
        new RegExp(`context\\.${hook}\\s*=\\s*${hook}`, "u").test(sliceText)
      ) {
        hooks.push(hook);
      }
    }
  }

  return {
    local_functions: localFunctions,
    factory_methods: [...factoryMethods].sort(),
    resolver_methods: [...resolverMethods].sort(),
    context_apis: [...contextApis].sort(),
    context_destructure: [...contextDestructure.entries()]
      .map(([local, member]) => ({ local, member }))
      .sort((a, b) => a.local.localeCompare(b.local)),
    helper_calls: [...helperCalls].sort(),
    external_utilities: [...bareCalls].sort(),
    hooks: hooks.sort(),
    enum_references: enumPairs,
  };
}

// Shared sub-transform modules: locate the top-level entry definitions,
// compute the transitive bare-call closure bounded to the contiguous
// module region, and pin each family member (validated by
// shared-module-closure-proto.mjs: destructuring family = 18 functions,
// tagged-template family = 2, bundle top-level declarations = 2,303).
function topLevelFunctionIndex(implementationText) {
  const index = [];
  for (const match of implementationText.matchAll(
    /^function ([A-Za-z_$][\w$]*)\(/gmu,
  )) {
    index.push({ name: match[1], start: match.index });
  }
  for (let i = 0; i < index.length; i += 1) {
    index[i].end =
      i + 1 < index.length ? index[i + 1].start : implementationText.length;
  }
  return new Map(index.map((entry) => [entry.name, entry]));
}

function bareCallsOfTopLevelFunction(implementationText, entry) {
  const sliceText = implementationText.slice(entry.start, entry.end);
  const sourceFile = ts.createSourceFile(
    `${entry.name}.js`,
    sliceText,
    ts.ScriptTarget.ES2022,
    true,
    ts.ScriptKind.JS,
  );
  const declaration = sourceFile.statements.find(
    (statement) =>
      ts.isFunctionDeclaration(statement) && statement.name?.text === entry.name,
  );
  requireCondition(
    declaration !== undefined,
    `${entry.name} top-level declaration not found in its slice`,
  );
  const declared = new Set();
  (function collect(node) {
    if (
      (ts.isFunctionDeclaration(node) || ts.isFunctionExpression(node)) &&
      node.name
    ) {
      declared.add(node.name.text);
    } else if (
      (ts.isVariableDeclaration(node) ||
        ts.isParameter(node) ||
        ts.isBindingElement(node)) &&
      ts.isIdentifier(node.name)
    ) {
      declared.add(node.name.text);
    }
    ts.forEachChild(node, collect);
  })(declaration);
  const bare = new Set();
  (function walk(node) {
    if (ts.isCallExpression(node) && ts.isIdentifier(node.expression)) {
      if (!declared.has(node.expression.text)) bare.add(node.expression.text);
    }
    ts.forEachChild(node, walk);
  })(declaration);
  return {
    bare,
    declarationEnd: entry.start + declaration.end,
    sliceSha256: sha256(
      implementationText.slice(entry.start, entry.start + declaration.end),
    ),
  };
}

function buildSharedModules(implementationText, ownerAnalyses) {
  const index = topLevelFunctionIndex(implementationText);
  const regionStart = index.get("flattenDestructuringAssignment")?.start;
  const regionEnd = index.get("processTaggedTemplateExpression")?.end;
  requireCondition(
    typeof regionStart === "number" && typeof regionEnd === "number",
    "shared sub-transform region anchors disappeared",
  );
  return SHARED_MODULE_ENTRIES.map((moduleSpec) => {
    const family = new Map();
    const externalEdges = new Set();
    const queue = [...moduleSpec.entries];
    while (queue.length > 0) {
      const name = queue.shift();
      if (family.has(name)) continue;
      const entry = index.get(name);
      if (
        entry === undefined ||
        entry.start < regionStart ||
        entry.start > regionEnd
      ) {
        externalEdges.add(name);
        continue;
      }
      const { bare, declarationEnd, sliceSha256 } =
        bareCallsOfTopLevelFunction(implementationText, entry);
      family.set(name, {
        name,
        start_offset: entry.start,
        end_offset: declarationEnd,
        declaration_sha256: sliceSha256,
      });
      for (const callee of bare) {
        if (index.has(callee)) queue.push(callee);
        else externalEdges.add(callee);
      }
    }
    for (const owner of moduleSpec.reached_from) {
      const analysis = ownerAnalyses.find((entry) => entry.owner.name === owner);
      requireCondition(
        analysis !== undefined &&
          moduleSpec.entries.every((name) =>
            analysis.analysis.external_utilities.includes(name),
          ),
        `${moduleSpec.module_id} is not reached from ${owner}`,
      );
    }
    return {
      module_id: moduleSpec.module_id,
      entries: [...moduleSpec.entries],
      reached_from: [...moduleSpec.reached_from],
      family: [...family.values()].sort(
        (left, right) => left.start_offset - right.start_offset,
      ),
      external_edges: [...externalEdges].sort(),
    };
  });
}
// TODO(next train): architecture-row assignment table (census surface ->
// E-*/EA-GAP-* row) + cross-check against emitter-architecture.md row ids.
// TODO(next train): foundation/witnesses lineage validation + coverage
// consistency guards (resolver/helper unions == foundation coverage).
// TODO(next train): artifact assembly + --write/--check + subset schema +
// ARTIFACT_SCHEMA_CONTRACTS row 6 + qualification test row.

function buildArtifact() {
  validateRuntime();
  const implementationText = readText(TYPESCRIPT_IMPLEMENTATION);
  const owners = loadOwnerSpans(implementationText);
  const analyses = owners.map((owner) => {
    const analysis = analyzeOwnerSpan(
      owner.name,
      implementationText.slice(owner.start, owner.end),
    );
    const expected = EXPECTED[owner.name];
    requireCondition(
      analysis.local_functions.length === expected.local_functions &&
        analysis.factory_methods.length === expected.factory_methods &&
        canonical(analysis.resolver_methods) ===
          canonical([...expected.resolver_methods]) &&
        canonical(analysis.helper_calls) ===
          canonical([...expected.helper_calls]) &&
        canonical(analysis.hooks) === canonical([...expected.hooks]) &&
        analysis.enum_references.length === expected.enum_pairs,
      `${owner.name} census drifted from the validated surface`,
    );
    return { owner, analysis };
  });
  const sharedModules = buildSharedModules(implementationText, analyses);
  requireCondition(
    sharedModules.length === 2 &&
      sharedModules[0].family.length === 18 &&
      sharedModules[1].family.length === 2,
    "shared sub-transform families drifted from the validated closure",
  );
  const lineage = validateLineage(analyses);
  const surfaces = verifySurfaceAssignments(analyses, sharedModules);
  const compositionEdges = buildCompositionEdges(
    implementationText,
    owners,
    analyses,
  );
  const ownerRecords = analyses.map(({ owner, analysis }, index) =>
    withFingerprint(
      {
        index,
        key: owner.key,
        name: owner.name,
        declaration: owner.declaration,
        local_functions: analysis.local_functions,
        factory_methods: analysis.factory_methods,
        resolver_methods: analysis.resolver_methods,
        context_apis: analysis.context_apis,
        context_destructure: analysis.context_destructure,
        helper_calls: analysis.helper_calls,
        external_utilities: analysis.external_utilities,
        hooks: analysis.hooks,
        enum_references: analysis.enum_references,
      },
      "owner_fingerprint_sha256",
    ),
  );
  return withFingerprint(
    {
      schema: 1,
      kind: "h2-owner-graph",
      status: "frozen-owner-graph",
      phase: SLICE,
      slice_id: SLICE,
      plan_step: "prerequisite-transition-step-2",
      typescript: {
        version: ts.version,
        source_commit: SOURCE_COMMIT,
        bundle: pathHash(TYPESCRIPT_BUNDLE),
        implementation: pathHash(TYPESCRIPT_IMPLEMENTATION),
      },
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      lineage: {
        foundation: lineage.foundation.artifact,
        foundation_fingerprint_sha256:
          lineage.foundation.foundation_fingerprint_sha256,
        comment_scope_witnesses: lineage.witnesses.artifact,
        witnesses_fingerprint_sha256:
          lineage.witnesses.witnesses_fingerprint_sha256,
        owner_inventory: pathHash(OWNER_INVENTORY_RELATIVE_PATH),
        architecture_map: pathHash(ARCHITECTURE_DOC_RELATIVE_PATH),
        interpretation:
          "prerequisite-transition step 2 freezes the complete pinned owner reference closure; it assigns architecture rows but dispositions nothing, authorizes no production edit, and activates nothing",
      },
      activation_contract: lineage.activation_contract,
      upstream_registration: lineage.upstream_registration,
      owners: ownerRecords,
      shared_modules: sharedModules,
      composition_edges: compositionEdges,
      surface_row_assignments: surfaces,
      summary: {
        owners: ownerRecords.length,
        local_functions: ownerRecords.reduce(
          (sum, owner) => sum + owner.local_functions.length,
          0,
        ),
        factory_methods_distinct: new Set(
          ownerRecords.flatMap((owner) => owner.factory_methods),
        ).size,
        resolver_methods_distinct: new Set(
          ownerRecords.flatMap((owner) => owner.resolver_methods),
        ).size,
        helper_calls_distinct: new Set(
          ownerRecords.flatMap((owner) => owner.helper_calls),
        ).size,
        external_utilities_distinct: new Set(
          ownerRecords.flatMap((owner) => owner.external_utilities),
        ).size,
        enum_reference_pairs: ownerRecords.reduce(
          (sum, owner) => sum + owner.enum_references.length,
          0,
        ),
        shared_modules: sharedModules.length,
        shared_module_functions: sharedModules.reduce(
          (sum, module) => sum + module.family.length,
          0,
        ),
        composition_edges: compositionEdges.length,
        surfaces: surfaces.length,
        rust_runs: 0,
        runtime_admissions_delta: 0,
      },
    },
    "owner_graph_fingerprint_sha256",
  );
}

function render(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

const mode = process.argv[2];
requireCondition(
  mode === "--write" || mode === "--check",
  "usage: h2-5h-a-owner-graph.mjs [--write|--check]",
);
const artifact = buildArtifact();
const rendered = render(artifact);
if (mode === "--write") {
  fs.writeFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), rendered);
  process.stdout.write(
    `wrote ${TARGET_RELATIVE_PATH}: owners=${artifact.summary.owners} surfaces=${artifact.summary.surfaces} edges=${artifact.summary.composition_edges} enums=${artifact.summary.enum_reference_pairs}\n`,
  );
} else {
  requireCondition(
    fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
      fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") ===
        rendered,
    `stale ${TARGET_RELATIVE_PATH}; run h2-5h-a-owner-graph.mjs --write and review`,
  );
  process.stdout.write(
    `H2.5h-a owner graph is fresh: owners=${artifact.summary.owners} surfaces=${artifact.summary.surfaces} edges=${artifact.summary.composition_edges}\n`,
  );
}
