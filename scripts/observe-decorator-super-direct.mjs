// Direct transform controls for the A6-41-SUPER value-use memo: synthetic
// CommaListExpression and shared expression nodes reaching the
// standard-decorator visitor in two value-use contexts. No Program input can
// produce these shapes (no transform before transformESDecorators emits a
// CommaListExpression or shares an expression node), so each case injects the
// shape with a `customTransformers.before` transformer on the parsed source
// and records the emitted JavaScript of the complete Program emit.
// Supplementary to the complete-command witnesses; JavaScript text only.
//
// usage: node scripts/observe-decorator-super-direct.mjs --write|--check
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/emitter/tests/fixtures/decorator-super-direct.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") assert.ok(!fs.existsSync(destination), "retain existing observations");

const PRELUDE = `const events: unknown[] = [];
function dec(value: any, context: any): any { return value; }
function rhs() { events.push("rhs"); return 3; }
function record(value: unknown) { events.push(value); }
class Base {
    static stored = 1;
    static get x() { events.push(["get", this.name]); return this.stored; }
    static set x(value: number) { events.push(["set", this.name, value]); this.stored = value; }
}
`;
const TAIL = `record(Derived.stored);\n`;
const cls = statements => `@dec\nclass Derived extends Base {\n    static {\n${statements.map(s => `        ${s}`).join("\n")}\n    }\n}\n`;
// shape → [static block statements, description of the injected mutation]
const shapes = [
  ["comma-list-discarded", [`super.x = rhs(), record(super.x);`],
    "statement 0: the comma binary becomes a synthetic CommaListExpression (both elements discarded)"],
  ["comma-list-used", [`record((super.x = rhs(), super.x));`],
    "statement 0: the parenthesized comma argument becomes a CommaListExpression (first discarded, last used)"],
  ["comma-list-used-assignment-last", [`record((rhs(), super.x = rhs()));`],
    "statement 0: the parenthesized comma argument becomes a CommaListExpression whose used last element is the super assignment"],
  ["comma-list-nested-discarded", [`record((super.x = rhs(), (super.x = rhs(), super.x)));`],
    "statement 0: a CommaListExpression whose last element is a parenthesized comma (discarded head, used tail)"],
  ["shared-node-discarded-then-used", [`super.x = rhs();`, `record(super.x = rhs());`],
    "statement 1's argument is replaced by statement 0's expression node (same object: discarded first, then used)"],
  ["shared-node-used-then-discarded", [`record(super.x = rhs());`, `super.x = rhs();`],
    "statement 1's expression is replaced by statement 0's argument node (same object: used first, then discarded)"],
  ["shared-node-update-discarded-then-used", [`super.x++;`, `record(super.x++);`],
    "statement 1's argument is replaced by statement 0's postfix update node (discarded first, then used)"],
  // Recorded divergence, no credit: tsc lowers the shared node twice with a
  // second result temp; the port memoizes the required-value lowering per
  // node id and prints the first lowering twice (memo policy, memo §3/§4).
  ["shared-node-used-twice", [`record(super.x = rhs());`, `record(super.x = rhs());`],
    "statement 1's argument is replaced by statement 0's argument node (same object: used twice)"],
];
const targets = [["es2015", ts.ScriptTarget.ES2015], ["es2022", ts.ScriptTarget.ES2022]];

