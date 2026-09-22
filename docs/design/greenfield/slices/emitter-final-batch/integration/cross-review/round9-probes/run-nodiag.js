const ts = require(process.env.TS_BUNDLE || "/tmp/tsrs-probe/typescript.js");
const path = require("path");
const fs = require("fs");
const LIB = "/Users/hiramatsu/dev/tsc-rs-emitter-final-audit/vendor/typescript-6.0.3/lib";
const residuals = JSON.parse(fs.readFileSync("/Users/hiramatsu/dev/tsc-rs-emitter-final-audit/docs/design/greenfield/slices/emitter-final-batch/integration/cross-review/checker-residuals.json","utf8"));
const which = process.argv[2];
const rec = residuals.find(r => r.case_id.includes(which));
const files = new Map(rec.input.files.map(f => [f.path, f.text]));
const opts = { target: ts.ScriptTarget.ES2015, noEmit: true };
for (const [k,v] of rec.settings) {
  const key = k[0].toLowerCase()+k.slice(1);
  if (["filename","fileName","noTypesAndSymbols"].includes(k) || key==="filename") continue;
  if (key==="target") opts.target = ts.ScriptTarget.ES2015; else if (key==="lib") opts.lib=["lib."+v+".d.ts"]; else opts[key]= v==="true"?true: v==="false"?false: v;
}
if (opts.module==="commonjs") opts.module = ts.ModuleKind.CommonJS;
const host = {
  getSourceFile(f, lv) { let t; if (files.has(f)) t = files.get(f); else if (f.startsWith("/lib/")) { const p = path.join(LIB, path.basename(f)); if (fs.existsSync(p)) t = fs.readFileSync(p,"utf8"); } if (t===undefined) return undefined; return ts.createSourceFile(f, t, lv, true); },
  getDefaultLibFileName: o => "/lib/" + ts.getDefaultLibFileName(o),
  writeFile(){}, getCurrentDirectory: () => rec.input.current_directory, getCanonicalFileName: f=>f, useCaseSensitiveFileNames: ()=>true, getNewLine: ()=>"\n",
  fileExists: f => files.has(f) || (f.startsWith("/lib/") && fs.existsSync(path.join(LIB, path.basename(f)))),
  readFile: f => files.get(f) ?? (f.startsWith("/lib/") ? fs.readFileSync(path.join(LIB, path.basename(f)),"utf8") : undefined),
  directoryExists: d => true, getDirectories: () => [],
};
const program = ts.createProgram(rec.input.roots, opts, host);
const diags = [];
for (const d of diags) console.log(d.code, d.file && d.file.fileName, d.start, d.length, ts.flattenDiagnosticMessageText(d.messageText, "\n"));
module.exports = { ts, program };
if (process.env.PROBE) require(process.env.PROBE)({ ts, program, files });
