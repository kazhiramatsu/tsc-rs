import fs from "node:fs";
import path from "node:path";
import readline from "node:readline";
import { fileURLToPath } from "node:url";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";
import {
  absoluteProgramFileName,
  compilerOptionsFromProgram,
  createHost,
  createProgramFromJsonPath,
  normalizeFileName,
} from "./program-host.mjs";

const DRIVER_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const VENDOR_LIB_DIRECTORY = path.resolve(
  DRIVER_DIRECTORY,
  "../../vendor/typescript-6.0.3/lib",
);
const VENDOR_LIB_NAME = /^lib(?:\.[A-Za-z0-9]+)*\.d\.ts$/;
const VENDOR_LIB_TEXT = new Map();
const EXACT_MODULE_LITERAL_CODES = new Set([2307, 2665, 2792, 2877, 2882]);
const EXCEPTION_CODES = new Set([2305, 2322, 2339, 2688, 2748, 2807]);
const SHA256_HEX = /^[0-9a-f]{64}$/;

function fail(message) {
  throw new Error(message);
}

function requireCondition(condition, message) {
  if (!condition) fail(message);
}

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function validateIdentity(identity, label) {
  requireCondition(isObject(identity), `${label} must be an object`);
  requireCondition(
    typeof identity.fixture === "string" && identity.fixture.length > 0,
    `${label}.fixture must be a non-empty string`,
  );
  requireCondition(
    typeof identity.matrix_key === "string",
    `${label}.matrix_key must be a string`,
  );
  requireCondition(identity.pass === "semantic", `${label}.pass must be semantic`);
  requireCondition(
    typeof identity.file === "string" && identity.file.length > 0,
    `${label}.file must be a non-empty string`,
  );
  for (const field of ["start", "length", "code", "occurrence"]) {
    requireCondition(
      Number.isSafeInteger(identity[field]) && identity[field] >= 0,
      `${label}.${field} must be a non-negative safe integer`,
    );
  }
  requireCondition(identity.length > 0, `${label}.length must be positive`);
  requireCondition(identity.category === "error", `${label}.category must be error`);
  requireCondition(
    typeof identity.chain_sha256 === "string" && SHA256_HEX.test(identity.chain_sha256),
    `${label}.chain_sha256 must be a lowercase SHA-256`,
  );
  requireCondition(
    typeof identity.related_sha256 === "string" && SHA256_HEX.test(identity.related_sha256),
    `${label}.related_sha256 must be a lowercase SHA-256`,
  );
  requireCondition(
    EXACT_MODULE_LITERAL_CODES.has(identity.code) || EXCEPTION_CODES.has(identity.code),
    `${label} has unsupported host-resolution diagnostic code ${identity.code}`,
  );
}

function normalizeIdentityEntry(entry, index) {
  const label = `identities[${index}]`;
  requireCondition(isObject(entry), `${label} must be an object`);
  if (!Object.hasOwn(entry, "identity")) {
    validateIdentity(entry, label);
    return { id: null, identity: entry };
  }
  requireCondition(
    typeof entry.id === "string" && entry.id.length > 0,
    `${label}.id must be a non-empty string`,
  );
  validateIdentity(entry.identity, `${label}.identity`);
  return { id: entry.id, identity: entry.identity };
}

function readVendoredLib(lib) {
  let text = VENDOR_LIB_TEXT.get(lib);
  if (text === undefined) {
    text = fs.readFileSync(path.join(VENDOR_LIB_DIRECTORY, lib), "utf8");
    VENDOR_LIB_TEXT.set(lib, text);
  }
  return text;
}

