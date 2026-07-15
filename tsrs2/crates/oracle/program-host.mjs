import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const vendorLibDir = path.resolve(__dirname, "../../vendor/typescript-6.0.3/lib");

export const categoryNames = ["warning", "error", "suggestion", "message"];

export function decodeProgram(programJsonPath) {
  const program = JSON.parse(fs.readFileSync(programJsonPath, "utf8"));
  const cwd = normalizeFileName(path.posix.resolve(program.cwd ?? "/"));
  const files = new Map();
  const publicNames = new Map();

  for (const lib of program.libs ?? []) {
    const publicName = normalizeFileName(lib);
    const fileName = absoluteProgramFileName(publicName, cwd);
    const libPath = path.join(vendorLibDir, publicName);
    files.set(fileName, fs.readFileSync(libPath, "utf8"));
    publicNames.set(fileName, publicName);
  }

  for (const file of program.files ?? []) {
    const publicName = normalizeFileName(file.name);
    const fileName = absoluteProgramFileName(publicName, cwd);
    files.set(fileName, Buffer.from(file.textB64, "base64").toString("utf8"));
    publicNames.set(fileName, publicName);
  }

  return { program, cwd, files, publicNames };
}

export function normalizeFileName(fileName) {
  return fileName.replace(/\\/g, "/");
}

export function absoluteProgramFileName(fileName, cwd) {
  const normalized = normalizeFileName(fileName);
  return path.posix.isAbsolute(normalized) ? normalized : normalizeFileName(path.posix.resolve(cwd, normalized));
}

export function compilerOptionsFromProgram(program) {
  const declarations = new Map(
    (ts.optionDeclarations ?? []).map((option) => [option.name.toLowerCase(), option])
  );
  const options = {};

  for (const [rawName, rawValue] of Object.entries(program.options ?? {})) {
    const declaration = declarations.get(rawName.toLowerCase());
    const name = declaration?.name ?? rawName;
    if (declaration?.type instanceof Map && typeof rawValue === "string") {
      const value = declaration.type.get(rawValue.toLowerCase());
      if (value === undefined) {
        throw new Error(
          `Argument for '--${name}' option must be: ${Array.from(declaration.type.keys()).join(", ")}.`
        );
      }
      options[name] = value;
    } else {
      options[name] = rawValue;
    }
  }

  return {
    ...options,
    noLib: true,
  };
}

export function createHost(options, files, cwd) {
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

export function createProgramFromJsonPath(programJsonPath) {
  const { program: programJson, cwd, files, publicNames } = decodeProgram(programJsonPath);
  const options = compilerOptionsFromProgram(programJson);
  const rootNames = [
    ...(programJson.libs ?? []).map((fileName) => absoluteProgramFileName(fileName, cwd)),
    ...(programJson.files ?? []).map((file) => absoluteProgramFileName(file.name, cwd)),
  ];
  const host = createHost(options, files, cwd);
  const program = ts.createProgram(rootNames, options, host);
  return { program, programJson, cwd, files, publicNames };
}

export function publicFileName(file, publicNames) {
  if (!file) return null;
  return publicNames.get(normalizeFileName(file.fileName)) ?? normalizeFileName(file.fileName);
}
