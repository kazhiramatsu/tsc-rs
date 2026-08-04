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
const TEST_SUITE_MANIFEST_RELATIVE_PATH =
  "vendor/typescript-6.0.3/test-suite-expansion.v1.json";
const TEST_SUITE_MANIFEST_PATH = path.join(
  WORKSPACE,
  TEST_SUITE_MANIFEST_RELATIVE_PATH,
);
const COMPILER_CASES_RELATIVE_PATH = "ts-tests/tests/cases/compiler";
const COMPILER_CASES_PATH = path.join(WORKSPACE, COMPILER_CASES_RELATIVE_PATH);

const EXPECTED_NODE_VERSION = "25.2.1";
const EXPECTED_TYPESCRIPT_VERSION = "6.0.3";
const EXPECTED_SOURCE_COMMIT =
  "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_TYPESCRIPT_BUNDLE_SHA256 =
  "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const EXPECTED_TYPESCRIPT_SOURCE_SHA256 =
  "1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3";
const EXPECTED_TEST_SUITE_MANIFEST_SHA256 =
  "9c6e991103b571f7a8800dc5e1ef66088017689f3769de8fcdb408b2dc125188";
const ROOT_FILE_NAME = "/project/tsconfig.json";
const ROOT_BASE_PATH = "/project";
const VIRTUAL_SOURCE_ROOT = "/.src";

// These are the two regular expressions used by harnessIO.ts. Both are global,
// so every single-line probe must restore lastIndex before the next probe.
const OPTION_REGEX = /^\/{2}\s*@(\w+)\s*:\s*([^\r\n]*)/gm;
const LINK_REGEX = /^\/{2}\s*@link\s*:\s*([^\r\n]*)\s*->\s*([^\r\n]*)/gm;

const PATHS_VALIDATION_CODES = new Set([5061, 5062, 5063, 5064, 5066, 5090]);
const PATHS_VALIDATION_CASES = [
  {
    source_index: 4978,
    path: "pathsValidation1.ts",
    bytes: 196,
    sha256: "c5262b1ee716c7333ba98ee2e909269ddd2ae2212ee6269ecd78e0d3560b28b7",
    git_blob_sha1: "5e1943d1a999e88d5ad2fc833a341a26e99365b5",
    expected_codes: [5063],
  },
  {
    source_index: 4979,
    path: "pathsValidation2.ts",
    bytes: 196,
    sha256: "79c03cdfcf5953cea36bd99642408574eb0637e74c59dc430eeeb5edb301bff0",
    git_blob_sha1: "4beabd8b6f49f10c64566636e313a3694f9d3fbb",
    expected_codes: [5064],
  },
  {
    source_index: 4980,
    path: "pathsValidation3.ts",
    bytes: 199,
    sha256: "ea33dbd7563b0b79a40aeb2f88a0c86832c6acf7b670d5152accc169912851ad",
    git_blob_sha1: "4e4ef9ff09172ef36f44ac33072fb05b96b47726",
    expected_codes: [5066],
  },
  {
    source_index: 4981,
    path: "pathsValidation4.ts",
    bytes: 433,
    sha256: "331ecaa940480ed702fd4b6654ea1218e2be98daee834c52609fcd9101029581",
    git_blob_sha1: "fdc4105ecba62a7c1848c76ca0ff178fee6f135b",
    expected_codes: [5061, 5061, 5062],
  },
  {
    source_index: 4982,
    path: "pathsValidation5.ts",
    bytes: 339,
    sha256: "583094876be6dc31e7de0dc91062715c9fb95186bd39c04568ac1a26c3965019",
    git_blob_sha1: "3c81bc579f0d5398847c801258f72ee8ace3e84b",
    expected_codes: [5090, 5090, 5090],
  },
];

const EXPECTED_PATHS_VALIDATION_SUMMARY = {
  fixture_total: 5,
  options_diagnostic_total: 9,
  located_options_diagnostic_total: 9,
  diagnostics_by_code: [
    { code: 5061, count: 2 },
    { code: 5062, count: 1 },
    { code: 5063, count: 1 },
    { code: 5064, count: 1 },
    { code: 5066, count: 1 },
    { code: 5090, count: 3 },
  ],
};

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

