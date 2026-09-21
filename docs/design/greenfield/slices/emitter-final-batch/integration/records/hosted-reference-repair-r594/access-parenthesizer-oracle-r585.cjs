const fs = require('node:fs');
const crypto = require('node:crypto');
const tsPath = '/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/vendor/typescript-6.0.3/lib/typescript.js';
const ts = require(tsPath);
const sources = [
  ['import-types', 'import<T>\nconst a = import<string, number>\n'],
  ['import-comments', 'import /*before*/<T> /*after*/;\nconst a = import /*before*/<string, number> /*after*/;\n'],
  ['import-parens', '(import<T>);\nconst a = (import<string, number>);\n'],
  ['new-arguments', 'const a = (new C)<T>;\nconst b = new C<T>();\n'],
  ['optional-instantiation', 'const a = value?.method<T>;\nconst b = value<T>?.();\n'],
  ['nested-instantiation', 'const a = value<T><U>;\nconst b = value!<T>;\n'],
];
const transforms=[];
for (const target of [ts.ScriptTarget.ES2015,ts.ScriptTarget.ESNext]) {
  for(const [id,input] of sources){
    const options={target,module:ts.ModuleKind.ESNext,alwaysStrict:false,newLine:ts.NewLineKind.LineFeed};
    const observe=()=>ts.transpileModule(input,{compilerOptions:options,fileName:'case.ts',reportDiagnostics:true});
    const projection=r=>({output:r.outputText,diagnostics:r.diagnostics?.map(d=>({code:d.code,start:d.start,length:d.length,category:d.category,message:ts.flattenDiagnosticMessageText(d.messageText,'\\n')}))}); const a=observe(),b=observe();if(JSON.stringify(projection(a))!==JSON.stringify(projection(b)))throw new Error(id);
    transforms.push({id,target:ts.ScriptTarget[target],input,output:a.outputText,diagnostics:a.diagnostics?.map(d=>({code:d.code,start:d.start,length:d.length}))});
  }
}
const factory=[];
for(const expr of ['import','value!','value<T>','new C','new C()','value?.x','x => x','/x/','value','x + y','(x, y)','obj.fn<T>']){
 const source=ts.createSourceFile('case.ts','const a = '+expr+';',ts.ScriptTarget.Latest,true);
 const node=source.statements[0].declarationList.declarations[0].initializer;
 const result=ts.factory.createPropertyAccessExpression(node,'member').expression;
 factory.push({expr,inputKind:ts.SyntaxKind[node.kind],outputKind:ts.SyntaxKind[result.kind],identity:node===result,inputRange:[node.pos,node.end],outputRange:[result.pos,result.end]});
}
const result={typescript:ts.version,compiler_sha256:crypto.createHash('sha256').update(fs.readFileSync(tsPath)).digest('hex'),repetitions:2,transforms,factory};
fs.writeFileSync('/tmp/emitter-hosted-reference-repair-r578/access-parenthesizer-oracle-r585.json',JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify(result,null,2));
