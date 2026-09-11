import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import assert from "node:assert/strict";
import fs from "node:fs";
import crypto from "node:crypto";
assert.equal(ts.version,"6.0.3");
assert.ok(["--write","--check"].includes(process.argv[2]));
const hash=bytes=>crypto.createHash("sha256").update(bytes).digest("hex");
const shapes = [
  ["named", 'import { value as local } from "./lib";\nexport { local };\n'],
  ["named-conflict", 'import { value as local } from "./lib";\nconst local = 2;\nexport { local };\n'],
  ["quoted-conflict", 'import { "value" as local } from "./lib";\nconst local = 2;\nexport { local };\n'],
  ["default", 'import local from "./lib";\nexport { local };\n'],
  ["default-conflict", 'import local from "./lib";\nconst local = 2;\nexport { local };\n'],
  ["namespace", 'import * as local from "./lib";\nexport { local };\n'],
  ["namespace-conflict", 'import * as local from "./lib";\nconst local = 2;\nexport { local };\n'],
  ["equals", 'import local = require("./lib");\nexport { local };\n'],
  ["exported-equals", 'export import local = require("./lib");\nexport { local as alias };\n'],
];
const cases=[];
for (const ext of ["js","ts"]) for (const [shape,text] of shapes) {
  const main = `/project/main.${ext}`;
  const files = new Map([[main,text],[`/project/lib.${ext}`, 'export const value = 1; export default value;\n']]);
  const options = {target:ts.ScriptTarget.ES2015,module:ts.ModuleKind.CommonJS,allowJs:true,checkJs:true,strict:true,noLib:true};
  const host = {...ts.createCompilerHost(options,true), getCurrentDirectory:()=>"/project",getCanonicalFileName:n=>n,
    fileExists:n=>files.has(n),readFile:n=>files.get(n),directoryExists:n=>n==="/project",getDirectories:()=>[],
    getSourceFile:(n,v)=>files.has(n)?ts.createSourceFile(n,files.get(n),v,true):undefined};
  function observe() {
  const program=ts.createProgram([main],options,host);
  ts.getPreEmitDiagnostics(program);
  const checker=program.getTypeChecker(),resolver=checker.getEmitResolver(),file=program.getSourceFile(main);
  const imports=[];
  function visit(n) {
    if (ts.isImportSpecifier(n)||ts.isNamespaceImport(n)||ts.isImportEqualsDeclaration(n)||(ts.isImportClause(n)&&n.name)) imports.push(n);
    ts.forEachChild(n,visit);
  }
  visit(file);
  const observations=[];
  for (const decl of imports) {
    const name=ts.factory.getDeclarationName(decl),symbol=checker.getSymbolAtLocation(decl.name);
    const exported=resolver.getReferencedExportContainer(name,false),imported=resolver.getReferencedImportDeclaration(name);
    observations.push({kind:ts.SyntaxKind[decl.kind],name:name.text,symbol_flags:symbol?.flags??null,
      export_container:exported?ts.SyntaxKind[exported.kind]:null,import_declaration:imported?ts.SyntaxKind[imported.kind]:null,
      declaration_emit_flags:ts.getEmitFlags(name)});
  }
  return observations;
  }
  const observations=observe();assert.deepEqual(observe(),observations);
  cases.push({case_id:`${ext}/${shape}`,files:[...files].map(([path,text])=>({path,text})),roots:[main],options,observations});
}
const artifact={version:1,typescript:ts.version,source_commit:"050880ce59e30b356b686bd3144efe24f875ebc8",repetitions:2,
 compiler_sha256:hash(fs.readFileSync(new URL("../vendor/typescript-6.0.3/lib/typescript.js",import.meta.url))),
 observer_sha256:hash(fs.readFileSync(import.meta.filename)),cases};
const destination=new URL("../crates/checker/tests/fixtures/import-publication-resolver.json",import.meta.url);
const bytes=JSON.stringify(artifact,null,2)+"\n";
if (process.argv[2]==="--write") fs.writeFileSync(destination,bytes);
else assert.equal(fs.readFileSync(destination,"utf8"),bytes);
console.log(`Import publication resolver: ${cases.length} query cases, each identical twice`);
