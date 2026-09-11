import fs from 'node:fs';import ts from '../vendor/typescript-6.0.3/lib/typescript.js';import assert from 'node:assert/strict';
const read=p=>JSON.parse(fs.readFileSync(p,'utf8'));
const b=read('ratchets/h2-8a-retained-accessor-owners-before.v1.json');
const cases=[...read('crates/compiler/tests/fixtures/retained-accessor-owners.json').cases,...read('crates/compiler/tests/fixtures/class-helper-accessor-producers.json').cases,...read('crates/compiler/tests/fixtures/class-field-alias-map-positions.json').cases.filter(c=>c.options.target===9),...read('crates/compiler/tests/fixtures/decorator-receiver-context.json').cases];
const out=[];const static_=n=>n.modifiers?.some(m=>m.kind===ts.SyntaxKind.StaticKeyword);const accessor=n=>ts.isPropertyDeclaration(n)&&n.modifiers?.some(m=>m.kind===ts.SyntaxKind.AccessorKeyword);
for(const c of cases){const causes=new Set();for(const file of c.files){if(!file.path.endsWith('.ts')||file.path.endsWith('.d.ts'))continue;const s=ts.createSourceFile(file.path,file.text,ts.ScriptTarget.ESNext,true);
 function visit(n){if(ts.isDecorator(n))causes.add('A6-41:decorator-producer');
 if(ts.isClassLike(n)){
  if(ts.isClassExpression(n)&&!n.name&&n.members.some(m=>accessor(m)&&static_(m)))causes.add('A6-40:anonymous-constructor');
  if(!c.options.useDefineForClassFields&&n.members.some(m=>accessor(m)&&!static_(m))&&n.members.some(m=>ts.isPropertyDeclaration(m)&&!accessor(m)&&!static_(m)&&!ts.isPrivateIdentifier(m.name)&&m.initializer))causes.add('A6-40:moved-accessor-initializer');
 }
 if(accessor(n)&&ts.isComputedPropertyName(n.name)&&!ts.isSimpleInlineableExpression(n.name.expression))causes.add('A6-40:computed-cache');
 ts.forEachChild(n,visit);
 }visit(s);}
 out.push({case_id:c.case_id,causes:[...causes].sort(),status:b.exact_twice.includes(c.case_id)?'prior-exact':causes.size?'successor-negative':'required-repair'});
}
assert.equal(out.length,454);
if(process.argv.includes('--check')) {
 const m=read('ratchets/h2-8a-retained-field-producers-readiness.v1.json');
 assert.deepEqual(m.witnesses.map(({case_id,causes,status})=>({case_id,causes,status})),out);
 console.log(out.reduce((acc,r)=>(acc[r.status]=(acc[r.status]??0)+1,acc),{}));
} else console.log(JSON.stringify(out,null,2));
