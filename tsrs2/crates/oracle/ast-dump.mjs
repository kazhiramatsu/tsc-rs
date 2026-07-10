import fs from "node:fs";
import * as readline from "node:readline";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

function scriptKindForFileName(fileName) {
  return fileName.endsWith(".tsx") || fileName.endsWith(".jsx")
    ? ts.ScriptKind.TSX
    : ts.ScriptKind.TS;
}

function astDump(fileName, text) {
  const sourceFile = ts.createSourceFile(
    fileName,
    text,
    ts.ScriptTarget.ESNext,
    /*setParentNodes*/ false,
    scriptKindForFileName(fileName)
  );

  const lines = [];
  function dump(node, depth) {
    lines.push(`${"  ".repeat(depth)}${node.kind} ${node.pos} ${node.end}`);
    ts.forEachChild(node, (child) => dump(child, depth + 1));
  }
  dump(sourceFile, 0);

  return {
    dump: lines.length === 0 ? "" : lines.join("\n") + "\n",
    parseErrors: sourceFile.parseDiagnostics.length,
  };
}

function runServerJsonl() {
  const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
  rl.on("line", (line) => {
    if (!line.trim()) return;
    let id = null;
    try {
      const request = JSON.parse(line);
      id = request.id === undefined ? null : request.id;
      const payload = request.payload || request;
      const text = payload.textBase64 === undefined
        ? (payload.text ?? "")
        : Buffer.from(payload.textBase64, "base64").toString("utf8");
      const fileName = payload.fileName ?? "a.ts";
      process.stdout.write(JSON.stringify({
        id,
        ok: true,
        result: astDump(fileName, text),
      }) + "\n");
    } catch (error) {
      process.stdout.write(JSON.stringify({
        id,
        ok: false,
        error: error && error.stack ? String(error.stack) : String(error),
      }) + "\n");
    }
  });
}

if (process.argv[2] === "--server-jsonl") {
  runServerJsonl();
} else {
  const fileName = process.argv[2];

  if (!fileName) {
    console.error("usage: node ast-dump.mjs <file>");
    console.error("   or: node ast-dump.mjs --server-jsonl");
    process.exit(2);
  }

  const text = fs.readFileSync(fileName, "utf8");
  const { dump, parseErrors } = astDump(fileName, text);
  process.stdout.write(dump);
  if (parseErrors > 0) {
    console.error(`parse errors: ${parseErrors}`);
  }
}
