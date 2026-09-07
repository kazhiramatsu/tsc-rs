// H2.7d/e candidate membership and original input preparation. No Rust admission.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

export const root = path.resolve(import.meta.dirname, "../..");
export const sourceCommit = "050880ce59e30b356b686bd3144efe24f875ebc8";
export const inputPath = "ratchets/h2-7de-candidate-inputs.v1.json";
export const inventoryPath = "ratchets/h2-7de-candidates.v1.json";
export const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
export const read = name => JSON.parse(fs.readFileSync(path.join(root, name)));
export const identity = name => ({ path: name, sha256: sha256(fs.readFileSync(path.join(root, name))) });
const ordered = value => Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b, "en")));
const unique = values => [...new Set(values)].sort();
const optionsByName = new Map(ts.optionDeclarations.map(option => [option.name.toLowerCase(), option]));
const harnessNames = new Set(["usecasesensitivefilenames", "baselinefile", "filename", "suppressoutputpathcheck",
  "noimplicitreferences", "currentdirectory", "symlink", "link", "notypesandsymbols", "fullemitpaths",
  "reportdiagnostics", "capturesuggestions", "typescriptversion"]);
const structuralProjectNames = new Set(["scenario", "projectRoot", "inputFiles", "baselineCheck", "runTest",
  "project", "emittedFiles", "resolveMapRoot", "resolveSourceRoot"]);

function setOption(options, name, raw) {
  const option = optionsByName.get(name.toLowerCase());
  if (!option) { assert.ok(harnessNames.has(name.toLowerCase()), name); return; }
  const errors = [];
  options[option.name] = option.type === "boolean" ? String(raw).toLowerCase() === "true"
    : option.type === "string" ? String(raw) : option.type === "number" ? Number.parseInt(raw, 10)
    : ["list", "listOrElement"].includes(option.type) ? ts.parseListTypeOption(option, String(raw), errors)
    : ts.parseCustomTypeOption(option, String(raw), errors);
  assert.deepEqual(errors, [], name);
}
function serialOptions(options) {
  return ordered(Object.fromEntries(Object.entries(options).filter(([name, value]) =>
    !["configFile", "configFilePath"].includes(name) && value !== undefined)));
}
function pinnedSource(suite, source) {
  const name = `ts-tests/tests/cases/${suite}/${source.path}`;
  const bytes = fs.readFileSync(path.join(root, name));
  assert.equal(bytes.length, source.bytes, name);
  assert.equal(sha256(bytes), source.sha256, name);
  assert.equal(crypto.createHash("sha1").update(`blob ${bytes.length}\0`).update(bytes).digest("hex"), source.git_blob_sha1, name);
  return ts.sys.readFile(path.join(root, name));
}

// Preserve the pinned compiler runner's directive removal and newline rules.
function unitsFromSource(text, name) {
  const units = [], links = [];
  let currentName, content, settings = {};
  const flush = () => units.push({ name: currentName, text: content || "", file_options: Object.entries(settings).map(([name, value]) => ({ name, value })) });
  for (const line of text.split(/\r?\n/)) {
    const link = /^\/{2}\s*@link\s*:\s*([^\r\n]*)\s*->\s*([^\r\n]*)/.exec(line);
    if (link) { links.push({ target: link[1].trim(), link_path: link[2].trim() }); continue; }
    const option = /^\/{2}\s*@([\w]+)\s*:\s*([^\r\n]*)/.exec(line);
    if (option) {
      settings[option[1]] = option[2].trim();
      if (option[1].toLowerCase() !== "filename") continue;
      if (currentName !== undefined) { flush(); content = undefined; settings = {}; }
      else { assert.ok(!content || ts.skipTrivia(content, 0, false, false) === content.length); content = ""; }
      currentName = option[2].trim();
      continue;
    }
    if (content === undefined) content = "";
    else if (content !== "") content += "\n";
    content += line;
  }
  currentName ??= path.posix.basename(name);
  flush();
  return { units, links };
}
function verifyUnits(units, recorded) {
  assert.equal(units.length, recorded.length);
  for (const [index, unit] of units.entries()) {
    const expected = recorded[index];
    assert.equal(unit.name, expected.name);
    assert.deepEqual(unit.file_options, expected.file_options);
    assert.equal(Buffer.byteLength(unit.text), expected.content.utf8_bytes);
    assert.equal(sha256(unit.text), expected.content.sha256);
  }
}

