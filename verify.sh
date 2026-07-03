#!/usr/bin/env bash
# tsrs refactor safety net. Usage:
#   ./verify.sh quick        build + corpus + cases + unit + parse-FP   (~fast, run every edit)
#   ./verify.sh golden-save  snapshot full 5907 --diag-json to golden    (run before a refactor)
#   ./verify.sh golden-check classify current-vs-golden diff against real tsc  (FP gate)
#   ./verify.sh crash        full 5907 crash/hang scan
#   ./verify.sh full         quick + crash + golden-check
set -uo pipefail
ROOT=${TSRS_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}
WORK=${TSRS_WORK:-$ROOT}
REL=${TSRS_BIN_RELEASE:-$WORK/target/release/tsrs}
GOLDEN=${TSRS_GOLDEN:-/tmp/golden_diag.txt}
LIB=${TSRS_LIB:-$WORK/lib/lib.tsrs.d.ts}
TSRS_BATCH_JOBS=${TSRS_BATCH_JOBS:-1}

TIMEOUT_BIN=${TIMEOUT_BIN:-}
if [ -z "$TIMEOUT_BIN" ]; then
  if command -v timeout >/dev/null 2>&1; then
    TIMEOUT_BIN=timeout
  elif command -v gtimeout >/dev/null 2>&1; then
    TIMEOUT_BIN=gtimeout
  fi
fi

run_timeout() {
  local seconds=$1
  shift
  if [ -n "$TIMEOUT_BIN" ]; then
    "$TIMEOUT_BIN" "$seconds" "$@"
  else
    "$@"
  fi
}

cd "$ROOT" || exit 1
if [ -z "$TIMEOUT_BIN" ]; then
  echo "verify: timeout/gtimeout not found; running without command time limits"
fi

build() {
  ( cd "$WORK" || exit 1
    re=$(cargo build --release 2>&1 | grep -cE '^error\[|^error:')
    de=$(cargo build 2>&1 | grep -cE '^error\[|^error:')
    w=$(cargo build --release 2>&1 | grep -c '^warning')
    echo "build: release-err=$re debug-err=$de warnings=$w"
    [ "$re" = 0 ] && [ "$de" = 0 ]
  )
}

corpus() {
  local file=$1 label=$2 n=0 m=0 f=""
  while IFS= read -r line; do
    [ -z "$line" ] && continue
    n=$((n+1))
    r=$(printf '%s\n' "$line" | ./difftest/cmp.sh - 2>/dev/null)
    [ "$r" = "MATCH" ] && m=$((m+1)) || f="$f $n"
  done < "$file"
  echo "$label: $m/$n (fails:$f)"
  [ "$m" = "$n" ]
}

quick() {
  local ok=0
  build || ok=1
  corpus difftest/corpus.txt  corpus  || ok=1
  corpus difftest/corpus2.txt corpus2 || ok=1
  ut=$(cargo test --release --manifest-path "$WORK/Cargo.toml" 2>&1 | grep -E 'test result' | head -1)
  echo "unit: $ut"
  echo "$ut" | grep -q '0 failed' || ok=1
  if [ -f /tmp/conf_list.txt ]; then
    run_timeout 200 "$REL" --parse-batch /tmp/conf_list.txt > /tmp/conf_out.txt 2>/dev/null
    pf=$(python3 difftest/parse_cmp.py 2>/dev/null | grep -i 'false-positive' | head -1)
    echo "parse-FP: $pf"
    echo "$pf" | grep -qE '\b0\b' || ok=1
  else
    echo "parse-FP: SKIP missing /tmp/conf_list.txt"
    ok=1
  fi
  for c in cases cases2 cases3 cases4 cases5; do
    if [ -f "/tmp/$c.json" ]; then
      line=$(run_timeout 240 python3 conf/run.py /tmp/$c.json 2>&1 | head -1)
      echo "$c: $line"
    else
      echo "$c: SKIP missing /tmp/$c.json"
      ok=1
    fi
  done
  return $ok
}

# full batch diag-json over all 5907 (also surfaces PANIC lines)
batch_all() {
  local batch_args=()
  if [ -n "$TSRS_BATCH_JOBS" ]; then
    case "$TSRS_BATCH_JOBS" in
      *[!0-9]*)
        echo "verify: TSRS_BATCH_JOBS must be a positive integer, got '$TSRS_BATCH_JOBS'" >&2
        return 2
        ;;
    esac
    if [ "$TSRS_BATCH_JOBS" -lt 1 ]; then
      echo "verify: TSRS_BATCH_JOBS must be a positive integer, got '$TSRS_BATCH_JOBS'" >&2
      return 2
    fi
    batch_args=(--jobs "$TSRS_BATCH_JOBS")
  fi
  : > "$1"
  for ch in /tmp/chunk1.txt /tmp/chunk2.txt /tmp/chunk3.txt /tmp/chunk_tail.txt; do
    run_timeout 600 "$REL" --check-batch "$ch" "${batch_args[@]}" >> "$1" 2>/dev/null
  done
}

classify_golden() {
  local old=$1 cur=$2
  if [ -x "$ROOT/scripts/parallel_classify.py" ]; then
    python3 "$ROOT/scripts/parallel_classify.py" "$old" "$cur" "$LIB"
  elif [ -x /tmp/parallel_classify.py ]; then
    python3 /tmp/parallel_classify.py "$old" "$cur" "$LIB"
  else
    python3 difftest/golden_classify.py "$old" "$cur" "$LIB"
  fi
}

