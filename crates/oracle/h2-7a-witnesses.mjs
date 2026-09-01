// W-H2.7A — public-observable declaration witness machine for h2-7a-m-2.
//
// Selection is frozen before observation.  F1-F14 are fixture-name/input
// strata; S is the signed W5 exact-but-for-declarations stratum and S2 is the
// frozen m-2 coverage supplement.  Every case
// is observed through the pinned TypeScript runtime in two fresh Node
// processes.  Declaration writes embed their full materialized bytes; script
// and map writes carry hashes only because their bytes remain in W-H2.6C.

import crypto from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "./vfs-directory-overlay.mjs";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const MODE = process.argv[2];

const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-7a-witnesses.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-7a-witnesses.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-7a-witnesses.schema.json";
const PACKET_RELATIVE_PATH = "docs/design/greenfield/slices/h2-7a-m-2.md";
const PARENT_PACKET_RELATIVE_PATH = "docs/design/greenfield/slices/h2-7a.md";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const TYPESCRIPT_LIB_DIRECTORY = "vendor/typescript-6.0.3/lib";
const TEST_SUITE_EXPANSION =
  "vendor/typescript-6.0.3/test-suite-expansion.v1.json";
const CONFORMANCE_EXPANSION =
  "vendor/typescript-6.0.3/conformance-suite-expansion.v1.json";
const COMPILER_CLASSIFICATION =
  "vendor/typescript-6.0.3/compiler-profile-classification.v1.json";
const CONFORMANCE_CLASSIFICATION =
  "vendor/typescript-6.0.3/conformance-profile-classification.v1.json";
const PROJECT_CLASSIFICATION =
  "vendor/typescript-6.0.3/project-profile-classification.v1.json";
const TRANSPILE_INVENTORY =
  "vendor/typescript-6.0.3/transpile-suite-inventory.v1.json";
const H2_6C_CENSUS = "ratchets/h2-6c-census.v1.json";
const H2_6C_QUALIFICATION = "ratchets/h2-6c-qualification.v1.json";
const H2_6C_DIVERGENCES = "ratchets/h2-6c-known-divergences.v1.json";
const VFS_DIRECTORY_OVERLAY = "crates/oracle/vfs-directory-overlay.mjs";

const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";
const VIRTUAL_SOURCE_ROOT = "/.src";
const PROJECT_VIRTUAL_PREFIX = "/.src/tests/cases/projects";
const INTERNAL_OBSERVE_MODE = "--internal-observe";
const CONTROL_FILE_ENV = "TSRS_H2_7A_OBSERVATION_CONTROL";
const STRATUM_CENSUS_ENV = "TSRS_H2_7A_STRATUM_CENSUS";
const CHECK_RECEIPT_RELATIVE_PATH =
  "target/h2-7a/check-receipt.v1.json";
const CONTROL_RELATIVE_PATH = "target/h2-7a/observation-control.v1.json";
const DEFAULT_CENSUS_DIRECTORY = "target/h2-7a/stratum-census";
const DEFAULT_CENSUS_RELATIVE_PATH =
  `${DEFAULT_CENSUS_DIRECTORY}/census.jsonl`;
const UTF8_BOM = Buffer.from([0xef, 0xbb, 0xbf]);

// This projection is the permanent bridge to the signed m-1 witness set.  It
// is deliberately independent of the successor artifact's new S2 fields.
const H2_7A_M1_PROJECTION_SHA256 =
  "44b0cca40a9ae8869ee219e6bbb6e449ce87346556084dfd622f167bd3f55b72";

const LANES = Object.freeze([
  "visibility/export graph",
  "type serialization",
  "late-bound/computed names",
  "signatures/overloads/accessors",
  "JS declaration synthesis",
  "directives/references",
  "diagnostics channel",
  "printer grammar/shape",
  "NodeBuilder result contracts",
  "symbol tracking/accessibility",
  "syntactic-builder arms",
  "generated/global names",
  "AST identity/provenance",
  "upstream-observation controls",
]);

const ROLES = Object.freeze([
  "positive",
  "adjacent-negative-control",
  "composition",
  "fault",
  "supplement",
]);

const S2_SELECTOR_VERSION = "m2-s2-v1";
const S2_FROZEN_FIXTURE_IDS = Object.freeze([
  "typescript-6.0.3/compiler/declarationEmitExpandoPropertyPrivateName.ts",
  "typescript-6.0.3/compiler/declarationEmitExpandoWithGenericConstraint.ts",
  "typescript-6.0.3/compiler/declarationEmitFunctionDuplicateNamespace.ts",
  "typescript-6.0.3/compiler/declarationEmitFunctionKeywordProp.ts",
  "typescript-6.0.3/compiler/computedPropertiesNarrowed.ts",
  "typescript-6.0.3/compiler/computedPropertyNameAndTypeParameterConflict.ts",
  "typescript-6.0.3/compiler/declarationEmitExpressionWithNonlocalPrivateUniqueSymbol.ts",
  "typescript-6.0.3/compiler/declarationEmitHigherOrderRetainedGenerics.ts",
  "typescript-6.0.3/compiler/declFileAmbientExternalModuleWithSingleExportedModule.ts",
  "typescript-6.0.3/compiler/declarationEmitAnyComputedPropertyInClass.ts",
  "typescript-6.0.3/compiler/declarationEmitForModuleImportingModuleAugmentationRetainsImport.ts",
  "typescript-6.0.3/compiler/declarationEmitRedundantTripleSlashModuleAugmentation.ts",
  "typescript-6.0.3/compiler/amdModuleBundleNoDuplicateDeclarationEmitComments.ts",
  "typescript-6.0.3/compiler/circularReferenceInImport.ts",
  "typescript-6.0.3/compiler/commentsExternalModules.ts",
  "typescript-6.0.3/compiler/commentsExternalModules2.ts",
]);
const S2_FROZEN_CASE_IDS = Object.freeze([
  "h2-7a/S2/expando-1",
  "h2-7a/S2/expando-2",
  "h2-7a/S2/expando-3",
  "h2-7a/S2/expando-4",
  "h2-7a/S2/latebound-1",
  "h2-7a/S2/latebound-2",
  "h2-7a/S2/latebound-3",
  "h2-7a/S2/latebound-4",
  "h2-7a/S2/augment-1",
  "h2-7a/S2/augment-2",
  "h2-7a/S2/augment-3",
  "h2-7a/S2/augment-4",
  "h2-7a/S2/entityname-1",
  "h2-7a/S2/entityname-2",
  "h2-7a/S2/entityname-3-c0",
  "h2-7a/S2/entityname-3-c1",
  "h2-7a/S2/entityname-4-c0",
  "h2-7a/S2/entityname-4-c1",
]);
const S2_FROZEN_TRIM_ROWS = Object.freeze([]);
const S2_PREDICATES = Object.freeze({
  expando:
    "fixture basename contains `expando` (ASCII case-insensitive) OR some multiline match `^\\s*function\\s+(\\w+)` has a later multiline match `^\\s*\\1\\.\\w+\\s*=` (backreference on the captured name)",
  latebound: "bytes contain `[Symbol.` or `unique symbol`",
  augment:
    "basename contains `augment` (case-insensitive) OR bytes contain `declare module \"` AND a multiline match `^import `",
  entityname:
    "bytes contain `namespace` AND match `:\\s*[A-Za-z_$][\\w$]*\\.[A-Za-z_$]`",
});
const S2_FROZEN_MEMBERS = Object.freeze([
  {
    member: "expando",
    predicate: S2_PREDICATES.expando,
    lanes: ["JS declaration synthesis"],
    fixture_ids: Object.freeze(S2_FROZEN_FIXTURE_IDS.slice(0, 4)),
  },
  {
    member: "latebound",
    predicate: S2_PREDICATES.latebound,
    lanes: ["late-bound/computed names"],
    fixture_ids: Object.freeze(S2_FROZEN_FIXTURE_IDS.slice(4, 8)),
  },
  {
    member: "augment",
    predicate: S2_PREDICATES.augment,
    lanes: ["visibility/export graph"],
    fixture_ids: Object.freeze(S2_FROZEN_FIXTURE_IDS.slice(8, 12)),
  },
  {
    member: "entityname",
    predicate: S2_PREDICATES.entityname,
    lanes: ["visibility/export graph"],
    fixture_ids: Object.freeze(S2_FROZEN_FIXTURE_IDS.slice(12, 16)),
  },
]);
const S2_VOLUME_TABLE = Object.freeze([
  Object.freeze({ member: "getPropertiesOfContainerFunction", entry_result_pairs: 0 }),
  Object.freeze({ member: "isDefinitelyReferenceToGlobalSymbolObject", entry_result_pairs: 1 }),
  Object.freeze({ member: "isLateBound", entry_result_pairs: 1 }),
  Object.freeze({ member: "getEnumMemberValue", entry_result_pairs: 3 }),
  Object.freeze({ member: "isImportRequiredByAugmentation", entry_result_pairs: 9 }),
  Object.freeze({ member: "isEntityNameVisible", entry_result_pairs: 28 }),
  Object.freeze({ member: "requiresAddingImplicitUndefined", entry_result_pairs: 86 }),
  Object.freeze({ member: "isImplementationOfOverload", entry_result_pairs: 149 }),
  Object.freeze({ member: "isOptionalParameter", entry_result_pairs: 205 }),
  Object.freeze({ member: "isSymbolAccessible", entry_result_pairs: 349 }),
  Object.freeze({ member: "isExpandoFunctionDeclaration", entry_result_pairs: 408 }),
  Object.freeze({ member: "isLiteralConstDeclaration", entry_result_pairs: 548 }),
  Object.freeze({ member: "isDeclarationVisible", entry_result_pairs: 1534 }),
]);

const QUOTA_MINIMUMS = Object.freeze({
  positive_cases: 20,
  adjacent_negative_cases: 2,
  composition_cases: 3,
  fault_cases: 2,
});

const OPTION_INDEX = new Map(
  ts.optionDeclarations.map((option) => [option.name.toLowerCase(), option]),
);
const HARNESS_ONLY_OPTIONS = new Set(
  [
    "useCaseSensitiveFileNames", "baselineFile", "fileName", "filename",
    "suppressOutputPathCheck", "noImplicitReferences", "currentDirectory",
    "symlink", "link", "noTypesAndSymbols", "fullEmitPaths",
    "reportDiagnostics", "captureSuggestions", "typeScriptVersion",
  ].map((name) => name.toLowerCase()),
);
const PROJECT_STRUCTURAL_KEYS = new Set([
  "scenario", "projectRoot", "inputFiles", "baselineCheck", "runTest",
  "project", "emittedFiles", "resolveMapRoot", "resolveSourceRoot",
]);
const PROJECT_MODULE_VARIANTS = Object.freeze({
  amd: Object.freeze({ name: "amd", value: ts.ModuleKind.AMD }),
  commonjs: Object.freeze({ name: "commonjs", value: ts.ModuleKind.CommonJS }),
});

// Resolved now from /^declarationEmit.*[Oo]verload/ with @declaration.
const F4_OVERLOAD =
  "declarationEmitDestructuringOptionalBindingParametersInOverloads.ts";
// Resolved now: the first two declaration-bearing Reference/TripleSlash names.
const F6_REFERENCES = Object.freeze([
  "declarationEmitBundleWithAmbientReferences.ts",
  "declarationEmitCommonJsModuleReferencedType.ts",
]);
// Resolved now from /declarationEmit.*(Error|Private)/ with @declaration.
const F7_PRIVATE = "declarationEmitClassPrivateConstructor.ts";
// Resolved now: the first two declaration-bearing grammar-name matches.
const F8_GRAMMAR = Object.freeze([
  "declarationEmitDistributiveConditionalWithInfer.ts",
  "declarationEmitInlinedDistributiveConditional.ts",
]);
// Resolved now from /declarationEmit.*(Truncat|Deep|Recursi)/.
const F9_NODE_BUILDER = "declarationEmitRecursiveConditionalAliasPreserved.ts";
// The strict F10 regex has only the explicit fixture in this corpus.  The
// deterministic substitution is the lexicographically first remaining
// declaration-bearing compiler fixture whose name contains Inaccessible.
const F10_ACCESSIBILITY_SUBSTITUTION = "aliasInaccessibleModule.ts";
// Resolved now from /declarationEmit.*(Collision|Shadow)/.
const F11_GENERATED_NAMES = "declarationEmitShadowing.ts";
// No declarationEmit Decorat/ClassField/StaticBlock match exists and the
// named classStaticBlock25 fallback is absent.  Freeze the lexicographically
// first declaration-bearing static-block fixture as the recorded substitute.
const F12_PROVENANCE_SUBSTITUTION =
  "javascriptThisAssignmentInStaticBlock.ts";
