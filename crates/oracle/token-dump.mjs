import fs from "node:fs";
import * as readline from "node:readline";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

function tokenDump(text, variantArg) {
  const variant = variantArg === "jsx" ? ts.LanguageVariant.JSX : ts.LanguageVariant.Standard;
  const scanner = ts.createScanner(
    ts.ScriptTarget.ESNext,
    true,
    variant,
    text
  );

  const lines = [];
  let token;
  while ((token = scanner.scan()) !== ts.SyntaxKind.EndOfFileToken) {
    lines.push([
      token,
      scanner.getTokenStart(),
      scanner.getTokenEnd(),
      scanner.getTokenFlags() & 1,
    ].join("\t"));
  }
  return lines.length === 0 ? "" : lines.join("\n") + "\n";
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
      const variant = payload.variant ?? "standard";
      process.stdout.write(JSON.stringify({
        id,
        ok: true,
        result: tokenDump(text, variant),
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
  const variantArg = process.argv[3] ?? "standard";

  if (!fileName) {
    console.error("usage: node token-dump.mjs <file> [standard|jsx]");
    console.error("   or: node token-dump.mjs --server-jsonl");
    process.exit(2);
  }

  const text = fs.readFileSync(fileName, "utf8");
  process.stdout.write(tokenDump(text, variantArg));
}
