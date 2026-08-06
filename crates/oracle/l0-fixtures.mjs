import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const workspace = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const manifestPath = path.join(workspace, "ratchets/l0-fixtures.v1.json");

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function utf16Length(value) {
  return value.length;
}

function file(name, text) {
  return { name, text };
}

function explicitRoot() {
  return {
    id: "explicit-root",
    purpose: "embedded-library startup and binary-layout control",
    args: ["--noEmit", "--ignoreConfig", "main.ts"],
    files: [
      file(
        "main.ts",
        "export const values: ReadonlyArray<number> = [1, 2, 3];\nexport const total = values.reduce((sum, value) => sum + value, 0);\n",
      ),
    ],
  };
}

function project() {
  return {
    id: "project",
    purpose: "config discovery plus local, package, and type resolution",
    args: ["--noEmit", "--project", "tsconfig.json"],
    files: [
      file(
        "tsconfig.json",
        `${JSON.stringify(
          {
            compilerOptions: {
              noEmit: true,
              strict: true,
              target: "es2022",
              module: "commonjs",
              moduleResolution: "node10",
              ignoreDeprecations: "6.0",
              types: ["fixture-globals"],
            },
            files: ["src/main.ts", "src/util.ts"],
          },
          null,
          2,
        )}\n`,
      ),
      file(
        "src/main.ts",
        'import { answer } from "fixture-package";\nimport { twice } from "./util";\nexport const result: number = twice(answer + fixtureBias);\n',
      ),
      file("src/util.ts", "export function twice(value: number): number { return value * 2; }\n"),
      file(
        "node_modules/fixture-package/package.json",
        '{"name":"fixture-package","version":"1.0.0","types":"index.d.ts"}\n',
      ),
      file("node_modules/fixture-package/index.d.ts", "export declare const answer: number;\n"),
      file(
        "node_modules/@types/fixture-globals/package.json",
        '{"name":"@types/fixture-globals","version":"1.0.0","types":"index.d.ts"}\n',
      ),
      file("node_modules/@types/fixture-globals/index.d.ts", "declare const fixtureBias: number;\n"),
    ],
  };
}

function scale() {
  const sourceCount = 32;
  const declarationsPerSource = 96;
  const names = Array.from({ length: sourceCount }, (_, index) => `src/file-${index}.ts`);
  const files = [
    file(
      "tsconfig.json",
      `${JSON.stringify(
        {
          compilerOptions: {
            noEmit: true,
            strict: true,
            target: "es2022",
            module: "commonjs",
            moduleResolution: "node10",
            ignoreDeprecations: "6.0",
          },
          files: names,
        },
        null,
        2,
      )}\n`,
    ),
  ];
  for (let source = 0; source < sourceCount; source += 1) {
    let text = "";
    for (let declaration = 0; declaration < declarationsPerSource; declaration += 1) {
      const stem = `Item_${source}_${declaration}`;
      text += `export interface ${stem} { id: number; label: string; next?: ${stem}; }\n`;
      text += `export const value_${source}_${declaration}: ${stem} = { id: ${declaration}, label: "${stem}" };\n`;
    }
    files.push(file(names[source], text));
  }
  return {
    id: "scale",
    purpose: "multi-file per-node allocation and hot-loop tax control",
    args: ["--noEmit", "--project", "tsconfig.json"],
    generator: { source_count: sourceCount, declarations_per_source: declarationsPerSource },
    files,
  };
}

