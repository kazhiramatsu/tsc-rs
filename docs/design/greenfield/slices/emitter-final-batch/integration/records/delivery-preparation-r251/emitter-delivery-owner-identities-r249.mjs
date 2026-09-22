import fs from 'node:fs';
import crypto from 'node:crypto';
import {pathToFileURL} from 'node:url';
const root='/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep';
const ts=(await import(pathToFileURL(`${root}/vendor/typescript-6.0.3/lib/typescript.js`))).default;
const plan=JSON.parse(fs.readFileSync('/tmp/emitter-delivery-owners-candidates-r247.json','utf8'));
const selected=JSON.parse(fs.readFileSync('/tmp/emitter-delivery-owner-selections-r249.json','utf8'));
const names=new Set([...Object.values(plan.rows).flat(),...Object.values(selected.rows).flat().map(x=>x.name),selected.adjacent_semantic_correction.name]);
const sourcePath='vendor/typescript-6.0.3/lib/_tsc.js';
const source=fs.readFileSync(`${root}/${sourcePath}`,'utf8');
const sha=x=>crypto.createHash('sha256').update(x).digest('hex');
const sf=ts.createSourceFile(sourcePath,source,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);
const pos=n=>{const p=sf.getLineAndCharacterOfPosition(n);return {offset:n,line:p.line+1,character:p.character+1}};
const found={};for(const name of names)found[name]=[];
function visit(node,parents){
 let scopes=parents;
 if(ts.isFunctionLike(node)&&node.body){
  let name=node.name?.getText(sf)??'<anonymous>';
  const parent=node.parent;
  if(parent&&ts.isVariableDeclaration(parent)&&ts.isIdentifier(parent.name))name=parent.name.text;
  else if(parent&&ts.isPropertyAssignment(parent)&&(ts.isIdentifier(parent.name)||ts.isStringLiteralLike(parent.name)||ts.isNumericLiteral(parent.name)))name=parent.name.text;
  const start=node.getStart(sf),location=pos(start);
  const identity=`${name}@${location.line}:${location.character}`;
  if(names.has(name)){
   const bodyStart=node.body.getStart(sf);
   found[name].push({name,lexical_path:[...parents,identity].join('/'),source_range:{start:location,end:pos(node.end)},declaration_sha256:sha(source.slice(start,node.end)),body_range:{start:pos(bodyStart),end:pos(node.body.end)},body_sha256:sha(source.slice(bodyStart,node.body.end))});
  }
  scopes=[...parents,identity];
 }
 ts.forEachChild(node,child=>visit(child,scopes));
}
visit(sf,['<top>']);
let matched=0;
for(const [name,candidates] of Object.entries(plan.candidates))for(const old of candidates){
 const matches=found[name].filter(x=>x.body_range.start.offset===old.body_range.start.offset&&x.body_range.end.offset===old.body_range.end.offset);
 if(matches.length!==1||matches[0].body_sha256!==old.body_sha256)throw Error(`owner identity mismatch ${old.lexical_path}`);
 matched++;
}
const out={status:'Preparation only. Every owner declaration is extracted from the pinned TypeScript source, not inferred from a shared name. Choose scoped declarations for each row and recheck source/artifact hashes at final V before documentation qualification.',typescript_version:ts.version,source_path:sourcePath,source_sha256:sha(source),h1_existing_owner_bodies_verified:matched,rows:plan.rows,candidates:found};
function resolve(owner){
 const matches=found[owner.name].filter(x=>x.source_range.start.line===owner.line);
 if(matches.length!==1)throw Error(`ambiguous/missing scoped owner ${owner.name}:${owner.line}`);
 return matches[0];
}
out.selected_rows=Object.fromEntries(Object.entries(selected.rows).map(([row,owners])=>[row,owners.map(resolve)]));
out.adjacent_semantic_correction={...resolve(selected.adjacent_semantic_correction),scope:selected.adjacent_semantic_correction.scope};
fs.writeFileSync('/tmp/emitter-delivery-owner-identities-r249.json',`${JSON.stringify(out,null,2)}\n`,{flag:'wx'});
console.log(JSON.stringify({source_sha256:out.source_sha256,existing_body_hashes_verified:matched,missing_names:[...names].filter(x=>!found[x].length),candidates:Object.values(found).flat().length}));
