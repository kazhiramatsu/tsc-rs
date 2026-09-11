# CFG1e config reuse / host boundary

CFG1の残存監査。pinned _tsc.js:39460–39500のgetExtendedConfigはoptional cacheを受ける。
通常CLI自身も:132652でMapを作成してparserに渡す。したがってcache有りをwatch/buildだけの
対象外として扱わない。既存のfresh APIを残して明示cache APIとCLI接続を追加する。

26ケース、60 parse試行を上流側で先に固定した。fresh/cacheのduplicate・diamond・変換失敗・
構文失敗・case alias・read absent/throw、呼出間のfile変更とcache clear、cycle、missing、
fileExists/readDirectory fault、Windows/UNC/URL、package extendsを比較する。
config projectionだけでなく、host callbackの順序・引数も一致させる。

cacheは変換済みnodeとsourceを再利用する。変換エラーはhit時に再報告されず、parse/readの
エラーはhitごとに再報告される。keyはhostのcase sensitivityに従う。cache clearは呼出元が
明示的に行い、filesystem変更を推測して無効化しない。新しいfresh呼出へ状態を漏らさない。

observer事前確認でcacheを9番目に渡してしまい、hitが起きていないことをhost call件数から
検出した。8番目へ訂正し、未到達だったdirectory faultと合法trailing commaの入力も訂正。
初期の未登録試行3ファイルはrun dir/reuse-observer-preflightに保存した。登録する観測は
正しいsignatureでfresh/cached差とfault到達を確認済み。native結果に合わせた変更ではない。

530794920のnative baselineは18/26一致×2。既存APIにはcache引数が無いため、要求された
cached caseも全件uncachedとして実行した。8不一致はcache hit、変換診断の重複、case aliasの
source表記、呼出間変更の保持であり、fresh/host-fault/path/package対照はすべて一致した。
基準ログと各actualをrun dirのreuse-baselineへ保存した。

最初のcache実装ee849e8c9の全契約実行は既存64段extends制限testでstack overflowとなり、
通常test完走に数えない。CachedExtendedConfig.nodeをBoxに移し再帰frameを小さくした。
cc74ba59cの同depth testは実exit0。制限64・parser nesting256・既存test入力は変えていない。

追加の実filesystem CLI対照6件はroot設定、変換失敗、同じbaseの変換/parse失敗、取得設定の
非継承、watch継承。各2回、vendored _tsc.jsを先に走らせ、同じinputからproduction binaryの
stdout/stderr/exit・全output path/bytesを比較する。両実行後にinput不変も検査する。

実CLI baselineは既存17通過/新規1失敗/既存ignore10。root-settingsの2回は一致したが、
invalid-watchで停止したため後続4caseは未実行。nativeはexit2/stderrにbuild prepared program
のlocated diagnostic has no owned source textと返した。診断のtsconfig.jsonという相対aliasが
absolute auxiliary identityに対応付かなかったためである。ProgramConfigFileの明示aliasを
読む際にcanonical auxiliary sourceとsnapshot textの一致を検証する。source無し/別textの
negative controlsも追加し、builderの所有権検証を保持する。
