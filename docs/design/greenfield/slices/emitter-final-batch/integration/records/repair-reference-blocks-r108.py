from pathlib import Path
import json,re,hashlib,subprocess
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-ledger-prep')
problems=json.load(open('/tmp/emitter-ledger-r107.json'))['problems']
index=json.load(open('/tmp/emitter-ts-functions-r108.json'))
source=(root/'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
explicit={
 0:['getTextOfJSDocComment','formatJSDocLink'],
 1:['transformClassFields.transformClassStaticBlockDeclaration'],
 11:['transformES2018.transformForOfStatementWithObjectRest'],
 13:['createPrinter.makeUniqueName'],
 15:['isAnonymousFunctionDefinition','transformESDecorators.canIgnoreEmptyStringLiteralInAssignedName'],
 53:['createPrinter.makeUniqueName'],
 82:['transformSystemModule.visitVariableStatement'],
 85:['createNodeFactory.getGeneratedNameForNode',('transformTypeScript.visitClassDeclaration',94456,94457),'createPrinter.generateNameForNode'],
 86:[('transformDeclarations.transformRoot',114446,114513)],
 87:['isExternalModuleAugmentation','isModuleAugmentationExternal'],
 90:['createTemplateLiteralLikeNode','createNodeFactory.update'],
 91:['createStringLiteral','createNodeFactory.update'],
 94:['createNodeFactory.createArrowFunction','createNodeFactory.updateArrowFunction','parenthesizeConciseBodyOfArrowFunction'],
 104:['createPrinter.pipelineEmitWithComments','createPrinter.emitLeadingCommentsOfNode','createPrinter.emitTrailingCommentsOfNode'],
 113:['createPrinter.pipelineEmitWithComments','createPrinter.emitLeadingCommentsOfNode','createPrinter.emitTrailingCommentsOfNode'],
 117:['propagateNameFlags','propagateIdentifierNameFlags','propagateChildFlags','getTransformFlagsSubtreeExclusions'],
 118:['parseIsolatedEntityName2'],
}
# Preserve narrowly described, valid interior spans. Other malformed/wide spans
# resolve to exact AST function boundaries, never a concatenated range.
subsets={3:(97218,97231),34:(99937,99942),99:(120125,120126),102:(120129,120150)}
plans=[]
for i,p in enumerate(problems):
 def resolve(name):
  matches=[x for x in index if x['path']==name]
  if len(matches)==1:return matches[0]
  matches=[x for x in index if x['name']==name]
  if len(matches)==1:return matches[0]
  pref=('transformESDecorators.' if 'standard_decorators' in p['path'] else
        'transformClassFields.' if 'class_fields/' in p['path'] else
        'createPrinter.' if 'printer.rs' in p['path'] else
        'transformDeclarations.' if 'declarations/' in p['path'] else '')
  narrowed=[x for x in matches if pref and x['path'].startswith(pref)]
  assert len(narrowed)==1,(i,name,matches)
  return narrowed[0]
 names=explicit.get(i)
 if names is None:
  raw=p['port'].split('@')[0].strip()
  raw=re.sub(r'\(.*','',raw).strip()
  names=[x.strip().split()[0] for x in raw.split('/')]
 refs=[]
 for name in names:
  node=resolve(name[0] if isinstance(name,tuple) else name)
  a,b=(name[1:] if isinstance(name,tuple) else subsets.get(i,(node['start'],node['end'])))
  assert node['start']<=a<=b<=node['end'],(i,node,a,b)
  digest=hashlib.sha256(b''.join(source[a-1:b])).hexdigest()
  refs.append({'name':node['path'],'start':a,'end':b,'hash':digest,'owner':node,'is_subspan':(a,b)!=(node['start'],node['end'])})
 plans.append({'index':i,'original':p,'references':refs})
for rel in sorted({p['path'] for p in problems}):
 path=root/rel;before=subprocess.check_output(['git','-C',str(root),'show','HEAD:'+rel],text=True);lines=before.splitlines(keepends=True)
 for plan in reversed([x for x in plans if x['original']['path']==rel]):
  p=plan['original'];needle='tsc-port: '+p['port']
  positions=[p['line']-1] if lines[p['line']-1].strip()=='/// '+needle else []
  assert len(positions)==1,(plan['index'],positions)
  start=positions[0];end=start+1
  while end<len(lines) and lines[end].lstrip().startswith('///') and not lines[end].strip().startswith('/// tsc-port:'):end+=1
  indent=lines[start].split('///')[0]
  kept=[]
  for line in lines[start+1:end]:
   if re.match(r'\s*/// tsc-(?:hash|span):',line):
    # A malformed span sometimes carries prose continuing on the next line.
    if ' — ' in line:kept.append(indent+'/// '+line.split(' — ',1)[1])
   else:kept.append(line)
  out=[]
  for ref in plan['references']:
   out.extend([f"{indent}/// tsc-port: {ref['name']} @6.0.3\n",f"{indent}/// tsc-hash: {ref['hash']}\n",f"{indent}/// tsc-span: _tsc.js:{ref['start']}-{ref['end']}\n"])
   if ref['is_subspan']:out.append(f"{indent}/// Reference scope: the documented projection within `{ref['name']}`.\n")
  # Preserve semantic qualifiers even when they formerly invalidated the tag.
  main=p['port'];simple=main.split('@')[0].strip()
  qualifier=main.split('@6.0.3',1)[1].strip() if '@6.0.3' in main else ''
  if qualifier or ' ' in simple or '(' in simple:
   out.append(f'{indent}/// Reference detail: {main}\n')
  lines[start:end]=out+kept
 after=''.join(lines)
 strip=lambda s:''.join(l for l in s.splitlines(keepends=True) if not l.lstrip().startswith('///'))
 assert strip(before)==strip(after),rel
 path.write_text(after)
base=root/'docs/design/greenfield/slices/emitter-final-batch/integration/cross-review'
report={'base':subprocess.check_output(['git','-C',str(root),'rev-parse','HEAD'],text=True).strip(),'vendor_sha256':hashlib.sha256(b''.join(source)).hexdigest(),'annotation_blocks_repaired':len(plans),'reference_count':sum(len(x['references']) for x in plans),'runtime_source_unchanged':True,'plans':plans}
(base/'r108-ledger-repairs.json').write_text(json.dumps(report,indent=2)+'\n')
print({k:v for k,v in report.items() if k!='plans'})
