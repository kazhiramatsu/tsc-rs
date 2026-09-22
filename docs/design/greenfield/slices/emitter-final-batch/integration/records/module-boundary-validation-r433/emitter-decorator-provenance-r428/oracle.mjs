import fs from 'node:fs';
import ts from "/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/vendor/typescript-6.0.3/lib/typescript.js";
const text=fs.readFileSync(new URL('./input.ts',import.meta.url),'utf8');
const s=ts.createSourceFile('decoratorOnUsing.ts',text,ts.ScriptTarget.ESNext,true);const rows=[];
function walk(n,parent=null){rows.push({kind:n.kind,pos:n.pos,end:n.end,parent:parent?.kind??-1,name:ts.SyntaxKind[n.kind],parentName:parent?ts.SyntaxKind[parent.kind]:null});ts.forEachChild(n,c=>{walk(c,n);});}walk(s);fs.writeFileSync(new URL('./oracle.json',import.meta.url),JSON.stringify(rows,null,2)+'\n');