export function parseConfig(config, files, cwd, caseSensitive) {
  const canonical = name => caseSensitive ? ts.getNormalizedAbsolutePath(name, cwd) : ts.getNormalizedAbsolutePath(name, cwd).toLowerCase();
  const byPath = new Map(files.map(file => [canonical(file.path), file.text]));
  const host = { useCaseSensitiveFileNames: caseSensitive,
    fileExists: name => byPath.has(canonical(name)), readFile: name => byPath.get(canonical(name)),
    readDirectory(directory, extensions, excludes, includes, depth) {
      return ts.matchFiles(directory, extensions, excludes, includes, caseSensitive, cwd, depth, dir => {
        const prefix = canonical(dir).replace(/\/$/, "") + "/", names = [], directories = new Set();
        for (const file of files) {
          if (!canonical(file.path).startsWith(prefix)) continue;
          const relative = file.path.slice(prefix.length), separator = relative.indexOf("/");
          if (separator < 0) names.push(relative); else directories.add(relative.slice(0, separator));
        }
        return { files: names, directories: [...directories] };
      }, name => ts.getNormalizedAbsolutePath(name, cwd));
    } };
  const source = ts.parseJsonText(config.path, config.text);
  const parsed = ts.parseJsonSourceFileConfigFileContent(source, host, ts.getDirectoryPath(config.path), undefined, config.path);
  assert.deepEqual(source.parseDiagnostics, []);
  assert.deepEqual(parsed.errors, []);
  return parsed;
}

