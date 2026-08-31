// H2.7a m-2: pinned, inert, schema-2 declaration-probe observations.
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
const INTERNAL_V1_PROJECTION_MODE = "--internal-v1-projection";
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

const PROBE_RUNTIME_LINE = [
  "const __h27aSyntacticFrames = [];",
  "const __h27aCallStack = [];",
  "let __h27aEventSeq = 0;",
  "let __h27aNextCallId = 0;",
  "const __h27aConfig = globalThis.__H2_7A_PROBE_CONFIG__ || { source_paths: [], source_aliases: [] };",
  "const __h27aNormalizePath = (value) => String(value).replace(/\\\\/g, \"/\");",
  "const __h27aSourceIndex = new Map((__h27aConfig.source_paths || []).map((value, index) => [__h27aNormalizePath(value), index]));",
  "for (const entry of __h27aConfig.source_aliases || []) __h27aSourceIndex.set(__h27aNormalizePath(entry[0]), entry[1]);",
  "const __h27aFileTable = [];",
  "const __h27aFileTags = new Map();",
  "globalThis.__H2_7A_PROBE_FILE_TABLE__ = __h27aFileTable;",
  "const __h27aNode = (value) => value && typeof value.kind === \"number\" ? value : void 0;",
  "const __h27aString = (value) => typeof value === \"string\" ? value : \"\";",
  "const __h27aName = (value) => typeof value === \"number\" ? String(value) : typeof value === \"string\" ? value : value && (typeof value.escapedName === \"string\" || typeof value.escapedName === \"number\") ? String(value.escapedName) : \"\";",
  "function __h27aInteger(value) { if (!Number.isSafeInteger(value)) throw new Error(`h2-7a probe unsafe integer ${value}`); return value; }",
  "const __h27aScalar = (value) => [typeof value, typeof value === \"string\" ? value : \"\", typeof value === \"number\" ? __h27aInteger(value) : 0, typeof value === \"boolean\" ? value : false];",
  "function __h27aInternSourceFile(node) { const sourceFile = getSourceFileOfNode(node); if (!sourceFile || typeof sourceFile.fileName !== \"string\") throw new Error(\"h2-7a probe node has no source file\"); const fileName = __h27aNormalizePath(sourceFile.fileName); const sourceKey = __h27aSourceIndex.get(fileName); let row; if (sourceKey !== void 0) row = [\"src\", sourceKey]; else { const baseName = fileName.slice(fileName.lastIndexOf(\"/\") + 1); if (!/^lib(?:\\..*)?\\.d\\.ts$/.test(baseName)) throw new Error(`h2-7a probe unclassified source file ${fileName}`); row = [\"lib\", baseName]; } const identity = `${row[0]}\\0${row[1]}`; let tag = __h27aFileTags.get(identity); if (tag === void 0) { tag = __h27aFileTable.length; __h27aFileTags.set(identity, tag); __h27aFileTable.push(row); } return tag; }",
  "const __h27aSentinelNodeRef = () => [-1, -1, -1, -1, -1, -1, -1, -1];",
  "function __h27aNodeRef(value) { const node = __h27aNode(value); if (!node) return __h27aSentinelNodeRef(); const original = getParseTreeNode(node); if (original === node) return [__h27aInternSourceFile(node), __h27aInteger(node.kind), __h27aInteger(node.pos), __h27aInteger(node.end), -1, -1, -1, -1]; if (original) return [-1, -1, -1, -1, __h27aInternSourceFile(original), __h27aInteger(original.kind), __h27aInteger(original.pos), __h27aInteger(original.end)]; return __h27aSentinelNodeRef(); }",
  "function __h27aSymbolRef(value) { const declarations = Array.isArray(value && value.declarations) ? value.declarations : []; return [__h27aName(value), declarations.length, declarations.slice(0, 8).map(__h27aNodeRef)]; }",
  "function __h27aEmit(site, callId, depth, args) { const hook = globalThis.__H2_7A_TRACE__; if (hook) hook(site, __h27aEventSeq++, callId, depth, [site, ...args]); }",
  "const __h27aTrace = (site, ...args) => __h27aEmit(site, -1, __h27aCallStack.length, args);",
  "function __h27aBeginCall(site, args) { const callId = __h27aNextCallId++; __h27aCallStack.push({ callId, site }); __h27aEmit(`${site}.entry`, callId, __h27aCallStack.length, args); return callId; }",
  "function __h27aEndCall(site, callId, args) { const frame = __h27aCallStack[__h27aCallStack.length - 1]; if (!frame || frame.callId !== callId || frame.site !== site) throw new Error(`h2-7a probe call stack mismatch at ${site}`); __h27aEmit(`${site}.result`, callId, __h27aCallStack.length, args); __h27aCallStack.pop(); }",
  "function __h27aEntryArgs(site, args) { const first = args[0]; const second = args[1]; if (site === \"resolver.isSymbolAccessible\") return [args.length, __h27aSymbolRef(first), __h27aNodeRef(second), args[2], !!args[3]]; if (site === \"resolver.isEntityNameVisible\") return [args[3] ? 3 : 2, __h27aNodeRef(first), __h27aNodeRef(second), !!args[2], !!args[3]]; if (site === \"resolver.collectLinkedAliases\") return [__h27aNodeRef(first), !!second]; return [args.length, __h27aName(first), __h27aNodeRef(first), __h27aNodeRef(second), __h27aScalar(first), __h27aScalar(second)]; }",
  "function __h27aAccessibilityResult(result) { const errorNode = __h27aNodeRef(result && result.errorNode); const aliases = result && Object.prototype.hasOwnProperty.call(result, \"aliasesToMakeVisible\") ? Array.isArray(result.aliasesToMakeVisible) ? result.aliasesToMakeVisible.map(__h27aNodeRef) : [] : null; return [result ? __h27aInteger(result.accessibility) : -1, result && typeof result.errorSymbolName === \"string\" ? result.errorSymbolName : \"\", result && typeof result.errorModuleName === \"string\" ? result.errorModuleName : null, errorNode, aliases]; }",
  "function __h27aResultArgs(site, result) { if (site === \"resolver.isSymbolAccessible\" || site === \"resolver.isEntityNameVisible\") return __h27aAccessibilityResult(result); if (site === \"resolver.getPropertiesOfContainerFunction\") return [(Array.isArray(result) ? result : []).map((property) => [__h27aName(property), __h27aSymbolRef(property && property.parent), property && property.valueDeclaration ? __h27aNodeRef(property.valueDeclaration) : null])]; if (site === \"resolver.getEnumMemberValue\") { const value = result && result.value; if (!(value === void 0 || value === null || typeof value === \"string\" || typeof value === \"boolean\" || typeof value === \"number\" && Number.isSafeInteger(value))) throw new Error(\"h2-7a probe enum value is not JSON-safe\"); return [typeof value, value === void 0 ? null : value, !!(result && result.isSyntacticallyString)]; } if (site === \"resolver.collectLinkedAliases\") return [\"void\"]; const value = result && typeof result === \"object\" ? result.value : void 0; return [__h27aScalar(result), result == null, __h27aNodeRef(result), Array.isArray(result) ? result.length : -1, __h27aScalar(value), !!(result && result.isSyntacticallyString)]; }",
  "function __h27aProbeCall(site, args, body) { const callId = __h27aBeginCall(site, __h27aEntryArgs(site, args)); let result; try { result = body(); } catch (error) { __h27aCallStack.pop(); throw error; } __h27aEndCall(site, callId, __h27aResultArgs(site, result)); return result; }",
  "function __h27aProbeSyntacticCall(site, args, body) { const frame = { fallback: false }; const callId = __h27aBeginCall(site, [__h27aNodeRef(args[0])]); __h27aSyntacticFrames.push(frame); let result; try { result = body(); } catch (error) { __h27aSyntacticFrames.pop(); __h27aCallStack.pop(); throw error; } __h27aSyntacticFrames.pop(); __h27aEndCall(site, callId, [!frame.fallback, frame.fallback, __h27aNodeRef(result)]); return result; }",
  "function __h27aMarkSyntacticFallback(source, node, reportFallback) { for (const frame of __h27aSyntacticFrames) frame.fallback = true; __h27aTrace(`${source}.checkerFallback`, !!reportFallback, __h27aNodeRef(node)); }",
  "function __h27aProbeTransform(site, input, body) { const output = body(); if (output !== input) { const outputs = Array.isArray(output) ? output : [output]; if (outputs.length === 0) __h27aTrace(`${site}.changed`, __h27aNodeRef(input), __h27aSentinelNodeRef(), false, 0); for (const candidate of outputs) { const node = __h27aNode(candidate); __h27aTrace(`${site}.changed`, __h27aNodeRef(input), __h27aNodeRef(node), !!(node && node.original), node && typeof node.transformFlags === \"number\" ? __h27aInteger(node.transformFlags) : 0); } } return output; }",
  "const __h27aVisibleNodes = new Map();",
  "function __h27aVisibleWrite(site, node, value) { __h27aVisibleNodes.set(node, !!value); __h27aTrace(site, __h27aNodeRef(node), !!value); }",
  "function __h27aSeed(site) { const rows = []; for (const [node, value] of __h27aVisibleNodes) rows.push([__h27aNodeRef(node), value]); const coordinate = (ref) => ref[0] >= 0 ? [ref[0], ref[2], ref[3], ref[1]] : [ref[4], ref[6], ref[7], ref[5]]; rows.sort((left, right) => { const a = coordinate(left[0]); const b = coordinate(right[0]); return a[0] - b[0] || a[1] - b[1] || a[2] - b[2] || a[3] - b[3]; }); __h27aTrace(site, rows); }",
  "/* site-id: probe.runtime */",
].join(" ");

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
    insert_before: '        __h27aTrace("nodebuilder.moduleSpecifierOverride.contextArm", context.bundled || context.enclosingFile !== getSourceFileOfNode(lit) ? "rewrite" : "skip", !!context.bundled, context.enclosingFile !== getSourceFileOfNode(lit), __h27aNodeRef(parent)); /* site-id: nodebuilder.moduleSpecifierOverride.contextArm */',
  },
  {
    anchor_line: 50910,
    expect: "          if (parentSymbol && isExternalModuleSymbol(parentSymbol)) {",
    insert_after: '            __h27aTrace("nodebuilder.moduleSpecifierOverride.sourceArm", "parent-symbol", __h27aNodeRef(parent)); /* site-id: nodebuilder.moduleSpecifierOverride.sourceArm */',
  },
  {
    anchor_line: 50914,
    expect: "            if (targetFile) {",
    insert_before: '            __h27aTrace("nodebuilder.moduleSpecifierOverride.sourceArm", targetFile ? "target-file" : "no-target", __h27aNodeRef(parent)); /* site-id: nodebuilder.moduleSpecifierOverride.sourceArm */',
  },
  {
    anchor_line: 50918,
    expect: '          if (name.includes("/node_modules/")) {',
    insert_after: '            __h27aTrace("nodebuilder.moduleSpecifierOverride.unsafe", true, !!nodeSymbol); /* site-id: nodebuilder.moduleSpecifierOverride.unsafe */',
  },
  {
    anchor_line: 50924,
    expect: "          if (name !== originalName) {",
    insert_after: '            __h27aTrace("nodebuilder.moduleSpecifierOverride.resultArm", "override", __h27aNodeRef(parent)); /* site-id: nodebuilder.moduleSpecifierOverride.resultArm */',
  },
  {
    anchor_line: 50927,
    expect: "        }",
    insert_before: '          __h27aTrace("nodebuilder.moduleSpecifierOverride.resultArm", "unchanged", __h27aNodeRef(parent)); /* site-id: nodebuilder.moduleSpecifierOverride.resultArm */',
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
    insert_before: '      __h27aTrace("nodebuilder.withContext.result", context.encounteredError ? "error" : resultingNode === void 0 ? "fallback-undefined" : "node", context.flags, context.internalFlags, context.approximateLength, context.typeStack.length, !!context.truncating, !!context.out.truncated, !!context.encounteredError, __h27aNodeRef(resultingNode)); /* site-id: nodebuilder.withContext.result */',
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
  {
    anchor_line: 50589,
    expect: "        getNodeLinks(declaration).isVisible = true;",
    insert_after: '        __h27aVisibleWrite("isVisible.addVisibleAlias", declaration, true); /* site-id: isVisible.addVisibleAlias */',
  },
  {
    anchor_line: 50606,
    expect: "  function isEntityNameVisible(entityName, enclosingDeclaration, shouldComputeAliasToMakeVisible = true) {",
    insert_after: '    return __h27aProbeCall("resolver.isEntityNameVisible", [entityName, enclosingDeclaration, shouldComputeAliasToMakeVisible, arguments.length >= 3], () => { /* site-id: resolver.isEntityNameVisible.entry + resolver.isEntityNameVisible.result */',
  },
  {
    anchor_line: 50648,
    expect: "  }",
    insert_before: '    }); /* site-id: resolver.isEntityNameVisible.result */',
  },
  ...exactCallProbe({
    siteId: "resolver.isDeclarationVisible",
    startLine: 55589,
    startExpect: "  function isDeclarationVisible(node) {",
    endLine: 55674,
    endExpect: "  }",
    indent: "    ",
  }),
  {
    anchor_line: 55593,
    expect: "        links.isVisible = !!determineIfDeclarationIsVisible();",
    insert_after: '        __h27aVisibleWrite("isVisible.memo", node, links.isVisible); /* site-id: isVisible.memo */',
  },
  ...exactCallProbe({
    siteId: "resolver.collectLinkedAliases",
    startLine: 55675,
    startExpect: "  function collectLinkedAliases(node, setVisibility) {",
    endLine: 55727,
    endExpect: "  }",
    indent: "    ",
  }),
  {
    anchor_line: 55702,
    expect: "          getNodeLinks(declaration).isVisible = true;",
    insert_after: '          __h27aVisibleWrite("isVisible.collectLinkedAliases", declaration, true); /* site-id: isVisible.collectLinkedAliases */',
  },
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
    insert_after: '    __h27aTrace("tracker.reportInferenceFallback", __h27aNodeRef(node)); /* site-id: tracker.reportInferenceFallback */',
  },
  {
    anchor_line: 114360,
    expect: "  function trackSymbol(symbol, enclosingDeclaration2, meaning) {",
    insert_after: '    __h27aTrace("tracker.trackSymbol", __h27aName(symbol), __h27aNodeRef(enclosingDeclaration2), meaning); /* site-id: tracker.trackSymbol */',
  },
  {
    anchor_line: 114371,
    expect: "  function reportPrivateInBaseOfClassExpression(propertyName) {",
    insert_after: '    __h27aTrace("tracker.reportPrivateInBaseOfClassExpression", __h27aName(propertyName), __h27aNodeRef(propertyName)); /* site-id: tracker.reportPrivateInBaseOfClassExpression */',
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
    insert_after: '    __h27aTrace("tracker.reportNonlocalAugmentation", __h27aNodeRef(containingFile), __h27aName(parentSymbol), __h27aName(symbol), Array.isArray(symbol && symbol.declarations) ? symbol.declarations.length : 0); /* site-id: tracker.reportNonlocalAugmentation */',
  },
  {
    anchor_line: 114426,
    expect: "  function reportNonSerializableProperty(propertyName) {",
    insert_after: '    __h27aTrace("tracker.reportNonSerializableProperty", __h27aName(propertyName), __h27aNodeRef(propertyName)); /* site-id: tracker.reportNonSerializableProperty */',
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

  // Emit-orchestration state markers. These observe the complete writer
  // closure without changing the vendored program or its source inputs.
  {
    anchor_line: 116648,
    expect: "    const inputListOrBundle = compilerOptions.outFile ? [factory.createBundle(filesForEmit)] : filesForEmit;",
    insert_after: '    __h27aSeed("probe.checkSeed"); /* site-id: probe.checkSeed */',
  },
  {
    anchor_line: 116650,
    expect: "      if (emitOnly && !getEmitDeclarations(compilerOptions) || compilerOptions.noCheck || emitResolverSkipsTypeChecking(emitOnly, forceDtsEmit) || !canIncludeBindAndCheckDiagnostics(sourceFile, compilerOptions)) {",
    insert_after: '        __h27aTrace("probe.fallbackSweep", __h27aNodeRef(sourceFile), emitOnly && !getEmitDeclarations(compilerOptions) ? "emitOnly-without-declarations" : compilerOptions.noCheck ? "noCheck" : emitResolverSkipsTypeChecking(emitOnly, forceDtsEmit) ? "resolver-skips-type-checking" : "diagnostics-excluded"); /* site-id: probe.fallbackSweep */',
  },
  {
    anchor_line: 116654,
    expect: "    const declarationTransform = transformNodes(",
    insert_before: '    __h27aSeed("probe.transformSeed"); /* site-id: probe.transformSeed */',
  },

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
      schema?.properties?.schema?.const === 2 &&
      schema?.additionalProperties === false,
    `${CONTRACT_RELATIVE_PATH} does not describe ${PHASE} schema 2`,
  );
  const expectedSites = [
    ...[...CALL_SITES].flatMap((site) => [`${site}.entry`, `${site}.result`]),
    ...NON_CALL_SITES,
  ].sort();
  requireCondition(
    stableStringify(schema?.$defs?.traceEvent?.properties?.site_id?.enum?.sort()) ===
      stableStringify(expectedSites),
    `${CONTRACT_RELATIVE_PATH} site enum differs from the generator`,
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

const LEGACY_RESOLVER_CALL_SITES = Object.freeze([
  "resolver.isDefinitelyReferenceToGlobalSymbolObject",
  "resolver.isSymbolAccessible",
  "resolver.isEntityNameVisible",
  "resolver.isDeclarationVisible",
  "resolver.isOptionalParameter",
  "resolver.isImplementationOfOverload",
  "resolver.requiresAddingImplicitUndefined",
  "resolver.isExpandoFunctionDeclaration",
  "resolver.getPropertiesOfContainerFunction",
  "resolver.getEnumMemberValue",
  "resolver.createTypeOfDeclaration",
  "resolver.createReturnTypeOfSignatureDeclaration",
  "resolver.createTypeOfExpression",
  "resolver.hasGlobalName",
  "resolver.isLiteralConstDeclaration",
  "resolver.createLiteralConstValue",
  "resolver.isLateBound",
  "resolver.getDeclarationStatementsForSourceFile",
  "resolver.createLateBoundIndexSignatures",
  "resolver.isImportRequiredByAugmentation",
]);

const SYNTACTIC_CALL_SITES = Object.freeze([
  "syntactic.serializeTypeOfDeclaration",
  "syntactic.serializeReturnTypeForSignature",
]);

const CALL_SITES = new Set([
  ...LEGACY_RESOLVER_CALL_SITES,
  "resolver.collectLinkedAliases",
  ...SYNTACTIC_CALL_SITES,
]);

const NON_CALL_SITES = new Set([
  "nodebuilder.moduleSpecifierOverride.contextArm",
  "nodebuilder.moduleSpecifierOverride.sourceArm",
  "nodebuilder.moduleSpecifierOverride.unsafe",
  "nodebuilder.moduleSpecifierOverride.resultArm",
  "nodebuilder.withContext.decision",
  "nodebuilder.withContext.result",
  "tracker.reportInferenceFallback",
  "tracker.trackSymbol",
  "tracker.reportPrivateInBaseOfClassExpression",
  "tracker.reportInaccessibleUniqueSymbolError",
  "tracker.reportCyclicStructureError",
  "tracker.reportInaccessibleThisError",
  "tracker.reportLikelyUnsafeImportRequiredError",
  "tracker.reportTruncationError",
  "tracker.reportNonlocalAugmentation",
  "tracker.reportNonSerializableProperty",
  "declarations.visitDeclarationSubtree.changed",
  "declarations.transformTopLevelDeclaration.changed",
  "declarations.declBlocked",
  "syntactic.serializeTypeOfDeclaration.checkerFallback",
  "syntactic.serializeReturnTypeForSignature.checkerFallback",
  "isVisible.memo",
  "isVisible.addVisibleAlias",
  "isVisible.collectLinkedAliases",
  "probe.checkSeed",
  "probe.transformSeed",
  "probe.fallbackSweep",
  "probe.bootstrap",
]);

const NODE_REF_SENTINEL = Object.freeze([-1, -1, -1, -1]);

function exactObjectKeys(value, expected) {
  return (
    value !== null &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    stableStringify(Object.keys(value).sort()) ===
      stableStringify([...expected].sort())
  );
}

function validateSafeJsonValue(value, label, forbiddenStrings) {
  if (typeof value === "number") {
    requireCondition(Number.isSafeInteger(value), `${label} is not a safe integer`);
    return;
  }
  if (typeof value === "string") {
    for (const forbidden of forbiddenStrings) {
      requireCondition(
        !value.includes(forbidden),
        `${label} contains program-root path ${JSON.stringify(forbidden)}`,
      );
    }
    return;
  }
  if (typeof value === "boolean" || value === null) return;
  requireCondition(Array.isArray(value), `${label} is not a stable JSON value`);
  value.forEach((entry, index) =>
    validateSafeJsonValue(entry, `${label}[${index}]`, forbiddenStrings),
  );
}

function validateFileTable(fileTable, caseId, sourceCount) {
  requireCondition(Array.isArray(fileTable), `${caseId} fileTable is not an array`);
  const identities = new Set();
  for (const [index, row] of fileTable.entries()) {
    requireCondition(
      Array.isArray(row) && row.length === 2,
      `${caseId} fileTable row ${index} is malformed`,
    );
    const [className, key] = row;
    requireCondition(
      className === "src" || className === "lib",
      `${caseId} fileTable row ${index} has invalid class`,
    );
    if (className === "src") {
      requireCondition(
        Number.isSafeInteger(key) && key >= 0 && key < sourceCount,
        `${caseId} fileTable row ${index} has invalid source index`,
      );
    } else {
      requireCondition(
        typeof key === "string" &&
          key.length > 0 &&
          path.posix.basename(key) === key &&
          !key.includes("\\"),
        `${caseId} fileTable row ${index} has invalid library basename`,
      );
    }
    const identity = `${className}\u0000${key}`;
    requireCondition(
      !identities.has(identity),
      `${caseId} fileTable row ${index} is duplicated`,
    );
    identities.add(identity);
  }
}

function nodeRefCoordinate(ref) {
  return ref[0] >= 0 ? ref.slice(0, 4) : ref.slice(4, 8);
}

function validateNodeRef(ref, state, label) {
  requireCondition(
    Array.isArray(ref) &&
      ref.length === 8 &&
      ref.every(Number.isSafeInteger),
    `${label} is not a node ref`,
  );
  const own = ref.slice(0, 4);
  const original = ref.slice(4, 8);
  const ownSentinel = stableStringify(own) === stableStringify(NODE_REF_SENTINEL);
  const originalSentinel =
    stableStringify(original) === stableStringify(NODE_REF_SENTINEL);
  requireCondition(
    ownSentinel || originalSentinel,
    `${label} mixes parse-tree and original coordinates`,
  );
  requireCondition(
    ownSentinel ||
      (own[0] >= 0 &&
        own[0] < state.fileTable.length &&
        own[1] >= 0 &&
        own[2] >= 0 &&
        own[3] >= own[2]),
    `${label} has invalid parse-tree coordinates`,
  );
  requireCondition(
    originalSentinel ||
      (original[0] >= 0 &&
        original[0] < state.fileTable.length &&
        original[1] >= 0 &&
        original[2] >= 0 &&
        original[3] >= original[2]),
    `${label} has invalid original coordinates`,
  );
  for (const fileTag of [ownSentinel ? -1 : own[0], originalSentinel ? -1 : original[0]]) {
    if (fileTag < 0 || state.seenFileTags.has(fileTag)) continue;
    requireCondition(
      fileTag === state.seenFileTags.size,
      `${label} first uses fileTag ${fileTag} out of interning order`,
    );
    state.seenFileTags.add(fileTag);
  }
  state.nodeRefs.push({ ref, label });
}

function validateScalarRef(value, label) {
  requireCondition(
    Array.isArray(value) &&
      value.length === 4 &&
      typeof value[0] === "string" &&
      typeof value[1] === "string" &&
      Number.isSafeInteger(value[2]) &&
      typeof value[3] === "boolean",
    `${label} is not a scalar ref`,
  );
}

function validateSymbolRef(value, state, label) {
  requireCondition(
    Array.isArray(value) &&
      value.length === 3 &&
      typeof value[0] === "string" &&
      Number.isSafeInteger(value[1]) &&
      value[1] >= 0 &&
      Array.isArray(value[2]) &&
      value[2].length === Math.min(value[1], 8),
    `${label} is not a symbol ref`,
  );
  value[2].forEach((ref, index) =>
    validateNodeRef(ref, state, `${label}.declarations[${index}]`),
  );
}

function validateGenericCallArgs(event, state, label) {
  const baseSite = event.site_id.slice(0, event.site_id.lastIndexOf("."));
  if (event.site_id.endsWith(".entry")) {
    if (baseSite === "resolver.isSymbolAccessible") {
      requireCondition(event.args.length === 6, `${label} has wrong symbol entry arity`);
      requireCondition(
        Number.isSafeInteger(event.args[1]) && event.args[1] >= 0,
        `${label} has invalid source arity`,
      );
      validateSymbolRef(event.args[2], state, `${label}.symbol`);
      validateNodeRef(event.args[3], state, `${label}.enclosing`);
      requireCondition(
        Number.isSafeInteger(event.args[4]) &&
          event.args[4] >= 0 &&
          event.args[4] <= 0xffff_ffff &&
          typeof event.args[5] === "boolean",
        `${label} has invalid accessibility trailing arguments`,
      );
      return;
    }
    if (baseSite === "resolver.isEntityNameVisible") {
      requireCondition(event.args.length === 6, `${label} has wrong entity entry arity`);
      requireCondition(
        Number.isSafeInteger(event.args[1]) && event.args[1] >= 0,
        `${label} has invalid source arity`,
      );
      validateNodeRef(event.args[2], state, `${label}.entityName`);
      validateNodeRef(event.args[3], state, `${label}.enclosing`);
      requireCondition(
        typeof event.args[4] === "boolean" &&
          typeof event.args[5] === "boolean" &&
          event.args[5] === (event.args[1] >= 3),
        `${label} does not preserve the entity visibility default`,
      );
      return;
    }
    if (baseSite === "resolver.collectLinkedAliases") {
      requireCondition(event.args.length === 3, `${label} has wrong collect entry arity`);
      validateNodeRef(event.args[1], state, `${label}.node`);
      requireCondition(typeof event.args[2] === "boolean", `${label} lacks setVisibility`);
      return;
    }
    if (SYNTACTIC_CALL_SITES.includes(baseSite)) {
      requireCondition(event.args.length === 2, `${label} has wrong syntactic entry arity`);
      validateNodeRef(event.args[1], state, `${label}.node`);
      return;
    }
    requireCondition(event.args.length === 7, `${label} has wrong generic entry arity`);
    requireCondition(
      Number.isSafeInteger(event.args[1]) &&
        event.args[1] >= 0 &&
        typeof event.args[2] === "string",
      `${label} has invalid generic entry fields`,
    );
    validateNodeRef(event.args[3], state, `${label}.firstNode`);
    validateNodeRef(event.args[4], state, `${label}.secondNode`);
    validateScalarRef(event.args[5], `${label}.firstScalar`);
    validateScalarRef(event.args[6], `${label}.secondScalar`);
    return;
  }

  if (
    baseSite === "resolver.isSymbolAccessible" ||
    baseSite === "resolver.isEntityNameVisible"
  ) {
    requireCondition(event.args.length === 6, `${label} has wrong accessibility result arity`);
    requireCondition(
      Number.isSafeInteger(event.args[1]) &&
        event.args[1] >= 0 &&
        event.args[1] <= 3 &&
        typeof event.args[2] === "string" &&
        (event.args[3] === null || typeof event.args[3] === "string"),
      `${label} has invalid accessibility result fields`,
    );
    validateNodeRef(event.args[4], state, `${label}.errorNode`);
    requireCondition(
      event.args[5] === null || Array.isArray(event.args[5]),
      `${label} loses aliasesToMakeVisible presence`,
    );
    if (Array.isArray(event.args[5])) {
      event.args[5].forEach((ref, index) =>
        validateNodeRef(ref, state, `${label}.aliases[${index}]`),
      );
    }
    return;
  }
  if (baseSite === "resolver.getPropertiesOfContainerFunction") {
    requireCondition(
      event.args.length === 2 && Array.isArray(event.args[1]),
      `${label} has invalid property rows`,
    );
    event.args[1].forEach((row, index) => {
      requireCondition(
        Array.isArray(row) && row.length === 3 && typeof row[0] === "string",
        `${label}.properties[${index}] is malformed`,
      );
      validateSymbolRef(row[1], state, `${label}.properties[${index}].parent`);
      requireCondition(
        row[2] === null || Array.isArray(row[2]),
        `${label}.properties[${index}] has invalid value declaration`,
      );
      if (row[2] !== null) {
        validateNodeRef(row[2], state, `${label}.properties[${index}].valueDeclaration`);
      }
    });
    return;
  }
  if (baseSite === "resolver.getEnumMemberValue") {
    requireCondition(
      event.args.length === 4 &&
        typeof event.args[1] === "string" &&
        (event.args[2] === null ||
          typeof event.args[2] === "string" ||
          typeof event.args[2] === "boolean" ||
          Number.isSafeInteger(event.args[2])) &&
        typeof event.args[3] === "boolean",
      `${label} has invalid enum result`,
    );
    return;
  }
  if (baseSite === "resolver.collectLinkedAliases") {
    requireCondition(
      stableStringify(event.args) ===
        stableStringify([event.site_id, "void"]),
      `${label} has invalid void marker`,
    );
    return;
  }
  if (SYNTACTIC_CALL_SITES.includes(baseSite)) {
    requireCondition(
      event.args.length === 4 &&
        typeof event.args[1] === "boolean" &&
        typeof event.args[2] === "boolean" &&
        event.args[1] !== event.args[2],
      `${label} has invalid syntactic result`,
    );
    validateNodeRef(event.args[3], state, `${label}.resultNode`);
    return;
  }
  requireCondition(event.args.length === 7, `${label} has wrong generic result arity`);
  validateScalarRef(event.args[1], `${label}.resultScalar`);
  requireCondition(typeof event.args[2] === "boolean", `${label} lacks null marker`);
  validateNodeRef(event.args[3], state, `${label}.resultNode`);
  requireCondition(Number.isSafeInteger(event.args[4]), `${label} has invalid array length`);
  validateScalarRef(event.args[5], `${label}.valueScalar`);
  requireCondition(typeof event.args[6] === "boolean", `${label} lacks string syntax marker`);
}

function validateNonCallArgs(event, state, label) {
  if (event.site_id.startsWith("isVisible.")) {
    requireCondition(
      event.args.length === 3 && typeof event.args[2] === "boolean",
      `${label} has invalid writer payload`,
    );
    validateNodeRef(event.args[1], state, `${label}.declaration`);
    return;
  }
  if (event.site_id === "probe.checkSeed" || event.site_id === "probe.transformSeed") {
    requireCondition(
      event.args.length === 2 && Array.isArray(event.args[1]),
      `${label} has invalid seed dump`,
    );
    let previous;
    for (const [index, row] of event.args[1].entries()) {
      requireCondition(
        Array.isArray(row) && row.length === 2 && typeof row[1] === "boolean",
        `${label}.rows[${index}] is malformed`,
      );
      validateNodeRef(row[0], state, `${label}.rows[${index}].node`);
      const coordinate = nodeRefCoordinate(row[0]);
      if (previous !== undefined) {
        const ordering =
          previous[0] - coordinate[0] ||
          previous[2] - coordinate[2] ||
          previous[3] - coordinate[3] ||
          previous[1] - coordinate[1];
        requireCondition(
          ordering <= 0,
          `${label}.rows are not ordered by (fileTag,pos,end,kind)`,
        );
      }
      previous = coordinate;
    }
    return;
  }
  if (event.site_id === "probe.fallbackSweep") {
    requireCondition(
      event.args.length === 3 &&
        [
          "emitOnly-without-declarations",
          "noCheck",
          "resolver-skips-type-checking",
          "diagnostics-excluded",
        ].includes(event.args[2]),
      `${label} has invalid fallback disjunct`,
    );
    validateNodeRef(event.args[1], state, `${label}.sourceFile`);
    return;
  }
  switch (event.site_id) {
    case "nodebuilder.moduleSpecifierOverride.contextArm":
      requireCondition(
        event.args.length === 5 &&
          typeof event.args[1] === "string" &&
          typeof event.args[2] === "boolean" &&
          typeof event.args[3] === "boolean",
        `${label} has invalid context-arm payload`,
      );
      validateNodeRef(event.args[4], state, `${label}.parent`);
      return;
    case "nodebuilder.moduleSpecifierOverride.sourceArm":
    case "nodebuilder.moduleSpecifierOverride.resultArm":
      requireCondition(
        event.args.length === 3 && typeof event.args[1] === "string",
        `${label} has invalid module-specifier arm payload`,
      );
      validateNodeRef(event.args[2], state, `${label}.parent`);
      return;
    case "nodebuilder.moduleSpecifierOverride.unsafe":
      requireCondition(
        event.args.length === 3 &&
          typeof event.args[1] === "boolean" &&
          typeof event.args[2] === "boolean",
        `${label} has invalid unsafe marker`,
      );
      return;
    case "nodebuilder.withContext.decision":
      requireCondition(
        (event.args.length === 7 || event.args.length === 8) &&
          ["report-truncation", "copy-out"].includes(event.args[1]) &&
          event.args.slice(2, 6).every(Number.isSafeInteger) &&
          event.args.slice(6).every((value) => typeof value === "boolean"),
        `${label} has invalid withContext decision`,
      );
      return;
    case "nodebuilder.withContext.result":
      requireCondition(
        event.args.length === 10 &&
          ["error", "fallback-undefined", "node"].includes(event.args[1]) &&
          event.args.slice(2, 6).every(Number.isSafeInteger) &&
          event.args.slice(6, 9).every((value) => typeof value === "boolean"),
        `${label} has invalid withContext result`,
      );
      validateNodeRef(event.args[9], state, `${label}.resultNode`);
      return;
    case "tracker.reportInferenceFallback":
      requireCondition(event.args.length === 2, `${label} has invalid inference fallback`);
      validateNodeRef(event.args[1], state, `${label}.node`);
      return;
    case "tracker.trackSymbol":
      requireCondition(
        event.args.length === 4 &&
          typeof event.args[1] === "string" &&
          Number.isSafeInteger(event.args[3]),
        `${label} has invalid tracked symbol`,
      );
      validateNodeRef(event.args[2], state, `${label}.enclosing`);
      return;
    case "tracker.reportPrivateInBaseOfClassExpression":
    case "tracker.reportNonSerializableProperty":
      requireCondition(
        event.args.length === 3 && typeof event.args[1] === "string",
        `${label} has invalid tracker property payload`,
      );
      validateNodeRef(event.args[2], state, `${label}.propertyName`);
      return;
    case "tracker.reportInaccessibleUniqueSymbolError":
    case "tracker.reportCyclicStructureError":
    case "tracker.reportInaccessibleThisError":
    case "tracker.reportTruncationError":
      requireCondition(event.args.length === 1, `${label} has unexpected marker fields`);
      return;
    case "tracker.reportLikelyUnsafeImportRequiredError":
      requireCondition(
        event.args.length === 4 &&
          typeof event.args[1] === "boolean" &&
          Number.isSafeInteger(event.args[2]) &&
          typeof event.args[3] === "string",
        `${label} has invalid unsafe-import payload`,
      );
      return;
    case "tracker.reportNonlocalAugmentation":
      requireCondition(
        event.args.length === 5 &&
          typeof event.args[2] === "string" &&
          typeof event.args[3] === "string" &&
          Number.isSafeInteger(event.args[4]),
        `${label} has invalid nonlocal-augmentation payload`,
      );
      validateNodeRef(event.args[1], state, `${label}.containingFile`);
      return;
    case "declarations.visitDeclarationSubtree.changed":
    case "declarations.transformTopLevelDeclaration.changed":
      requireCondition(
        event.args.length === 5 &&
          typeof event.args[3] === "boolean" &&
          Number.isSafeInteger(event.args[4]),
        `${label} has invalid transform payload`,
      );
      validateNodeRef(event.args[1], state, `${label}.input`);
      validateNodeRef(event.args[2], state, `${label}.output`);
      return;
    case "declarations.declBlocked":
      requireCondition(
        event.args.length === 6 &&
          Number.isSafeInteger(event.args[1]) &&
          event.args.slice(2).every((value) => typeof value === "boolean"),
        `${label} has invalid declaration-block payload`,
      );
      return;
    case "syntactic.serializeTypeOfDeclaration.checkerFallback":
    case "syntactic.serializeReturnTypeForSignature.checkerFallback":
      requireCondition(
        event.args.length === 3 && typeof event.args[1] === "boolean",
        `${label} has invalid checker-fallback payload`,
      );
      validateNodeRef(event.args[2], state, `${label}.node`);
      return;
    case "probe.bootstrap":
      requireCondition(
        stableStringify(event.args) ===
          stableStringify(["probe.bootstrap", "6.0.3"]),
        `${label} has invalid bootstrap marker`,
      );
      return;
    default:
      requireCondition(false, `${label} lacks a payload validator`);
  }
}

function programRootSubstrings(control) {
  // Machine-specific roots only. Canonical virtual overlay roots
  // (control.roots, e.g. "/.src") are legitimate recorded content:
  // external-module symbols are NAMED by their virtual path, so
  // errorModuleName/escapedName must be able to carry them
  // (h2-7a-m-2.md §6.4 hygiene intent; E3 repair 2026-08-31).
  const values = new Set([
    WORKSPACE,
    fs.realpathSync(WORKSPACE),
    control.current_directory,
  ]);
  return [...values]
    .filter((value) => typeof value === "string" && value.length > 1)
    .flatMap((value) => [value, value.replace(/\\/g, "/")])
    .filter((value, index, all) => value.length > 1 && all.indexOf(value) === index);
}

function validateTraceEvents(events, fileTable, caseId, control) {
  requireCondition(Array.isArray(events), `${caseId} trace_events is not an array`);
  validateFileTable(fileTable, caseId, control.files.length);
  const forbiddenStrings = programRootSubstrings(control);
  const state = { fileTable, seenFileTags: new Set(), nodeRefs: [] };
  const callStack = [];
  let nextCallId = 0;
  for (const [index, event] of events.entries()) {
    const label = `${caseId} trace event ${index} (${String(event?.site_id ?? "unknown")})`;
    requireCondition(
      exactObjectKeys(event, ["site_id", "event_seq", "call_id", "depth", "args"]) &&
        typeof event.site_id === "string" &&
        /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(event.site_id) &&
        event.event_seq === index &&
        Number.isSafeInteger(event.call_id) &&
        Number.isSafeInteger(event.depth) &&
        event.depth >= 0 &&
        Array.isArray(event.args) &&
        event.args[0] === event.site_id,
      `${label} is malformed`,
    );
    validateSafeJsonValue(event.args, `${label}.args`, forbiddenStrings);
    const candidateSuffix = event.site_id.endsWith(".entry")
      ? ".entry"
      : event.site_id.endsWith(".result")
        ? ".result"
        : null;
    const candidateBaseSite =
      candidateSuffix === null
        ? null
        : event.site_id.slice(0, -candidateSuffix.length);
    const suffix = CALL_SITES.has(candidateBaseSite) ? candidateSuffix : null;
    const baseSite = suffix === null ? null : candidateBaseSite;
    if (suffix === ".entry") {
      requireCondition(
        CALL_SITES.has(baseSite) &&
          event.call_id === nextCallId &&
          event.depth === callStack.length + 1,
        `${label} violates call entry ordering`,
      );
      callStack.push({ call_id: event.call_id, site: baseSite });
      nextCallId += 1;
      validateGenericCallArgs(event, state, label);
    } else if (suffix === ".result") {
      const frame = callStack.at(-1);
      requireCondition(
        CALL_SITES.has(baseSite) &&
          frame?.call_id === event.call_id &&
          frame?.site === baseSite &&
          event.depth === callStack.length,
        `${label} violates call-result LIFO`,
      );
      validateGenericCallArgs(event, state, label);
      callStack.pop();
    } else {
      requireCondition(
        NON_CALL_SITES.has(event.site_id) &&
          event.call_id === -1 &&
          event.depth === callStack.length,
        `${label} has an unknown or misnested non-call site`,
      );
      validateNonCallArgs(event, state, label);
    }
  }
  requireCondition(callStack.length === 0, `${caseId} ends with unpaired probe calls`);
  requireCondition(
    state.seenFileTags.size === fileTable.length,
    `${caseId} fileTable contains an unreferenced row`,
  );
  return state.nodeRefs;
}

function parseTreeIdentityCounts(control, sourceIndex) {
  const source = control.files[sourceIndex];
  requireCondition(source !== undefined, `${control.case_id} lacks source ${sourceIndex}`);
  const sourceFile = publicTs.createSourceFile(
    source.path,
    source.text,
    control.compiler_options.target ?? publicTs.ScriptTarget.Latest,
    true,
    publicTs.getScriptKindFromFileName(source.path),
  );
  const counts = new Map();
  const seen = new Set();
  function visit(node) {
    if (seen.has(node)) return;
    seen.add(node);
    const identity = `${node.kind}:${node.pos}:${node.end}`;
    counts.set(identity, (counts.get(identity) ?? 0) + 1);
    for (const child of node.getChildren(sourceFile)) visit(child);
    for (const jsDoc of node.jsDoc ?? []) visit(jsDoc);
  }
  visit(sourceFile);
  return counts;
}

function validateNodeRefUniqueness(control, fileTable, nodeRefs, caseId) {
  const countsBySource = new Map();
  function validateCoordinate(coordinate, label) {
    const [fileTag, kind, pos, end] = coordinate;
    if (fileTag < 0 || fileTable[fileTag][0] === "lib") return;
    const sourceIndex = fileTable[fileTag][1];
    let counts = countsBySource.get(sourceIndex);
    if (counts === undefined) {
      counts = parseTreeIdentityCounts(control, sourceIndex);
      countsBySource.set(sourceIndex, counts);
    }
    const identity = `${kind}:${pos}:${end}`;
    const matches = counts.get(identity) ?? 0;
    requireCondition(
      matches === 1,
      `${caseId} ${label} node ref ${stableStringify(coordinate)} resolves ${matches} times`,
    );
  }
  for (const { ref, label } of nodeRefs) {
    validateCoordinate(ref.slice(0, 4), label);
    validateCoordinate(ref.slice(4, 8), `${label}.original`);
  }
}

function runInternalObservation(contextPath, caseId) {
  const context = JSON.parse(fs.readFileSync(contextPath, "utf8"));
  const selected = context.cases.find((entry) => entry.case_id === caseId);
  requireCondition(selected !== undefined, `unknown internal probe case ${caseId}`);
  const traceEvents = [];
  globalThis.__H2_7A_INTERNAL__ = true;
  const sourceIndex = new Map(
    selected.control.files.map((entry, index) => [entry.path, index]),
  );
  globalThis.__H2_7A_PROBE_CONFIG__ = {
    source_paths: selected.control.files.map((entry) => entry.path),
    source_aliases: selected.control.symlinks.map((entry) => {
      const index = sourceIndex.get(entry.target_path);
      requireCondition(
        index !== undefined,
        `${caseId} symlink target is absent from manifest inputs`,
      );
      return [entry.link_path, index];
    }),
  };
  globalThis.__H2_7A_TRACE__ = (siteId, eventSeq, callId, depth, args) => {
    traceEvents.push({
      site_id: siteId,
      event_seq: eventSeq,
      call_id: callId,
      depth,
      args,
    });
  };
  require(path.join(WORKSPACE, INSTRUMENTED_RELATIVE_PATH));
  const tsApi = globalThis.__H2_7A_TS__;
  requireCondition(tsApi?.version === "6.0.3", "instrumented TypeScript export unavailable");
  const publicOutputs = observeControl(tsApi, selected.control);
  const fileTable = structuredClone(globalThis.__H2_7A_PROBE_FILE_TABLE__ ?? []);
  validateTraceEvents(traceEvents, fileTable, caseId, selected.control);
  process.stdout.write(
    JSON.stringify({
      fileTable,
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

function observeCaseInFreshProcess(contextPath, expected) {
  const caseId = expected.case_id;
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
  validateTraceEvents(
    observation.trace_events,
    observation.fileTable,
    caseId,
    expected.control,
  );
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
    const first = observeCaseInFreshProcess(contextPath, expected);
    const second = observeCaseInFreshProcess(contextPath, expected);
    requireCondition(
      stableStringify({
        fileTable: first.fileTable,
        trace_events: first.trace_events,
      }) ===
        stableStringify({
          fileTable: second.fileTable,
          trace_events: second.trace_events,
        }),
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
    const nodeRefs = validateTraceEvents(
      first.trace_events,
      first.fileTable,
      expected.case_id,
      expected.control,
    );
    validateNodeRefUniqueness(
      expected.control,
      first.fileTable,
      nodeRefs,
      expected.case_id,
    );
    return {
      case_id: expected.case_id,
      fileTable: first.fileTable,
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
        cases.map(({ case_id, fileTable, trace_events }) => ({
          case_id,
          fileTable,
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
      schema: 2,
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
      artifact.schema === 2 &&
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
    "probe artifact has fields outside schema 2",
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
            ["case_id", "fileTable", "trace_events", "public_output_roll", "inert"].sort(),
          ) &&
        entry.case_id === expected.case_id &&
        entry.inert === true &&
        entry.public_output_roll === expected.public_output_roll,
      `probe artifact case ${index} differs from lane-B manifest/public output`,
    );
    const nodeRefs = validateTraceEvents(
      entry.trace_events,
      entry.fileTable,
      entry.case_id,
      expected.control,
    );
    validateNodeRefUniqueness(
      expected.control,
      entry.fileTable,
      nodeRefs,
      entry.case_id,
    );
  });
  requireCondition(
    stableStringify(artifact.summary) ===
      stableStringify(summaryForCases(artifact.cases)),
    "probe artifact summary is stale",
  );
  if (witnessContext.relative_path === WITNESSES_RELATIVE_PATH) {
    requireCondition(
      (artifact.summary.per_site_counts["probe.fallbackSweep"] ?? 0) === 0,
      "witness corpus entered the declaration fallback sweep",
    );
  }
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

const V1_MIGRATION_CASES = 94;
const V1_MIGRATION_CASE_IDS_SHA256 =
  "9d78a2f47e9cbb053afd815ada93de0f52c05d45559ac0c1646b403f6bc74735";
const SCHEMA2_ONLY_SITES = new Set([
  "resolver.collectLinkedAliases.entry",
  "resolver.collectLinkedAliases.result",
  "isVisible.memo",
  "isVisible.addVisibleAlias",
  "isVisible.collectLinkedAliases",
  "probe.checkSeed",
  "probe.transformSeed",
  "probe.fallbackSweep",
]);

function v1String(value) {
  return typeof value === "string" &&
    !value.includes("/") &&
    !value.includes("\\")
    ? value
    : "";
}

function v1NodeTuple(ref) {
  if (ref[0] >= 0) return ref.slice(1, 4);
  if (ref[4] >= 0) return ref.slice(5, 8);
  return [-1, -1, -1];
}

function v1ScalarTuple(ref) {
  return [ref[0], v1String(ref[1]), ref[2], ref[3]];
}

function v1GenericCallProjection(event, baseSite) {
  const args = event.args;
  if (event.site_id.endsWith(".entry")) {
    if (baseSite === "resolver.isSymbolAccessible") {
      const symbol = args[2];
      return [
        args[1],
        v1String(symbol[0]),
        -1,
        -1,
        -1,
        ...v1NodeTuple(args[3]),
        "object",
        "",
        0,
        false,
        "object",
        "",
        0,
        false,
      ];
    }
    if (baseSite === "resolver.isEntityNameVisible") {
      return [
        args[1],
        "",
        ...v1NodeTuple(args[2]),
        ...v1NodeTuple(args[3]),
        "object",
        "",
        0,
        false,
        "object",
        "",
        0,
        false,
      ];
    }
    if (SYNTACTIC_CALL_SITES.includes(baseSite)) {
      return v1NodeTuple(args[1]);
    }
    return [
      args[1],
      v1String(args[2]),
      ...v1NodeTuple(args[3]),
      ...v1NodeTuple(args[4]),
      ...v1ScalarTuple(args[5]),
      ...v1ScalarTuple(args[6]),
    ];
  }

  if (
    baseSite === "resolver.isSymbolAccessible" ||
    baseSite === "resolver.isEntityNameVisible"
  ) {
    return [
      "object",
      false,
      false,
      0,
      "",
      args[1],
      -1,
      -1,
      -1,
      -1,
      "undefined",
      "",
      0,
      false,
      false,
    ];
  }
  if (baseSite === "resolver.getPropertiesOfContainerFunction") {
    return [
      "object",
      false,
      false,
      0,
      "",
      -1,
      -1,
      -1,
      -1,
      args[1].length,
      "undefined",
      "",
      0,
      false,
      false,
    ];
  }
  if (baseSite === "resolver.getEnumMemberValue") {
    const type = args[1];
    const value = args[2];
    return [
      "object",
      false,
      false,
      0,
      "",
      -1,
      -1,
      -1,
      -1,
      -1,
      type,
      type === "string" ? v1String(value) : "",
      type === "number" ? value : 0,
      type === "boolean" ? value : false,
      args[3],
    ];
  }
  if (SYNTACTIC_CALL_SITES.includes(baseSite)) {
    return [args[1], args[2], ...v1NodeTuple(args[3])];
  }
  const result = args[1];
  return [
    result[0],
    args[2],
    result[3],
    result[2],
    v1String(result[1]),
    -1,
    ...v1NodeTuple(args[3]),
    args[4],
    ...v1ScalarTuple(args[5]),
    args[6],
  ];
}

function v1NonCallProjection(event) {
  const args = event.args;
  switch (event.site_id) {
    case "nodebuilder.moduleSpecifierOverride.contextArm":
      return [args[1], args[2], args[3], ...v1NodeTuple(args[4])];
    case "nodebuilder.moduleSpecifierOverride.sourceArm":
    case "nodebuilder.moduleSpecifierOverride.resultArm":
      return [args[1], ...v1NodeTuple(args[2])];
    case "nodebuilder.withContext.result":
      return [...args.slice(1, -1), ...v1NodeTuple(args.at(-1))];
    case "tracker.reportInferenceFallback":
      return v1NodeTuple(args[1]);
    case "tracker.trackSymbol":
      return [v1String(args[1]), ...v1NodeTuple(args[2]), args[3]];
    case "tracker.reportPrivateInBaseOfClassExpression":
    case "tracker.reportNonSerializableProperty":
      return [v1String(args[1]), ...v1NodeTuple(args[2])];
    case "tracker.reportLikelyUnsafeImportRequiredError":
      return [args[1], args[2], v1String(args[3])];
    case "tracker.reportNonlocalAugmentation":
      return [
        ...v1NodeTuple(args[1]),
        v1String(args[2]),
        v1String(args[3]),
        args[4],
      ];
    case "declarations.visitDeclarationSubtree.changed":
    case "declarations.transformTopLevelDeclaration.changed":
      return [
        ...v1NodeTuple(args[1]),
        ...v1NodeTuple(args[2]),
        args[3],
        args[4],
      ];
    case "syntactic.serializeTypeOfDeclaration.checkerFallback":
    case "syntactic.serializeReturnTypeForSignature.checkerFallback":
      return [args[1], ...v1NodeTuple(args[2])];
    default:
      return args.slice(1).map((value) =>
        typeof value === "string" ? v1String(value) : value,
      );
  }
}

function v1EventProjection(event) {
  const candidateSuffix = event.site_id.endsWith(".entry")
    ? ".entry"
    : event.site_id.endsWith(".result")
      ? ".result"
      : null;
  const baseSite =
    candidateSuffix === null
      ? null
      : event.site_id.slice(0, -candidateSuffix.length);
  return {
    site_id: event.site_id,
    args: CALL_SITES.has(baseSite)
      ? v1GenericCallProjection(event, baseSite)
      : v1NonCallProjection(event),
  };
}

function normalizeV1NodeTuple(args, start) {
  // Schema 1 retained a synthetic node's kind beside sentinel positions.
  // Schema 2 freezes synthetic-without-original as the all-sentinel class, so
  // both sides of the one-time projection use that canonical representation.
  if (args[start + 1] === -1 && args[start + 2] === -1) {
    args[start] = -1;
  }
}

function currentV1EventProjection(event) {
  const args = structuredClone(event.args);
  const candidateSuffix = event.site_id.endsWith(".entry")
    ? ".entry"
    : event.site_id.endsWith(".result")
      ? ".result"
      : null;
  const baseSite =
    candidateSuffix === null
      ? null
      : event.site_id.slice(0, -candidateSuffix.length);
  if (CALL_SITES.has(baseSite)) {
    if (SYNTACTIC_CALL_SITES.includes(baseSite)) {
      normalizeV1NodeTuple(args, candidateSuffix === ".entry" ? 0 : 2);
    } else if (candidateSuffix === ".entry") {
      normalizeV1NodeTuple(args, 2);
      normalizeV1NodeTuple(args, 5);
    } else {
      normalizeV1NodeTuple(args, 6);
    }
  } else {
    switch (event.site_id) {
      case "nodebuilder.moduleSpecifierOverride.contextArm":
        normalizeV1NodeTuple(args, 3);
        break;
      case "nodebuilder.moduleSpecifierOverride.sourceArm":
      case "nodebuilder.moduleSpecifierOverride.resultArm":
        normalizeV1NodeTuple(args, 1);
        break;
      case "nodebuilder.withContext.result":
        normalizeV1NodeTuple(args, 8);
        break;
      case "tracker.reportInferenceFallback":
        normalizeV1NodeTuple(args, 0);
        break;
      case "tracker.trackSymbol":
      case "tracker.reportPrivateInBaseOfClassExpression":
      case "tracker.reportNonSerializableProperty":
        normalizeV1NodeTuple(args, 1);
        break;
      case "tracker.reportNonlocalAugmentation":
        normalizeV1NodeTuple(args, 0);
        break;
      case "declarations.visitDeclarationSubtree.changed":
      case "declarations.transformTopLevelDeclaration.changed":
        normalizeV1NodeTuple(args, 0);
        normalizeV1NodeTuple(args, 3);
        break;
      case "syntactic.serializeTypeOfDeclaration.checkerFallback":
      case "syntactic.serializeReturnTypeForSignature.checkerFallback":
        normalizeV1NodeTuple(args, 1);
        break;
    }
  }
  return { site_id: event.site_id, args };
}

function v1FieldProjection(artifact, caseCount = V1_MIGRATION_CASES) {
  requireCondition(
    (artifact?.schema === 1 || artifact?.schema === 2) &&
      Array.isArray(artifact.cases) &&
      artifact.cases.length >= caseCount,
    `probe artifact cannot supply the ${caseCount}-case v1 migration denominator`,
  );
  if (caseCount === V1_MIGRATION_CASES) {
    const caseIds = artifact.cases
      .slice(0, V1_MIGRATION_CASES)
      .map((entry) => entry.case_id);
    requireCondition(
      sha256(Buffer.from(stableStringify(caseIds), "utf8")) ===
        V1_MIGRATION_CASE_IDS_SHA256,
      "probe artifact changed or reordered the frozen 94-case v1 denominator",
    );
  }
  return artifact.cases.slice(0, caseCount).map((entry) => ({
    case_id: entry.case_id,
    trace_events: entry.trace_events
      .filter((event) => !SCHEMA2_ONLY_SITES.has(event.site_id))
      .map((event) =>
        artifact.schema === 1
          ? currentV1EventProjection(event)
          : v1EventProjection(event),
      ),
  }));
}

function emitV1Projection(expectedSchema) {
  const artifact = readJson(TARGET_RELATIVE_PATH);
  requireCondition(
    artifact?.schema === expectedSchema,
    `${TARGET_RELATIVE_PATH} is schema ${artifact?.schema ?? "unknown"}; expected ${expectedSchema}`,
  );
  const cases = v1FieldProjection(artifact);
  process.stdout.write(
    render({
      schema: 1,
      kind: "h2-7a-probe-traces-v1-field-projection",
      cases,
      projection_sha256: sha256(Buffer.from(stableStringify(cases), "utf8")),
    }),
  );
}

const mode = process.argv[2];
if (mode === INTERNAL_OBSERVE_MODE) {
  requireCondition(
    process.argv.length === 5,
    "internal observation requires context path and case id",
  );
  runInternalObservation(process.argv[3], process.argv[4]);
} else if (mode === INTERNAL_V1_PROJECTION_MODE) {
  requireCondition(
    process.argv.length === 5 && Number.isSafeInteger(Number(process.argv[4])),
    "internal v1 projection requires an artifact path and case count",
  );
  const artifact = JSON.parse(fs.readFileSync(process.argv[3], "utf8"));
  const cases = v1FieldProjection(artifact, Number(process.argv[4]));
  process.stdout.write(
    JSON.stringify({
      cases,
      projection_sha256: sha256(Buffer.from(stableStringify(cases), "utf8")),
    }),
  );
} else if (mode === "--selftest") {
  requireCondition(process.argv.length === 3, "--selftest takes no arguments");
  runSelftest();
} else if (mode === "--write") {
  requireCondition(process.argv.length === 3, "--write takes no arguments");
  runWrite();
} else if (mode === "--check") {
  requireCondition(process.argv.length === 3, "--check takes no arguments");
  runCheck();
} else if (mode === "--v1-projection") {
  requireCondition(process.argv.length === 3, "--v1-projection takes no arguments");
  emitV1Projection(2);
} else if (mode === "--v1-projection-of-current") {
  requireCondition(
    process.argv.length === 3,
    "--v1-projection-of-current takes no arguments",
  );
  emitV1Projection(1);
} else {
  fail("usage: node crates/oracle/h2-7a-probe-traces.mjs --write|--check|--selftest|--v1-projection|--v1-projection-of-current");
}
