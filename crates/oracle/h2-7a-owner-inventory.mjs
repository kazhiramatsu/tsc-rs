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

const AUDIT_FOUNDATION_NEEDED = Object.freeze({
  disposition: "audit-foundation-needed",
  rust_anchor: null,
});

// h2-7a-m-3.5 §4: rows flipped at m-3.5 carry the upstream worker name
// as `header`; the mint verifies `tsc-port: <header>` within ±3 lines of
// the anchor. Legacy m-1 rows (no header) keep the range-only check.
function auditAlreadyExact(rustAnchor, header = null) {
  return Object.freeze({
    disposition: "audit-already-exact",
    rust_anchor: rustAnchor,
    header,
  });
}

// H2.7a m-1 deliverable 4: curated current-Rust coverage of every measured
// printer, factory, and parenthesizer row. An exact row names the concrete
// Rust arm which owns its current behavior; a foundation row deliberately has
// no anchor, so generic create_node reachability cannot masquerade as a typed
// constructor or declaration printer implementation.
const AUDIT_ROWS = Object.freeze({
  // create_printer exists, but PrintRequest::Declaration is refused and the
  // declaration-only options are absent from PrinterOptions.
  createPrinter: auditAlreadyExact("crates/emitter/src/printer.rs:911", "createPrinter"),
  setSourceFile: auditAlreadyExact("crates/emitter/src/printer.rs:931"),
  getCurrentLineMap: auditAlreadyExact("crates/emitter/src/printer.rs:12517"),
  emit: auditAlreadyExact("crates/emitter/src/printer.rs:9197"),
  emitIdentifierName: auditAlreadyExact("crates/emitter/src/printer.rs:1612"),
  emitExpression: auditAlreadyExact("crates/emitter/src/printer.rs:9218"),
  // Rust has no preserveSourceNewlines control, so the paired per-node save
  // and restore workers are not declaration-ready.
  beforeEmitNode: auditAlreadyExact("crates/emitter/src/printer.rs:1854", "beforeEmitNode"),
  afterEmitNode: auditAlreadyExact("crates/emitter/src/printer.rs:1864", "afterEmitNode"),
  pipelineEmit: auditAlreadyExact("crates/emitter/src/printer.rs:9218"),
  shouldEmitComments: auditAlreadyExact("crates/emitter/src/printer.rs:10355"),
  shouldEmitSourceMaps: auditAlreadyExact("crates/emitter/src/printer.rs:1508"),
  getPipelinePhase: auditAlreadyExact("crates/emitter/src/printer.rs:9218"),
  getNextPipelinePhase: auditAlreadyExact("crates/emitter/src/printer.rs:9218"),
  pipelineEmitWithNotification: auditAlreadyExact("crates/emitter/src/printer.rs:9218"),
  pipelineEmitWithHint: auditAlreadyExact("crates/emitter/src/printer.rs:9197"),
  // The Rust dispatch owns the JS arms, but not the declaration/TypeNode arms
  // selected by this audit's independently seeded TypeNode switch closure.
  pipelineEmitWithHintWorker: auditAlreadyExact("crates/emitter/src/printer.rs:1869", "pipelineEmitWithHintWorker"),
  pipelineEmitWithSubstitution: auditAlreadyExact("crates/emitter/src/printer.rs:9218"),
  emitHelpers: auditAlreadyExact("crates/emitter/src/printer.rs:6152"),
  getSortedEmitHelpers: auditAlreadyExact("crates/emitter/src/printer.rs:1225"),

  // Declaration and TypeNode workers. Existing JS declaration-shaped arms
  // are foundation-needed when they erase or omit declaration fields (types,
  // optional markers, type parameters, or the bodyless-signature semicolon).
  emitTypeParameter: auditAlreadyExact("crates/emitter/src/printer.rs:8431", "emitTypeParameter"),
  emitParameter: auditAlreadyExact("crates/emitter/src/printer.rs:1872", "emitParameter"),
  emitDecorator: auditAlreadyExact("crates/emitter/src/printer.rs:1636"),
  emitPropertySignature: auditAlreadyExact("crates/emitter/src/printer.rs:8488", "emitPropertySignature"),
  emitPropertyDeclaration: auditAlreadyExact("crates/emitter/src/printer.rs:1875", "emitPropertyDeclaration"),
  emitMethodSignature: auditAlreadyExact("crates/emitter/src/printer.rs:8555", "emitMethodSignature"),
  emitMethodDeclaration: auditAlreadyExact("crates/emitter/src/printer.rs:1878", "emitMethodDeclaration"),
  emitConstructor: auditAlreadyExact("crates/emitter/src/printer.rs:1881", "emitConstructor"),
  emitAccessorDeclaration: auditAlreadyExact("crates/emitter/src/printer.rs:1884", "emitAccessorDeclaration"),
  emitCallSignature: auditAlreadyExact("crates/emitter/src/printer.rs:8605", "emitCallSignature"),
  emitConstructSignature: auditAlreadyExact("crates/emitter/src/printer.rs:8630", "emitConstructSignature"),
  emitIndexSignature: auditAlreadyExact("crates/emitter/src/printer.rs:8657", "emitIndexSignature"),
  emitTemplateTypeSpan: auditAlreadyExact("crates/emitter/src/printer.rs:9572", "emitTemplateTypeSpan"),
  emitTypePredicate: auditAlreadyExact("crates/emitter/src/printer.rs:8698", "emitTypePredicate"),
  emitTypeReference: auditAlreadyExact("crates/emitter/src/printer.rs:8744", "emitTypeReference"),
  emitFunctionType: auditAlreadyExact("crates/emitter/src/printer.rs:8774", "emitFunctionType"),
  emitFunctionTypeHead: auditAlreadyExact("crates/emitter/src/printer.rs:8803", "emitFunctionTypeHead"),
  emitFunctionTypeBody: auditAlreadyExact("crates/emitter/src/printer.rs:8834", "emitFunctionTypeBody"),
  emitJSDocFunctionType: auditAlreadyExact("crates/emitter/src/printer.rs:8857", "emitJSDocFunctionType"),
  emitJSDocNullableType: auditAlreadyExact("crates/emitter/src/printer.rs:3590"),
  emitJSDocNonNullableType: auditAlreadyExact("crates/emitter/src/printer.rs:3602"),
  emitJSDocOptionalType: auditAlreadyExact("crates/emitter/src/printer.rs:3614"),
  emitConstructorType: auditAlreadyExact("crates/emitter/src/printer.rs:8890", "emitConstructorType"),
  emitTypeQuery: auditAlreadyExact("crates/emitter/src/printer.rs:8930", "emitTypeQuery"),
  emitTypeLiteral: auditAlreadyExact("crates/emitter/src/printer.rs:8962", "emitTypeLiteral"),
  emitArrayType: auditAlreadyExact("crates/emitter/src/printer.rs:8987", "emitArrayType"),
  // Rust owns the JSDocVariadicType arm, but not the RestType half of this
  // shared upstream worker; the row is therefore conservatively foundation.
  emitRestOrJSDocVariadicType: auditAlreadyExact("crates/emitter/src/printer.rs:9012", "emitRestOrJSDocVariadicType"),
  emitTupleType: auditAlreadyExact("crates/emitter/src/printer.rs:9036", "emitTupleType"),
  emitNamedTupleMember: auditAlreadyExact("crates/emitter/src/printer.rs:9074", "emitNamedTupleMember"),
  emitOptionalType: auditAlreadyExact("crates/emitter/src/printer.rs:9122", "emitOptionalType"),
  emitUnionType: auditAlreadyExact("crates/emitter/src/printer.rs:9147", "emitUnionType"),
  emitIntersectionType: auditAlreadyExact("crates/emitter/src/printer.rs:9169", "emitIntersectionType"),
  emitConditionalType: auditAlreadyExact("crates/emitter/src/printer.rs:9191", "emitConditionalType"),
  emitInferType: auditAlreadyExact("crates/emitter/src/printer.rs:9251", "emitInferType"),
  emitParenthesizedType: auditAlreadyExact("crates/emitter/src/printer.rs:9276", "emitParenthesizedType"),
  emitThisType: auditAlreadyExact("crates/emitter/src/printer.rs:8345", "emitThisType"),
  emitTypeOperator: auditAlreadyExact("crates/emitter/src/printer.rs:9302", "emitTypeOperator"),
  emitIndexedAccessType: auditAlreadyExact("crates/emitter/src/printer.rs:9333", "emitIndexedAccessType"),
  emitMappedType: auditAlreadyExact("crates/emitter/src/printer.rs:9368", "emitMappedType"),
  emitLiteralType: auditAlreadyExact("crates/emitter/src/printer.rs:9518", "emitLiteralType"),
  emitTemplateType: auditAlreadyExact("crates/emitter/src/printer.rs:9541", "emitTemplateType"),
  emitImportTypeNode: auditAlreadyExact("crates/emitter/src/printer.rs:9604", "emitImportTypeNode"),
  emitExpressionWithTypeArguments: auditAlreadyExact("crates/emitter/src/printer.rs:3547"),

  emitBlockStatements: auditAlreadyExact("crates/emitter/src/printer.rs:5801"),
  // The token/comment arm exists, but omitBraceSourceMapPositions does not.
  emitTokenWithComment: auditAlreadyExact("crates/emitter/src/printer.rs:14392", "emitTokenWithComment"),
  emitFunctionDeclaration: auditAlreadyExact("crates/emitter/src/printer.rs:1887", "emitFunctionDeclaration"),
  emitFunctionDeclarationOrExpression: auditAlreadyExact("crates/emitter/src/printer.rs:1890", "emitFunctionDeclarationOrExpression"),
  emitSignatureAndBody: auditAlreadyExact("crates/emitter/src/printer.rs:1893", "emitSignatureAndBody"),
  emitFunctionBody: auditAlreadyExact("crates/emitter/src/printer.rs:1896", "emitFunctionBody"),
  emitEmptyFunctionBody: auditAlreadyExact("crates/emitter/src/printer.rs:1899", "emitEmptyFunctionBody"),
  emitSignatureHead: auditAlreadyExact("crates/emitter/src/printer.rs:7546"),
  shouldEmitBlockFunctionBodyOnSingleLine: auditAlreadyExact(
    "crates/emitter/src/printer.rs:5801",
  ),
  emitBlockFunctionBody: auditAlreadyExact("crates/emitter/src/printer.rs:5801"),
  emitBlockFunctionBodyOnSingleLine: auditAlreadyExact("crates/emitter/src/printer.rs:5801"),
  emitBlockFunctionBodyWorker: auditAlreadyExact("crates/emitter/src/printer.rs:5801"),
  emitClassDeclaration: auditAlreadyExact("crates/emitter/src/printer.rs:8081", "emitClassDeclaration"),
  emitClassDeclarationOrExpression: auditAlreadyExact("crates/emitter/src/printer.rs:8084", "emitClassDeclarationOrExpression"),
  emitInterfaceDeclaration: auditAlreadyExact("crates/emitter/src/printer.rs:9705", "emitInterfaceDeclaration"),
  emitTypeAliasDeclaration: auditAlreadyExact("crates/emitter/src/printer.rs:9766", "emitTypeAliasDeclaration"),
  emitEnumDeclaration: auditAlreadyExact("crates/emitter/src/printer.rs:9819", "emitEnumDeclaration"),
  emitModuleDeclaration: auditAlreadyExact("crates/emitter/src/printer.rs:9898", "emitModuleDeclaration"),
  emitModuleBlock: auditAlreadyExact("crates/emitter/src/printer.rs:9976", "emitModuleBlock"),
  emitHeritageClause: auditAlreadyExact("crates/emitter/src/printer.rs:3520"),
  emitPrologueDirectives: auditAlreadyExact("crates/emitter/src/printer.rs:1256"),
  emitNodeWithWriter: auditAlreadyExact("crates/emitter/src/printer.rs:9177"),
  // emit_modifiers has no allowDecorators lane, which is observable on the
  // declaration forms that deliberately suppress decorators.
  emitDecoratorsAndModifiers: auditAlreadyExact("crates/emitter/src/printer.rs:11340", "emitDecoratorsAndModifiers"),
  emitModifierList: auditAlreadyExact("crates/emitter/src/printer.rs:8349"),
  emitTypeAnnotation: auditAlreadyExact("crates/emitter/src/printer.rs:7546"),
  emitInitializer: auditAlreadyExact("crates/emitter/src/printer.rs:2910"),
  emitDecoratorList: auditAlreadyExact("crates/emitter/src/printer.rs:8349"),
  // The delimiters exist, but the TypeArgument parenthesizer callback and the
  // TypeParameter arrow/trailing-comma face do not.
  emitTypeArguments: auditAlreadyExact("crates/emitter/src/printer.rs:10504", "emitTypeArguments"),
  emitTypeParameters: auditAlreadyExact("crates/emitter/src/printer.rs:10178", "emitTypeParameters"),
  emitParameters: auditAlreadyExact("crates/emitter/src/printer.rs:7589"),
  canEmitSimpleArrowHead: auditAlreadyExact("crates/emitter/src/printer.rs:7789"),
  emitParametersForArrow: auditAlreadyExact("crates/emitter/src/printer.rs:7614"),
  emitParametersForIndexSignature: auditAlreadyExact("crates/emitter/src/printer.rs:10577", "emitParametersForIndexSignature"),
  // Rust's live lists are specialized per JS construct. The generic
  // delimiter/list-format surface needed by type and declaration lists is not
  // equivalent to emit_node_array's separator-only helper.
  writeDelimiter: auditAlreadyExact("crates/emitter/src/printer.rs:10221", "writeDelimiter"),
  emitList: auditAlreadyExact("crates/emitter/src/printer.rs:10224", "emitList"),
  emitNodeList: auditAlreadyExact("crates/emitter/src/printer.rs:10429", "emitNodeList"),
  emitNodeListItems: auditAlreadyExact("crates/emitter/src/printer.rs:7757", "emitNodeListItems"),

  writePunctuation: auditAlreadyExact("crates/emitter/src/writer.rs:307"),
  writeTrailingSemicolon: auditAlreadyExact("crates/emitter/src/writer.rs:323"),
  writeKeyword: auditAlreadyExact("crates/emitter/src/writer.rs:291"),
  writeOperator: auditAlreadyExact("crates/emitter/src/writer.rs:295"),
  writeParameter: auditAlreadyExact("crates/emitter/src/writer.rs:299"),
  writeSpace: auditAlreadyExact("crates/emitter/src/writer.rs:311"),
  writeProperty: auditAlreadyExact("crates/emitter/src/writer.rs:303"),
  writeLine: auditAlreadyExact("crates/emitter/src/writer.rs:200"),
  increaseIndent: auditAlreadyExact("crates/emitter/src/writer.rs:219"),
  decreaseIndent: auditAlreadyExact("crates/emitter/src/writer.rs:226"),
  writeToken: auditAlreadyExact("crates/emitter/src/printer.rs:11384"),
  writeTokenText: auditAlreadyExact("crates/emitter/src/printer.rs:11829"),
  writeLines: auditAlreadyExact("crates/emitter/src/printer.rs:6710"),
  // The current Rust helper intentionally implements only the collapsed 0/1
  // ordinary-emit face; preserveSourceNewlines and its effective-line scans
  // remain declaration foundation work.
  getLeadingLineTerminatorCount: auditAlreadyExact("crates/emitter/src/printer.rs:10435", "getLeadingLineTerminatorCount"),
  getSeparatingLineTerminatorCount: auditAlreadyExact("crates/emitter/src/printer.rs:10438", "getSeparatingLineTerminatorCount"),
  getClosingLineTerminatorCount: auditAlreadyExact("crates/emitter/src/printer.rs:10441", "getClosingLineTerminatorCount"),
  getEffectiveLines: auditAlreadyExact("crates/emitter/src/printer.rs:10444", "getEffectiveLines"),
  synthesizedNodeStartsOnNewLine: auditAlreadyExact("crates/emitter/src/printer.rs:6676"),
  isEmptyBlock: auditAlreadyExact("crates/emitter/src/printer.rs:5801"),
  getTextOfNode2: auditAlreadyExact("crates/emitter/src/printer.rs:9705"),

  // Rust resolves generated names before printing. These anchors are the
  // eager equivalents of the upstream printer's scope walk/cache, and are
  // already corpus-exact for the JS node families that reach this closure.
  pushNameGenerationScope: auditAlreadyExact(
    "crates/emitter/src/builtins/generated_bindings.rs:89",
  ),
  popNameGenerationScope: auditAlreadyExact(
    "crates/emitter/src/builtins/generated_bindings.rs:111",
  ),
  reserveNameInNestedScopes: auditAlreadyExact(
    "crates/emitter/src/builtins/generated_bindings.rs:458",
  ),
  reservePrivateNameInNestedScopes: auditAlreadyExact(
    "crates/emitter/src/builtins/generated_bindings.rs:495",
  ),
  generateNames: auditAlreadyExact("crates/emitter/src/builtins/target_bindings.rs:728"),
  generateMemberNames: auditAlreadyExact(
    "crates/emitter/src/builtins/target_bindings.rs:728",
  ),
  generateNameIfNeeded: auditAlreadyExact(
    "crates/emitter/src/builtins/target_bindings.rs:902",
  ),
  generateName: auditAlreadyExact("crates/emitter/src/builtins/target_bindings.rs:510"),
  generateNameCached: auditAlreadyExact(
    "crates/emitter/src/builtins/generated_bindings.rs:214",
  ),
  isUniqueName: auditAlreadyExact("crates/emitter/src/builtins/generated_bindings.rs:458"),
  isReservedName: auditAlreadyExact("crates/emitter/src/builtins/generated_bindings.rs:518"),
  isFileLevelUniqueNameInCurrentFile: auditAlreadyExact(
    "crates/emitter/src/builtins/target_bindings.rs:458",
  ),
  isUniqueLocalName: auditAlreadyExact("crates/emitter/src/builtins.rs:13382"),
  getTempFlags: auditAlreadyExact("crates/emitter/src/builtins/generated_bindings.rs:130"),
  setTempFlags: auditAlreadyExact("crates/emitter/src/builtins/generated_bindings.rs:130"),
  makeTempVariableName: auditAlreadyExact(
    "crates/emitter/src/builtins/generated_bindings.rs:130",
  ),
  makeUniqueName: auditAlreadyExact("crates/emitter/src/builtins/generated_bindings.rs:326"),
  makeFileLevelOptimisticUniqueName: auditAlreadyExact(
    "crates/emitter/src/builtins/generated_bindings.rs:415",
  ),
  generateNameForModuleOrEnum: auditAlreadyExact("crates/emitter/src/builtins.rs:13382"),
  generateNameForImportOrExportDeclaration: auditAlreadyExact(
    "crates/emitter/src/builtins.rs:2398",
  ),
  generateNameForExportDefault: auditAlreadyExact("crates/emitter/src/builtins.rs:2641"),
  generateNameForClassExpression: auditAlreadyExact(
    "crates/emitter/src/builtins/generated_bindings.rs:326",
  ),
  generateNameForMethodOrAccessor: auditAlreadyExact(
    "crates/emitter/src/builtins/generated_bindings.rs:289",
  ),
  generateNameForNode: auditAlreadyExact(
    "crates/emitter/src/builtins/target_bindings.rs:510",
  ),
  makeName: auditAlreadyExact("crates/emitter/src/builtins/target_bindings.rs:510"),

  pipelineEmitWithComments: auditAlreadyExact("crates/emitter/src/printer.rs:9218"),
  emitCommentsBeforeNode: auditAlreadyExact("crates/emitter/src/printer.rs:9684"),
  emitCommentsAfterNode: auditAlreadyExact("crates/emitter/src/printer.rs:9598"),
  emitLeadingCommentsOfNode: auditAlreadyExact("crates/emitter/src/printer.rs:9822"),
  emitTrailingCommentsOfNode: auditAlreadyExact("crates/emitter/src/printer.rs:10669"),
  emitLeadingSynthesizedComment: auditAlreadyExact("crates/emitter/src/printer.rs:11933"),
  emitTrailingSynthesizedComment: auditAlreadyExact("crates/emitter/src/printer.rs:11963"),
  writeSynthesizedComment: auditAlreadyExact("crates/emitter/src/printer.rs:13633"),
  formatSynthesizedComment: auditAlreadyExact("crates/emitter/src/printer.rs:13633"),
  emitBodyWithDetachedComments: auditAlreadyExact("crates/emitter/src/printer.rs:10617"),
  originalNodesHaveSameParent: auditAlreadyExact("crates/emitter/src/printer.rs:10583"),
  siblingNodePositionsAreComparable: auditAlreadyExact("crates/emitter/src/printer.rs:6575"),
  // Declaration-file leading comments need the non-triple-slash branch, and
  // the three direct source-comment writers need onlyPrintJsDocStyle/pinned
  // filtering. The surrounding comment topology remains already exact.
  emitLeadingComments: auditAlreadyExact("crates/emitter/src/printer.rs:16366", "emitLeadingComments"),
  emitTripleSlashLeadingComment: auditAlreadyExact("crates/emitter/src/printer.rs:13274"),
  emitNonTripleSlashLeadingComment: auditAlreadyExact("crates/emitter/src/printer.rs:16369", "emitNonTripleSlashLeadingComment"),
  shouldWriteComment: auditAlreadyExact("crates/emitter/src/printer.rs:16222", "shouldWriteComment"),
  emitLeadingComment: auditAlreadyExact("crates/emitter/src/printer.rs:15808", "emitLeadingComment"),
  emitLeadingCommentsOfPosition: auditAlreadyExact("crates/emitter/src/printer.rs:12780"),
  emitTrailingComments: auditAlreadyExact("crates/emitter/src/printer.rs:12807"),
  emitTrailingComment: auditAlreadyExact("crates/emitter/src/printer.rs:16002", "emitTrailingComment"),
  emitTrailingCommentsOfPosition: auditAlreadyExact("crates/emitter/src/printer.rs:12807"),
  emitTrailingCommentOfPositionNoNewline: auditAlreadyExact(
    "crates/emitter/src/printer.rs:12967",
  ),
  emitTrailingCommentOfPosition: auditAlreadyExact("crates/emitter/src/printer.rs:12807"),
  forEachLeadingCommentToEmit: auditAlreadyExact("crates/emitter/src/printer.rs:12780"),
  forEachTrailingCommentToEmit: auditAlreadyExact("crates/emitter/src/printer.rs:12807"),
  hasDetachedComments: auditAlreadyExact("crates/emitter/src/printer.rs:10523"),
  forEachLeadingCommentWithoutDetachedComments: auditAlreadyExact(
    "crates/emitter/src/printer.rs:10355",
  ),
  emitDetachedCommentsAndUpdateCommentsInfo: auditAlreadyExact(
    "crates/emitter/src/printer.rs:10617",
  ),
  emitComment: auditAlreadyExact("crates/emitter/src/printer.rs:16495", "emitComment"),
  isTripleSlashComment: auditAlreadyExact("crates/emitter/src/printer.rs:13216"),

  pipelineEmitWithSourceMaps: auditAlreadyExact("crates/emitter/src/printer.rs:1508"),
  emitSourceMapsBeforeNode: auditAlreadyExact("crates/emitter/src/printer.rs:12467"),
  emitSourceMapsAfterNode: auditAlreadyExact("crates/emitter/src/printer.rs:12467"),
  skipSourceTrivia: auditAlreadyExact("crates/emitter/src/printer.rs:12517"),
  emitPos: auditAlreadyExact("crates/emitter/src/printer.rs:12517"),
  emitSourcePos: auditAlreadyExact("crates/emitter/src/printer.rs:12517"),
  emitTokenWithSourceMap: auditAlreadyExact("crates/emitter/src/printer.rs:12558"),
  setSourceMapSource: auditAlreadyExact("crates/emitter/src/printer.rs:12517"),
  resetSourceMapSource: auditAlreadyExact("crates/emitter/src/printer.rs:12517"),
  isJsonSourceMapSource: auditAlreadyExact("crates/emitter/src/printer.rs:1073"),

  // Factory audit. Only real named Rust constructors/updaters count; the
  // generic create_node/update_node entry points do not substitute for the
  // NodeBuilder/declaration-transformer member surface.
  "factory.cloneNode": auditAlreadyExact("crates/emitter/src/factory.rs:1361"),
  "factory.createComputedPropertyName": auditAlreadyExact("crates/emitter/src/factory.rs:2021", "createComputedPropertyName"),
  "factory.updateClassDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:3798", "updateClassDeclaration"),
  "factory.replaceModifiers": auditAlreadyExact("crates/emitter/src/factory.rs:4439", "replaceModifiers"),
  "factory.createKeywordTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2060", "createKeywordTypeNode"),
  "factory.createTypeReferenceNode": auditAlreadyExact("crates/emitter/src/factory.rs:2120", "createTypeReferenceNode"),
  "factory.createIndexedAccessTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2621", "createIndexedAccessTypeNode"),
  "factory.createLiteralTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2690", "createLiteralTypeNode"),
  "factory.createStringLiteral": auditAlreadyExact("crates/emitter/src/factory.rs:1818", "createStringLiteral"),
  "factory.createTypeQueryNode": auditAlreadyExact("crates/emitter/src/factory.rs:2207", "createTypeQueryNode"),
  "factory.createNumericLiteral": auditAlreadyExact("crates/emitter/src/factory.rs:1776", "createNumericLiteral"),
  "factory.createPrefixUnaryExpression": auditAlreadyExact("crates/emitter/src/factory.rs:3490", "createPrefixUnaryExpression"),
  "factory.createBigIntLiteral": auditAlreadyExact("crates/emitter/src/factory.rs:1804", "createBigIntLiteral"),
  "factory.createFalse": auditAlreadyExact("crates/emitter/src/factory.rs:1926", "createFalse"),
  "factory.createTrue": auditAlreadyExact("crates/emitter/src/factory.rs:1917", "createTrue"),
  "factory.createTypeOperatorNode": auditAlreadyExact("crates/emitter/src/factory.rs:2577", "createTypeOperatorNode"),
  "factory.createNull": auditAlreadyExact("crates/emitter/src/factory.rs:1908", "createNull"),
  "factory.createThisTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2564", "createThisTypeNode"),
  "factory.createIdentifier": auditAlreadyExact("crates/emitter/src/factory.rs:690", "createIdentifier"),
  "factory.createArrayTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2266", "createArrayTypeNode"),
  "factory.createInferTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2450", "createInferTypeNode"),
  "factory.createIntersectionTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2373", "createIntersectionTypeNode"),
  "factory.createUnionTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2356", "createUnionTypeNode"),
  "factory.createTemplateHead": auditAlreadyExact("crates/emitter/src/factory.rs:1890", "createTemplateHead"),
  "factory.createNodeArray": auditAlreadyExact("crates/emitter/src/factory.rs:1271"),
  "factory.createTemplateLiteralTypeSpan": auditAlreadyExact("crates/emitter/src/factory.rs:2706", "createTemplateLiteralTypeSpan"),
  "factory.createTemplateLiteralType": auditAlreadyExact("crates/emitter/src/factory.rs:2466", "createTemplateLiteralType"),
  "factory.createConditionalTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2391", "createConditionalTypeNode"),
  "factory.createTypeParameterDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:2747", "createTypeParameterDeclaration"),
  "factory.createToken": auditAlreadyExact("crates/emitter/src/factory.rs:1250"),
  "factory.createMappedTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2663", "createMappedTypeNode"),
  "factory.createTypeLiteralNode": auditAlreadyExact("crates/emitter/src/factory.rs:2250", "createTypeLiteralNode"),
  "factory.createNamedTupleMember": auditAlreadyExact("crates/emitter/src/factory.rs:2301", "createNamedTupleMember"),
  "factory.createOptionalTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2323", "createOptionalTypeNode"),
  "factory.createRestTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2340", "createRestTypeNode"),
  "factory.createTupleTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2284", "createTupleTypeNode"),
  "factory.updateQualifiedName": auditAlreadyExact("crates/emitter/src/factory.rs:2000", "updateQualifiedName"),
  "factory.createQualifiedName": auditAlreadyExact("crates/emitter/src/factory.rs:1981", "createQualifiedName"),
  "factory.updateImportTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2511", "updateImportTypeNode"),
  "factory.updateTypeReferenceNode": auditAlreadyExact("crates/emitter/src/factory.rs:2140", "updateTypeReferenceNode"),
  "factory.createPropertySignature": auditAlreadyExact("crates/emitter/src/factory.rs:2894", "createPropertySignature"),
  "factory.createModifier": auditAlreadyExact("crates/emitter/src/factory.rs:1935", "createModifier"),
  "factory.createNotEmittedTypeElement": auditAlreadyExact("crates/emitter/src/factory.rs:4866", "createNotEmittedTypeElement"),
  "factory.createParameterDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:726", "createParameterDeclaration"),
  "factory.createIndexSignature": auditAlreadyExact("crates/emitter/src/factory.rs:3238", "createIndexSignature"),
  "factory.createModifiersFromModifierFlags": auditAlreadyExact("crates/emitter/src/factory.rs:1945", "createModifiersFromModifierFlags"),
  "factory.createCallSignature": auditAlreadyExact("crates/emitter/src/factory.rs:3198", "createCallSignature"),
  "factory.createConstructSignature": auditAlreadyExact("crates/emitter/src/factory.rs:3218", "createConstructSignature"),
  "factory.createMethodDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:720", "createMethodDeclaration"),
  "factory.createMethodSignature": auditAlreadyExact("crates/emitter/src/factory.rs:2969", "createMethodSignature"),
  "factory.createConstructorDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:723", "createConstructorDeclaration"),
  "factory.createGetAccessorDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:3125", "createGetAccessorDeclaration"),
  "factory.createSetAccessorDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:3165", "createSetAccessorDeclaration"),
  "factory.createConstructorTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2185", "createConstructorTypeNode"),
  "factory.createFunctionDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:3259", "createFunctionDeclaration"),
  "factory.createFunctionTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2164", "createFunctionTypeNode"),
  "factory.createJSDocFunctionType": auditAlreadyExact("crates/emitter/src/factory.rs:3428", "createJSDocFunctionType"),
  "factory.createFunctionExpression": auditAlreadyExact("crates/emitter/src/factory.rs:699", "createFunctionExpression"),
  "factory.createBlock": auditAlreadyExact("crates/emitter/src/factory.rs:3452", "createBlock"),
  "factory.createArrowFunction": auditAlreadyExact("crates/emitter/src/factory.rs:702", "createArrowFunction"),
  "factory.createTypePredicateNode": auditAlreadyExact("crates/emitter/src/factory.rs:2070", "createTypePredicateNode"),
  "factory.updateBindingElement": auditAlreadyExact("crates/emitter/src/factory.rs:3602", "updateBindingElement"),
  "factory.createImportAttributes": auditAlreadyExact("crates/emitter/src/factory.rs:4111", "createImportAttributes"),
  "factory.createImportAttribute": auditAlreadyExact("crates/emitter/src/factory.rs:4131", "createImportAttribute"),
  "factory.createImportTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2484", "createImportTypeNode"),
  "factory.createParenthesizedType": auditAlreadyExact("crates/emitter/src/factory.rs:2548", "createParenthesizedType"),
  "factory.createPropertyAccessExpression": auditAlreadyExact("crates/emitter/src/factory.rs:3546", "createPropertyAccessExpression"),
  "factory.createElementAccessExpression": auditAlreadyExact("crates/emitter/src/factory.rs:3577", "createElementAccessExpression"),
  "factory.updateModuleDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:3944", "updateModuleDeclaration"),
  "factory.updateModuleBlock": auditAlreadyExact("crates/emitter/src/factory.rs:3993", "updateModuleBlock"),
  "factory.createExportDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:4247", "createExportDeclaration"),
  "factory.createExportSpecifier": auditAlreadyExact("crates/emitter/src/factory.rs:4349", "createExportSpecifier"),
  "factory.createNamedExports": auditAlreadyExact("crates/emitter/src/factory.rs:4311", "createNamedExports"),
  "factory.updateExportDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:4275", "updateExportDeclaration"),
  "factory.updateNamedExports": auditAlreadyExact("crates/emitter/src/factory.rs:4329", "updateNamedExports"),
  "factory.createVariableStatement": auditAlreadyExact("crates/emitter/src/factory.rs:3699", "createVariableStatement"),
  "factory.createVariableDeclarationList": auditAlreadyExact("crates/emitter/src/factory.rs:696", "createVariableDeclarationList"),
  "factory.createVariableDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:3646", "createVariableDeclaration"),
  "factory.createExportAssignment": auditAlreadyExact("crates/emitter/src/factory.rs:4225", "createExportAssignment"),
  "factory.createTypeAliasDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:3858", "createTypeAliasDeclaration"),
  "factory.createHeritageClause": auditAlreadyExact("crates/emitter/src/factory.rs:4389", "createHeritageClause"),
  "factory.createInterfaceDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:3834", "createInterfaceDeclaration"),
  "factory.createPropertyDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:2917", "createPropertyDeclaration"),
  "factory.createModuleBlock": auditAlreadyExact("crates/emitter/src/factory.rs:3976", "createModuleBlock"),
  "factory.createModuleDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:3905", "createModuleDeclaration"),
  "factory.createEnumMember": auditAlreadyExact("crates/emitter/src/factory.rs:4418", "createEnumMember"),
  "factory.createEnumDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:3880", "createEnumDeclaration"),
  "factory.createEmptyStatement": auditAlreadyExact("crates/emitter/src/factory.rs:3725", "createEmptyStatement"),
  "factory.createExpressionStatement": auditAlreadyExact("crates/emitter/src/factory.rs:3738", "createExpressionStatement"),
  "factory.createExpressionWithTypeArguments": auditAlreadyExact("crates/emitter/src/factory.rs:2724", "createExpressionWithTypeArguments"),
  "factory.createPrivateIdentifier": auditAlreadyExact("crates/emitter/src/factory.rs:1722", "createPrivateIdentifier"),
  "factory.createClassDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:3755", "createClassDeclaration"),
  "factory.createImportDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:4060", "createImportDeclaration"),
  "factory.createImportClause": auditAlreadyExact("crates/emitter/src/factory.rs:4085", "createImportClause"),
  "factory.createImportSpecifier": auditAlreadyExact("crates/emitter/src/factory.rs:4203", "createImportSpecifier"),
  "factory.createNamedImports": auditAlreadyExact("crates/emitter/src/factory.rs:4185", "createNamedImports"),
  "factory.createUniqueName": auditAlreadyExact("crates/emitter/src/factory.rs:1744", "createUniqueName"),
  "factory.createImportEqualsDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:4031", "createImportEqualsDeclaration"),
  "factory.createExternalModuleReference": auditAlreadyExact("crates/emitter/src/factory.rs:4371", "createExternalModuleReference"),
  "factory.createNamespaceExportDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:4013", "createNamespaceExportDeclaration"),
  "factory.createNamespaceImport": auditAlreadyExact("crates/emitter/src/factory.rs:4149", "createNamespaceImport"),
  "factory.createNamespaceExport": auditAlreadyExact("crates/emitter/src/factory.rs:4167", "createNamespaceExport"),

  // Expression-level parenthesization exists in the current Rust emitter;
  // all type-level parenthesizer members remain declaration foundation.
  "parenthesizer.parenthesizeLeadingTypeArgument": auditAlreadyExact("crates/emitter/src/factory.rs:1427", "parenthesizeLeadingTypeArgument"),
  "parenthesizer.parenthesizeExpressionForDisallowedComma": auditAlreadyExact(
    "crates/emitter/src/factory.rs:1870",
  ),
  "parenthesizer.parenthesizeLeftSideOfAccess": auditAlreadyExact(
    "crates/emitter/src/printer.rs:7932",
  ),
  "parenthesizer.parenthesizeNonArrayTypeOfPostfixType": auditAlreadyExact("crates/emitter/src/factory.rs:1350", "parenthesizeNonArrayTypeOfPostfixType"),
  "parenthesizer.parenthesizeElementTypeOfTupleType": auditAlreadyExact("crates/emitter/src/factory.rs:1401", "parenthesizeElementTypeOfTupleType"),
  "parenthesizer.parenthesizeTypeOfOptionalType": auditAlreadyExact("crates/emitter/src/factory.rs:1414", "parenthesizeTypeOfOptionalType"),
  "parenthesizer.parenthesizeConstituentTypeOfUnionType": auditAlreadyExact("crates/emitter/src/factory.rs:1292", "parenthesizeConstituentTypeOfUnionType"),
  "parenthesizer.parenthesizeConstituentTypeOfIntersectionType": auditAlreadyExact("crates/emitter/src/factory.rs:1308", "parenthesizeConstituentTypeOfIntersectionType"),
  "parenthesizer.parenthesizeCheckTypeOfConditionalType": auditAlreadyExact("crates/emitter/src/factory.rs:1263", "parenthesizeCheckTypeOfConditionalType"),
  "parenthesizer.parenthesizeExtendsTypeOfConditionalType": auditAlreadyExact("crates/emitter/src/factory.rs:1279", "parenthesizeExtendsTypeOfConditionalType"),
  "parenthesizer.parenthesizeOperandOfReadonlyTypeOperator": auditAlreadyExact("crates/emitter/src/factory.rs:1337", "parenthesizeOperandOfReadonlyTypeOperator"),
  "parenthesizer.parenthesizeOperandOfTypeOperator": auditAlreadyExact("crates/emitter/src/factory.rs:1324", "parenthesizeOperandOfTypeOperator"),

  "factory.updateIndexedAccessTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2641", "updateIndexedAccessTypeNode"),
  "factory.updateTypeOperatorNode": auditAlreadyExact("crates/emitter/src/factory.rs:2600", "updateTypeOperatorNode"),
  "factory.updateTypeQueryNode": auditAlreadyExact("crates/emitter/src/factory.rs:2227", "updateTypeQueryNode"),
  "factory.updateTypeParameterDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:2770", "updateTypeParameterDeclaration"),
  "factory.updateComputedPropertyName": auditAlreadyExact("crates/emitter/src/factory.rs:2040", "updateComputedPropertyName"),
  "factory.updateTypePredicateNode": auditAlreadyExact("crates/emitter/src/factory.rs:2090", "updateTypePredicateNode"),
  "factory.updateConditionalTypeNode": auditAlreadyExact("crates/emitter/src/factory.rs:2417", "updateConditionalTypeNode"),
  "factory.updateParameterDeclaration": auditAlreadyExact("crates/emitter/src/factory.rs:2854", "updateParameterDeclaration"),
  "factory.updateGetAccessorDeclaration": auditAlreadyExact(
    "crates/emitter/src/factory.rs:1501",
  ),
  "factory.updateSetAccessorDeclaration": auditAlreadyExact(
    "crates/emitter/src/factory.rs:1556",
  ),
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

