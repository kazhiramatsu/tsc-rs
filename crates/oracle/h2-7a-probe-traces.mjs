// H2.7a m-1: pinned, inert, instrumented declaration-probe observations.
//
// The checked-in edit table below is the sole patch authority. It applies only
// to the byte-pinned TypeScript 6.0.3 `_tsc.js`, writes only below target/, and
// refuses every stale line anchor. Probe observations always run twice in
// separate Node processes and are accepted only when their public outputs are
// byte-identical to the lane-B witness observation.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-7a-probe-traces.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-7a-probe-traces.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-7a-probe-traces.schema.json";
const WITNESSES_RELATIVE_PATH = "ratchets/h2-7a-witnesses.v1.json";
const WITNESSES_CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-7a-witnesses.schema.json";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const TYPESCRIPT_LIB_DIRECTORY = "vendor/typescript-6.0.3/lib";
const TEST_SUITE_EXPANSION =
  "vendor/typescript-6.0.3/test-suite-expansion.v1.json";
const CONFORMANCE_EXPANSION =
  "vendor/typescript-6.0.3/conformance-suite-expansion.v1.json";
const H2_6C_QUALIFICATION = "ratchets/h2-6c-qualification.v1.json";
const VIRTUAL_SOURCE_ROOT = "/.src";
const PROJECT_VIRTUAL_PREFIX = "/.src/tests/cases/projects";
const INSTRUMENTED_RELATIVE_PATH =
  "target/h2-7a-probe/_tsc.instrumented.js";
const POSITION_MAP_RELATIVE_PATH =
  "target/h2-7a-probe/position-map.json";
const OBSERVE_CONTEXT_RELATIVE_PATH =
  "target/h2-7a-probe/observe-context.json";
const CHECK_RECEIPT_RELATIVE_PATH =
  "target/h2-7a-probe/check-receipt.v1.json";
const SELFTEST_WITNESSES_RELATIVE_PATH =
  "target/h2-7a-probe/selftest-witnesses.v1.json";
const SELFTEST_ARTIFACT_RELATIVE_PATH =
  "target/h2-7a-probe/selftest-probe-traces.v1.json";
const EXPECTED_BASE_SHA256 =
  "1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3";
const INTERNAL_OBSERVE_MODE = "--internal-observe-probe";
const PHASE = "H2.7a-probe-traces";

const require = createRequire(import.meta.url);
const publicTs = require(path.join(WORKSPACE, TYPESCRIPT_BUNDLE));

function exactCallProbe({ siteId, startLine, startExpect, endLine, endExpect, indent }) {
  return [
    {
      anchor_line: startLine,
      expect: startExpect,
      insert_after: `${indent}return __h27aProbeCall(${JSON.stringify(siteId)}, arguments, () => { /* site-id: ${siteId}.entry + ${siteId}.result */`,
    },
    {
      anchor_line: endLine,
      expect: endExpect,
      insert_before: `${indent}}); /* site-id: ${siteId}.result */`,
    },
  ];
}

function exactArrowProbe({ siteId, startLine, startExpect, endLine, endExpect, indent, args }) {
  return [
    {
      anchor_line: startLine,
      expect: startExpect,
      insert_after: `${indent}return __h27aProbeCall(${JSON.stringify(siteId)}, [${args}], () => { /* site-id: ${siteId}.entry + ${siteId}.result */`,
    },
    {
      anchor_line: endLine,
      expect: endExpect,
      insert_before: `${indent}}); /* site-id: ${siteId}.result */`,
    },
  ];
}

function exactSyntacticProbe({ siteId, startLine, startExpect, endLine, endExpect }) {
  return [
    {
      anchor_line: startLine,
      expect: startExpect,
      insert_after: `    return __h27aProbeSyntacticCall(${JSON.stringify(siteId)}, arguments, () => { /* site-id: ${siteId}.entry */`,
    },
    {
      anchor_line: endLine,
      expect: endExpect,
      insert_before: `    }); /* site-id: ${siteId}.result */`,
    },
  ];
}

function exactTransformProbe({ siteId, startLine, startExpect, endLine, endExpect }) {
  return [
    {
      anchor_line: startLine,
      expect: startExpect,
      insert_after: `    return __h27aProbeTransform(${JSON.stringify(siteId)}, input, () => { /* site-id: ${siteId}.changed */`,
    },
    {
      anchor_line: endLine,
      expect: endExpect,
      insert_before: `    }); /* site-id: ${siteId}.changed */`,
    },
  ];
}

const PROBE_RUNTIME_LINE = String.raw`const __h27aSyntacticFrames = []; const __h27aTrace = (site, ...args) => { const hook = globalThis.__H2_7A_TRACE__; if (hook) hook(site, ...args); }; const __h27aNode = (value) => value && typeof value.kind === "number" ? value : void 0; const __h27aString = (value) => typeof value === "string" && !value.includes("/") && !value.includes("\\") ? value : ""; const __h27aName = (value) => typeof value === "number" ? String(value) : typeof value === "string" ? __h27aString(value) : value && (typeof value.escapedName === "string" || typeof value.escapedName === "number") ? __h27aString(String(value.escapedName)) : ""; const __h27aScalar = (value) => [typeof value, typeof value === "string" ? __h27aString(value) : "", typeof value === "number" && Number.isFinite(value) ? value : 0, typeof value === "boolean" ? value : false]; const __h27aNodeArgs = (value) => { const node = __h27aNode(value); return node ? [node.kind, Number.isInteger(node.pos) ? node.pos : -1, Number.isInteger(node.end) ? node.end : -1] : [-1, -1, -1]; }; function __h27aProbeCall(site, args, body) { const first = args[0]; const second = args[1]; __h27aTrace(site + ".entry", args.length, __h27aName(first), ...__h27aNodeArgs(first), ...__h27aNodeArgs(second), ...__h27aScalar(first), ...__h27aScalar(second)); const result = body(); const resultNode = __h27aNode(result); const value = result && typeof result === "object" ? result.value : void 0; __h27aTrace(site + ".result", typeof result, result == null, typeof result === "boolean" ? result : false, typeof result === "number" && Number.isFinite(result) ? result : 0, typeof result === "string" ? __h27aString(result) : "", result && typeof result.accessibility === "number" ? result.accessibility : -1, ...__h27aNodeArgs(resultNode), Array.isArray(result) ? result.length : -1, ...__h27aScalar(value), result && typeof result.isSyntacticallyString === "boolean" ? result.isSyntacticallyString : false); return result; } function __h27aProbeSyntacticCall(site, args, body) { const node = args[0]; __h27aTrace(site + ".entry", ...__h27aNodeArgs(node)); const frame = { fallback: false }; __h27aSyntacticFrames.push(frame); let result; try { result = body(); } finally { __h27aSyntacticFrames.pop(); } __h27aTrace(site + ".result", !frame.fallback, frame.fallback, ...__h27aNodeArgs(result)); return result; } function __h27aMarkSyntacticFallback(source, node, reportFallback) { for (const frame of __h27aSyntacticFrames) frame.fallback = true; __h27aTrace(source + ".checkerFallback", !!reportFallback, ...__h27aNodeArgs(node)); } function __h27aProbeTransform(site, input, body) { const output = body(); if (output !== input) { const outputs = Array.isArray(output) ? output : [output]; if (outputs.length === 0) __h27aTrace(site + ".changed", ...__h27aNodeArgs(input), -1, -1, -1, false, 0); for (const candidate of outputs) { const node = __h27aNode(candidate); __h27aTrace(site + ".changed", ...__h27aNodeArgs(input), ...__h27aNodeArgs(node), !!(node && node.original), node && Number.isFinite(node.transformFlags) ? node.transformFlags : 0); } } return output; } /* site-id: probe.runtime */`;

