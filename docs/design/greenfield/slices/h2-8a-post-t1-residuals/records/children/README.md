# 子スライスごとの編集ループ記録（oracle 無し、`TSC_RS_POST_T1_RESIDUALS_CASE_SET` 選択）

すべて `CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test --manifest-path crates/compiler/Cargo.toml --test <target> <exact test names> -- --exact --nocapture --test-threads=1`。
observer（oracle）は実行していない（開始点と最終候補は `records/before/`、`records/final/` の正規 runner）。

| file | 段階 | 選択 / 内容 | 結果 |
| --- | --- | --- | --- |
| `r9-test.log.gz` | R9 修復後 | `/r9/`（21） | 20 exact / 1 failed（`define-system-static-field-two-files` は R12 の map 差のみ） |
| `r12-test.log.gz` | R12 修復後 | `/r12/,/r9/`（39） | 39 exact / 0 failed |
| `chain-r12.txt`, `r12-pipeline-targets.log.gz`, `r12-t1.log.gz` | R12 commit + 6 行 retire 後 | pipeline 4 対象（oracle 無し）、T1 18 | pipeline 4 exact / 0 known；T1 17 exact / 1 known（receiver）、packet 12 / 3 |
| `receiver-test.log.gz` | RECEIVER-MAP 修復後 | `/receiver-map/,/private-set-comments/`（35） | 35 exact / 0 failed |
| `chain-receiver.txt`, `receiver-t1.log.gz` | RECEIVER commit + T1 receiver 行 retire 後 | T1 18 | 18 exact / 0 known、packet 12 / 3 |
| `private-set-experiment-test.log.gz`, `private-set-experiment-t1.log.gz` | `create_private_set` の `NO_TRAILING_COMMENTS` を外した実験（採用） | `/private-set-comments/,/receiver-map/`（35）+ packet、T1 | 35 exact / 0 failed、packet 33 / 33；T1 18 exact、packet 13 exact / 2 known |
| `decorator-test.log.gz`, `decorator-after-transform-analyze.txt.gz` | DECORATOR：NoComments + fresh target + printer decorator phase | `/decorator-comments/`（27）+ packet | 20 exact / 7 failed、packet 13 / 13 failed（member 名の NoLeadingComments 未実装、`set-line-between` の `obj.m`） |
| `decorator2-test.log.gz`, `decorator2-t1.log.gz` | + member 名 NoLeadingComments（gate 無し） | 同上、T1 | 22 exact / 5 failed、packet 26 / 26；T1 packet 14 / 1 failed（undecorated class の名前に Rust だけ 1024） |
| `decorator3-test.log.gz`, `decorator3-t1.log.gz` | + classInfo gate（採用） | `/decorator-comments/,/receiver-map/,/private-set-comments/`（62）+ packet、T1 | 57 exact / 5 failed（printer 所有の 5 行、known-native に凍結）、packet 59 / 59；T1 18 exact、packet 15 / 15 |

