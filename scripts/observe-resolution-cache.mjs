#!/usr/bin/env node
// L2.3 native source oracle: replay the resolution-cache change trace against
// the vendored TypeScript 6.0.3 and record the fresh result of every request
// after every generation.
//
//   node scripts/observe-resolution-cache.mjs \
//     --manifest crates/program/tests/fixtures/resolution_cache/manifest.v1.json \
//     --out crates/program/tests/fixtures/resolution_cache/expected.v1.json
//
// Every state is observed twice; the run refuses to write unless both
// observations are byte-identical. The observer models the filesystem with a
// virtual host (files, explicit directories, directory symlinks, optional
// case folding) so the Rust contract can rebuild the identical host state.
// Cache-control entries in the manifest ("controls") are Rust-only and are
// ignored here: TypeScript is always run fresh.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(SCRIPT), "..");
const TS_PATH = path.join(WORKSPACE, "vendor/typescript-6.0.3/lib/typescript.js");
const EXPECTED_BUNDLE_SHA256 =
  "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const EXPECTED_NODE_VERSION = fs
  .readFileSync(path.join(WORKSPACE, ".node-version"), "utf8")
  .trim();

function sha256(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}
function requireCondition(condition, message) {
  if (!condition) throw new Error(message);
}

const bundleBytes = fs.readFileSync(TS_PATH);
requireCondition(sha256(bundleBytes) === EXPECTED_BUNDLE_SHA256, "typescript.js hash drift");
const ts = (await import(TS_PATH)).default;
requireCondition(ts.version === "6.0.3", `unexpected TypeScript version ${ts.version}`);
requireCondition(
  process.version.replace(/^v/, "") === EXPECTED_NODE_VERSION,
  `expected Node ${EXPECTED_NODE_VERSION}, running ${process.version}`,
);
for (const name of [
  "resolveModuleName",
  "resolveTypeReferenceDirective",
  "getAutomaticTypeDirectiveNames",
  "getImpliedNodeFormatForFile",
  "toFileNameLowerCase",
  "matchFiles",
  "readJsonConfigFile",
  "parseJsonSourceFileConfigFileContent",
  "createProgram",
]) {
  requireCondition(typeof ts[name] === "function", `ts.${name} is not available`);
}

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

const args = process.argv.slice(2);
function argValue(flag) {
  const index = args.indexOf(flag);
  return index === -1 ? undefined : args[index + 1];
}
const manifestPath = argValue("--manifest");
const outPath = argValue("--out");
const checkPath = argValue("--check");
requireCondition(manifestPath && Boolean(outPath) !== Boolean(checkPath),
  "usage: --manifest <path> (--out <path> | --check <frozen-path>)");
const manifestBytes = fs.readFileSync(manifestPath);
const manifest = JSON.parse(manifestBytes.toString("utf8"));
requireCondition(
  manifest.schema === "tsc-rs/resolution-cache-manifest/v1",
  `unexpected manifest schema ${manifest.schema}`,
);

// ---------------------------------------------------------------------------
// Virtual filesystem with directory symlinks
// ---------------------------------------------------------------------------

class VirtualFs {
  constructor(caseSensitive) {
    this.caseSensitive = caseSensitive;
    this.files = new Map(); // key -> { path, text }
    this.directories = new Map(); // key -> path (explicit)
    this.links = new Map(); // key -> { path, target }
  }

  key(p) {
    return this.caseSensitive ? p : ts.toFileNameLowerCase(p);
  }

  clone() {
    const copy = new VirtualFs(this.caseSensitive);
    copy.files = new Map(this.files);
    copy.directories = new Map(this.directories);
    copy.links = new Map(this.links);
    return copy;
  }

  // Substitute link prefixes until no link applies (bounded).
  resolve(p) {
    let current = p;
    for (let round = 0; round < 32; round++) {
      let substituted = false;
      const parts = current.split("/");
      let prefix = "";
      for (let index = 1; index < parts.length; index++) {
        prefix += "/" + parts[index];
        const link = this.links.get(this.key(prefix));
        if (link) {
          const rest = parts.slice(index + 1);
          current = rest.length === 0 ? link.target : link.target + "/" + rest.join("/");
          substituted = true;
          break;
        }
      }
      if (!substituted) return current;
    }
    throw new Error(`symlink chain too deep for ${p}`);
  }

