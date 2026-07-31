import path from "node:path";
import { TextDecoder } from "node:util";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const U32_MAX = 0xffff_ffff;
const I32_MIN = -0x8000_0000;
const I32_MAX = 0x7fff_ffff;
const BASE64_PATTERN =
  /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/;
const CONTROL_PATTERN = /[\u0000-\u001f\u007f-\u009f]/u;

const optionDeclarations = new Map(
  (ts.optionDeclarations ?? []).map((declaration) => [
    declaration.name.toLowerCase(),
    declaration,
  ]),
);
const absentCompilerOption = Symbol("absent-compiler-option");
const supportedCompilerOptions = new Set([
  "allowarbitraryextensions",
  "allowimportingtsextensions",
  "allowjs",
  "allowsyntheticdefaultimports",
  "allowumdglobalaccess",
  "allowunreachablecode",
  "allowunusedlabels",
  "alwaysstrict",
  "baseurl",
  "checkjs",
  "customconditions",
  "downleveliteration",
  "esmoduleinterop",
  "exactoptionalpropertytypes",
  "experimentaldecorators",
  "importhelpers",
  "isolatedmodules",
  "jsx",
  "jsxfactory",
  "jsxfragmentfactory",
  "jsximportsource",
  "lib",
  "module",
  "moduledetection",
  "moduleresolution",
  "nodtsresolution",
  "noemit",
  "noerrortruncation",
  "nofallthroughcasesinswitch",
  "noimplicitany",
  "noimplicitoverride",
  "noimplicitreturns",
  "noimplicitthis",
  "nolib",
  "nopropertyaccessfromindexsignature",
  "nouncheckedindexedaccess",
  "nouncheckedsideeffectimports",
  "nounusedlocals",
  "nounusedparameters",
  "preserveconstenums",
  "reactnamespace",
  "resolvejsonmodule",
  "resolvepackagejsonexports",
  "resolvepackagejsonimports",
  "rewriterelativeimportextensions",
  "skiplibcheck",
  "strict",
  "strictbindcallapply",
  "strictbuiltiniteratorreturn",
  "strictfunctiontypes",
  "strictnullchecks",
  "strictpropertyinitialization",
  "target",
  "usedefineforclassfields",
  "useunknownincatchvariables",
  "verbatimmodulesyntax",
]);

export class M9RequestError extends Error {
  constructor(message) {
    super(message);
    this.name = "M9RequestError";
  }
}

export function assertClosedObject(value, keys, context) {
  if (
    value === null ||
    typeof value !== "object" ||
    Array.isArray(value)
  ) {
    throw new M9RequestError(`${context} must be an object`);
  }
  const actual = Object.keys(value);
  const expected = [...keys];
  if (
    actual.length !== expected.length ||
    actual.some((key, index) => key !== expected[index])
  ) {
    throw new M9RequestError(
      `${context} keys must be exactly ordered as ${JSON.stringify(expected)}, found ${JSON.stringify(actual)}`,
    );
  }
  return value;
}

export function assertCanonicalU64(value, context) {
  if (
    typeof value !== "string" ||
    !/^(?:0|[1-9][0-9]*)$/u.test(value)
  ) {
    throw new M9RequestError(
      `${context} must be a canonical unsigned decimal string`,
    );
  }
  let decoded;
  try {
    decoded = BigInt(value);
  } catch {
    throw new M9RequestError(`${context} does not fit in u64`);
  }
  if (decoded > 0xffff_ffff_ffff_ffffn) {
    throw new M9RequestError(`${context} does not fit in u64`);
  }
  return value;
}

export function assertLowerSha256(value, context) {
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/u.test(value)) {
    throw new M9RequestError(
      `${context} must be exactly 64 lowercase hexadecimal characters`,
    );
  }
  return value;
}

export function decodeCanonicalUtf8Base64(value, context) {
  if (
    typeof value !== "string" ||
    value.length % 4 !== 0 ||
    !BASE64_PATTERN.test(value)
  ) {
    throw new M9RequestError(`${context} is not canonical base64`);
  }
  const bytes = Buffer.from(value, "base64");
  if (bytes.toString("base64") !== value) {
    throw new M9RequestError(`${context} is not canonical base64`);
  }
  try {
    // `ignoreBOM: true` means the decoded U+FEFF is retained. This matches
    // Rust's `String::from_utf8`; TextDecoder's default consumes the BOM.
    return new TextDecoder("utf-8", {
      fatal: true,
      ignoreBOM: true,
    }).decode(bytes);
  } catch (error) {
    throw new M9RequestError(
      `${context} is not UTF-8 source text: ${String(error?.message ?? error)}`,
    );
  }
}

