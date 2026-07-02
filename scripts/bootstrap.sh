#!/usr/bin/env bash
#
# tsc-rs verification environment bootstrap.
#
# This prepares the local tools and /tmp fixtures used by verify.sh:
#
#   ./oracle/                  TypeScript oracle installation
#   ./ts-tests/                TypeScript conformance corpus
#   /tmp/chunk*.txt            conformance batch lists
#   /tmp/golden_diag.txt       current tsrs baseline for golden-check
#   /tmp/conf_list.txt         parser comparison input list
#   /tmp/conf_tsc.txt          parser oracle output
#   /tmp/cases{,2,3,4,5}.json  targeted diagnostic case corpora
#
# Usage:
#   ./scripts/bootstrap.sh
#   ./scripts/bootstrap.sh --skip-corpus
#   ./scripts/bootstrap.sh --skip-oracle
#   ./scripts/bootstrap.sh --skip-golden
#   ./scripts/bootstrap.sh --skip-build
#   ./scripts/bootstrap.sh --skip-smoke

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)

TYPESCRIPT_VERSION=${TSRS_TYPESCRIPT_VERSION:-6.0.3}
ORACLE=${TSRS_ORACLE:-$REPO_ROOT/oracle}
TS_TESTS=${TSRS_TS_TESTS:-$REPO_ROOT/ts-tests}
WORK=${TSRS_WORK:-$REPO_ROOT}
LIB=${TSRS_LIB:-$WORK/lib/lib.tsrs.d.ts}
REL=${TSRS_BIN_RELEASE:-$WORK/target/release/tsrs}
GOLDEN=${TSRS_GOLDEN:-/tmp/golden_diag.txt}

SKIP_CORPUS=0
SKIP_ORACLE=0
SKIP_GOLDEN=0
SKIP_BUILD=0
SKIP_SMOKE=0

usage() {
  cat <<'EOF'
tsc-rs verification environment bootstrap.

Usage:
  ./scripts/bootstrap.sh
  ./scripts/bootstrap.sh --skip-corpus
  ./scripts/bootstrap.sh --skip-oracle
  ./scripts/bootstrap.sh --skip-golden
  ./scripts/bootstrap.sh --skip-build
  ./scripts/bootstrap.sh --skip-smoke

Environment overrides:
  TSRS_TYPESCRIPT_VERSION
  TSRS_ORACLE
  TSRS_TS_TESTS
  TSRS_WORK
  TSRS_LIB
  TSRS_BIN_RELEASE
  TSRS_GOLDEN
EOF
}

for arg in "$@"; do
  case "$arg" in
    --skip-corpus) SKIP_CORPUS=1 ;;
    --skip-oracle) SKIP_ORACLE=1 ;;
    --skip-golden) SKIP_GOLDEN=1 ;;
    --skip-build) SKIP_BUILD=1 ;;
    --skip-smoke) SKIP_SMOKE=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown option: $arg" >&2; usage; exit 2 ;;
  esac
done

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

echo "==> checking system prerequisites"
for bin in git rustc cargo node npm python3; do
  if ! command -v "$bin" >/dev/null 2>&1; then
    echo "MISSING: $bin - install it and re-run" >&2
    exit 1
  fi
done
echo "  rustc: $(rustc --version)"
echo "  node:  $(node --version)"
echo "  python: $(python3 --version)"
if [ -z "$TIMEOUT_BIN" ]; then
  echo "  timeout: not found; bootstrap will run without command time limits"
else
  echo "  timeout: $TIMEOUT_BIN"
fi

echo "==> paths"
echo "  repo:     $REPO_ROOT"
echo "  work:     $WORK"
echo "  oracle:   $ORACLE"
echo "  ts-tests: $TS_TESTS"

if [ "$SKIP_CORPUS" = 0 ] && [ ! -d "$TS_TESTS/tests/cases/conformance" ]; then
  echo "==> cloning TypeScript conformance corpus v$TYPESCRIPT_VERSION"
  mkdir -p "$(dirname "$TS_TESTS")"
  git clone --depth 1 --branch "v$TYPESCRIPT_VERSION" --single-branch \
    https://github.com/microsoft/TypeScript.git "$TS_TESTS"
elif [ -d "$TS_TESTS/tests/cases/conformance" ]; then
  echo "==> ts-tests present"
else
  echo "==> ts-tests skipped"
fi

if [ "$SKIP_ORACLE" = 0 ] && [ ! -d "$ORACLE/node_modules/typescript" ]; then
  echo "==> installing typescript@$TYPESCRIPT_VERSION into oracle/"
  mkdir -p "$ORACLE"
  cat > "$ORACLE/package.json" <<EOF
{
  "name": "tsrs-oracle",
  "version": "1.0.0",
  "private": true,
  "dependencies": {
    "typescript": "$TYPESCRIPT_VERSION"
  }
}
EOF
  (
    cd "$ORACLE"
    npm install --silent
  )
elif [ -d "$ORACLE/node_modules/typescript" ]; then
  echo "==> oracle present"