function flattenComma(node) {
  return ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.CommaToken
    ? [...flattenComma(node.left), node.right] : [node];
}
function injector(shape) {
  return context => sourceFile => {
    const factory = context.factory;
    const classDeclaration = sourceFile.statements.find(statement => ts.isClassDeclaration(statement) && statement.name?.text === "Derived");
    const block = classDeclaration.members.find(ts.isClassStaticBlockDeclaration);
    const statements = [...block.body.statements];
    const call = statement => { assert.ok(ts.isExpressionStatement(statement) && ts.isCallExpression(statement.expression)); return statement.expression; };
    const replaceArgument = (statement, argument) =>
      factory.updateExpressionStatement(statement, factory.updateCallExpression(call(statement), call(statement).expression, undefined, [argument]));
    if (shape.startsWith("comma-list")) {
      const statement = statements[0];
      if (shape === "comma-list-discarded") {
        assert.ok(ts.isExpressionStatement(statement));
        statements[0] = factory.updateExpressionStatement(statement, factory.createCommaListExpression(flattenComma(statement.expression)));
      } else {
        const argument = call(statement).arguments[0];
        assert.ok(ts.isParenthesizedExpression(argument));
        statements[0] = replaceArgument(statement, factory.createCommaListExpression(flattenComma(argument.expression)));
      }
    } else if (shape === "shared-node-discarded-then-used" || shape === "shared-node-update-discarded-then-used") {
      const shared = statements[0].expression;
      statements[1] = replaceArgument(statements[1], shared);
    } else if (shape === "shared-node-used-then-discarded") {
      const shared = call(statements[0]).arguments[0];
      statements[1] = factory.updateExpressionStatement(statements[1], shared);
    } else if (shape === "shared-node-used-twice") {
      const shared = call(statements[0]).arguments[0];
      statements[1] = replaceArgument(statements[1], shared);
    } else assert.fail(shape);
    const updatedBlock = factory.updateClassStaticBlockDeclaration(block, factory.updateBlock(block.body, statements));
    const updatedClass = factory.updateClassDeclaration(classDeclaration, classDeclaration.modifiers, classDeclaration.name,
      classDeclaration.typeParameters, classDeclaration.heritageClauses,
      classDeclaration.members.map(member => member === block ? updatedBlock : member));
    return factory.updateSourceFile(sourceFile, sourceFile.statements.map(statement => statement === classDeclaration ? updatedClass : statement));
  };
}
function observe(input) {
  const canonical = ts.createGetCanonicalFileName(true);
  const files = new Map([["/project/main.ts", input.text]]);
  const libraryRoot = path.join(root, "vendor/typescript-6.0.3/lib");
  const library = name => /^\/lib\/lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(name) && fs.existsSync(path.join(libraryRoot, path.basename(name)));
  const read = name => files.get(canonical(ts.normalizePath(name))) ?? (library(name) ? fs.readFileSync(path.join(libraryRoot, path.basename(name)), "utf8") : undefined);
  const overlay = createHermeticDirectoryOverlay(files.keys(), { currentDirectory: "/project", useCaseSensitiveFileNames: true,
    fallbackHost: { directoryExists: name => name === "/lib", getDirectories: () => [] } });
  const host = { ...ts.createCompilerHost(input.options, true), ...overlay,
    getCurrentDirectory: () => "/project", getDefaultLibFileName: options => "/lib/" + ts.getDefaultLibFileName(options),
    getDefaultLibLocation: () => "/lib", useCaseSensitiveFileNames: () => true, getCanonicalFileName: canonical,
    readFile: read, fileExists: name => files.has(canonical(ts.normalizePath(name))) || library(name),
    getSourceFile: (name, options) => { const text = read(name); return text === undefined ? undefined : ts.createSourceFile(name, text, options, true); },
    writeFile: () => assert.fail("unexpected host write") };
  const program = ts.createProgram({ rootNames: ["/project/main.ts"], options: input.options, host });
  const diagnostics = ts.getPreEmitDiagnostics(program).map(d => ({ code: d.code, message: ts.flattenDiagnosticMessageText(d.messageText, "\n") }));
  assert.deepEqual(diagnostics, [], input.case_id);
  const writes = [];
  const result = program.emit(undefined, (name, text) => writes.push({ path: name, text }), undefined, false, { before: [injector(input.shape)] });
  assert.equal(result.emitSkipped, false);
  const js = writes.find(w => w.path === "/project/out/main.js");
  assert.ok(js, "main.js written");
  return { emitted_paths: writes.map(w => w.path), js_text: js.text, js_sha256: sha256(js.text) };
}
const cases = [];
for (const [shape, statements, mutation] of shapes) for (const [targetName, target] of targets) for (const [modeName, define] of [["set", false], ["define", true]]) {
  const input = { case_id: `decorator-super-direct/${targetName}/${modeName}/${shape}`, shape, mutation,
    text: PRELUDE + cls(statements) + TAIL,
    options: { strict: true, target, module: ts.ModuleKind.Preserve, useDefineForClassFields: define, newLine: ts.NewLineKind.LineFeed,
      outDir: "/project/out", skipDefaultLibCheck: true, sourceMap: false, declaration: false, removeComments: false } };
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  cases.push({ ...input, typescript_observation: first });
}
assert.equal(cases.length, shapes.length * targets.length * 2);
const artifact = { version: 1, typescript: ts.version, route: "direct-transform-custom-before", repetitions: 2,
  source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases };
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered, { flag: "wx" });
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(JSON.stringify({ cases: cases.length, repetitions: 2, native_executions: 0, sha256: sha256(rendered) }));