export function validateVirtualCwd(value, context = "program.cwd") {
  if (
    typeof value !== "string" ||
    !value.startsWith("/") ||
    value.includes("\\") ||
    CONTROL_PATTERN.test(value) ||
    (value.length > 1 && value.endsWith("/"))
  ) {
    throw new M9RequestError(
      `${context} must be an absolute canonical virtual POSIX path`,
    );
  }
  if (value !== "/") {
    for (const component of value.slice(1).split("/")) {
      if (
        component.length === 0 ||
        component === "." ||
        component === ".."
      ) {
        throw new M9RequestError(
          `${context} must be an absolute canonical virtual POSIX path`,
        );
      }
    }
  }
  return value;
}

export function validatePublicFileName(value, context) {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.includes("\\") ||
    CONTROL_PATTERN.test(value) ||
    value.endsWith("/")
  ) {
    throw new M9RequestError(
      `${context} must be a non-empty canonical POSIX public file name`,
    );
  }
  const components = value.split("/");
  if (value.startsWith("/")) components.shift();
  if (
    components.some(
      (component) =>
        component.length === 0 ||
        component === "." ||
        component === "..",
    )
  ) {
    throw new M9RequestError(
      `${context} must be a non-empty canonical POSIX public file name`,
    );
  }
  return value;
}

export function absoluteInlineFileName(publicName, cwd) {
  return publicName.startsWith("/")
    ? publicName
    : cwd === "/"
      ? `/${publicName}`
      : `${cwd}/${publicName}`;
}

function assertOrdinal(value, expected, context) {
  if (
    !Number.isInteger(value) ||
    value < 0 ||
    value > U32_MAX ||
    value !== expected
  ) {
    throw new M9RequestError(
      `${context}.ordinal must be ${expected}, found ${JSON.stringify(value)}`,
    );
  }
}

function decodeFiles(value, context, cwd) {
  if (!Array.isArray(value)) {
    throw new M9RequestError(`${context} must be an array`);
  }
  return value.map((rawFile, index) => {
    const file = assertClosedObject(
      rawFile,
      ["ordinal", "name", "text_base64"],
      `${context}[${index}]`,
    );
    assertOrdinal(file.ordinal, index, `${context}[${index}]`);
    const publicName = validatePublicFileName(
      file.name,
      `${context}[${index}].name`,
    );
    return {
      ordinal: index,
      publicName,
      resolvedName: absoluteInlineFileName(publicName, cwd),
      text: decodeCanonicalUtf8Base64(
        file.text_base64,
        `${context}[${index}].text_base64`,
      ),
    };
  });
}