// Generator-owned exact edit table. Every insertion is one physical line and
// carries its trace site-id. The canonical JSON of this expanded array is the
// edit-table hash authority.
const EDIT_TABLE = Object.freeze([
  {
    anchor_line: 17,
    expect: '"use strict";',
    insert_after: PROBE_RUNTIME_LINE,
  },

  // site-id: nodebuilder.moduleSpecifierOverride.*
  {
    anchor_line: 50892,
    expect: "        if (context.bundled || context.enclosingFile !== getSourceFileOfNode(lit)) {",
    insert_before: '        __h27aTrace("nodebuilder.moduleSpecifierOverride.contextArm", context.bundled || context.enclosingFile !== getSourceFileOfNode(lit) ? "rewrite" : "skip", !!context.bundled, context.enclosingFile !== getSourceFileOfNode(lit), ...__h27aNodeArgs(parent)); /* site-id: nodebuilder.moduleSpecifierOverride.contextArm */',
  },
  {
    anchor_line: 50910,
    expect: "          if (parentSymbol && isExternalModuleSymbol(parentSymbol)) {",
    insert_after: '            __h27aTrace("nodebuilder.moduleSpecifierOverride.sourceArm", "parent-symbol", ...__h27aNodeArgs(parent)); /* site-id: nodebuilder.moduleSpecifierOverride.sourceArm */',
  },
  {
    anchor_line: 50914,
    expect: "            if (targetFile) {",
    insert_before: '            __h27aTrace("nodebuilder.moduleSpecifierOverride.sourceArm", targetFile ? "target-file" : "no-target", ...__h27aNodeArgs(parent)); /* site-id: nodebuilder.moduleSpecifierOverride.sourceArm */',
  },
  {
    anchor_line: 50918,
    expect: '          if (name.includes("/node_modules/")) {',
    insert_after: '            __h27aTrace("nodebuilder.moduleSpecifierOverride.unsafe", true, !!nodeSymbol); /* site-id: nodebuilder.moduleSpecifierOverride.unsafe */',
  },
  {
    anchor_line: 50924,
    expect: "          if (name !== originalName) {",
    insert_after: '            __h27aTrace("nodebuilder.moduleSpecifierOverride.resultArm", "override", ...__h27aNodeArgs(parent)); /* site-id: nodebuilder.moduleSpecifierOverride.resultArm */',
  },
  {
    anchor_line: 50927,
    expect: "        }",
    insert_before: '          __h27aTrace("nodebuilder.moduleSpecifierOverride.resultArm", "unchanged", ...__h27aNodeArgs(parent)); /* site-id: nodebuilder.moduleSpecifierOverride.resultArm */',
  },

  // site-id: nodebuilder.withContext.*
  {
    anchor_line: 51249,
    expect: "        context.tracker.reportTruncationError();",
    insert_before: '        __h27aTrace("nodebuilder.withContext.decision", "report-truncation", context.flags, context.internalFlags, context.approximateLength, context.typeStack.length, !!context.out.truncated); /* site-id: nodebuilder.withContext.decision */',
  },
  {
    anchor_line: 51251,
    expect: "      if (out) {",
    insert_after: '        __h27aTrace("nodebuilder.withContext.decision", "copy-out", context.flags, context.internalFlags, context.approximateLength, context.typeStack.length, !!context.out.canIncreaseExpansionDepth, !!context.out.truncated); /* site-id: nodebuilder.withContext.decision */',
  },
  {
    anchor_line: 51255,
    expect: "      return context.encounteredError ? void 0 : resultingNode;",
    insert_before: '      __h27aTrace("nodebuilder.withContext.result", context.encounteredError ? "error" : resultingNode === void 0 ? "fallback-undefined" : "node", context.flags, context.internalFlags, context.approximateLength, context.typeStack.length, !!context.truncating, !!context.out.truncated, !!context.encounteredError, ...__h27aNodeArgs(resultingNode)); /* site-id: nodebuilder.withContext.result */',
  },

  // Resolver-query site ids: exactly the 19 declaration-consumed workers.
  ...exactCallProbe({
    siteId: "resolver.isDefinitelyReferenceToGlobalSymbolObject",
    startLine: 47469,
    startExpect: "  function isDefinitelyReferenceToGlobalSymbolObject(node) {",
    endLine: 47483,
    endExpect: "  }",
    indent: "    ",
  }),
  ...exactCallProbe({
    siteId: "resolver.isSymbolAccessible",
    startLine: 50499,
    startExpect: "  function isSymbolAccessible(symbol, enclosingDeclaration, meaning, shouldComputeAliasesToMakeVisible) {",
    endLine: 50508,
    endExpect: "  }",
    indent: "    ",
  }),
  ...exactCallProbe({
    siteId: "resolver.isEntityNameVisible",
    startLine: 50606,
    startExpect: "  function isEntityNameVisible(entityName, enclosingDeclaration, shouldComputeAliasToMakeVisible = true) {",
    endLine: 50648,
    endExpect: "  }",
    indent: "    ",
  }),
  ...exactCallProbe({
    siteId: "resolver.isDeclarationVisible",
    startLine: 55589,
    startExpect: "  function isDeclarationVisible(node) {",
    endLine: 55674,
    endExpect: "  }",
    indent: "    ",
  }),
  ...exactCallProbe({
    siteId: "resolver.isOptionalParameter",
    startLine: 59509,
    startExpect: "  function isOptionalParameter(node) {",
    endLine: 59527,
    endExpect: "  }",
    indent: "    ",
  }),
  ...exactCallProbe({
    siteId: "resolver.isImplementationOfOverload",
    startLine: 88055,
    startExpect: "  function isImplementationOfOverload(node) {",
    endLine: 88068,
    endExpect: "  }",
    indent: "    ",
  }),
  ...exactCallProbe({
    siteId: "resolver.requiresAddingImplicitUndefined",
    startLine: 88075,
    startExpect: "  function requiresAddingImplicitUndefined(parameter, enclosingDeclaration) {",
    endLine: 88077,
    endExpect: "  }",
    indent: "    ",
  }),
  ...exactCallProbe({
    siteId: "resolver.isExpandoFunctionDeclaration",
    startLine: 88090,
    startExpect: "  function isExpandoFunctionDeclaration(node) {",
    endLine: 88112,
    endExpect: "  }",
    indent: "    ",
  }),
  ...exactCallProbe({
    siteId: "resolver.getPropertiesOfContainerFunction",
    startLine: 88113,
    startExpect: "  function getPropertiesOfContainerFunction(node) {",
    endLine: 88120,
    endExpect: "  }",
    indent: "    ",
  }),
  ...exactCallProbe({
    siteId: "resolver.getEnumMemberValue",
    startLine: 88231,
    startExpect: "  function getEnumMemberValue(node) {",
    endLine: 88237,
    endExpect: "  }",
    indent: "    ",
  }),
  ...exactCallProbe({
    siteId: "resolver.createTypeOfDeclaration",
    startLine: 88359,
    startExpect: "  function createTypeOfDeclaration(declarationIn, enclosingDeclaration, flags, internalFlags, tracker) {",
    endLine: 88366,
    endExpect: "  }",
    indent: "    ",
  }),
  ...exactCallProbe({
    siteId: "resolver.createReturnTypeOfSignatureDeclaration",
    startLine: 88382,
    startExpect: "  function createReturnTypeOfSignatureDeclaration(signatureDeclarationIn, enclosingDeclaration, flags, internalFlags, tracker) {",
    endLine: 88388,
    endExpect: "  }",
    indent: "    ",
  }),
  ...exactCallProbe({
    siteId: "resolver.createTypeOfExpression",
    startLine: 88389,
    startExpect: "  function createTypeOfExpression(exprIn, enclosingDeclaration, flags, internalFlags, tracker) {",
    endLine: 88395,
    endExpect: "  }",
    indent: "    ",
  }),

  // hasGlobalName is an additional internal-decision lane beyond the 19.
  ...exactCallProbe({
    siteId: "resolver.hasGlobalName",
    startLine: 88396,
    startExpect: "  function hasGlobalName(name) {",
    endLine: 88398,
    endExpect: "  }",
    indent: "    ",
  }),
  ...exactCallProbe({
    siteId: "resolver.isLiteralConstDeclaration",
    startLine: 88485,
    startExpect: "  function isLiteralConstDeclaration(node) {",
    endLine: 88490,
    endExpect: "  }",
    indent: "    ",
  }),
  ...exactCallProbe({
    siteId: "resolver.createLiteralConstValue",
    startLine: 88506,
    startExpect: "  function createLiteralConstValue(node, tracker) {",
    endLine: 88509,
    endExpect: "  }",
    indent: "    ",
  }),
  ...exactArrowProbe({
    siteId: "resolver.isLateBound",
    startLine: 88600,
    startExpect: "      isLateBound: (nodeIn) => {",
    endLine: 88604,
    endExpect: "      },",
    indent: "        ",
    args: "nodeIn",
  }),
  ...exactArrowProbe({
    siteId: "resolver.getDeclarationStatementsForSourceFile",
    startLine: 88612,
    startExpect: "      getDeclarationStatementsForSourceFile: (node, flags, internalFlags, tracker) => {",
    endLine: 88621,
    endExpect: "      },",
    indent: "        ",
    args: "node, flags, internalFlags",
  }),
  ...exactArrowProbe({
    siteId: "resolver.createLateBoundIndexSignatures",
    startLine: 88624,
    startExpect: "      createLateBoundIndexSignatures: (cls, enclosing, flags, internalFlags, tracker) => {",
    endLine: 88691,
    endExpect: "      },",
    indent: "        ",
    args: "cls, enclosing, flags, internalFlags",
  }),
  ...exactCallProbe({
    siteId: "resolver.isImportRequiredByAugmentation",
    startLine: 88696,
    startExpect: "    function isImportRequiredByAugmentation(node) {",
    endLine: 88717,
    endExpect: "    }",
    indent: "      ",
  }),

  // Transformer symbol tracker entries.
  {
    anchor_line: 114327,
    expect: "  function reportInferenceFallback(node) {",
    insert_after: '    __h27aTrace("tracker.reportInferenceFallback", ...__h27aNodeArgs(node)); /* site-id: tracker.reportInferenceFallback */',
  },
  {
    anchor_line: 114360,
    expect: "  function trackSymbol(symbol, enclosingDeclaration2, meaning) {",
    insert_after: '    __h27aTrace("tracker.trackSymbol", __h27aName(symbol), ...__h27aNodeArgs(enclosingDeclaration2), meaning); /* site-id: tracker.trackSymbol */',
  },
  {
    anchor_line: 114371,
    expect: "  function reportPrivateInBaseOfClassExpression(propertyName) {",
    insert_after: '    __h27aTrace("tracker.reportPrivateInBaseOfClassExpression", __h27aName(propertyName), ...__h27aNodeArgs(propertyName)); /* site-id: tracker.reportPrivateInBaseOfClassExpression */',
  },
  {
    anchor_line: 114384,
    expect: "  function reportInaccessibleUniqueSymbolError() {",
    insert_after: '    __h27aTrace("tracker.reportInaccessibleUniqueSymbolError"); /* site-id: tracker.reportInaccessibleUniqueSymbolError */',
  },
  {
    anchor_line: 114389,
    expect: "  function reportCyclicStructureError() {",
    insert_after: '    __h27aTrace("tracker.reportCyclicStructureError"); /* site-id: tracker.reportCyclicStructureError */',
  },
  {
    anchor_line: 114394,
    expect: "  function reportInaccessibleThisError() {",
    insert_after: '    __h27aTrace("tracker.reportInaccessibleThisError"); /* site-id: tracker.reportInaccessibleThisError */',
  },
  {
    anchor_line: 114399,
    expect: "  function reportLikelyUnsafeImportRequiredError(specifier, symbolName2) {",
    insert_after: '    __h27aTrace("tracker.reportLikelyUnsafeImportRequiredError", typeof specifier === "string", typeof specifier === "string" ? specifier.split("/").length : 0, __h27aString(symbolName2)); /* site-id: tracker.reportLikelyUnsafeImportRequiredError */',
  },
  {
    anchor_line: 114408,
    expect: "  function reportTruncationError() {",
    insert_after: '    __h27aTrace("tracker.reportTruncationError"); /* site-id: tracker.reportTruncationError */',
  },
  {
    anchor_line: 114413,
    expect: "  function reportNonlocalAugmentation(containingFile, parentSymbol, symbol) {",
    insert_after: '    __h27aTrace("tracker.reportNonlocalAugmentation", ...__h27aNodeArgs(containingFile), __h27aName(parentSymbol), __h27aName(symbol), Array.isArray(symbol && symbol.declarations) ? symbol.declarations.length : 0); /* site-id: tracker.reportNonlocalAugmentation */',
  },
  {
    anchor_line: 114426,
    expect: "  function reportNonSerializableProperty(propertyName) {",
    insert_after: '    __h27aTrace("tracker.reportNonSerializableProperty", __h27aName(propertyName), ...__h27aNodeArgs(propertyName)); /* site-id: tracker.reportNonSerializableProperty */',
  },

  // Changed declaration nodes: input/output identity, provenance, flags.
  ...exactTransformProbe({
    siteId: "declarations.visitDeclarationSubtree",
    startLine: 114952,
    startExpect: "  function visitDeclarationSubtree(input) {",
    endLine: 115256,
    endExpect: "  }",
  }),
  ...exactTransformProbe({
    siteId: "declarations.transformTopLevelDeclaration",
    startLine: 115337,
    startExpect: "  function transformTopLevelDeclaration(input) {",
    endLine: 115704,
    endExpect: "  }",
  }),

  // declBlocked inputs are captured from the original single evaluation of
  // host.isEmitBlocked; the method is restored immediately after the expression.
  {
    anchor_line: 116669,
    expect: "    const declBlocked = !!declarationTransform.diagnostics && !!declarationTransform.diagnostics.length || !!host.isEmitBlocked(declarationFilePath) || !!compilerOptions.noEmit;",
    insert_before: '    let __h27aEmitBlocked = false; let __h27aEmitBlockedEvaluated = false; const __h27aIsEmitBlocked = host.isEmitBlocked; host.isEmitBlocked = function(...args) { __h27aEmitBlockedEvaluated = true; const value = __h27aIsEmitBlocked.apply(this, args); __h27aEmitBlocked = !!value; return value; }; /* site-id: declarations.declBlocked */',
  },
  {
    anchor_line: 116669,
    expect: "    const declBlocked = !!declarationTransform.diagnostics && !!declarationTransform.diagnostics.length || !!host.isEmitBlocked(declarationFilePath) || !!compilerOptions.noEmit;",
    insert_after: '    host.isEmitBlocked = __h27aIsEmitBlocked; __h27aTrace("declarations.declBlocked", length(declarationTransform.diagnostics), __h27aEmitBlockedEvaluated, __h27aEmitBlocked, !!compilerOptions.noEmit, declBlocked); /* site-id: declarations.declBlocked */',
  },

  // Syntactic-builder entry/result and checker-fallback sentinels.
  ...exactSyntacticProbe({
    siteId: "syntactic.serializeTypeOfDeclaration",
    startLine: 133753,
    startExpect: "  function serializeTypeOfDeclaration(node, symbol, context) {",
    endLine: 133785,
    endExpect: "  }",
  }),
  ...exactSyntacticProbe({
    siteId: "syntactic.serializeReturnTypeForSignature",
    startLine: 133807,
    startExpect: "  function serializeReturnTypeForSignature(node, symbol, context) {",
    endLine: 133829,
    endExpect: "  }",
  }),
  {
    anchor_line: 133943,
    expect: "  function inferTypeOfDeclaration(node, symbol, context, reportFallback = true) {",
    insert_after: '    __h27aMarkSyntacticFallback("syntactic.serializeTypeOfDeclaration", node, reportFallback); /* site-id: syntactic.serializeTypeOfDeclaration.checkerFallback */',
  },
  {
    anchor_line: 133962,
    expect: "  function inferReturnTypeOfSignatureSignature(node, context, symbol, reportFallback) {",
    insert_after: '    __h27aMarkSyntacticFallback("syntactic.serializeReturnTypeForSignature", node, reportFallback); /* site-id: syntactic.serializeReturnTypeForSignature.checkerFallback */',
  },

  // Harness-only export seam. The CLI remains unchanged unless the child sets
  // the marker before loading the generated copy.
  {
    anchor_line: 134464,
    expect: "executeCommandLine(sys, noop, sys.args);",
    insert_before: 'if (globalThis.__H2_7A_INTERNAL__) { globalThis.__H2_7A_TS__ = { version, createProgram, createCompilerHost, createSourceFile, normalizePath, getScriptKindFromFileName, flattenDiagnosticMessageText, getDefaultLibFileName, DiagnosticCategory, emitFilesAndReportErrorsAndGetExitStatus }; __h27aTrace("probe.bootstrap", version); } else /* site-id: probe.bootstrap */',
  },
]);