// Resolved now by the signed tiny-control rule (no declaration/sourceMap
// directive and fewer than 20 physical lines).
const F13_TINY_CONTROL = "2dArrays.ts";

function fixtureCase(slug, fixture, role = "positive", mutation = "fixture") {
  return { slug, fixture, roles: [role], mutation };
}

const FAMILY_SPECS = Object.freeze([
  {
    family_id: "F1",
    description: "visibility and export-graph declaration shaping",
    lanes: ["visibility/export graph"],
    suite: "compiler",
    cases: [
      fixtureCase("alias-export-star", "declarationEmitAliasExportStar.ts"),
      fixtureCase(
        "alias-from-indirect-file",
        "declarationEmitAliasFromIndirectFile.ts",
      ),
      fixtureCase("alias-inlining", "declarationEmitAliasInlineing.ts"),
    ],
  },
  {
    family_id: "F2",
    description: "annotated and inferred type serialization arms",
    lanes: ["type serialization", "syntactic-builder arms"],
    suite: "compiler",
    cases: [
      fixtureCase("inferred-type-alias-1", "declarationEmitInferredTypeAlias1.ts"),
      fixtureCase(
        "inferred-default-export",
        "declarationEmitInferredDefaultExportType.ts",
      ),
      fixtureCase("inferred-type-alias-8", "declarationEmitInferredTypeAlias8.ts"),
    ],
  },
  {
    family_id: "F3",
    description: "late-bound and computed declaration names",
    lanes: ["late-bound/computed names"],
    suite: "compiler",
    cases: [
      fixtureCase(
        "computed-const-enum-alias",
        "declarationEmitComputedNameConstEnumAlias.ts",
      ),
      fixtureCase(
        "computed-question-token",
        "declarationEmitComputedNameWithQuestionToken.ts",
      ),
    ],
  },
  {
    family_id: "F4",
    description: "signature, overload, and accessor declaration shaping",
    lanes: ["signatures/overloads/accessors"],
    suite: "compiler",
    cases: [
      fixtureCase(
        "accessor-visibility-error",
        "accessorDeclarationEmitVisibilityErrors.ts",
        "fault",
      ),
      fixtureCase("overload", F4_OVERLOAD),
    ],
  },
  {
    family_id: "F5",
    description: "allowJs declaration synthesis",
    lanes: ["JS declaration synthesis"],
    suite: "conformance",
    cases: [
      fixtureCase(
        "js-classes",
        "jsdoc/declarations/jsDeclarationsClasses.ts",
        "composition",
      ),
      fixtureCase(
        "js-functions",
        "jsdoc/declarations/jsDeclarationsFunctions.ts",
      ),
    ],
  },
  {
    family_id: "F6",
    description: "reference and directive synthesis",
    lanes: ["directives/references"],
    suite: "compiler",
    cases: [
      fixtureCase("references-first", F6_REFERENCES[0]),
      fixtureCase("references-second", F6_REFERENCES[1]),
    ],
  },
  {
    family_id: "F7",
    description: "declaration diagnostics and privacy faults",
    lanes: ["diagnostics channel"],
    suite: "compiler",
    cases: [
      fixtureCase(
        "accessor-visibility-error",
        "accessorDeclarationEmitVisibilityErrors.ts",
        "fault",
      ),
      fixtureCase("private-surface", F7_PRIVATE),
    ],
  },
  {
    family_id: "F8",
    description: "composed TypeNode printer grammar",
    lanes: ["printer grammar/shape"],
    suite: "compiler",
    cases: [
      fixtureCase("grammar-first", F8_GRAMMAR[0]),
      fixtureCase("grammar-second", F8_GRAMMAR[1]),
    ],
  },
  {
    family_id: "F9",
    description: "NodeBuilder recursion and result contracts",
    lanes: ["NodeBuilder result contracts"],
    suite: "compiler",
    cases: [fixtureCase("recursive-contract", F9_NODE_BUILDER)],
  },
  {
    family_id: "F10",
    description: "symbol tracking and accessibility",
    lanes: ["symbol tracking/accessibility"],
    suite: "compiler",
    cases: [
      fixtureCase(
        "computed-inaccessible",
        "declarationEmitComputedNamesInaccessible.ts",
      ),
      fixtureCase("inaccessible-substitution", F10_ACCESSIBILITY_SUBSTITUTION),
    ],
  },
  {
    family_id: "F11",
    description: "generated and global-name collision handling",
    lanes: ["generated/global names"],
    suite: "compiler",
    cases: [fixtureCase("shadowing", F11_GENERATED_NAMES)],
  },
  {
    family_id: "F12",
    description: "transform-heavy public byte baseline for AST provenance",
    lanes: ["AST identity/provenance"],
    suite: "compiler",
    cases: [fixtureCase("static-block-substitution", F12_PROVENANCE_SUBSTITUTION)],
  },
  {
    family_id: "F13",
    description: "declaration-off and JS-only adjacent controls",
    lanes: ["upstream-observation controls"],
    suite: "compiler",
    cases: [
      fixtureCase(
        "declaration-removed",
        "declarationEmitAliasExportStar.ts",
        "adjacent-negative-control",
        "remove-declaration",
      ),
      fixtureCase(
        "tiny-js-only",
        F13_TINY_CONTROL,
        "adjacent-negative-control",
      ),
    ],
  },
  {
    family_id: "F14",
    description: "declaration composition with source maps and CommonJS",
    lanes: ["printer grammar/shape", "visibility/export graph"],
    suite: "compiler",
    cases: [
      fixtureCase(
        "declaration-source-map",
        "declarationEmitAliasExportStar.ts",
        "composition",
        "declaration-source-map",
      ),
      fixtureCase(
        "declaration-commonjs",
        "declarationEmitInferredTypeAlias1.ts",
        "composition",
        "declaration-commonjs",
      ),
    ],
  },
]);

class CheckReceiptMiss extends Error {}

function fail(message) {
  throw new Error(`h2-7a-witnesses: ${message}`);
}

function requireCondition(condition, message) {
  if (!condition) fail(message);
}

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function gitBlobSha1(value) {
  const bytes = Buffer.isBuffer(value) ? value : Buffer.from(value);
  return crypto
    .createHash("sha1")
    .update(`blob ${bytes.length}\0`)
    .update(bytes)
    .digest("hex");
}

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

function fingerprintIsValid(record, field) {
  if (record === null || typeof record !== "object") return false;
  const { [field]: stored, ...payload } = record;
  return (
    typeof stored === "string" &&
    stored === sha256(Buffer.from(stableStringify(payload), "utf8"))
  );
}