  fileExists(p) {
    return this.files.has(this.key(this.resolve(p)));
  }

  readFile(p) {
    const entry = this.files.get(this.key(this.resolve(p)));
    return entry === undefined ? undefined : entry.text;
  }

  // A directory exists when it is explicit, is an ancestor of any file,
  // explicit directory or link, or is a link to an existing directory.
  directoryExistsResolved(resolved) {
    const key = this.key(resolved);
    if (this.directories.has(key)) return true;
    const prefix = key === "/" ? "/" : key + "/";
    for (const candidate of this.allEntryKeys()) {
      if (candidate.startsWith(prefix) && candidate.length > prefix.length) return true;
    }
    return false;
  }

  directoryExists(p) {
    return this.directoryExistsResolved(this.resolve(p));
  }

  allEntryKeys() {
    return [...this.files.keys(), ...this.directories.keys(), ...this.links.keys()];
  }

  realpath(p) {
    const resolved = this.resolve(p);
    const fileEntry = this.files.get(this.key(resolved));
    if (fileEntry) return fileEntry.path;
    const directory = this.directories.get(this.key(resolved));
    if (directory) return directory;
    return resolved;
  }

  // Immediate children (files and directories) of a resolved directory, with
  // their display names, in UTF-16 code unit order.
  entries(p) {
    const resolved = this.resolve(p);
    const prefix = resolved === "/" ? "/" : resolved + "/";
    const prefixKey = this.key(prefix);
    const files = new Map();
    const directories = new Map();
    const record = (displayPath, isFile) => {
      const displayKey = this.key(displayPath);
      if (!displayKey.startsWith(prefixKey) || displayKey.length <= prefixKey.length) return;
      const rest = displayPath.slice(prefix.length);
      const slash = rest.indexOf("/");
      const name = slash === -1 ? rest : rest.slice(0, slash);
      if (slash === -1 && isFile) {
        files.set(this.key(name), name);
      } else {
        directories.set(this.key(name), name);
      }
    };
    for (const entry of this.files.values()) record(entry.path, true);
    for (const directory of this.directories.values()) record(directory, false);
    for (const link of this.links.values()) record(link.path, false);
    const compare = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
    return {
      files: [...files.values()].sort(compare),
      directories: [...directories.values()].sort(compare),
    };
  }

  getDirectories(p) {
    if (!this.directoryExists(p)) return [];
    return this.entries(p).directories;
  }

