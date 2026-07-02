#!/bin/bash
# Differential test: compare tsrs vs tsc (with the same lib) on a snippet.
# Usage: cmp.sh <file.ts>   OR   echo 'code' | cmp.sh -
set -uo pipefail
ROOT=${TSRS_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}
TSRS=${TSRS_BIN_DEBUG:-$ROOT/target/debug/tsrs}
LIB=${TSRS_LIB:-$ROOT/lib/lib.tsrs.d.ts}
ORACLE=${TSRS_ORACLE:-$ROOT/oracle}

if [ ! -x "$TSRS" ]; then
  echo "cmp.sh: missing tsrs debug binary: $TSRS" >&2
  exit 2
fi
if [ ! -f "$LIB" ]; then
  echo "cmp.sh: missing lib file: $LIB" >&2
  exit 2
fi
if [ -n "${TSC_BIN:-}" ]; then
  TSC_CMD=("$TSC_BIN")
elif [ -x "$ORACLE/node_modules/.bin/tsc" ]; then
  TSC_CMD=("$ORACLE/node_modules/.bin/tsc")
elif command -v tsc >/dev/null 2>&1; then
  TSC_CMD=(tsc)
else
  echo "cmp.sh: missing TypeScript compiler; set TSC_BIN or TSRS_ORACLE" >&2
  exit 2
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
if [ "$1" = "-" ]; then cat > "$WORK/main.ts"; else cp "$1" "$WORK/main.ts"; fi
cp "$LIB" "$WORK/lib.tsrs.d.ts"
shift 2>/dev/null
# tsrs: prepends its own lib; report on main.ts
( cd "$WORK" && TSRS_VIRTUAL_CWD="$WORK" "$TSRS" --strict main.ts 2>&1 ) \
  | sed "s|$WORK/||g; s|^main.ts|main.ts|" | grep -E '^main\.ts' | sort > "$WORK/tsrs.out"
# tsc: --noLib + the lib file; report on main.ts only
( cd "$WORK" && "${TSC_CMD[@]}" --noEmit --pretty false --noLib --strict lib.tsrs.d.ts main.ts 2>&1 ) \
  | sed "s|$WORK/||g" | grep -E '^main\.ts' | sort > "$WORK/tsc.out"
if diff -q "$WORK/tsrs.out" "$WORK/tsc.out" >/dev/null; then
  echo "MATCH"
else
  echo "DIFF:"
  diff <(sed 's/^/tsrs: /' "$WORK/tsrs.out") <(sed 's/^/tsc:  /' "$WORK/tsc.out") | grep -E '^[<>]'
fi
