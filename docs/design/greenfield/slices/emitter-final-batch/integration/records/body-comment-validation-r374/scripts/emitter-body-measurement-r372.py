from pathlib import Path
import json,subprocess,os,time,gzip,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');O=T/'emitter-body-measurement-r372'
assert json.loads((T/'emitter-body-tests-followup-r370/manifest.json').read_text())['qualified']
review=json.loads(Path('/tmp/emitter-claude-review-round187-opus.json').read_text());assert review.get('result')
O.mkdir(exist_ok=False);p=R/'crates/emitter/tests/printer_failure_contract.rs';original=p.read_bytes();s=original.decode()
a='''    let text = printed.text();
    serde_json::json!({'''
b='''    let text = printed.text();
    // The TypeScript observer measures the returned string with
    // computeLineStarts. A writer's next column includes pending indentation
    // at a line start, especially after a retained failure; it is not the
    // physical end column of the returned string. Keep that writer state out
    // of this string-only observation and preserve the raw UTF16 value.
    let index = tsc_diagnostics::compute_line_map(text);
    let utf16_len = index.utf16_len();
    let end = index
        .line_and_character_utf16(utf16_len)
        .expect("end of printed text");
    assert_eq!(utf16_len as usize, printed.text_utf16().len());
    assert_eq!(printed.end().position().value(), utf16_len);
    serde_json::json!({'''
assert s.count(a)==1;s=s.replace(a,b)
a='''"end_utf16": {"position": printed.end().position().value(), "line": printed.end().line(), "column": printed.end().column()}''';b='''"end_utf16": {"position": utf16_len, "line": end.line, "column": end.character}''';assert s.count(a)==1;s=s.replace(a,b)
a='''            Ok(printed) => {
                let mut value = measure(&printed, index);'''
b='''            Ok(printed) => {
                if case["case_id"] == "printer-failure/printNode/review/function-body-sticky-comments"
                    && index == 4
                {
                    // The callback fault retains one indentation level. This
                    // writer coordinate is independent of the string's final
                    // empty line, whose measured column is zero.
                    assert_eq!(printed.end().column(), 4);
                }
                let mut value = measure(&printed, index);'''
assert s.count(a)==1;s=s.replace(a,b);p.write_text(s)
h=lambda b:hashlib.sha256(b).hexdigest();report={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'qualified':False,'scope':'TESTADAPTERONLY: end_utf16 measured from returnedrawUTF16 string exactly asTSobserver computeLineStarts, notwriterpendingindent. Fulltext/events/error/state comparisons unchanged. Original21frozenrows unchanged; new22ndcase includes successx2thenfault/sharedrecoveryx2. No productionwriter/PrintedText/map change.','test_before_sha256':h(original),'test_after_sha256':h(p.read_bytes()),'steps':[]}
def save():(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
env=os.environ.copy();env.update(CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR=str(T),TSC_RS_PRINTER_FAILURE_REVIEW_ACTUAL=str(O/'failure-review-actual.json'))
def run(label,args):
 log=O/(label+'.log');start=time.monotonic()
 with log.open('xb') as f:c=subprocess.run(args,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
 b=log.read_bytes();(O/(label+'.log.gz')).write_bytes(gzip.compress(b,mtime=0));row={'label':label,'argv':args,'exit':c,'seconds':time.monotonic()-start,'log_sha256':h(b)};report['steps'].append(row);save();print(json.dumps(row),flush=True);return c
save();codes=[run('fmt',['cargo','fmt','--all'])];assert not codes[-1];codes.append(run('failure',['taskpolicy','-b','nice','-n','15','cargo','test','--offline','-p','tsc-rs-emitter','--test','printer_failure_contract','--','--nocapture','--test-threads=1']))
report['qualified']=not any(codes);diff=subprocess.check_output(['git','diff','HEAD'],cwd=R);(O/'private.diff.gz').write_bytes(gzip.compress(diff,mtime=0));report['diff_sha256']=h(diff);save();raise SystemExit(max(codes))
