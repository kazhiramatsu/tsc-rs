// Config file-path option values before host access or output planning.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const units = text => Array.from({length: text.length}, (_, i) => text.charCodeAt(i));
const values = ["", ".", "./\uD800", "./\uD801", "./�", "./😀", "${configDir}/\uD800", "${CONFIGDIR}/\uD800", "${confıgDir}/\uD800", "${configDir}/../\uDC00", "${configDir}/\uD800/\uDC00/", "${configDir}/./😀", "./\uD800/../\uDC00", "\uD800${configDir}", "${configDir}\uD800", "${configDi\uD800}/name", "./a\0b"];
const bases = ["/base", "C:/base", "/base/😀"];
const host = {useCaseSensitiveFileNames:true, readDirectory(){return []}, fileExists(){return false}, readFile(){}};
const cases=[];
for(const [vi,value] of values.entries()) for(const [bi,base] of bases.entries()) {
  const observe=()=> {
    const json = {files:[], compilerOptions:{outDir:value}};
    const parsed = ts.parseJsonConfigFileContent(json, host, base, {}, base + "/tsconfig.json");
    assert.equal(typeof parsed.options.outDir, "string");
    return units(parsed.options.outDir);
  };
  const expected=observe(); assert.deepEqual(observe(),expected);
  cases.push({case_id:`config-value-${vi}-base-${bi}`, value_utf16:units(value), base, out_dir_utf16:expected});
}
const artifact={version:1, scope:"parseJsonConfigFileContent outDir value projection only; no host/output-plan qualification", typescript:ts.version,
 compiler_sha256:compilerSha, observer_sha256:sha(fs.readFileSync(import.meta.filename)), repetitions:2, executions:cases.length*2,cases};
const output=path.join(root,"crates/program/tests/fixtures/utf16-config-path-values.json");
assert.ok(["--write","--check"].includes(process.argv[2]));
if(process.argv[2]==="--write") fs.writeFileSync(output,JSON.stringify(artifact,null,2)+"\n",{flag:"wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output)),artifact);
console.log(JSON.stringify({output,sha256:sha(fs.readFileSync(output)),cases:cases.length,executions:cases.length*2}));
