// Exact Established-floor input reconstruction for the parser census replay.
// This observer mirrors the pinned harness loaders. It does not alter them or
// substitute another qualification artifact for a recorded fixture.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

export const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
export const decode = bytes => new TextDecoder("utf-8", {fatal: true, ignoreBOM: true}).decode(bytes);
export const absolute = (name, cwd) => ts.getNormalizedAbsolutePath(name, cwd);
const optionsByName = new Map(ts.optionDeclarations.map(option => [option.name.toLowerCase(), option]));
const floorDropped = new Set([
  "sourcemap", "inlinesourcemap", "inlinesources", "sourceroot", "maproot", "emitbom",
  "emitdeclarationonly", "declarationmap", "outfile", "outdir", "noemithelpers",
  "declarationdir", "incremental", "assumechangesonlyaffectdirectdependencies", "stripinternal",
  "disablesizelimit", "out", "rootdir", "tsbuildinfofile", "pretty", "traceresolution",
  "listfilesonly", "stabletypeordering", "nocheck",
]);
const metadata = new Set([
  "capturesuggestions", "fullemitpaths", "typescriptversion", "notypesandsymbols",
  "noimplicitreferences", "currentdirectory", "usecasesensitivefilenames", "filename",
  "link", "symlink", "suppressoutputpathcheck",
]);

export function compilerOptions(settings, base, cwd) {
  const options = ts.cloneCompilerOptions(base);
  options.skipDefaultLibCheck ??= true;
  let explicitAllowJs = typeof base.allowJs === "boolean";
  const decisions = [];
  for (const [name, raw] of settings) {
    const key = name.toLowerCase();
    const decision = floorDropped.has(key) ? "floor-dropped" : metadata.has(key) ? "metadata" : "applied";
    decisions.push({name, raw, decision});
    if (decision !== "applied") continue;
    if (key === "allowjs") explicitAllowJs = true;
    const option = optionsByName.get(key);
    assert.ok(option, `unsupported compiler option ${name}`);
    const errors = [];
    let parsed;
    if (option.type === "boolean") parsed = raw.toLowerCase() === "true";
    else if (option.type === "string") parsed = raw;
    else if (option.type === "number") parsed = Number(raw);
    else if (["list", "listOrElement"].includes(option.type)) parsed = ts.parseListTypeOption(option, raw, errors);
    else parsed = ts.parseCustomTypeOption(option, raw, errors);
    assert.deepEqual(errors, [], `invalid @${name}: ${raw}`);
    if (key === "typeroots") parsed = raw.split(",").map(s => s.trim()).filter(Boolean).map(s => absolute(s, cwd));
    // The loader retains empty module suffixes and their spelling.
    if (key === "modulesuffixes") parsed = raw.split(",");
    options[option.name] = parsed;
  }
  if (!explicitAllowJs) options.allowJs = options.checkJs ?? false;
  options.newLine ??= ts.NewLineKind.CarriageReturnLineFeed;
  options.noErrorTruncation = true;
  return {options, decisions};
}

export function documentPool(encoded) {
  return new Map(Object.entries(encoded).map(([hash, text]) => {
    const bytes = Buffer.from(text, "base64");
    assert.equal(sha256(bytes), hash, "document pool hash differs");
    return [hash, decode(bytes)];
  }));
}

function document(pool, hash) {
  assert.ok(pool.has(hash), `missing document ${hash}`);
  return pool.get(hash);
}