function createProgramFromInlineJson(programJson) {
  requireCondition(isObject(programJson), "request.programJson must be an object");
  requireCondition(programJson.schema === 1, "request.programJson.schema must be 1");
  requireCondition(
    programJson.cwd === undefined || typeof programJson.cwd === "string",
    "request.programJson.cwd must be a string when present",
  );
  requireCondition(
    programJson.options === undefined || isObject(programJson.options),
    "request.programJson.options must be an object when present",
  );
  requireCondition(Array.isArray(programJson.libs), "request.programJson.libs must be an array");
  requireCondition(Array.isArray(programJson.files), "request.programJson.files must be an array");

  const cwd = normalizeFileName(path.posix.resolve(programJson.cwd ?? "/"));
  const files = new Map();
  for (const [index, lib] of programJson.libs.entries()) {
    requireCondition(
      typeof lib === "string" && VENDOR_LIB_NAME.test(lib),
      `request.programJson.libs[${index}] must be a vendored lib*.d.ts basename`,
    );
    const fileName = absoluteProgramFileName(lib, cwd);
    files.set(fileName, readVendoredLib(lib));
  }
  for (const [index, file] of programJson.files.entries()) {
    requireCondition(isObject(file), `request.programJson.files[${index}] must be an object`);
    requireCondition(
      typeof file.name === "string" && file.name.length > 0,
      `request.programJson.files[${index}].name must be a non-empty string`,
    );
    requireCondition(
      typeof file.textB64 === "string",
      `request.programJson.files[${index}].textB64 must be a string`,
    );
    const fileName = absoluteProgramFileName(file.name, cwd);
    requireCondition(
      !files.has(fileName),
      `request.programJson contains duplicate canonical file ${fileName}`,
    );
    files.set(fileName, Buffer.from(file.textB64, "base64").toString("utf8"));
  }

  const options = compilerOptionsFromProgram(programJson);
  const rootNames = [
    ...programJson.libs.map((fileName) => absoluteProgramFileName(fileName, cwd)),
    ...programJson.files.map((file) => absoluteProgramFileName(file.name, cwd)),
  ];
  const host = createHost(options, files, cwd);
  const program = ts.createProgram(rootNames, options, host);
  return { program, programJson, cwd };
}

function createProgramFromRequest(request) {
  const hasPath = Object.hasOwn(request, "programJsonPath");
  const hasInline = Object.hasOwn(request, "programJson");
  requireCondition(
    hasPath !== hasInline,
    "request must contain exactly one of programJsonPath or programJson",
  );
  if (hasInline) return createProgramFromInlineJson(request.programJson);
  requireCondition(
    typeof request.programJsonPath === "string" && request.programJsonPath.length > 0,
    "request.programJsonPath must be a non-empty string",
  );
  return createProgramFromJsonPath(request.programJsonPath);
}

function modeName(mode, context) {
  if (mode === undefined) return "unspecified";
  if (mode === ts.ModuleKind.CommonJS) return "common-js";
  if (mode === ts.ModuleKind.ESNext) return "es-next";
  fail(`${context} has unsupported TypeScript resolution mode ${String(mode)}`);
}

function diagnosticCategoryName(category) {
  switch (category) {
    case ts.DiagnosticCategory.Warning:
      return "warning";
    case ts.DiagnosticCategory.Error:
      return "error";
    case ts.DiagnosticCategory.Suggestion:
      return "suggestion";
    case ts.DiagnosticCategory.Message:
      return "message";
    default:
      return `unknown-${String(category)}`;
  }
}

function canonicalSourceName(value) {
  return normalizeFileName(String(value));
}

function collectResolutionInventory(program) {
  const modules = [];
  const typeReferences = [];
  program.forEachResolvedModule((_resolution, specifier, mode, source) => {
    modules.push({
      canonicalSource: canonicalSourceName(source),
      specifier: String(specifier),
      mode: modeName(mode, `module request ${String(source)} -> ${String(specifier)}`),
    });
  });
  program.forEachResolvedTypeReferenceDirective((_resolution, specifier, mode, source) => {
    typeReferences.push({
      canonicalSource: canonicalSourceName(source),
      specifier: String(specifier),
      mode: modeName(mode, `type-reference request ${String(source)} -> ${String(specifier)}`),
    });
  });
  return { modules, typeReferences };
}

