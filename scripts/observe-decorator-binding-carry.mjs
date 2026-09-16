// Supplemental failure-carry controls: rejected candidates and skipped temp slots.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";

assert.equal(ts.version, "6.0.3");
const action = process.argv[2];
assert.ok(["--write", "--check"].includes(action));
const destination = new URL("../crates/emitter/tests/fixtures/decorator-binding-carry-edge.json", import.meta.url);
const hash = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const f = ts.factory;
const options = { newLine: ts.NewLineKind.CarriageReturnLineFeed };
const rows = [];
for (const [label, count] of [["collision", 1], ["skip-i", 8], ["skip-n", 13], ["numeric", 26]]) {
  const names = Array.from({ length: count }, (_, i) => "_" + String.fromCharCode(97 + i));
  for (const phase of ["before", "after"]) {
    rows.push({ case_id: `decorator-binding-direct/lifecycle/temp-ordinal/${label}/${phase}`,
      group: "lifecycle", op: "failure", kind: "temp", base: null, phase,
      source: `let _a = 1;\n_a;\n${names.map(name => name + ";").join("\n")}\n` });
  }
}
for (const [kind, base] of [["temp", null], ["scoped", "_s"]]) {
  rows.push({ case_id: `decorator-binding-direct/lifecycle/scope-entry/${kind}/unprinted-declaration`,
    group: "lifecycle", op: "scope-fault", kind, base, fault: "inside-nested", extra_nested: true });
}
function observeScope(row) {
  const sf = ts.createSourceFile("main.ts", "let x = 1;\nx;\n", ts.ScriptTarget.ESNext, true);
  const make = () => f.createVariableStatement(undefined, f.createVariableDeclarationList([
    f.createVariableDeclaration(row.kind === "temp" ? f.getGeneratedNameForNode(f.createEmptyStatement())
      : f.createUniqueName(row.base, ts.GeneratedIdentifierFlags.Optimistic | ts.GeneratedIdentifierFlags.ReservedInNestedScopes),
    undefined, undefined, f.createNumericLiteral(1))], ts.NodeFlags.Const));
  const d = Array.from({ length: 5 }, make);
  const fn = f.createFunctionDeclaration(undefined, undefined, "f", undefined, [], undefined, f.createBlock([d[2], d[4]], true));
  const file = f.updateSourceFile(sf, [d[0], d[1], fn, d[3]]);
  let fault = true;
  const shared = ts.createPrinter(options, {
    isEmitNotificationEnabled: node => node.kind === ts.SyntaxKind.VariableStatement,
    onEmitNode(hint, node, callback) {
      callback(hint, node);
      if (fault && node === d[2]) { fault = false; throw Error("injected"); }
    },
  });
  const results = [];
  const run = callback => {
    const op = results.length;
    try { results.push({ op, status: "returned", text: callback() }); }
    catch (error) { results.push({ op, status: "threw", error: error.message }); }
  };
  run(() => shared.printFile(file));
  assert.equal(fault, false);
  assert.equal(results[0].status, "threw");
  const next = make();
  run(() => shared.printNode(ts.EmitHint.Unspecified, next, sf));
  run(() => ts.createPrinter(options).printNode(ts.EmitHint.Unspecified, next, sf));
  return { results };
}
function observe(row) {
  if (row.op === "scope-fault") return observeScope(row);
  const sf = ts.createSourceFile("main.ts", row.source, ts.ScriptTarget.ESNext, true);
  const declarations = sf.statements.slice(0, 2).map(statement => f.createVariableStatement(undefined,
    f.createVariableDeclarationList([f.createVariableDeclaration(f.getGeneratedNameForNode(statement),
      undefined, undefined, f.createNumericLiteral(1))], ts.NodeFlags.Const)));
  let fault;
  const handlers = {
    isEmitNotificationEnabled: node => node.kind === ts.SyntaxKind.VariableStatement,
    onEmitNode(hint, node, callback) {
      if (node === declarations[0] && fault === "before") { fault = undefined; throw Error("injected"); }
      callback(hint, node);
      if (node === declarations[0] && fault === "after") { fault = undefined; throw Error("injected"); }
    },
  };
  const results = [];
  const print = (printer, node) => {
    const op = results.length;
    try {
      const text = printer.printNode(ts.EmitHint.Unspecified, node, sf);
      results.push({ op, status: "returned", text });
    } catch (error) { results.push({ op, status: "threw", error: error.message }); }
  };
  print(ts.createPrinter(options), declarations[0]);
  const shared = ts.createPrinter(options, handlers);
  fault = row.phase;
  print(shared, declarations[0]);
  assert.equal(fault, undefined);
  assert.equal(results[1].status, "threw");
  print(shared, declarations[1]);
  print(ts.createPrinter(options), declarations[1]);
  return { results };
}
const cases = rows.map(row => {
  const first = observe(row);
  assert.deepEqual(observe(row), first);
  return { ...row, typescript_observation: first };
});
const artifact = { version: 1, typescript: ts.version, repetitions: 2,
  compiler_sha256: hash(fs.readFileSync(new URL("../vendor/typescript-6.0.3/lib/typescript.js", import.meta.url))),
  observer_sha256: hash(fs.readFileSync(import.meta.filename)), cases };
if (action === "--write") {
  fs.writeFileSync(destination, JSON.stringify(artifact, null, 2) + "\n", { flag: "wx" });
} else {
  assert.deepEqual(JSON.parse(fs.readFileSync(destination, "utf8")), artifact);
}
console.log(JSON.stringify({ cases: cases.length, repetitions: 2, action }));
