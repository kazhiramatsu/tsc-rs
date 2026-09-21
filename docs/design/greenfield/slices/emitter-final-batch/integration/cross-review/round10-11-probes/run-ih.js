const ts = require(process.env.TS_BUNDLE || "/tmp/tsrs-probe/typescript.js");
const path = require("path"), fs = require("fs");
const LIB = "/Users/hiramatsu/dev/tsc-rs-emitter-final-audit/vendor/typescript-6.0.3/lib";
const fx = JSON.parse(fs.readFileSync("/Users/hiramatsu/dev/tsc-rs-emitter-final-audit/crates/compiler/tests/fixtures/import-helpers.json","utf8"));
const entries = Array.isArray(fx) ? fx : (fx.cases || fx.records || fx.entries);
const rec = entries.find(e => e.case_id === process.argv[2]);
if (!rec) { console.error("no case; sample ids:", entries.slice(0,3).map(e=>e.case_id)); process.exit(1); }
const files = new Map(rec.files.map(f => [f.path, f.text]));
const cfg = JSON.parse(rec.config.text || rec.config);
const parsed = ts.convertCompilerOptionsFromJson(cfg.compilerOptions, "/project");
const opts = parsed.options; opts.noEmit = false;
const out = {};
const host = {
  getSourceFile(f, lv) { let t = files.get(f); if (t===undefined && f.startsWith("/lib/")) { const p = path.join(LIB, path.basename(f)); if (fs.existsSync(p)) t = fs.readFileSync(p,"utf8"); } return t===undefined? undefined : ts.createSourceFile(f, t, lv, true); },
  getDefaultLibFileName: o => "/lib/" + ts.getDefaultLibFileName(o),
  writeFile(f, text){ out[f]=text; }, getCurrentDirectory: () => "/project", getCanonicalFileName: f=>f, useCaseSensitiveFileNames: ()=>true, getNewLine: ()=>"\n",
  fileExists: f => files.has(f) || (f.startsWith("/lib/") && fs.existsSync(path.join(LIB, path.basename(f)))),
  readFile: f => files.get(f) ?? (f.startsWith("/lib/") ? fs.readFileSync(path.join(LIB, path.basename(f)),"utf8") : undefined),
  directoryExists: () => true, getDirectories: () => [],
};
const program = ts.createProgram(rec.roots.map(r => path.resolve("/project", r)), opts, host);
program.emit();
for (const [f,t] of Object.entries(out)) if (f.endsWith("main.js")) console.log(t.split("\n").filter(l=>l.includes("exports.")).join("\n"));
