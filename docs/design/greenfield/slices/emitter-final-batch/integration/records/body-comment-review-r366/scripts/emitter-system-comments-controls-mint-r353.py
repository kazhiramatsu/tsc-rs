from pathlib import Path
import json,subprocess,gzip,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');O=T/'emitter-system-comments-controls-mint-r353';O.mkdir(exist_ok=False)
sha=lambda b:hashlib.sha256(b).hexdigest();script=R/'scripts/observe-emitter-context-recovery.mjs';fixture=R/'crates/compiler/tests/fixtures/emitter-context-recovery.json';raw=fixture.read_bytes();old=json.loads(raw);oldscript=script.read_bytes()
assert len(old['cases'])==684 and len(old['refused_cases'])==72 and old['observer_sha256']==sha(oldscript)
(O/'previous756.json.gz').write_bytes(gzip.compress(raw,mtime=0));(O/'original-observer.mjs').write_bytes(oldscript)
s=oldscript.decode();needle='assert.equal(inputs.length, 756);';assert s.count(needle)==1
extra=r'''
// Range-owned function-body comment boundaries, including empty and erased lists.
const systemCommentBoundaries = [
  ["pinned-prefix", "/*! keep */\n\nexport const x = 5;\n"],
  ["line-prefix", "// detached\n\nexport const x = 5;\n"],
  ["attached-prefix", "/* attached */\nexport const x = 5;\n"],
  ["brace-in-trivia", "export const x = '}';\n// } last\n"],
  ["removed-tail", "export const x = 5;\n// before type\ninterface Gone {}\n// after type\n"],
  ["empty-execute", "/*! keep */\n\nexport {};\n// end\n"],
  ["erased-prologue", "/*! keep */\n\ndeclare const marker: number;\nexport const x = 5;\n// end\n"],
  ["ordinary-function", "export function f() {/* inner */\n\nreturn 1;\n// last\n}\n"],
];
for (const target of ["es5", "es2015"])
  for (const alwaysStrict of [false, true])
    for (const removeComments of [false, true])
      for (const [shape, text] of systemCommentBoundaries) {
        inputs.push({case_id: `emitter-context-recovery/system-comment-boundary/${target}/strict-${alwaysStrict}/remove-${removeComments}/${shape}`,
          roots: ["/project/main.ts"], files: [{path: "/project/main.ts", text}], options: {},
          config: JSON.stringify({compilerOptions: {target, module: "system", alwaysStrict, removeComments,
            strict: false, skipDefaultLibCheck: true, noErrorTruncation: true,
            sourceMap: true, declaration: true, declarationMap: true,
            ignoreDeprecations: "6.0", outDir: "/project/out"}, files: ["main.ts"]})});
      }
assert.equal(inputs.length, 820);
'''
s=s.replace(needle,needle+'\n'+extra)
# The supported count check is separate from the whole input count.
assert s.count('assert.equal(cases.length, 684);')==1;s=s.replace('assert.equal(cases.length, 684);','assert.equal(cases.length, 748);')
(O/'proposed-observer.mjs').write_text(s);script.write_text(s);fixture.unlink()
try:
 code=subprocess.run(['python3','docs/design/greenfield/slices/emitter-final-batch/integration/run-local.py','system-comments-oracle-r353','node','scripts/observe-emitter-context-recovery.mjs','--write'],cwd=R).returncode
 assert code==0
 new=json.loads(fixture.read_bytes());assert len(new['cases'])==748 and new['refused_cases']==old['refused_cases'];byid={c['case_id']:c for c in new['cases']};assert len(byid)==748
 for c in old['cases']:assert byid[c['case_id']]==c,c['case_id']
 for k,v in old.items():
  if k not in ['cases','observer_sha256']:assert new[k]==v,k
except BaseException:
 fixture.write_bytes(raw);script.write_bytes(oldscript);raise
roster=R/'crates/compiler/tests/fixtures/emitter-r77-regressions.json';before=roster.read_bytes();j=json.loads(before);assert j['fixtures']['emitter-context-recovery.json']==sha(raw);assert len(j['cases'])==70
(O/'original-r77-roster.json').write_bytes(before);after=before.replace(sha(raw).encode(),sha(fixture.read_bytes()).encode());check=json.loads(after);assert check['cases']==j['cases'];check['fixtures']['emitter-context-recovery.json']=sha(raw);assert check==j;roster.write_bytes(after)
for path,needle,replacement in [('crates/compiler/tests/integration/h2_8a_import_helpers.rs','assert_eq!(cases.len(), 684);','assert_eq!(cases.len(), 748);'),('crates/syntax/tests/emitter_recovery.rs','[("cases", true, 684), ("refused_cases", false, 72)]','[("cases", true, 748), ("refused_cases", false, 72)]')]:
 p=R/path;text=p.read_text();assert text.count(needle)==1;p.write_text(text.replace(needle,replacement))
report={'scope':'Official original684+72 preserved,64additional function-body comment controls x2. Nativecommands unqualified, actual342fourcommentsdifferences remainopen. No source admission/AST mutation.','oracle_exit':0,'old684_and72_unchanged':True,'fixture_sha256':sha(fixture.read_bytes()),'observer_sha256':sha(s.encode()),'qualified_native':False,'new_case_ids':[c['case_id'] for c in new['cases'] if '/system-comment-boundary/' in c['case_id']]}
(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in report.items() if k!='new_case_ids'}))
