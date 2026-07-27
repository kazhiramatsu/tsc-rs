import readline from "node:readline";
import { createRequire } from "node:module";
import {
  absoluteProgramFileName,
  decodeProgram,
} from "./program-host.mjs";

const instrumentedPath = process.argv[2];
if (!instrumentedPath) {
  throw new Error("usage: coverage-driver.mjs <instrumented-_tsc.cjs>");
}
const require = createRequire(import.meta.url);
const ts = require(instrumentedPath);

function compilerOptionsFromProgram(program) {
  const declarations = new Map(
    (ts.optionDeclarations ?? []).map((option) => [option.name.toLowerCase(), option]),
  );
  const options = {};
  for (const [rawName, rawValue] of Object.entries(program.options ?? {})) {
    const declaration = declarations.get(rawName.toLowerCase());
    const name = declaration?.name ?? rawName;
    if (declaration?.type instanceof Map && typeof rawValue === "string") {
      const value = declaration.type.get(rawValue.toLowerCase());
      if (value === undefined) {
        throw new Error(
          `Argument for '--${name}' option must be: ${Array.from(declaration.type.keys()).join(", ")}.`,
        );
      }
      options[name] = value;
    } else {
      options[name] = rawValue;
    }
  }
  return { ...options, noLib: true };
}

function createHost(options, files, cwd) {
  const languageVersion = options.target ?? ts.ScriptTarget.Latest;
  return {
    getSourceFile(fileName, languageVersionOrOptions = languageVersion) {
      const normalized = absoluteProgramFileName(fileName, cwd);
      const text = files.get(normalized);
      return text === undefined
        ? undefined
        : ts.createSourceFile(normalized, text, languageVersionOrOptions, true);
    },
    getDefaultLibFileName() {
      return "lib.d.ts";
    },
    getCurrentDirectory() {
      return cwd;
    },
    getDirectories() {
      return [];
    },
    getCanonicalFileName(fileName) {
      return absoluteProgramFileName(fileName, cwd);
    },
    useCaseSensitiveFileNames() {
      return true;
    },
    getNewLine() {
      return "\n";
    },
    fileExists(fileName) {
      return files.has(absoluteProgramFileName(fileName, cwd));
    },
    readFile(fileName) {
      return files.get(absoluteProgramFileName(fileName, cwd));
    },
    writeFile() {},
  };
}

function createProgramFromJsonPath(programJsonPath) {
  const { program: programJson, cwd, files } = decodeProgram(programJsonPath);
  const options = compilerOptionsFromProgram(programJson);
  const rootNames = [
    ...(programJson.libs ?? []).map((fileName) => absoluteProgramFileName(fileName, cwd)),
    ...(programJson.files ?? []).map((file) => absoluteProgramFileName(file.name, cwd)),
  ];
  const host = createHost(options, files, cwd);
  const program = ts.createProgram(rootNames, options, host);
  return { program, programJson, cwd };
}

function exercise(programJsonPath) {
  const { program, programJson, cwd } = createProgramFromJsonPath(programJsonPath);
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