function render(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
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

function compareBytes(left, right) {
  return Buffer.compare(Buffer.from(left), Buffer.from(right));
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

function safeSourcePath(suite, relativePath) {
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

function serializeOptions(options) {
  return Object.fromEntries(
    Object.entries(options)
      .filter(([, value]) => value !== undefined)
      .sort(([left], [right]) => compareBytes(left, right)),
  );
}

function libraryInventoryRecord() {
  const directory = path.join(WORKSPACE, TYPESCRIPT_LIB_DIRECTORY);
  const names = fs
    .readdirSync(directory)
    .filter((name) => name.startsWith("lib.") && name.endsWith(".d.ts"))
    .sort(compareBytes);
  requireCondition(names.length > 0, "vendored TypeScript lib inventory is empty");
  const hash = crypto.createHash("sha256");
  for (const name of names) {
    hash.update(name);
    hash.update("\0");
    hash.update(fs.readFileSync(path.join(directory, name)));
    hash.update("\0");
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

function validateRuntime() {
  const node = readBytes(".node-version").toString("utf8").trim();
  requireCondition(node === EXPECTED_NODE, "unexpected Node pin");
  requireCondition(process.version === `v${node}`, `requires Node ${node}`);
  requireCondition(ts.version === "6.0.3", "unexpected TypeScript runtime");
  for (const name of [
    "createProgram",
    "createCompilerHost",
    "createSourceFile",
    "emitFilesAndReportErrorsAndGetExitStatus",
    "parseListTypeOption",
    "parseCustomTypeOption",
  ]) {
    requireCondition(
      typeof ts[name] === "function",
      `pinned TypeScript does not expose ${name}`,
    );
  }
}

function orderedSettings(object) {
  return Object.entries(object).map(([name, value]) => ({ name, value }));
}

function contentIdentity(text) {
  if (text === undefined) return { state: "missing" };
  const bytes = Buffer.from(text, "utf8");
  return { state: "present", utf8_bytes: bytes.length, sha256: sha256(bytes) };
}

function documentSymlinks(fileOptions) {
  const setting = fileOptions.find(
    (entry) => entry.name.toLowerCase() === "symlink",
  );
  if (!setting || setting.value === "") return [];
  return setting.value.split(",").map((entry) => entry.trim());
}

// This is the TypeScript test-harness unit splitter used by W-H2.6C.
function makeUnits(text, fixturePath) {
  const units = [];
  const links = [];
  let currentContent;
  let currentOptions = {};
  let currentName;
  const optionPattern = /^\/{2}\s*@([\w]+)\s*:\s*([^\r\n]*)/;
  const linkPattern = /^\/{2}\s*@link\s*:\s*([^\r\n]*)\s*->\s*([^\r\n]*)/;
  for (const line of text.split(/\r?\n/)) {
    const link = linkPattern.exec(line);
    if (link) {
      links.push({ target: link[1].trim(), link_path: link[2].trim() });
      continue;
    }
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
            ts.skipTrivia(currentContent, 0, false, false) ===
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
  return { units, links };
}

function mergedSettings(base, overrides) {
  const settings = new Map(base.map((setting) => [setting.name, setting.value]));
  for (const setting of overrides ?? []) settings.set(setting.name, setting.value);
  return settings;
}

function optionValue(option, raw) {
  const errors = [];
  let value;
  if (option.type === "boolean") value = String(raw).toLowerCase() === "true";
  else if (option.type === "string") value = String(raw);
  else if (option.type === "number") value = Number.parseInt(raw, 10);
  else if (option.type === "list" || option.type === "listOrElement") {
    value = ts.parseListTypeOption(option, raw, errors);
  } else {
    value = ts.parseCustomTypeOption(option, raw, errors);
  }
  requireCondition(errors.length === 0, `invalid @${option.name}: ${raw}`);
  return value;
}

function effectiveCompilerOptions(settings, baseOptions = { noResolve: false }) {
  const options = ts.cloneCompilerOptions(baseOptions);
  options.newLine = ts.NewLineKind.CarriageReturnLineFeed;
  options.noErrorTruncation = true;
  options.skipDefaultLibCheck = true;
  for (const [name, raw] of settings) {
    if (name === "typeScriptVersion") continue;
    const option = OPTION_INDEX.get(name.toLowerCase());
    if (option !== undefined) {
      options[option.name] = optionValue(option, raw);
      continue;
    }
    requireCondition(
      HARNESS_ONLY_OPTIONS.has(name.toLowerCase()),
      `unknown harness/compiler option @${name}`,
    );
  }
  return options;
}

function exactSetting(settings, name) {
  return [...settings].find(([candidate]) => candidate === name)?.[1];
}

function currentDirectory(settings) {
  const configured = exactSetting(settings, "currentDirectory");
  return configured === undefined
    ? VIRTUAL_SOURCE_ROOT
    : ts.getNormalizedAbsolutePath(configured, VIRTUAL_SOURCE_ROOT);
}

function containsReferencePath(text) {
  return [...text.matchAll(/reference/g)].some((match) =>
    /^\s+path/.test(text.slice(match.index + "reference".length)),
  );
}

function explicitRootSelection(units, settings, options) {
  const cwd = currentDirectory(settings);
  const lastUnitByPath = new Map();
  units.forEach((unit, id) => {
    lastUnitByPath.set(ts.getNormalizedAbsolutePath(unit.name, cwd), id);
  });
  const candidates = [...lastUnitByPath.values()].sort((left, right) => left - right);
  const last = candidates.at(-1);
  requireCondition(last !== undefined, "fixture has no source unit");
  const lastUnit = units[last];
  const implicitReferences =
    exactSetting(settings, "noImplicitReferences") !== undefined ||
    (lastUnit.text ?? "").includes("require(") ||
    containsReferencePath(lastUnit.text ?? "");
  const rootUnitIds = implicitReferences ? [last] : candidates;
  const otherUnitIds = implicitReferences
    ? candidates.filter((id) => units[id].name !== lastUnit.name)
    : [];
  return {
    root_unit_ids: rootUnitIds,
    other_unit_ids: otherUnitIds,
    program_root_unit_ids: rootUnitIds.filter(
      (id) =>
        !ts.fileExtensionIs(units[id].name, ts.Extension.Json) &&
        ts.isSupportedSourceFileName(units[id].name, options),
    ),
    vfs_write_order: [...rootUnitIds, ...otherUnitIds],
  };
}

function hasDeclarationDirective(text) {
  return /^\s*\/\/\s*@declaration\s*:\s*true\b/im.test(text);
}

function compilerFixtureNames() {
  const directory = path.join(WORKSPACE, "ts-tests/tests/cases/compiler");
  return fs
    .readdirSync(directory, { withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => entry.name)
    .sort(compareBytes);
}

function declarationBearingCompilerNames(pattern) {
  return compilerFixtureNames().filter((name) => {
    if (!pattern.test(name)) return false;
    return hasDeclarationDirective(
      fs.readFileSync(safeSourcePath("compiler", name), "utf8"),
    );
  });
}

function validateFrozenFixtureChoices() {
  const overloads = declarationBearingCompilerNames(
    /^declarationEmit.*[Oo]verload/,
  );
  requireCondition(
    overloads[0] === F4_OVERLOAD,
    `F4 overload selection changed: ${overloads[0]}`,
  );

  const references = declarationBearingCompilerNames(
    /^declarationEmit.*(TripleSlash|Reference)/,
  );
  requireCondition(
    stableStringify(references.slice(0, 2)) === stableStringify(F6_REFERENCES),
    "F6 reference selection changed",
  );

  const faults = declarationBearingCompilerNames(
    /declarationEmit.*(Error|Private)/,
  );
  requireCondition(faults[0] === F7_PRIVATE, "F7 private selection changed");

  const grammar = declarationBearingCompilerNames(
    /declarationEmit.*(Mapped|Conditional|Template|Tuple)/,
  );
  requireCondition(
    stableStringify(grammar.slice(0, 2)) === stableStringify(F8_GRAMMAR),
    "F8 grammar selection changed",
  );

  const nodeBuilder = declarationBearingCompilerNames(
    /declarationEmit.*(Truncat|Deep|Recursi)/,
  );
  requireCondition(
    (nodeBuilder[0] ?? "declarationEmitInferredTypeAlias9.ts") ===
      F9_NODE_BUILDER,
    "F9 NodeBuilder selection changed",
  );

  const accessibility = declarationBearingCompilerNames(
    /declarationEmit.*(Inaccessible|Accessib)/,
  );
  requireCondition(
    stableStringify(accessibility) ===
      stableStringify(["declarationEmitComputedNamesInaccessible.ts"]),
    "F10 strict accessibility population changed",
  );
  const remainingInaccessible = declarationBearingCompilerNames(
    /(Inaccessible|Accessib)/,
  ).filter((name) => name !== "declarationEmitComputedNamesInaccessible.ts");
  requireCondition(
    remainingInaccessible[0] === F10_ACCESSIBILITY_SUBSTITUTION,
    "F10 accessibility substitution changed",
  );

  const generatedNames = declarationBearingCompilerNames(
    /declarationEmit.*(Collision|Shadow)/,
  );
  requireCondition(
    generatedNames[0] === F11_GENERATED_NAMES,
    "F11 generated-name selection changed",
  );

  const transformHeavy = declarationBearingCompilerNames(
    /declarationEmit.*(Decorat|ClassField|StaticBlock)/,
  );
  requireCondition(transformHeavy.length === 0, "F12 primary selection appeared");
  requireCondition(
    !fs.existsSync(safeSourcePath("compiler", "classStaticBlock25.ts")),
    "F12 named fallback appeared; revise the frozen substitution",
  );
  const staticBlocks = declarationBearingCompilerNames(/StaticBlock/i);
  requireCondition(
    staticBlocks[0] === F12_PROVENANCE_SUBSTITUTION,
    "F12 static-block substitution changed",
  );

  const tinyControls = compilerFixtureNames().filter((name) => {
    const text = fs.readFileSync(safeSourcePath("compiler", name), "utf8");
    const physicalLines = text === "" ? 0 : text.split(/\r?\n/).length;
    return (
      physicalLines < 20 &&
      !/^\s*\/\/\s*@declaration\s*:/im.test(text) &&
      !/^\s*\/\/\s*@sourceMap\s*:/im.test(text)
    );
  });
  requireCondition(
    tinyControls[0] === F13_TINY_CONTROL,
    `F13 tiny control changed: ${tinyControls[0]}`,
  );
}

function walkSuiteFiles(suite) {
  const root = path.join(WORKSPACE, "ts-tests/tests/cases", suite);
  const files = [];
  const visit = (directory) => {
    const entries = fs
      .readdirSync(directory, { withFileTypes: true })
      .sort((left, right) => compareBytes(left.name, right.name));
    for (const entry of entries) {
      const absolute = path.join(directory, entry.name);
      requireCondition(!entry.isSymbolicLink(), `unsupported fixture symlink ${absolute}`);
      if (entry.isDirectory()) visit(absolute);
      else if (entry.isFile()) files.push(absolute);
    }
  };
  visit(root);
  return files;
}

function asciiLower(value) {
  return value.replace(/[A-Z]/g, (character) =>
    String.fromCharCode(character.charCodeAt(0) + 0x20),
  );
}

function s2CandidateUniverse() {
  const candidates = [];
  for (const suite of ["compiler", "conformance"]) {
    const root = path.join(WORKSPACE, "ts-tests/tests/cases", suite);
    for (const absolute of walkSuiteFiles(suite)) {
      const relativePath = path
        .relative(root, absolute)
        .split(path.sep)
        .join("/");
      if (!/\.tsx?$/.test(relativePath)) continue;
      const raw = fs.readFileSync(absolute);
      if (!/@declaration\s*:\s*true/i.test(raw.toString("utf8"))) continue;
      candidates.push({
        suite,
        suite_rank: suite === "compiler" ? 0 : 1,
        relative_path: relativePath,
        fixture_id: `typescript-6.0.3/${suite}/${relativePath}`,
        raw,
      });
    }
  }
  return candidates.sort(
    (left, right) =>
      left.suite_rank - right.suite_rank ||
      compareBytes(left.relative_path, right.relative_path),
  );
}

const S2_EXPANDO_FUNCTION_ASSIGNMENT =
  /^\s*function\s+(\w+)[^\r\n]*(?:\r?\n[\s\S]*?)^\s*\1\.\w+\s*=/m;
const S2_ENTITY_NAME = /:\s*[A-Za-z_$][\w$]*\.[A-Za-z_$]/;

function s2MemberMatches(member, candidate) {
  const text = candidate.raw.toString("utf8");
  const basename = asciiLower(path.posix.basename(candidate.relative_path));
  if (member === "expando") {
    return (
      basename.includes("expando") ||
      S2_EXPANDO_FUNCTION_ASSIGNMENT.test(text)
    );
  }
  if (member === "latebound") {
    return (
      candidate.raw.includes(Buffer.from("[Symbol.")) ||
      candidate.raw.includes(Buffer.from("unique symbol"))
    );
  }
  if (member === "augment") {
    return (
      basename.includes("augment") ||
      (candidate.raw.includes(Buffer.from('declare module "')) &&
        /^import /m.test(text))
    );
  }
  if (member === "entityname") {
    return candidate.raw.includes(Buffer.from("namespace")) && S2_ENTITY_NAME.test(text);
  }
  fail(`unknown S2 member ${member}`);
}

function s2FixtureIdsFromCaseSpecs(caseSpecs) {
  return new Set(
    caseSpecs.map((entry) => entry.manifest?.fixture_id ?? entry.fixture_id),
  );
}

function buildS2Selection(m1CaseSpecs) {
  const excludedFixtureIds = s2FixtureIdsFromCaseSpecs(m1CaseSpecs);
  requireCondition(
    excludedFixtureIds.size === 57,
    `S2 m-1 fixture exclusion pool changed: ${excludedFixtureIds.size}/57`,
  );
  const selectedFixtureIds = new Set();
  const candidates = s2CandidateUniverse();
  const members = S2_FROZEN_MEMBERS.map((frozenMember) => {
    const selected = [];
    for (const candidate of candidates) {
      if (
        excludedFixtureIds.has(candidate.fixture_id) ||
        selectedFixtureIds.has(candidate.fixture_id)
      ) {
        continue;
      }
      if (!s2MemberMatches(frozenMember.member, candidate)) continue;
      selected.push(candidate);
      selectedFixtureIds.add(candidate.fixture_id);
      if (selected.length === 4) break;
    }
    requireCondition(
      selected.length === 4,
      `S2 ${frozenMember.member} selection changed: ${selected.length}/4`,
    );
    return { frozen: frozenMember, selected };
  });
  const selectedFixtureIdsInOrder = members.flatMap((entry) =>
    entry.selected.map((candidate) => candidate.fixture_id),
  );
  requireCondition(
    stableStringify(selectedFixtureIdsInOrder) ===
      stableStringify(S2_FROZEN_FIXTURE_IDS),
    "S2 fixture selection changed",
  );
  return { candidates, members };
}

function declarationClassificationRecord(relativePath, expectedCases, expectedFixtures) {
  const classification = readJson(relativePath);
  const selected = classification.cases.filter((entry) =>
    (entry.effective_profile?.rejected_when_effective ?? []).some(
      (option) => option.name === "declaration" && option.value === true,
    ),
  );
  requireCondition(selected.length === expectedCases, `${relativePath} declaration cases changed`);
  requireCondition(
    new Set(selected.map((entry) => entry.source)).size === expectedFixtures,
    `${relativePath} declaration fixture count changed`,
  );
  const summaryDeclaration = classification.summary.rejected_option_cases.find(
    (entry) => entry.name === "declaration",
  );
  requireCondition(
    summaryDeclaration?.cases === expectedCases,
    `${relativePath} declaration summary changed`,
  );
  return { cases: selected.length, unique_fixtures: expectedFixtures };
}

function validatePopulation() {
  const literal = {};
  for (const suite of ["compiler", "conformance"]) {
    let files = 0;
    let lines = 0;
    for (const absolute of walkSuiteFiles(suite)) {
      const text = fs.readFileSync(absolute, "utf8");
      const matches = text.match(/^\s*\/\/\s*@declaration\s*:\s*true\b/gim) ?? [];
      if (matches.length > 0) files += 1;
      lines += matches.length;
    }
    literal[suite] = { unique_files: files, directive_lines: lines };
  }
  requireCondition(
    stableStringify(literal) ===
      stableStringify({
        compiler: { unique_files: 889, directive_lines: 893 },
        conformance: { unique_files: 526, directive_lines: 527 },
      }),
    "literal declaration population changed",
  );

  const effective = {
    compiler: declarationClassificationRecord(
      COMPILER_CLASSIFICATION,
      1030,
      897,
    ),
    conformance: declarationClassificationRecord(
      CONFORMANCE_CLASSIFICATION,
      861,
      528,
    ),
    project: declarationClassificationRecord(PROJECT_CLASSIFICATION, 528, 264),
  };
  const transpile = readJson(TRANSPILE_INVENTORY);
  requireCondition(
    transpile.summary.declaration_cases === 21 &&
      transpile.summary.deferred_declaration_controls === 20 &&
      transpile.summary.deferred_declaration_map_controls === 1,
    "transpile declaration population changed",
  );
  effective.transpile = {
    cases: 21,
    declaration_controls: 20,
    declaration_map_controls: 1,
  };
  return {
    literal_directive_fixture_population: literal,
    effective_declaration_case_population: effective,
    selector: "F1-F14 use frozen fixture-name/input properties; S uses the signed W5 roster and revalidates its path-joined exact-but-for-declarations predicate",
  };
}

function validateFixtureUnits(source, fixture, decoded) {
  const parsed = makeUnits(decoded, source.path);
  requireCondition(
    stableStringify(parsed.links) === stableStringify(fixture.links),
    `${source.path} links changed`,
  );
  requireCondition(fixture.virtual_config === null, `${source.path} gained a virtual config`);
  requireCondition(
    parsed.units.length === fixture.normal_units.length,
    `${source.path} unit count changed`,
  );
  parsed.units.forEach((unit, index) => {
    const expected = fixture.normal_units[index];
    requireCondition(
      unit.name === expected.name &&
        stableStringify(unit.file_options) ===
          stableStringify(expected.file_options) &&
        stableStringify(contentIdentity(unit.text)) ===
          stableStringify(expected.content) &&
        stableStringify(documentSymlinks(unit.file_options)) ===
          stableStringify(expected.document_symlinks),
      `${source.path} unit ${index} changed`,
    );
  });
  return parsed.units;
}

function inputFileRecord(filePath, text) {
  const bytes = Buffer.from(text, "utf8");
  return {
    path: ts.normalizePath(filePath),
    sha256: sha256(bytes),
  };
}

function controlFile(filePath, text) {
  const bytes = Buffer.from(text, "utf8");
  return {
    path: ts.normalizePath(filePath),
    text_base64: bytes.toString("base64"),
    sha256: sha256(bytes),
    bytes: bytes.length,
  };
}

function applyFixtureMutation(options, mutation) {
  if (mutation === "fixture") return;
  if (mutation === "remove-declaration") {
    delete options.declaration;
    delete options.declarationMap;
    return;
  }
  if (mutation === "declaration-source-map") {
    options.declaration = true;
    options.sourceMap = true;
    delete options.declarationMap;
    return;
  }
  if (mutation === "declaration-commonjs") {
    options.declaration = true;
    options.module = ts.ModuleKind.CommonJS;
    delete options.sourceMap;
    delete options.declarationMap;
    return;
  }
  fail(`unknown fixture mutation ${mutation}`);
}

function expansionFixture(suite, fixturePath, expansions) {
  const expansion = suite === "compiler" ? expansions.compiler : expansions.conformance;
  const sourceIndices = expansion.sources
    .map((source, index) => ({ source, index }))
    .filter(({ source }) => source.path === fixturePath);
  requireCondition(
    sourceIndices.length === 1,
    `${suite}/${fixturePath} source identity is ambiguous`,
  );
  const { source, index: sourceIndex } = sourceIndices[0];
  if (suite === "compiler") {
    requireCondition(source.suite === "compiler", `${fixturePath} suite changed`);
  }
  const fixture =
    suite === "compiler"
      ? expansion.compiler_fixtures[sourceIndex]
      : expansion.fixtures.find((candidate) => candidate.source === sourceIndex);
  requireCondition(fixture !== undefined, `${fixturePath} expansion fixture is absent`);
  return { source, fixture };
}

function loadCuratedCase(family, selected, expansions, configurationIndex = 0) {
  const { source, fixture } = expansionFixture(
    family.suite,
    selected.fixture,
    expansions,
  );
  const absolute = safeSourcePath(family.suite, selected.fixture);
  const raw = fs.readFileSync(absolute);
  requireCondition(
    raw.length === source.bytes &&
      sha256(raw) === source.sha256 &&
      gitBlobSha1(raw) === source.git_blob_sha1,
    `${family.suite}/${selected.fixture} source identity changed`,
  );
  const decoded = ts.sys.readFile(absolute);
  requireCondition(typeof decoded === "string", `cannot decode ${selected.fixture}`);
  requireCondition(
    Buffer.byteLength(decoded, "utf8") === fixture.decoded_utf8_bytes &&
      sha256(Buffer.from(decoded, "utf8")) === fixture.decoded_sha256,
    `${selected.fixture} decoded identity changed`,
  );
  const expectsDeclarationDirective = selected.fixture !== F13_TINY_CONTROL;
  requireCondition(
    hasDeclarationDirective(decoded) === expectsDeclarationDirective,
    `${selected.fixture} declaration-directive expectation changed`,
  );
  const units = validateFixtureUnits(source, fixture, decoded);
  requireCondition(fixture.links.length === 0, `${selected.fixture} has unsupported global links`);
  // Matrix-bearing fixtures use the upstream expansion's first/default arm;
  // no observed output participates in this choice.
  const configuration = fixture.configurations[configurationIndex];
  requireCondition(
    configuration !== undefined,
    `${selected.fixture} configuration ${configurationIndex} is absent`,
  );
  const settings = mergedSettings(fixture.settings, configuration.settings);
  const options = effectiveCompilerOptions(settings);
  applyFixtureMutation(options, selected.mutation);
  requireCondition(
    selected.roles.includes("adjacent-negative-control")
      ? options.declaration !== true
      : options.declaration === true,
    `${selected.fixture} effective declaration option disagrees with its role`,
  );
  const selection = explicitRootSelection(units, settings, options);
  const cwd = currentDirectory(settings);
  const files = selection.vfs_write_order.map((id) => {
    const unit = units[id];
    return controlFile(ts.getNormalizedAbsolutePath(unit.name, cwd), unit.text);
  });
  const symlinks = [];
  for (const id of selection.vfs_write_order) {
    const unit = units[id];
    const target = ts.getNormalizedAbsolutePath(unit.name, cwd);
    for (const rawLink of documentSymlinks(unit.file_options)) {
      symlinks.push({
        link_path: ts.getNormalizedAbsolutePath(rawLink, cwd),
        target_path: target,
      });
    }
  }
  const roots = selection.program_root_unit_ids.map((id) =>
    ts.getNormalizedAbsolutePath(units[id].name, cwd),
  );
  requireCondition(roots.length > 0, `${selected.fixture} has no program root`);
  const caseId = `h2-7a/${family.family_id}/${selected.slug}`;
  const control = {
    current_directory: cwd,
    roots,
    files,
    symlinks,
    compiler_options: serializeOptions(options),
    default_library: "compiler-host",
  };
  return {
    case_id: caseId,
    family_id: family.family_id,
    roles: selected.roles,
    lanes: family.lanes,
    fixture_source: source,
    expected_observation: null,
    control,
    manifest: {
      case_id: caseId,
      suite: family.suite,
      fixture_id: `typescript-6.0.3/${family.suite}/${selected.fixture}`,
      matrix: {
        configuration_index: configurationIndex,
        fixture_variant: configuration.variant,
        observation_variant: selected.mutation,
      },
      family_id: family.family_id,
      roles: selected.roles,
      lanes: family.lanes,
      option_record: serializeOptions(options),
      input_files: files
        .map((entry) => ({ path: entry.path, sha256: entry.sha256 }))
        .sort((left, right) =>
          compareBytes(left.path, right.path) || compareBytes(left.sha256, right.sha256),
        ),
    },
  };
}

function readDivergencePool() {
  const divergence = readJson(H2_6C_DIVERGENCES);
  requireCondition(
    divergence.schema === 1 &&
      Array.isArray(divergence.cases) &&
      divergence.cases.length === 451,
    "invalid H2.6c divergence pool",
  );
  const nonRefused = divergence.cases
    .filter((entry) => entry.emit_refused === false)
    .map((entry) => entry.case_id);
  requireCondition(
    nonRefused.length === 172 && new Set(nonRefused).size === 172,
    "H2.6c non-refused pool changed",
  );
  return nonRefused;
}

function ensureStratumCensus(nonRefusedIds) {
  const configured = process.env[STRATUM_CENSUS_ENV];
  if (configured !== undefined) {
    const absolute = path.resolve(WORKSPACE, configured);
    requireCondition(fs.existsSync(absolute), `${STRATUM_CENSUS_ENV} is absent`);
    return absolute;
  }
  const cached = path.join(WORKSPACE, DEFAULT_CENSUS_RELATIVE_PATH);
  if (fs.existsSync(cached)) return cached;

  const outputDirectory = path.join(WORKSPACE, DEFAULT_CENSUS_DIRECTORY);
  fs.mkdirSync(outputDirectory, { recursive: true });
  const listPath = path.join(outputDirectory, "non-refused-case-ids.txt");
  writeFileAtomic(listPath, `${nonRefusedIds.join("\n")}\n`);
  process.stderr.write(
    "W-H2.7A stratum census: no precomputed JSONL; building/running the checked-in Rust census instrument\n",
  );
  execFileSync(
    "cargo",
    [
      "test",
      "-p",
      "tsc-rs-compiler",
      "--test",
      "contracts",
      "probe_one_band_row",
      "--",
      "--ignored",
      "--nocapture",
    ],
    {
      cwd: WORKSPACE,
      env: {
        ...process.env,
        TSRS_H2_6C_PROBE_LIST: listPath,
        TSRS_H2_6C_PROBE_OUT: outputDirectory,
      },
      stdio: ["ignore", "inherit", "inherit"],
      maxBuffer: 128 * 1024 * 1024,
    },
  );
  requireCondition(fs.existsSync(cached), "Rust census did not produce census.jsonl");
  return cached;
}

function parseCensusJsonl(absolutePath, expectedIds) {
  const bytes = fs.readFileSync(absolutePath);
  const text = bytes.toString("utf8");
  const rows = text
    .split(/\r?\n/)
    .filter((line) => line.trim() !== "")
    .map((line, index) => {
      try {
        return JSON.parse(line);
      } catch (error) {
        fail(`invalid census JSON line ${index + 1}: ${error.message}`);
      }
    });
  requireCondition(rows.length === 172, `stratum census has ${rows.length}/172 rows`);
  requireCondition(
    new Set(rows.map((entry) => entry.case_id)).size === rows.length,
    "stratum census case IDs are not unique",
  );
  requireCondition(
    stableStringify(rows.map((entry) => entry.case_id).sort(compareBytes)) ===
      stableStringify([...expectedIds].sort(compareBytes)),
    "stratum census does not cover the exact non-refused pool",
  );
  return { rows, sha256: sha256(bytes) };
}

function exactButForDeclarations(row, qualificationCase) {
  if (
    !Array.isArray(row.writes_diverging) ||
    row.writes_diverging.length !== 0 ||
    !Array.isArray(row.writes_rust_only) ||
    row.writes_rust_only.length !== 0 ||
    !Array.isArray(row.writes_missing) ||
    row.writes_missing.length === 0 ||
    !row.writes_missing.every(
      (write) => write.kind === "declaration" && write.path.endsWith(".d.ts"),
    ) ||
    row.reported_diagnostics?.rust !== row.reported_diagnostics?.expected
  ) {
    return false;
  }
  const expected = qualificationCase.typescript_observation;
  const expectedSourceMaps = expected.emit_result.source_maps?.length ?? 0;
  return (
    row.emit_skipped === expected.emit_result.emit_skipped &&
    row.source_maps_count === expectedSourceMaps &&
    row.writes_exact + row.writes_missing.length === expected.writes.length
  );
}

// W5 §2 froze 64 project root-shape rows plus three compiler rows.  Later W5
// fixes make additional rows satisfy the byte predicate, so the signed roster
// is applied first and every roster member is then re-proven by the current
// census.  This is a shrink-only checkpoint, not output-driven reselection.
function isFrozenW5StratumCase(caseId) {
  if (
    caseId === "typescript-6.0.3/compiler/properties.ts#target%3Des5" ||
    caseId === "typescript-6.0.3/compiler/properties.ts#target%3Des2015" ||
    caseId === "typescript-6.0.3/compiler/out-flag.ts#target%3Des2015"
  ) {
    return true;
  }
  const match =
    /^typescript-6\.0\.3\/project\/([^#]+)\.json#module%3D(?:amd|commonjs)$/.exec(
      caseId,
    );
  if (match === null) return false;
  const descriptor = match[1];
  return (
    !descriptor.includes("Mixed") &&
    !descriptor.includes("Module") &&
    /(SingleFile|Simple|Subfolder|Multifolder)NoOutdir$/.test(descriptor)
  );
}

function projectEffectiveOptions(descriptor, variant) {
  const options = {
    moduleResolution: ts.ModuleResolutionKind.Classic,
    noErrorTruncation: false,
    skipDefaultLibCheck: false,
    newLine: ts.NewLineKind.CarriageReturnLineFeed,
  };
  for (const [key, raw] of Object.entries(descriptor)) {
    if (PROJECT_STRUCTURAL_KEYS.has(key)) continue;
    const option = OPTION_INDEX.get(key.toLowerCase());
    requireCondition(
      option !== undefined,
      `project descriptor option ${key} is not a compiler option`,
    );
    options[option.name] = optionValue(option, String(raw));
  }
  options.module = variant.value;
  delete options.noEmit;
  return options;
}

function loadProjectStratumControl(qualificationCase) {
  const descriptorPath = qualificationCase.source.path;
  const descriptorAbsolute = safeSourcePath("project", descriptorPath);
  const raw = fs.readFileSync(descriptorAbsolute);
  requireCondition(
    raw.length === qualificationCase.source.bytes &&
      sha256(raw) === qualificationCase.source.sha256 &&
      gitBlobSha1(raw) === qualificationCase.source.git_blob_sha1,
    `${qualificationCase.case_id} project descriptor identity changed`,
  );
  const descriptor = JSON.parse(raw.toString("utf8"));
  for (const key of Object.keys(descriptor)) {
    requireCondition(
      PROJECT_STRUCTURAL_KEYS.has(key) || OPTION_INDEX.has(key.toLowerCase()),
      `${qualificationCase.case_id} project descriptor has unknown key ${key}`,
    );
  }
  const projectInput = qualificationCase.project_input;
  requireCondition(
    projectInput.root_selection.state === "explicit-inputs",
    `${qualificationCase.case_id} stratum project is no longer explicit-inputs`,
  );
  const variant = PROJECT_MODULE_VARIANTS[projectInput.module_variant.name];
  requireCondition(
    variant !== undefined && variant.value === projectInput.module_variant.value,
    `${qualificationCase.case_id} project module variant changed`,
  );
  const files = projectInput.analyzed_files.map((file) => {
    requireCondition(
      file.path.startsWith(`${PROJECT_VIRTUAL_PREFIX}/`),
      `${qualificationCase.case_id} project input escaped the mount`,
    );
    const relative = file.path.slice(PROJECT_VIRTUAL_PREFIX.length + 1);
    const text = ts.sys.readFile(safeSourcePath("projects", relative));
    requireCondition(typeof text === "string", `cannot decode project input ${relative}`);
    const record = controlFile(file.path, text);
    requireCondition(
      record.sha256 === file.text_sha256,
      `${qualificationCase.case_id} project input ${file.path} changed`,
    );
    return record;
  });
  const roots = projectInput.root_selection.roots
    .filter((root) => root.present)
    .map((root) => root.path);
  requireCondition(roots.length > 0, `${qualificationCase.case_id} has no project root`);
  const options = projectEffectiveOptions(descriptor, variant);
  return {
    current_directory: projectInput.current_directory,
    roots,
    files,
    symlinks: [],
    compiler_options: serializeOptions(options),
    default_library: "project-es5",
  };
}

function loadCompilerStratumControl(qualificationCase) {
  const input = qualificationCase.input;
  requireCondition(input.virtual_config === null, `${qualificationCase.case_id} gained a virtual config`);
  const settings = new Map(input.settings.map((entry) => [entry.name, entry.value]));
  const options = effectiveCompilerOptions(settings);
  const files = input.files.map((file) => {
    const bytes = Buffer.from(file.utf8_base64, "base64");
    requireCondition(
      bytes.length === file.utf8_bytes && sha256(bytes) === file.utf8_sha256,
      `${qualificationCase.case_id} embedded input ${file.path} changed`,
    );
    return {
      path: file.path,
      text_base64: file.utf8_base64,
      sha256: file.utf8_sha256,
      bytes: file.utf8_bytes,
    };
  });
  return {
    current_directory: input.current_directory,
    roots: input.roots,
    files,
    symlinks: input.vfs_symlinks,
    compiler_options: serializeOptions(options),
    default_library: "compiler-host",
  };
}

function normalizeExpectedObservation(expected) {
  const observation = {
    reported_diagnostics: expected.reported_diagnostics,
    writes: expected.writes.map((write) => {
      const declaration = write.kind === "declaration";
      return {
        index: write.index,
        path: write.path,
        kind: write.kind,
        callback_utf8_sha256: write.callback_utf8_sha256,
        callback_utf8_bytes: write.callback_utf8_bytes,
        write_byte_order_mark: write.write_byte_order_mark,
        materialized_utf8_sha256: write.materialized_utf8_sha256,
        materialized_utf8_bytes: write.materialized_utf8_bytes,
        declaration_callback_base64: declaration
          ? write.callback_utf8_base64
          : null,
        declaration_materialized_base64: declaration
          ? write.materialized_utf8_base64
          : null,
        source_files: write.source_files,
      };
    }),
    emit_result: {
      emit_skipped: expected.emit_result.emit_skipped,
      emitted_files: expected.emit_result.emitted_files,
      diagnostics: expected.emit_result.diagnostics,
    },
  };
  return withFingerprint(observation, "observation_fingerprint_sha256");
}

function loadStratumCases() {
  const nonRefusedIds = readDivergencePool();
  const censusPath = ensureStratumCensus(nonRefusedIds);
  const measured = parseCensusJsonl(censusPath, nonRefusedIds);
  const census = readJson(H2_6C_CENSUS);
  const qualification = readJson(H2_6C_QUALIFICATION);
  requireCondition(
    census.schema === 1 && Array.isArray(census.cases),
    "invalid H2.6c census artifact",
  );
  requireCondition(
    qualification.schema === 1 &&
      qualification.phase === "H2.6c-map-observation" &&
      Array.isArray(qualification.cases),
    "invalid H2.6c qualification artifact",
  );
  const censusById = new Map(census.cases.map((entry) => [entry.case_id, entry]));
  const qualificationById = new Map(
    qualification.cases.map((entry) => [entry.case_id, entry]),
  );
  const measuredById = new Map(measured.rows.map((entry) => [entry.case_id, entry]));

  const frozenIds = nonRefusedIds.filter(isFrozenW5StratumCase).sort(compareBytes);
  requireCondition(
    frozenIds.length === 67 && new Set(frozenIds).size === 67,
    `signed W5 stratum roster changed: ${frozenIds.length}/67`,
  );
  const cases = frozenIds.map((caseId) => {
    const row = censusById.get(caseId);
    const qualificationCase = qualificationById.get(caseId);
    const measuredRow = measuredById.get(caseId);
    requireCondition(
      row?.positive === true && qualificationCase !== undefined && measuredRow !== undefined,
      `${caseId} is missing from a 6c authority`,
    );
    requireCondition(
      row.fixture_id === qualificationCase.fixture_id &&
        row.matrix_key === qualificationCase.matrix_key &&
        row.suite === qualificationCase.suite,
      `${caseId} 6c census/qualification identity changed`,
    );
    requireCondition(
      exactButForDeclarations(measuredRow, qualificationCase),
      `${caseId} no longer satisfies exact-but-for-declarations`,
    );
    const control =
      row.suite === "project"
        ? loadProjectStratumControl(qualificationCase)
        : loadCompilerStratumControl(qualificationCase);
    requireCondition(
      control.compiler_options.declaration === true,
      `${caseId} stratum declaration option is not true`,
    );
    const inputFiles = control.files
      .map((entry) => ({ path: entry.path, sha256: entry.sha256 }))
      .sort((left, right) =>
        compareBytes(left.path, right.path) || compareBytes(left.sha256, right.sha256),
      );
    return {
      case_id: caseId,
      family_id: "S",
      roles: ["positive"],
      lanes: ["printer grammar/shape"],
      fixture_source: row.source,
      expected_observation: normalizeExpectedObservation(
        qualificationCase.typescript_observation,
      ),
      control,
      manifest: {
        case_id: caseId,
        suite: row.suite,
        fixture_id: row.fixture_id,
        matrix: {
          configuration_index: row.configuration_index ?? null,
          fixture_variant: row.configuration_variant ?? row.matrix_key,
          observation_variant: "h2-6c-exact-but-for-declarations",
        },
        family_id: "S",
        roles: ["positive"],
        lanes: ["printer grammar/shape"],
        option_record: serializeOptions(control.compiler_options),
        input_files: inputFiles,
      },
    };
  });
  const projectCount = cases.filter((entry) => entry.manifest.suite === "project").length;
  const compilerCount = cases.filter((entry) => entry.manifest.suite === "compiler").length;
  requireCondition(
    projectCount === 64 && compilerCount === 3,
    `stratum suite split changed: project=${projectCount} compiler=${compilerCount}`,
  );
  return {
    cases,
    section: {
      stratum_id: "S",
      description: "signed W5 exact-but-for-declarations stratum",
      count: 67,
      source_pool_rows: 172,
      compiler_cases: 3,
      project_cases: 64,
      selection_contract: "signed W5 §2 roster; each row must have byte-exact matched JS/map writes, no rust-only writes, no missing non-declaration writes, no diagnostic-count delta, and only absent .d.ts writes",
      census_jsonl_sha256: measured.sha256,
      case_ids: frozenIds,
    },
    censusPath,
  };
}

function buildCuratedCases(expansions) {
  const cases = [];
  for (const family of FAMILY_SPECS) {
    for (const selected of family.cases) {
      cases.push(loadCuratedCase(family, selected, expansions));
    }
  }
  requireCondition(cases.length === 27, `curated case count changed: ${cases.length}/27`);
  requireCondition(
    new Set(cases.map((entry) => entry.case_id)).size === cases.length,
    "curated case IDs collide",
  );
  return cases;
}

function declarationEffectiveConfigurationIndexes(suite, fixturePath, expansions) {
  const { fixture } = expansionFixture(suite, fixturePath, expansions);
  requireCondition(
    Array.isArray(fixture.configurations) && fixture.configurations.length > 0,
    `S2 ${fixturePath} has no configurations`,
  );
  const indexes = fixture.configurations
    .map((configuration, index) => ({ configuration, index }))
    .filter(({ configuration }) => {
      const settings = mergedSettings(fixture.settings, configuration.settings);
      return effectiveCompilerOptions(settings).declaration === true;
    })
    .map(({ index }) => index);
  return { configuration_count: fixture.configurations.length, indexes };
}

function buildS2Supplement(m1CaseSpecs, expansions) {
  const selection = buildS2Selection(m1CaseSpecs);
  const cases = [];
  const trimRows = [];
  for (const { frozen, selected } of selection.members) {
    for (const [fixtureOffset, candidate] of selected.entries()) {
      const configurationSelection = declarationEffectiveConfigurationIndexes(
        candidate.suite,
        candidate.relative_path,
        expansions,
      );
      const configurationIndexes = configurationSelection.indexes;
      requireCondition(
        configurationIndexes.length > 0,
        `S2 ${candidate.fixture_id} has no declaration-effective configuration`,
      );
      const retainedConfigurationIndexes = configurationIndexes.slice(0, 2);
      const trimmedConfigurationIndexes = configurationIndexes.slice(2);
      if (trimmedConfigurationIndexes.length > 0) {
        trimRows.push({
          fixture_id: candidate.fixture_id,
          trimmed_configuration_indexes: trimmedConfigurationIndexes,
        });
      }
      const family = {
        family_id: "S2",
        suite: candidate.suite,
        lanes: frozen.lanes,
      };
      for (const configurationIndex of retainedConfigurationIndexes) {
        const suffix =
          configurationSelection.configuration_count > 1
            ? `-c${configurationIndex}`
            : "";
        cases.push(
          loadCuratedCase(
            family,
            {
              slug: `${frozen.member}-${fixtureOffset + 1}${suffix}`,
              fixture: candidate.relative_path,
              roles: ["supplement"],
              mutation: "fixture",
            },
            expansions,
            configurationIndex,
          ),
        );
      }
    }
  }
  const caseIds = cases.map((entry) => entry.case_id);
  const selectedFixtureIdsByMember = selection.members.map((entry) =>
    entry.selected.map((candidate) => candidate.fixture_id),
  );
  requireCondition(
    stableStringify(selectedFixtureIdsByMember) ===
      stableStringify(S2_FROZEN_MEMBERS.map((member) => member.fixture_ids)),
    "S2 member fixture selection changed",
  );
  requireCondition(
    stableStringify(caseIds) === stableStringify(S2_FROZEN_CASE_IDS),
    "S2 case expansion changed",
  );
  requireCondition(
    stableStringify(trimRows) === stableStringify(S2_FROZEN_TRIM_ROWS),
    "S2 configuration trim changed",
  );
  requireCondition(
    cases.length === 18 && new Set(cases.map((entry) => entry.manifest.fixture_id)).size === 16,
    "S2 case volume changed",
  );
  return {
    cases,
    selection,
    trimRows,
    supplement: {
      selector_version: S2_SELECTOR_VERSION,
      volume_table: S2_VOLUME_TABLE.map((entry) => ({ ...entry })),
      members: S2_FROZEN_MEMBERS.map(({ member, predicate, fixture_ids }) => ({
        member,
        predicate,
        fixture_ids: [...fixture_ids],
      })),
      case_ids: caseIds,
      trim_rows: trimRows,
      counts: {
        fixtures: 16,
        cases: 18,
        observations: 36,
      },
    },
  };
}

function buildCaseManifest(caseSpecs) {
  const cases = caseSpecs
    .map((entry) => entry.manifest)
    .sort((left, right) => compareBytes(left.case_id, right.case_id));
  const universe = new Map();
  for (const manifestCase of cases) {
    for (const input of manifestCase.input_files) {
      universe.set(`${input.path}\0${input.sha256}`, input);
    }
  }
  const sourceUniverse = [...universe.values()].sort((left, right) =>
    compareBytes(left.path, right.path) || compareBytes(left.sha256, right.sha256),
  );
  const sourceUniverseSha256 = sha256(
    Buffer.from(stableStringify(sourceUniverse), "utf8"),
  );
  const payload = {
    cases,
    source_universe: sourceUniverse,
    source_universe_sha256: sourceUniverseSha256,
  };
  return {
    ...payload,
    case_manifest_fingerprint: sha256(
      Buffer.from(stableStringify(payload), "utf8"),
    ),
  };
}

function buildCoverageMatrix(caseSpecs) {
  requireCondition(
    caseSpecs.every((entry) => entry.family_id !== "S2"),
    "m-1 coverage projection includes S2 cases",
  );
  const familyRows = FAMILY_SPECS.map((family) => ({
    family_id: family.family_id,
    description: family.description,
    lanes: family.lanes,
    case_ids: caseSpecs
      .filter((entry) => entry.family_id === family.family_id)
      .map((entry) => entry.case_id)
      .sort(compareBytes),
  }));
  familyRows.push({
    family_id: "S",
    description: "signed W5 exact-but-for-declarations stratum",
    lanes: ["printer grammar/shape"],
    case_ids: caseSpecs
      .filter((entry) => entry.family_id === "S")
      .map((entry) => entry.case_id)
      .sort(compareBytes),
  });
  requireCondition(
    familyRows.length === 15 &&
      familyRows.slice(0, 14).every((entry, index) => entry.family_id === `F${index + 1}`),
    "family matrix order changed",
  );
  const laneCoverage = LANES.map((lane) => ({
    lane,
    families: familyRows
      .filter((family) => family.lanes.includes(lane))
      .map((family) => family.family_id),
  }));
  for (const coverage of laneCoverage) {
    requireCondition(
      coverage.families.length > 0,
      `uncovered W-H2.7A lane ${coverage.lane}`,
    );
  }
  for (const spec of caseSpecs) {
    requireCondition(
      spec.roles.length > 0 && spec.roles.every((role) => ROLES.includes(role)),
      `${spec.case_id} has an invalid role`,
    );
    requireCondition(
      spec.lanes.length > 0 && spec.lanes.every((lane) => LANES.includes(lane)),
      `${spec.case_id} has an invalid lane`,
    );
  }
  return { lanes: LANES, families: familyRows, lane_coverage: laneCoverage };
}

function buildQuotas(caseSpecs) {
  requireCondition(
    caseSpecs.every((entry) => entry.family_id !== "S2"),
    "m-1 quota projection includes S2 cases",
  );
  const count = (role) => caseSpecs.filter((entry) => entry.roles.includes(role)).length;
  const quotas = {
    positive_cases: count("positive"),
    adjacent_negative_cases: count("adjacent-negative-control"),
    composition_cases: count("composition"),
    fault_cases: count("fault"),
  };
  for (const [name, minimum] of Object.entries(QUOTA_MINIMUMS)) {
    requireCondition(quotas[name] >= minimum, `${name} quota failed`);
  }
  requireCondition(
    Object.values(quotas).reduce((sum, value) => sum + value, 0) === caseSpecs.length,
    "case roles do not form a single-role partition",
  );
  return quotas;
}

function canonicalM1Projection(caseManifestCases, observations, stratum) {
  const cases = caseManifestCases
    .filter((entry) => entry.family_id !== "S2")
    .sort((left, right) => compareBytes(left.case_id, right.case_id));
  const caseIds = new Set(cases.map((entry) => entry.case_id));
  return {
    cases,
    observations: observations
      .filter((entry) => caseIds.has(entry.case_id))
      .sort((left, right) => compareBytes(left.case_id, right.case_id)),
    stratum,
  };
}

function verifyM1Projection(caseManifestCases, observations, stratum, source) {
  const projection = canonicalM1Projection(
    caseManifestCases,
    observations,
    stratum,
  );
  requireCondition(
    projection.cases.length === 94 && projection.observations.length === 94,
    `m-1 projection guard input changed (${source})`,
  );
  const actual = sha256(Buffer.from(stableStringify(projection), "utf8"));
  requireCondition(
    actual === H2_7A_M1_PROJECTION_SHA256,
    `m-1 projection guard failed (${source}): ${actual}`,
  );
  return projection;
}

function prepareStaticContext() {
  validateRuntime();
  validateFrozenFixtureChoices();
  const population = validatePopulation();
  const expansions = {
    compiler: readJson(TEST_SUITE_EXPANSION),
    conformance: readJson(CONFORMANCE_EXPANSION),
  };
  const curatedCases = buildCuratedCases(expansions);
  const stratum = loadStratumCases();
  const m1CaseSpecs = [...curatedCases, ...stratum.cases].sort((left, right) =>
    compareBytes(left.case_id, right.case_id),
  );
  requireCondition(
    m1CaseSpecs.length === 94 &&
      new Set(m1CaseSpecs.map((entry) => entry.case_id)).size === 94,
    "W-H2.7A case denominator changed",
  );
  const m1CaseManifest = buildCaseManifest(m1CaseSpecs);
  const coverageMatrix = buildCoverageMatrix(m1CaseSpecs);
  const quotas = buildQuotas(m1CaseSpecs);
  const s2 = buildS2Supplement(m1CaseSpecs, expansions);
  const caseSpecs = [...m1CaseSpecs, ...s2.cases].sort((left, right) =>
    compareBytes(left.case_id, right.case_id),
  );
  requireCondition(
    caseSpecs.length === 112 &&
      new Set(caseSpecs.map((entry) => entry.case_id)).size === 112,
    "W-H2.7A successor case denominator changed",
  );
  const caseManifest = buildCaseManifest(caseSpecs);
  return {
    caseSpecs,
    m1CaseSpecs,
    s2CaseSpecs: s2.cases,
    m1CaseManifest,
    caseManifest,
    coverageMatrix,
    quotas,
    m2Supplement: s2.supplement,
    population,
    stratum: stratum.section,
    censusPath: stratum.censusPath,
    typescript: typescriptRecord(),
    inputs: {
      packet: pathHash(PACKET_RELATIVE_PATH),
      parent_packet: pathHash(PARENT_PACKET_RELATIVE_PATH),
      test_suite_expansion: pathHash(TEST_SUITE_EXPANSION),
      conformance_expansion: pathHash(CONFORMANCE_EXPANSION),
      compiler_classification: pathHash(COMPILER_CLASSIFICATION),
      conformance_classification: pathHash(CONFORMANCE_CLASSIFICATION),
      project_classification: pathHash(PROJECT_CLASSIFICATION),
      transpile_inventory: pathHash(TRANSPILE_INVENTORY),
      h2_6c_census: pathHash(H2_6C_CENSUS),
      h2_6c_qualification: pathHash(H2_6C_QUALIFICATION),
      h2_6c_divergences: pathHash(H2_6C_DIVERGENCES),
      vfs_directory_overlay: pathHash(VFS_DIRECTORY_OVERLAY),
    },
  };
}

function createVirtualProgram(control) {
  const files = new Map();
  for (const file of control.files) {
    const bytes = Buffer.from(file.text_base64, "base64");
    requireCondition(
      bytes.length === file.bytes && sha256(bytes) === file.sha256,
      `observation control input ${file.path} changed`,
    );
    files.set(ts.normalizePath(file.path), {
      text: bytes.toString("utf8"),
    });
  }
  const symlinks = new Map(
    control.symlinks.map((entry) => [
      ts.normalizePath(entry.link_path),
      ts.normalizePath(entry.target_path),
    ]),
  );
  for (const [link, target] of symlinks) {
    const source = files.get(target);
    requireCondition(source !== undefined, `symlink target ${target} is absent`);
    if (!files.has(link)) files.set(link, source);
  }
  const options = control.compiler_options;
  const baseHost = ts.createCompilerHost(options, true);
  const directoryOverlay = createHermeticDirectoryOverlay(files.keys(), {
    currentDirectory: control.current_directory,
    useCaseSensitiveFileNames: true,
    fallbackHost: baseHost,
  });
  const defaultLibraryFileName = ts.combinePaths(
    baseHost.getDefaultLibLocation(),
    "lib.es5.d.ts",
  );
  const host = {
    ...baseHost,
    getCurrentDirectory: () => control.current_directory,
    useCaseSensitiveFileNames: () => true,
    getCanonicalFileName: (fileName) => fileName,
    trace() {},
    fileExists(fileName) {
      const normalized = ts.normalizePath(fileName);
      return files.has(normalized) || baseHost.fileExists(normalized);
    },
    readFile(fileName) {
      const normalized = ts.normalizePath(fileName);
      return files.get(normalized)?.text ?? baseHost.readFile(normalized);
    },
    directoryExists(directory) {
      return directoryOverlay.directoryExists(directory);
    },
    getDirectories(directory) {
      return directoryOverlay.getDirectories(directory);
    },
    realpath(fileName) {
      const normalized = ts.normalizePath(fileName);
      if (symlinks.has(normalized)) return symlinks.get(normalized);
      return files.has(normalized)
        ? normalized
        : (baseHost.realpath?.(normalized) ?? normalized);
    },
    getSourceFile(fileName, languageVersion) {
      const normalized = ts.normalizePath(fileName);
      const mounted = files.get(normalized);
      if (mounted === undefined) {
        return baseHost.getSourceFile(fileName, languageVersion);
      }
      return ts.createSourceFile(
        normalized,
        mounted.text,
        languageVersion,
        true,
        ts.getScriptKindFromFileName(normalized),
      );
    },
  };
  if (control.default_library === "project-es5") {
    host.getDefaultLibFileName = () => defaultLibraryFileName;
  } else {
    requireCondition(
      control.default_library === "compiler-host",
      `unknown default-library control ${control.default_library}`,
    );
  }
  return ts.createProgram(control.roots, options, host);
}

function serializeDiagnostic(diagnostic) {
  return {
    code: diagnostic.code,
    category: ts.DiagnosticCategory[diagnostic.category],
    message: ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n"),
    file: diagnostic.file?.fileName ?? null,
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
  };
}

function outputKind(fileName) {
  const lower = fileName.toLowerCase();
  if (
    lower.endsWith(".d.ts") ||
    lower.endsWith(".d.mts") ||
    lower.endsWith(".d.cts")
  ) {
    return "declaration";
  }
  if (lower.endsWith(".map")) return "source-map";
  if (
    lower.endsWith(".js") ||
    lower.endsWith(".jsx") ||
    lower.endsWith(".mjs") ||
    lower.endsWith(".cjs")
  ) {
    return "javascript";
  }
  return "other";
}

function serializeWrite(arguments_, index) {
  const [fileName, text, writeByteOrderMark, , sourceFiles] = arguments_;
  const callback = Buffer.from(text, "utf8");
  const materialized = writeByteOrderMark
    ? Buffer.concat([UTF8_BOM, callback])
    : callback;
  const kind = outputKind(fileName);
  const declaration = kind === "declaration";
  return {
    index,
    path: ts.normalizePath(fileName),
    kind,
    callback_utf8_sha256: sha256(callback),
    callback_utf8_bytes: callback.length,
    write_byte_order_mark: writeByteOrderMark,
    materialized_utf8_sha256: sha256(materialized),
    materialized_utf8_bytes: materialized.length,
    declaration_callback_base64: declaration
      ? callback.toString("base64")
      : null,
    declaration_materialized_base64: declaration
      ? materialized.toString("base64")
      : null,
    source_files: (sourceFiles ?? []).map((source) =>
      ts.normalizePath(source.fileName),
    ),
  };
}

function validateObservation(observation, caseId) {
  requireCondition(
    fingerprintIsValid(observation, "observation_fingerprint_sha256"),
    `${caseId} observation fingerprint is invalid`,
  );
  requireCondition(
    Array.isArray(observation.reported_diagnostics) &&
      Array.isArray(observation.writes) &&
      typeof observation.emit_result?.emit_skipped === "boolean" &&
      Array.isArray(observation.emit_result.diagnostics),
    `${caseId} observation shape is invalid`,
  );
  observation.writes.forEach((write, index) => {
    requireCondition(
      write.index === index &&
        typeof write.path === "string" &&
        typeof write.callback_utf8_sha256 === "string" &&
        typeof write.callback_utf8_bytes === "number" &&
        typeof write.write_byte_order_mark === "boolean" &&
        typeof write.materialized_utf8_sha256 === "string" &&
        typeof write.materialized_utf8_bytes === "number",
      `${caseId} write ${index} shape is invalid`,
    );
    if (write.kind === "declaration") {
      const callback = Buffer.from(write.declaration_callback_base64, "base64");
      const materialized = Buffer.from(
        write.declaration_materialized_base64,
        "base64",
      );
      requireCondition(
        callback.length === write.callback_utf8_bytes &&
          sha256(callback) === write.callback_utf8_sha256 &&
          materialized.length === write.materialized_utf8_bytes &&
          sha256(materialized) === write.materialized_utf8_sha256 &&
          Buffer.compare(
            materialized,
            write.write_byte_order_mark
              ? Buffer.concat([UTF8_BOM, callback])
              : callback,
          ) === 0,
        `${caseId} declaration write ${write.path} byte embedding is invalid`,
      );
    } else {
      requireCondition(
        write.declaration_callback_base64 === null &&
          write.declaration_materialized_base64 === null,
        `${caseId} non-declaration write ${write.path} embeds bytes`,
      );
    }
  });
}

function observeControl(control) {
  const program = createVirtualProgram(control);
  const writes = [];
  const reportedDiagnostics = [];
  let emitResult;
  const originalEmit = program.emit;
  program.emit = function captureEmit(...arguments_) {
    requireCondition(emitResult === undefined, "TypeScript emitted more than once");
    emitResult = originalEmit.apply(this, arguments_);
    return emitResult;
  };
  ts.emitFilesAndReportErrorsAndGetExitStatus(
    program,
    (diagnostic) => reportedDiagnostics.push(diagnostic),
    () => {},
    undefined,
    (...arguments_) => writes.push(arguments_),
  );
  requireCondition(emitResult !== undefined, "TypeScript did not call Program.emit");
  return withFingerprint(
    {
      reported_diagnostics: reportedDiagnostics.map(serializeDiagnostic),
      writes: writes.map(serializeWrite),
      emit_result: {
        emit_skipped: emitResult.emitSkipped,
        emitted_files:
          emitResult.emittedFiles === undefined
            ? null
            : emitResult.emittedFiles.map((fileName) => ts.normalizePath(fileName)),
        diagnostics: emitResult.diagnostics.map(serializeDiagnostic),
      },
    },
    "observation_fingerprint_sha256",
  );
}

function readInternalObservationControl(caseId) {
  const configured = process.env[CONTROL_FILE_ENV];
  requireCondition(configured !== undefined, `${CONTROL_FILE_ENV} is required`);
  const record = JSON.parse(fs.readFileSync(configured, "utf8"));
  requireCondition(
    record.schema === 1 &&
      record.case_id === caseId &&
      fingerprintIsValid(record, "control_fingerprint_sha256"),
    `${caseId} observation control is invalid`,
  );
  return record.control;
}

function observeCaseInFreshProcess(caseSpec) {
  const controlRecord = withFingerprint(
    { schema: 1, case_id: caseSpec.case_id, control: caseSpec.control },
    "control_fingerprint_sha256",
  );
  const controlPath = path.join(WORKSPACE, CONTROL_RELATIVE_PATH);
  writeFileAtomic(controlPath, render(controlRecord));
  const stdout = execFileSync(
    process.execPath,
    [GENERATOR_PATH, INTERNAL_OBSERVE_MODE, caseSpec.case_id],
    {
      cwd: WORKSPACE,
      encoding: "utf8",
      env: { ...process.env, [CONTROL_FILE_ENV]: controlPath },
      maxBuffer: 128 * 1024 * 1024,
    },
  );
  const observation = JSON.parse(stdout);
  validateObservation(observation, caseSpec.case_id);
  return observation;
}

function observeAllCases(caseSpecs) {
  const observations = [];
  let completed = 0;
  for (const caseSpec of caseSpecs) {
    const first = observeCaseInFreshProcess(caseSpec);
    const second = observeCaseInFreshProcess(caseSpec);
    requireCondition(
      first.observation_fingerprint_sha256 ===
          second.observation_fingerprint_sha256 &&
        stableStringify(first) === stableStringify(second),
      `${caseSpec.case_id} TypeScript observation is nondeterministic`,
    );
    if (caseSpec.expected_observation !== null) {
      requireCondition(
        stableStringify(first) === stableStringify(caseSpec.expected_observation),
        `${caseSpec.case_id} fresh observation diverges from W-H2.6C`,
      );
    }
    if (caseSpec.roles.includes("fault")) {
      requireCondition(
        first.reported_diagnostics.length + first.emit_result.diagnostics.length > 0,
        `${caseSpec.case_id} fault case observed no diagnostic`,
      );
    }
    if (caseSpec.roles.includes("adjacent-negative-control")) {
      requireCondition(
        caseSpec.control.compiler_options.declaration !== true &&
          first.writes.every((write) => write.kind !== "declaration"),
        `${caseSpec.case_id} adjacent-negative control emitted a declaration`,
      );
    }
    observations.push(
      withFingerprint(
        {
          case_id: caseSpec.case_id,
          repetitions: 2,
          observation: first,
        },
        "case_observation_fingerprint_sha256",
      ),
    );
    completed += 1;
    if (completed % 10 === 0 || completed === caseSpecs.length) {
      process.stderr.write(
        `W-H2.7A fresh observations: ${completed}/${caseSpecs.length} (${caseSpec.case_id})\n`,
      );
    }
  }
  return observations.sort((left, right) => compareBytes(left.case_id, right.case_id));
}

function observeStaticContext(staticContext) {
  const m1Observations = observeAllCases(staticContext.m1CaseSpecs);
  verifyM1Projection(
    staticContext.m1CaseManifest.cases,
    m1Observations,
    staticContext.stratum,
    "fresh m-1 observation",
  );
  const s2Observations = observeAllCases(staticContext.s2CaseSpecs);
  return [...m1Observations, ...s2Observations].sort((left, right) =>
    compareBytes(left.case_id, right.case_id),
  );
}

function validateStoredObservations(caseSpecs, stored) {
  requireCondition(Array.isArray(stored), "stored observations are absent");
  const expectedIds = caseSpecs.map((entry) => entry.case_id).sort(compareBytes);
  const actualIds = stored.map((entry) => entry.case_id).sort(compareBytes);
  requireCondition(
    stableStringify(actualIds) === stableStringify(expectedIds),
    "stored observation case IDs changed",
  );
  const byId = new Map();
  for (const entry of stored) {
    requireCondition(
      entry.repetitions === 2 &&
        fingerprintIsValid(entry, "case_observation_fingerprint_sha256"),
      `${entry.case_id} stored case-observation fingerprint is invalid`,
    );
    validateObservation(entry.observation, entry.case_id);
    byId.set(entry.case_id, entry);
  }
  return caseSpecs
    .map((entry) => byId.get(entry.case_id))
    .sort((left, right) => compareBytes(left.case_id, right.case_id));
}

function observationContentRoll(observations) {
  const hash = crypto.createHash("sha256");
  for (const entry of [...observations].sort((left, right) =>
    compareBytes(left.case_id, right.case_id),
  )) {
    hash.update(entry.case_id);
    hash.update("\0");
    hash.update(entry.case_observation_fingerprint_sha256);
    hash.update("\0");
  }
  return hash.digest("hex");
}

function buildSummary(staticContext, observations) {
  const writes = observations.flatMap((entry) => entry.observation.writes);
  const reportedDiagnostics = observations.reduce(
    (sum, entry) => sum + entry.observation.reported_diagnostics.length,
    0,
  );
  const emitDiagnostics = observations.reduce(
    (sum, entry) => sum + entry.observation.emit_result.diagnostics.length,
    0,
  );
  return {
    frozen_families: 14,
    declared_strata: 1,
    curated_cases: 27,
    stratum_cases: 67,
    cases: observations.length,
    positive_cases: staticContext.quotas.positive_cases,
    adjacent_negative_cases: staticContext.quotas.adjacent_negative_cases,
    composition_cases: staticContext.quotas.composition_cases,
    fault_cases: staticContext.quotas.fault_cases,
    declaration_writes: writes.filter((write) => write.kind === "declaration").length,
    javascript_writes: writes.filter((write) => write.kind === "javascript").length,
    source_map_writes: writes.filter((write) => write.kind === "source-map").length,
    other_writes: writes.filter((write) => write.kind === "other").length,
    reported_diagnostics: reportedDiagnostics,
    emit_diagnostics: emitDiagnostics,
    typescript_oracle_runs: observations.length * 2,
    deterministic_cases: observations.length,
    rust_runs: 0,
  };
}

function validateContractAssertions() {
  const contract = readJson(CONTRACT_RELATIVE_PATH);
  requireCondition(
    contract.$schema === "https://json-schema.org/draft/2020-12/schema" &&
      contract.type === "object" &&
      contract.properties?.schema?.const === 1 &&
      contract.properties?.status?.const === "qualified-typescript-oracle" &&
      contract.properties?.phase?.const === "H2.7a-witnesses" &&
      contract.properties?.stratum?.properties?.count?.const === 67 &&
      contract.properties?.observations?.minItems === 112 &&
      contract.properties?.observations?.maxItems === 112 &&
      contract.properties?.m2_supplement?.$ref ===
        "#/$defs/m2_supplement" &&
      contract.$defs?.case_manifest?.properties?.cases?.minItems === 112 &&
      contract.$defs?.case_manifest?.properties?.cases?.maxItems === 112 &&
      contract.$defs?.summary?.properties?.cases?.const === 112 &&
      contract.$defs?.summary?.properties?.typescript_oracle_runs?.const === 224 &&
      contract.$defs?.summary?.properties?.deterministic_cases?.const === 112 &&
      contract.$defs?.role?.enum?.includes("supplement") &&
      stableStringify(contract.properties?.coverage_matrix?.properties?.lanes?.const) ===
        stableStringify(LANES),
    "witness contract consts changed",
  );
  const quotaProperties = contract.properties?.quotas?.properties;
  for (const [name, minimum] of Object.entries(QUOTA_MINIMUMS)) {
    requireCondition(
      quotaProperties?.[name]?.minimum === minimum,
      `contract ${name} minimum changed`,
    );
  }
}

function validateArtifact(artifact, staticContext) {
  validateContractAssertions();
  requireCondition(
    artifact.schema === 1 &&
      artifact.kind === "h2-7a-public-observable-witnesses" &&
      artifact.status === "qualified-typescript-oracle" &&
      artifact.phase === "H2.7a-witnesses" &&
      fingerprintIsValid(artifact, "witnesses_fingerprint_sha256"),
    "artifact identity or fingerprint is invalid",
  );
  requireCondition(
    stableStringify(artifact.generator) ===
        stableStringify(pathHash(GENERATOR_RELATIVE_PATH)) &&
      stableStringify(artifact.contract) ===
        stableStringify(pathHash(CONTRACT_RELATIVE_PATH)),
    "artifact generator/contract pin changed",
  );
  verifyM1Projection(
    artifact.case_manifest.cases,
    artifact.observations,
    artifact.stratum,
    "artifact",
  );
  const manifestPayload = {
    cases: artifact.case_manifest.cases,
    source_universe: artifact.case_manifest.source_universe,
    source_universe_sha256: artifact.case_manifest.source_universe_sha256,
  };
  requireCondition(
    artifact.case_manifest.source_universe_sha256 ===
        sha256(
          Buffer.from(
            stableStringify(artifact.case_manifest.source_universe),
            "utf8",
          ),
        ) &&
      artifact.case_manifest.case_manifest_fingerprint ===
        sha256(Buffer.from(stableStringify(manifestPayload), "utf8")) &&
      artifact.case_manifest.case_manifest_fingerprint ===
        staticContext.caseManifest.case_manifest_fingerprint &&
      stableStringify(artifact.case_manifest) ===
        stableStringify(staticContext.caseManifest),
    "case manifest fingerprint changed",
  );
  requireCondition(
    stableStringify(
      artifact.case_manifest.cases.filter((entry) => entry.family_id !== "S2"),
    ) === stableStringify(staticContext.m1CaseManifest.cases),
    "m-1 case projection changed",
  );
  requireCondition(
    stableStringify(artifact.coverage_matrix.lanes) === stableStringify(LANES) &&
      artifact.coverage_matrix.lane_coverage.every(
        (entry) => entry.families.length > 0,
      ),
    "14-lane coverage matrix is incomplete",
  );
  for (const [name, minimum] of Object.entries(QUOTA_MINIMUMS)) {
    requireCondition(
      artifact.quotas[name] >= minimum &&
        artifact.quotas[name] === staticContext.quotas[name],
      `${name} artifact quota failed`,
    );
  }
  const manifestStratumIds = artifact.case_manifest.cases
    .filter((entry) => entry.family_id === "S")
    .map((entry) => entry.case_id)
    .sort(compareBytes);
  requireCondition(
    artifact.stratum.count === 67 &&
      artifact.stratum.case_ids.length === 67 &&
      new Set(artifact.stratum.case_ids).size === 67 &&
      stableStringify(artifact.stratum.case_ids) ===
        stableStringify(manifestStratumIds),
    "stratum checkpoint failed",
  );
  const supplementCaseIds = artifact.case_manifest.cases
    .filter((entry) => entry.family_id === "S2")
    .map((entry) => entry.case_id)
    .sort(compareBytes);
  requireCondition(
    supplementCaseIds.length === 18 &&
      stableStringify(supplementCaseIds) ===
        stableStringify([...staticContext.m2Supplement.case_ids].sort(compareBytes)) &&
      stableStringify(artifact.m2_supplement) ===
        stableStringify(staticContext.m2Supplement),
    "S2 supplement checkpoint failed",
  );
  requireCondition(
    artifact.observations.length === 112 &&
      artifact.summary.cases === 112 &&
      artifact.summary.typescript_oracle_runs === 224 &&
      artifact.summary.deterministic_cases === 112 &&
      artifact.summary.rust_runs === 0,
    "artifact summary denominator changed",
  );
  const sortedObservationIds = artifact.observations
    .map((entry) => entry.case_id)
    .sort(compareBytes);
  requireCondition(
    stableStringify(artifact.observations.map((entry) => entry.case_id)) ===
        stableStringify(sortedObservationIds) &&
      stableStringify(sortedObservationIds) ===
        stableStringify(
          artifact.case_manifest.cases.map((entry) => entry.case_id).sort(compareBytes),
        ),
    "per-case observations are not sorted or manifest-complete",
  );
  for (const entry of artifact.observations) {
    requireCondition(
      fingerprintIsValid(entry, "case_observation_fingerprint_sha256"),
      `${entry.case_id} case observation fingerprint is invalid`,
    );
    validateObservation(entry.observation, entry.case_id);
  }
  requireCondition(
    artifact.observation_content_roll_sha256 ===
      observationContentRoll(artifact.observations),
    "observation-content roll changed",
  );
}

function buildArtifact(staticContext, observations) {
  const observationRoll = observationContentRoll(observations);
  const artifact = withFingerprint(
    {
      schema: 1,
      kind: "h2-7a-public-observable-witnesses",
      status: "qualified-typescript-oracle",
      phase: "H2.7a-witnesses",
      typescript: staticContext.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      inputs: staticContext.inputs,
      population: staticContext.population,
      case_manifest: staticContext.caseManifest,
      coverage_matrix: staticContext.coverageMatrix,
      quotas: staticContext.quotas,
      stratum: staticContext.stratum,
      m2_supplement: staticContext.m2Supplement,
      observations,
      summary: buildSummary(staticContext, observations),
      observation_content_roll_sha256: observationRoll,
    },
    "witnesses_fingerprint_sha256",
  );
  validateArtifact(artifact, staticContext);
  return artifact;
}

function typescriptRecordFingerprint(record) {
  return sha256(Buffer.from(stableStringify(record), "utf8"));
}

function loadTrackedArtifact() {
  try {
    return readJson(TARGET_RELATIVE_PATH);
  } catch {
    throw new CheckReceiptMiss("stored-artifact");
  }
}

function verifyTrackedM1Projection() {
  const artifact = loadTrackedArtifact();
  verifyM1Projection(
    artifact.case_manifest?.cases ?? [],
    artifact.observations ?? [],
    artifact.stratum,
    "checked-in artifact",
  );
  return artifact;
}

function loadReceipt() {
  let receipt;
  try {
    receipt = readJson(CHECK_RECEIPT_RELATIVE_PATH);
  } catch {
    throw new CheckReceiptMiss("absent-or-invalid");
  }
  if (
    receipt === null ||
    typeof receipt !== "object" ||
    receipt.schema !== 1 ||
    receipt.kind !== "h2-7a-witnesses-check-receipt" ||
    receipt.minted_by !== "successful-full-reobservation-check" ||
    !fingerprintIsValid(receipt, "receipt_fingerprint_sha256")
  ) {
    throw new CheckReceiptMiss("receipt-shape");
  }
  return receipt;
}

function attemptReceiptHit(staticContext) {
  const stored = loadTrackedArtifact();
  const observations = validateStoredObservations(
    staticContext.caseSpecs,
    stored.observations,
  );
  const roll = observationContentRoll(observations);
  const receipt = loadReceipt();
  if (
    receipt.workspace !== fs.realpathSync(WORKSPACE) ||
    receipt.node !== process.version ||
    receipt.generator_sha256 !== pathHash(GENERATOR_RELATIVE_PATH).sha256 ||
    receipt.typescript_record_sha256 !==
      typescriptRecordFingerprint(staticContext.typescript) ||
    receipt.case_manifest_fingerprint !==
      staticContext.caseManifest.case_manifest_fingerprint ||
    receipt.observation_content_roll_sha256 !== roll
  ) {
    throw new CheckReceiptMiss("stale-key");
  }
  const artifact = buildArtifact(staticContext, observations);
  requireCondition(
    artifact.observation_content_roll_sha256 === roll,
    "receipt observation roll and artifact disagree",
  );
  return artifact;
}

function mintCheckReceipt(artifact) {
  const receipt = withFingerprint(
    {
      schema: 1,
      kind: "h2-7a-witnesses-check-receipt",
      minted_by: "successful-full-reobservation-check",
      workspace: fs.realpathSync(WORKSPACE),
      node: process.version,
      generator_sha256: artifact.generator.sha256,
      typescript_record_sha256: typescriptRecordFingerprint(artifact.typescript),
      case_manifest_fingerprint:
        artifact.case_manifest.case_manifest_fingerprint,
      observation_content_roll_sha256:
        artifact.observation_content_roll_sha256,
    },
    "receipt_fingerprint_sha256",
  );
  writeFileAtomic(
    path.join(WORKSPACE, CHECK_RECEIPT_RELATIVE_PATH),
    render(receipt),
  );
}

function compareWholeArtifact(artifact) {
  const targetPath = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
  requireCondition(
    fs.existsSync(targetPath) &&
      fs.readFileSync(targetPath, "utf8") === render(artifact),
    `stale ${TARGET_RELATIVE_PATH}; run h2-7a-witnesses.mjs --write and review`,
  );
}

function runInternalObserve() {
  requireCondition(process.argv.length === 4, "internal observation requires one case ID");
  validateRuntime();
  const caseId = process.argv[3];
  const observation = observeControl(readInternalObservationControl(caseId));
  validateObservation(observation, caseId);
  process.stdout.write(JSON.stringify(observation));
}

function runS2Dry() {
  const artifact = verifyTrackedM1Projection();
  const expansions = {
    compiler: readJson(TEST_SUITE_EXPANSION),
    conformance: readJson(CONFORMANCE_EXPANSION),
  };
  const s2 = buildS2Supplement(
    artifact.case_manifest.cases.filter((entry) => entry.family_id !== "S2"),
    expansions,
  );
  process.stdout.write(
    render({
      fixtures: s2.selection.members.flatMap((entry) =>
        entry.selected.map((candidate) => candidate.fixture_id),
      ),
      case_ids: s2.supplement.case_ids,
      trim_rows: s2.supplement.trim_rows,
    }),
  );
}

function runWrite() {
  verifyTrackedM1Projection();
  const staticContext = prepareStaticContext();
  const observations = observeStaticContext(staticContext);
  const artifact = buildArtifact(staticContext, observations);
  writeFileAtomic(path.join(WORKSPACE, TARGET_RELATIVE_PATH), render(artifact));
  process.stdout.write(
    `wrote ${TARGET_RELATIVE_PATH}: cases=${artifact.summary.cases} stratum=${artifact.stratum.count} lanes=${artifact.coverage_matrix.lanes.length} oracle_runs=${artifact.summary.typescript_oracle_runs}\n`,
  );
}

function runCheck() {
  const tracked = verifyTrackedM1Projection();
  requireCondition(
    stableStringify(tracked.generator) ===
        stableStringify(pathHash(GENERATOR_RELATIVE_PATH)) &&
      stableStringify(tracked.contract) ===
        stableStringify(pathHash(CONTRACT_RELATIVE_PATH)),
    `stale ${TARGET_RELATIVE_PATH}; run h2-7a-witnesses.mjs --write and review`,
  );
  const staticContext = prepareStaticContext();
  let artifact;
  try {
    artifact = attemptReceiptHit(staticContext);
    compareWholeArtifact(artifact);
    process.stdout.write(
      `W-H2.7A witnesses are fresh: cases=${artifact.summary.cases} stratum=${artifact.stratum.count} lanes=${artifact.coverage_matrix.lanes.length} check_receipt=hit\n`,
    );
    return;
  } catch (error) {
    if (!(error instanceof CheckReceiptMiss)) throw error;
    process.stderr.write(
      `W-H2.7A check receipt: miss (${error.message}); running full fresh-process double observation\n`,
    );
  }
  const observations = observeStaticContext(staticContext);
  artifact = buildArtifact(staticContext, observations);
  compareWholeArtifact(artifact);
  mintCheckReceipt(artifact);
  process.stdout.write(
    `W-H2.7A witnesses are fresh: cases=${artifact.summary.cases} stratum=${artifact.stratum.count} lanes=${artifact.coverage_matrix.lanes.length} check_receipt=minted\n`,
  );
}

try {
  if (MODE === INTERNAL_OBSERVE_MODE) runInternalObserve();
  else if (MODE === "--s2-dry") runS2Dry();
  else if (MODE === "--write") runWrite();
  else if (MODE === "--check") runCheck();
  else {
    fail(
      "usage: h2-7a-witnesses.mjs [--write|--check|--s2-dry|--internal-observe <case_id>]",
    );
  }
} catch (error) {
  process.stderr.write(`${error.message}\n`);
  process.exitCode = 1;
}