else
  echo "==> oracle skipped"
fi

if [ "$SKIP_BUILD" = 0 ]; then
  echo "==> building tsrs (release + debug)"
  (
    cd "$WORK"
    cargo build --release
    cargo build
  )
else
  echo "==> build skipped"
fi

if [ -d "$TS_TESTS/tests/cases/conformance" ]; then
  echo "==> building /tmp conformance lists"
  find "$TS_TESTS/tests/cases/conformance" -type f \
    \( -name '*.ts' -o -name '*.tsx' \) | sort > /tmp/all_conf.txt
  cp /tmp/all_conf.txt /tmp/conf_list.txt
  n=$(wc -l < /tmp/all_conf.txt | tr -d ' ')
  echo "  conformance files: $n"
  head -n 2000 /tmp/all_conf.txt > /tmp/chunk1.txt
  sed -n '2001,3500p' /tmp/all_conf.txt > /tmp/chunk2.txt
  sed -n '3501,5065p' /tmp/all_conf.txt > /tmp/chunk3.txt
  sed -n '5066,$p' /tmp/all_conf.txt > /tmp/chunk_tail.txt
  echo "  chunks: $(wc -l < /tmp/chunk1.txt | tr -d ' ') $(wc -l < /tmp/chunk2.txt | tr -d ' ') $(wc -l < /tmp/chunk3.txt | tr -d ' ') $(wc -l < /tmp/chunk_tail.txt | tr -d ' ')"

  if [ -d "$ORACLE/node_modules/typescript" ]; then
    echo "==> creating /tmp/conf_tsc.txt parser oracle"
    TSRS_ORACLE="$ORACLE" node "$WORK/difftest/parse_oracle.js" /tmp/conf_list.txt > /tmp/conf_tsc.txt
  else
    echo "==> parser oracle skipped; oracle is unavailable"
  fi
else
  echo "==> conformance lists skipped; ts-tests unavailable"
fi

echo "==> generating targeted diagnostic corpora"
for gen in gen.py gen2.py gen3.py gen4.py gen5.py; do
  python3 "$WORK/conf/$gen"
done

echo "==> installing /tmp/parallel_classify.py"
cp "$WORK/scripts/parallel_classify.py" /tmp/parallel_classify.py
chmod +x /tmp/parallel_classify.py

if [ "$SKIP_GOLDEN" = 0 ] && [ ! -f "$GOLDEN" ]; then
  if [ ! -x "$REL" ]; then
    echo "ERROR: release binary missing: $REL" >&2
    exit 1
  fi
  if [ ! -f /tmp/chunk1.txt ]; then
    echo "ERROR: /tmp/chunk*.txt missing; cannot create golden baseline" >&2
    exit 1
  fi
  echo "==> creating golden baseline at $GOLDEN"
  echo "  note: this snapshots current tsrs output, not the tsc oracle"
  : > "$GOLDEN"
  for c in /tmp/chunk1.txt /tmp/chunk2.txt /tmp/chunk3.txt /tmp/chunk_tail.txt; do
    run_timeout 600 "$REL" --check-batch "$c" >> "$GOLDEN" 2>/dev/null
  done
  echo "  golden: $(wc -l < "$GOLDEN" | tr -d ' ') lines"
elif [ -f "$GOLDEN" ]; then
  echo "==> golden present: $GOLDEN"
else
  echo "==> golden skipped"
fi

if [ "$SKIP_SMOKE" = 0 ]; then
  echo "==> sanity: nested-class 2454 repro"
  if [ ! -x "$REL" ]; then
    echo "ERROR: release binary missing: $REL" >&2
    exit 1
  fi
  if [ ! -f "$LIB" ]; then
    echo "ERROR: lib file missing: $LIB" >&2
    exit 1
  fi
  w=$(mktemp -d)
  trap 'rm -rf "$w"' EXIT
  cp "$LIB" "$w/lib.tsrs.d.ts"
  cat > "$w/main.ts" <<'EOF'
class C {
  m() {
    let inner: string;
    class N { method() { console.log(inner); } }
  }
}
EOF
  n=$(
    ( TSRS_VIRTUAL_CWD="$w" "$REL" --strict --diag-json "$w/main.ts" 2>/dev/null || true ) |
      python3 -c "import json,sys; d=json.load(sys.stdin); print(sum(1 for x in d.get('diagnostics', []) if x.get('file', '').endswith('main.ts') and x.get('code') == 2454))"
  )
  if [ "$n" = "1" ]; then
    echo "  sanity: OK"
  else
    echo "  sanity: FAIL - got $n TS2454, expected 1" >&2
    exit 1
  fi
else
  echo "==> sanity skipped"
fi

echo ""
echo "==> DONE"
echo "Next commands:"
echo "  ./verify.sh quick"
echo "  ./verify.sh golden-check"
echo "  python3 scripts/parallel_classify.py /tmp/golden_diag.txt /tmp/golden_now.txt lib/lib.tsrs.d.ts"
