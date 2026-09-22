import fs from "node:fs";
import crypto from "node:crypto";
import assert from "node:assert/strict";
import ts from "/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/vendor/typescript-6.0.3/lib/typescript.js";

const cases = [
  ["empty-variable", "export {};\nconst = 5;\n"],
  ["ordinary-variable", "export const value = 5;\n"],
  ["empty-module", "export {};\n"],
  ["explicit-directive", '"use strict";\nexport const value = 5;\n'],
  ["erased-declaration", "declare const source: any;\nexport const value = 5;\n"],
  ["async-helper", "export async function f() { return 1; }\n"],
  ["terminal-trivia", "export const value = 5; /* trailing */\n// last\n"],
  ["initial-trivia", "/* 😀 */\n\nexport const value = 5;\n"],
];
const settings = [
  ["strict-false", {strict: false}],
  ["strict-true", {strict: true}],
  ["always-strict", {strict: false, alwaysStrict: true}],
  ["strict-with-override", {strict: true, alwaysStrict: false}],
];
const rows = [];
const array = a => a && ({pos: a.pos, end: a.end, count: a.length,
  elements: a.map(n => ({kind: ts.SyntaxKind[n.kind], pos: n.pos, end: n.end,
    emitFlags: ts.getEmitFlags(n), text: ts.isExpressionStatement(n) && ts.isStringLiteral(n.expression) ? n.expression.text : null}))});
for (const target of [ts.ScriptTarget.ES5, ts.ScriptTarget.ES2015]) {
  for (const [name, text] of cases) for (const [mode, extra] of settings) {
    const fileName = "/project/main.ts";
    const options = {target, module: ts.ModuleKind.System, sourceMap: true, noLib: true, ...extra};
    const source = ts.createSourceFile(fileName, text, target, true);
    const row = {name, mode, target, text, parse_diagnostics: source.parseDiagnostics.map(d => d.code), input: array(source.statements)};
    const host = {...ts.createCompilerHost(options), getCurrentDirectory: () => "/project",
      fileExists: p => p === fileName, readFile: p => p === fileName ? text : undefined,
      getSourceFile: p => p === fileName ? source : undefined, writeFile() {}};
    const program = ts.createProgram([fileName], options, host);
    const writes = [];
    const result = program.emit(undefined, (path, text) => writes.push({path, text}), undefined, false, {
      after: [() => root => {
        const bodies = [];
        const visit = node => {
          if (ts.isPropertyAssignment(node) && ts.isIdentifier(node.name) && node.name.text === "execute"
              && ts.isFunctionExpression(node.initializer)) {
            const b = node.initializer.body;
            bodies.push({pos: b.pos, end: b.end, statements: array(b.statements)});
          }
          ts.forEachChild(node, visit);
        };
        visit(root);assert.equal(bodies.length, 1);row.execute = bodies[0];return root;
      }],
    });
    assert.equal(result.emitSkipped, false);
    row.writes = writes;rows.push(row);
  }
}
const output = {typescript: ts.version, scope: "Read-only upstream transform metadata inspection, noLib to avoid irrelevant library loading; not complete-command compatibility qualification.",
  script_sha256: crypto.createHash("sha256").update(fs.readFileSync(import.meta.filename)).digest("hex"), rows};
process.stdout.write(JSON.stringify(output, null, 2) + "\n");
