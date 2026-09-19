from pathlib import Path
import gzip,json,subprocess
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
b=r/'docs/design/greenfield/slices/emitter-final-batch/integration'
p=r/'crates/compiler/tests/fixtures/emitter-r168-corpus-controls.json'
archive=b/'records/unicode-layout-oracle-r209'/ (p.name+'.gz')
raw=gzip.decompress(archive.read_bytes());assert p.read_bytes()==raw
old=json.loads(raw);p.unlink()
subprocess.run(['python3',str(b/'run-local.py'),'unicode-layout-oracle-r210','node','scripts/observe-emitter-r168-corpus-controls.mjs','--write'],cwd=r,check=True)
new=json.loads(p.read_text());by={c['case_id']:c for c in new['cases']};assert len(by)==1080
assert len(old['cases'])==832
for case in old['cases']:assert by[case['case_id']]==case,case['case_id']
print('Retained all 832 predecessor inputs and observations unchanged; added 248 complete-command controls, each TS-observed twice.',flush=True)
