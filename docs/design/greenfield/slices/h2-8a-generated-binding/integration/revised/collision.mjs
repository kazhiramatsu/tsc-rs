import path from 'node:path';
import { pathToFileURL } from 'node:url';
const { default: ts } = await import(pathToFileURL(path.join(process.argv[2], 'vendor/typescript-6.0.3/lib/typescript.js')));
if (ts.version !== '6.0.3') throw Error('unexpected TypeScript version');
import assert from 'node:assert/strict';
const f=ts.factory;
const decl=name=>f.createVariableStatement(undefined,f.createVariableDeclarationList([f.createVariableDeclaration(name,undefined,undefined,f.createNumericLiteral(1))],ts.NodeFlags.Const));
const sf = ts.createSourceFile('main.ts', 'let _a = 1;\n_a;\n', ts.ScriptTarget.ESNext, true);
const node1 = decl(f.getGeneratedNameForNode(sf.statements[0]));
const node2 = decl(f.getGeneratedNameForNode(sf.statements[1]));
let fault=true;
const p=ts.createPrinter({newLine:ts.NewLineKind.CarriageReturnLineFeed},{isEmitNotificationEnabled:n=>n.kind===ts.SyntaxKind.VariableStatement,onEmitNode(h,n,cb){cb(h,n);if(n===node1&&fault){fault=false;throw Error('injected');}}});
let first;try{p.printNode(ts.EmitHint.Unspecified,node1,sf);assert.fail('fault did not fire');}catch(e){first={status:'threw',error:e.message};}
assert.equal(fault,false);
const second={status:'returned',text:p.printNode(ts.EmitHint.Unspecified,node2,sf)};
console.log(JSON.stringify({first,second}));