function fail(message) {
  process.stderr.write(`h2-7a-probe-traces: ${message}\n`);
  process.exit(1);
}

let softValidation = false;

function requireCondition(condition, message) {
  if (!condition) {
    if (softValidation) throw new Error(message);
    fail(message);
  }
}

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

// stableStringify authority cloned from h2-5g-profile.mjs.
function stableStringify(value) {
  if (Array.isArray(value)) {
    return `[${value.map(stableStringify).join(",")}]`;
  }
  if (value !== null && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function withFingerprint(value, field) {
  return {
    ...value,
    [field]: sha256(Buffer.from(stableStringify(value), "utf8")),
  };
}

function hasValidFingerprint(value, field) {
  if (value === null || typeof value !== "object") return false;
  const { [field]: stored, ...rest } = value;
  return (
    typeof stored === "string" &&
    stored === sha256(Buffer.from(stableStringify(rest), "utf8"))
  );
}

function readBytes(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE, relativePath));
}

function readJson(relativePath) {
  return JSON.parse(readBytes(relativePath).toString("utf8"));
}

function pathHash(relativePath) {
  return { path: relativePath, sha256: sha256(readBytes(relativePath)) };
}

function render(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

function writeFileAtomic(absolutePath, contents) {
  fs.mkdirSync(path.dirname(absolutePath), { recursive: true });
  const temporary = path.join(
    path.dirname(absolutePath),
    `.${path.basename(absolutePath)}.tmp`,
  );
  fs.writeFileSync(temporary, contents);
  fs.renameSync(temporary, absolutePath);
}

function isSha256(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
}

function libraryInventoryRecord() {
  const directory = path.join(WORKSPACE, TYPESCRIPT_LIB_DIRECTORY);
  const names = fs
    .readdirSync(directory)
    .filter((name) => name.startsWith("lib.") && name.endsWith(".d.ts"))
    .sort();
  requireCondition(names.length > 0, "vendored TypeScript lib inventory is empty");
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

function vendoredRuntimeRoll() {
  return sha256(
    Buffer.from(
      stableStringify({
        bundle: pathHash(TYPESCRIPT_BUNDLE),
        implementation: pathHash(TYPESCRIPT_IMPLEMENTATION),
        libraries: libraryInventoryRecord(),
      }),
      "utf8",
    ),
  );
}

function validateContractDocument() {
  const schema = readJson(CONTRACT_RELATIVE_PATH);
  requireCondition(
    schema?.$schema === "https://json-schema.org/draft/2020-12/schema",
    `${CONTRACT_RELATIVE_PATH} is not the H2.7a draft-2020-12 contract`,
  );
  requireCondition(
    schema?.properties?.phase?.const === PHASE &&
      schema?.properties?.schema?.const === 1 &&
      schema?.additionalProperties === false,
    `${CONTRACT_RELATIVE_PATH} does not describe ${PHASE} schema 1`,
  );
  return schema;
}

function validateRuntime() {
  const expectedNode = readBytes(".node-version").toString("utf8").trim();
  requireCondition(
    process.version === `v${expectedNode}`,
    `requires Node ${expectedNode}`,
  );
  requireCondition(
    publicTs.version === "6.0.3",
    `unexpected TypeScript runtime ${publicTs.version}`,
  );
  for (const name of [
    "createProgram",
    "createCompilerHost",
    "createSourceFile",
    "emitFilesAndReportErrorsAndGetExitStatus",
  ]) {
    requireCondition(
      typeof publicTs[name] === "function",
      `pinned TypeScript does not expose ${name}`,
    );
  }
}

function applyExactEdits() {
  const base = readBytes(TYPESCRIPT_IMPLEMENTATION);
  const baseSha = sha256(base);
  requireCondition(
    baseSha === EXPECTED_BASE_SHA256,
    `${TYPESCRIPT_IMPLEMENTATION} sha256 ${baseSha} does not match pinned ${EXPECTED_BASE_SHA256}`,
  );
  const text = base.toString("utf8");
  requireCondition(text.endsWith("\n"), `${TYPESCRIPT_IMPLEMENTATION} lacks final newline`);
  const lines = text.slice(0, -1).split("\n");
  const before = new Map();
  const after = new Map();
  EDIT_TABLE.forEach((edit, index) => {
    const keys = Object.keys(edit).sort();
    requireCondition(
      stableStringify(keys) ===
        stableStringify(
          [
            "anchor_line",
            "expect",
            edit.insert_before === undefined ? "insert_after" : "insert_before",
          ].sort(),
        ),
      `edit ${index} has fields outside the exact-edit contract`,
    );
    requireCondition(
      Number.isInteger(edit.anchor_line) &&
        edit.anchor_line >= 1 &&
        edit.anchor_line <= lines.length,
      `edit ${index} has invalid anchor line`,
    );
    requireCondition(
      lines[edit.anchor_line - 1] === edit.expect,
      `edit ${index} anchor ${edit.anchor_line} expect mismatch\nexpected: ${JSON.stringify(edit.expect)}\nactual:   ${JSON.stringify(lines[edit.anchor_line - 1])}`,
    );
    const insertion = edit.insert_before ?? edit.insert_after;
    requireCondition(
      typeof insertion === "string" &&
        insertion.length > 0 &&
        !insertion.includes("\n") &&
        insertion.includes("site-id:"),
      `edit ${index} must insert one site-id-documented line`,
    );
    const table = edit.insert_before === undefined ? after : before;
    const entries = table.get(edit.anchor_line) ?? [];
    entries.push({ index, insertion, placement: edit.insert_before === undefined ? "after" : "before" });
    table.set(edit.anchor_line, entries);
  });

  const generated = [];
  const mappings = new Array(EDIT_TABLE.length);
  for (let vendorLine = 1; vendorLine <= lines.length; vendorLine += 1) {
    for (const entry of before.get(vendorLine) ?? []) {
      generated.push(entry.insertion);
      mappings[entry.index] = {
        edit_index: entry.index,
        vendor_line: vendorLine,
        generated_anchor_line: generated.length + 1,
        generated_inserted_line: generated.length,
        placement: entry.placement,
      };
    }
    const generatedAnchorLine = generated.length + 1;
    generated.push(lines[vendorLine - 1]);
    for (const entry of after.get(vendorLine) ?? []) {
      generated.push(entry.insertion);
      mappings[entry.index] = {
        edit_index: entry.index,
        vendor_line: vendorLine,
        generated_anchor_line: generatedAnchorLine,
        generated_inserted_line: generated.length,
        placement: entry.placement,
      };
    }
  }
  const output = Buffer.from(`${generated.join("\n")}\n`, "utf8");
  const editTableSha = sha256(Buffer.from(stableStringify(EDIT_TABLE), "utf8"));
  const outputSha = sha256(output);
  const positionMap = {
    schema: 1,
    base_sha256: baseSha,
    edit_table_sha256: editTableSha,
    instrumented_output_sha256: outputSha,
    edits: mappings,
  };
  const instrumentedPath = path.join(WORKSPACE, INSTRUMENTED_RELATIVE_PATH);
  const positionMapPath = path.join(WORKSPACE, POSITION_MAP_RELATIVE_PATH);
  writeFileAtomic(instrumentedPath, output);
  writeFileAtomic(positionMapPath, render(positionMap));
  return {
    base_sha256: baseSha,
    edit_table_sha256: editTableSha,
    instrumented_output_sha256: outputSha,
    position_map_sha256: sha256(Buffer.from(render(positionMap), "utf8")),
  };
}

function caseManifestFingerprint(witnesses) {
  const manifest = witnesses?.case_manifest;
  requireCondition(
    manifest !== null &&
      typeof manifest === "object" &&
      !Array.isArray(manifest),
    `${WITNESSES_RELATIVE_PATH} lacks case_manifest`,
  );
  const stored = manifest.case_manifest_fingerprint;
  requireCondition(
    isSha256(stored),
    `${WITNESSES_RELATIVE_PATH} lacks case_manifest_fingerprint`,
  );
  const { case_manifest_fingerprint: _fingerprint, ...manifestPayload } =
    manifest;
  requireCondition(
    stored === sha256(Buffer.from(stableStringify(manifestPayload), "utf8")),
    `${WITNESSES_RELATIVE_PATH} case_manifest fingerprint is invalid`,
  );
  return stored;
}

function manifestCases(witnesses) {
  const manifest = witnesses.case_manifest;
  const cases = Array.isArray(manifest) ? manifest : manifest.cases;
  requireCondition(
    Array.isArray(cases) && cases.length > 0,
    `${WITNESSES_RELATIVE_PATH} case_manifest has no cases`,
  );
  const ids = cases.map(
    (entry) => entry?.case_id ?? entry?.id ?? entry?.caseId,
  );
  requireCondition(
    ids.every((value) => typeof value === "string" && value.length > 0) &&
      new Set(ids).size === ids.length,
    `${WITNESSES_RELATIVE_PATH} case_manifest case ids are missing or duplicate`,
  );
  return cases.map((entry, index) => ({
    ...entry,
    case_id: ids[index],
  }));
}

function collectWitnessCaseRecords(witnesses) {
  const records = [];
  for (const key of ["cases", "case_observations", "observations"]) {
    const value = witnesses[key];
    if (Array.isArray(value)) records.push(...value);
    if (value !== null && typeof value === "object" && !Array.isArray(value)) {
      for (const [caseId, record] of Object.entries(value)) {
        records.push({ case_id: caseId, ...record });
      }
    }
  }
  for (const family of witnesses.families ?? []) {
    if (Array.isArray(family?.cases)) records.push(...family.cases);
  }
  return records;
}

function observationFromRecord(record) {
  return (
    record?.public_observation ??
    record?.observation ??
    record?.typescript_observation ??
    record?.public_outputs ??
    null
  );
}

function decodeTextRecord(entry) {
  if (typeof entry.text === "string") return entry.text;
  if (typeof entry.content === "string") return entry.content;
  for (const key of [
    "utf8_base64",
    "content_utf8_base64",
    "text_utf8_base64",
    "source_utf8_base64",
  ]) {
    if (typeof entry[key] === "string") {
      return Buffer.from(entry[key], "base64").toString("utf8");
    }
  }
  const sourcePath =
    entry.workspace_path ?? entry.source_path ?? entry.fixture_path;
  if (typeof sourcePath === "string") {
    requireCondition(
      !path.isAbsolute(sourcePath) &&
        !sourcePath.split(/[\\/]/).includes(".."),
      `source path ${sourcePath} escapes the workspace`,
    );
    return readBytes(sourcePath).toString("utf8");
  }
  return null;
}

function fileArrayFrom(value) {
  if (Array.isArray(value)) return value;
  if (value !== null && typeof value === "object") {
    return Object.entries(value).map(([filePath, record]) =>
      typeof record === "string"
        ? { path: filePath, text: record }
        : { path: filePath, ...record },
    );
  }
  return null;
}

function embeddedSourceFilesForCase(caseEntry) {
  const candidates = [
    caseEntry.files,
    caseEntry.source_files,
    caseEntry.inputs?.files,
    caseEntry.input?.files,
    caseEntry.vfs?.files,
    caseEntry.source_universe?.files,
  ];
  for (const candidate of candidates) {
    const files = fileArrayFrom(candidate);
    if (
      files &&
      files.length > 0 &&
      files.every((entry) => decodeTextRecord(entry) !== null)
    ) {
      return files;
    }
  }
  return null;
}

function safeSuiteSourcePath(suite, relativePath) {
  requireCondition(
    typeof relativePath === "string" &&
      relativePath.length > 0 &&
      relativePath === path.posix.normalize(relativePath) &&
      !relativePath.startsWith("/") &&
      !relativePath.startsWith("../"),
    `unsafe ${suite} source path ${JSON.stringify(relativePath)}`,
  );
  const root = path.join(WORKSPACE, "ts-tests/tests/cases", suite);
  const absolute = path.resolve(root, ...relativePath.split("/"));
  requireCondition(
    absolute.startsWith(`${path.resolve(root)}${path.sep}`),
    `${suite} source escaped its suite root: ${relativePath}`,
  );
  return absolute;
}

function orderedSettings(object) {
  return Object.entries(object).map(([name, value]) => ({ name, value }));
}

// Same TypeScript test-harness splitter used by the lane-B witness machine.
function makeFixtureUnits(text, fixturePath) {
  const units = [];
  let currentContent;
  let currentOptions = {};
  let currentName;
  const optionPattern = /^\/{2}\s*@([\w]+)\s*:\s*([^\r\n]*)/;
  const linkPattern = /^\/{2}\s*@link\s*:\s*([^\r\n]*)\s*->\s*([^\r\n]*)/;
  for (const line of text.split(/\r?\n/)) {
    if (linkPattern.test(line)) continue;
    const metadata = optionPattern.exec(line);
    if (metadata) {
      const name = metadata[1];
      const value = metadata[2].trim();
      currentOptions[name] = value;
      if (name.toLowerCase() !== "filename") continue;
      if (currentName !== undefined) {
        units.push({
          name: currentName,
          text: currentContent,
          file_options: orderedSettings(currentOptions),
        });
        currentContent = undefined;
        currentOptions = {};
        currentName = value;
      } else {
        currentName = value;
        if (currentContent) {
          requireCondition(
            publicTs.skipTrivia(currentContent, 0, false, false) ===
              currentContent.length,
            `${fixturePath} has content before its first @filename`,
          );
        }
        currentContent = "";
      }
      continue;
    }
    if (currentContent === undefined) currentContent = "";
    else if (currentContent !== "") currentContent += "\n";
    currentContent += line;
  }
  currentName =
    units.length > 0 || currentName !== undefined
      ? currentName
      : path.posix.basename(fixturePath);
  units.push({
    name: currentName,
    text: currentContent || "",
    file_options: orderedSettings(currentOptions),
  });
  units.forEach((unit, index) => {
    unit.original_id = index;
  });
  return units;
}

function mergedFixtureSettings(base, overrides) {
  const settings = new Map(base.map((setting) => [setting.name, setting.value]));
  for (const setting of overrides ?? []) settings.set(setting.name, setting.value);
  return settings;
}

function exactFixtureSetting(settings, name) {
  return [...settings].find(([candidate]) => candidate === name)?.[1];
}

function fixtureCurrentDirectory(settings) {
  const configured = exactFixtureSetting(settings, "currentDirectory");
  return configured === undefined
    ? VIRTUAL_SOURCE_ROOT
    : publicTs.getNormalizedAbsolutePath(configured, VIRTUAL_SOURCE_ROOT);
}

function containsReferencePath(text) {
  return [...text.matchAll(/reference/g)].some((match) =>
    /^\s+path/.test(text.slice(match.index + "reference".length)),
  );
}

function fixtureRootSelection(units, settings, options) {
  const cwd = fixtureCurrentDirectory(settings);
  const lastUnitByPath = new Map();
  units.forEach((unit, id) => {
    lastUnitByPath.set(publicTs.getNormalizedAbsolutePath(unit.name, cwd), id);
  });
  const candidates = [...lastUnitByPath.values()].sort((left, right) => left - right);
  const last = candidates.at(-1);
  requireCondition(last !== undefined, "fixture has no source unit");
  const lastUnit = units[last];
  const implicitReferences =
    exactFixtureSetting(settings, "noImplicitReferences") !== undefined ||
    (lastUnit.text ?? "").includes("require(") ||
    containsReferencePath(lastUnit.text ?? "");
  const rootUnitIds = implicitReferences ? [last] : candidates;
  const otherUnitIds = implicitReferences
    ? candidates.filter((id) => units[id].name !== lastUnit.name)
    : [];
  return {
    program_root_unit_ids: rootUnitIds.filter(
      (id) =>
        !publicTs.fileExtensionIs(units[id].name, publicTs.Extension.Json) &&
        publicTs.isSupportedSourceFileName(units[id].name, options),
    ),
    vfs_write_order: [...rootUnitIds, ...otherUnitIds],
  };
}

function fixtureSymlinks(fileOptions) {
  const setting = fileOptions.find(
    (entry) => entry.name.toLowerCase() === "symlink",
  );
  if (!setting || setting.value === "") return [];
  return setting.value.split(",").map((entry) => entry.trim());
}

let expansionAuthorities;
function expansionAuthority(suite) {
  expansionAuthorities ??= {
    compiler: readJson(TEST_SUITE_EXPANSION),
    conformance: readJson(CONFORMANCE_EXPANSION),
  };
  return expansionAuthorities[suite];
}

function expandedFixture(suite, relativePath) {
  const expansion = expansionAuthority(suite);
  requireCondition(expansion !== undefined, `unsupported fixture suite ${suite}`);
  const sourceIndex = expansion.sources.findIndex(
    (source) => source.path === relativePath,
  );
  requireCondition(sourceIndex >= 0, `${suite}/${relativePath} expansion source is absent`);
  const fixture =
    suite === "compiler"
      ? expansion.compiler_fixtures[sourceIndex]
      : expansion.fixtures.find((candidate) => candidate.source === sourceIndex);
  requireCondition(fixture !== undefined, `${suite}/${relativePath} expansion fixture is absent`);
  return fixture;
}

function assertManifestInputs(caseEntry, files) {
  const actual = files
    .map((entry) => ({
      path: publicTs.normalizePath(entry.path),
      sha256: sha256(Buffer.from(entry.text, "utf8")),
    }))
    .sort((left, right) =>
      left.path.localeCompare(right.path) || left.sha256.localeCompare(right.sha256),
    );
  const expected = [...(caseEntry.input_files ?? [])].sort((left, right) =>
    left.path.localeCompare(right.path) || left.sha256.localeCompare(right.sha256),
  );
  requireCondition(
    stableStringify(actual) === stableStringify(expected),
    `${caseEntry.case_id} reconstructed inputs differ from case_manifest`,
  );
}

function curatedManifestControl(caseEntry) {
  const prefix = `typescript-6.0.3/${caseEntry.suite}/`;
  requireCondition(
    typeof caseEntry.fixture_id === "string" &&
      caseEntry.fixture_id.startsWith(prefix),
    `${caseEntry.case_id} fixture identity does not match suite`,
  );
  const relativePath = caseEntry.fixture_id.slice(prefix.length);
  const fixture = expandedFixture(caseEntry.suite, relativePath);
  const configurationIndex = caseEntry.matrix?.configuration_index ?? 0;
  const configuration = fixture.configurations[configurationIndex];
  requireCondition(
    configuration !== undefined &&
      configuration.variant === caseEntry.matrix?.fixture_variant,
    `${caseEntry.case_id} fixture configuration changed`,
  );
  const decoded = publicTs.sys.readFile(
    safeSuiteSourcePath(caseEntry.suite, relativePath),
  );
  requireCondition(typeof decoded === "string", `${caseEntry.case_id} fixture is unreadable`);
  const units = makeFixtureUnits(decoded, relativePath);
  requireCondition(
    units.length === fixture.normal_units.length,
    `${caseEntry.case_id} fixture unit count changed`,
  );
  units.forEach((unit, index) => {
    const expected = fixture.normal_units[index];
    requireCondition(
      unit.name === expected.name &&
        sha256(Buffer.from(unit.text, "utf8")) === expected.content.sha256,
      `${caseEntry.case_id} fixture unit ${index} changed`,
    );
  });
  const settings = mergedFixtureSettings(fixture.settings, configuration.settings);
  const cwd = fixtureCurrentDirectory(settings);
  const options = compilerOptionsForCase(caseEntry);
  const selection = fixtureRootSelection(units, settings, options);
  const files = selection.vfs_write_order.map((id) => ({
    path: publicTs.getNormalizedAbsolutePath(units[id].name, cwd),
    text: units[id].text,
  }));
  const symlinks = [];
  for (const id of selection.vfs_write_order) {
    const target = publicTs.getNormalizedAbsolutePath(units[id].name, cwd);
    for (const rawLink of fixtureSymlinks(units[id].file_options)) {
      symlinks.push({
        link_path: publicTs.getNormalizedAbsolutePath(rawLink, cwd),
        target_path: target,
      });
    }
  }
  assertManifestInputs(caseEntry, files);
  return {
    current_directory: cwd,
    roots: selection.program_root_unit_ids.map((id) =>
      publicTs.getNormalizedAbsolutePath(units[id].name, cwd),
    ),
    files,
    symlinks,
    compiler_options: options,
    default_library: "compiler-host",
  };
}

let qualificationByCaseId;
function qualificationCase(caseId) {
  if (qualificationByCaseId === undefined) {
    const qualification = readJson(H2_6C_QUALIFICATION);
    requireCondition(
      qualification?.schema === 1 && Array.isArray(qualification.cases),
      `${H2_6C_QUALIFICATION} is invalid`,
    );
    qualificationByCaseId = new Map(
      qualification.cases.map((entry) => [entry.case_id, entry]),
    );
  }
  const entry = qualificationByCaseId.get(caseId);
  requireCondition(entry !== undefined, `${caseId} is absent from H2.6c qualification`);
  return entry;
}

function stratumManifestControl(caseEntry) {
  const authority = qualificationCase(caseEntry.case_id);
  let files;
  let roots;
  let cwd;
  let symlinks;
  let defaultLibrary;
  if (caseEntry.suite === "project") {
    const projectInput = authority.project_input;
    requireCondition(
      projectInput?.root_selection?.state === "explicit-inputs",
      `${caseEntry.case_id} project input is invalid`,
    );
    files = projectInput.analyzed_files.map((file) => {
      requireCondition(
        file.path.startsWith(`${PROJECT_VIRTUAL_PREFIX}/`),
        `${caseEntry.case_id} project input escaped its mount`,
      );
      const relative = file.path.slice(PROJECT_VIRTUAL_PREFIX.length + 1);
      const text = publicTs.sys.readFile(safeSuiteSourcePath("projects", relative));
      requireCondition(typeof text === "string", `${caseEntry.case_id} project input is unreadable`);
      requireCondition(
        sha256(Buffer.from(text, "utf8")) === file.text_sha256,
        `${caseEntry.case_id} project input ${file.path} changed`,
      );
      return { path: file.path, text };
    });
    roots = projectInput.root_selection.roots
      .filter((root) => root.present)
      .map((root) => root.path);
    cwd = projectInput.current_directory;
    symlinks = [];
    defaultLibrary = "project-es5";
  } else {
    requireCondition(caseEntry.suite === "compiler", `${caseEntry.case_id} unsupported stratum suite`);
    const input = authority.input;
    files = input.files.map((file) => {
      const bytes = Buffer.from(file.utf8_base64, "base64");
      requireCondition(
        bytes.length === file.utf8_bytes && sha256(bytes) === file.utf8_sha256,
        `${caseEntry.case_id} embedded input ${file.path} changed`,
      );
      return { path: file.path, text: bytes.toString("utf8") };
    });
    roots = input.roots;
    cwd = input.current_directory;
    symlinks = input.vfs_symlinks;
    defaultLibrary = "compiler-host";
  }
  assertManifestInputs(caseEntry, files);
  return {
    current_directory: cwd,
    roots,
    files,
    symlinks,
    compiler_options: compilerOptionsForCase(caseEntry),
    default_library: defaultLibrary,
  };
}

function rehydrateManifestControl(caseEntry) {
  return caseEntry.family_id === "S"
    ? stratumManifestControl(caseEntry)
    : curatedManifestControl(caseEntry);
}

function normalizedPath(fileName, cwd) {
  const slashed = fileName.replace(/\\/g, "/");
  return publicTs.normalizePath(
    slashed.startsWith("/") ? slashed : path.posix.join(cwd, slashed),
  );
}

function compilerOptionsForCase(caseEntry) {
  const raw =
    caseEntry.compiler_options ??
    caseEntry.options ??
    caseEntry.option_record ??
    caseEntry.effective_options ??
    caseEntry.control?.compiler_options;
  requireCondition(
    raw !== null && typeof raw === "object" && !Array.isArray(raw),
    `${caseEntry.case_id} case_manifest row lacks compiler options`,
  );
  // Lane B records the already-parsed CompilerOptions object: enums are
  // numbers and `lib` entries are canonical lib.*.d.ts names. Re-parsing that
  // record as tsconfig JSON would change its meaning.
  return structuredClone(raw);
}

function normalizeCaseControl(caseEntry) {
  const embeddedFiles = embeddedSourceFilesForCase(caseEntry);
  if (embeddedFiles === null) {
    const hydrated = rehydrateManifestControl(caseEntry);
    return {
      case_id: caseEntry.case_id,
      route: "program",
      transpile_api: null,
      current_directory: publicTs.normalizePath(hydrated.current_directory),
      files: hydrated.files.map((entry) => ({
        path: publicTs.normalizePath(entry.path),
        text: entry.text,
      })),
      roots: hydrated.roots.map((entry) => publicTs.normalizePath(entry)),
      compiler_options: hydrated.compiler_options,
      symlinks: hydrated.symlinks.map((entry) => ({
        link_path: publicTs.normalizePath(entry.link_path),
        target_path: publicTs.normalizePath(entry.target_path),
      })),
      default_library: hydrated.default_library,
    };
  }
  const cwd = publicTs.normalizePath(
    caseEntry.current_directory ??
      caseEntry.cwd ??
      caseEntry.control?.current_directory ??
      "/project",
  );
  requireCondition(cwd.startsWith("/"), `${caseEntry.case_id} cwd must be absolute`);
  const files = embeddedFiles.map((entry, index) => {
    const fileName = entry.path ?? entry.file_name ?? entry.name;
    requireCondition(
      typeof fileName === "string" && fileName.length > 0,
      `${caseEntry.case_id} file ${index} lacks path`,
    );
    const text = decodeTextRecord(entry);
    requireCondition(
      typeof text === "string",
      `${caseEntry.case_id} file ${fileName} lacks source bytes`,
    );
    const bytes = Buffer.from(text, "utf8");
    const expectedSha =
      entry.utf8_sha256 ?? entry.content_sha256 ?? entry.sha256;
    requireCondition(
      expectedSha === undefined || expectedSha === sha256(bytes),
      `${caseEntry.case_id} file ${fileName} source hash mismatch`,
    );
    return { path: normalizedPath(fileName, cwd), text };
  });
  const rootsRaw =
    caseEntry.roots ??
    caseEntry.root_names ??
    caseEntry.root_file_names ??
    caseEntry.program_roots ??
    caseEntry.control?.roots ??
    files.map((entry) => entry.path);
  requireCondition(
    Array.isArray(rootsRaw) && rootsRaw.length > 0,
    `${caseEntry.case_id} has no program roots`,
  );
  const options = compilerOptionsForCase(caseEntry);
  const route =
    caseEntry.route ??
    caseEntry.observation_route ??
    caseEntry.control?.route ??
    (caseEntry.transpile_api || caseEntry.api ? "transpile-api" : "program");
  const symlinks = caseEntry.symlinks ?? caseEntry.vfs?.symlinks ?? [];
  requireCondition(Array.isArray(symlinks), `${caseEntry.case_id} symlinks must be an array`);
  return {
    case_id: caseEntry.case_id,
    route,
    transpile_api: caseEntry.transpile_api ?? caseEntry.api ?? null,
    current_directory: cwd,
    files,
    roots: rootsRaw.map((fileName) => normalizedPath(fileName, cwd)),
    compiler_options: options,
    symlinks: symlinks.map((entry) => ({
      link_path: normalizedPath(entry.link_path ?? entry.path, cwd),
      target_path: normalizedPath(entry.target_path ?? entry.target, cwd),
    })),
    default_library:
      caseEntry.default_library ?? caseEntry.control?.default_library ??
      "compiler-host",
  };
}

function normalizedDiagnostic(diagnostic) {
  if (diagnostic === null || typeof diagnostic !== "object") return diagnostic;
  return {
    code: diagnostic.code,
    category:
      typeof diagnostic.category === "string"
        ? diagnostic.category
        : publicTs.DiagnosticCategory[diagnostic.category],
    file:
      diagnostic.file === null || diagnostic.file === undefined
        ? null
        : publicTs.normalizePath(
            typeof diagnostic.file === "string"
              ? diagnostic.file
              : diagnostic.file.fileName,
          ),
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
    message:
      typeof diagnostic.message === "string"
        ? diagnostic.message
        : publicTs.flattenDiagnosticMessageText(diagnostic.messageText, "\n"),
  };
}

function hashFromWrite(write) {
  for (const key of [
    "materialized_utf8_sha256",
    "callback_utf8_sha256",
    "utf8_sha256",
    "sha256",
    "output_utf8_sha256",
    "output_sha256",
  ]) {
    if (isSha256(write[key])) return write[key];
  }
  for (const key of [
    "materialized_utf8_base64",
    "callback_utf8_base64",
    "utf8_base64",
    "output_utf8_base64",
  ]) {
    if (typeof write[key] === "string") {
      return sha256(Buffer.from(write[key], "base64"));
    }
  }
  if (typeof write.text === "string") {
    return sha256(Buffer.from(write.text, "utf8"));
  }
  fail(`witness write ${write.path ?? write.file_name ?? "<unknown>"} lacks bytes/hash`);
}

function bytesFromWrite(write) {
  for (const key of [
    "materialized_utf8_bytes",
    "callback_utf8_bytes",
    "utf8_bytes",
    "output_utf8_bytes",
    "output_bytes",
    "bytes",
  ]) {
    if (Number.isInteger(write[key])) return write[key];
  }
  for (const key of [
    "materialized_utf8_base64",
    "callback_utf8_base64",
    "utf8_base64",
    "output_utf8_base64",
  ]) {
    if (typeof write[key] === "string") {
      return Buffer.from(write[key], "base64").length;
    }
  }
  if (typeof write.text === "string") {
    return Buffer.byteLength(write.text, "utf8");
  }
  return null;
}

function normalizeWrites(writes) {
  return (writes ?? []).map((write, index) => ({
    index: write.index ?? index,
    path: publicTs.normalizePath(write.path ?? write.file_name ?? write.name),
    sha256: hashFromWrite(write),
    bytes: bytesFromWrite(write),
    write_byte_order_mark: !!(
      write.write_byte_order_mark ?? write.bom ?? false
    ),
  }));
}

function publicOutputProjection(observation) {
  requireCondition(
    observation !== null && typeof observation === "object",
    "witness case lacks public observation",
  );
  const emitResult = observation.emit_result ?? observation.emitResult ?? {};
  const reported =
    observation.reported_diagnostics ??
    observation.pre_emit_diagnostics ??
    observation.diagnostics ??
    [];
  const emitDiagnostics =
    observation.emit_diagnostics ?? emitResult.diagnostics ?? [];
  const emittedFiles =
    observation.emitted_files ??
    emitResult.emitted_files ??
    emitResult.emittedFiles ??
    null;
  const transpileOutputs = (
    observation.transpile_outputs ?? observation.outputs ?? []
  ).map((output, index) => ({
    index,
    path: publicTs.normalizePath(
      output.path ?? output.file_name ?? output.unit ?? `transpile-${index}`,
    ),
    output_sha256: hashFromWrite(output),
    output_bytes: bytesFromWrite(output),
    source_map_sha256:
      output.source_map_sha256 ??
      output.source_map_json_utf8_sha256 ??
      output.source_map_utf8_sha256 ??
      (typeof output.sourceMapText === "string"
        ? sha256(Buffer.from(output.sourceMapText, "utf8"))
        : null),
  }));
  return {
    writes: normalizeWrites(observation.writes),
    reported_diagnostics: reported.map(normalizedDiagnostic),
    emit_diagnostics: emitDiagnostics.map(normalizedDiagnostic),
    emit_result: {
      emit_skipped:
        observation.emit_skipped ??
        observation.emit_refused ??
        emitResult.emit_skipped ??
        emitResult.emitSkipped ??
        false,
      emitted_files:
        emittedFiles === null
          ? null
          : emittedFiles.map((fileName) => publicTs.normalizePath(fileName)),
    },
    transpile_outputs: transpileOutputs,
  };
}

function validateWitnessArtifact(witnesses, relativePath) {
  if (relativePath !== WITNESSES_RELATIVE_PATH) return;
  requireCondition(
    witnesses.kind === "h2-7a-public-observable-witnesses" &&
      witnesses.status === "qualified-typescript-oracle" &&
      witnesses.phase === "H2.7a-witnesses" &&
      hasValidFingerprint(witnesses, "witnesses_fingerprint_sha256"),
    `${relativePath} identity/fingerprint is invalid`,
  );
  const contractPath = path.join(WORKSPACE, WITNESSES_CONTRACT_RELATIVE_PATH);
  if (fs.existsSync(contractPath)) {
    const contract = readJson(WITNESSES_CONTRACT_RELATIVE_PATH);
    requireCondition(
      contract?.$schema === "https://json-schema.org/draft/2020-12/schema" &&
        contract?.properties?.schema?.const === 1 &&
        contract?.properties?.phase?.const === "H2.7a-witnesses" &&
        contract?.additionalProperties === false,
      `${WITNESSES_CONTRACT_RELATIVE_PATH} does not match lane-B schema 1`,
    );
  }
  const manifest = witnesses.case_manifest;
  if (Array.isArray(manifest?.source_universe)) {
    requireCondition(
      manifest.source_universe_sha256 ===
        sha256(Buffer.from(stableStringify(manifest.source_universe), "utf8")),
      `${relativePath} source_universe fingerprint is invalid`,
    );
  }
}

function loadWitnessContext(relativePath = WITNESSES_RELATIVE_PATH) {
  const absolute = path.join(WORKSPACE, relativePath);
  requireCondition(
    fs.existsSync(absolute),
    `witnesses artifact required first: missing ${relativePath}`,
  );
  const witnesses = readJson(relativePath);
  requireCondition(
    witnesses?.schema === 1,
    `${relativePath} must be lane-B schema 1`,
  );
  validateWitnessArtifact(witnesses, relativePath);
  const fingerprint = caseManifestFingerprint(witnesses);
  const cases = manifestCases(witnesses);
  const records = collectWitnessCaseRecords(witnesses);
  const recordById = new Map();
  for (const record of records) {
    const caseId = record?.case_id ?? record?.id ?? record?.caseId;
    if (typeof caseId === "string") recordById.set(caseId, record);
  }
  const normalized = cases.map((caseEntry) => {
    const record = recordById.get(caseEntry.case_id) ?? caseEntry;
    const observation = observationFromRecord(record) ?? observationFromRecord(caseEntry);
    requireCondition(
      observation !== null,
      `${caseEntry.case_id} lacks a lane-B public observation`,
    );
    const publicOutputs = publicOutputProjection(observation);
    return {
      case_id: caseEntry.case_id,
      control: normalizeCaseControl(caseEntry),
      expected_public_outputs: publicOutputs,
      public_output_roll: sha256(
        Buffer.from(stableStringify(publicOutputs), "utf8"),
      ),
    };
  });
  return {
    relative_path: relativePath,
    path_hash: pathHash(relativePath),
    witnesses,
    case_manifest_fingerprint: fingerprint,
    cases: normalized,
  };
}

function hasVirtualDirectory(files, directory) {
  const normalized = publicTs.normalizePath(directory).replace(/\/$/, "");
  const prefix = `${normalized}/`;
  return [...files.keys()].some((fileName) => fileName.startsWith(prefix));
}

function virtualDirectories(files, directory) {
  const normalized = publicTs.normalizePath(directory).replace(/\/$/, "");
  const prefix = `${normalized}/`;
  const result = new Set();
  for (const fileName of files.keys()) {
    if (!fileName.startsWith(prefix)) continue;
    const rest = fileName.slice(prefix.length);
    const slash = rest.indexOf("/");
    if (slash !== -1) result.add(rest.slice(0, slash));
  }
  return [...result].sort();
}

function createVirtualProgram(tsApi, control) {
  const files = new Map(
    control.files.map((entry) => [tsApi.normalizePath(entry.path), entry.text]),
  );
  const symlinkByPath = new Map(
    control.symlinks.map((entry) => [
      tsApi.normalizePath(entry.link_path),
      tsApi.normalizePath(entry.target_path),
    ]),
  );
  for (const [link, target] of symlinkByPath) {
    if (files.has(target) && !files.has(link)) files.set(link, files.get(target));
  }
  const baseHost = tsApi.createCompilerHost(control.compiler_options, true);
  const defaultLibrary = path.join(
    ...(control.default_library === "barebones"
      ? [""]
      : [WORKSPACE, TYPESCRIPT_LIB_DIRECTORY]),
    control.default_library === "barebones"
      ? "lib.d.ts"
      : control.default_library === "project-es5"
        ? "lib.es5.d.ts"
        : tsApi.getDefaultLibFileName(control.compiler_options),
  );
  const host = {
    ...baseHost,
    getCurrentDirectory: () => control.current_directory,
    useCaseSensitiveFileNames: () => true,
    getCanonicalFileName: (fileName) => fileName,
    getDefaultLibFileName: () => defaultLibrary,
    getDefaultLibLocation: () =>
      path.join(WORKSPACE, TYPESCRIPT_LIB_DIRECTORY),
    trace() {},
    fileExists(fileName) {
      const normalized = tsApi.normalizePath(fileName);
      return files.has(normalized) || baseHost.fileExists(normalized);
    },
    readFile(fileName) {
      const normalized = tsApi.normalizePath(fileName);
      if (files.has(normalized)) return files.get(normalized);
      return baseHost.readFile(normalized);
    },
    directoryExists(directory) {
      const normalized = tsApi.normalizePath(directory);
      return (
        hasVirtualDirectory(files, normalized) ||
        (baseHost.directoryExists?.(normalized) ?? false)
      );
    },
    getDirectories(directory) {
      const normalized = tsApi.normalizePath(directory);
      return hasVirtualDirectory(files, normalized)
        ? virtualDirectories(files, normalized)
        : (baseHost.getDirectories?.(normalized) ?? []);
    },
    realpath(fileName) {
      const normalized = tsApi.normalizePath(fileName);
      if (symlinkByPath.has(normalized)) return symlinkByPath.get(normalized);
      if (files.has(normalized)) return normalized;
      return baseHost.realpath?.(normalized) ?? normalized;
    },
    getSourceFile(fileName, languageVersion) {
      const normalized = tsApi.normalizePath(fileName);
      const text = files.get(normalized);
      if (text !== undefined) {
        return tsApi.createSourceFile(
          normalized,
          text,
          languageVersion,
          true,
          tsApi.getScriptKindFromFileName(normalized),
        );
      }
      return baseHost.getSourceFile(normalized, languageVersion);
    },
  };
  return tsApi.createProgram(
    control.roots,
    control.compiler_options,
    host,
  );
}

function serializeDiagnostic(tsApi, diagnostic) {
  return {
    code: diagnostic.code,
    category: tsApi.DiagnosticCategory[diagnostic.category],
    file: diagnostic.file ? tsApi.normalizePath(diagnostic.file.fileName) : null,
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
    message: tsApi.flattenDiagnosticMessageText(diagnostic.messageText, "\n"),
  };
}

function materializedWrite(tsApi, arguments_, index) {
  const [fileName, text, writeByteOrderMark] = arguments_;
  const callback = Buffer.from(text, "utf8");
  const materialized = writeByteOrderMark
    ? Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), callback])
    : callback;
  return {
    index,
    path: tsApi.normalizePath(fileName),
    materialized_utf8_sha256: sha256(materialized),
    materialized_utf8_bytes: materialized.length,
    write_byte_order_mark: !!writeByteOrderMark,
  };
}