function decodeCompilerOptionValue(value, declaration, context) {
  if (
    value === null ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    typeof value.kind !== "string"
  ) {
    throw new M9RequestError(`${context} must be a typed option value`);
  }

  if (value.kind === "null") {
    assertClosedObject(value, ["kind"], context);
    return absentCompilerOption;
  }

  if (declaration.type instanceof Map) {
    if (value.kind === "text") {
      assertClosedObject(value, ["kind", "value"], context);
      if (typeof value.value !== "string") {
        throw new M9RequestError(`${context}.value must be a string`);
      }
      if (CONTROL_PATTERN.test(value.value)) {
        throw new M9RequestError(
          `${context}.value must not contain control characters`,
        );
      }
      const mapped = declaration.type.get(value.value.toLowerCase());
      if (mapped === undefined) {
        throw new M9RequestError(
          `${context} is not a valid --${declaration.name} enum value`,
        );
      }
      return mapped;
    }
    if (value.kind === "number") {
      assertClosedObject(value, ["kind", "value"], context);
      if (
        !Number.isInteger(value.value) ||
        value.value < I32_MIN ||
        value.value > I32_MAX ||
        ![...declaration.type.values()].some(
          (candidate) => candidate === value.value,
        )
      ) {
        throw new M9RequestError(
          `${context} is not a valid numeric --${declaration.name} enum value`,
        );
      }
      return value.value;
    }
    throw new M9RequestError(
      `${context} must be text or a declared numeric enum value`,
    );
  }

  switch (declaration.type) {
    case "boolean":
      assertClosedObject(value, ["kind", "value"], context);
      if (value.kind !== "boolean" || typeof value.value !== "boolean") {
        throw new M9RequestError(`${context} must be boolean`);
      }
      return value.value;
    case "number":
      assertClosedObject(value, ["kind", "value"], context);
      if (
        value.kind !== "number" ||
        !Number.isInteger(value.value) ||
        value.value < I32_MIN ||
        value.value > I32_MAX
      ) {
        throw new M9RequestError(`${context} must be an i32 number`);
      }
      return value.value;
    case "string":
      assertClosedObject(value, ["kind", "value"], context);
      if (value.kind !== "text" || typeof value.value !== "string") {
        throw new M9RequestError(`${context} must be text`);
      }
      if (CONTROL_PATTERN.test(value.value)) {
        throw new M9RequestError(
          `${context}.value must not contain control characters`,
        );
      }
      return value.value;
    case "list": {
      let values;
      if (value.kind === "text") {
        assertClosedObject(value, ["kind", "value"], context);
        if (typeof value.value !== "string") {
          throw new M9RequestError(`${context}.value must be a string`);
        }
        values = value.value
          .split(",")
          .map((entry) => entry.trim())
          .filter((entry) => entry.length !== 0);
      } else {
        assertClosedObject(value, ["kind", "values"], context);
        if (
          value.kind !== "string-list" ||
          !Array.isArray(value.values) ||
          value.values.some((entry) => typeof entry !== "string")
        ) {
          throw new M9RequestError(
            `${context} must be text or a string-list`,
          );
        }
        values = value.values
          .map((entry) => entry.trim())
          .filter((entry) => entry.length !== 0);
      }
      if (values.some((entry) => CONTROL_PATTERN.test(entry))) {
        throw new M9RequestError(
          `${context} must not contain control characters`,
        );
      }
      if (
        declaration.element?.type !== "string" &&
        !(declaration.element?.type instanceof Map)
      ) {
        throw new M9RequestError(
          `${context} uses an unsupported --${declaration.name} list element type`,
        );
      }
      return declaration.name === "lib"
        ? values.map((entry) => entry.toLowerCase())
        : values;
    }
    default:
      throw new M9RequestError(
        `${context} uses unsupported --${declaration.name} option type`,
      );
  }
}

export function compilerOptionsFromOrderedSettings(value) {
  if (!Array.isArray(value)) {
    throw new M9RequestError("program.options must be an array");
  }
  const options = {};
  const seen = new Set();
  let previousName = null;
  for (const [index, rawSetting] of value.entries()) {
    const setting = assertClosedObject(
      rawSetting,
      ["ordinal", "name", "value"],
      `program.options[${index}]`,
    );
    assertOrdinal(setting.ordinal, index, `program.options[${index}]`);
    if (
      typeof setting.name !== "string" ||
      !/^[A-Za-z][A-Za-z0-9]*$/u.test(setting.name)
    ) {
      throw new M9RequestError(
        `program.options[${index}].name must be an ASCII compiler-option name`,
      );
    }
    const foldedName = setting.name.toLowerCase();
    if (seen.has(foldedName)) {
      throw new M9RequestError(
        `program.options contains case-insensitive duplicate ${JSON.stringify(setting.name)}`,
      );
    }
    if (
      previousName !== null &&
      Buffer.compare(
        Buffer.from(previousName, "utf8"),
        Buffer.from(setting.name, "utf8"),
      ) >= 0
    ) {
      throw new M9RequestError(
        "program.options must be strictly sorted by option-name UTF-8 bytes",
      );
    }
    previousName = setting.name;
    seen.add(foldedName);
    const declaration = optionDeclarations.get(foldedName);
    if (!declaration || !supportedCompilerOptions.has(foldedName)) {
      throw new M9RequestError(
        `program.options contains unsupported compiler option ${JSON.stringify(setting.name)}`,
      );
    }
    const decoded = decodeCompilerOptionValue(
      setting.value,
      declaration,
      `program.options[${index}].value`,
    );
    if (
      foldedName === "nolib" &&
      decoded !== absentCompilerOption &&
      decoded !== true
    ) {
      throw new M9RequestError(
        "program.options.noLib must be true or null for the inline M9 host",
      );
    }
    if (decoded !== absentCompilerOption && foldedName !== "nolib") {
      options[declaration.name] = decoded;
    }
  }
  return { ...options, noLib: true };
}