  apply(op) {
    switch (op.op) {
      case "create_file":
      case "update_file":
        this.files.set(this.key(op.path), { path: op.path, text: op.text });
        return;
      case "delete_file":
        requireCondition(this.files.delete(this.key(op.path)), `delete_file: ${op.path} is absent`);
        return;
      case "create_directory":
        this.directories.set(this.key(op.path), op.path);
        return;
      case "delete_directory": {
        const key = this.key(op.path);
        const prefix = key + "/";
        for (const map of [this.files, this.directories, this.links]) {
          for (const candidate of [...map.keys()]) {
            if (candidate === key || candidate.startsWith(prefix)) map.delete(candidate);
          }
        }
        return;
      }
      case "set_symlink":
        this.links.set(this.key(op.path), { path: op.path, target: op.target });
        return;
      case "remove_symlink":
        requireCondition(this.links.delete(this.key(op.path)), `remove_symlink: ${op.path} is absent`);
        return;
      case "set_options":
      case "set_program_options":
      case "unset_options":
      case "unset_program_options":
      case "invalidate_all":
        return;
      default:
        throw new Error(`unknown op ${op.op}`);
    }
  }
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

const MODULE_RESOLUTION = {
  classic: ts.ModuleResolutionKind.Classic,
  node10: ts.ModuleResolutionKind.Node10,
  node16: ts.ModuleResolutionKind.Node16,
  nodenext: ts.ModuleResolutionKind.NodeNext,
  bundler: ts.ModuleResolutionKind.Bundler,
};
const MODULE = {
  none: ts.ModuleKind.None,
  commonjs: ts.ModuleKind.CommonJS,
  esnext: ts.ModuleKind.ESNext,
  node16: ts.ModuleKind.Node16,
  nodenext: ts.ModuleKind.NodeNext,
  preserve: ts.ModuleKind.Preserve,
};
const MODE = {
  undefined: undefined,
  commonjs: ts.ModuleKind.CommonJS,
  esnext: ts.ModuleKind.ESNext,
};

function buildOptions(rawOptions, rawProgramOptions, cwd) {
  const options = {};
  for (const [name, value] of Object.entries(rawOptions)) {
    switch (name) {
      case "moduleResolution":
        requireCondition(name in rawOptions && MODULE_RESOLUTION[value] !== undefined, `moduleResolution ${value}`);
        options.moduleResolution = MODULE_RESOLUTION[value];
        break;
      case "module":
        requireCondition(MODULE[value] !== undefined, `module ${value}`);
        options.module = MODULE[value];
        break;
      case "moduleSuffixes":
      case "customConditions":
        options[name] = [...value];
        break;
      case "baseUrl":
        options.baseUrl = value;
        break;
      case "libReplacement":
      case "resolveJsonModule":
      case "allowJs":
      case "noDtsResolution":
      case "allowArbitraryExtensions":
      case "allowImportingTsExtensions":
      case "resolvePackageJsonExports":
      case "resolvePackageJsonImports":
        options[name] = value;
        break;
      default:
        throw new Error(`unsupported manifest option ${name}`);
    }
  }
  for (const [name, value] of Object.entries(rawProgramOptions)) {
    switch (name) {
      case "paths":
        options.paths = Object.fromEntries(Object.entries(value).map(([k, v]) => [k, [...v]]));
        break;
      case "pathsBasePath":
        options.pathsBasePath = value;
        break;
      case "rootDirs":
      case "typeRoots":
      case "types":
        options[name] = [...value];
        break;
      case "preserveSymlinks":
        options.preserveSymlinks = value;
        break;
      case "configFilePath":
        options.configFilePath = value;
        break;
      default:
        throw new Error(`unsupported manifest program option ${name}`);
    }
  }
  if (options.paths && !options.baseUrl && !options.pathsBasePath) {
    options.pathsBasePath = cwd;
  }
  return options;
}

// ---------------------------------------------------------------------------
// Hosts
// ---------------------------------------------------------------------------

function moduleResolutionHost(vfs, cwd) {
  return {
    fileExists: (p) => vfs.fileExists(p),
    readFile: (p) => vfs.readFile(p),
    directoryExists: (p) => vfs.directoryExists(p),
    realpath: (p) => vfs.realpath(p),
    getCurrentDirectory: () => cwd,
    getDirectories: (p) => vfs.getDirectories(p).map((name) => (p === "/" ? "/" : p + "/") + name),
    useCaseSensitiveFileNames: vfs.caseSensitive,
  };
}

function parseConfigHost(vfs, cwd) {
  const host = moduleResolutionHost(vfs, cwd);
  return {
    ...host,
    useCaseSensitiveFileNames: vfs.caseSensitive,
    readDirectory: (rootDir, extensions, excludes, includes, depth) =>
      ts.matchFiles(
        rootDir,
        extensions,
        excludes,
        includes,
        vfs.caseSensitive,
        cwd,
        depth,
        (p) => vfs.entries(p),
        (p) => vfs.realpath(p),
      ),
  };
}

function compilerHost(vfs, cwd, options) {
  const host = moduleResolutionHost(vfs, cwd);
  const getCanonicalFileName = ts.createGetCanonicalFileName(vfs.caseSensitive);
  return {
    ...host,
    getSourceFile: (fileName, languageVersionOrOptions) => {
      const text = vfs.readFile(fileName);
      return text === undefined
        ? undefined
        : ts.createSourceFile(fileName, text, languageVersionOrOptions, true);
    },
    getDefaultLibFileName: () => "/lib.d.ts",
    writeFile: () => {
      throw new Error("no emit");
    },
    getCanonicalFileName,
    useCaseSensitiveFileNames: () => vfs.caseSensitive,
    getNewLine: () => "\n",
    getCurrentDirectory: () => cwd,
    trace: undefined,
    options,
  };
}

// ---------------------------------------------------------------------------
// Observations
// ---------------------------------------------------------------------------

function packageIdText(packageId) {
  if (!packageId) return null;
  const sub = packageId.subModuleName ? `/${packageId.subModuleName}` : "";
  return `${packageId.name}@${packageId.version}${sub}`;
}

function observeModule(result) {
  const resolved = result.resolvedModule;
  return {
    resolved: resolved
      ? {
          resolvedFileName: resolved.resolvedFileName,
          extension: resolved.extension,
          isExternalLibraryImport: !!resolved.isExternalLibraryImport,
          resolvedUsingTsExtension: !!resolved.resolvedUsingTsExtension,
          originalPath: resolved.originalPath ?? null,
          packageId: packageIdText(resolved.packageId),
        }
      : null,
    alternateResult: result.alternateResult ?? null,
    failedLookupLocations: [...(result.failedLookupLocations ?? [])],
    affectingLocations: [...(result.affectingLocations ?? [])],
    resolutionDiagnostics: (result.resolutionDiagnostics ?? []).map((d) => d.code),
  };
}

function observeTypeReference(result) {
  const resolved = result.resolvedTypeReferenceDirective;
  return {
    resolved:
      resolved && resolved.resolvedFileName
        ? {
            resolvedFileName: resolved.resolvedFileName,
            primary: !!resolved.primary,
            isExternalLibraryImport: !!resolved.isExternalLibraryImport,
            originalPath: resolved.originalPath ?? null,
            packageId: packageIdText(resolved.packageId),
          }
        : null,
    failedLookupLocations: [...(result.failedLookupLocations ?? [])],
    affectingLocations: [...(result.affectingLocations ?? [])],
    resolutionDiagnostics: (result.resolutionDiagnostics ?? []).map((d) => d.code),
  };
}

function observeRequest(request, vfs, cwd, options) {
  const host = moduleResolutionHost(vfs, cwd);
  switch (request.kind) {
    case "module": {
      const result = ts.resolveModuleName(
        request.specifier,
        request.containing,
        options,
        host,
        undefined,
        undefined,
        MODE[request.mode],
      );
      return { kind: "module", ...observeModule(result) };
    }
    case "type_reference":
    case "automatic_type_reference": {
      const result = ts.resolveTypeReferenceDirective(
        request.specifier,
        request.containing,
        options,
        host,
        undefined,
        undefined,
        MODE[request.mode],
      );
      return { kind: request.kind, ...observeTypeReference(result) };
    }
    case "library": {
      const result = ts.resolveModuleName(
        request.specifier,
        request.containing,
        { moduleResolution: ts.ModuleResolutionKind.Node10 },
        host,
      );
      return { kind: "library", ...observeModule(result) };
    }
    case "automatic_type_names":
      return { kind: "automatic_type_names", names: [...ts.getAutomaticTypeDirectiveNames(options, host)] };
    case "package_scope": {
      const implied = ts.getImpliedNodeFormatForFile(request.containing, undefined, host, options);
      return {
        kind: "package_scope",
        impliedNodeFormat: implied === undefined ? null : implied === ts.ModuleKind.ESNext ? "esnext" : "commonjs",
      };
    }
    case "config":
      return { kind: "config" };
    default:
      throw new Error(`unknown request kind ${request.kind}`);
  }
}

function observeProgram(roots, vfs, cwd, options) {
  const programOptions = { ...options, noLib: true, noEmit: true, types: [] };
  const host = compilerHost(vfs, cwd, programOptions);
  const program = ts.createProgram({ rootNames: roots, options: programOptions, host });
  return {
    sourceFiles: program.getSourceFiles().map((file) => file.fileName),
    optionsDiagnostics: program.getOptionsDiagnostics().map((d) => d.code),
    configFileParsingDiagnostics: program.getConfigFileParsingDiagnostics().map((d) => d.code),
  };
}

const CONFIG_OPTION_KEYS = [
  "moduleResolution",
  "module",
  "baseUrl",
  "paths",
  "pathsBasePath",
  "rootDirs",
  "moduleSuffixes",
  "customConditions",
  "types",
  "typeRoots",
];

function observeConfig(configPath, vfs, cwd) {
  const host = parseConfigHost(vfs, cwd);
  // The CLI path: parse the root as a JSON source file so extended source
  // files and their syntax provenance are retained on the result.
  const read = ts.readJsonConfigFile(configPath, (p) => vfs.readFile(p));
  const parsed = ts.parseJsonSourceFileConfigFileContent(
    read,
    host,
    ts.getDirectoryPath(configPath),
    undefined,
    configPath,
  );
  const options = {};
  for (const key of CONFIG_OPTION_KEYS) {
    if (parsed.options[key] !== undefined) options[key] = parsed.options[key];
  }
  return {
    readErrors: (read.parseDiagnostics ?? []).map((d) => d.code),
    fileNames: [...parsed.fileNames],
    errors: parsed.errors.map((d) => d.code),
    options,
    extendedSourceFiles: [...(parsed.options.configFile?.extendedSourceFiles ?? [])].sort(),
  };
}

// ---------------------------------------------------------------------------
// Replay
// ---------------------------------------------------------------------------

function observeAll() {
  const families = {};
  let generationCount = 0;
  let requestCount = 0;
  for (const family of manifest.families) {
    const host = { ...manifest.defaults.host, ...(family.host ?? {}) };
    let rawOptions = { ...manifest.defaults.options, ...(family.options ?? {}) };
    let rawProgramOptions = { ...(family.program_options ?? {}) };
    const vfs = new VirtualFs(host.case_sensitive);
    for (const [filePath, text] of Object.entries(family.initial.files ?? {})) {
      vfs.apply({ op: "create_file", path: filePath, text });
    }
    for (const directory of family.initial.directories ?? []) {
      vfs.apply({ op: "create_directory", path: directory });
    }
    for (const [link, target] of Object.entries(family.initial.symlinks ?? {})) {
      vfs.apply({ op: "set_symlink", path: link, target });
    }
    const generations = {};
    const allGenerations = [{ id: "g0", ops: [] }, ...family.generations];
    for (const generation of allGenerations) {
      for (const op of generation.ops) {
        if (op.op === "set_options") rawOptions = { ...rawOptions, ...op.options };
        else if (op.op === "set_program_options") rawProgramOptions = { ...rawProgramOptions, ...op.program_options };
        else if (op.op === "unset_options") for (const name of op.names) delete rawOptions[name];
        else if (op.op === "unset_program_options") for (const name of op.names) delete rawProgramOptions[name];
        else vfs.apply(op);
      }
      const options = buildOptions(rawOptions, rawProgramOptions, host.current_directory);
      const record = {
        requests: family.requests.map((request) => observeRequest(request, vfs, host.current_directory, options)),
      };
      requestCount += family.requests.length;
      if (family.roots) record.program = observeProgram(family.roots, vfs, host.current_directory, options);
      if (family.config) record.config = observeConfig(family.config, vfs, host.current_directory);
      generations[generation.id] = record;
      generationCount += 1;
    }
    families[family.id] = { generations };
  }
  return {
    schema: "tsc-rs/resolution-cache-expected/v1",
    typescript: ts.version,
    typescript_bundle_sha256: EXPECTED_BUNDLE_SHA256,
    manifest_sha256: sha256(manifestBytes),
    node: process.version,
    counts: { families: manifest.families.length, generations: generationCount, requests: requestCount },
    families,
  };
}

const first = JSON.stringify(observeAll(), null, 2) + "\n";
const second = JSON.stringify(observeAll(), null, 2) + "\n";
requireCondition(first === second, "two native observations differ");
if (checkPath) {
  requireCondition(fs.readFileSync(checkPath, "utf8") === first, `frozen observation drift: ${checkPath}`);
} else {
  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  fs.writeFileSync(outPath, first);
}
const parsed = JSON.parse(first);
console.log(
  ` ${checkPath ? "checked " + checkPath : "wrote " + outPath}: families=${parsed.counts.families} generations=${parsed.counts.generations} requests=${parsed.counts.requests} (two runs identical, sha256 ${sha256(first)})`,
);