function observeProgram(tsApi, control) {
  const program = createVirtualProgram(tsApi, control);
  const writes = [];
  const reportedDiagnostics = [];
  let emitResult;
  const originalEmit = program.emit;
  program.emit = function captureEmit(...arguments_) {
    requireCondition(emitResult === undefined, "TypeScript emitted more than once");
    emitResult = originalEmit.apply(this, arguments_);
    return emitResult;
  };
  tsApi.emitFilesAndReportErrorsAndGetExitStatus(
    program,
    (diagnostic) => reportedDiagnostics.push(diagnostic),
    () => {},
    undefined,
    (...arguments_) => writes.push(arguments_),
  );
  requireCondition(emitResult !== undefined, "TypeScript did not call Program.emit");
  return publicOutputProjection({
    writes: writes.map((arguments_, index) =>
      materializedWrite(tsApi, arguments_, index),
    ),
    reported_diagnostics: reportedDiagnostics.map((diagnostic) =>
      serializeDiagnostic(tsApi, diagnostic),
    ),
    emit_diagnostics: emitResult.diagnostics.map((diagnostic) =>
      serializeDiagnostic(tsApi, diagnostic),
    ),
    emit_skipped: emitResult.emitSkipped,
    emitted_files:
      emitResult.emittedFiles === undefined
        ? null
        : emitResult.emittedFiles.map((fileName) =>
            tsApi.normalizePath(fileName),
          ),
  });
}

