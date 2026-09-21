from pathlib import Path
import json,subprocess,hashlib,time
T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');O=Path('/tmp/emitter-leading-binding-panic-r321');O.mkdir(exist_ok=False);snap=json.loads((T/'emitter-recovery-census-r78/parse-snapshot.json').read_bytes());binary=T/'emitter-leading-binding-successor-r314/build/target/debug/leading-binding-parse-replay';keys=list(snap['inputs']);steps=[];start=time.monotonic()
def probe(ids):
 data={k:snap[k] for k in ['schema','kind','digest_code_sha256']};data['inputs']={k:snap['inputs'][k] for k in ids};raw=json.dumps(data,separators=(',',':')).encode();p=subprocess.run(['taskpolicy','-b','nice','-n','15',str(binary)],input=raw,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE);steps.append({'count':len(ids),'exit':p.returncode,'stderr':p.stderr.decode()});assert p.returncode in [0,101]
 if p.returncode:assert 'context.rs:132:75' in p.stderr.decode() and 'index out of bounds' in p.stderr.decode()
 return p.returncode!=0
while len(keys)>1:
 left=keys[:len(keys)//2]
 if probe(left):keys=left
 else:keys=keys[len(keys)//2:]
assert probe(keys);key=keys[0];inp=snap['inputs'][key];(O/'input.json').write_text(json.dumps(inp,indent=2)+'\n');rows=[{'case_id':r['case_id'],'loader':r['loader'],'units':[u for u in r['units'] if u['input_id']==key]} for r in snap['rows'] if any(u['input_id']==key for u in r['units'])];report={'scope':'read-only isolation using subsetinputs through unchanged frozenreplaybinary; this is failure reproduction, notcorpusqualification','prototype_head':'2df8d1cd0bae5a26efd1d4268bab4cc7cfd9cc12','input_id':key,'original_rows':rows,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'steps':steps,'seconds':time.monotonic()-start};(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'id':key,'input':inp,'rows':rows,'seconds':report['seconds']},indent=2))