function directiveInput(row, expansion, configPlans) {
  const entry = expansion.cases.find(entry => entry.id === row.id);
  assert.ok(entry, row.id);
  const fixture = (row.suite === "compiler" ? expansion.compiler_fixtures : expansion.fixtures).find(f => f.source === entry.source);
  const source = expansion.sources[entry.source];
  for (const key of Object.keys(row.source)) assert.deepEqual(source[key], row.source[key]);
  const text = pinnedSource(row.suite, row.source);
  assert.equal(sha256(text), fixture.decoded_sha256);
  const { units, links } = unitsFromSource(text, row.source.path);
  assert.deepEqual(links, fixture.links);
  const configUnit = fixture.virtual_config ? units.find(unit => unit.name === fixture.virtual_config.name) : null;
  if (configUnit) verifyUnits([configUnit], [fixture.virtual_config]);
  const normal = units.filter(unit => unit !== configUnit);
  verifyUnits(normal, fixture.normal_units);
  const index = row.suite === "compiler" ? entry.configuration.configuration : entry.configuration;
  const settings = new Map(fixture.settings.map(s => [s.name, s.value]));
  for (const setting of fixture.configurations[index].settings) settings.set(setting.name, setting.value);
  const cwd = ts.getNormalizedAbsolutePath(settings.get("currentDirectory") ?? "/.src", "/.src");
  const config = configUnit ? { path: ts.getNormalizedAbsolutePath(configUnit.name, cwd), text: configUnit.text } : null;
  const allFiles = units.map(unit => ({ path: ts.getNormalizedAbsolutePath(unit.name, cwd), text: unit.text }));
  const parsed = config ? parseConfig(config, allFiles, cwd, false) : null;
  const options = parsed ? ts.cloneCompilerOptions(parsed.options) : { noResolve: false };
  Object.assign(options, { newLine: ts.NewLineKind.CarriageReturnLineFeed, noErrorTruncation: true, skipDefaultLibCheck: true });
  for (const [name, raw] of settings) setOption(options, name, raw);
  const candidates = [...new Map(normal.map((unit, id) => [ts.getNormalizedAbsolutePath(unit.name, cwd), id])).values()].sort((a, b) => a - b);
  const last = candidates.at(-1);
  const implicit = settings.has("noImplicitReferences") || normal[last].text.includes("require(") || /reference\s+path/.test(normal[last].text);
  const roots = parsed ? candidates.filter(id => parsed.fileNames.includes(ts.getNormalizedAbsolutePath(normal[id].name, cwd))) : implicit ? [last] : candidates;
  const other = candidates.filter(id => !roots.includes(id));
  const programRoots = roots.filter(id => !normal[id].name.endsWith(".json") && ts.isSupportedSourceFileName(normal[id].name, options));
  if (parsed && row.suite === "compiler") {
    const plan = configPlans.fixtures.find(plan => plan.source.index === entry.source);
    assert.deepEqual(parsed.fileNames, plan.parsed_file_names);
    assert.deepEqual(programRoots.map(id => units.indexOf(normal[id])), plan.program_root_unit_ids);
  }
  const files = [...roots, ...other].map(id => ({ path: ts.getNormalizedAbsolutePath(normal[id].name, cwd), text: normal[id].text }));
  const symlinks = new Map();
  for (const link of links) {
    const target = ts.getNormalizedAbsolutePath(link.target, cwd), destination = ts.getNormalizedAbsolutePath(link.link_path, cwd);
    const matches = files.filter(file => file.path === target || file.path.startsWith(target + "/"));
    assert.ok(matches.length, link.target);
    for (const file of matches) symlinks.set(destination + file.path.slice(target.length), file.path);
  }
  for (const [id, unit] of normal.entries()) for (const link of fixture.normal_units[id].document_symlinks) symlinks.set(ts.getNormalizedAbsolutePath(link, cwd), ts.getNormalizedAbsolutePath(unit.name, cwd));
  return { settings: [...settings], selection: { root_unit_ids: roots, other_unit_ids: other,
    program_root_unit_ids: programRoots, vfs_write_order: [...roots, ...other] },
    effective_options: serialOptions(options), input: { route: "whole-program", current_directory: cwd,
      use_case_sensitive_file_names: settings.get("useCaseSensitiveFileNames")?.toLowerCase() !== "false",
      roots: programRoots.map(id => ts.getNormalizedAbsolutePath(normal[id].name, cwd)), files, config,
      vfs_symlinks: [...symlinks].map(([link_path, target_path]) => ({ link_path, target_path })),
      shared_mount: null, default_library_file_name: null }, project_descriptor: null };
}

function projectInput(row, expansion, classification, projectFiles) {
  const descriptor = JSON.parse(pinnedSource("project", row.source));
  const recorded = classification.cases.find(entry => entry.id === row.id);
  const entry = expansion.cases.find(entry => entry.id === row.id);
  assert.equal(entry.source, recorded.source);
  assert.equal(entry.configuration.module, recorded.module_variant.name);
  const cwd = ts.getNormalizedAbsolutePath(descriptor.projectRoot, "/.src");
  assert.equal(cwd, recorded.current_directory);
  const byPath = new Map(projectFiles.map(file => [file.path, file]));
  const configPath = recorded.root_selection.config?.path;
  const config = configPath ? byPath.get(configPath) : null;
  const parsed = config ? parseConfig(config, projectFiles, cwd, true) : null;
  const roots = parsed?.fileNames ?? descriptor.inputFiles.map(name => ts.getNormalizedAbsolutePath(name, cwd));
  assert.deepEqual(roots, recorded.root_selection.roots.map(root => root.path), row.id);
  const options = { ...parsed?.options, moduleResolution: parsed?.options.moduleResolution ?? ts.ModuleResolutionKind.Classic,
    noErrorTruncation: false, skipDefaultLibCheck: false, newLine: ts.NewLineKind.CarriageReturnLineFeed };
  for (const [name, raw] of Object.entries(descriptor)) if (!structuralProjectNames.has(name)) setOption(options, name, raw);
  options.module = recorded.module_variant.value;
  // projectsRunner.ts createCompilerOptions resolves these before config parsing.
  for (const name of ["mapRoot", "sourceRoot"]) if (descriptor[`resolve${name[0].toUpperCase()}${name.slice(1)}`] && descriptor[name]) options[name] = ts.getNormalizedAbsolutePath(descriptor[name], "/.src");
  return { settings: null, selection: recorded.root_selection, project_descriptor: descriptor,
    effective_options: serialOptions(options), input: { route: "whole-program", current_directory: cwd,
      use_case_sensitive_file_names: true, roots, files: [], config,
      vfs_symlinks: [], shared_mount: "projects", default_library_file_name: "lib.es5.d.ts" } };
}

