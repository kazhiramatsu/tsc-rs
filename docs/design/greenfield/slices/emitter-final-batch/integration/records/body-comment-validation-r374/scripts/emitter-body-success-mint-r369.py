from pathlib import Path
import json,subprocess,gzip,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');O=Path('/tmp/emitter-body-success-mint-r369');O.mkdir(exist_ok=False)
p=R/'scripts/observe-printer-failure-review.mjs';f=R/'crates/emitter/tests/fixtures/printer-failure-review.json';a=p.read_bytes();b=f.read_bytes();old=json.loads(b);assert len(old['cases'])==22
(O/'original-observer.mjs').write_bytes(a);(O/'original-fixture.json.gz').write_bytes(gzip.compress(b,mtime=0))
s=a.decode();x="""  ops:[{...select('printFile'),printer:'fresh'},
    {target:'s0',fault:identifierFault('before',2)},select('printFile'),select('printFile'),{target:'s1'}]""";y="""  ops:[{...select('printFile'),printer:'fresh'},select('printFile'),select('printFile'),
    {target:'s0',fault:identifierFault('before',2)},select('printFile'),select('printFile'),{target:'s1'}]""";assert s.count(x)==1;s=s.replace(x,y);p.write_text(s)
try:
 f.unlink();code=subprocess.run(['python3','docs/design/greenfield/slices/emitter-final-batch/integration/run-local.py','body-success-oracle-r369','node','scripts/observe-printer-failure-review.mjs','--write'],cwd=R).returncode;assert code==0
 new=json.loads(f.read_bytes());ids={r['case_id']:r for r in new['cases']};assert len(ids)==22
 for c in old['cases']:
  if not c['case_id'].endswith('/review/function-body-sticky-comments'):assert ids[c['case_id']]==c,c['case_id']
 changed=[c['case_id'] for c in old['cases'] if ids[c['case_id']]!=c];assert len(changed)==1
except BaseException:p.write_bytes(a);f.write_bytes(b);raise
h=lambda b:hashlib.sha256(b).hexdigest();report={'scope':'Actual186 requests same-printer success twin; add2successfulsharedprintFile operations beforefault tosingle newbodycase, old21cases unchanged. Existingparent-and-body172flagcontrol alreadyaddressesreviewers otherclaimedgap. JS+events+failure exactTSobservationsx2, nativepending.','changed':changed,'observer_sha256':h(p.read_bytes()),'fixture_sha256':h(f.read_bytes()),'qualified_native':False};(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(report)