crash() {
  local out=/tmp/crash_scan.txt
  batch_all "$out"
  python3 - "$out" <<'PY'
import sys
data=open(sys.argv[1],'rb').read()
panics=[p[0].decode('utf8','replace') for seg in data.split(b'\n') for p in [seg.split(b'\x01')] if len(p)>=2 and p[1].rstrip()==b'PANIC']
print(f"crash-scan: files-with-PANIC={len(panics)}")
for p in panics[:20]: print("   PANIC:", p.split('/')[-1])
PY
}

mf() {
  # Multi-file PARALLEL determinism (stage-3). One program, many files sharing an
  # immutable BindResult (Phase 3a), checked by the worker pool. Asserts the
  # output does not depend on worker count. Fixtures are generated inline so the
  # check is self-contained and portable.
  local d; d=$(mktemp -d)
  # cross-file generics + shared types (transient sig scopes + per-worker synth)
  cat > "$d/01_defs.ts" <<'EOF'
function identity<T>(x: T): T { return x; }
function pair<A, B>(a: A, b: B): [A, B] { return [a, b]; }
interface Container<U> { value: U; map<R>(f: (u: U) => R): Container<R>; }
type Lookup<O, K extends keyof O> = O[K];
EOF
  cat > "$d/02_uses.ts" <<'EOF'
const a = identity(42);
const b = pair("x", true);
declare const c: Container<number>;
const e = c.map(n => n.toString());
type T1 = Lookup<{ a: number; b: string }, "a">;
EOF
  # declaration merging across files (interface + namespace)
  cat > "$d/03_merge_a.ts" <<'EOF'
interface Box { width: number; }
namespace Geo { export const origin = 0; }
EOF
  cat > "$d/04_merge_b.ts" <<'EOF'
interface Box { height: number; }
namespace Geo { export function dist(x: number): number { return x; } }
EOF
  cat > "$d/05_merge_use.ts" <<'EOF'
const box: Box = { width: 1, height: 2 };
const g = Geo.origin + Geo.dist(5);
EOF
  # merge CONFLICT -> TS2428 (per-symbol dedup, canonical decl positions)
  printf 'interface Pairing<T> { a: T; }\n' > "$d/06_conflict_a.ts"
  printf 'interface Pairing<U, V> { b: U; c: V; }\n' > "$d/07_conflict_b.ts"
  # unused locals/params in functions -> TS6133 (unused pass over MERGED usage)
  local k
  for k in 1 2 3 4 5; do
    cat > "$d/2${k}_locals.ts" <<EOF
function fn${k}_a() { const usedL = ${k}; const unusedL_${k} = ${k} + 1; return usedL; }
function fn${k}_b(usedParam: number, unusedParam_${k}: string) { let ev_${k}; ev_${k} = usedParam; return ev_${k}; }
EOF
  done
  # type errors -> TS2322 / TS2345
  cat > "$d/30_errors.ts" <<'EOF'
const bad: number = "string";
function fer(p: number) { return p; }
const r2 = fer("nope");
EOF
  # auto-variable capture read -> TS7005 at the read + TS7034 at the decl
  # (worker-transported auto_fired: the decl's file may be checked by any
  # worker; check_unused emits 7034 on the merged state)
  cat > "$d/08_auto.ts" <<'EOF'
let evCap; evCap = 1;
export const evReader = () => evCap;
EOF
  # several files trigger the SAME file-less missing-global -> TS2318 cross-worker dedup
  for k in 1 2 3; do
    cat > "$d/4${k}_deco.ts" <<EOF
function deco${k}(t: any, key: string) {}
class WithDeco${k} { @deco${k} method() {} }
EOF
  done

  local all; all=$(ls "$d"/*.ts | sort)
  local nfiles; nfiles=$(printf '%s\n' "$all" | wc -l | tr -d ' ')
  local base; base=$(TSRS_JOBS=1 "$REL" --noUnusedLocals --noUnusedParameters --noImplicitAny true $all 2>&1 | sort)
  local ok=0 j out
  for j in 2 3 4 8 16; do
    out=$(TSRS_JOBS=$j "$REL" --noUnusedLocals --noUnusedParameters --noImplicitAny true $all 2>&1 | sort)
    [ "$out" = "$base" ] || { echo "mf: jobs=$j DIFFERS from jobs=1"; ok=1; }
  done
  local codes; codes=$(printf '%s\n' "$base" | grep -oE 'TS[0-9]+' | sort | uniq -c | tr -s ' ' | paste -sd' ' -)
  rm -rf "$d"
  [ "$ok" = 0 ] && echo "mf: PASS — ${nfiles}-file program, jobs 1/2/3/4/8/16 byte-identical [$codes]" || echo "mf: FAIL"
  return $ok
}

case "${1:-quick}" in
  quick)
    if quick; then
      echo "== quick: PASS =="
      exit 0
    else
      echo "== quick: FAIL =="
      exit 1
    fi
    ;;
  golden-save) build && batch_all "$GOLDEN" && echo "golden saved: $(wc -l < "$GOLDEN") lines, $(wc -c < "$GOLDEN") bytes -> $GOLDEN" ;;
  golden-check)
    build || { echo "BUILD FAILED — golden-check aborted (would run a stale binary)"; exit 1; }
    cur=/tmp/golden_now.txt; batch_all "$cur"
    classify_golden "$GOLDEN" "$cur"
    ;;
  crash) crash ;;
  mf) build && mf ;;
  full)
    quick; crash; mf
    if build; then
      cur=/tmp/golden_now.txt; batch_all "$cur"
      classify_golden "$GOLDEN" "$cur"
    else
      echo "BUILD FAILED — golden-check skipped (stale binary)"; exit 1
    fi ;;
  *) echo "usage: verify.sh {quick|golden-save|golden-check|crash|mf|full}"; exit 2 ;;
esac