function transpileInput(row, inventory) {
  const entry = inventory.cases.find(entry => entry.id === row.id);
  const fixture = inventory.fixtures.find(fixture => fixture.source === entry.source);
  const { units, links } = unitsFromSource(pinnedSource("transpile", row.source), row.source.path);
  verifyUnits(units, fixture.units); assert.deepEqual(links, []);
  const settings = new Map(fixture.settings.map(s => [s.name, s.value]));
  for (const setting of fixture.configurations[entry.configuration].overrides) settings.set(setting.name, setting.value);
  const options = { noResolve: false, newLine: ts.NewLineKind.CarriageReturnLineFeed, noErrorTruncation: true, skipDefaultLibCheck: true };
  for (const [name, raw] of settings) setOption(options, name, raw);
  return { settings: [...settings], selection: null, project_descriptor: null, effective_options: serialOptions(options),
    input: { route: "transpile-api", api: entry.api, report_diagnostics: entry.report_diagnostics,
      units: units.map(({ name, text }) => ({ name, text })) } };
}

function remainingOwners(row, prepared) {
  const owners = row.required_slices.filter(owner => owner >= "H2.7d" || !owner.startsWith("H2."));
  const added = [], options = prepared.effective_options;
  const add = (owner, reason) => { owners.push(owner); added.push({ owner, reason }); };
  if (options.outFile !== undefined || options.out !== undefined) add("H2.7d", "effective outFile/out");
  if (options.declarationMap === true) add("H2.7e", "effective declarationMap=true");
  if (options.rootDir !== undefined || options.outDir !== undefined) add("H2.8a", "effective rootDir/outDir");
  if (options.composite === true) add("H2.8b", "effective composite=true (build/reference runtime remains BLD1)");
  if (options.noCheck === true || prepared.input.route === "transpile-api") add("H2.8c", "noCheck/transpile API");
  if (options.noEmit === true) add("H2.9", "effective noEmit=true retained outside emitter admission");
  if (prepared.input.use_case_sensitive_file_names === false) add("H2.8b", "explicit case-insensitive host");
  return { required_slices: unique(owners), effective_owner_reasons: added,
    added_slices: unique(owners.filter(owner => !row.required_slices.includes(owner))) };
}

