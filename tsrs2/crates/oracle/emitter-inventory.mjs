import crypto from "node:crypto";
import fs from "node:fs";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const bundlePath = process.argv[2];
const band = process.argv[3] ?? "all";
if (!bundlePath || !["all", "2xxx"].includes(band)) {
  throw new Error("usage: emitter-inventory.mjs <_tsc.js> <all|2xxx>");
}

const text = fs.readFileSync(bundlePath, "utf8");
const source = ts.createSourceFile(
  bundlePath,
  text,
  ts.ScriptTarget.Latest,
  true,
  ts.ScriptKind.JS,
);

function declaredName(node) {
  if (node.name && ts.isIdentifier(node.name)) return node.name.text;
  const parent = node.parent;
  if (parent && ts.isVariableDeclaration(parent) && ts.isIdentifier(parent.name)) {
    return parent.name.text;
  }
  if (parent && ts.isPropertyAssignment(parent)) return parent.name.getText(source);
  return null;
}

function calledName(expression) {
  if (ts.isIdentifier(expression)) return expression.text;
  if (ts.isPropertyAccessExpression(expression)) return expression.name.text;
  if (
    ts.isElementAccessExpression(expression) &&
    expression.argumentExpression &&
    ts.isStringLiteralLike(expression.argumentExpression)
  ) {
    return expression.argumentExpression.text;
  }
  return null;
}

function diagnosticAt(node) {
  if (!ts.isPropertyAccessExpression(node)) return null;
  if (!ts.isIdentifier(node.expression) || node.expression.text !== "Diagnostics") return null;
  const membershipRead =
    ts.isPropertyAccessExpression(node.parent) &&
    node.parent.expression === node &&
    node.parent.name.text === "code";
  const diagnostic = ts.Diagnostics[node.name.text];
  return {
    membershipRead,
    name: node.name.text,
    code: diagnostic?.code ?? null,
  };
}

function inBand(code) {
  return band === "all" || (code !== null && code >= 2000 && code < 3000);
}

const records = new Map();
let membershipReads = 0;

function recordFor(name) {
  let record = records.get(name);
  if (!record) {
    record = {
      id: name,
      name,
      declarationLines: new Set(),
      sites: [],
      calleeNames: new Set(),
    };
    records.set(name, record);
  }
  return record;
}

function discover(node, containingFunction = "<top>") {
  let currentFunction = containingFunction;
  if (ts.isFunctionLike(node)) {
    const name = declaredName(node);
    if (name !== null) {
      currentFunction = name;
      recordFor(currentFunction).declarationLines.add(
        source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1,
      );
    }
  }
  const record = recordFor(currentFunction);
  const diagnostic = diagnosticAt(node);
  if (diagnostic) {
    if (diagnostic.membershipRead) {
      membershipReads += 1;
    } else if (inBand(diagnostic.code)) {
      record.sites.push({
        line: source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1,
        name: diagnostic.name,
        code: diagnostic.code,
      });
    }
  }
  if (ts.isCallExpression(node)) {
    const callee = calledName(node.expression);
    if (callee) record.calleeNames.add(callee);
  }
  ts.forEachChild(node, (child) => discover(child, currentFunction));
}
discover(source);

// The ledger vocabulary is the tsc function name, so repeated bundle
// declarations with the same name form one inventory identity. The 2XXX
// inventory's raw owning-function/reference counts are intentionally
// separate from the checked-in audit's hand-classified 247/623 census.
// Anonymous callbacks are attributed to their nearest named owner rather
// than collapsed into one unauditable pseudo-function. A callback with no
// named owner belongs to the stable <top> identity.
// Property-call names deliberately over-approximate dynamic dispatch:
// a reviewer may disposition extra dependencies, but an obvious direct
// helper cannot silently disappear from the closure.
const directEmitters = [...records.values()].filter((record) => record.sites.length > 0);
const closure = new Map(directEmitters.map((record) => [record.name, record]));
const worklist = [...directEmitters];
while (worklist.length > 0) {
  const record = worklist.pop();
  for (const calleeName of record.calleeNames) {
    const callee = records.get(calleeName);
    if (callee && !closure.has(callee.name)) {
      closure.set(callee.name, callee);
      worklist.push(callee);
    }
  }
}

const functions = [...closure.values()]
  .sort((left, right) => left.name.localeCompare(right.name))
  .map((record) => ({
    id: record.id,
    name: record.name,
    declaration_lines: [...record.declarationLines].sort((left, right) => left - right),
    direct_emitter: record.sites.length > 0,
    sites: record.sites,
  }));

const output = {
  schema: 1,
  typescript_version: "6.0.3",
  source: "vendor/typescript-6.0.3/lib/_tsc.js",
  source_sha256: crypto.createHash("sha256").update(text).digest("hex"),
  band,
  closure_model: "transitive identifier/property-call name over-approximation over the tsc function-name ledger vocabulary",
  summary: {
    emitter_functions: directEmitters.length,
    diagnostic_references: directEmitters.reduce(
      (total, record) => total + record.sites.length,
      0,
    ),
    membership_reads: membershipReads,
    closure_functions: functions.length,
  },
  functions,
};

process.stdout.write(`${JSON.stringify(output, null, 2)}\n`);
