#!/bin/bash
# Exports the literal-name / lexical-prologue follow-up as reviewable
# per-cause candidate patches (one per commit that changes crates/ or
# scripts/ between the fixed start and end points) plus the cumulative
# production diff. Both endpoints are fixed to this candidate; pass explicit
# commits only to reproduce a different range. The earlier exporters
# (`export-decorator-next-candidates.sh` @306930ab7 and
# `export-decorator-followup-candidates.sh` @0fda49509..a4c089c7b) and their
# default endpoints are untouched.
set -euo pipefail
cd "$(dirname "$0")/.."
START="${1:-5134bb0187f9f3f6ac2a0218f8d8e98859cf4fea}"   # production base of this candidate (PR #513 head)
END="${2:-8884fcc05622df73812f9fb73cd51be9d9d67cf2}"   # final production commit of this candidate (cause 6)
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
    *"register "*"witnesses"*) name="witnesses" ;;
    *"pending static initializer"*) name="cause1-static-initializer-map-range" ;;
    *"template and numeric key"*) name="cause2-text-source-spelling" ;;
    *"prologue directives"*) name="cause3-function-body-prologue" ;;
    *"custom prologue"*) name="cause4-hoisted-var-custom-prologue" ;;
    *"class-fields function preludes"*) name="cause5-class-fields-prelude-merge" ;;
    *"lower parameter defaults"*) name="cause6-parameter-list-lowering" ;;
    *) name="misc" ;;
  esac
  i=$((i+1))
  file="$OUT/h2-8a-decorator-literal-prologue-$(printf '%02d' $i)-$name.candidate.patch"
  git format-patch -1 --stdout "$sha" > "$file"
  echo "$file $(shasum -a 256 "$file" | cut -d' ' -f1)"
done
git diff "$START" "$END" -- crates/emitter/src > "$OUT/h2-8a-decorator-literal-prologue-production.cumulative.patch"
echo "$OUT/h2-8a-decorator-literal-prologue-production.cumulative.patch $(shasum -a 256 "$OUT/h2-8a-decorator-literal-prologue-production.cumulative.patch" | cut -d' ' -f1)"
