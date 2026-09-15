// Runtime control for A6-41-SUPER: executes the upstream and native
// JavaScript outputs of each executable witness in the same Node runtime and
// compares their event logs (evaluation order and counts of key/rhs/get/set/
// call/tag events). Supplementary evidence only: it never replaces the
// complete-command tuple comparison, and cases whose emitted JavaScript uses
// syntax this Node cannot parse (native decorators or auto-accessors) are
// reported as not executable rather than as agreement.
//
// usage: node scripts/decorator-super-runtime-control.mjs <captures-dir> <out.json> [--extra|--followup|--followup2|--followup3]
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import zlib from "node:zlib";
import { spawnSync } from "node:child_process";
const root = path.resolve(import.meta.dirname, "..");
const [,, capturesDir, outPath] = process.argv;
const extra = process.argv.includes("--extra");
const followup = process.argv.includes("--followup");
const followup2 = process.argv.includes("--followup2");
const followup3 = process.argv.includes("--followup3");
const fixture = JSON.parse(zlib.zstdDecompressSync(fs.readFileSync(path.join(root, followup3
  ? "crates/compiler/tests/fixtures/decorator-super-followup3.json.zst"
  : followup2
  ? "crates/compiler/tests/fixtures/decorator-super-followup2.json.zst"
  : followup
  ? "crates/compiler/tests/fixtures/decorator-super-followup.json.zst"
  : extra
  ? "crates/compiler/tests/fixtures/decorator-super-extra.json.zst"
  : "crates/compiler/tests/fixtures/decorator-super.json.zst"))).toString("utf8"));
const captures = new Map();
for (const name of fs.readdirSync(capturesDir)) {
  if (!name.endsWith(".json")) continue;
  const capture = JSON.parse(fs.readFileSync(path.join(capturesDir, name), "utf8"));
  if (!captures.has(capture.case_id)) captures.set(capture.case_id, capture);
}
const SUFFIX = `\n;console.log(JSON.stringify(events, (k, v) => typeof v === "bigint" ? v.toString() + "n" : v));\n`;
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "dec-super-runtime-"));
function run(js, label) {
  const file = path.join(tmp, label + ".mjs");
  fs.writeFileSync(file, js + SUFFIX);
  const result = spawnSync(process.execPath, ["--no-warnings", file], { encoding: "utf8", timeout: 20000 });
  return { status: result.status, stdout: result.stdout, stderr: result.stderr.split(tmp).join("<tmp>") };
}
const jsOf = writes => { const w = writes.find(w => w.path === "/project/out/main.js"); return w ? Buffer.from(w.callback_utf8_base64, "base64").toString("utf8") : null; };
const rows = [];
let agree = 0, disagree = 0, notExecutable = 0, missing = 0, skipped = 0;
for (const c of fixture.cases) {
  const [, target, mode] = c.case_id.split("/");
  if (target === "esnext" && mode === "define") { skipped++; continue; }
  const expectedJs = jsOf(c.typescript_observation.writes);
  const capture = captures.get(c.case_id);
  if (!expectedJs) { skipped++; continue; }
  if (!capture?.actual) { missing++; rows.push({ case_id: c.case_id, outcome: "missing-native-capture" }); continue; }
  const actualJs = jsOf(capture.actual.writes);
  if (!actualJs) { missing++; rows.push({ case_id: c.case_id, outcome: "missing-native-js" }); continue; }
  const upstream = run(expectedJs, "u"), native = run(actualJs, "n");
  const parseFailure = r => r.status !== 0 && /SyntaxError/.test(r.stderr);
  if (parseFailure(upstream)) { notExecutable++; rows.push({ case_id: c.case_id, outcome: "not-executable-upstream", stderr: upstream.stderr.slice(0, 300) }); continue; }
  const same = upstream.status === native.status && upstream.stdout === native.stdout;
  if (same) agree++; else disagree++;
  rows.push({ case_id: c.case_id, outcome: same ? "agree" : "disagree", upstream_status: upstream.status, native_status: native.status,
    upstream_events: upstream.stdout.trim(), native_events: native.stdout.trim(), upstream_stderr: upstream.stderr.slice(0, 500), native_stderr: native.stderr.slice(0, 500) });
}
fs.rmSync(tmp, { recursive: true, force: true });
const summary = { captures_dir: capturesDir, executable_cases: agree + disagree, agree, disagree, not_executable_upstream: notExecutable, missing_native: missing, skipped_esnext_define_or_no_js: skipped };
fs.writeFileSync(outPath, JSON.stringify({ summary, rows }, null, 2) + "\n");
console.log(JSON.stringify(summary));
