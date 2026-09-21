from pathlib import Path
import hashlib,json,difflib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');O=Path('/tmp/emitter-module-controls-r414');O.mkdir(exist_ok=False)
h=lambda b:hashlib.sha256(b).hexdigest();files=[]
shapes=[('import-'+p.replace(' ','-'),p+" /* a */ import /* b */ './dep';\n") for p in ['export','declare','public','async','declare export']]+[('export-star-'+p,p+" /* a */ export /* b */ * /* c */ from /* d */ './dep';\n") for p in ['export','declare','public','async']]+[('assignment-'+p.replace(' ','-'),p+' /* a */ export /* b */ default /* c */ 1;\n') for p in ['export','async','declare export']]+[('namespace-'+p,p+" /* a */ export /* b */ * /* c */ as /* d */ ns from './dep';\n") for p in ['export','declare']]+[('import-line-unicode',"export // 😀 a\r\nimport /* b */ './dep';\r\n"),('import-multiple-comments',"declare /* a */ export /* b */ import /* c */ './dep';\n")]
assert len(shapes)==16
append='''
// Recovery modifiers survive only on the upstream declaration-specific paths.
// Compare complete JS/declaration/map commands, including comments removed.
const moduleRecoveryOwners = '''+json.dumps(shapes,ensure_ascii=False,indent=2)+r''';
for (const [name,text] of moduleRecoveryOwners)
 for (const removeComments of [false,true])
  for (const emitDeclarationOnly of [false,true])
   add(`module-recovery-owner/${name}/remove-${removeComments}/${emitDeclarationOnly?"declaration-only":"all"}`,
    text,{removeComments,declaration:true,declarationMap:true,emitDeclarationOnly,sourceMap:!emitDeclarationOnly});
for (const [moduleName,module] of [["commonjs",ts.ModuleKind.CommonJS],["system",ts.ModuleKind.System]])
 for (const [name,text] of moduleRecoveryOwners.filter(([name])=>
  ["import-export","import-declare","export-star-declare","assignment-export"].includes(name)))
  add(`module-recovery-lowered/${moduleName}/${name}`,text,
   {module,declaration:true,declarationMap:true});
for (const [name,text] of [
 ["declare", "declare /* a */ export /* b */ = /* c */ 1;\n"],
 ["export", "export /* a */ export /* b */ = /* c */ 1;\n"],
]) for (const removeComments of [false,true])
 for (const emitDeclarationOnly of [false,true])
  add(`module-recovery-equals/${name}/remove-${removeComments}/${emitDeclarationOnly?"declaration-only":"all"}`,
   text,{module:ts.ModuleKind.CommonJS,removeComments,declaration:true,declarationMap:true,
    emitDeclarationOnly,sourceMap:!emitDeclarationOnly});
assert.equal(inputs.length,245);
'''
for rel in ['scripts/observe-token-comment-phases.mjs','crates/compiler/tests/integration/h2_8a_token_comment_phases.rs','crates/compiler/tests/integration/h2_8a_ellipsis_comment_owners.rs']:
 before=(R/rel).read_text();after=before
 if rel.endswith('.mjs'):
  needle='assert.equal(inputs.length,165);';assert after.count(needle)==1;after=after.replace(needle,needle+append)
 elif rel.endswith('h2_8a_token_comment_phases.rs'):
  for a,b in [('assert_eq!(cases.len(), 165);','assert_eq!(cases.len(), 245);'),('assert_eq!(cases.len(), 329);','assert_eq!(cases.len(), 409);')]:assert after.count(a)==1;after=after.replace(a,b)
 else:
  assert '434' in after
  after=after.replace('434','514').replace('assert_eq!(prior.len(), 165);','assert_eq!(prior.len(), 245);')
  # Fixture cardinalities are independently guarded by the token owner suite.
 dest=O/rel;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_text(after);files.append({'path':rel,'before':h(before.encode()),'after':h(after.encode())})
(O/'manifest.json').write_text(json.dumps({'scope':'Unapplied controls proposal; observer must run from canonical after frozen396 finishes, old165 observations must stay exact; new80 rows require native complete command comparisons.','files':files,'base_token_rows':165,'new_rows':80,'final_token_rows':245,'token_suite':409,'ellipsis_suite':514},indent=2)+'\n');print(O)