// File insertion and alias resolution mirror load_compiler_program, including
// the bounded fixpoint for directory links into directories containing links.
export function compilerLayout(input, pool) {
  const files = new Map(), aliases = new Map();
  const unit = id => { const u = input.units[id]; assert.equal(u?.id, id); return u; };
  for (const id of input.vfs_write_order) {
    const u = unit(id);
    if (u.content_sha256 !== null) files.set(absolute(u.name, input.current_directory), document(pool, u.content_sha256));
  }
  const links = [...input.global_symlinks, ...input.units.flatMap(u => u.document_symlinks)];
  assert.deepEqual(links, input.vfs_symlinks, "recorded symlink operation order differs");
  const physical = name => {
    for (let i = 0; i < Math.max(links.length, 1); i++) {
      const next = aliases.get(name);
      if (next === undefined || next === name) break;
      name = next;
    }
    return name;
  };
  let expanded = true, passes = 0;
  while (expanded && passes <= links.length) {
    expanded = false; passes++;
    for (const {target, link} of links) {
      const prefix = target.replace(/\/$/, "") + "/";
      const candidates = files.has(target) ? [[link, target]] : [...files.keys()]
        .filter(name => name.startsWith(prefix)).map(name => [link.replace(/\/$/, "") + "/" + name.slice(prefix.length), name]);
      for (const [alias, rawTarget] of candidates) {
        const target = physical(rawTarget);
        if (alias === target) continue;
        if (!files.has(alias)) {
          if (!files.has(target)) continue;
          files.set(alias, files.get(target)); expanded = true;
        }
        if (aliases.get(alias) !== target) { aliases.set(alias, target); expanded = true; }
      }
    }
  }
  return {files, aliases, roots: input.program_root_units.map(id => absolute(unit(id).name, input.current_directory))};
}

function directoryEntries(names, cwd, sensitive) {
  const canonical = ts.createGetCanonicalFileName(sensitive);
  return directory => {
    const prefix = canonical(absolute(directory, cwd)).replace(/\/$/, "") + "/";
    const files = [], directories = new Set();
    for (const name of names) {
      const normalized = absolute(name, cwd);
      if (!canonical(normalized).startsWith(prefix)) continue;
      const relative = normalized.slice(prefix.length), slash = relative.indexOf("/");
      if (slash < 0) files.push(relative); else directories.add(relative.slice(0, slash));
    }
    return {files, directories: [...directories]};
  };
}

function readDirectory(names, cwd, sensitive) {
  return (directory, extensions, excludes, includes, depth) => ts.matchFiles(
    directory, extensions, excludes, includes, sensitive, cwd, depth,
    directoryEntries(names, cwd, sensitive), name => absolute(name, cwd));
}

// The recorded compiler config planner has its own harnessIO boundary:
// insensitive raw-unit lookup and /.src-based directory matching, independent
// of the eventual compiler host's case sensitivity and currentDirectory.
function compilerConfig(input, pool) {
  if (input.config_unit === null) return {options: {}, errors: []};
  const unit = input.units[input.config_unit];
  assert.equal(unit.id, input.config_unit);
  const lower = ts.createGetCanonicalFileName(false);
  const configHost = {
    useCaseSensitiveFileNames: false,
    fileExists: name => input.units.some(u => lower(u.name) === lower(name)),
    readFile: name => {
      const unit = input.units.find(u => lower(u.name) === lower(name) && u.content_sha256 !== null);
      return unit ? document(pool, unit.content_sha256) : undefined;
    },
    readDirectory: readDirectory(input.units.map(u => u.name), "/.src", false),
  };
  const source = ts.parseJsonText(unit.name, document(pool, unit.content_sha256));
  return ts.parseJsonSourceFileConfigFileContent(source, configHost, "/.src", undefined, unit.name);
}

function artifactLayout(input, route) {
  const files = new Map(), aliases = new Map();
  const insert = (name, text) => { assert.ok(!files.has(name), `duplicate qualified file ${name}`); files.set(name, text); };
  const decoded = file => {
    const bytes = Buffer.from(file.utf8_base64, "base64");
    assert.equal(bytes.length, file.utf8_bytes);
    assert.equal(sha256(bytes), file.utf8_sha256);
    return decode(bytes);
  };
  for (const file of input.files) insert(file.path, route === "qualified" ? decoded(file) : file.text);
  if (route === "qualified" && input.virtual_config) insert(input.virtual_config.path, decoded(input.virtual_config));
  if (route === "candidate" && input.config && !files.has(input.config.path)) insert(input.config.path, input.config.text);
  // Qualified links target original files only, never another link.
  const originals = new Map(files);
  for (const {link_path, target_path} of input.vfs_symlinks ?? []) {
    assert.ok(originals.has(target_path), `qualified link target absent: ${target_path}`);
    insert(link_path, originals.get(target_path)); aliases.set(link_path, target_path);
  }
  for (const root of input.roots) assert.ok(files.has(root), `qualified root absent: ${root}`);
  return {files, aliases, roots: input.roots};
}

