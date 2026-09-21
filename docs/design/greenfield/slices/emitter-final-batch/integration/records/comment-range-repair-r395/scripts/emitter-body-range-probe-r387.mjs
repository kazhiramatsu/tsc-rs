import ts from '/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/vendor/typescript-6.0.3/lib/typescript.js';
import assert from 'node:assert/strict';
const rows=[];
for(const semi of ['', ';']) for(const alwaysStrict of [false,true]) for(const removeComments of [false,true]) {
 const source=`const Home = {};\nexport default Home${semi}\n// trailing export comment\n`;
 const options={target:ts.ScriptTarget.ES2015,module:ts.ModuleKind.System,alwaysStrict,removeComments};
 const one=ts.transpileModule(source,{fileName:'module.ts',compilerOptions:options,reportDiagnostics:true});
 const two=ts.transpileModule(source,{fileName:'module.ts',compilerOptions:options,reportDiagnostics:true});
 assert.deepEqual(one,two);rows.push({semi,alwaysStrict,removeComments,source,output:one.outputText,diagnostics:one.diagnostics?.map(d=>d.code)});
}
console.log(JSON.stringify({scope:'Pinned TypeScript transpileModule twice; JS diagnostic probe only, not a complete-command proof.',rows},null,2));
