import ts from '/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/vendor/typescript-6.0.3/lib/typescript.js';
import fs from 'node:fs';
const source='// https://github.com/microsoft/TypeScript/issues/59373\n\nif (true)\nexport const cssExports: CssExports;\nexport default cssExports;\n';
const observations=[];
for(const alwaysStrict of [false,true]){
 const options={target:ts.ScriptTarget.ES2015,module:ts.ModuleKind.System,useDefineForClassFields:true,alwaysStrict};
 const one=ts.transpileModule(source,{compilerOptions:options,fileName:'system-export-in-if.ts',reportDiagnostics:true});
 const two=ts.transpileModule(source,{compilerOptions:options,fileName:'system-export-in-if.ts',reportDiagnostics:true});
 if(one.outputText!==two.outputText)throw Error('nondeterministic');
 observations.push({alwaysStrict,outputText:one.outputText,diagnosticCodes:one.diagnostics?.map(d=>d.code)});
}
console.log(JSON.stringify({scope:'Official pinned TS transpileModule JS probe twice; no whole compiler/checker/map/admission claim.',observations},null,2));
