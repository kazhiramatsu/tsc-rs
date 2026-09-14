// JavaScript replacement-string operations used by TypeScript's path helpers.
// This observes values, not filesystem resolution or complete commands.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
const root = path.resolve(import.meta.dirname, "..");
const sha = data => crypto.createHash("sha256").update(data).digest("hex");
const compilerSha = sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/_tsc.js")));
assert.equal(compilerSha, "1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3");
const units = value => Array.from({length: value.length}, (_, i) => value.charCodeAt(i));
const inputs = [
  ["empty", "", ""], ["no-star", "a\ud800b", "$$$&$`$'"],
  ["remove-star-forms-pair", "\ud800*\udc00", ""],
  ["replacement-forms-pair-left", "\ud800*", "\udc00"],
  ["replacement-forms-pair-right", "*\udc00", "\ud800"],
  ["distinct-surrogates", "a*/*b", "\ud801\ud800"],
  ["paired-input", "😀*😀*", "😀"],
  ["all-context-tokens", "ab*cd*ef", "$$|$&|$`|$'|$1|$<name>|$"],
  ["surrogate-context-tokens", "\ud800*\udc00*\ud801", "$`$'$&$$"],
  ["empty-contexts", "*", "$`$'$`$'"],
  ["unknown-dollar-token", "a*b", "$\ud800"],
  ["trailing-dollar", "a*b", "\ud800$"],
  ["literal-escape-spelling", "*/*", "\\uD800"],
  ["replacement-star-is-literal", "a*b*c", "*\ud800*"],
];
const cases = inputs.map(([id, target, replacement]) => {
  const observe = () => {
    const first = target.replace("*", replacement);
    const all = target.replace(/\*/g, replacement);
    return {first: units(first), all: units(all), first_bytes: Buffer.byteLength(first), all_bytes: Buffer.byteLength(all)};
  };
  const expected = observe();
  assert.deepEqual(observe(), expected, id);
  return {id, target: units(target), replacement: units(replacement), ...expected};
});
const content = JSON.stringify({
  scope: "String.replace first-star and global-star values only; Buffer byteLength agrees with canonical WTF-8 length but is not an observation of its surrogate bytes",
  compiler_sha256: compilerSha, observer_sha256: sha(fs.readFileSync(import.meta.filename)),
  node: process.version, repetitions: 2, cases,
}, null, 2) + "\n";
const output = path.join(root, "crates/program/tests/fixtures/utf16-string-replacement.json");
if (process.argv[2] === "--write") fs.writeFileSync(output, content, {flag: "wx"});
else if (process.argv[2] === "--check") assert.equal(fs.readFileSync(output, "utf8"), content);
else throw new Error("Expected --write or --check");
console.log(JSON.stringify({output, sha256: sha(fs.readFileSync(output)), cases: cases.length, repetitions: 2}));
