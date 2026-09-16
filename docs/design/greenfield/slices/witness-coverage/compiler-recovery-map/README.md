# OPS-COVER-3H / 3I: literal recovery and map option witnesses

2026-09-16。Codex／統合担当。基点は main `65aa1a159`。
2スライスを同じbranchで実装し、focusedローカル検証後にまとめてhostedへ提出する。
既存テストのCI入口追加であり、runtime admissionや新たな互換修復ではない。

| Slice / suite | 登録する既存入力 | 観測と重複 |
| --- | --- | --- |
| OPS-COVER-3H / `utf16-recovery-corpus` | parser-owned literal recoveryの50 corpus ID、1 exact test | qualified-loaderのEstablished floorによるcommand tupleを2回。JS bytes、callback metadata、診断、emit result、status、exit。parse diagnosticの独立projectionはoracle内だけの観測 |
| OPS-COVER-3I / `map-option-projection` | 31専用入力と原本5 ID、3 exact tests | 専用入力はqualified/recorded両adapterを各2回。原本5はacceptanceと重複。うち3は旧SourceMap floorの差と既存MapFamilyの一致も各2回確認。status/exitはRust testで構成するため実command status/exitの新規実行とは数えない |

Rust test、既存fixture、observer、qualification、productionは変更しない。
recovery oracleが必要とする旧censusは未追跡targetにのみ存在していた。
元のfixtureに固定されたSHA-256と完全一致する580,479 bytesを
`crates/compiler/tests/fixtures/utf16-literal-recovery-census.json`へ保存する。
observer実行中のみ元の相対pathへexclusive作成し、自分が作成したファイルだけ終了時に削除する。
既存ファイルがあればhash一致を確認し、上書き・削除しない。元observerと期待値は不変。
これは当時の選択証拠の保存であり、現在のparser censusを再計測した記録ではない。

旧 `h2_5h_utf16_literal_rows` の4 IDは、既に `utf16-original-commands` で
同じ入力を完全command比較する。旧target自身の追加実行は今回行わず、直接入口なしのまま記録する。
mapのignoredな `existing_witness_route_census` と、importされた
`source_map_band_probe::probe_one_band_row` は過去の調査入口として未選択に保つ。
3 exact testsを選び、これら2 testsをfilterする。
共有qualification・harness・VFS・vendorは全体選択を保持し、専用target/fixture/observerだけを
各suiteに割り当てる。observer失敗、0件、欠落、ignore、filter件数の変化は非0で失敗させる。
`TSRS_MAP_OPTION_CAPTURE`は明示実行時に除去し、余分なcaptureの副作用を避ける。

既存controls job・2 workers・60分上限を使う。前回34分54秒に今回分を加えた実測が45分へ
近づく場合はjob分割を検討する。既存全件をローカルで繰り返さず、最終合成sourceをhostedで検証する。

## 検証

[基点と候補のsource照合](source.v1.json)でRust/test/build768 filesと既存owner入力6 filesの
SHA-256不変を確認した。2 targetのsource変更は基点では全group選択だったが専用target自身は
呼ばれず、候補では各専用suiteをcontrols jobで選ぶ。これは入口のbefore/afterである。

旧literal4 IDは既存complete-command fixtureと順序込みで一致し、recovery50とはIDが重ならない。
recoveryはcompiler4 / conformance46、元のqualified inputを参照するartifactはH2.5g32 /
H2.1a1 / H2.5a1 / H2.5h16。profile間の同一IDでも観測floorを区別する。
mapは31×2 routes＋原本5＋別floor6＝73観測を各2回実行する設計で、うち3は旧floorの
意図された不一致対照。完全一致の新規corpus数としては加算しない。

[ローカル実行記録](local.v1.json)にcommand・実exit・入力とbinaryのSHA・ログを保存した。

- 既存のRust4 testsはすべて成功。recovery50 command comparisonsは各2回、map73観測も各2回。
  mapの旧floor3観測は意図された不一致対照であり、exactへ加算しない。
- 初回runnerは8分40秒。oracleは2回一致、Rustはrecovery48.53秒とmap71.60秒で成功したが、
  runnerのfilter期待件数を1としていたため実際の2件を拒否してexit1。import先のignored probeを
  含めた2件へ訂正した。訂正以外のrunner bytesは初回入力SHAへ再構成して一致を確認した。
- 最終map入口はexit0、89.458秒（oracle16.733秒、Cargo build/replay72.591秒）。
  初回と同じRust binary SHA、同じfixture/observer、3 pass /0 ignored /2 filtered。
  recoveryに影響しない件数訂正のため、成功済みのrecoveryをローカルで再実行していない。
- planner51 tests、実行policy、fmt、台帳v14再生成、diff checkが成功。
  census欠損/衝突、oracle失敗、既存ファイル保全、cleanup、test欠落/0件/ignore/filter変更を検査した。

専用2 suiteと既存coverageをまとめたhosted検証は次節へ記録する。
全legacy CI、未登録target、H2全体のqualificationは今回のローカル実行には含まない。


