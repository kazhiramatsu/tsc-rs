from pathlib import Path
import gzip,hashlib,json,shutil,subprocess
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');b=r/'docs/design/greenfield/slices/emitter-final-batch/integration'
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=r,text=True).strip()
assert head=='628562c31a5102348a7e8769f3a2aa88deef70f1'
assert (b/'records/prewalk-validation-r244/manifest.json').is_file(),'Archive the completed frozen-source validation first'
out=b/'records/delivery-preparation-r258';out.mkdir(exist_ok=False)
sha=lambda x:hashlib.sha256(x).hexdigest();files=[]
def put(name,data):
 p=out/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data)
 files.append({'path':name,'sha256':sha(data),'bytes':len(data)})
for name in ['emitter-walk-cache-preflight-r253.json','emitter-delivery-final-input-audit-r254.py','emitter-delivery-final-input-audit-r254-failure.md','emitter-delivery-final-input-audit-r255.py','emitter-delivery-final-input-audit-r255.json','emitter-delivery-draft-r256.py','emitter-delivery-draft-r259.py','emitter-final-ci-run-r260.py']:
 put(name,(Path('/tmp')/name).read_bytes())
put(Path(__file__).name,Path(__file__).read_bytes())
review=json.loads(Path('/tmp/emitter-claude-review-round168-opus.json').read_text())
put('cross-review/round168-request.md',Path('/tmp/emitter-round168-request.md').read_bytes())
put('cross-review/round168-opus.md',(review['result']+'\n').encode())
put('cross-review/round168-opus.meta.json',(json.dumps({k:v for k,v in review.items() if k!='result'},indent=2)+'\n').encode())
put('final-ci-log-destination.md',b'The final local CI enumerates tracked and non-ignored untracked files and compares file lengths/mtimes after every phase (crates/xtask/src/local_ci_resume.rs:repository_paths/stability_marker). Therefore the ordinary focused run-local.py log under records/local would itself change a monitored input during CI. The prepared r260 wrapper saves final gate logs and receipts outside the canonical checkout, preserves the exact unsplit argv and demoted/Cargo2 policy, and copies any official failure journal before a permitted normal-priority retry. No gate, input-set exclusion, source, or policy change is needed. The wrapper has only been syntax parsed; final CI has not run.\n')
p=b/'records/local/delivery-final-input-audit-r255.json';j=json.loads(p.read_bytes());raw=gzip.decompress(p.with_suffix('.log.gz').read_bytes())
assert j['exit']==0 and j['head']==head and j['tracked_clean']
assert sha(raw)==j['log_sha256'] and len(raw)==j['log_bytes']
put('local/'+p.name,p.read_bytes());put('local/'+p.with_suffix('.log.gz').name,p.with_suffix('.log.gz').read_bytes())
(out/'manifest.json').write_text(json.dumps({'source_head':head,'qualification_claim':False,'scope':'Read-only preparation:18 rows,74 TypeScript routing-owner instances with declaration/body digests recomputed using UTF16 ranges;24 existing profile artifacts retain1251 stale pre-walk runtime-input references. This is not final qualification. r254 stopped on an already-deleted path in the old profile; r255 records absent/current mismatches and still refuses any mismatch in --merge mode. Actual Opus168 reviewed the first D draft; r259 adds actual walk certificate, named gates SUCCESS, PR561, all-phase completion/reuse, exact tree, and reference guards with narrower test-file wording. The r259 D generator and r260 external-log final CI wrapper were syntax parsed but never executed: actual final local+hosted+merge receipts do not exist yet. The local new-ci/target Cargo-cache symlink was later found non-ignored because the existing target/ rule matches directories; it must be replaced with an ignored directory layout before final CI. No policy/source change is needed. Canonical H2.5g Node receipt remains absent and the default walk allows one cold observation without an override.','files':files},indent=2)+'\n')
print('Archived',len(files),'preparation artifacts; no qualification claim.')
