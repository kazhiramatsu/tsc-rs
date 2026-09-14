// Value domain of the supported RegExp subset used by module specifier exclusion.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const hash = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = hash(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const units = value => Array.from({length: value.length}, (_, i) => value.charCodeAt(i));
const inputs = [
  ["lone-high-dot", "^.$", "", "\ud800"], ["lone-low-dot-u", "^.$", "u", "\udc00"],
  ["pair-one-unit", "^.$", "", "\ud800\udc00"], ["pair-two-units", "^..$", "", "\ud800\udc00"],
  ["pair-one-point", "^.$", "u", "\ud800\udc00"], ["pair-two-points", "^..$", "u", "\ud800\udc00"],
  ["literal-pair-units", "^𐀀+$", "", "𐀀\udc00"], ["literal-pair-points", "^𐀀+$", "u", "𐀀\udc00"],
  ["line-separator-dot", "^.$", "", "\u2028"], ["paragraph-separator-dot-u", "^.$", "u", "\u2029"],
  ["bom-whitespace", "^\\s$", "", "\ufeff"], ["nel-not-whitespace", "^\\s$", "", "\u0085"],
];
function observe() { return inputs.map(([id, source, flags, value]) => ({id, pattern: "/" + source + "/" + flags, value: units(value), matched: new RegExp(source, flags).test(value)})); }
const cases = observe(); assert.deepEqual(observe(), cases);
const artifact = {version: 1, scope: "supported specifier exclusion RegExp subset: UTF-16 versus Unicode mode, literal pairs and JS whitespace/dot rules; not a full RegExp implementation claim", typescript: ts.version, compiler_sha256: compilerSha, observer_sha256: hash(fs.readFileSync(import.meta.filename)), repetitions: 2, cases};
const output = path.join(root, "crates/checker/tests/fixtures/utf16-specifier-regex-values.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", {flag: "wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output, "utf8")), artifact);
console.log(JSON.stringify({output, sha256: hash(fs.readFileSync(output)), cases: cases.length, repetitions: 2}));
