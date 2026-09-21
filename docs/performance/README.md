# tsc-rs / tsgo 性能比較の準備

2026-09-21。測定器・入力生成・比較条件を準備した段階。実際の Rust/Go
コンパイラのビルド、速度測定、最適化はまだ実行していない。
emitter 統合の基準は PR #561 の main `8e63b77a676311fe660a32a3ce18227e8c0f2ad5`。
性能測定は既存の H1 ratchet や参照ハッシュ chain から独立した調査として行う。

## 比較条件

本番 CLI の release binary と、固定した Go source の通常の最適化ビルドを使う。
`cargo test`、debug build、ビルド時間はコンパイル性能に含めない。
Node 版 TypeScript 6.0.3 は結果が食い違った場合の意味論の対照に使う。

| 比較軸 | 最初の測定 |
| --- | --- |
| 起動・ライブラリ準備 | 1 source の `startup` |
| ファイル数・import 解決 | `modules-64` / `modules-512`（共通 module と入口を別に含む） |
| 型計算 | mapped type / intersection を使う `types` |
| 出力 | `check`、JS の `emit`、JS・宣言・両 map の `full-emit` |
| tsgo 並列処理 | 既定を主結果とし、下表の制限実験を別々に実行 |

生成入力は合成ベンチマークであり、実アプリケーションの代表値ではない。
共通の `target: ES2022`、`module: ESNext`、`moduleResolution: Bundler`、
`strict: true`、`lib: ["es2022"]`、`types: []`、`skipLibCheck: true` を指定する。
一般的な既定設定やライブラリ自体の型検査の性能はこの集合から主張しない。

Rust は 6.0.3、固定 native source は 7.1 系であり、付属 lib の bytes と仕様は異なる。
同じ source/options に対する製品比較として記録し、同一の lib 作業量とは呼ばない。
両者とも成功し、生成ファイルの path・bytes が一致する場合だけ比率を出す。
入力の互換性は実際の事前実行で確認する。今回の準備段階では未確認。

| `--tsgo-mode` | 実際の制御 |
| --- | --- |
| `default` | `GOMAXPROCS` 等の引き継ぎを除き、native の既定を使う |
| `gomaxprocs-1` | Go のスケジューラを `GOMAXPROCS=1` に制限。checker 数は既定のまま |
| `checkers-1` | `--checkers 1`。他の並列段階と CPU 数は制限しない |
| `single-threaded` | `--singleThreaded`。compiler の work group と checker を直列化する設定 |