export function createHost(layout, cwd, sensitive, libraryRoot, project = false) {
  const canonical = ts.createGetCanonicalFileName(sensitive);
  const key = name => canonical(absolute(name, cwd));
  const files = new Map([...layout.files].map(([name, text]) => [key(name), text]));
  const aliases = new Map([...layout.aliases].map(([name, target]) => [key(name), target]));
  const library = name => {
    const absoluteName = absolute(name, cwd);
    if (!absoluteName.startsWith(libraryRoot + "/") || !/^lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(path.posix.basename(absoluteName))) return undefined;
    return fs.existsSync(absoluteName) ? absoluteName : undefined;
  };
  const readFile = name => files.get(key(name)) ?? (library(name) ? decode(fs.readFileSync(library(name))) : undefined);
  const overlay = createHermeticDirectoryOverlay(layout.files.keys(), {
    currentDirectory: cwd, useCaseSensitiveFileNames: sensitive,
    fallbackHost: {directoryExists: name => name === libraryRoot, getDirectories: () => []},
  });
  return {
    ...overlay, getCurrentDirectory: () => cwd, useCaseSensitiveFileNames: () => sensitive,
    getCanonicalFileName: canonical, getNewLine: () => "\n", trace() {},
    getDefaultLibFileName: options => libraryRoot + "/" + (project ? "lib.es5.d.ts" : ts.getDefaultLibFileName(options)),
    getDefaultLibLocation: () => libraryRoot,
    readFile, fileExists: name => files.has(key(name)) || library(name) !== undefined,
    realpath: name => aliases.get(key(name)) ?? absolute(name, cwd),
    readDirectory: readDirectory([...layout.files.keys()], cwd, sensitive),
    getSourceFile(name, options) {
      const text = readFile(name);
      return text === undefined ? undefined : ts.createSourceFile(name, text, options, true);
    },
    writeFile: () => assert.fail("write must use the complete-command callback"),
  };
}

