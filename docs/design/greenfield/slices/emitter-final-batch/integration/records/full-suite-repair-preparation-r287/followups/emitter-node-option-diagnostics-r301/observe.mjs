import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/vendor/typescript-6.0.3/lib/typescript.js';
const input=JSON.parse(fs.readFileSync(new URL('inputs.json',import.meta.url),'utf8'));
const diag=d=>({code:d.code,category:ts.DiagnosticCategory[d.category],file:d.file?.fileName??null,start:d.start??null,length:d.length??null,text:ts.flattenDiagnosticMessageText(d.messageText,'\n'),related_information:d.relatedInformation?.map(diag)??null});
function observe(module){
 const options={noEmit:true,module,moduleResolution:1,ignoreDeprecations:'6.0'};
 const files=new Map([['lib.d.ts',input.lib],['main.ts',input.text]]);
 const host={...ts.createCompilerHost(options,true),getCurrentDirectory:()=>'/Display/Project',getDefaultLibFileName:()=> 'lib.d.ts',fileExists:n=>files.has(n),readFile:n=>files.get(n),getSourceFile:(n,o)=>{const s=files.get(n);return s===undefined?undefined:ts.createSourceFile(n,s,o,true);},writeFile:()=>assert.fail('unexpected write')};
 return ts.createProgram(['main.ts'],options,host).getOptionsDiagnostics().map(diag);
}
const rows=input.modules.map(module=>{const diagnostics=observe(module);assert.deepEqual(observe(module),diagnostics);return{module,diagnostics};});
const result={typescript:ts.version,repetitions:2,scope:'getOptionsDiagnostics programmatic control, not emit qualification',rows};
fs.writeFileSync(new URL('typescript.json',import.meta.url),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify(result));