固定 source の checker は既定 4 個で、ファイル数に応じて制限される。
`singleThreaded` も Go runtime 全体の OS thread 数や CPU affinity を固定する意味ではない。
[実装](https://github.com/microsoft/TypeScript/blob/1f70213d4922b434345f639b441681e470c7cfc1/tsc/internal/compiler/checkerpool.go#L306)と
[CLI 宣言](https://github.com/microsoft/TypeScript/blob/1f70213d4922b434345f639b441681e470c7cfc1/tsc/internal/tsoptions/declscompiler.go#L233)を確認した。
既存 `scripts/typescript7.py` は調査用に `GOMAXPROCS=2` を設定するため、測定入口には使わない。

## 用意した入口

[compiler_bench.py](../../scripts/perf/compiler_bench.py) は標準ライブラリのみを使う
Python 3.12 以降の macOS 用（Linux は 3.11 以降）ツール。`prepare` は新しい入力を生成するだけで、
compiler・ダウンロード・ビルドを実行しない。既存の保存先は上書きしない。
以下のコマンドは、この準備ブランチの repository root で実行する。

```sh
python3 scripts/perf/compiler_bench.py prepare target/compiler-bench-20260921/suite
```

後でビルドする場合の macOS 上の例。実行前に Rust source が上記 merge の実装を持つことと、
native checkout の HEAD が [references.json](references.json) の pin に一致することを確認する。
ビルドの exit とログ、toolchain version、features・flags を保存する。
`RUSTFLAGS` / Cargo profile override 等がある場合は暗黙に流用せず、条件として確定する。

```sh
set -eu
BENCH_DIR="$PWD/target/compiler-bench-20260921"
RUST_SOURCE="$PWD"
TSGO_SOURCE="/Users/hiramatsu/dev/tsc-rs/target/typescript7/upstream"
mkdir -p "$BENCH_DIR/bin"
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$BENCH_DIR/rust-target" \
  taskpolicy -b nice -n 15 cargo build --locked --release -p tsc-rs-compiler --bin tsc-rs \
  > "$BENCH_DIR/rust-build.log" 2>&1
(
  cd "$TSGO_SOURCE"
  GOTOOLCHAIN=go1.26.0 GOWORK="$TSGO_SOURCE/go.work" \
    taskpolicy -b nice -n 15 go build -p 2 -o "$BENCH_DIR/bin/tsgo" ./tsc/cmd/tsc \
    > "$BENCH_DIR/tsgo-build.log" 2>&1
)
rustc -Vv > "$BENCH_DIR/rustc-version.txt"
go version -m "$BENCH_DIR/bin/tsgo" > "$BENCH_DIR/tsgo-build-info.txt"
```

Go の固定参照では entrypoint のディレクトリ名は `tsc/cmd/tsc`。
比較時の名前を `tsgo` にしている。標準 lib を埋め込む通常の build を使い、
`noembed` や profiler の flags を無断で加えない。ビルド中の時間は測定しない。

測定はビルド完了後、通常優先度の別コマンドとして実行する。
まず小さい集合を確認し、次に `--cases` を省いて全 4 scenarios へ広げる。
出力先は毎回新しくする。

```sh
python3 scripts/perf/compiler_bench.py run \
  --suite "$BENCH_DIR/suite/suite.json" \
  --rust "$BENCH_DIR/rust-target/release/tsc-rs" \
  --tsgo "$BENCH_DIR/bin/tsgo" \
  --rust-source "$RUST_SOURCE" --tsgo-source "$TSGO_SOURCE" \
  --output "$BENCH_DIR/default-first" \
  --cases startup,modules-64 --modes check,emit,full-emit
```

制限実験では新しい `--output` と `--tsgo-mode` を指定する。
測定は直列実行し、別のビルド・測定・高負荷処理を重ねない。
nice 値と Darwin の process background policy を確認し、背景実行を拒否する。
実機では `taskpolicy -b` が nice 0 のまま background 1 を返すことを確認した。
Python の [Darwin priority API](https://docs.python.org/3/library/os.html#os.PRIO_DARWIN_PROCESS) を使う。
温度、電源モード、他の QoS 制約やプロセスの干渉まで保証するものではない。
同じ電源条件・負荷条件で実行記録を残す。

## 結果の検査と読み方

- fresh process を毎回起動し、filesystem は warm の条件で測る。
  初回 1 回は warmup として除外し、8 回ずつ AB/BA の順序を均等にして比較する。
  cold-cache や増分コンパイルの結果ではない。
- wall time はプロセス起動と待機を含む。user/system CPU と peak RSS は
  `wait4` で対象 child ごとに採り、macOS/Linux の RSS 単位を bytes へ揃える。
- 起動前に TS2322 の型エラー対照を確認する。正常集合では exit 0、診断なし、
  noEmit の書込み 0、または全出力 path/bytes 一致を要求する。
- 入力・binary の変更、欠落出力、差分、診断、異常終了、timeout があれば
  `status: invalid` とし、比率を出さない。生の exit・標準出力・標準エラー・出力一覧は保存する。
- `results.json` に実コマンド、version、binary digest、申告 source checkout の commit と
  untracked を含む変更状態、入力、機種、CPU、メモリ、負荷、全試行を残す。
  GC・アロケータ・profiler の既知の環境変数を除き、除外したキーと上書き値を記録する。
  source commit と binary のビルド対応は
  この情報だけでは証明できないため、上記の build logs と flags を併せて保管する。
- median、min/max、median absolute deviation を見る。8 試行から安定した p95 は主張しない。
  `rust_over_tsgo_wall_ratio > 1` は、その条件で Rust の median が遅い意味。
  CPU 時間・RSS の比率も併記する。別 run でも再現するか確認してから最適化判断に使う。

異なる版の lib やオプションに起因する失敗も比較範囲の結果として残す。
`noLib`、診断の無視、出力比較の省略で無理に速度表へ入れない。

## 実プロジェクトへの拡張

[公式ベンチマーク](https://github.com/microsoft/typescript-benchmarking/tree/ab43240fa9389fc66827502a0532591ac9502c06)
の XState / VS Code scenarios を参照し、2026-09-21 に source と lockfile の参照を固定した。
設定・pin は [references.json](references.json) にある。まだ clone、依存 install、
Rust/native の互換性確認、測定は行っていない。

| 候補 | 最初の対象 | 準備時に確認した制約 |
| --- | --- | --- |
| XState | root `tsconfig.json` の noEmit | pnpm の pin と lockfile あり。NodeNext、ES2024/DOM、vitest types、TS extension imports を使う |
| VS Code | `src/tsconfig.json` の noEmit | npm lockfile あり。extends と ambient types の解決が必要。これは VS Code 全体の build 時間ではない |

公式 setup は外部 repo の先端を clone し、該当 scenarios は Linux を対象とする。
そのまま実行せず、上記 commit を checkout して lockfile 固定の依存環境を用意する。
macOS 向けの差分、解決した全 source/lib/dependency と有効な設定を記録してから、
生成入力とは別の実プロジェクト測定を用意する。setup/install 時間は compiler 時間に含めない。

## 測定後のチューニング順序

1. 起動・読み込み、parse/bind/check、transform/print/map のどこに時間とメモリが
   使われているかを profiler で確認する。check と emit の時間差だけで段階を断定しない。
2. Rust は macOS の `sample` または Time Profiler、native は固定 CLI の `--pprofDir`
   を使う。2026-09-21 のこの環境では `xctrace` が未導入だった。
   profiling は速度測定と別に行い、計測用 flags を基準 binary に混ぜない。
3. 実測した重複計算、lookup、割当て・コピーなど、狭い範囲の修正から試す。
   同じ source と出力で before/after を比較し、ばらつきと RSS の悪化も確認する。
4. 共通処理・cache・並列化・AST/CST の所有関係へ影響する案は、Claude と独立に
   検討して影響範囲を照合する。checker・emitter・両 source map・コメントの回帰を
   対象にする。今回の準備による本体の変更はない。

測定器の確認は `python3 -m unittest discover -s scripts/perf -p 'test_*.py' -v`。
偽 compiler と短い child process を使う 8 tests（複数の失敗対照、RSS の独立性、
背景実行の拒否を含む）が、この macOS 環境で成功した。
Opus 239 のレビューを反映し、Darwin の戻り値は実機で照合した。
これは測定器の動作確認であり、tsc-rs/tsgo の性能結果ではない。
