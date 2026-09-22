from pathlib import Path
import json,subprocess,gzip,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');O=Path('/tmp/emitter-synthetic-meta-controls-mint-r347');O.mkdir(exist_ok=False)
sha=lambda b:hashlib.sha256(b).hexdigest()
script=R/'scripts/observe-meta-property-token-map-invariants.mjs';fixture=R/'crates/emitter/tests/fixtures/meta-property-token-map-invariants.json'
s=script.read_bytes();raw=fixture.read_bytes();old=json.loads(raw)
assert len(old['rows'])==8 and old['observer_sha256']==sha(s)
(O/'original-observer.mjs').write_bytes(s);(O/'original-fixture.json.gz').write_bytes(gzip.compress(raw,mtime=0))
new=s.decode().replace("'token-override', 'absent-name'", "'token-override', 'absent-name', 'synthetic'")
a="""        } else if (mode === 'absent-name') {
          return ts.factory.updateMetaProperty(node, undefined);
        }"""
b="""        } else if (mode === 'absent-name') {
          return ts.factory.updateMetaProperty(node, undefined);
        } else if (mode === 'synthetic') {
          return ts.factory.createMetaProperty(node.keywordToken,
            ts.factory.createIdentifier(node.name.text));
        }"""
assert new.count(a)==1;new=new.replace(a,b).replace('eight rows observed twice','ten rows observed twice');script.write_text(new)
try:
 code=subprocess.run(['python3','docs/design/greenfield/slices/emitter-final-batch/integration/run-local.py','synthetic-meta-oracle-r347','node','scripts/observe-meta-property-token-map-invariants.mjs','--write'],cwd=R).returncode
 assert code==0
 observed=json.loads(fixture.read_bytes());assert len(observed['rows'])==10
 byid={r['case_id']:r for r in observed['rows']}
 for r in old['rows']:assert byid[r['case_id']]==r
 for k,v in old.items():
  if k not in ['rows','observer_sha256']:assert observed[k]==v,k
except BaseException:
 script.write_bytes(s);fixture.write_bytes(raw);raise
report={'scope':'Internal printer factory controls only, not compiler command/source admission. Canonical TypeScript6.0.3 twice; original8 rowsexact unchanged;2synthetic MetaProperty rows added. Nativequalificationpending.','oracle_exit':0,'fixture_sha256':sha(fixture.read_bytes()),'observer_sha256':sha(new.encode()),'new_rows':[r['case_id'] for r in observed['rows'] if r['mode']=='synthetic'],'qualified_native':False}
(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