function sourceFileFor(program, cwd, fileName, context) {
  const canonical = absoluteProgramFileName(fileName, cwd);
  const sourceFile = program.getSourceFile(canonical);
  requireCondition(sourceFile, `${context} source file ${canonical} is absent from ProgramJson`);
  return sourceFile;
}

function realNodeStart(node, sourceFile, context) {
  requireCondition(
    Number.isSafeInteger(node.pos) && node.pos >= 0 && Number.isSafeInteger(node.end) && node.end >= 0,
    `${context} must have a real source position`,
  );
  return node.getStart(sourceFile);
}

function allStringLiterals(sourceFile) {
  const literals = [];
  const visit = (node) => {
    if (ts.isStringLiteralLike(node)) literals.push(node);
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);
  return literals;
}

function findLiteral(sourceFile, { specifier, anchorStart, anchorEnd, predicate }, context) {
  // JSDoc import() literals are registered in SourceFile.imports but are not
  // children of the ordinary syntax tree walked by forEachChild.
  const candidates = [...new Set([...allStringLiterals(sourceFile), ...sourceFile.imports])];
  const matches = candidates.filter((literal) => {
    if (literal.pos < 0 || literal.end < 0) return false;
    if (literal.text !== specifier) return false;
    if (realNodeStart(literal, sourceFile, context) !== anchorStart) return false;
    if (anchorEnd !== undefined && literal.end !== anchorEnd) return false;
    return predicate === undefined || predicate(literal);
  });
  requireCondition(
    matches.length === 1,
    `${context} expected exactly one ${JSON.stringify(specifier)} literal at ${anchorStart}, found ${matches.length}`,
  );
  return matches[0];
}

function requireCachedRequest(entries, kind, canonicalSource, specifier, mode, context) {
  const matches = entries.filter(
    (entry) =>
      entry.canonicalSource === canonicalSource &&
      entry.specifier === specifier &&
      entry.mode === mode,
  );
  requireCondition(
    matches.length === 1,
    `${context} expected exactly one cached ${kind} request ${canonicalSource} -> ${JSON.stringify(specifier)} (${mode}), found ${matches.length}`,
  );
  return matches[0];
}

function moduleRequest(
  program,
  inventory,
  sourceFile,
  literal,
  anchorKind,
  anchorStart,
  synthetic,
  context,
) {
  const canonicalSource = canonicalSourceName(sourceFile.fileName);
  const specifier = literal.text;
  const mode = modeName(
    program.getModeForUsageLocation(sourceFile, literal),
    `${context} usage mode`,
  );
  requireCachedRequest(
    inventory.modules,
    "module",
    canonicalSource,
    specifier,
    mode,
    context,
  );
  return {
    kind: "module",
    canonicalSource,
    specifier,
    mode,
    anchorKind,
    anchorStart,
    synthetic,
  };
}

function typeReferenceRequest(
  inventory,
  sourceFile,
  directive,
  anchorKind,
  context,
) {
  const canonicalSource = canonicalSourceName(sourceFile.fileName);
  const specifier = directive.fileName;
  const mode = "unspecified";
  requireCachedRequest(
    inventory.typeReferences,
    "type-reference",
    canonicalSource,
    specifier,
    mode,
    context,
  );
  return {
    kind: "type-reference",
    canonicalSource,
    specifier,
    mode,
    anchorKind,
    anchorStart: directive.pos,
    synthetic: false,
  };
}

function validateDiagnostic(identity, diagnostics, cwd, context) {
  const canonicalFile = absoluteProgramFileName(identity.file, cwd);
  const matches = diagnostics.filter(
    (diagnostic) =>
      diagnostic.file !== undefined &&
      canonicalSourceName(diagnostic.file.fileName) === canonicalFile &&
      diagnostic.start === identity.start &&
      diagnostic.length === identity.length &&
      diagnostic.code === identity.code &&
      diagnosticCategoryName(diagnostic.category) === identity.category,
  );
  requireCondition(
    identity.occurrence < matches.length,
    `${context} exact diagnostic is absent (matched ${matches.length}, occurrence ${identity.occurrence})`,
  );
}

