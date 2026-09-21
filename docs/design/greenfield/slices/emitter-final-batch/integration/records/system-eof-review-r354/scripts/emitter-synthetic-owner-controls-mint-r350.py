from pathlib import Path
import json,subprocess,gzip,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');O=Path('/tmp/emitter-synthetic-owner-controls-mint-r350');O.mkdir(exist_ok=False)
sha=lambda b:hashlib.sha256(b).hexdigest()
script=R/'scripts/observe-meta-property-token-map-invariants.mjs';fixture=R/'crates/emitter/tests/fixtures/meta-property-token-map-invariants.json'
s=script.read_bytes();raw=fixture.read_bytes();old=json.loads(raw)
assert len(old['rows'])==10 and old['observer_sha256']==sha(s)
(O/'original-observer.mjs').write_bytes(s);(O/'original-fixture.json.gz').write_bytes(gzip.compress(raw,mtime=0))
new=s.decode().replace("'absent-name', 'synthetic']", "'absent-name', 'synthetic', 'synthetic-owner']")
a="""        } else if (mode === 'synthetic') {
          return ts.factory.createMetaProperty(node.keywordToken,
            ts.factory.createIdentifier(node.name.text));
        }"""
b="""        } else if (mode === 'synthetic') {
          return ts.factory.createMetaProperty(node.keywordToken,
            ts.factory.createIdentifier(node.name.text));
        } else if (mode === 'synthetic-owner') {
          return ts.factory.createMetaProperty(node.keywordToken, node.name);
        }"""
assert new.count(a)==1;new=new.replace(a,b).replace('ten rows observed twice','twelve rows observed twice');script.write_text(new)
try:
 code=subprocess.run(['python3','docs/design/greenfield/slices/emitter-final-batch/integration/run-local.py','synthetic-owner-oracle-r350','node','scripts/observe-meta-property-token-map-invariants.mjs','--write'],cwd=R).returncode
 assert code==0
 observed=json.loads(fixture.read_bytes());assert len(observed['rows'])==12
 byid={r['case_id']:r for r in observed['rows']}
 for r in old['rows']:assert byid[r['case_id']]==r
 for k,v in old.items():
  if k not in ['rows','observer_sha256']:assert observed[k]==v,k
except BaseException:
 script.write_bytes(s);fixture.write_bytes(raw);raise
ready=R/'scripts/check-meta-property-token-maps-readiness.py';before=ready.read_bytes();text=before.decode();assert text.count("len(f['rows'])==8")==1
ready.write_text(text.replace("len(f['rows'])==8","len(f['rows'])==12").replace(';8 internal rows,',';12 internal rows (8 original +4 synthetic),'))
(O/'original-readiness.py').write_bytes(before)
report={'scope':'Actual Opus182 isolating control: synthetic MetaProperty owner keeps parsed identifier child.12internalrows twice, original8 and347new2 unchanged. Historical source_commit continues identifying original family origin; this receipt identifies supplemental observations at current head. Native pending; shared5g inputs requalify in mandatory final walk, no pin bypass.','head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'oracle_exit':0,'fixture_sha256':sha(fixture.read_bytes()),'observer_sha256':sha(new.encode()),'new_rows':[r['case_id'] for r in observed['rows'] if r['mode']=='synthetic-owner'],'qualified_native':False}
(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