function projectOptions(input, descriptor, host, layout, noEmit) {
  let parsed = {options: {}, errors: []}, roots;
  if (typeof descriptor.project === "string" && descriptor.project.length) {
    assert.ok(!descriptor.inputFiles?.length);
    const config = absolute(descriptor.project + "/tsconfig.json", input.current_directory);
    const text = host.readFile(config); assert.notEqual(text, undefined, config);
    const read = ts.parseConfigFileTextToJson(config, text);
    parsed = ts.parseJsonConfigFileContent(read.config, {...host, useCaseSensitiveFileNames: input.mount.case_sensitive}, input.current_directory, undefined, config);
    if (read.error) parsed.errors.unshift(read.error);
    roots = parsed.fileNames;
  } else if (descriptor.inputFiles?.length) {
    roots = descriptor.inputFiles.map(name => absolute(name, input.current_directory));
  } else {
    const config = absolute("tsconfig.json", input.current_directory);
    const text = host.readFile(config); assert.notEqual(text, undefined, config);
    const read = ts.parseConfigFileTextToJson(config, text);
    parsed = ts.parseJsonConfigFileContent(read.config, {...host, useCaseSensitiveFileNames: input.mount.case_sensitive}, input.current_directory, undefined, config);
    if (read.error) parsed.errors.unshift(read.error);
    roots = parsed.fileNames;
  }
  const options = {...parsed.options, noErrorTruncation: false, skipDefaultLibCheck: false,
    moduleResolution: ts.ModuleResolutionKind.Classic};
  if (!noEmit) options.newLine = ts.NewLineKind.CarriageReturnLineFeed;
  else { options.noEmit = true; delete options.configFilePath; }
  assert.ok(["Commonjs", "Amd"].includes(input.module_variant));
  options.module = input.module_variant === "Commonjs" ? ts.ModuleKind.CommonJS : ts.ModuleKind.AMD;
  const ignored = new Set(["scenario", "projectRoot", "inputFiles", "baselineCheck", "runTest", "project"]);
  const applied = new Set(["module", "moduleResolution", "declaration", "strict", "noResolve", "sourceMap", "sourceRoot", "mapRoot", "outDir", "outFile", "rootDir"]);
  for (const [name, value] of Object.entries(descriptor)) {
    if (ignored.has(name)) continue;
    if (noEmit && name === "declaration") { assert.equal(value, false); continue; }
    if (noEmit && ["sourceMap", "sourceRoot", "mapRoot", "outDir", "outFile", "declarationDir", "resolveSourceRoot", "resolveMapRoot", "emittedFiles"].includes(name)) {
      const requested = value !== null && value !== false && !(Array.isArray(value) && !value.length)
        && !(typeof value === "object" && !Array.isArray(value) && !Object.keys(value).length);
      assert.equal(requested, false, `unsupported no-emit project option ${name}`); continue;
    }
    assert.ok(!noEmit || name !== "rootDir", "no-emit project does not admit rootDir");
    assert.ok(applied.has(name), `unsupported project descriptor property ${name}`);
    if (["module", "moduleResolution"].includes(name) && typeof value === "string") {
      const errors = [];
      options[name] = ts.parseCustomTypeOption(optionsByName.get(name.toLowerCase()), value, errors);
      assert.deepEqual(errors, []);
    } else options[name] = value;
  }
  layout.roots = roots;
  return {options, errors: parsed.errors, decisions: []};
}

export function prepare(row, pool, libraryRoot) {
  const input = row.command_input;
  assert.equal(input.floor, "established");
  let layout, cwd, sensitive, parsed, projected;
  if (input.route === "recorded-compiler") {
    layout = compilerLayout(input, pool); cwd = input.current_directory; sensitive = input.use_case_sensitive_file_names;
    parsed = compilerConfig(input, pool);
    projected = compilerOptions(input.settings, parsed.options, cwd);
  } else if (["qualified", "candidate"].includes(input.route)) {
    assert.equal(input.use_case_sensitive_file_names, true);
    if (input.route === "candidate") {
      assert.equal(input.input.route, "whole-program"); assert.equal(input.input.shared_mount, null);
    }
    layout = artifactLayout(input.input, input.route); cwd = input.input.current_directory; sensitive = true;
    parsed = {options: {}, errors: []}; // Established ignores the virtual config on these routes.
    const settings = input.route === "qualified" ? input.input.settings.map(s => [s.name, s.value]) : input.settings;
    projected = compilerOptions(settings, {}, cwd);
  } else {
    assert.equal(input.route, "recorded-project");
    assert.equal(input.mount.case_sensitive, true); assert.equal(input.mount.read_only, true);
    layout = {files: new Map(input.mount.files.map(file => [file.path, document(pool, file.content_sha256)])), aliases: new Map(), roots: []};
    cwd = input.current_directory; sensitive = true;
  }
  const host = createHost(layout, cwd, sensitive, libraryRoot, input.route === "recorded-project");
  if (input.route === "recorded-project") {
    const descriptor = JSON.parse(document(pool, input.descriptor_utf8.content_sha256));
    projected = projectOptions(input, descriptor, host, layout, row.loader === "load_project_no_emit");
    parsed = {errors: projected.errors};
  }
  if (row.loader === "load_compiler_no_emit") projected.options.noEmit = true;
  assert.deepEqual(layout.roots, input.prepared.roots, `${row.case_id}: reconstructed root order differs`);
  return {layout, host, options: projected.options, errors: parsed.errors, decisions: projected.decisions};
}
