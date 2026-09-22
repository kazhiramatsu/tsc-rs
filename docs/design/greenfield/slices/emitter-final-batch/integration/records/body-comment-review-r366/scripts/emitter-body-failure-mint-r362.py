from pathlib import Path
import json,subprocess,gzip,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');O=Path('/tmp/emitter-body-failure-mint-r362');O.mkdir(exist_ok=False)
script=R/'scripts/observe-printer-failure-review.mjs';fixture=R/'crates/emitter/tests/fixtures/printer-failure-review.json'
a=script.read_bytes();b=fixture.read_bytes();old=json.loads(b);assert len(old['cases'])==21
(O/'original-observer.mjs').write_bytes(a);(O/'original-fixture.json.gz').write_bytes(gzip.compress(b,mtime=0))
s=a.decode();marker="for (const phase of ['success', 'before']) {";assert s.count(marker)==1
addition=r'''// A function body's own detached head/tail surround its suppression extent.
// Failure in the callback must keep that extent active on the shared printer.
reviewSpecs.push(printNodeSpec('review/function-body-sticky-comments', {
  sources:[{name:'main.ts',text:'function f() {\r\n// head\r\n\r\nx(/* inner */);\r\n// tail\r\n}\r\n// next\r\ng(/* next arg */);\r\n'}],
  tracked:[K.SourceFile,K.ExpressionStatement,K.Identifier],
  targets:files=>{ts.setEmitFlags(files[0].statements[0].body,ts.EmitFlags.NoNestedComments);return targets(files);},
  emit_flags:[{target:'s0',path:'body',flags:ts.EmitFlags.NoNestedComments}],
  ops:[{...select('printFile'),printer:'fresh'},
    {target:'s0',fault:identifierFault('before',2)},select('printFile'),select('printFile'),{target:'s1'}]
}));
'''
s=s.replace(marker,addition+marker).replace('assert.equal(cases.length,21);','assert.equal(cases.length,22);');script.write_text(s)
try:
 fixture.unlink()
 code=subprocess.run(['python3','docs/design/greenfield/slices/emitter-final-batch/integration/run-local.py','body-failure-oracle-r362','node','scripts/observe-printer-failure-review.mjs','--write'],cwd=R).returncode;assert code==0
 fresh=json.loads(fixture.read_bytes());assert len(fresh['cases'])==22;ids={c['case_id']:c for c in fresh['cases']}
 for c in old['cases']:assert ids[c['case_id']]==c,c['case_id']
 for k,v in old.items():
  if k!='cases':assert fresh[k]==v,k
except BaseException:
 script.write_bytes(a);fixture.write_bytes(b);raise
sha=lambda b:hashlib.sha256(b).hexdigest()
report={'scope':'Existing serialized failure fixture21->22; old21 unchanged; function body NoNestedComments fault then same-printer printFile twice and subsequent node. TS observations twice, native pending.','oracle_exit':0,'observer_sha256':sha(script.read_bytes()),'fixture_sha256':sha(fixture.read_bytes()),'qualified_native':False};(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
