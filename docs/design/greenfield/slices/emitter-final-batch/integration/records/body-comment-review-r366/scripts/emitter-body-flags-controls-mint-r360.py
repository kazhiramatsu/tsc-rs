from pathlib import Path
import json,subprocess,gzip,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');O=Path('/tmp/emitter-body-flags-controls-mint-r360');O.mkdir(exist_ok=False);sha=lambda b:hashlib.sha256(b).hexdigest()
script=R/'scripts/observe-list-comment-flags.mjs';fixture=R/'crates/emitter/tests/fixtures/list-comment-flags.json';oldscript=script.read_bytes();raw=fixture.read_bytes();old=json.loads(raw);assert len(old['cases'])==147 and old['observer_sha256']==sha(oldscript)
(O/'original-observer.mjs').write_bytes(oldscript);(O/'original-fixture.json.gz').write_bytes(gzip.compress(raw,mtime=0));s=oldscript.decode()
a='''  const parent = statement.expression ?? statement.declarationList;''';b='''  if (ts.isFunctionDeclaration(statement)) {
    if (variant === "ParentNoNestedComments" || variant === "ParentAndBodyNoNestedComments")
      ts.setEmitFlags(statement, ts.EmitFlags.NoNestedComments);
    if (variant !== "ParentNoNestedComments")
      ts.setEmitFlags(statement.body, variant === "ParentAndBodyNoNestedComments"
        ? ts.EmitFlags.NoNestedComments : ts.EmitFlags[variant]);
    return ts.createPrinter({newLine:ts.NewLineKind.LineFeed, removeComments}).printFile(file);
  }
'''+a;assert s.count(a)==1;s=s.replace(a,b)
a='''const artifact = {version:1, typescript:ts.version, repetitions:2,'''
b=r'''const bodies = [
  ["function-body", "function f() {\n// head\n\nx(/* inner */);\n// tail\n}\n"],
  ["empty-function-body", "function f() {\n// head\n\n// tail\n}\n"],
];
for (const [shape, source] of bodies) for (const variant of variants) for (const remove_comments of [false, true]) {
  const output = observe(source, variant, remove_comments);
  assert.equal(observe(source, variant, remove_comments), output);
  cases.push({case_id:`${shape}/${variant}/${remove_comments ? "removed" : "retained"}`, source, variant, remove_comments, output});
}
// Both extents must leave the parent's suppression active until its own exit.
{
  const [shape, source] = bodies[0], variant = "ParentAndBodyNoNestedComments", remove_comments = false;
  const output = observe(source, variant, remove_comments);
  assert.equal(observe(source, variant, remove_comments), output);
  cases.push({case_id:`${shape}/${variant}/retained`, source, variant, remove_comments, output});
}
'''+a
assert s.count(a)==1;s=s.replace(a,b);script.write_text(s)
try:
 code=subprocess.run(['python3','docs/design/greenfield/slices/emitter-final-batch/integration/run-local.py','body-flags-oracle-r360','node','scripts/observe-list-comment-flags.mjs','--write'],cwd=R).returncode;assert code==0
 new=json.loads(fixture.read_bytes());assert len(new['cases'])==172;byid={c['case_id']:c for c in new['cases']}
 for c in old['cases']:assert byid[c['case_id']]==c,c['case_id']
 for k,v in old.items():
  if k not in ['cases','observer_sha256']:assert new[k]==v,k
except BaseException:
 script.write_bytes(oldscript);fixture.write_bytes(raw);raise
report={'scope':'Existing printer-flags fixture extended by24parsed-function body controls (nonempty/empty×6flags×remove2) +1nestedparent-and-body extent. All147originaloutputs unchanged; exactTSprintFilebytes twice only, not source maps or compiler commands/admission. Nativepending. Existingregisteredfixture/generator reused, final5gwalkrequalifies input.','oracle_exit':0,'fixture_sha256':sha(fixture.read_bytes()),'observer_sha256':sha(s.encode()),'qualified_native':False};(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
