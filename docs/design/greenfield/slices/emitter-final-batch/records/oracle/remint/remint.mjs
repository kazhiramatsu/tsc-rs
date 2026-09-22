// Standalone re-mint (proposal evidence only): the h2-6c-qualification host in its current shape
// ("old": useCaseSensitiveFileNames hard-coded true, identity canonical) versus the proposed shape
// ("new": harness directive honored through ts.createGetCanonicalFileName; VFS lookups keyed by the
// same canonicalization; original spellings preserved). Mirrors observeTypeScript/serializeWrite.
import { createRequire } from "node:module";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { createHermeticDirectoryOverlay } from "/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/oracle/vfs-directory-overlay.mjs";
const require = createRequire(import.meta.url);
const ts = require("/Users/hiramatsu/dev/tsc-rs-emitter-final/vendor/typescript-6.0.3/lib/typescript.js");
const VIRTUAL_SOURCE_ROOT = "/.src";
const sha256 = (buf) => createHash("sha256").update(buf).digest("hex");

function loadCase(file) {
  const text = fs.readFileSync(file, "utf8");
  const settings = new Map(); const units = []; let current = null;
  for (const line of text.split(/\r?\n/)) {
    const m = /^\/\/\s*@(\w+)\s*:\s*(.*)$/.exec(line);
    if (m) {
      const name = m[1], value = m[2].trim();
      if (name.toLowerCase() === "filename") { current = { name: value, lines: [] }; units.push(current); continue; }
      settings.set(name, value); continue;
    }
    if (current) current.lines.push(line);
  }
  const unitsOut = units.map((u) => ({ name: u.name, text: u.lines.join("\n").replace(/\n+$/, "\n") }));
  return { settings, units: unitsOut };
}
function optionsFrom(settings) {
  const json = {};
  for (const [name, raw] of settings) {
    if (["useCaseSensitiveFileNames", "filename", "fileName", "baselineFile"].includes(name)) continue;
    const option = ts.optionDeclarations.find((o) => o.name.toLowerCase() === name.toLowerCase());
    if (!option) continue;
    json[option.name] = option.type === "boolean" ? raw.toLowerCase() === "true" : raw;
  }
  const { options } = ts.convertCompilerOptionsFromJson(json, VIRTUAL_SOURCE_ROOT);
  options.noErrorTruncation = true; options.skipDefaultLibCheck = true; options.newLine = ts.NewLineKind.CarriageReturnLineFeed;
  return options;
}
function makeProgram(loaded, options, useCaseSensitiveFileNames) {
  const cwd = VIRTUAL_SOURCE_ROOT;
  const canonicalize = ts.createGetCanonicalFileName(useCaseSensitiveFileNames);
  const vfs = new Map(); // canonical key -> { spelled, text }
  for (const unit of loaded.units) {
    const spelled = ts.getNormalizedAbsolutePath(unit.name, cwd);
    vfs.set(canonicalize(spelled), { spelled, text: unit.text });
  }
  const baseHost = ts.createCompilerHost(options, true);
  const overlay = createHermeticDirectoryOverlay([...vfs.values()].map((v) => v.spelled), { currentDirectory: cwd, useCaseSensitiveFileNames, fallbackHost: baseHost });
  const host = {
    ...baseHost,
    getCurrentDirectory: () => cwd,
    useCaseSensitiveFileNames: () => useCaseSensitiveFileNames,
    getCanonicalFileName: canonicalize,
    trace() {},
    fileExists(f) { const n = ts.normalizePath(f); return vfs.has(canonicalize(n)) || baseHost.fileExists(n); },
    readFile(f) { const n = ts.normalizePath(f); return vfs.get(canonicalize(n))?.text ?? baseHost.readFile(n); },
    directoryExists: (d) => overlay.directoryExists(d),
    getDirectories: (d) => overlay.getDirectories(d),
    realpath(f) { const n = ts.normalizePath(f); return vfs.has(canonicalize(n)) ? n : (baseHost.realpath?.(n) ?? n); },
    getSourceFile(f, lv) { const n = ts.normalizePath(f); const e = vfs.get(canonicalize(n)); if (!e) return baseHost.getSourceFile(f, lv);
      return ts.createSourceFile(n, e.text, lv, true, ts.getScriptKindFromFileName(n)); },
  };
  const roots = loaded.units.map((u) => ts.getNormalizedAbsolutePath(u.name, cwd));
  return ts.createProgram(roots, options, host);
}
function observe(program) {
  const writes = [], reported = [], statusWrites = [];
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, (d) => reported.push(d), (t) => statusWrites.push(t), undefined, (...a) => writes.push(a));
  return {
    writes: writes.map(([fileName, text, bom]) => ({ path: ts.normalizePath(fileName), sha256: sha256(Buffer.from(text, "utf8")), bom: !!bom, text })),
    diagnostics: reported.map((d) => ({ code: d.code, file: d.file ? ts.normalizePath(d.file.fileName) : null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n") })),
    exit,
  };
}
const outDir = process.argv[2];
for (const file of process.argv.slice(3)) {
  const loaded = loadCase(file);
  const directive = (loaded.settings.get("useCaseSensitiveFileNames") ?? "true").toLowerCase() === "true";
  const name = path.basename(file, ".ts");
  const result = {};
  for (const [mode, flag] of [["old", true], ["new", directive]]) {
    const options = optionsFrom(loaded.settings);
    result[mode] = { host_use_case_sensitive_file_names: flag, ...observe(makeProgram(loaded, options, flag)) };
  }
  fs.writeFileSync(path.join(outDir, name + ".json"), JSON.stringify(result, null, 1));
  const summary = (r) => r.writes.map((w) => w.path + " " + w.sha256.slice(0, 12) + (w.path.endsWith(".map") ? " sources=" + JSON.stringify(JSON.parse(w.text).sources) : "")).join(" | ") + " || diagnostics " + JSON.stringify(r.diagnostics.map((d) => d.code)) + " exit " + r.exit;
  console.log("== " + name + " (directive " + directive + ")");
  console.log("  old: " + summary(result.old));
  console.log("  new: " + summary(result.new));
}
