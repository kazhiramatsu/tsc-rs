import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

let ts;

const DRIVER_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE = path.resolve(DRIVER_DIRECTORY, "../..");
const NODE_VERSION_PATH = path.join(WORKSPACE, ".node-version");
const ARTIFACT_RELATIVE_PATH =
  "vendor/typescript-6.0.3/compiler-config-diagnostics.v1.json";
const ARTIFACT_PATH = path.join(WORKSPACE, ARTIFACT_RELATIVE_PATH);
const TYPESCRIPT_BUNDLE_RELATIVE_PATH =
  "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_BUNDLE_PATH = path.join(
  WORKSPACE,
  TYPESCRIPT_BUNDLE_RELATIVE_PATH,
);
const TYPESCRIPT_SOURCE_RELATIVE_PATH =
  "vendor/typescript-6.0.3/lib/_tsc.js";
const TYPESCRIPT_SOURCE_PATH = path.join(
  WORKSPACE,
  TYPESCRIPT_SOURCE_RELATIVE_PATH,
);

const EXPECTED_NODE_VERSION = "25.2.1";
const EXPECTED_TYPESCRIPT_VERSION = "6.0.3";
const EXPECTED_SOURCE_COMMIT =
  "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_TYPESCRIPT_BUNDLE_SHA256 =
  "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const EXPECTED_TYPESCRIPT_SOURCE_SHA256 =
  "1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3";
const ROOT_FILE_NAME = "/project/tsconfig.json";
const ROOT_BASE_PATH = "/project";

const MALFORMED_CONFIG =
  '{"note":"😀","compilerOptions":{"strict":true,"ALLOWJS":true},"files":["root.ts"]';

const COMPILER_LIST_OPTION_NAMES = new Set([
  "lib",
  "rootDirs",
  "typeRoots",
  "types",
  "moduleSuffixes",
  "customConditions",
  "plugins",
]);

