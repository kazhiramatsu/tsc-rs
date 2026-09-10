#!/bin/bash
# Exports the decorator-next draft as reviewable per-cause candidate patches
# (one per commit on the draft branch after the witness commit) plus the
# cumulative production diff against the attempt65 start point.
set -euo pipefail
cd "$(dirname "$0")/.."
START="$1"      # start-point commit (attempt65 snapshot)
OUT="docs/design/greenfield/slices"
i=0
for sha in $(git rev-list --reverse "$START"..HEAD); do
  subject=$(git log -1 --format=%s "$sha")
  case "$subject" in
    *"witness groups"*) name="witnesses" ;;
    *"item 1, follow-up"*) name="item1-named-evaluation-target" ;;
    *"item 1)"*) name="item1-visit-order" ;;
    *"item 3a)"*) name="item3a-temp-bindings" ;;
    *"item 2)"*) name="item2-super-paths" ;;
    *"item 3c)"*) name="item3c-class-fields-handoff" ;;
    *"item 3c, follow-up"*) name="item3c-class-fields-handoff-followup" ;;
    *"item 3b)"*) name="item3b-outer-this-binding" ;;
    *"item 4, follow-up"*) name="item4-emitted-name-not-reentered" ;;
    *"item 1, follow-up"*) name="item1-named-evaluation-target" ;;
    *"item 4)"*) name="item4-memo-audit" ;;
    *) name="misc" ;;
  esac
  i=$((i+1))
  file="$OUT/h2-8a-decorator-next-$(printf '%02d' $i)-$name.candidate.patch"
  git format-patch -1 --stdout "$sha" > "$file"
  echo "$file $(shasum -a 256 "$file" | cut -d' ' -f1)"
done
git diff "$START" HEAD -- crates/emitter/src > "$OUT/h2-8a-decorator-next-production.cumulative.patch"
echo "$OUT/h2-8a-decorator-next-production.cumulative.patch $(shasum -a 256 "$OUT/h2-8a-decorator-next-production.cumulative.patch" | cut -d' ' -f1)"
