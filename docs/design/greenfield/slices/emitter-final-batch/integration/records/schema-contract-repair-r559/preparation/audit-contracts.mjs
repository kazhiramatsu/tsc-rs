import fs from "node:fs";
import path from "node:path";
import { ARTIFACT_SCHEMA_CONTRACTS, validateJsonSchemaSubset } from "file:///Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/.github/ci/qualification.mjs";
const root = "/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep";
const result = [];
for (const c of ARTIFACT_SCHEMA_CONTRACTS) {
  try {
    validateJsonSchemaSubset(JSON.parse(fs.readFileSync(path.join(root,c.schema))), JSON.parse(fs.readFileSync(path.join(root,c.artifact))), c.label);
    result.push({...c, ok:true});
  } catch (e) { result.push({...c,ok:false,error:e.message}); }
}
fs.writeFileSync("/tmp/emitter-schema-cycle-repair-r553/contracts-before.json",JSON.stringify(result,null,2)+"\n",{flag:"wx"});
console.log(JSON.stringify({total:result.length,passed:result.filter(x=>x.ok).length,failures:result.filter(x=>!x.ok)},null,2));
