#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const checker = path.join(here, "slice-readiness.mjs");

for (const argument of ["--schema-check", "--test"]) {
    const result = spawnSync(process.execPath, [checker, argument], {
        cwd: path.resolve(here, "..", ".."),
        encoding: "utf8",
    });
    if (result.status !== 0) {
        process.stderr.write(result.stderr || result.stdout || `${argument} failed\n`);
        process.exit(result.status ?? 1);
    }
}

console.log("slice readiness: focused tests passed");
