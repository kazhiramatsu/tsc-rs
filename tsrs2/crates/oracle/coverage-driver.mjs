import readline from "node:readline";
import { createRequire } from "node:module";
import { createProgramFromJsonPathWithTs } from "./program-host.mjs";

const instrumentedPath = process.argv[2];
if (!instrumentedPath) {
  throw new Error("usage: coverage-driver.mjs <instrumented-_tsc.cjs>");
}
const require = createRequire(import.meta.url);
const ts = require(instrumentedPath);

function exercise(programJsonPath) {
  const { program, programJson, cwd } = createProgramFromJsonPathWithTs(ts, programJsonPath);
  for (const file of programJson.files ?? []) {
    const fileName = file.name.replace(/\\/g, "/");
    const absolute = fileName.startsWith("/") ? fileName : `${cwd}/${fileName}`.replace(/\/+/g, "/");
    const sourceFile = program.getSourceFile(absolute);
    if (!sourceFile) continue;
    program.getSyntacticDiagnostics(sourceFile);
    program.getSemanticDiagnostics(sourceFile);
    program.getSuggestionDiagnostics(sourceFile);
  }
}

const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of rl) {
  if (!line.trim()) continue;
  const request = JSON.parse(line);
  if (request.finish) {
    process.stdout.write(
      `${JSON.stringify({ schema: 1, counts: ts.__tsrsM8Counts })}\n`,
    );
    break;
  }
  exercise(request.programJsonPath);
}
