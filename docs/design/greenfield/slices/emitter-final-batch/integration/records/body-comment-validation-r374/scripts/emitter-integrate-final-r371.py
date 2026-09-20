from pathlib import Path
import subprocess,json,gzip,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');P=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');O=Path('/tmp/emitter-integrate-final-r371');O.mkdir(exist_ok=False)
def git(tree,*args):return subprocess.check_output(['git',*args],cwd=tree)
base='628562c31a5102348a7e8769f3a2aa88deef70f1';private='4aeb1640156f291f4b5189b8ad9ed370cf17cf9d';assert git(P,'rev-parse','HEAD').decode().strip()==private
assert git(R,'rev-parse','HEAD').decode().strip()=='9e46ae514fb6fa7f389abb22335ffddfef79b2ea'
qualified=json.loads((T/'emitter-body-commands-r365/manifest.json').read_text());assert qualified['qualified'] and qualified['test_files_restored']
assert json.loads((T/'emitter-body-tests-followup-r370/manifest.json').read_text())['qualified']
assert json.loads((T/'emitter-body-measurement-r372/manifest.json').read_text())['qualified']
syntax=['crates/syntax/src/recovery.rs','crates/syntax/src/recovery/context.rs','crates/syntax/tests/unit/parser/recovery.rs'];product=['crates/emitter/src/printer.rs','crates/emitter/src/builtins/system.rs','crates/emitter/tests/unit/source_map/tests.rs','crates/emitter/tests/unit/builtins/tests.rs']
for p in syntax+product:
 if not p.endswith('/src/printer.rs'):assert (R/p).read_bytes()==git(R,'show',base+':'+p),p
patch=git(P,'diff',base,private,'--',*syntax)+git(P,'diff','HEAD','--',*product)
(O/'integration.patch.gz').write_bytes(gzip.compress(patch,mtime=0));before={p:(R/p).read_bytes() for p in syntax+product}
subprocess.run(['git','apply','--check','-'],cwd=R,input=patch,check=True)
subprocess.run(['git','apply','-'],cwd=R,input=patch,check=True)
for p in syntax+product:
 if not p.endswith('/src/printer.rs'):assert (R/p).read_bytes()==(P/p).read_bytes(),p
assert b'let modifiers_erased = data.modifiers.is_none()' in (R/product[0]).read_bytes()
for name in ['source_map_recording_witness_contract.rs','source_map_emit_witness_contract.rs']:(R/'crates/compiler/tests/integration'/name).write_bytes((Path('/tmp/emitter-empty-variable-witness-controls-r334')/name).read_bytes())
for name in ['list_comment_flags_contract.rs','printer_failure_contract.rs']:(R/'crates/emitter/tests'/name).write_bytes((P/'crates/emitter/tests'/name).read_bytes())
sha=lambda b:hashlib.sha256(b).hexdigest();report={'canonical_head':git(R,'rev-parse','HEAD').decode().strip(),'private_head':private,'scope':'Integrated bounded syntax4commits (330 original16994 proof), validated System/currentarray EOFmap + typednegativeguard + functionbodyhead/tail/NoNested extents. CanonicalA279 importmodifiercomment fix preserved. Witness334 retired onlyafterfullactual8witnessPASS365. Productintegrationpendingcanonicalfocused/fullgates.','patch_sha256':sha(patch),'before':{p:sha(b) for p,b in before.items()},'after':{p:sha((R/p).read_bytes()) for p in before},'canonical_qualified':False};(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(report)