function requireExactIdentity(identity, expected, context) {
  for (const [field, value] of Object.entries(expected)) {
    requireCondition(
      identity[field] === value,
      `${context} expected ${field}=${JSON.stringify(value)}, got ${JSON.stringify(identity[field])}`,
    );
  }
}

function directLiteralRequests(program, inventory, cwd, identity, context) {
  const sourceFile = sourceFileFor(program, cwd, identity.file, context);
  const anchorEnd = identity.start + identity.length;
  let literal;
  let anchorKind;
  if (identity.code === 2665) {
    literal = findLiteral(
      sourceFile,
      {
        specifier: "foo",
        anchorStart: identity.start,
        anchorEnd,
        predicate: (candidate) =>
          ts.isModuleDeclaration(candidate.parent) && candidate.parent.name === candidate,
      },
      context,
    );
    requireExactIdentity(
      identity,
      {
        fixture: "conformance/moduleResolution/untypedModuleImport_withAugmentation.ts",
        matrix_key: "",
        file: "/a.ts",
        start: 15,
        length: 5,
        code: 2665,
      },
      context,
    );
    anchorKind = "module-augmentation-literal";
  } else {
    const matches = sourceFile.imports.filter(
      (candidate) =>
        candidate.pos >= 0 &&
        candidate.end >= 0 &&
        realNodeStart(candidate, sourceFile, context) === identity.start &&
        candidate.end === anchorEnd,
    );
    requireCondition(
      matches.length === 1,
      `${context} diagnostic span must identify exactly one SourceFile.imports literal, found ${matches.length}`,
    );
    literal = matches[0];
    anchorKind = "module-literal";
  }
  return [
    moduleRequest(
      program,
      inventory,
      sourceFile,
      literal,
      anchorKind,
      identity.start,
      false,
      context,
    ),
  ];
}

function typesVersionsRequests(program, inventory, cwd, identity, context) {
  requireExactIdentity(
    identity,
    {
      fixture:
        "conformance/declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToSelf.ts",
      matrix_key: "",
      file: "main.ts",
      start: 9,
      length: 2,
      code: 2305,
    },
    context,
  );
  const main = sourceFileFor(program, cwd, "/main.ts", context);
  const entry = findLiteral(
    main,
    { specifier: "ext", anchorStart: 19, anchorEnd: 24 },
    context,
  );
  const declaration = sourceFileFor(
    program,
    cwd,
    "/node_modules/ext/ts3.1/index.d.ts",
    context,
  );
  const selfReference = findLiteral(
    declaration,
    { specifier: "../", anchorStart: 14, anchorEnd: 19 },
    context,
  );
  return [
    moduleRequest(
      program,
      inventory,
      main,
      entry,
      "containing-import",
      19,
      false,
      context,
    ),
    moduleRequest(
      program,
      inventory,
      declaration,
      selfReference,
      "types-versions-self-reference",
      14,
      false,
      context,
    ),
  ];
}

function jsdocImportRequests(program, inventory, cwd, identity, context) {
  const anchors = new Map([
    [209, 28],
    [281, 103],
  ]);
  requireExactIdentity(
    identity,
    {
      fixture: "conformance/jsdoc/importTag17.ts",
      matrix_key: "",
      file: "/a.js",
      length: 6,
      code: 2322,
    },
    context,
  );
  const anchorStart = anchors.get(identity.start);
  requireCondition(anchorStart !== undefined, `${context} has an unknown TS2322 span`);
  const sourceFile = sourceFileFor(program, cwd, identity.file, context);
  const literal = findLiteral(
    sourceFile,
    { specifier: "foo", anchorStart, anchorEnd: anchorStart + 5 },
    context,
  );
  return [
    moduleRequest(
      program,
      inventory,
      sourceFile,
      literal,
      "jsdoc-import",
      anchorStart,
      false,
      context,
    ),
  ];
}