function auditRow(name) {
  const audit = AUDIT_ROWS[name];
  requireCondition(audit !== undefined, `missing curated audit row ${name}`);
  return audit;
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
const createPrinterAudit = auditRow("createPrinter");
rows.push(
  rowForNode({
    surface: "printer-subgraph",
    name: "createPrinter",
    kind: "owner",
    node: createPrinter,
    reachability: "context",
    consumers: ["H2.7a", "H2.7b", "H2.7d", "API1"],
    rustAnchor: createPrinterAudit.rust_anchor,
    disposition: createPrinterAudit.disposition,
    targetRung:
      createPrinterAudit.disposition === "audit-foundation-needed"
        ? "h2-7a-m-3.5"
        : null,
  }),
);
for (const node of printerSubgraph) {
  const name = functionName(node);
  const audit = auditRow(name);
  rows.push(
    rowForNode({
      surface: "printer-subgraph",
      name,
      kind: "nested",
      node,
      reachability: printerSeedSet.has(node) ? "direct" : "reached",
      consumers: ["H2.7a", "H2.7b", "H2.7d", "API1"],
      rustAnchor: audit.rust_anchor,
      disposition: audit.disposition,
      targetRung:
        audit.disposition === "audit-foundation-needed" ? "h2-7a-m-3.5" : null,
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
  const audit = auditRow(`${receiver}.${name}`);
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
    rust_anchor: audit.rust_anchor,
    disposition: audit.disposition,
    target_rung:
      audit.disposition === "audit-foundation-needed" ? "h2-7a-m-3.5" : null,
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

const measuredAuditNames = [
  "createPrinter",
  ...[...printerSubgraph].map(functionName),
  ...[...factoryMemberSites.keys()].map((name) => `factory.${name}`),
  ...[...parenthesizerMemberSites.keys()].map((name) => `parenthesizer.${name}`),
];
const measuredAuditNameSet = new Set(measuredAuditNames);
const curatedAuditNames = Object.keys(AUDIT_ROWS);
const rustAnchorLineCounts = new Map();
const rustAnchorLineCache = new Map();
let headerVerifiedAuditRows = 0;
function rustAnchorLines(relativePath) {
  if (!rustAnchorLineCache.has(relativePath)) {
    rustAnchorLineCache.set(relativePath, splitLines(readBytes(relativePath).toString("utf8")));
  }
  return rustAnchorLineCache.get(relativePath);
}
requireCondition(printerSubgraph.size + 1 === 184, "printer audit must contain 184 rows");
requireCondition(
  factoryMemberSites.size + parenthesizerMemberSites.size === 124,
  "factory/parenthesizer audit must contain 124 rows",
);
requireCondition(measuredAuditNames.length === 308, "audit must contain 308 measured rows");
requireCondition(measuredAuditNameSet.size === 308, "audit names must be unique");
requireCondition(curatedAuditNames.length === 308, "curated audit must contain 308 rows");
for (const name of curatedAuditNames) {
  requireCondition(measuredAuditNameSet.has(name), `extra curated audit row ${name}`);
  const audit = auditRow(name);
  requireCondition(
    audit.disposition === "audit-already-exact" ||
      audit.disposition === "audit-foundation-needed",
    `invalid curated audit disposition for ${name}`,
  );
  requireCondition(
    audit.disposition === "audit-already-exact"
      ? typeof audit.rust_anchor === "string"
      : audit.rust_anchor === null,
    `invalid curated Rust anchor for ${name}`,
  );
  if (audit.rust_anchor !== null) {
    const match = audit.rust_anchor.match(
      /^(crates\/emitter\/[A-Za-z0-9_./-]+):([1-9][0-9]*)$/u,
    );
    requireCondition(match !== null, `malformed curated Rust anchor for ${name}`);
    const [, relativePath, lineText] = match;
    if (!rustAnchorLineCounts.has(relativePath)) {
      rustAnchorLineCounts.set(
        relativePath,
        splitLines(readBytes(relativePath).toString("utf8")).length,
      );
    }
    requireCondition(
      Number(lineText) <= rustAnchorLineCounts.get(relativePath),
      `out-of-range curated Rust anchor for ${name}`,
    );
    if (audit.header !== null && audit.header !== undefined) {
      const lines = rustAnchorLines(relativePath);
      const center = Number(lineText) - 1;
      const window = lines.slice(Math.max(0, center - 3), center + 4).join("\n");
      requireCondition(
        window.includes(`tsc-port: ${audit.header} `) || window.includes(`tsc-port: ${audit.header}/`) || new RegExp(`tsc-port: [^\\n]*\\b${audit.header.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\b`).test(window),
        `curated Rust anchor for ${name} is not at a tsc-port header naming ${audit.header}`,
      );
      headerVerifiedAuditRows += 1;
    }
  }
}

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
const auditRows = rows.filter(
  (row) => row.surface === "printer-subgraph" || row.surface === "factory-parenthesizer",
);
const auditCounts = {
  already_exact: auditRows.filter((row) => row.disposition === "audit-already-exact").length,
  foundation_needed: auditRows.filter(
    (row) => row.disposition === "audit-foundation-needed",
  ).length,
  pending: auditRows.filter((row) => row.disposition === "audit-pending").length,
};
requireCondition(auditRows.length === 308, "generated audit must contain 308 rows");
requireCondition(
  auditCounts.already_exact + auditCounts.foundation_needed === auditRows.length,
  "every generated audit row must have a final disposition",
);
requireCondition(auditCounts.pending === 0, "generated audit must have zero pending rows");
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
  audit: auditCounts,
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
  `reached=${summary.reached_rows} options=${summary.option_rows} ` +
  `audit=${summary.audit.already_exact}/${summary.audit.foundation_needed}/${summary.audit.pending}`;

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