function largeEdit() {
  const declarations = 13_000;
  const editDeclaration = 6_137;
  let before = "// L1 large-file edit fixture; UTF-16境界😀; generated deterministically.\n";
  for (let index = 0; index < declarations; index += 1) {
    before += `interface LargeItem${index} { index: ${index}; label: "row-${index}"; next?: LargeItem${index}; }\n`;
  }
  before += "export const sentinel = \"終端😀\";\n";
  const oldText = `label: "row-${editDeclaration}"`;
  const newText = `label: "編集-${editDeclaration}-😀"`;
  const startByte = Buffer.byteLength(before.slice(0, before.indexOf(oldText)), "utf8");
  const startUtf16 = utf16Length(before.slice(0, before.indexOf(oldText)));
  const after = before.replace(oldText, newText);
  if (after === before || after.indexOf(oldText) >= 0) throw new Error("large edit was not unique");
  return {
    id: "large-edit",
    purpose: "L1 ordinary-parser copy-on-reuse latency and Unicode coordinate proof",
    generator: { declarations, edit_declaration: editDeclaration },
    files: [file("large-edit.ts", before)],
    edit: {
      file: "large-edit.ts",
      start_byte: startByte,
      delete_bytes: Buffer.byteLength(oldText, "utf8"),
      insert_bytes: Buffer.byteLength(newText, "utf8"),
      start_utf16: startUtf16,
      delete_utf16: utf16Length(oldText),
      insert_utf16: utf16Length(newText),
      old_text: oldText,
      new_text: newText,
      after_sha256: sha256(after),
      after_bytes: Buffer.byteLength(after, "utf8"),
      after_utf16: utf16Length(after),
    },
  };
}

function materializedWorkload(workload) {
  const files = workload.files.map(({ name, text }) => ({
    path: name,
    bytes: Buffer.byteLength(text, "utf8"),
    utf16: utf16Length(text),
    sha256: sha256(text),
  }));
  const totalBytes = files.reduce((total, entry) => total + entry.bytes, 0);
  const canonical = JSON.stringify({
    id: workload.id,
    args: workload.args ?? null,
    generator: workload.generator ?? null,
    files,
    edit: workload.edit ?? null,
  });
  return {
    id: workload.id,
    purpose: workload.purpose,
    args: workload.args ?? null,
    generator: workload.generator ?? null,
    files,
    total_bytes: totalBytes,
    workload_sha256: sha256(canonical),
    edit: workload.edit ?? null,
  };
}

function generated() {
  const workloads = [explicitRoot(), project(), scale(), largeEdit()];
  return {
    manifest: {
      schema: 1,
      status: "frozen",
      generator: "crates/oracle/l0-fixtures.mjs",
      node_version: "25.2.1",
      workloads: workloads.map(materializedWorkload),
    },
    workloads,
  };
}

function renderManifest(manifest) {
  return `${JSON.stringify(manifest, null, 2)}\n`;
}

function writeTree(root, workloads) {
  fs.rmSync(root, { force: true, recursive: true });
  fs.mkdirSync(root, { recursive: true });
  for (const workload of workloads) {
    const workloadRoot = path.join(root, workload.id);
    for (const entry of workload.files) {
      const target = path.join(workloadRoot, entry.name);
      fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.writeFileSync(target, entry.text);
    }
  }
}

const { manifest, workloads } = generated();
const mode = process.argv[2];
if (mode === "--write") {
  fs.writeFileSync(manifestPath, renderManifest(manifest));
  process.stdout.write(`wrote ${path.relative(workspace, manifestPath)}\n`);
} else if (mode === "--check") {
  if (!fs.existsSync(manifestPath) || fs.readFileSync(manifestPath, "utf8") !== renderManifest(manifest)) {
    throw new Error("stale ratchets/l0-fixtures.v1.json; run with --write and review");
  }
  const large = manifest.workloads.find((workload) => workload.id === "large-edit");
  if (!large || large.total_bytes < 1_000_000 || large.edit.start_byte === large.edit.start_utf16) {
    throw new Error("large-edit fixture must exceed 1 MB and exercise distinct byte/UTF-16 coordinates");
  }
  process.stdout.write("L0 workload and large-edit fixtures are fresh\n");
} else if (mode === "--materialize") {
  const output = process.argv[3];
  if (!output) throw new Error("--materialize requires an output directory");
  writeTree(path.resolve(output), workloads);
  process.stdout.write(`${path.resolve(output)}\n`);
} else if (mode === "--smoke") {
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "tsc-rs-l0-fixtures-"));
  try {
    writeTree(temporary, workloads);
  } finally {
    fs.rmSync(temporary, { force: true, recursive: true });
  }
  process.stdout.write("L0 fixture materialization smoke ok\n");
} else {
  throw new Error("usage: l0-fixtures.mjs --write|--check|--materialize <dir>|--smoke");
}