const BAREBONES_LIB_CONTENT = `interface Boolean {}
interface Function {}
interface CallableFunction {}
interface NewableFunction {}
interface IArguments {}
interface Number {}
interface Object {}
interface RegExp {}
interface String {}
interface Array<T> { length: number; [n: number]: T; }
interface SymbolConstructor {
    (desc?: string | number): symbol;
    for(name: string): symbol;
    readonly toStringTag: symbol;
}
declare var Symbol: SymbolConstructor;
interface Symbol {
    readonly [Symbol.toStringTag]: string;
}`;

function transpileEffectiveOptions(rawOptions) {
  const options = {
    ...publicTs.getDefaultCompilerOptions(),
    ...rawOptions,
  };
  for (const option of publicTs.optionDeclarations) {
    if (!Object.hasOwn(option, "transpileOptionValue")) continue;
    if (
      options.verbatimModuleSyntax &&
      new Set(["isolatedModules"]).has(option.name)
    ) {
      continue;
    }
    options[option.name] = option.transpileOptionValue;
  }
  options.suppressOutputPathCheck = true;
  options.allowNonTsExtensions = true;
  options.declaration = true;
  options.emitDeclarationOnly = true;
  options.isolatedDeclarations = true;
  options.noLib = false;
  return options;
}

function observeTranspile(tsApi, control) {
  requireCondition(
    control.transpile_api === null ||
      control.transpile_api === "transpileDeclaration",
    `${control.case_id} unsupported probe transpile API ${control.transpile_api}`,
  );
  const outputs = [];
  for (const [index, input] of control.files.entries()) {
    const options = transpileEffectiveOptions(control.compiler_options);
    const barebonesPath = tsApi.normalizePath("lib.d.ts");
    const files = [
      { path: input.path, text: input.text },
      { path: barebonesPath, text: BAREBONES_LIB_CONTENT },
    ];
    const programControl = {
      ...control,
      files,
      roots: [input.path],
      compiler_options: options,
      current_directory: "",
      symlinks: [],
      default_library: "barebones",
    };
    const writes = [];
    const program = createVirtualProgram(tsApi, programControl);
    const diagnostics = [
      ...program.getSyntacticDiagnostics(),
      ...program.getOptionsDiagnostics(),
    ];
    const result = program.emit(
      undefined,
      (...arguments_) => writes.push(arguments_),
      undefined,
      true,
      undefined,
      true,
    );
    diagnostics.push(...result.diagnostics);
    const outputWrite = writes.find(
      ([fileName]) => !String(fileName).endsWith(".map"),
    );
    requireCondition(outputWrite !== undefined, `${control.case_id} transpile emitted no output`);
    const outputBytes = Buffer.from(outputWrite[1], "utf8");
    const sourceMapWrite = writes.find(([fileName]) =>
      String(fileName).endsWith(".map"),
    );
    outputs.push({
      index,
      path: input.path,
      output_utf8_sha256: sha256(outputBytes),
      output_utf8_bytes: outputBytes.length,
      source_map_json_utf8_sha256:
        sourceMapWrite === undefined
          ? null
          : sha256(Buffer.from(sourceMapWrite[1], "utf8")),
      diagnostics: diagnostics.map((diagnostic) =>
        serializeDiagnostic(tsApi, diagnostic),
      ),
    });
  }
  return publicOutputProjection({
    transpile_outputs: outputs,
    reported_diagnostics: outputs.flatMap((output) => output.diagnostics),
    emit_diagnostics: [],
    emit_skipped: false,
    emitted_files: null,
  });
}

