const fs = require("fs");
const path = require("path");

function loadTypescript() {
  const root = path.resolve(__dirname, "..");
  const oracle = process.env.TSRS_ORACLE || path.join(root, "oracle");
  const candidates = [
    process.env.TSRS_TYPESCRIPT,
    path.join(oracle, "node_modules", "typescript"),
    "typescript",
  ].filter(Boolean);
  for (const candidate of candidates) {
    try { return require(candidate); } catch (_) {}
  }
  console.error("parse_oracle.js: missing TypeScript module; set TSRS_TYPESCRIPT or TSRS_ORACLE");
  process.exit(2);
}

const ts = loadTypescript();
const listFile = process.argv[2];
const paths = fs.readFileSync(listFile, "utf8").split("\n").map(s => s.trim()).filter(Boolean);
const out = [];
for (const p of paths) {
  let text;
  try { text = fs.readFileSync(p, "utf8"); } catch { out.push(p + "\u0001READERR"); continue; }
  const isTsx = p.endsWith(".tsx");
  const kind = isTsx ? ts.ScriptKind.TSX : ts.ScriptKind.TS;
  // createSourceFile parses and records syntactic (parse) diagnostics.
  const sf = ts.createSourceFile(p, text, ts.ScriptTarget.Latest, /*setParentNodes*/ false, kind);
  const diags = sf.parseDiagnostics || [];
  const codes = diags.map(d => d.code + ":" + d.start);
  out.push(p + "\u0001" + codes.join(","));
}
process.stdout.write(out.join("\n") + "\n");