function resolvedAliasRequests(program, inventory, cwd, identity, context) {
  let expected;
  let specifier;
  let anchorStart;
  if (identity.fixture === "conformance/moduleResolution/untypedModuleImport_allowJs.ts") {
    expected = {
      fixture: "conformance/moduleResolution/untypedModuleImport_allowJs.ts",
      matrix_key: "",
      file: "/a.ts",
      start: 28,
      length: 3,
      code: 2339,
    };
    specifier = "foo";
    anchorStart = 16;
  } else {
    expected = {
      fixture: "conformance/salsa/namespaceAssignmentToRequireAlias.ts",
      matrix_key: "",
      file: "bug40140.js",
      code: 2339,
    };
    requireCondition(
      (identity.start === 32 && identity.length === 10) ||
        (identity.start === 59 && identity.length === 7),
      `${context} has an unknown salsa TS2339 span`,
    );
    specifier = "untyped";
    anchorStart = 18;
  }
  requireExactIdentity(identity, expected, context);
  const sourceFile = sourceFileFor(program, cwd, identity.file, context);
  const literal = findLiteral(
    sourceFile,
    { specifier, anchorStart, anchorEnd: anchorStart + specifier.length + 2 },
    context,
  );
  return [
    moduleRequest(
      program,
      inventory,
      sourceFile,
      literal,
      "resolved-alias-import",
      anchorStart,
      false,
      context,
    ),
  ];
}

function typeReferenceRequests(inventory, program, cwd, identity, context) {
  requireExactIdentity(
    identity,
    {
      fixture: "conformance/typings/typingsLookup3.ts",
      matrix_key: "",
      file: "/a.ts",
      start: 22,
      length: 6,
      code: 2688,
    },
    context,
  );
  const sourceFile = sourceFileFor(program, cwd, identity.file, context);
  const matches = sourceFile.typeReferenceDirectives.filter(
    (directive) =>
      directive.fileName === "JqUeRy" &&
      directive.pos === identity.start &&
      directive.end === identity.start + identity.length,
  );
  requireCondition(
    matches.length === 1,
    `${context} expected exactly one JqUeRy type-reference directive, found ${matches.length}`,
  );
  return [
    typeReferenceRequest(
      inventory,
      sourceFile,
      matches[0],
      "type-reference-directive",
      context,
    ),
  ];
}

function constEnumRequests(program, inventory, cwd, identity, context) {
  requireCondition(
    identity.file === "/a.ts" || identity.file === "/b.ts",
    `${context} TS2748 file must be /a.ts or /b.ts`,
  );
  requireExactIdentity(
    identity,
    {
      fixture: "conformance/externalModules/verbatimModuleSyntaxAmbientConstEnum.ts",
      matrix_key: "",
      start: 9,
      length: 1,
      code: 2748,
    },
    context,
  );
  const sourceFile = sourceFileFor(program, cwd, identity.file, context);
  const literal = findLiteral(
    sourceFile,
    { specifier: "pkg", anchorStart: 18, anchorEnd: 23 },
    context,
  );
  return [
    moduleRequest(
      program,
      inventory,
      sourceFile,
      literal,
      "containing-import",
      18,
      false,
      context,
    ),
  ];
}

function importHelpersRequests(program, inventory, cwd, identity, context) {
  const allowed = new Map([
    [
      "conformance/classes/members/privateNames/privateNameEmitHelpers.ts",
      new Map([
        [41, 7],
        [81, 7],
      ]),
    ],
    [
      "conformance/classes/members/privateNames/privateNameStaticEmitHelpers.ts",
      new Map([
        [55, 7],
        [100, 4],
      ]),
    ],
  ]);
  requireExactIdentity(
    identity,
    { matrix_key: "", file: "main.ts", code: 2807 },
    context,
  );
  const spans = allowed.get(identity.fixture);
  requireCondition(spans !== undefined, `${context} has an unknown TS2807 fixture`);
  requireCondition(
    spans.get(identity.start) === identity.length,
    `${context} has an unknown TS2807 span`,
  );
  const sourceFile = sourceFileFor(program, cwd, identity.file, context);
  const matches = sourceFile.imports.filter(
    (literal) =>
      literal.text === "tslib" && literal.pos < 0 && literal.end < 0,
  );
  requireCondition(
    matches.length === 1,
    `${context} expected exactly one synthetic tslib import, found ${matches.length}`,
  );
  return [
    moduleRequest(
      program,
      inventory,
      sourceFile,
      matches[0],
      "synthetic-import-helpers",
      null,
      true,
      context,
    ),
  ];
}