function gitBlobSha1(bytes) {
  return crypto
    .createHash("sha1")
    .update(`blob ${bytes.length}\0`)
    .update(bytes)
    .digest("hex");
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
    [
      TEST_SUITE_MANIFEST_RELATIVE_PATH,
      TEST_SUITE_MANIFEST_PATH,
      EXPECTED_TEST_SUITE_MANIFEST_SHA256,
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

function readPathsValidationManifest() {
  const bytes = fs.readFileSync(TEST_SUITE_MANIFEST_PATH);
  requireCondition(
    sha256(bytes) === EXPECTED_TEST_SUITE_MANIFEST_SHA256,
    `${TEST_SUITE_MANIFEST_RELATIVE_PATH} does not match its SHA-256 pin`,
  );
  const manifest = JSON.parse(bytes.toString("utf8"));
  requireCondition(manifest.schema === 1, "test suite expansion schema must be 1");
  requireCondition(
    manifest.typescript_version === EXPECTED_TYPESCRIPT_VERSION,
    "test suite expansion TypeScript version does not match the oracle pin",
  );
  requireCondition(
    manifest.source_commit === EXPECTED_SOURCE_COMMIT,
    "test suite expansion source commit does not match the oracle pin",
  );
  requireCondition(
    manifest.virtual_source_root === VIRTUAL_SOURCE_ROOT,
    `test suite expansion virtual root must be ${VIRTUAL_SOURCE_ROOT}`,
  );
  requireCondition(
    Array.isArray(manifest.sources) && Array.isArray(manifest.compiler_fixtures),
    "test suite expansion manifest is missing compiler inventories",
  );
  const compilerSuites = manifest.corpus_pin?.suites?.filter(
    (suite) => suite.name === "compiler",
  );
  requireCondition(
    compilerSuites?.length === 1 &&
      compilerSuites[0].vendored_path === COMPILER_CASES_RELATIVE_PATH,
    `test suite expansion compiler suite must be ${COMPILER_CASES_RELATIVE_PATH}`,
  );
  return manifest;
}

function safeCompilerCasePath(relativePath) {
  requireCondition(
    typeof relativePath === "string" &&
      relativePath.length > 0 &&
      relativePath === path.posix.normalize(relativePath) &&
      !relativePath.startsWith("/") &&
      !relativePath.startsWith("../"),
    `compiler case path is not normalized and relative: ${JSON.stringify(relativePath)}`,
  );
  const resolved = path.resolve(COMPILER_CASES_PATH, relativePath);
  requireCondition(
    resolved.startsWith(`${path.resolve(COMPILER_CASES_PATH)}${path.sep}`),
    `compiler case path escaped its suite: ${JSON.stringify(relativePath)}`,
  );
  return resolved;
}

function matchOnce(regex, line) {
  const match = regex.exec(line);
  regex.lastIndex = 0;
  return match;
}

function makeUnitsFromTest(code, fixturePath) {
  const units = [];
  const links = [];
  let currentFileContent;
  let currentFileOptions = {};
  let currentFileName;

  for (const line of code.split(/\r?\n/)) {
    const link = matchOnce(LINK_REGEX, line);
    if (link) {
      links.push({ target: link[1].trim(), link_path: link[2].trim() });
      continue;
    }

    const option = matchOnce(OPTION_REGEX, line);
    if (option) {
      const name = option[1];
      const value = option[2].trim();
      currentFileOptions[name] = value;
      if (name.toLowerCase() !== "filename") continue;

      if (currentFileName) {
        units.push({
          name: currentFileName,
          content: currentFileContent,
          fileOptions: currentFileOptions,
        });
        currentFileContent = undefined;
        currentFileOptions = {};
        currentFileName = value;
      } else {
        currentFileName = value;
        if (
          currentFileContent &&
          ts.skipTrivia(currentFileContent, 0, false, false) !==
            currentFileContent.length
        ) {
          fail(
            `compiler fixture ${JSON.stringify(fixturePath)} contains non-comment content before its first @filename directive`,
          );
        }
        currentFileContent = "";
      }
      continue;
    }

    if (currentFileContent === undefined) currentFileContent = "";
    else if (currentFileContent !== "") currentFileContent += "\n";
    currentFileContent += line;
  }

  currentFileName =
    units.length > 0 || currentFileName
      ? currentFileName
      : ts.getBaseFileName(fixturePath);
  units.push({
    name: currentFileName,
    content: currentFileContent || "",
    fileOptions: currentFileOptions,
  });
  return { units, links };
}

function isConfigFileName(fileName) {
  const baseName = ts.getBaseFileName(fileName).toLowerCase();
  return baseName === "tsconfig.json" || baseName === "jsconfig.json";
}

function recordedUnitContent(unit) {
  if (unit.content === undefined) return { state: "missing" };
  const bytes = Buffer.from(unit.content, "utf8");
  return {
    state: "present",
    utf8_bytes: bytes.length,
    sha256: sha256(bytes),
  };
}

function recordedDocumentSymlinks(fileOptions) {
  const symlink = Object.entries(fileOptions).find(([name]) => name === "symlink");
  if (!symlink || symlink[1] === "") return [];
  return symlink[1].split(",").map((entry) => entry.trim());
}

function verifyExpandedUnit(unit, expected, label) {
  requireCondition(isPlainObject(expected), `${label} is absent from the manifest`);
  requireCondition(unit.name === expected.name, `${label} name does not match`);
  requireJsonEqual(
    Object.entries(unit.fileOptions).map(([name, value]) => ({ name, value })),
    expected.file_options,
    `${label} file options`,
  );
  requireJsonEqual(recordedUnitContent(unit), expected.content, `${label} content`);
  requireJsonEqual(
    recordedDocumentSymlinks(unit.fileOptions),
    expected.document_symlinks,
    `${label} document symlinks`,
  );
}

function verifyPathsValidationExpansion(recorded, source, units, links, configIndex) {
  requireCondition(
    units.length === recorded.normal_units.length + 1,
    `${source.path} unit count does not match the expansion manifest`,
  );
  let normalIndex = 0;
  for (const [unitIndex, unit] of units.entries()) {
    const expected =
      unitIndex === configIndex
        ? recorded.virtual_config
        : recorded.normal_units[normalIndex++];
    verifyExpandedUnit(unit, expected, `${source.path} unit occurrence ${unitIndex}`);
  }
  requireCondition(
    normalIndex === recorded.normal_units.length,
    `${source.path} normal unit partition does not match the expansion manifest`,
  );
  requireJsonEqual(links, recorded.links, `${source.path} @link directives`);
}

function absoluteUnitName(unit) {
  return ts.getNormalizedAbsolutePath(unit.name, VIRTUAL_SOURCE_ROOT);
}

function createPathsValidationParseHost(units) {
  const unitMap = new Map(
    units.map((unit) => [absoluteUnitName(unit).toLowerCase(), unit]),
  );
  return {
    useCaseSensitiveFileNames: false,
    readDirectory(directory, extensions, excludes, includes, depth) {
      return ts.matchFiles(
        directory,
        extensions,
        excludes,
        includes,
        false,
        "",
        depth,
        (dir) => {
          const files = [];
          const directories = new Set();
          const canonicalDirectory = dir.toLowerCase();
          for (const unit of units) {
            const fileName = absoluteUnitName(unit);
            if (!fileName.toLowerCase().startsWith(canonicalDirectory)) continue;
            let relative = fileName.substring(dir.length);
            if (relative.startsWith("/")) relative = relative.substring(1);
            const separator = relative.indexOf("/");
            if (separator >= 0) directories.add(relative.substring(0, separator));
            else files.push(relative);
          }
          return { files, directories: ts.arrayFrom(directories) };
        },
        ts.identity,
      );
    },
    fileExists(fileName) {
      return unitMap.has(
        ts.getNormalizedAbsolutePath(fileName, VIRTUAL_SOURCE_ROOT).toLowerCase(),
      );
    },
    readFile(fileName) {
      return unitMap.get(
        ts.getNormalizedAbsolutePath(fileName, VIRTUAL_SOURCE_ROOT).toLowerCase(),
      )?.content;
    },
  };
}

function createPathsValidationCompilerHost(units, options) {
  const host = ts.createCompilerHost(options, true);
  const unitMap = new Map(
    units.map((unit) => [absoluteUnitName(unit).toLowerCase(), unit]),
  );
  const defaultFileExists = host.fileExists;
  const defaultReadFile = host.readFile;
  const defaultGetSourceFile = host.getSourceFile;
  const findUnit = (fileName) =>
    unitMap.get(
      ts.getNormalizedAbsolutePath(fileName, VIRTUAL_SOURCE_ROOT).toLowerCase(),
    );

  host.getCurrentDirectory = () => VIRTUAL_SOURCE_ROOT;
  host.useCaseSensitiveFileNames = () => false;
  host.getCanonicalFileName = (fileName) => fileName.toLowerCase();
  host.fileExists = (fileName) =>
    findUnit(fileName) !== undefined || defaultFileExists(fileName);
  host.readFile = (fileName) => findUnit(fileName)?.content ?? defaultReadFile(fileName);
  host.getSourceFile = (
    fileName,
    languageVersion,
    onError,
    shouldCreateNewSourceFile,
  ) => {
    const unit = findUnit(fileName);
    return unit
      ? ts.createSourceFile(
          fileName,
          unit.content,
          languageVersion,
          true,
          ts.getScriptKindFromFileName(fileName),
        )
      : defaultGetSourceFile(
          fileName,
          languageVersion,
          onError,
          shouldCreateNewSourceFile,
        );
  };
  // pathsValidation4/5 enable traceResolution in their configs. The conformance
  // harness captures trace output separately; it is not part of this oracle.
  host.trace = () => {};
  return host;
}

function buildPathsValidationFixture(manifest, pinned) {
  const inventory = manifest.sources[pinned.source_index];
  requireCondition(
    isPlainObject(inventory) &&
      inventory.suite === "compiler" &&
      inventory.path === pinned.path &&
      inventory.bytes === pinned.bytes &&
      inventory.sha256 === pinned.sha256 &&
      inventory.git_blob_sha1 === pinned.git_blob_sha1,
    `${pinned.path} does not match its pinned source inventory`,
  );
  const recordedFixtures = manifest.compiler_fixtures.filter(
    (fixture) => fixture.source === pinned.source_index,
  );
  requireCondition(
    recordedFixtures.length === 1,
    `${pinned.path} must have exactly one expansion-manifest fixture`,
  );
  const recorded = recordedFixtures[0];
  requireCondition(
    recorded.configurations?.length === 1 &&
      recorded.configurations[0].upstream_name === pinned.path,
    `${pinned.path} must have exactly one matching harness configuration`,
  );

  const sourcePath = safeCompilerCasePath(pinned.path);
  const rawSource = fs.readFileSync(sourcePath);
  requireCondition(
    rawSource.length === pinned.bytes &&
      sha256(rawSource) === pinned.sha256 &&
      gitBlobSha1(rawSource) === pinned.git_blob_sha1,
    `${pinned.path} does not match its byte/blob pins`,
  );
  const decoded = ts.sys.readFile(sourcePath);
  requireCondition(
    typeof decoded === "string" &&
      Buffer.byteLength(decoded, "utf8") === recorded.decoded_utf8_bytes &&
      sha256(Buffer.from(decoded, "utf8")) === recorded.decoded_sha256,
    `${pinned.path} decoded text does not match the expansion manifest`,
  );
  const { units, links } = makeUnitsFromTest(decoded, pinned.path);
  const configIndices = units
    .map((unit, index) => (isConfigFileName(unit.name) ? index : -1))
    .filter((index) => index >= 0);
  requireCondition(
    configIndices.length === 1,
    `${pinned.path} must expand to exactly one tsconfig/jsconfig unit`,
  );
  const configUnitId = configIndices[0];
  verifyPathsValidationExpansion(
    recorded,
    { path: pinned.path },
    units,
    links,
    configUnitId,
  );

  const configUnit = units[configUnitId];
  const configFileName = absoluteUnitName(configUnit);
  const configSource = ts.parseJsonText(configFileName, configUnit.content);
  const parsed = ts.parseJsonSourceFileConfigFileContent(
    configSource,
    createPathsValidationParseHost(units),
    ts.getDirectoryPath(configFileName),
    undefined,
    configFileName,
  );
  const program = ts.createProgram({
    rootNames: parsed.fileNames,
    options: parsed.options,
    host: createPathsValidationCompilerHost(units, parsed.options),
    projectReferences: parsed.projectReferences,
    configFileParsingDiagnostics: parsed.errors,
  });
  const optionsDiagnostics = program
    .getOptionsDiagnostics()
    .filter((diagnostic) => PATHS_VALIDATION_CODES.has(diagnostic.code))
    .map((diagnostic, index) =>
      diagnosticRecord(diagnostic, `${pinned.path} options diagnostic ${index}`),
    );
  requireJsonEqual(
    optionsDiagnostics.map((diagnostic) => diagnostic.code),
    pinned.expected_codes,
    `${pinned.path} options diagnostic codes`,
  );
  requireCondition(
    optionsDiagnostics.every(
      (diagnostic) =>
        diagnostic.file === configFileName &&
        Number.isSafeInteger(diagnostic.start) &&
        Number.isSafeInteger(diagnostic.length),
    ),
    `${pinned.path} options diagnostics must all have config-file locations`,
  );

  return {
    id: pinned.path,
    source: {
      index: pinned.source_index,
      path: `${COMPILER_CASES_RELATIVE_PATH}/${pinned.path}`,
      mode: inventory.mode,
      bytes: pinned.bytes,
      sha256: pinned.sha256,
      git_blob_sha1: pinned.git_blob_sha1,
      decoded_utf8_bytes: recorded.decoded_utf8_bytes,
      decoded_sha256: recorded.decoded_sha256,
    },
    input: {
      virtual_source_root: VIRTUAL_SOURCE_ROOT,
      use_case_sensitive_file_names: false,
      config_unit_id: configUnitId,
      units: units.map((unit, id) => ({
        id,
        name: unit.name,
        file_options: Object.entries(unit.fileOptions).map(([name, value]) => ({
          name,
          value,
        })),
        source: sourceTextRecord(absoluteUnitName(unit), unit.content),
      })),
      parsed_file_names: [...parsed.fileNames],
    },
    options_diagnostics: optionsDiagnostics,
  };
}

function summarizePathsValidation(fixtures) {
  const counts = new Map();
  let located = 0;
  for (const fixture of fixtures) {
    for (const diagnostic of fixture.options_diagnostics) {
      counts.set(diagnostic.code, (counts.get(diagnostic.code) ?? 0) + 1);
      if (diagnostic.file !== null) located += 1;
    }
  }
  return {
    fixture_total: fixtures.length,
    options_diagnostic_total: [...counts.values()].reduce(
      (total, count) => total + count,
      0,
    ),
    located_options_diagnostic_total: located,
    diagnostics_by_code: [...counts]
      .sort(([left], [right]) => left - right)
      .map(([code, count]) => ({ code, count })),
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
  const pathsValidationManifest = readPathsValidationManifest();
  const pathsValidationFixtures = PATHS_VALIDATION_CASES.map((pinned) =>
    buildPathsValidationFixture(pathsValidationManifest, pinned),
  );
  const pathsValidationSummary = summarizePathsValidation(
    pathsValidationFixtures,
  );
  requireJsonEqual(
    pathsValidationSummary,
    EXPECTED_PATHS_VALIDATION_SUMMARY,
    "paths validation options diagnostic oracle summary",
  );
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
        { symbol: "pathIsAbsolute", lines: "5311-5313" },
        { symbol: "pathIsRelative", lines: "5314-5316" },
        { symbol: "sortAndDeduplicateDiagnostics", lines: "11237-11239" },
        { symbol: "getPathsBasePath", lines: "16595-16599" },
        {
          symbol: "hasZeroOrOneAsteriskCharacter",
          lines: "18318-18330",
        },
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
        { symbol: "getOptionsDiagnostics", lines: "124024-124029" },
        {
          symbol: "verifyCompilerOptions paths validation",
          lines: "124805-124854",
        },
        {
          symbol: "createDiagnosticForOptionPathKeyValue",
          lines: "125298-125314",
        },
        {
          symbol: "createDiagnosticForOptionPaths",
          lines: "125315-125333",
        },
        { symbol: "forEachOptionPathsSyntax", lines: "125334-125336" },
        {
          symbol: "createCompilerOptionsDiagnostic",
          lines: "125375-125388",
        },
        {
          symbol: "getCompilerOptionsObjectLiteralSyntax",
          lines: "125389-125395",
        },
        {
          symbol: "getCompilerOptionsPropertySyntax",
          lines: "125396-125405",
        },
        {
          symbol: "createOptionDiagnosticInObjectLiteralSyntax",
          lines: "125406-125417",
        },
      ],
    },
    summary,
    fixtures,
    options_diagnostics: {
      scope: "paths_validation",
      diagnostic_codes: [...PATHS_VALIDATION_CODES],
      corpus: {
        suite_path: COMPILER_CASES_RELATIVE_PATH,
        manifest: {
          path: TEST_SUITE_MANIFEST_RELATIVE_PATH,
          sha256: EXPECTED_TEST_SUITE_MANIFEST_SHA256,
        },
        virtual_source_root: VIRTUAL_SOURCE_ROOT,
      },
      summary: pathsValidationSummary,
      fixtures: pathsValidationFixtures,
    },
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
