from pathlib import Path
import json,re,hashlib,subprocess
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-ledger-prep');base=root/'docs/design/greenfield/slices/emitter-final-batch/integration/cross-review'
rep=json.loads((base/'r108-ledger-repairs.json').read_text());idx=json.load(open('/tmp/emitter-ts-functions-r108.json'))
entries=set()
for p in (root/'crates').rglob('*.rs'):
 docs=[]
 for line in p.read_text().splitlines():
  t=line.strip()
  if t.startswith('///'):docs.append(t[3:].strip());continue
  if not t or t.startswith('#['):continue
  if re.match(r'(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:const\s+)?fn\s+',t):
   text='\n'.join(docs)
   for block in text.split('tsc-port:')[1:]:
    h=re.search(r'tsc-hash:\s*([a-f0-9]{64})',block);s=re.search(r'tsc-span:\s*_tsc.js:(\d+)-(\d+)',block)
    if h and s:entries.add((*map(int,s.groups()),h[1]))
  docs=[]
inv=json.loads((root/'m8-emitter-inventory.json').read_text());disp=json.loads((root/'m8-emitter-dispositions.json').read_text());by={e['id']:e for e in inv['functions']};stats={};violations=[]
for entry in disp['entries']:
 f=by[entry['declaration']];joined=(f['source_range']['start']['line'],f['source_range']['end']['line'],f['source_slice_sha256']) in entries
 key=entry['disposition']+('-joined' if joined else '-unjoined');stats[key]=stats.get(key,0)+1
 if (entry['disposition']=='ported' and not joined) or(entry['disposition']=='not-applicable' and joined):violations.append(entry['declaration'])
changed=subprocess.check_output(['git','-C',str(root),'diff','--name-only'],text=True).splitlines();identities=[]
strip=lambda b:b'\n'.join(l for l in b.split(b'\n') if not l.lstrip().startswith(b'///'))
for rel in changed:
 before=subprocess.check_output(['git','-C',str(root),'show','HEAD:'+rel]);after=(root/rel).read_bytes();assert strip(before)==strip(after),rel
 identities.append({'path':rel,'before_sha256':hashlib.sha256(before).hexdigest(),'after_sha256':hashlib.sha256(after).hexdigest(),'non_doc_bytes_equal':True})
for plan in rep['plans']:
 for ref in plan['references']:
  owner=ref['owner'];assert owner in idx;assert owner['start']<=ref['start']<=ref['end']<=owner['end'];assert owner['path']==ref['name']
report={'base':rep['base'],'annotation_blocks':len(rep['plans']),'references':rep['reference_count'],'containment_valid':True,'d2_status':disp['status'],'d2_stats':stats,'d2_violations':violations,'source_identities':identities}
(base/'r108-ledger-containment-and-d2.json').write_text(json.dumps(report,indent=2)+'\n')
print({k:v for k,v in report.items() if k!='source_identities'});assert not violations
