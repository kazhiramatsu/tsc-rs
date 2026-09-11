# H2.8b-LR2 — library実ファイル順位の引継ぎ

2026-09-11。LR1の実測を受けた原因別引継ぎ。**production未変更、runtime-ready未宣言**。
[LR1報告](h2-8b-library-replacement-report.md)と
[失敗baseline](../../../../ratchets/h2-8b-library-replacement-baseline.v1.json)が開始証拠である。

## 固定した不一致

実測HEAD `f01ab82293dd4b7c33fa696bc8f2a6ad192282aa`。
通常command 12/12 ×2は一致するが、replacementを選ぶ7件のsource/library順序が一致しない。
5件のfallback/disabled/noLib controlsは順序も×2一致する。
原因は論理lib名の順位が解決後の実ファイルへ引き継がれることである。

上流authorityは`_tsc.js`の以下の入口。SHAはLR1開始registryで固定済み。

- `createProgram` 122874行：`toSorted(processingDefaultLibFiles, compareDefaultLibFiles)`。
- `compareDefaultLibFiles` 123121–123123行。
- `getDefaultLibFilePriority` 123124–123138行：defaultLibraryPathをcase-sensitiveにcontains判定し、
  その中にある既知basenameならcatalog順位、それ以外は`libs.length + 2`。

## 修正方向と共有範囲

主なownerは`crates/program/src/loader.rs`と`library.rs`。

1. `load_selected_libraries`と`process_lib_references`の両producerで、論理要求名と
   解決されたsourceの順位を区別する。実ファイルのdisplay pathと正規化済みlibrary directoryに
   基づいて同じ規則で順位を算出するcalleeを設計する。
2. `LibraryCatalog::file_name_priority`の論理basename計算は実ファイルがcatalog内にある場合の
   部品として使える。現在の「physical fileから独立」という説明は上流の全契約ではない。
3. `visit_source`の新規登録と`observe_existing_source`のpromotion/再参照が、同じphysical sourceに
   一貫した順位を保存することを確認する。後者は現在`existing.min(priority)`で統合する。
4. `finish`のstable sort、library membership、root/source ID再対応は維持する。
   replacementかどうかだけで一律に末尾へ送る修正にはしない。catalog内に解決されるケースでは
   上流が実際のbasenameを使うためである。

emitter/printer/metadata/generated bindingと共有comparatorはこの原因の変更候補に含めない。
既存fixture・失敗受領証・LR1開始registryは保存する。新しい修正受領証に修正後HEADを記す。

## runtime設計を確定する前に埋める項目

- 既存のpath正規化・directory包含のRust calleeを確認し、sourceのcase-sensitive containsと一致する型/関数を固定する。
- 2個以上のreplacementが同順位になる場合のstable orderと、root promotion/重複参照を組み合わせた上流対照を追加する。
- catalog外にあるcatalog風basename、およびcatalog内へ解決される場合の到達性を確認して観測する。
  default-lib host hookを必要とする場合はHOST1/API1と分離する。
- 現行library loader契約testsと通常emitの影響範囲を固定し、A-close依存とBのactivation境界を再確認する。

この引継ぎだけで全ての未観測分岐を実装済み扱いしない。元の24分割計画のLR2 outlineを
自動でruntime-readyへ昇格させるものでもない。

## 修正後の受入

元の12件を変更せず、通常command全タプルとsource/library/root順を各×2一致、通常test exit0にする。
7件の失敗をcase除外やsort済み比較へ変えない。5件の既存positiveとconfig-directory-anchorの
診断6059/5011・command exit2を維持する。追加対照は別IDで上流観測してから比較する。
productionを変更したLR2では既存library loader契約と影響するcommand群を検証し、統合時に
既存hosted acceptanceを確認する。LR1の完全command一致をLR2の回帰検査の代替にはしない。