const FIXTURES = [
  {
    id: "unknown-invalid-options",
    rootText: `{
  "compilerOptions": {
    "ALLOWJS": true,
    "allowJs": "yes",
    "target": "not-a-target",
    "strict": true
  },
  "excludes": ["tmp"],
  "files": ["root.ts"]
}`,
    optionProbeKeys: ["ALLOWJS", "allowJs", "target", "strict"],
  },
  {
    id: "mixed-extends-two-phase",
    rootText:
      '{"extends":["./a",7,"./missing","./b"],"files":["root.ts"]}',
    hostFiles: [
      {
        path: "/project/a.json",
        text: '{"compilerOptions":{"strict":true}}',
      },
      {
        path: "/project/b.json",
        text: '{"compilerOptions":{"allowJs":true}}',
      },
    ],
    optionProbeKeys: ["strict", "allowJs"],
  },
  {
    id: "missing-explicit-and-implicit",
    rootText:
      '{"extends":["./explicit.json","./implicit"],"files":["root.ts"]}',
  },
  {
    id: "read-file-error-continues-sibling",
    rootText:
      '{"extends":["./throws.json","./ok.json"],"files":["root.ts"]}',
    hostFiles: [
      {
        path: "/project/throws.json",
        readError: "synthetic read failure",
      },
      {
        path: "/project/ok.json",
        text: '{"compilerOptions":{"allowJs":true}}',
      },
    ],
    optionProbeKeys: ["strict", "allowJs"],
  },
  {
    id: "cycle-continues-sibling",
    rootText:
      '{"extends":["./a.json","./ok.json"],"files":["root.ts"]}',
    hostFiles: [
      {
        path: ROOT_FILE_NAME,
        text: '{"extends":["./a.json","./ok.json"],"files":["root.ts"]}',
      },
      {
        path: "/project/a.json",
        text:
          '{"extends":"./tsconfig.json","compilerOptions":{"strict":true}}',
      },
      {
        path: "/project/ok.json",
        text:
          '{"compilerOptions":{"strict":false,"allowJs":true}}',
      },
    ],
    optionProbeKeys: ["strict", "allowJs"],
  },
  {
    id: "root-syntax-partial",
    rootText: MALFORMED_CONFIG,
    optionProbeKeys: ["strict", "allowJs"],
  },
  {
    id: "extended-syntax-atomic-sibling",
    rootText:
      '{"extends":["./bad.json","./ok.json"],"files":["root.ts"]}',
    hostFiles: [
      { path: "/project/bad.json", text: MALFORMED_CONFIG },
      {
        path: "/project/ok.json",
        text: '{"compilerOptions":{"allowJs":true}}',
      },
    ],
    optionProbeKeys: ["strict", "allowJs"],
  },
  {
    id: "mixed-invalid-spec-elements",
    rootText: `{
  "files": ["literal.ts", 1, null, "", "\${configDir}/a*/../template.ts"],
  "include": ["src/**", 7, "src/**/*.ts", "**/../escape/*.ts", "\${configDir}/a*/../src/**/*.ts"],
  "exclude": ["vendor/**", false, "**/../bad", "out", "\${configDir}/a*/../generated/**"]
}`,
    readDirectoryResult: ["/project/src/main.ts"],
  },
  {
    id: "empty-source-no-input",
    rootText: "",
  },
  {
    id: "empty-files",
    rootText: '{"files":[]}',
  },
  {
    id: "empty-include-no-input",
    rootText: '{"include":[]}',
  },
  {
    id: "duplicate-compiler-options",
    rootText:
      '{"compilerOptions":{"strict":true,"allowJs":"bad"},"compilerOptions":{"allowJs":true},"files":["x.ts"]}',
    optionProbeKeys: ["strict", "allowJs"],
  },
  {
    id: "duplicate-extends-last-effective",
    rootText:
      '{"extends":"./first-missing","extends":"./ok.json","files":["x.ts"]}',
    hostFiles: [
      {
        path: "/project/ok.json",
        text: '{"compilerOptions":{"strict":true}}',
      },
    ],
    optionProbeKeys: ["strict"],
  },
  {
    id: "omitted-files-element",
    rootText: '{"files":[,"x.ts"]}',
  },
  {
    id: "misplaced-root-compiler-option",
    rootText: '{"strict":true,"target":"es2020","files":["x.ts"]}',
    optionProbeKeys: ["strict", "target"],
  },
  {
    id: "typed-file-option-normalization",
    rootText:
      '{"compilerOptions":{"outDir":"./out","rootDir":"src"},"files":["x.ts"]}',
    optionProbeKeys: ["outDir", "rootDir"],
  },
  {
    id: "prototype-compiler-options-own-undefined-placement",
    rootText:
      '{"__proto__":{"compilerOptions":{}},"strict":true,"compilerOptions":,"files":["x.ts"]}',
    optionProbeKeys: ["strict"],
    rawOwnPropertyProbeKeys: ["compilerOptions", "strict", "files"],
  },
  {
    id: "prototype-files-own-undefined-inheritance",
    rootText:
      '{"extends":"./base.json","__proto__":{"files":["proto.ts"]},"files":}',
    hostFiles: [
      {
        path: "/project/base.json",
        text: '{"files":["base.ts"]}',
      },
    ],
    rawOwnPropertyProbeKeys: ["files"],
  },
  {
    id: "missing-command-line-option-value",
    rootText: '{"compilerOptions":{"help":},"files":["x.ts"]}',
    optionProbeKeys: ["help"],
    rawOwnPropertyProbeKeys: ["compilerOptions"],
  },
  {
    id: "extended-truthy-scalar-files",
    rootText: '{"extends":"./base.json"}',
    hostFiles: [
      {
        path: "/project/base.json",
        text: '{"files":true}',
      },
    ],
    rawOwnPropertyProbeKeys: ["files"],
  },
  {
    id: "extended-null-file-element",
    rootText: '{"extends":"./base.json"}',
    hostFiles: [
      {
        path: "/project/base.json",
        text: '{"files":[null]}',
      },
    ],
    rawOwnPropertyProbeKeys: ["files"],
  },
  {
    id: "unquoted-config-properties",
    rootText: '{compilerOptions:{strict:true},files:["x.ts"]}',
    optionProbeKeys: ["strict"],
  },
  {
    id: "single-quoted-config-properties",
    rootText: "{'compilerOptions':{'strict':true},'files':['x.ts']}",
    optionProbeKeys: ["strict"],
  },
  {
    id: "identifier-compiler-option-value",
    rootText: '{"compilerOptions":{"strict":foo},"files":["x.ts"]}',
    optionProbeKeys: ["strict"],
  },
  {
    id: "array-compiler-options",
    rootText:
      '{"compilerOptions":[true,{"strict":true}],"files":["x.ts"]}',
    optionProbeKeys: ["strict"],
  },
  {
    id: "invalid-module-option",
    rootText: '{"compilerOptions":{"module":"wat"},"files":["x.ts"]}',
    optionProbeKeys: ["module"],
  },
  {
    id: "unquoted-keyword-option-name",
    rootText: '{compilerOptions:{module:"esnext"},files:["x.ts"]}',
    optionProbeKeys: ["module"],
  },
  {
    id: "typed-rooted-path-options",
    rootText:
      '{"compilerOptions":{"outDir":"//server/share/out","rootDir":"file:///tmp/src"},"files":["x.ts"]}',
    optionProbeKeys: ["outDir", "rootDir"],
  },
  {
    id: "quoted-array-root-cycle-suffix-order",
    rootText: '{"extends":"./a.json","files":["x.ts"]}',
    hostFiles: [
      {
        path: "/project/a.json",
        text: "[{'extends':'./a.json'},{'note':'x'}]",
      },
    ],
  },
  {
    id: "file-url-parent-normalization",
    rootText:
      '{"compilerOptions":{"rootDir":"file:///c:/../x"},"files":["x.ts"]}',
    optionProbeKeys: ["rootDir"],
  },
  {
    id: "file-url-parent-boundary",
    rootText:
      '{"compilerOptions":{"rootDir":"file:///c:/.."},"files":["x.ts"]}',
    optionProbeKeys: ["rootDir"],
  },
  {
    id: "keyword-compiler-option-value",
    rootText: '{"compilerOptions":{"strict":module},"files":["x.ts"]}',
    optionProbeKeys: ["strict"],
  },
  {
    id: "path-option-normalization-boundaries",
    rootText:
      '{"compilerOptions":{"outDir":"/a//","rootDir":"/a/.//","declarationDir":"c:","mapRoot":"foo_bar://h/a"},"files":["x.ts"]}',
    optionProbeKeys: ["outDir", "rootDir", "declarationDir", "mapRoot"],
  },
  {
    id: "array-compiler-option-value",
    rootText: '{"compilerOptions":{"strict":[foo]},"files":["x.ts"]}',
    optionProbeKeys: ["strict"],
  },
  {
    id: "extended-self-cycle-conversion-replay",
    rootText: '{"extends":"./a.json","files":["x.ts"]}',
    hostFiles: [
      {
        path: "/project/a.json",
        text: '{"extends":"./a.json","note":foo}',
      },
    ],
  },
  {
    id: "unknown-option-single-quoted-value",
    rootText: '{"compilerOptions":{"wat":\'x\'},"files":["x.ts"]}',
    optionProbeKeys: ["wat"],
  },
  {
    id: "command-line-only-string-value",
    rootText: '{"compilerOptions":{"help":\'x\'},"files":["x.ts"]}',
    optionProbeKeys: ["help"],
  },
  {
    id: "object-compiler-option-value",
    rootText:
      '{"compilerOptions":{"strict":{"x":\'y\'}},"files":["x.ts"]}',
    optionProbeKeys: ["strict"],
  },
  {
    id: "file-spec-diagnostic-owner-order",
    rootText: '{"files":[foo,{"x":\'y\'}]}',
  },
  {
    id: "compiler-list-valid-conversions",
    rootText:
      '{"compilerOptions":{"lib":["ES5","DOM"],"rootDirs":["src",""],"typeRoots":["types","../shared"],"types":["node","jest"],"moduleSuffixes":[".ios",""],"customConditions":["development","browser"],"plugins":[{"name":"example"},[]]},"files":["x.ts"]}',
    optionProbeKeys: [
      "lib",
      "rootDirs",
      "typeRoots",
      "types",
      "moduleSuffixes",
      "customConditions",
      "plugins",
    ],
  },
  {
    id: "compiler-list-outer-wrong-kinds",
    rootText:
      '{"compilerOptions":{"lib":true,"rootDirs":{},"typeRoots":"types","types":false,"moduleSuffixes":0,"customConditions":true,"plugins":"plugin"},"files":["x.ts"]}',
    optionProbeKeys: [
      "lib",
      "rootDirs",
      "typeRoots",
      "types",
      "moduleSuffixes",
      "customConditions",
      "plugins",
    ],
  },
  {
    id: "compiler-list-top-level-null",
    rootText:
      '{"compilerOptions":{"lib":null,"rootDirs":null,"typeRoots":null,"types":null,"moduleSuffixes":null,"customConditions":null,"plugins":null},"files":["x.ts"]}',
    optionProbeKeys: [
      "lib",
      "rootDirs",
      "typeRoots",
      "types",
      "moduleSuffixes",
      "customConditions",
      "plugins",
    ],
  },
  {
    id: "compiler-list-mixed-invalid-elements",
    rootText:
      '{"compilerOptions":{"lib":["ES5",foo,"wat",false,"DOM"],"types":["node",foo,false,"jest"],"moduleSuffixes":[".ios",null,false,foo,""],"plugins":[{},[],null,false,foo,{"name":bar}]},"files":["x.ts"]}',
    optionProbeKeys: ["lib", "types", "moduleSuffixes", "plugins"],
  },
  {
    id: "compiler-list-extends-path-bases",
    rootText: '{"extends":"./config/base.json","files":["x.ts"]}',
    hostFiles: [
      {
        path: "/project/config/base.json",
        text:
          '{"compilerOptions":{"rootDirs":["src","${configDir}/generated"],"typeRoots":["types","${configDir}/ambient"]}}',
      },
    ],
    optionProbeKeys: ["rootDirs", "typeRoots"],
  },
  {
    id: "compiler-list-extends-replace-and-null-mask",
    rootText:
      '{"extends":"./base.json","compilerOptions":{"lib":["DOM"],"rootDirs":["own"],"typeRoots":null,"types":["own"],"moduleSuffixes":[".own"],"customConditions":null,"plugins":[[]]},"files":["x.ts"]}',
    hostFiles: [
      {
        path: "/project/base.json",
        text:
          '{"compilerOptions":{"lib":["ES5"],"rootDirs":["base"],"typeRoots":["base-types"],"types":["base"],"moduleSuffixes":[".base"],"customConditions":["base"],"plugins":[{}]}}',
      },
    ],
    optionProbeKeys: [
      "lib",
      "rootDirs",
      "typeRoots",
      "types",
      "moduleSuffixes",
      "customConditions",
      "plugins",
    ],
  },
  {
    id: "compiler-object-paths-valid-nested-recovery",
    rootText:
      '{"compilerOptions":{"paths":{"@/*":["./src/*",foo,"./fallback/*"],"drop":foo,"keep":true,"numbers":[1,-0],"negativeZero":-0}},"files":["x.ts"]}',
    optionProbeKeys: ["paths", "pathsBasePath"],
  },
  {
    id: "compiler-object-paths-extends-config-dir",
    rootText:
      '{"extends":"./config/base.json","files":["x.ts"]}',
    hostFiles: [
      {
        path: "/project/config/base.json",
        text:
          '{"compilerOptions":{"paths":{"@/*":["./src/*","${configDir}/a*/../generated/*","${confıgDir}/unicode/*"],"plain":true},"rootDirs":["${configDir}/a*/../roots/*","${confıgDir}/unicode-roots"],"outDir":"${configDir}/a*/../dist","declarationDir":"${confıgDir}/unicode-declarations","rootDir":"${conﬁgDir}"}}',
      },
    ],
    optionProbeKeys: [
      "paths",
      "pathsBasePath",
      "rootDirs",
      "outDir",
      "declarationDir",
      "rootDir",
    ],
  },
  {
    id: "compiler-object-paths-array-unchanged",
    rootText:
      '{"compilerOptions":{"paths":[["./src/*"],{"nested":true},"plain",[]]},"files":["x.ts"]}',
    optionProbeKeys: ["paths", "pathsBasePath"],
  },
  {
    id: "compiler-object-paths-array-template-object",
    rootText:
      '{"compilerOptions":{"paths":[["${configDir}/src/*"],["./plain/*"],true]},"files":["x.ts"]}',
    optionProbeKeys: ["paths", "pathsBasePath"],
  },
  {
    id: "compiler-object-paths-null-own-mask",
    rootText:
      '{"extends":"./config/base.json","compilerOptions":{"paths":null},"files":["x.ts"]}',
    hostFiles: [
      {
        path: "/project/config/base.json",
        text:
          '{"compilerOptions":{"paths":{"base/*":["./src/*"]}}}',
      },
    ],
    optionProbeKeys: ["paths", "pathsBasePath"],
  },
  {
    id: "compiler-object-paths-invalid-own-mask",
    rootText:
      '{"extends":"./config/base.json","compilerOptions":{"paths":true},"files":["x.ts"]}',
    hostFiles: [
      {
        path: "/project/config/base.json",
        text:
          '{"compilerOptions":{"paths":{"base/*":["./src/*"]}}}',
      },
    ],
    optionProbeKeys: ["paths", "pathsBasePath"],
  },
];

