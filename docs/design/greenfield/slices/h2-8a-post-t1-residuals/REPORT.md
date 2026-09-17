# H2.8a-A-RES-POST-T1 — 結果報告（before / after、新規対照、retire、未解決、未実行）

作成日：2026-09-17。設計は [DESIGN.md](DESIGN.md)。依頼書は [../h2-8a-post-t1-residuals-claude-handoff.md](../h2-8a-post-t1-residuals-claude-handoff.md)。
記録は [records/](records/)。全件 replay / hosted / PR / admission は統合担当（§6）。

## 1. 開始点と候補

| 項目 | 値 |
| --- | --- |
| 開始 SHA（origin/main、PR #549 / #550 を含む） | `eb6dc2c7872b18442657f8eefde5efc9e8fb4cf7`（[records/before/start-state.txt](records/before/start-state.txt)） |
| worktree / branch | `~/dev/tsc-rs-post-t1-residuals` / `draft/h2-8a-post-t1-residuals` |
| toolchain | rustc 1.93.0、cargo 1.93.0、Node v25.2.1（`.node-version` と一致） |
| vendor | `_tsc.js` sha256 `1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3`、`typescript.js` `569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39` |
| 候補 | branch `draft/h2-8a-post-t1-residuals` の原因別 commit 列（§5）。patch は `git format-patch --binary` で生成、sha256 は §5 |

（以下、各節は測定後に確定する。）
