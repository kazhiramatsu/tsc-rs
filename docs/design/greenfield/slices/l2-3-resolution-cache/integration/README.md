# C05 再提出の統合レビュー

2026-09-16。統合先 base `5903caabbdaf0a2653bb0631dd910ce9fb5670d5`。
Claude の再提出 base は `d9cfb664a`、patch SHA-256 は
`fa2cf8d3aab1bf00735320aa098dc5f8bb00fb5ec51e91251d8750e980bbc3b1`。
[提出 patch](../validation/candidate.patch.gz) と `validation/` は提出時の原本を保存した。
本書と `integration/` が統合側の追加修正・実測を記録する。

## レビュー結果と追加修正

R1〜R3の元の再現テスト3件は成功したが、追加監査でpath表記の衝突も発見し、下記のとおり修正した。R2だけ削除された entry accessor を
`held.view().last_used(original.key())` に置換し、同じ不変条件を確認した。
[再現 source](review-02-repro.rs) と [修正前ログ](review-02-before.log.gz) を保持する。
元の22 familyの native expected は再提出でも構造的に同一。新しい2 familyだけが追加された。

R4 の件数上限は直ったが、統合時に次の2点が残っていた。

| 再現 | 提出 bytes での結果 | 統合側の修正 |
| --- | --- | --- |
| option text 65536 bytes、`max_bytes=4096` | 1 entryを公開、報告174 bytes | keyの推定サイズに option identity全体を含める。共有identityでもkeyごとに計上し、過小集計を避ける |
| 履歴1件の後 `max_eviction_history=0`、新規requestなしでpublish | 履歴1件が残る | publish / evict_allの双方で、victimがなくても変更された履歴上限を適用 |

`max_bytes` は公開entriesと退避履歴のそれぞれに適用する推定payload上限。
履歴は件数・bytesの両方を満たすまで古いkeyを忘れる。
intern slotは`Weak`に変更し、過大・破棄されたidentityを単独では保持しない。
`interned_identity_bytes` は最新の生存identityの別名で、所有keyの計上に含まれる。
これはallocatorの実使用量やRSSではない。外部reader / candidateの寿命は別管理。

2件の回帰を既存contractへ追加。修正後は過大entryと履歴がともに0、intern slotも0になる。
この段階で元の公開API再現3件、program単体56件（cache単体8件を含む）、contract9件が成功。

## path表記の追加監査

最初の統合候補 `1e2668a36` で追加2件が失敗した。
[再現source](path-spelling-repro.rs)と[修正前ログ](path-spelling-before.log.gz)を保存した。

- case-insensitive hostでtypeRootsを `/p/types` から `/p/TYPES` へ変えると、cacheは古い小文字パス、freshは大文字パスを返した。
- 同じcandidateで参照元directoryを `/p` と `/P` に変えても同様の差が発生した。

`OptionsIdentity` は `ProgramPath` のdisplayとcanonicalを両方保持する。
対象はtypeRoots / rootDirs / configFilePath。RequestKeyもcanonical directoryに加えて
正規化された元のdirectory表記を保持する。依存の照合は引き続きcanonical pathを用いる。
new `identity/path-spelling/option-inputs` と `.../containing-and-config` の2 familyを追加し、
変更・復帰・同一candidate内の各パスをfresh Rustとnativeの両方で比較した。
直前24 familyのexpectedは変更していない。

最初のhosted runsは追加監査の失敗を受けて停止した。
[superseded-hosted.v1.json](superseded-hosted.v1.json)は中断時の記録で、最終成功証跡に含めない。

## 最終sourceの検証

| 検証 | 結果 / 証跡 |
| --- | --- |
| Program単体＋cache contract | 56 / 11 pass、[final-focused.log.gz](final-focused.log.gz)。元R1–R3、追加R4の2件、path表記2件を含む |
| 専用CI入口 `python3 scripts/witness.py resolution-cache --all` | nativeを2回採取してbyte照合、unit56 / contract11 pass。[final-entry.log.gz](final-entry.log.gz) |
| fresh Rust / native TypeScript / 未被覆依存 | 178 / 190 / 0、[summary.json](summary.json) |
| 1000世代seeded soak | 9408 parity、deterministic、max公開11475推定bytes。[soak.json](soak.json) |
| 2000世代identity/key churn | 履歴最大32件 / 25952推定bytes、live最大6、evict_all拒否11/42。[churn.json](churn.json) |
| 既存Program contracts | 481 pass / 5 ignored、[program-contracts.log.gz](program-contracts.log.gz)。loader抽出の検証後、変更は隔離cacheと専用test/fixtureのみ |
| planner / policy | 38 / 10 pass、[final-planner.log.gz](final-planner.log.gz)、[final-policy.log.gz](final-policy.log.gz) |
| clippy（program libと新contract、通常warnings） | exit0、既存lib144 warnings、新規2 Rust fileのwarningなし。[final-clippy.log.gz](final-clippy.log.gz)。`-D warnings`成功とは報告しない |

提出時の履歴1472 bytesはidentityを含まない旧集計。統合後はdirectory表記も含め25952 bytes。
メモリ増加の測定ではなく計上範囲の訂正。soakの比較・reuse・拒否件数は同一。
初回9-test段階のlogs / `local.v1.json` は経過として保持し、最終sourceは `local.v2.json` に固定する。

## Hosted入口

controls jobの `resolution-cache` が、固定Node25.2.1でobserverの`--check`を実行し、
続いて `cargo test --manifest-path crates/program/Cargo.toml --lib --test resolution_cache_contract -- --nocapture --test-threads=1` を実行する。
unit56 / contract11、ignored0 / filtered0を要求し、欠落・0件・部分実行を拒否。
manifest26 family /112世代 /197 requestも入口で検査する。
専用test・fixture・observer変更はこのsuiteだけを選択。
cache本体を含むProgram production変更は従来の全関連replayと本suiteを選択する。
`cargo xtask acceptance`へ混在させず、専用witnessとして実行する。

Hosted結果とmerge identityは、実行完了後に追記する。
Program reuse、watch/LSP activation、実FS I/O / heap測定は本統合の対象外。
