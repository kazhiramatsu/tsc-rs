import assert from "node:assert/strict";
import test from "node:test";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";
import {
  createHermeticDirectoryOverlay,
  createVirtualDirectoryIndex,
} from "./vfs-directory-overlay.mjs";

function automaticTypeDirectiveNames(fileNames, options, currentDirectory) {
  const normalizedFiles = new Set(
    fileNames.map((fileName) =>
      ts.getNormalizedAbsolutePath(fileName, currentDirectory),
    ),
  );
  const directoryOverlay = createHermeticDirectoryOverlay(normalizedFiles, {
    currentDirectory,
    useCaseSensitiveFileNames: true,
  });
  const host = {
    getCurrentDirectory: () => currentDirectory,
    fileExists: (fileName) =>
      normalizedFiles.has(
        ts.getNormalizedAbsolutePath(fileName, currentDirectory),
      ),
    readFile: () => undefined,
    directoryExists: directoryOverlay.directoryExists,
    getDirectories: directoryOverlay.getDirectories,
  };
  return ts.getAutomaticTypeDirectiveNames(options, host);
}

test("virtual directory index exposes immediate child basenames in JavaScript order", () => {
  const index = createVirtualDirectoryIndex(
    [
      "/types/B/index.d.ts",
      "/types/a/index.d.ts",
      "/types/\u{10000}/index.d.ts",
      "/types/\u{e000}/index.d.ts",
      "/types/direct.d.ts",
    ],
    "/",
  );

  assert.equal(index.has("/types/"), true);
  assert.deepEqual(index.getDirectories("/types"), [
    "B",
    "a",
    "\u{10000}",
    "\u{e000}",
  ]);
  assert.deepEqual(index.getDirectories("/types/a"), []);
  assert.equal(index.getDirectories("/missing"), undefined);
});

test("virtual directories shadow fallback enumeration even when they have no child directories", () => {
  const calls = [];
  const fallbackHost = {
    directoryExists(directory) {
      calls.push(`exists:${directory}`);
      return true;
    },
    getDirectories(directory) {
      calls.push(`directories:${directory}`);
      return ["physical-only"];
    },
  };
  const overlay = createHermeticDirectoryOverlay(
    ["/node_modules/@types/a/index.d.ts"],
    {
      currentDirectory: "/",
      fallbackHost,
    },
  );

  assert.equal(overlay.directoryExists("/node_modules/@types"), true);
  assert.deepEqual(overlay.getDirectories("/node_modules/@types"), ["a"]);
  assert.deepEqual(overlay.getDirectories("/node_modules/@types/a"), []);
  assert.deepEqual(calls, []);

  assert.deepEqual(overlay.getDirectories("/physical"), ["physical-only"]);
  assert.deepEqual(calls, ["directories:/physical"]);
});

test("wildcard automatic types see virtual packages at every effective ancestor root", () => {
  assert.deepEqual(
    automaticTypeDirectiveNames(
      [
        "/node_modules/@types/.a/index.d.ts",
        "/node_modules/@types/a/index.d.ts",
      ],
      { types: ["*"] },
      "/.src",
    ),
    ["a"],
  );

  assert.deepEqual(
    automaticTypeDirectiveNames(
      [
        "/node_modules/@types/dopey/index.d.ts",
        "/foo/node_modules/@types/grumpy/index.d.ts",
        "/foo/node_modules/@types/sneezy/index.d.ts",
      ],
      { types: ["*"], configFilePath: "/foo/bar/tsconfig.json" },
      "/src",
    ),
    ["grumpy", "sneezy", "dopey"],
  );

  assert.deepEqual(
    automaticTypeDirectiveNames(
      ["/node_modules/@types/jquery/index.d.ts"],
      { types: ["*"], configFilePath: "/tsconfig.json" },
      "/",
    ),
    ["jquery"],
  );
});
