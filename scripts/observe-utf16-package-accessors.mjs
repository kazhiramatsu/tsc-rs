// Public TypeScript package-object and filename operations; no compiler qualification.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const hash = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = hash(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const units = value => Array.from({ length: value.length }, (_, i) => value.charCodeAt(i));
const describe = value => value === undefined ? { kind: "absent" } : typeof value === "string" ? { kind: "string", units: units(value) } : { kind: typeof value, null: value === null, array: Array.isArray(value) };
const queryKeys = ["name", "type", "1", "2", "01", "\0key", "\ud800", "\ud801", "__proto__", "inherited"];
function observePackage(text) {
  const read = ts.readJson("/pkg/package.json", { readFile: () => text });
  const object = read && typeof read === "object" && !Array.isArray(read) ? read : {};
  return {
    text,
    entries: Object.keys(object).map(key => ({ key: units(key), value: describe(object[key]) })),
    properties: queryKeys.map(key => ({ key: units(key), inherited: describe(object[key]), own: describe(Object.hasOwn(object, key) ? object[key] : undefined) })),
  };
}
function observe() {
  const texts = [
    '{"name":" \\ud800 ","type":"\\ud801","2":"two","1":"one","01":"padded","\\u0000key":"nul","\\ud800":"high","\\ud801":"other"}',
    '{/* JSONC */"__proto__":{"name":" \\ud800 ","inherited":"\\ud801"},"2":"two","1":"one","\\u0000key":"nul","\\ud800":"high",}',
    '{"__proto__":{"inherited":"strict own prototype property"},"name":"  pkg  "}',
    '{"name":', '', '[]', 'null',
  ];
  const names = ["pkg", "@scope/pkg", "@scope", "@scope/pkg/sub", "@@scope/pkg", "@\ud800/\ud801", "@\ud800"];
  const fileNames = ["/A/\ud800/\ud801/FILE.TS", "/\u0130\u0131\u00df/\u{10400}/\u03a3\ud800\u03a3.ts"];
  return {
    packages: texts.map(observePackage),
    scoped_names: names.map(name => ({ name: units(name), mangled: units(ts.mangleScopedPackageName(name)) })),
    file_names: fileNames.map(name => ({ name: units(name), folded: units(ts.toFileNameLowerCase(name)) })),
  };
}
const first = observe(); assert.deepEqual(observe(), first);
const artifact = { version: 1, scope: "readJson object view, inherited/own property access, Object.keys order, scoped package mangling and filename casing; no complete-command qualification", typescript: ts.version, compiler_sha256: compilerSha, observer_sha256: hash(fs.readFileSync(import.meta.filename)), repetitions: 2, ...first };
const output = path.join(root, "crates/program/tests/fixtures/utf16-package-accessors.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", { flag: "wx" });
else assert.deepEqual(JSON.parse(fs.readFileSync(output, "utf8")), artifact);
console.log(JSON.stringify({ output, sha256: hash(fs.readFileSync(output)), packages: first.packages.length, scoped_names: first.scoped_names.length, file_names: first.file_names.length, repetitions: 2 }));