export function decodeInlineProgram(value) {
  const program = assertClosedObject(
    value,
    ["cwd", "options", "libs", "files"],
    "program",
  );
  const cwd = validateVirtualCwd(program.cwd);
  const options = compilerOptionsFromOrderedSettings(program.options);
  const libs = decodeFiles(program.libs, "program.libs", cwd);
  const files = decodeFiles(program.files, "program.files", cwd);
  if (files.length === 0) {
    throw new M9RequestError("program.files must not be empty");
  }

  const sources = new Map();
  const publicNames = new Map();
  const rawPublicNames = new Set();
  for (const entry of [...libs, ...files]) {
    if (rawPublicNames.has(entry.publicName)) {
      throw new M9RequestError(
        `duplicate public file name across libs/files: ${JSON.stringify(entry.publicName)}`,
      );
    }
    if (sources.has(entry.resolvedName)) {
      throw new M9RequestError(
        `duplicate resolved file name across libs/files: ${JSON.stringify(entry.resolvedName)}`,
      );
    }
    rawPublicNames.add(entry.publicName);
    sources.set(entry.resolvedName, entry.text);
    publicNames.set(entry.resolvedName, entry.publicName);
  }

  return {
    cwd,
    options,
    libs,
    files,
    rootNames: [...libs, ...files].map((entry) => entry.resolvedName),
    sources,
    publicNames,
  };
}

export function createMapOnlyHost(decoded) {
  const languageVersion = decoded.options.target ?? ts.ScriptTarget.Latest;
  const sourceFiles = new Map();
  return {
    getSourceFile(
      fileName,
      languageVersionOrOptions = languageVersion,
    ) {
      const resolved = resolveHostFileName(fileName, decoded.cwd);
      const text = decoded.sources.get(resolved);
      if (text === undefined) return undefined;
      let sourceFile = sourceFiles.get(resolved);
      if (!sourceFile) {
        sourceFile = ts.createSourceFile(
          resolved,
          text,
          languageVersionOrOptions,
          true,
        );
        sourceFiles.set(resolved, sourceFile);
      }
      return sourceFile;
    },
    getDefaultLibFileName() {
      return "lib.d.ts";
    },
    getCurrentDirectory() {
      return decoded.cwd;
    },
    getDirectories() {
      return [];
    },
    getCanonicalFileName(fileName) {
      return resolveHostFileName(fileName, decoded.cwd);
    },
    useCaseSensitiveFileNames() {
      return true;
    },
    getNewLine() {
      return "\n";
    },
    fileExists(fileName) {
      return decoded.sources.has(resolveHostFileName(fileName, decoded.cwd));
    },
    readFile(fileName) {
      return decoded.sources.get(resolveHostFileName(fileName, decoded.cwd));
    },
    writeFile() {},
  };
}

function resolveHostFileName(fileName, cwd) {
  if (typeof fileName !== "string") return "";
  const normalized = fileName.replaceAll("\\", "/");
  return path.posix.isAbsolute(normalized)
    ? path.posix.normalize(normalized)
    : path.posix.resolve(cwd, normalized);
}

export function createProgramFromInline(decoded) {
  const host = createMapOnlyHost(decoded);
  const program = ts.createProgram(
    decoded.rootNames,
    decoded.options,
    host,
  );
  return { ...decoded, host, program };
}

export function publicFileNameForSource(file, publicNames) {
  if (file === undefined || file === null) return null;
  const fileName =
    typeof file.fileName === "string" ? file.fileName.replaceAll("\\", "/") : "";
  const publicName = publicNames.get(fileName);
  if (publicName === undefined) {
    throw new M9RequestError(
      `diagnostic references a source outside the inline program: ${JSON.stringify(fileName)}`,
    );
  }
  return publicName;
}
