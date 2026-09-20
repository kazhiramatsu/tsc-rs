from pathlib import Path
import hashlib,json,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');O=Path('/tmp/emitter-export-clause-controls-r438');O.mkdir(exist_ok=False);h=lambda b:hashlib.sha256(b).hexdigest();rows=[]
extra=r'''// Export clauses own a fresh leading phase when recovery leaves a prior modifier.
for (const [name,text] of [
 ["named-recovered", "export /* a */ export /* b */ { ns } from './dep';\n"],
 ["namespace-recovered-line", "export /* a */ export // 😀 b\r\n* /* c */ as /* d */ ns from './dep';\r\n"],
 ["namespace-normal-inline", "export /* a */ * /* c */ as /* d */ ns from './dep';\n"],
 ["namespace-normal-line", "export // 😀 a\r\n* /* c */ as /* d */ ns from './dep';\r\n"],
 ["namespace-type-recovered", "export /* a */ export /* b */ type /* c */ * /* d */ as /* e */ ns from './dep';\n"],
]) for (const removeComments of [false,true])
 for (const emitDeclarationOnly of [false,true])
  add(`export-clause-owner/${name}/remove-${removeComments}/${emitDeclarationOnly?"declaration-only":"all"}`,
   text,{removeComments,declaration:true,declarationMap:true,emitDeclarationOnly,sourceMap:!emitDeclarationOnly});
'''
for p in ['scripts/observe-token-comment-phases.mjs','crates/compiler/tests/integration/h2_8a_token_comment_phases.rs','crates/compiler/tests/integration/h2_8a_ellipsis_comment_owners.rs']:
 old=(R/p).read_bytes();s=old.decode()
 if p.endswith('.mjs'):
  a='assert.equal(inputs.length,245);';assert s.count(a)==1;s=s.replace(a,extra+'assert.equal(inputs.length,265);')
 elif 'token_comment_phases.rs' in p:
  for a,b in [('assert_eq!(cases.len(), 245);','assert_eq!(cases.len(), 265);'),('assert_eq!(cases.len(), 409);','assert_eq!(cases.len(), 429);')]:assert s.count(a)==1;s=s.replace(a,b)
 else:
  for a,b in [('assert_eq!(prior.len(), 245);','assert_eq!(prior.len(), 265);'),('assert_eq!(cases.len(), 514);','assert_eq!(cases.len(), 534);')]:assert s.count(a)==1;s=s.replace(a,b)
 dst=O/p;dst.parent.mkdir(parents=True,exist_ok=True);dst.write_text(s);rows.append({'path':p,'before':h(old),'after':h(dst.read_bytes())})
subprocess.run(['node','--check',str(O/'scripts/observe-token-comment-phases.mjs')],check=True)
patch=b''
for r in rows:
 q=subprocess.run(['diff','-u','--label','a/'+r['path'],'--label','b/'+r['path'],str(R/r['path']),str(O/r['path'])],capture_output=True);assert q.returncode==1;patch+=q.stdout
(O/'proposal.patch').write_bytes(patch);(O/'manifest.json').write_text(json.dumps({'files':rows,'prior_complete_commands':245,'added_complete_commands':20,'final_complete_commands':265,'token_suite':429,'ellipsis_suite':534,'qualified':False},indent=2)+'\n');print(patch.decode())
