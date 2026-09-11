#!/bin/bash
# Exports the decorator-next draft as reviewable per-cause candidate patches
# (one per commit on the draft branch after the witness commit) plus the
# cumulative production diff against the attempt65 start point.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: $0 <start-commit> [candidate-commit]" >&2
  exit 2
fi
START="$1"      # start-point commit (attempt65 snapshot)
END="${2:-306930ab7c7fabab39c4e66321de68181b1abfe9}"
START=$(git rev-parse --verify "$START^{commit}")
END=$(git rev-parse --verify "$END^{commit}")
git merge-base --is-ancestor "$START" "$END"
OUT="docs/design/greenfield/slices"
i=0
for sha in $(git rev-list --reverse "$START".."$END"); do
  # Evidence commits contain the exported patches themselves. Only commits
  # that change the implementation or its tests belong to this series.
  if git diff-tree --quiet "$sha^" "$sha" -- crates; then
    continue
  fi
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
    *"item 4)"*) name="item4-memo-audit" ;;
    *) name="misc" ;;
  esac
  i=$((i+1))
  file="$OUT/h2-8a-decorator-next-$(printf '%02d' $i)-$name.candidate.patch"
  git format-patch -1 --stdout "$sha" > "$file"
  echo "$file $(shasum -a 256 "$file" | cut -d' ' -f1)"
done
git diff "$START" "$END" -- crates/emitter/src > "$OUT/h2-8a-decorator-next-production.cumulative.patch"
echo "$OUT/h2-8a-decorator-next-production.cumulative.patch $(shasum -a 256 "$OUT/h2-8a-decorator-next-production.cumulative.patch" | cut -d' ' -f1)"