export function prepare() {
  assert.equal(ts.version, "6.0.3");
  assert.equal(process.versions.node, fs.readFileSync(path.join(root, ".node-version"), "utf8").trim());
  const paths = ["ratchets/h2-candidate-dispositions.v1.json", "ratchets/h2-6c-qualification.v1.json",
    "ratchets/h2-7b-qualification.v1.json", "ratchets/h2-7c-qualification.v1.json",
    "vendor/typescript-6.0.3/test-suite-expansion.v1.json", "vendor/typescript-6.0.3/conformance-suite-expansion.v1.json",
    "vendor/typescript-6.0.3/compiler-config-plans.v1.json", "vendor/typescript-6.0.3/project-profile-classification.v1.json",
    "vendor/typescript-6.0.3/transpile-suite-inventory.v1.json"];
  const [global, ...rest] = paths.map(read), [h26c, h27b, h27c, test, conformance, plans, project, transpile] = rest;
  const parents = new Map([["H2.6c", h26c], ["H2.7b", h27b], ["H2.7c", h27c]]);
  const selected = global.cases.filter(row => row.required_slices.some(owner => ["H2.7d", "H2.7e"].includes(owner)));
  assert.equal(selected.length, 325);
  assert.equal(new Set(selected.map(row => row.id)).size, selected.length);
  const projectSources = test.sources.filter(source => source.suite === "projects");
  const projectFiles = projectSources.map(source => ({ path: "/.src/tests/cases/projects/" + source.path,
    text: pinnedSource("projects", source) }));
  const prepared = selected.map(row => ({ case_id: row.id, suite: row.suite, source: row.source,
    ...row.suite === "project" ? projectInput(row, test, project, projectFiles)
      : row.suite === "transpile" ? transpileInput(row, transpile)
      : directiveInput(row, row.suite === "compiler" ? test : conformance, plans) }));
  const cases = selected.map((row, index) => {
    const overlaps = [];
    for (const [name, parent] of parents) {
      const found = parent.cases.find(entry => entry.case_id === row.id);
      if (!found) continue;
      assert.deepEqual(found.source, row.source, row.id);
      overlaps.push({ parent: name, disposition: found.disposition, remaining_slices: found.remaining_slices ?? found.required_slices });
    }
    const owners = remainingOwners(row, prepared[index]);
    return { case_id: row.id, suite: row.suite, source: row.source, original_required_slices: row.required_slices,
      ...owners, parent_membership: overlaps, input_sha256: sha256(JSON.stringify(prepared[index])),
      disposition: "candidate-only", runtime_admitted: false };
  });
  for (const [name, parent] of parents) for (const row of parent.cases) {
    const later = row.remaining_slices ?? row.required_slices;
    if (later.some(owner => ["H2.7d", "H2.7e"].includes(owner))) assert.ok(cases.some(entry => entry.case_id === row.case_id), `${name}: missing successor ${row.case_id}`);
  }
  const count = predicate => cases.filter(predicate).length;
  const summary = { unique_candidates: cases.length, h2_7d: count(row => row.required_slices.includes("H2.7d")),
    h2_7e: count(row => row.required_slices.includes("H2.7e")),
    d_and_e: count(row => ["H2.7d", "H2.7e"].every(owner => row.required_slices.includes(owner))),
    newly_found_later_intersections: count(row => row.added_slices.length),
    whole_program_inputs: prepared.filter(row => row.input.route === "whole-program").length,
    transpile_controls: prepared.filter(row => row.input.route === "transpile-api").length,
    runtime_admitted: 0, project_mount_files: projectFiles.length,
    by_remaining_slices: Object.fromEntries(unique(cases.map(row => row.required_slices.join(","))).map(key => [key, count(row => row.required_slices.join(",") === key)])) };
  const inventory = { schema: 1, kind: "h2-7de-candidates", status: "candidate-inputs-only", typescript: ts.version,
    source_commit: sourceCommit, generator: identity("crates/oracle/h2-7de-candidates.mjs"), inputs: paths.map(identity),
    selection_contract: "Path and case-ID join of global H2.7d/e rows with frozen H2.6c/H2.7b/H2.7c membership; union, never addition. Original owners are retained and effective options may add later intersections. No Rust admission or H2 closure.",
    project_contract: "Full pinned 233-file project mount, original roots/config and descriptor. resolveMapRoot/resolveSourceRoot resolve against /.src as in pinned projectsRunner.ts:448-480; previous H2.6c/H2.7b project observers kept raw descriptor values. Their evidence is unchanged and is not transferred to this new input route.",
    cases, summary };
  const inputs = { schema: 1, kind: "h2-7de-candidate-inputs", typescript: ts.version, source_commit: sourceCommit,
    shared_mounts: { projects: projectFiles }, cases: prepared };
  return { inventory, inputs };
}

export function persist(name, value, mode) {
  const text = JSON.stringify(value, null, 2) + "\n";
  assert.ok(!text.includes(root), "local workspace path escaped into artifact");
  if (mode === "--write") fs.writeFileSync(path.join(root, name), text);
  else assert.equal(fs.readFileSync(path.join(root, name), "utf8"), text, `${name} is stale`);
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const mode = process.argv[2]; assert.ok(["--write", "--check"].includes(mode), "use --write or --check");
  const { inventory, inputs } = prepare();
  persist(inputPath, inputs, mode); persist(inventoryPath, inventory, mode);
  console.log(JSON.stringify(inventory.summary));
}
