from pathlib import Path
import json,re,textwrap,hashlib,collections
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-ledger-prep');base=root/'docs/design/greenfield/slices/emitter-final-batch/integration/cross-review'
rows=json.loads((base/'r110-checker-dispositions.json').read_text());index=json.load(open('/tmp/emitter-ts-functions-r108.json'));source=(root/'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True);checks=[]
for row in rows:
 rel,name=row['id'].split('::');p=root/'crates/checker/src'/rel;s=p.read_text();pattern=re.compile(r'^([ \t]*)(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:const\s+)?fn '+re.escape(name)+r'\b',re.M);matches=list(pattern.finditer(s));assert len(matches)==1,row['id'];m=matches[0];indent=m[1];assert '\n' not in indent,(row['id'],repr(indent))
 lines=[]
 if row['kind']=='tsrs-native':lines+=textwrap.wrap('tsrs-native: '+row['reason'],88-len(indent))
 else:
  for block in row.get('blocks',[row]):
   match=re.fullmatch(r'_tsc.js:(\d+)-(\d+)',block['span']);a,b=map(int,match.groups());digest=hashlib.sha256(b''.join(source[a-1:b])).hexdigest();assert block['hash']==digest,row['id']
   owners=[v for v in index if v['name']==block['port'] and v['start']<=a<=b<=v['end']];assert len(owners)==1,(row['id'],owners);owner=owners[0]
   lines += ['tsc-port: '+owner['path']+' @6.0.3','tsc-hash: '+digest,'tsc-span: '+block['span']]
   if (a,b)!=(owner['start'],owner['end']):lines+=textwrap.wrap('Reference scope: '+block.get('prose','the projected statement within '+owner['path'])+'.',88-len(indent))
   checks.append({'id':row['id'],'upstream_owner':owner,'span':block['span'],'sha256':digest})
  if name.endswith('_js'):lines.append('JS-valued twin of `'+name[:-3]+'`; keep their diagnostic behavior aligned.')
  if name=='first_transformable_static_class_element':lines+=textwrap.wrap('Under standard decorators, constructor parameters cannot be decorated, so class decoration is the class-or-parameter predicate projection here.',88-len(indent))
  if name=='is_array_or_tuple_or_intersection':lines+=textwrap.wrap('This combines the array/tuple and intersection predicates. The instantiate caller guards INTERSECTION before using this projection; retain that guard.',88-len(indent))
 addition=''.join(indent+'/// '+line+'\n' for line in lines);s=s[:m.start()]+addition+s[m.start():];p.write_text(s)
p=root/'crates/checker/src/links.rs';s=p.read_text();old='    /// `links.calculatedFlags |= bits` (calculateNodeCheckFlagWorker 88132).';assert old in s;s=s.replace(old,'    /// `links.calculatedFlags |= bits` (calculateNodeCheckFlagWorker, e.g. 88170).');p.write_text(s)
report={'classifications':dict(collections.Counter(r['kind'] for r in rows)),'total':len(rows),'port_reference_checks':checks,'note':'Classification counts come from the JSON table, correcting the prose tallies in round110. No backlog or gate changes.'};(base/'r110-checker-disposition-validation.json').write_text(json.dumps(report,indent=2)+'\n');print(report['classifications'])