function observeControl(tsApi, control) {
  return control.route === "transpile-api"
    ? observeTranspile(tsApi, control)
    : observeProgram(tsApi, control);
}

function validateTraceEvents(events, caseId) {
  requireCondition(Array.isArray(events), `${caseId} trace_events is not an array`);
  for (const [index, event] of events.entries()) {
    requireCondition(
      event !== null &&
        typeof event === "object" &&
        Object.keys(event).length === 2 &&
        typeof event.site_id === "string" &&
        event.site_id.length > 0 &&
        Array.isArray(event.args),
      `${caseId} trace event ${index} is malformed`,
    );
    for (const argument of event.args) {
      requireCondition(
        typeof argument === "string" ||
          typeof argument === "boolean" ||
          (typeof argument === "number" && Number.isInteger(argument)),
        `${caseId} trace event ${index} has a non-stable argument`,
      );
      if (typeof argument === "string") {
        requireCondition(
          !path.isAbsolute(argument) &&
            !argument.includes("/") &&
            !argument.includes("\\") &&
            !argument.includes(WORKSPACE) &&
            !argument.includes("target/h2-7a-probe"),
          `${caseId} trace event ${index} leaks a path`,
        );
      }
    }
  }
}

function runInternalObservation(contextPath, caseId) {
  const context = JSON.parse(fs.readFileSync(contextPath, "utf8"));
  const selected = context.cases.find((entry) => entry.case_id === caseId);
  requireCondition(selected !== undefined, `unknown internal probe case ${caseId}`);
  const traceEvents = [];
  globalThis.__H2_7A_INTERNAL__ = true;
  globalThis.__H2_7A_TRACE__ = (siteId, ...args) => {
    traceEvents.push({ site_id: siteId, args });
  };
  require(path.join(WORKSPACE, INSTRUMENTED_RELATIVE_PATH));
  const tsApi = globalThis.__H2_7A_TS__;
  requireCondition(tsApi?.version === "6.0.3", "instrumented TypeScript export unavailable");
  const publicOutputs = observeControl(tsApi, selected.control);
  validateTraceEvents(traceEvents, caseId);
  process.stdout.write(
    JSON.stringify({
      trace_events: traceEvents,
      public_outputs: publicOutputs,
    }),
  );
}

