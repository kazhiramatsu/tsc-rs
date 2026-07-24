// Phase 9.7a: exact recovery-tree declaration/binder census.
//
// For every fixture-owned source file in a harness program, serialize the
// function/constructor symbols inspected by checkFunctionOrConstructorSymbol.
// The Rust side mirrors this schema in xtask/recovery_census.rs.

import readline from "node:readline";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";
import {
  absoluteProgramFileName,
  createProgramFromJsonPath,
  normalizeFileName,
} from "./program-host.mjs";

function isCensusDeclaration(node) {
  return (
    ts.isClassDeclaration(node) ||
    ts.isClassExpression(node) ||
    ts.isFunctionDeclaration(node) ||
    ts.isMethodDeclaration(node) ||
    ts.isMethodSignature(node) ||
    ts.isConstructorDeclaration(node)
  );
}

function nameText(name) {
  if (!name) return null;
  if (
    ts.isIdentifier(name) ||
    ts.isPrivateIdentifier(name) ||
    ts.isStringLiteral(name) ||
    ts.isNumericLiteral(name)
  ) {
    return name.text;
  }
  return name.getText(name.getSourceFile());
}

function nodeRef(node) {
  if (!node) return null;
  return {
    kind: node.kind,
    pos: node.pos,
    end: node.end,
    missing: node.pos === node.end && node.kind !== ts.SyntaxKind.EndOfFileToken,
  };
}

function declarationRef(node) {
  const name = node.name;
  return {
    ...nodeRef(node),
    parentKind: node.parent?.kind ?? null,
    name: name
      ? {
          ...nodeRef(name),
          text: nameText(name),
        }
      : null,
    body: nodeRef(node.body),
  };
}

function symbolForDeclaration(checker, node) {
  return node.symbol ?? (node.name ? checker.getSymbolAtLocation(node.name) : undefined);
}

function fileDump(checker, sourceFile) {
  const declarations = [];
  function visit(node) {
    if (isCensusDeclaration(node)) {
      const symbol = symbolForDeclaration(checker, node);
      declarations.push({
        declaration: declarationRef(node),
        symbol: symbol
          ? {
              escapedName: String(symbol.escapedName),
              declarations: (symbol.declarations ?? []).map(declarationRef),
            }
          : null,
      });
    }
    ts.forEachChild(node, visit);
  }
  visit(sourceFile);

  return {
    name: normalizeFileName(sourceFile.fileName),
    parseDiagnostics: sourceFile.parseDiagnostics.map((diagnostic) => ({
      code: diagnostic.code,
      start: diagnostic.start ?? 0,
      length: diagnostic.length ?? 0,
    })),
    declarations,
  };
}

function recoveryDump(programJsonPath) {
  const { program, programJson, cwd } = createProgramFromJsonPath(programJsonPath);
  const checker = program.getTypeChecker();
  const files = [];
  for (const file of programJson.files ?? []) {
    const sourceFile = program.getSourceFile(absoluteProgramFileName(file.name, cwd));
    if (sourceFile) files.push(fileDump(checker, sourceFile));
  }
  return { files };
}

function runServerJsonl() {
  const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
  rl.on("line", (line) => {
    if (!line.trim()) return;
    let id = null;
    try {
      const request = JSON.parse(line);
      id = request.id === undefined ? null : request.id;
      process.stdout.write(
        JSON.stringify({
          id,
          ok: true,
          result: recoveryDump(request.programJsonPath),
        }) + "\n"
      );
    } catch (error) {
      process.stdout.write(
        JSON.stringify({
          id,
          ok: false,
          error: error && error.stack ? String(error.stack) : String(error),
        }) + "\n"
      );
    }
  });
}

if (process.argv[2] === "--server-jsonl") {
  runServerJsonl();
} else {
  const programJsonPath = process.argv[2];
  if (!programJsonPath) {
    console.error("usage: node recovery-dump.mjs <program.json>");
    console.error("   or: node recovery-dump.mjs --server-jsonl");
    process.exit(2);
  }
  process.stdout.write(JSON.stringify(recoveryDump(programJsonPath), null, 2) + "\n");
}
