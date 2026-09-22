const fs=require('node:fs'),assert=require('node:assert/strict');
const ts=require('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/vendor/typescript-6.0.3/lib/typescript.js');
const f=ts.factory, id=()=>f.createIdentifier('value');
const constructors=[
 ['import',()=>f.createToken(ts.SyntaxKind.ImportKeyword)],
 ['non-null',()=>f.createNonNullExpression(id())],
 ['instantiation',()=>f.createExpressionWithTypeArguments(id(),[])],
 ['missing-declaration',()=>f.createMissingDeclaration()],
 ['arrow',()=>f.createArrowFunction(undefined,undefined,[],undefined,undefined,id())],
 ['omitted',()=>f.createOmittedExpression()],
 ['new-no-args',()=>f.createNewExpression(id(),undefined,undefined)],
 ['new-empty-args',()=>f.createNewExpression(id(),undefined,[])],
 ['optional',()=>f.createPropertyAccessChain(id(),f.createToken(ts.SyntaxKind.QuestionDotToken),'item')],
 ['identifier',id],
 ['ordinary-property',()=>f.createPropertyAccessExpression(id(),'item')],
 ['partial-import',()=>f.createPartiallyEmittedExpression(f.createToken(ts.SyntaxKind.ImportKeyword))],
 ['partial-arrow',()=>f.createPartiallyEmittedExpression(f.createArrowFunction(undefined,undefined,[],undefined,undefined,id()))],
];
function observe(){return constructors.map(([id,make])=>{const original=ts.setTextRange(make(),{pos:5,end:15});const result=f.createPropertyAccessExpression(original,'member').expression;return {id,input_kind:ts.SyntaxKind[original.kind],output_kind:ts.SyntaxKind[result.kind],wrapped:original!==result,range:[result.pos,result.end]};});}
const first=observe();assert.deepEqual(first,observe());const result={typescript:ts.version,repetitions:2,cases:first};fs.writeFileSync('/tmp/emitter-hosted-reference-repair-r578/factory-parenthesizer-oracle-r589.json',JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(result,null,2));