const EXPECTED_SUMMARY = {
  fixture_total: 51,
  root_parse_diagnostic_total: 4,
  parsed_error_total: 86,
  config_diagnostic_total: 90,
  located_config_diagnostic_total: 83,
  file_name_total: 49,
  extended_source_total: 21,
  extended_source_text_total: 19,
  host_call_total: 56,
  host_calls: {
    file_exists: 31,
    read_file: 23,
    read_directory: 2,
  },
};

function fail(message) {
  throw new Error(message);
}

function requireCondition(condition, message) {
  if (!condition) fail(message);
}

function sha256(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

function jsonEqual(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function requireJsonEqual(actual, expected, label) {
  requireCondition(
    jsonEqual(actual, expected),
    `${label} mismatch:\nactual: ${JSON.stringify(actual)}\nexpected: ${JSON.stringify(expected)}`,
  );
}

function normalizeVersion(version) {
  return version.startsWith("v") ? version.slice(1) : version;
}

function validateRuntime() {
  const recordedNodeVersion = fs.readFileSync(NODE_VERSION_PATH, "utf8").trim();
  requireCondition(
    recordedNodeVersion === EXPECTED_NODE_VERSION,
    `.node-version is ${JSON.stringify(recordedNodeVersion)}; expected ${EXPECTED_NODE_VERSION}`,
  );
  const runningNodeVersion = normalizeVersion(process.version);
  requireCondition(
    runningNodeVersion === recordedNodeVersion,
    `compiler config diagnostic oracle requires Node ${recordedNodeVersion}; running ${runningNodeVersion}`,
  );
  for (const [relativePath, absolutePath, expectedHash] of [
    [
      TYPESCRIPT_BUNDLE_RELATIVE_PATH,
      TYPESCRIPT_BUNDLE_PATH,
      EXPECTED_TYPESCRIPT_BUNDLE_SHA256,
    ],
    [
      TYPESCRIPT_SOURCE_RELATIVE_PATH,
      TYPESCRIPT_SOURCE_PATH,
      EXPECTED_TYPESCRIPT_SOURCE_SHA256,
    ],
  ]) {
    const actualHash = sha256(fs.readFileSync(absolutePath));
    requireCondition(
      actualHash === expectedHash,
      `${relativePath} SHA-256 is ${actualHash}; expected ${expectedHash}`,
    );
  }
}

function validateTypeScriptRuntime() {
  requireCondition(
    ts?.version === EXPECTED_TYPESCRIPT_VERSION,
    `vendored TypeScript reports ${ts?.version}; expected ${EXPECTED_TYPESCRIPT_VERSION}`,
  );
}

function isPlainObject(value) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

function jsonValue(
  value,
  label,
  ancestors = new Set(),
  allowCustomPrototype = false,
) {
  if (value === null || typeof value === "string" || typeof value === "boolean") {
    return value;
  }
  if (typeof value === "number") {
    requireCondition(Number.isFinite(value), `${label} contains a non-finite number`);
    return value;
  }
  requireCondition(value !== undefined, `${label} contains undefined`);
  requireCondition(typeof value === "object", `${label} is not JSON data`);
  requireCondition(!ancestors.has(value), `${label} contains a cycle`);
  ancestors.add(value);
  let result;
  if (Array.isArray(value)) {
    result = value.map((entry, index) =>
      jsonValue(entry, `${label}[${index}]`, ancestors),
    );
  } else {
    requireCondition(
      allowCustomPrototype || isPlainObject(value),
      `${label} contains a non-plain object`,
    );
    result = Object.create(null);
    for (const key of Object.keys(value)) {
      // JSON cannot carry an own property whose value is JavaScript
      // `undefined`. The public Rust raw projection omits it as well; the
      // fixture-specific raw own-property probes below retain that state.
      if (value[key] === undefined) continue;
      result[key] = jsonValue(value[key], `${label}.${key}`, ancestors);
    }
  }
  ancestors.delete(value);
  return result;
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
      fail(`unknown diagnostic category ${String(category)}`);
  }
}

function diagnosticRecord(diagnostic, label) {
  requireCondition(
    typeof diagnostic.messageText === "string",
    `${label} unexpectedly has a diagnostic message chain`,
  );
  return {
    code: diagnostic.code,
    category: diagnosticCategoryName(diagnostic.category),
    file: diagnostic.file?.fileName ?? null,
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
    message: diagnostic.messageText,
  };
}

function sourceTextRecord(fileName, text) {
  const bytes = Buffer.from(text, "utf8");
  return {
    file_name: fileName,
    text,
    utf8_bytes: bytes.length,
    sha256: sha256(bytes),
  };
}

function inputHostFileRecord(hostFile) {
  const read = hostFile.readError
    ? { state: "error", detail: hostFile.readError }
    : { state: "text", source: sourceTextRecord(hostFile.path, hostFile.text) };
  return {
    path: hostFile.path,
    file_exists: true,
    read,
  };
}

function validateFixtureDefinitions() {
  const ids = new Set();
  for (const [index, fixture] of FIXTURES.entries()) {
    const label = `fixture definition ${index}`;
    requireCondition(
      typeof fixture.id === "string" && fixture.id.length > 0,
      `${label} id must be a non-empty string`,
    );
    requireCondition(!ids.has(fixture.id), `${label} id is duplicated`);
    ids.add(fixture.id);
    requireCondition(typeof fixture.rootText === "string", `${label} rootText must be a string`);
    const paths = new Set();
    for (const [hostIndex, hostFile] of (fixture.hostFiles ?? []).entries()) {
      const hostLabel = `${label} host file ${hostIndex}`;
      requireCondition(
        typeof hostFile.path === "string" && hostFile.path.startsWith("/"),
        `${hostLabel} path must be absolute`,
      );
      requireCondition(!paths.has(hostFile.path), `${hostLabel} path is duplicated`);
      paths.add(hostFile.path);
      requireCondition(
        (typeof hostFile.text === "string") !==
          (typeof hostFile.readError === "string"),
        `${hostLabel} must define exactly one of text or readError`,
      );
    }
    requireCondition(
      (fixture.readDirectoryResult ?? []).every(
        (fileName) => typeof fileName === "string" && fileName.startsWith("/"),
      ),
      `${label} readDirectoryResult entries must be absolute paths`,
    );
    const probeKeys = fixture.optionProbeKeys ?? [];
    requireCondition(
      probeKeys.every((key) => typeof key === "string" && key.length > 0) &&
        new Set(probeKeys).size === probeKeys.length,
      `${label} option probe keys must be unique non-empty strings`,
    );
    const rawProbeKeys = fixture.rawOwnPropertyProbeKeys ?? [];
    requireCondition(
      rawProbeKeys.every((key) => typeof key === "string" && key.length > 0) &&
        new Set(rawProbeKeys).size === rawProbeKeys.length,
      `${label} raw own-property probe keys must be unique non-empty strings`,
    );
  }
}

function createParseConfigHost(fixture) {
  const files = new Map(
    (fixture.hostFiles ?? []).map((hostFile) => [hostFile.path, hostFile]),
  );
  const log = [];
  return {
    host: {
      useCaseSensitiveFileNames: true,
      fileExists(fileName) {
        const result = files.has(fileName);
        log.push({ operation: "file_exists", path: fileName, result });
        return result;
      },
      readFile(fileName) {
        const file = files.get(fileName);
        if (!file) {
          log.push({ operation: "read_file", path: fileName, result: "missing" });
          return undefined;
        }
        if (file.readError !== undefined) {
          log.push({ operation: "read_file", path: fileName, result: "error" });
          throw new Error(file.readError);
        }
        log.push({ operation: "read_file", path: fileName, result: "text" });
        return file.text;
      },
      readDirectory(directory, extensions, excludes, includes, depth) {
        const result = [...(fixture.readDirectoryResult ?? [])];
        log.push({
          operation: "read_directory",
          directory,
          extensions: [...extensions],
          excludes: excludes === undefined ? null : [...excludes],
          includes: includes === undefined ? null : [...includes],
          depth: depth ?? null,
          result,
        });
        return result;
      },
    },
    log,
  };
}

function optionProbeRecord(options, names, preserveListElementState = false) {
  return names.map((name) => {
    if (!Object.hasOwn(options, name)) return { name, state: "absent" };
    const value = options[name];
    if (value === undefined) return { name, state: "undefined" };
    if (
      preserveListElementState &&
      COMPILER_LIST_OPTION_NAMES.has(name) &&
      Array.isArray(value)
    ) {
      return {
        name,
        state: "list",
        elements: value.map((element, index) =>
          element === undefined
            ? { state: "undefined" }
            : {
                state: "value",
                value: jsonValue(
                  element,
                  `compiler option ${name} element ${index}`,
                ),
              },
        ),
      };
    }
    return {
      name,
      state: "value",
      value: jsonValue(value, `compiler option ${name}`),
    };
  });
}

function buildFixture(fixture) {
  const { host, log } = createParseConfigHost(fixture);
  const sourceFile = ts.parseJsonText(ROOT_FILE_NAME, fixture.rootText);
  const parsed = ts.parseJsonSourceFileConfigFileContent(
    sourceFile,
    host,
    ROOT_BASE_PATH,
    undefined,
    ROOT_FILE_NAME,
  );
  const rootParseDiagnostics = sourceFile.parseDiagnostics.map((diagnostic, index) =>
    diagnosticRecord(diagnostic, `${fixture.id} root parse diagnostic ${index}`),
  );
  const parsedErrors = parsed.errors.map((diagnostic, index) =>
    diagnosticRecord(diagnostic, `${fixture.id} parsed error ${index}`),
  );
  const configDiagnostics = ts
    .getConfigFileParsingDiagnostics(parsed)
    .map((diagnostic, index) =>
      diagnosticRecord(diagnostic, `${fixture.id} config diagnostic ${index}`),
    );
  requireJsonEqual(
    configDiagnostics,
    [...rootParseDiagnostics, ...parsedErrors],
    `${fixture.id} config diagnostic partition`,
  );
  const extendedSourceFiles = [
    ...(parsed.options.configFile?.extendedSourceFiles ?? []),
  ];
  const hostFiles = new Map(
    (fixture.hostFiles ?? []).map((hostFile) => [hostFile.path, hostFile]),
  );
  const extendedSources = [];
  for (const fileName of extendedSourceFiles) {
    const hostFile = hostFiles.get(fileName);
    if (hostFile?.text !== undefined) {
      extendedSources.push(sourceTextRecord(fileName, hostFile.text));
    }
  }

  return {
    id: fixture.id,
    input: {
      root: sourceTextRecord(ROOT_FILE_NAME, fixture.rootText),
      base_path: ROOT_BASE_PATH,
      use_case_sensitive_file_names: true,
      host_files: (fixture.hostFiles ?? []).map(inputHostFileRecord),
      read_directory_result: [...(fixture.readDirectoryResult ?? [])],
      option_probe_keys: [...(fixture.optionProbeKeys ?? [])],
      raw_own_property_probe_keys: [
        ...(fixture.rawOwnPropertyProbeKeys ?? []),
      ],
    },
    root_parse_diagnostics: rootParseDiagnostics,
    parsed_errors: parsedErrors,
    config_diagnostics: configDiagnostics,
    plan: {
      raw: jsonValue(
        parsed.raw,
        `${fixture.id} raw config`,
        new Set(),
        true,
      ),
      file_names: [...parsed.fileNames],
      extended_sources: extendedSources,
      extended_source_files: extendedSourceFiles,
      option_probes: optionProbeRecord(
        parsed.options,
        fixture.optionProbeKeys ?? [],
        true,
      ),
      raw_own_property_probes: optionProbeRecord(
        parsed.raw,
        fixture.rawOwnPropertyProbeKeys ?? [],
      ),
    },
    host_log: log,
  };
}

function summarize(fixtures) {
  const summary = {
    fixture_total: fixtures.length,
    root_parse_diagnostic_total: 0,
    parsed_error_total: 0,
    config_diagnostic_total: 0,
    located_config_diagnostic_total: 0,
    file_name_total: 0,
    extended_source_total: 0,
    extended_source_text_total: 0,
    host_call_total: 0,
    host_calls: {
      file_exists: 0,
      read_file: 0,
      read_directory: 0,
    },
  };
  for (const fixture of fixtures) {
    summary.root_parse_diagnostic_total += fixture.root_parse_diagnostics.length;
    summary.parsed_error_total += fixture.parsed_errors.length;
    summary.config_diagnostic_total += fixture.config_diagnostics.length;
    summary.located_config_diagnostic_total += fixture.config_diagnostics.filter(
      (diagnostic) => diagnostic.file !== null,
    ).length;
    summary.file_name_total += fixture.plan.file_names.length;
    summary.extended_source_total += fixture.plan.extended_source_files.length;
    summary.extended_source_text_total += fixture.plan.extended_sources.length;
    summary.host_call_total += fixture.host_log.length;
    for (const call of fixture.host_log) {
      summary.host_calls[call.operation] += 1;
    }
  }
  return summary;
}

function generateArtifact() {
  validateRuntime();
  validateTypeScriptRuntime();
  validateFixtureDefinitions();
  const fixtures = FIXTURES.map(buildFixture);
  const summary = summarize(fixtures);
  requireJsonEqual(summary, EXPECTED_SUMMARY, "compiler config diagnostic oracle summary");
  return {
    schema: 1,
    typescript_version: EXPECTED_TYPESCRIPT_VERSION,
    source_commit: EXPECTED_SOURCE_COMMIT,
    node_version: EXPECTED_NODE_VERSION,
    producer: {
      path: TYPESCRIPT_BUNDLE_RELATIVE_PATH,
      sha256: EXPECTED_TYPESCRIPT_BUNDLE_SHA256,
    },
    source_reference: {
      path: TYPESCRIPT_SOURCE_RELATIVE_PATH,
      sha256: EXPECTED_TYPESCRIPT_SOURCE_SHA256,
      spans: [
        { symbol: "equateStringsCaseInsensitive", lines: "905-906" },
        { symbol: "startsWith", lines: "1078-1079" },
        { symbol: "getPathsBasePath", lines: "16595-16599" },
        { symbol: "paths option declaration", lines: "37363-37374" },
        { symbol: "convertToJson", lines: "38521-38600" },
        { symbol: "isCompilerOptionsValue", lines: "38604-38617" },
        { symbol: "parseJsonConfigFileContentWorker", lines: "39004-39171" },
        {
          symbol: "handleOptionConfigDirTemplateSubstitution",
          lines: "39175-39207",
        },
        {
          symbol: "getSubstitutedPathWithConfigDirTemplate",
          lines: "39217-39219",
        },
        {
          symbol: "getSubstitutedStringArrayWithConfigDirTemplate",
          lines: "39220-39228",
        },
        {
          symbol: "getSubstitutedMapLikeOfStringArrayWithConfigDirTemplate",
          lines: "39229-39239",
        },
        { symbol: "parseConfig", lines: "39272-39330" },
        { symbol: "getExtendsConfigPathOrArray", lines: "39342-39378" },
        { symbol: "getExtendsConfigPath", lines: "39436-39459" },
        { symbol: "getExtendedConfig", lines: "39460-39499" },
        { symbol: "convertJsonOption", lines: "39555-39574" },
        {
          symbol: "convertJsonOptionOfListType",
          lines: "39603-39605",
        },
        { symbol: "validateSpecs", lines: "39697-39710" },
        { symbol: "specToDiagnostic", lines: "39711-39718" },
        { symbol: "compilerOptionValueToString", lines: "40327-40341" },
      ],
    },
    summary,
    fixtures,
  };
}

function renderArtifact() {
  return `${JSON.stringify(generateArtifact(), null, 2)}\n`;
}

function checkRecordedArtifact(rendered) {
  const expected = Buffer.from(rendered, "utf8");
  const recorded = fs.readFileSync(ARTIFACT_PATH);
  if (recorded.equals(expected)) return;
  let firstDifference = 0;
  while (
    firstDifference < recorded.length &&
    firstDifference < expected.length &&
    recorded[firstDifference] === expected[firstDifference]
  ) {
    firstDifference += 1;
  }
  fail(
    `${ARTIFACT_RELATIVE_PATH} is stale at byte ${firstDifference} ` +
      `(recorded sha256 ${sha256(recorded)}, generated sha256 ${sha256(expected)}); ` +
      `regenerate with: node crates/oracle/compiler-config-diagnostics.mjs > ${ARTIFACT_RELATIVE_PATH}`,
  );
}

const arguments_ = process.argv.slice(2);
requireCondition(
  arguments_.length === 0 ||
    (arguments_.length === 1 &&
      (arguments_[0] === "--check" || arguments_[0] === "--write")),
  "usage: node crates/oracle/compiler-config-diagnostics.mjs [--check|--write]",
);
validateRuntime();
const typescriptModule = await import(pathToFileURL(TYPESCRIPT_BUNDLE_PATH).href);
ts = typescriptModule.default;
validateTypeScriptRuntime();
const rendered = renderArtifact();
if (arguments_[0] === "--check") {
  checkRecordedArtifact(rendered);
  process.stdout.write(`${ARTIFACT_RELATIVE_PATH} is current\n`);
} else if (arguments_[0] === "--write") {
  fs.writeFileSync(ARTIFACT_PATH, rendered);
  process.stdout.write(`${ARTIFACT_RELATIVE_PATH} refreshed\n`);
} else {
  process.stdout.write(rendered);
}
