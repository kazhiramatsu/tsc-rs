import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
import fs from 'node:fs';import crypto from 'node:crypto';import assert from 'node:assert/strict';
const text=fs.readFileSync('vendor/typescript-6.0.3/lib/_tsc.js','utf8');
const source=ts.createSourceFile('_tsc.js',text,ts.ScriptTarget.ESNext,true,ts.ScriptKind.JS);
const sha=s=>crypto.createHash('sha256').update(s).digest('hex');const lines=text.split(/(?<=\n)/);
const names=['transformConstructor','transformConstructorBody','createConstructorDeclaration','setTextRange','getClassFacts','tryGetClassThis','transformAutoAccessor','transformPublicFieldInitializer','createAccessorPropertyBackingField','createAccessorPropertyGetRedirector','createAccessorPropertySetRedirector','createGetAccessorDeclaration','createSetAccessorDeclaration','createPropertyAccessExpression','createBasePropertyAccessExpression','createModifiersFromModifierFlags','createPropertyDeclaration','updatePropertyDeclaration','getCommentRange','getSourceMapRange','setOriginalNode','setEmitFlags','setSourceMapRange','setCommentRange','createBinaryExpression','createBlock','createExpressionStatement','createReturnStatement','propagateNameFlags','propagateIdentifierNameFlags','propagatePropertyNameFlagsOfChild','propagateChildFlags','propagateChildrenFlags','aggregateChildrenFlags','getTransformFlagsSubtreeExclusions','createBaseNode','createBaseDeclaration','createClassStaticBlockDeclaration','asNodeArray','asName','asInitializer','asToken','mergeEmitNode','mergeTokenSourceMapRanges','getOrCreateEmitNode','setSyntheticLeadingComments','setSyntheticTrailingComments'];
const owners=[],branches=[],calls=[];
function walk(n){
 if(ts.isFunctionDeclaration(n)&&names.includes(n.name?.text)){
  const start=source.getLineAndCharacterOfPosition(n.getStart(source)).line+1;
  if(n.name.text==='getClassFacts'&&start!==96844)return;
  if(['transformConstructor','transformConstructorBody'].includes(n.name.text)&&(start<97250||start>97431))return;
  if((start<21000||start>30000)&&!['transformConstructor','transformConstructorBody','getClassFacts','tryGetClassThis','transformAutoAccessor','transformPublicFieldInitializer'].includes(n.name.text))return;
  const end=source.getLineAndCharacterOfPosition(n.end-1).line+1;
  const owner=n.name.text;owners.push({owner,start,end,sha256:sha(lines.slice(start-1,end).join('')),body_sha256:sha(n.getText(source)),review:'whole body read; v2 review or current A39 factory/metadata review'});
  function scan(c){
   // Inline callback bodies belong to their enclosing whole-owner span.
   let e;if(ts.isIfStatement(c)||ts.isConditionalExpression(c))e=c.expression??c.condition;
   else if(ts.isBinaryExpression(c)&&[ts.SyntaxKind.AmpersandAmpersandToken,ts.SyntaxKind.BarBarToken,ts.SyntaxKind.QuestionQuestionToken].includes(c.operatorToken.kind))e=c;
   else if(ts.isCaseClause(c))e=c.expression;
   else if(ts.isDefaultClause(c))e=c;
   else if(ts.isForOfStatement(c)||ts.isForInStatement(c)||ts.isForStatement(c)||ts.isWhileStatement(c)||ts.isDoStatement(c))e=c.expression??c.condition??c;
   if(e)branches.push({owner,line:source.getLineAndCharacterOfPosition(c.getStart(source)).line+1,predicate:e.getText(source),sha256:sha(e.getText(source))});
   if(ts.isCallExpression(c))calls.push({owner,line:source.getLineAndCharacterOfPosition(c.getStart(source)).line+1,callee:c.expression.getText(source)});
   ts.forEachChild(c,scan);
  }scan(n);
 }
 ts.forEachChild(n,walk);
}walk(source);
assert.equal(owners.length,names.length);assert.equal(new Set(owners.map(o=>o.owner)).size,names.length);
const result={owners,branches,calls};
if(process.argv.includes('--check')) {
 const manifest=JSON.parse(fs.readFileSync('ratchets/h2-8a-retained-field-producers-readiness.v1.json','utf8'));
 for(const key of ['owners','branches','calls']) {
  const fields=Object.keys(result[key][0]);
  assert.deepEqual(manifest[key].map(row=>Object.fromEntries(fields.map(field=>[field,row[field]]))),result[key],key);
 }
 console.log(JSON.stringify({verified_whole_owners:owners.length,predicates:branches.length,calls:calls.length}));
} else console.log(JSON.stringify(result,null,2));