function requestsForIdentity(program, inventory, cwd, identity, context) {
  if (EXACT_MODULE_LITERAL_CODES.has(identity.code)) {
    return directLiteralRequests(program, inventory, cwd, identity, context);
  }
  switch (identity.code) {
    case 2305:
      return typesVersionsRequests(program, inventory, cwd, identity, context);
    case 2322:
      return jsdocImportRequests(program, inventory, cwd, identity, context);
    case 2339:
      return resolvedAliasRequests(program, inventory, cwd, identity, context);
    case 2688:
      return typeReferenceRequests(inventory, program, cwd, identity, context);
    case 2748:
      return constEnumRequests(program, inventory, cwd, identity, context);
    case 2807:
      return importHelpersRequests(program, inventory, cwd, identity, context);
    default:
      return fail(`${context} has unsupported diagnostic code ${identity.code}`);
  }
}

function executeRequest(request) {
  requireCondition(isObject(request), "request must be an object");
  requireCondition(
    (typeof request.id === "string" && request.id.length > 0) ||
      (Number.isSafeInteger(request.id) && request.id >= 0),
    "request.id must be a non-empty string or non-negative safe integer",
  );
  requireCondition(
    Array.isArray(request.identities) && request.identities.length > 0,
    "request.identities must be a non-empty array",
  );
  const entries = request.identities.map(normalizeIdentityEntry);
  const rowIds = entries.filter((entry) => entry.id !== null).map((entry) => entry.id);
  requireCondition(
    new Set(rowIds).size === rowIds.length,
    "request.identities contains duplicate row ids",
  );
  const fixtures = new Set(entries.map((entry) => entry.identity.fixture));
  const matrixKeys = new Set(entries.map((entry) => entry.identity.matrix_key));
  requireCondition(fixtures.size === 1, "one ProgramJson batch must contain exactly one fixture");
  requireCondition(matrixKeys.size === 1, "one ProgramJson batch must contain exactly one matrix key");

  const { program, programJson, cwd } = createProgramFromRequest(request);
  const matrixKey = programJson.matrixKey ?? "";
  requireCondition(
    matrixKey === entries[0].identity.matrix_key,
    `ProgramJson matrixKey ${JSON.stringify(matrixKey)} does not match identity matrix_key ${JSON.stringify(entries[0].identity.matrix_key)}`,
  );
  const diagnostics = program.getSemanticDiagnostics();
  const inventory = collectResolutionInventory(program);
  const identities = entries.map((entry, index) => {
    const context = entry.id === null
      ? `identities[${index}] ${entry.identity.fixture}`
      : `row ${entry.id}`;
    validateDiagnostic(entry.identity, diagnostics, cwd, context);
    return {
      id: entry.id,
      requests: requestsForIdentity(
        program,
        inventory,
        cwd,
        entry.identity,
        context,
      ),
    };
  });
  return { id: request.id, identities };
}

const lines = readline.createInterface({
  input: process.stdin,
  crlfDelay: Infinity,
});

for await (const line of lines) {
  if (!line.trim()) continue;
  let id = null;
  try {
    const request = JSON.parse(line);
    id = request?.id ?? null;
    if (request?.versionProbe === true) {
      process.stdout.write(`${JSON.stringify({ id, version: process.version })}\n`);
      continue;
    }
    process.stdout.write(`${JSON.stringify(executeRequest(request))}\n`);
  } catch (error) {
    process.stdout.write(
      `${JSON.stringify({ id, error: String(error?.stack ?? error) })}\n`,
    );
  }
}