function writeObservationContext(witnessContext) {
  const absolute = path.join(WORKSPACE, OBSERVE_CONTEXT_RELATIVE_PATH);
  writeFileAtomic(
    absolute,
    render({
      schema: 1,
      instrumented_output_sha256: sha256(
        readBytes(INSTRUMENTED_RELATIVE_PATH),
      ),
      cases: witnessContext.cases.map(({ case_id, control }) => ({
        case_id,
        control,
      })),
    }),
  );
  return absolute;
}

function observeCaseInFreshProcess(contextPath, caseId) {
  const stdout = execFileSync(
    process.execPath,
    [GENERATOR_PATH, INTERNAL_OBSERVE_MODE, contextPath, caseId],
    {
      cwd: WORKSPACE,
      encoding: "utf8",
      maxBuffer: 512 * 1024 * 1024,
    },
  );
  const observation = JSON.parse(stdout);
  validateTraceEvents(observation.trace_events, caseId);
  requireCondition(
    observation.public_outputs !== null &&
      typeof observation.public_outputs === "object",
    `${caseId} child returned invalid public outputs`,
  );
  return observation;
}

function observeProbeCases(witnessContext) {
  const contextPath = writeObservationContext(witnessContext);
  return witnessContext.cases.map((expected) => {
    const first = observeCaseInFreshProcess(contextPath, expected.case_id);
    const second = observeCaseInFreshProcess(contextPath, expected.case_id);
    requireCondition(
      stableStringify(first.trace_events) ===
        stableStringify(second.trace_events),
      `${expected.case_id} trace-event sequence is nondeterministic`,
    );
    requireCondition(
      stableStringify(first.public_outputs) ===
        stableStringify(second.public_outputs),
      `${expected.case_id} instrumented public outputs are nondeterministic`,
    );
    requireCondition(
      stableStringify(first.public_outputs) ===
        stableStringify(expected.expected_public_outputs),
      `${expected.case_id} instrumentation is not inert: public outputs differ from lane B`,
    );
    const roll = sha256(
      Buffer.from(stableStringify(first.public_outputs), "utf8"),
    );
    requireCondition(
      roll === expected.public_output_roll,
      `${expected.case_id} public output roll differs from lane B`,
    );
    return {
      case_id: expected.case_id,
      trace_events: first.trace_events,
      public_output_roll: roll,
      inert: true,
    };
  });
}

function traceContentRoll(cases) {
  return sha256(
    Buffer.from(
      stableStringify(
        cases.map(({ case_id, trace_events }) => ({
          case_id,
          trace_events,
        })),
      ),
      "utf8",
    ),
  );
}

function summaryForCases(cases) {
  const counts = new Map();
  let events = 0;
  for (const entry of cases) {
    events += entry.trace_events.length;
    for (const event of entry.trace_events) {
      counts.set(event.site_id, (counts.get(event.site_id) ?? 0) + 1);
    }
  }
  return {
    cases: cases.length,
    events,
    per_site_counts: Object.fromEntries(
      [...counts].sort(([left], [right]) => left.localeCompare(right)),
    ),
    trace_content_roll: traceContentRoll(cases),
  };
}

function buildArtifact(witnessContext, instrumentation, cases) {
  return withFingerprint(
    {
      schema: 1,
      phase: PHASE,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      witnesses: witnessContext.path_hash,
      base_sha256: instrumentation.base_sha256,
      edit_table_sha256: instrumentation.edit_table_sha256,
      instrumented_output_sha256:
        instrumentation.instrumented_output_sha256,
      position_map_sha256: instrumentation.position_map_sha256,
      case_manifest_fingerprint:
        witnessContext.case_manifest_fingerprint,
      cases,
      summary: summaryForCases(cases),
    },
    "probe_traces_fingerprint_sha256",
  );
}

function validateArtifact(artifact, witnessContext, instrumentation) {
  requireCondition(
    artifact !== null &&
      typeof artifact === "object" &&
      artifact.schema === 1 &&
      artifact.phase === PHASE &&
      hasValidFingerprint(artifact, "probe_traces_fingerprint_sha256"),
    "probe artifact schema/fingerprint is invalid",
  );
  const expectedKeys = [
    "schema",
    "phase",
    "generator",
    "contract",
    "witnesses",
    "base_sha256",
    "edit_table_sha256",
    "instrumented_output_sha256",
    "position_map_sha256",
    "case_manifest_fingerprint",
    "cases",
    "summary",
    "probe_traces_fingerprint_sha256",
  ].sort();
  requireCondition(
    stableStringify(Object.keys(artifact).sort()) ===
      stableStringify(expectedKeys),
    "probe artifact has fields outside schema 1",
  );
  requireCondition(
    stableStringify(artifact.generator) ===
      stableStringify(pathHash(GENERATOR_RELATIVE_PATH)) &&
      stableStringify(artifact.contract) ===
        stableStringify(pathHash(CONTRACT_RELATIVE_PATH)) &&
      stableStringify(artifact.witnesses) ===
        stableStringify(witnessContext.path_hash),
    "probe artifact pathHash pin is stale",
  );
  for (const [field, expected] of Object.entries({
    base_sha256: instrumentation.base_sha256,
    edit_table_sha256: instrumentation.edit_table_sha256,
    instrumented_output_sha256:
      instrumentation.instrumented_output_sha256,
    position_map_sha256: instrumentation.position_map_sha256,
    case_manifest_fingerprint:
      witnessContext.case_manifest_fingerprint,
  })) {
    requireCondition(
      isSha256(artifact[field]) && artifact[field] === expected,
      `probe artifact ${field} is stale`,
    );
  }
  requireCondition(
    Array.isArray(artifact.cases) &&
      artifact.cases.length === witnessContext.cases.length,
    "probe artifact case denominator differs from lane B",
  );
  artifact.cases.forEach((entry, index) => {
    const expected = witnessContext.cases[index];
    requireCondition(
      entry !== null &&
        typeof entry === "object" &&
        stableStringify(Object.keys(entry).sort()) ===
          stableStringify(
            ["case_id", "trace_events", "public_output_roll", "inert"].sort(),
          ) &&
        entry.case_id === expected.case_id &&
        entry.inert === true &&
        entry.public_output_roll === expected.public_output_roll,
      `probe artifact case ${index} differs from lane-B manifest/public output`,
    );
    validateTraceEvents(entry.trace_events, entry.case_id);
  });
  requireCondition(
    stableStringify(artifact.summary) ===
      stableStringify(summaryForCases(artifact.cases)),
    "probe artifact summary is stale",
  );
  return artifact;
}

