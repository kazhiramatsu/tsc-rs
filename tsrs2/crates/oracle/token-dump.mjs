import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const fileName = process.argv[2];
const variantArg = process.argv[3] ?? "standard";

if (!fileName) {
  console.error("usage: node token-dump.mjs <file> [standard|jsx]");
  process.exit(2);
}

const text = fs.readFileSync(fileName, "utf8");
const variant = variantArg === "jsx" ? ts.LanguageVariant.JSX : ts.LanguageVariant.Standard;
const scanner = ts.createScanner(
  ts.ScriptTarget.ESNext,
  true,
  variant,
  text
);

let token;
while ((token = scanner.scan()) !== ts.SyntaxKind.EndOfFileToken) {
  console.log([
    token,
    scanner.getTokenStart(),
    scanner.getTokenEnd(),
    scanner.getTokenFlags() & 1,
  ].join("\t"));
}
