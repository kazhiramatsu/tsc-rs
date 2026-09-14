// Program JSON ownership/serialization API evidence, not module resolution.
// node scripts/observe-utf16-json-values.mjs --write|--check
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";

const root = path.resolve(import.meta.dirname, "..");
const sha = data => crypto.createHash("sha256").update(data).digest("hex");
const compilerSha = sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const units = text => Array.from({length: text.length}, (_, i) => text.charCodeAt(i));
const cases = [
  {id: "strict-distinct", source: String.raw`{"\uD800":"\uD800","\uD801":"\uD801","\uDC00":"\uDC00","\uFFFD":"\uFFFD","\\uD800":"\\uD800","ascii":"__call"}`},
  {id: "strict-pair-equivalence", source: String.raw`{"\uD83D\uDE00":"first","😀":"last","10":"ten","2":"two"}`},
  {id: "jsonc-values", source: String.raw`{/* comments force convertToJson */"keys":["\ud800","\ud801","\udc00","\ufffd","\\ud800","\ud83d\ude00"],}`},
  {id: "strict-prototype-and-surrogate", source: String.raw`{"\uD800":"retained","__proto__":{"\uD801":"own, not inherited"}}`},
  {id: "jsonc-inherited-and-marker", source: String.raw`{/* convertToJson */"__proto__":{"\uD800":"inherited"},"\u0000tsc-rs:jsonc-prototype\u0000":"\uD801","own":"\uDC00"}`},
  {id: "jsonc-null-then-own-prototype", source: String.raw`{/* convertToJson */"__proto__":null,"__proto__":{"\uD800":"own"},"\uD801":"value"}`},
  {id: "paths-values", source: String.raw`{"compilerOptions":{"paths":{"\uD800*\uDC00":["./mapped","./\uD801/*"],"*": ["./fallback"]}}}`},
  {id: "unrelated-surrogate-description", source: String.raw`{"name":"pkg","description":"\uD800","main":"./index.js","types":"./index.d.ts"}`},
];
function observe(value) {
  if (value === null) return ["null"];
  switch (typeof value) {
    case "string": return ["string", units(value), JSON.stringify(value)];
    case "boolean": return ["boolean", value];
    case "number": return ["number", String(value)];
    case "object": {
      if (Array.isArray(value)) return ["array", value.map(observe)];
      const prototype = Object.getPrototypeOf(value);
      return ["object", Object.keys(value).map(key => [units(key), observe(value[key])]),
        prototype === Object.prototype ? "default" : prototype === null ? "null" : observe(prototype)];
    }
    default: throw new Error(`Unexpected JSON value: ${typeof value}`);
  }
}
const observed = cases.map(input => {
  const run = () => observe(ts.readJson("package.json", {readFile: () => input.source}));
  const expected = run();
  assert.deepEqual(run(), expected, input.id);
  return {...input, expected};
});
const quotedUnits = [
  [], [0, 1, 8, 9, 10, 12, 13, 31, 34, 92], [0xd800], [0xd801], [0xdc00],
  [0xfffd], [92, 117, 100, 56, 48, 48], [0xd83d, 0xde00],
  [0xd800, 0xd800, 0xdc00, 0xdc00], [0x2028, 0x2029, 0xe000, 0xffff],
];
const quoting = quotedUnits.map(input => {
  const value = String.fromCharCode(...input);
  const json = JSON.stringify(value);
  assert.equal(JSON.stringify(value), json);
  assert.deepEqual(units(JSON.parse(json)), input);
  return {units: input, json};
});
const content = JSON.stringify({
  scope: "TypeScript readJson ownership and JSON.stringify quoting only; module resolution and commands are unqualified",
  typescript_version: ts.version, compiler_sha256: compilerSha,
  observer_sha256: sha(fs.readFileSync(import.meta.filename)),
  repetitions: 2, cases: observed, quoting,
}, null, 2) + "\n";
const output = path.join(root, "crates/program/tests/fixtures/utf16-json-values.json");
if (process.argv[2] === "--write") {
  fs.mkdirSync(path.dirname(output), {recursive: true});
  fs.writeFileSync(output, content, {flag: "wx"});
} else if (process.argv[2] === "--check") assert.equal(fs.readFileSync(output, "utf8"), content);
else throw new Error("Expected --write or --check");
console.log(JSON.stringify({output, sha256: sha(fs.readFileSync(output)), cases: cases.length, repetitions: 2, quoting: quoting.length}));
