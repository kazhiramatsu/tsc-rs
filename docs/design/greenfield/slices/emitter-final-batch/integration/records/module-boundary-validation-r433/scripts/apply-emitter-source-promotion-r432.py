from pathlib import Path
import json,hashlib,subprocess,time
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';P=Path('/tmp/emitter-source-promotion-r429');O=Path('/tmp/emitter-source-promotion-apply-r432');O.mkdir(exist_ok=False);h=lambda b:hashlib.sha256(b).hexdigest();m=json.loads((P/'manifest.json').read_bytes());assert json.loads(Path('/tmp/emitter-decorator-provenance-r428/manifest.json').read_bytes())['full_tree_equal'];assert json.loads(Path('/tmp/emitter-utf16-retirement-resume-r430/manifest.json').read_bytes())['type_script_observations_unchanged'];assert 'Remaining blocker: none' in json.loads(Path('/tmp/emitter-source-promotion-review-r431/response.json').read_bytes())['result']
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip();assert head==m['head'];original=h((R/'ratchets/h2-1a-qualification.v1.json').read_bytes())
for row in m['files']:
 p=R/row['path'];assert h(p.read_bytes())==row['before'];data=(P/row['path']).read_bytes();assert h(data)==row['after'];p.write_bytes(data)
p=R/'crates/xtask/tests/unit/h2_1a_acceptance/tests.rs';s=p.read_text();needle='    assert_eq!(super::CURRENT_EXACT_SOURCE_PROMOTIONS.len(), 8);';assert s.count(needle)==1;s=s.replace(needle,needle+'''
    assert_eq!(
        super::CURRENT_EXACT_SOURCE_PROMOTIONS
            .iter()
            .filter(|promotion| !promotion.expected_extra_activity.is_empty())
            .count(),
        1,
    );''');p.write_text(s)
p=B/'cross-review/post-integration-dependency-boundaries.md';s=p.read_text();addition=Path('/tmp/emitter-comment-range-migration-note-r397.md').read_text();assert 'r349–r395 namespace and factory controls' not in s;p.write_text(s+'\n'+addition)
p=B/'README.md';s=p.read_text();needle='## 統合後の関数本体コメント検証（r349–r395）';assert s.count(needle)==1
s=s.replace(needle,'''## canonical 再検証と残る境界修復（r396–r432）

[固定候補 r396 の実行](records/canonical-focused-r398/manifest.json)では、emitter 全1,020 tests、
helper551・MetaProperty228・export名80・context788・空ブロック72件の各2回比較、
構文回復2 tests、関連xtask3 bands、workspace全targetsのClippyが成功した。
全体は20段階中5段階が失敗したため未qualified。M4の古い関数名assertion、
同一のimportコメント重複を検出したtoken/ellipsisの2段階、UTF-16の古い拒否1件、
H2.1aの古い拒否1件を区別して記録した。

[境界の追加観測](records/module-boundary-review-r419/manifest.json)に基づき、
importのmodifier末尾とkeyword先頭のコメント所有、side-effect import/star/namespace exportの
現在のmodifier保持、exportのkeyword位置、export assignmentのmodifier省略を限定修復した。
TypeScript観測は既存165件を保持して80件追加し、計245件。共有parser/checker/map処理は変更していない。
actual Opus191/192の議論で棄却した仮説も記録に残す。

UTF-16 keyword-escapeは元の完全観測を各2回比較して一致したため、拒否期待値だけを更新した。
残る8拒否境界は維持する。H2.1a decoratorOnUsingも元の出力・診断・結果tupleが各2回一致し、
追加で構文木25 nodesの種類・位置・親をTypeScriptと照合した。両方のdecoratorが変数文に属し、
class-fields経路のH2.4b活動1回が正しい。actual Opus193/194と確認し、現在の昇格記録に
その正確な回数を持たせた。旧qualificationは変更せず、他の活動は引き続き0を要求する。
修正後の固定差分に対する再検証、元corpus再実行、walk、最終unsplit CIは継続中。

'''+needle,1);p.write_text(s)
label='source-promotion-format-r432';t=time.monotonic();code=subprocess.run(['python3',str(B/'run-local.py'),label,'cargo','fmt','--all'],cwd=R).returncode
assert h((R/'ratchets/h2-1a-qualification.v1.json').read_bytes())==original
report={'head':head,'historical_qualification_sha256':original,'historical_qualification_unchanged':True,'review':'actual Opus194; added its suggested guard exactly one promotion with extra activity','source_qualified':False,'format_exit':code,'seconds':time.monotonic()-t,'files':[{'path':row['path'],'before':row['before'],'after':h((R/row['path']).read_bytes())} for row in m['files']],'candidate_diff_sha256':h(subprocess.check_output(['git','diff','HEAD'],cwd=R))};(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report));assert code==0