class CheckReceiptMiss extends Error {}

function receiptKey(witnessContext, instrumentation, cases) {
  return {
    generator_sha256: pathHash(GENERATOR_RELATIVE_PATH).sha256,
    node: process.version,
    vendored_bundle_libs_sha256: vendoredRuntimeRoll(),
    case_manifest_fingerprint:
      witnessContext.case_manifest_fingerprint,
    edit_table_sha256: instrumentation.edit_table_sha256,
    instrumented_output_sha256:
      instrumentation.instrumented_output_sha256,
    trace_content_roll: traceContentRoll(cases),
  };
}

function reusableReceiptCases(witnessContext, instrumentation) {
  let artifact;
  let receipt;
  try {
    artifact = readJson(TARGET_RELATIVE_PATH);
    receipt = readJson(CHECK_RECEIPT_RELATIVE_PATH);
  } catch {
    throw new CheckReceiptMiss("absent-or-invalid");
  }
  try {
    softValidation = true;
    validateArtifact(artifact, witnessContext, instrumentation);
  } catch {
    throw new CheckReceiptMiss("stored-artifact");
  } finally {
    softValidation = false;
  }
  if (
    receipt === null ||
    typeof receipt !== "object" ||
    receipt.schema !== 1 ||
    receipt.kind !== "h2-7a-probe-traces-check-receipt" ||
    receipt.minted_by !== "full-re-observation-check" ||
    receipt.workspace !== fs.realpathSync(WORKSPACE) ||
    !hasValidFingerprint(receipt, "receipt_fingerprint_sha256")
  ) {
    throw new CheckReceiptMiss("receipt-shape");
  }
  const key = receiptKey(witnessContext, instrumentation, artifact.cases);
  for (const [field, expected] of Object.entries(key)) {
    if (receipt[field] !== expected) throw new CheckReceiptMiss(field);
  }
  return artifact.cases;
}

function mintCheckReceipt(witnessContext, instrumentation, artifact) {
  const receipt = withFingerprint(
    {
      schema: 1,
      kind: "h2-7a-probe-traces-check-receipt",
      minted_by: "full-re-observation-check",
      workspace: fs.realpathSync(WORKSPACE),
      ...receiptKey(witnessContext, instrumentation, artifact.cases),
    },
    "receipt_fingerprint_sha256",
  );
  writeFileAtomic(
    path.join(WORKSPACE, CHECK_RECEIPT_RELATIVE_PATH),
    render(receipt),
  );
}

function ordinarySelftestObservation(control) {
  if (control.route === "transpile-api") {
    const outputs = control.files.map((input, index) => {
      const output = publicTs.transpileDeclaration(input.text, {
        compilerOptions: control.compiler_options,
        fileName: input.path,
        reportDiagnostics: true,
      });
      const outputBytes = Buffer.from(output.outputText, "utf8");
      return {
        index,
        path: input.path,
        output_utf8_sha256: sha256(outputBytes),
        output_utf8_bytes: outputBytes.length,
        source_map_json_utf8_sha256:
          output.sourceMapText == null
            ? null
            : sha256(Buffer.from(output.sourceMapText, "utf8")),
        diagnostics: (output.diagnostics ?? []).map((diagnostic) =>
          serializeDiagnostic(publicTs, diagnostic),
        ),
      };
    });
    return publicOutputProjection({
      transpile_outputs: outputs,
      reported_diagnostics: outputs.flatMap((output) => output.diagnostics),
      emit_diagnostics: [],
      emit_skipped: false,
      emitted_files: null,
    });
  }
  return observeProgram(publicTs, control);
}

function makeSelftestWitnesses() {
  const caseSpecs = [
    {
      case_id: "selftest-annotated-and-inferred",
      role: "positive",
      lanes: ["type-serialization", "syntactic-builder-arms", "ast-provenance"],
      current_directory: "/project",
      roots: ["/project/main.ts"],
      compiler_options: {
        declaration: true,
        emitDeclarationOnly: true,
        listEmittedFiles: true,
        module: publicTs.ModuleKind.ESNext,
        target: publicTs.ScriptTarget.ES2022,
        strict: true,
      },
      files: [
        {
          path: "/project/main.ts",
          text: [
            "export const annotated: string = 'x';",
            "export const inferred = { count: 1 as const, label: 'ok' };",
            "export function choose(flag: boolean) { return flag ? inferred : { count: 2 as const, label: 'no' }; }",
            "",
          ].join("\n"),
        },
      ],
    },
    {
      case_id: "selftest-js-expando-and-computed",
      role: "composition",
      lanes: ["js-declaration-synthesis", "symbol-tracking", "generated-global-names"],
      current_directory: "/project",
      roots: ["/project/expando.js"],
      compiler_options: {
        allowJs: true,
        checkJs: true,
        declaration: true,
        emitDeclarationOnly: true,
        listEmittedFiles: true,
        module: publicTs.ModuleKind.CommonJS,
        target: publicTs.ScriptTarget.ES2020,
      },
      files: [
        {
          path: "/project/expando.js",
          text: [
            "/** @param {number} value */",
            "function box(value) { return { value }; }",
            "box.extra = 'stable';",
            "module.exports = box;",
            "",
          ].join("\n"),
        },
      ],
    },
    {
      case_id: "selftest-transpile-declaration",
      role: "adjacent",
      lanes: ["diagnostics-channel", "upstream-observation-controls"],
      route: "transpile-api",
      transpile_api: "transpileDeclaration",
      current_directory: "/project",
      roots: ["/project/transpile.ts"],
      compiler_options: {
        module: publicTs.ModuleKind.ESNext,
        target: publicTs.ScriptTarget.ES2022,
      },
      files: [
        {
          path: "/project/transpile.ts",
          text: "export const value: { readonly answer: 42 } = { answer: 42 };\n",
        },
      ],
    },
  ];
  for (const entry of caseSpecs) {
    for (const source of entry.files) {
      source.utf8_sha256 = sha256(Buffer.from(source.text, "utf8"));
    }
  }
  const controls = caseSpecs.map((entry) =>
    normalizeCaseControl(entry),
  );
  const cases = caseSpecs.map((entry, index) => ({
    case_id: entry.case_id,
    public_observation: ordinarySelftestObservation(controls[index]),
  }));
  const manifestPayload = { cases: caseSpecs };
  const caseManifest = {
    ...manifestPayload,
    case_manifest_fingerprint: sha256(
      Buffer.from(stableStringify(manifestPayload), "utf8"),
    ),
  };
  return {
    schema: 1,
    phase: "H2.7a-public-witnesses-selftest",
    case_manifest: caseManifest,
    cases,
  };
}

function runSelftest() {
  validateRuntime();
  validateContractDocument();
  const instrumentation = applyExactEdits();
  const witnesses = makeSelftestWitnesses();
  writeFileAtomic(
    path.join(WORKSPACE, SELFTEST_WITNESSES_RELATIVE_PATH),
    render(witnesses),
  );
  const witnessContext = loadWitnessContext(SELFTEST_WITNESSES_RELATIVE_PATH);
  const cases = observeProbeCases(witnessContext);
  const artifact = buildArtifact(witnessContext, instrumentation, cases);
  validateArtifact(artifact, witnessContext, instrumentation);
  requireCondition(
    artifact.summary.events > 0 &&
      artifact.summary.per_site_counts["probe.bootstrap"] === cases.length,
    "selftest collected no valid instrumented traces",
  );
  writeFileAtomic(
    path.join(WORKSPACE, SELFTEST_ARTIFACT_RELATIVE_PATH),
    render(artifact),
  );
  process.stdout.write(
    `H2.7a probe selftest is green: cases=${artifact.summary.cases} events=${artifact.summary.events} sites=${Object.keys(artifact.summary.per_site_counts).length} inert=all double_observation=identical\n`,
  );
}

function runWrite() {
  validateRuntime();
  validateContractDocument();
  const witnessContext = loadWitnessContext();
  const instrumentation = applyExactEdits();
  const cases = observeProbeCases(witnessContext);
  const artifact = buildArtifact(witnessContext, instrumentation, cases);
  validateArtifact(artifact, witnessContext, instrumentation);
  writeFileAtomic(path.join(WORKSPACE, TARGET_RELATIVE_PATH), render(artifact));
  process.stdout.write(
    `wrote ${TARGET_RELATIVE_PATH}: cases=${artifact.summary.cases} events=${artifact.summary.events} receipt=not-minted\n`,
  );
}

function runCheck() {
  validateRuntime();
  validateContractDocument();
  const witnessContext = loadWitnessContext();
  const instrumentation = applyExactEdits();
  let cases;
  let receiptHit = false;
  try {
    cases = reusableReceiptCases(witnessContext, instrumentation);
    receiptHit = true;
    process.stderr.write(
      `H2.7a probe check receipt: hit; adopted ${cases.length} stored trace observations\n`,
    );
  } catch (error) {
    if (!(error instanceof CheckReceiptMiss)) throw error;
    process.stderr.write(
      `H2.7a probe check receipt: miss (${error.message}); running full fresh-process double observation\n`,
    );
    cases = observeProbeCases(witnessContext);
  }
  const artifact = buildArtifact(witnessContext, instrumentation, cases);
  validateArtifact(artifact, witnessContext, instrumentation);
  const rendered = render(artifact);
  requireCondition(
    fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
      fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") ===
        rendered,
    `stale ${TARGET_RELATIVE_PATH}; run h2-7a-probe-traces.mjs --write and review`,
  );
  if (!receiptHit) mintCheckReceipt(witnessContext, instrumentation, artifact);
  process.stdout.write(
    `H2.7a probe traces are fresh: cases=${artifact.summary.cases} events=${artifact.summary.events} receipt=${receiptHit ? "hit" : "minted"}\n`,
  );
}

const mode = process.argv[2];
if (mode === INTERNAL_OBSERVE_MODE) {
  requireCondition(
    process.argv.length === 5,
    "internal observation requires context path and case id",
  );
  runInternalObservation(process.argv[3], process.argv[4]);
} else if (mode === "--selftest") {
  requireCondition(process.argv.length === 3, "--selftest takes no arguments");
  runSelftest();
} else if (mode === "--write") {
  requireCondition(process.argv.length === 3, "--write takes no arguments");
  runWrite();
} else if (mode === "--check") {
  requireCondition(process.argv.length === 3, "--check takes no arguments");
  runCheck();
} else {
  fail("usage: node crates/oracle/h2-7a-probe-traces.mjs --write|--check|--selftest");
}
