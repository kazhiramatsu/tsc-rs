#!/bin/bash
# Exports the decorator source follow-up as reviewable per-cause candidate
# patches (one per commit that changes crates/ or scripts/ between the fixed
# start and end points) plus the cumulative production diff. Both endpoints
# are fixed to this candidate; pass explicit commits only to reproduce a
# different range. The decorator-next exporter and its default endpoint
# `306930ab7` are untouched.
set -euo pipefail
cd "$(dirname "$0")/.."
START="${1:-0fda49509aed6280b9969a2bde6d947acf68e25c}"   # prep/h2-8a-decorator-followup before this candidate
END="${2:-a4c089c7b1dff7f1c794d93bdcf15e7fb061770a}"                                    # final production commit of this candidate
START=$(git rev-parse --verify "$START^{commit}")
END=$(git rev-parse --verify "$END^{commit}")
git merge-base --is-ancestor "$START" "$END"
OUT="docs/design/greenfield/slices"
i=0
for sha in $(git rev-list --reverse "$START".."$END"); do
  if git diff-tree --quiet "$sha^" "$sha" -- crates scripts; then
    continue
  fi
  subject=$(git log -1 --format=%s "$sha")
  case "$subject" in
    *"register 48 decorator source follow-up witnesses"*) name="witnesses-48" ;;
    *"literal computed names"*) name="cause1-literal-computed-names" ;;
    *"anonymous classes in decorated computed fields"*) name="cause2-decorated-named-evaluation-double-hoist" ;;
    *"witness the source-file owner"*) name="witnesses-top-level" ;;
    *"undecorated"*) name="cause3-undecorated-owner-hoist" ;;
    *) name="misc" ;;
  esac
  i=$((i+1))
  file="$OUT/h2-8a-decorator-followup-$(printf '%02d' $i)-$name.candidate.patch"
  git format-patch -1 --stdout "$sha" > "$file"
  echo "$file $(shasum -a 256 "$file" | cut -d' ' -f1)"
done
git diff "$START" "$END" -- crates/emitter/src > "$OUT/h2-8a-decorator-followup-production.cumulative.patch"
echo "$OUT/h2-8a-decorator-followup-production.cumulative.patch $(shasum -a 256 "$OUT/h2-8a-decorator-followup-production.cumulative.patch" | cut -d' ' -f1)"
