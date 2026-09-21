# H0 検証ソースの入力分類（r615）

正式 walk r613 は 35.32 秒で事前条件により終了 2 となり、生成段へ入らなかった。
新しく変更した H0 検証器の 2 ファイルが H2.5g の変更ファイル分類に未登録だったためである。
この実行から収束証明は発行されていない。元のログ・終了状態を保存した。

実際の Opus 237 と、呼出箇所・既存の分類規則を独立に確認した。
`host_resolution.rs` は semantic history / host-resolution command の検証器であり、
固定の H2.5g acceptance command は呼び出さない。outlined unit tests も同コマンドに
組み込まれない。既存の ratchet・families・CI 証拠生成処理と同じ
`NON_RUNTIME_SHADOW_INPUTS` に 2 ファイルを明示登録する。
全体 CI によるライブラリテスト・semantic history 検証は引き続き必須である。

変更は分類表の 2 エントリと説明コメントだけ。実行入力の配列・順序・920 件の条件、
スキーマ、旧 530 の最終検証 helper 5 ファイルはすべて不変である。
shadow 分類は 80→82 ファイル。生産側の Rust 変更もない。
当初検討した runtime 入力への追加案と新 helper は採用しておらず、
input-identity-plan.json は不採用の提案記録である。検証対象の分類に合う既存の仕組みを使う。

元の static-preconditions checker、pin preflight、Node 構文検査が成功した。
直接の profile --check は runtime closure 条件を通過した後、既存生成物が古いため終了 1
となったことも確認した。この生成物の更新は正式 walk で行う。
他の profile に同じ変更ファイル分類器は見つからなかった。

次の正式 walk 後に、runtime_inputs の全 920 path と順序が元のものと等しいことを確認する。
ケースや観測内容も従来の差分検証で保持を確認する。新しい収束証明、最後に作る台帳、
同一最終候補の full CI 全18 phases と hosted が必要であり、本記録はその代わりにはならない。
