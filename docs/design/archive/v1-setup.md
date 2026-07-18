# tsc-rs Phase 1 - 実行環境

この文書は Phase 1 の検証環境に必要な外部要素と生成物をまとめたものです。
現在のリポジトリでは、まず `./scripts/bootstrap.sh` を実行する前提です。

## クイックスタート

```bash
./scripts/bootstrap.sh
./verify.sh quick
./verify.sh golden-check
```

初回は TypeScript corpus の clone、TypeScript oracle の npm install、Rust build、
golden baseline 作成が入るため時間がかかります。

## 必要な外部要素

| 要素 | 推奨バージョン | 用途 |
|---|---:|---|
| Rust | 1.75.0 known-good / stable | `tsrs` のビルド |
| Node.js | 22.x | TypeScript oracle と `diag_oracle.js` の実行 |
| TypeScript | 6.0.3 | `tsc` oracle |
| Python | 3.10+ | classifier と補助スクリプト |
| git | 任意 | TypeScript corpus の取得 |

`scripts/bootstrap.sh` は `TSRS_TYPESCRIPT_VERSION` を見ます。既定値は `6.0.3`
です。

## 静的ファイル

- `src/`: Rust 実装
- `lib/lib.tsrs.d.ts`: checker が使う curated lib
- `difftest/`: `diag_oracle.js`, `golden_classify.py`, corpus lists
- `conf/`: targeted diagnostic run harness
- `verify.sh`: quick / golden-check / crash / full の統合スクリプト
- `docs/phase1-status.md`: Phase 1 の現状レポート

## bootstrap が用意するもの

- `oracle/`: `typescript@6.0.3`
- `ts-tests/`: TypeScript conformance corpus
- `/tmp/chunk{1,2,3,tail}.txt`: `--check-batch` 用 file list
- `/tmp/golden_diag.txt`: 現在の `tsrs` 出力 baseline
- `/tmp/parallel_classify.py`: 並列 classifier
- `/tmp/conf_list.txt`: parse-FP 用 file list
- `/tmp/conf_tsc.txt`: parse-FP 用 TypeScript oracle 出力
- `/tmp/cases.json`, `/tmp/cases2.json`, ..., `/tmp/cases5.json`: targeted diagnostic cases

`oracle/` と `ts-tests/` は大きいため `.gitignore` で除外しています。

## 主要コマンド

```bash
./verify.sh quick
./verify.sh golden-check
./verify.sh crash
./verify.sh full
```

負荷を下げて `golden-check` を走らせる場合:

```bash
TSRS_BATCH_JOBS=1 TSRS_CLASSIFY_JOBS=1 ./verify.sh golden-check
```

少し並列させる場合:

```bash
TSRS_BATCH_JOBS=2 TSRS_CLASSIFY_JOBS=2 ./verify.sh golden-check
```

`TSRS_JOBS` は checker 内部の multi-file worker 数です。`golden-check` の
外側の並列数だけを落とす場合は `TSRS_BATCH_JOBS` を使います。

`verify.sh golden-check` は `scripts/parallel_classify.py` を優先して使います。
この classifier は harness directive (`@filename:`, `@module:`,
`@target:` など) を tsc oracle 側にも反映します。`/tmp/parallel_classify.py`
だけがある場合はそちらに fallback し、どちらも無い場合は legacy の
`difftest/golden_classify.py` を使います。

手動で再分類する場合:

```bash
REL=target/release/tsrs
: > /tmp/golden_now.txt
for c in /tmp/chunk1.txt /tmp/chunk2.txt /tmp/chunk3.txt /tmp/chunk_tail.txt; do
  timeout 300 "$REL" --check-batch "$c" --jobs "${TSRS_BATCH_JOBS:-1}" >> /tmp/golden_now.txt 2>/dev/null
done
TSRS_CLASSIFY_JOBS="${TSRS_CLASSIFY_JOBS:-1}" python3 scripts/parallel_classify.py \
  /tmp/golden_diag.txt /tmp/golden_now.txt \
  lib/lib.tsrs.d.ts
```

macOS などで `timeout` が無い場合は、`gtimeout` を入れるか、時間制限なしで
同等のコマンドを実行します。`verify.sh` と `bootstrap.sh` は `timeout` が無い
場合も継続します。

## 環境変数

- `TSRS_ROOT`: repository root
- `TSRS_WORK`: `Cargo.toml` を含む作業ディレクトリ
- `TSRS_LIB`: 使用する `lib.tsrs.d.ts`
- `TSRS_BIN_RELEASE`: release binary のパス
- `TSRS_ORACLE`: TypeScript oracle directory
- `TSRS_TS_TESTS`: TypeScript corpus directory
- `TSRS_GOLDEN`: golden baseline path
- `TSRS_VIRTUAL_CWD`: diagnostic span の cwd を固定
- `TSRS_JOBS`: checker の parallel worker 数
- `TSRS_BATCH_JOBS`: `verify.sh` の `--check-batch` worker 数。未指定時は `tsrs` の既定値
- `TSRS_CLASSIFY_JOBS`: `scripts/parallel_classify.py` が同時に走らせる `tsc` oracle 数。既定値は `4`
- `TSRS_TSC_TIMEOUT`: classifier の per-file `tsc` timeout 秒数。既定値は `45`

## 最小 smoke test

```bash
W=$(mktemp -d)
cp lib/lib.tsrs.d.ts "$W/"
cat > "$W/main.ts" <<'TS'
class C {
  m() {
    let inner: string;
    class N { method() { console.log(inner); } }
  }
}
TS
TSRS_VIRTUAL_CWD="$W" target/release/tsrs \
  --strict --diag-json "$W/main.ts" |
  python3 -c "import json,sys; d=json.load(sys.stdin); print([x['code'] for x in d['diagnostics'] if x.get('file','').endswith('main.ts')])"
```

期待値は `[2454]` です。Phase 1 bug 5 の nested-class use-before-declaration
確認に使います。

## 注意点

- `/tmp/golden_diag.txt` は TypeScript oracle ではなく、現在の `tsrs` 出力の
  snapshot です。
- `--check-batch` は harness directive (`@filename:`, `@module:` など) を
  認識します。`scripts/parallel_classify.py` も同じ fixture を tsc oracle 用に
  展開します。比較対象は従来どおり `main.ts` / `main.tsx` の診断です。
- `verify.sh quick` の parse-FP は `/tmp/conf_list.txt` と `/tmp/conf_tsc.txt`
  が必要です。`scripts/bootstrap.sh` が両方を生成します。
- exact historical zip layout (`/home/claude/work`) を再現したい場合は、
  `TSRS_ROOT=/home/claude TSRS_WORK=/home/claude/work` のように明示します。
