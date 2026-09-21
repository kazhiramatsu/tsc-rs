from pathlib import Path
import subprocess,json,hashlib,datetime
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');D=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/debug/deps');O=Path('/tmp/emitter-synthetic-body-probe-r391');assert set(x.name for x in O.iterdir())=={"main.rs"}
receipt_path=R/"docs/design/greenfield/slices/emitter-final-batch/integration/records/local/canonical-focused-emitter-all-r349.json";receipt=json.loads(receipt_path.read_bytes());started=datetime.datetime.fromisoformat(receipt["started_at"]).timestamp();finished=started+receipt["seconds"];assert receipt["head"]=="70696be105e0c1f8009e2ac0a57236ace99c798d" and receipt["tracked_clean"]
s=(R/'crates/emitter/tests/list_comment_flags_contract.rs').read_text().split('#[test]',1)[0].replace('//!','//')
s+='''
fn main() {
 let parsed=parse_source_file("list-comments.ts","function f() {\\n// head\\n\\n// tail\\n}\\n",Default::default(),None);
 let mut arena=TransformArena::new();let source=arena.add_source(&parsed,None);
 let mut result=transform_nodes(arena,vec![TransformRoot::SourceFile(source)],vec![Box::new(FlagTransformer{flags:EmitFlags::NONE,parent:false,parent_and_body:false,clone_name:false,body_range:Some((false,false))})],false).unwrap();
 let printed=create_printer(PrinterOptions::new(NewLineKind::LineFeed).with_source_file_text_mode(SourceFileTextMode::Canonical)).print(&mut result,PrintRequest::SourceFile(source),None).unwrap();
 print!("{}",printed.text());
}
''';(O/'main.rs').write_text(s)
args=['rustc','--edition=2021',str(O/'main.rs'),'-L','dependency='+str(D),'-o',str(O/'probe')];libs={}
for name in ['tsc_emitter','tsc_syntax']:
 p=max(D.glob('lib'+name+'-*.rlib'),key=lambda p:p.stat().st_mtime_ns)
 dep=D/(p.name.removeprefix('lib').removesuffix('.rlib')+'.d');assert 'crates/'+name.removeprefix('tsc_')+'/src/lib.rs' in dep.read_text();assert started <= p.stat().st_mtime <= finished
 libs[name]={'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()};args+=['--extern',name+'='+str(p)]
report={'scope':'Read-only direct factory/printer probe using existing canonical70696 compiled libraries; no source edits or full-command claim.','libraries':libs,'build_argv':args,'build_receipt_sha256':hashlib.sha256(receipt_path.read_bytes()).hexdigest(),'setup_note':'Initial source-path check refused because Cargo dep-info uses relative paths. Bound rlibs by their mtimes inside the exclusive canonical349 build window, its clean-head receipt, and hashes instead.'}
prefix=['taskpolicy','-b','nice','-n','15']
with (O/'build.log').open('w') as f:report['build_exit']=subprocess.run(prefix+args,cwd=R,stdout=f,stderr=subprocess.STDOUT).returncode
if report['build_exit']==0:
 with (O/'native.log').open('w') as f:report['native_exit']=subprocess.run(prefix+[str(O/'probe')],cwd=R,stdout=f,stderr=subprocess.STDOUT).returncode
 if report['native_exit']==0:
  rows=json.loads((R/'crates/emitter/tests/fixtures/list-comment-flags.json').read_bytes())['cases'];expected=next(r['output'] for r in rows if r['case_id']=='empty-function-body/Synthesized/None/retained');actual=(O/'native.log').read_text();report.update(actual=actual,expected=expected,exact=actual==expected)
(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
