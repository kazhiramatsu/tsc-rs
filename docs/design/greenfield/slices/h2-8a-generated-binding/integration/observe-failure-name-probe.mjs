import path from 'node:path';
import { pathToFileURL } from 'node:url';
const { default: ts } = await import(pathToFileURL(path.join(process.argv[2], 'vendor/typescript-6.0.3/lib/typescript.js')));
if (ts.version !== '6.0.3') throw Error('unexpected TypeScript version');
import assert from 'node:assert/strict';
const f=ts.factory;
const decl=name=>f.createVariableStatement(undefined,f.createVariableDeclarationList([f.createVariableDeclaration(name,undefined,undefined,f.createNumericLiteral(1))],ts.NodeFlags.Const));
for (const secondText of ["let x = 1;\nx;\n", "let y = 1;\ny;\n"]) {
 const sf1=ts.createSourceFile('main.ts',"let x = 1;\nx;\n",ts.ScriptTarget.ESNext,true);
 const sf2=ts.createSourceFile('main.ts',secondText,ts.ScriptTarget.ESNext,true);
 const u1=decl(f.getGeneratedNameForNode(sf1.statements[0].declarationList.declarations[0].name));
 const u2=decl(f.getGeneratedNameForNode(sf2.statements[0].declarationList.declarations[0].name));
 let fault=true;
 const opts={newLine:ts.NewLineKind.CarriageReturnLineFeed};
 const p=ts.createPrinter(opts,{isEmitNotificationEnabled:n=>n.kind===ts.SyntaxKind.VariableStatement,onEmitNode(h,n,cb){cb(h,n);if(n===u1&&fault){fault=false;throw Error('injected');}}});
 let first;try{p.printNode(ts.EmitHint.Unspecified,u1,sf1);assert.fail('fault did not fire');}catch(e){first={status:'threw',error:e.message};}
 assert.equal(fault,false);
 const second={status:'returned',text:p.printNode(ts.EmitHint.Unspecified,u2,sf2)};
 const fresh={status:'returned',text:ts.createPrinter(opts).printNode(ts.EmitHint.Unspecified,u2,sf2)};
 console.log(JSON.stringify({second_source:secondText,first,second,fresh}));
}

for (const [kind, base] of [['numbered','x'], ['scoped','_s'], ['file-wide','_o'], ['file-level','_m'], ['temp',null]]) {
 const sf=ts.createSourceFile('main.ts',"let x = 1;\nx;\n",ts.ScriptTarget.ESNext,true);
 const flags=kind==='scoped'?ts.GeneratedIdentifierFlags.Optimistic|ts.GeneratedIdentifierFlags.ReservedInNestedScopes:kind==='file-level'?ts.GeneratedIdentifierFlags.Optimistic|ts.GeneratedIdentifierFlags.FileLevel:kind==='file-wide'?ts.GeneratedIdentifierFlags.Optimistic:0;
 const name=kind==='temp'?f.getGeneratedNameForNode(sf.statements[0]):f.createUniqueName(base,flags);
 const node=decl(name); let fault=true;
 const opts={newLine:ts.NewLineKind.CarriageReturnLineFeed};
 const p=ts.createPrinter(opts,{isEmitNotificationEnabled:n=>n.kind===ts.SyntaxKind.VariableStatement,onEmitNode(h,n,cb){cb(h,n);if(n===node&&fault){fault=false;throw Error('injected');}}});
 let first;try{p.printNode(ts.EmitHint.Unspecified,node,sf);assert.fail('fault did not fire');}catch(e){first={status:'threw',error:e.message};}
 assert.equal(fault,false);
 const second={status:'returned',text:p.printNode(ts.EmitHint.Unspecified,node,sf)};
 const fresh={status:'returned',text:ts.createPrinter(opts).printNode(ts.EmitHint.Unspecified,node,sf)};
 console.log(JSON.stringify({same_binding:kind,first,second,fresh}));
}
