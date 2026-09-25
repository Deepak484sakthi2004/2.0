#!/usr/bin/env bash
# Merge one writer report (notes/part-NN-report.md) into PROGRESS.md:
#   - inserts "## Part <ROMAN> concepts introduced" (rows from the report's "PROGRESS concepts" table) in Part order
#   - appends the report's Meridian rows to the Meridian table
# Usage (from the book root): bash tools/merge-report.sh 10 [extra-report ...]
# Status rows and the open-promises list are edited by hand.
set -euo pipefail
nn="$1"; shift
romans=(_ I II III IV V VI VII VIII IX X XI XII XIII XIV XV XVI XVII XVIII XIX XX XXI XXII XXIII XXIV XXV XXVI)
n=$((10#$nn)); roman="${romans[$n]}"
reports=("notes/part-$nn-report.md" "$@")
prog=PROGRESS.md

if grep -q "^## Part $roman concepts introduced$" "$prog"; then
  echo "Part $roman concepts already in $prog; skipping concepts"; skip_concepts=1
else skip_concepts=0; fi

block=$(mktemp); rows=$(mktemp)
{
  echo "## Part $roman concepts introduced"; echo
  first=1
  for r in "${reports[@]}"; do
    sed -n '/^## PROGRESS concepts/,/^## /p' "$r" | grep '^|' | { if [ $first = 1 ]; then cat; else grep -v -e '^| Concept' -e '^|---'; fi; }
    first=0
  done
  echo
} > "$block"
for r in "${reports[@]}"; do
  sed -n '/^## Meridian facts/,/^## /p' "$r" | grep '^|' | grep -v -e '^| System' -e '^|---' || true
done > "$rows"

if [ $skip_concepts = 0 ]; then
  # insert before the concepts header of the highest-numbered Part below this one
  target=""
  for ((k=n-1; k>=1; k--)); do
    if grep -q "^## Part ${romans[$k]} concepts introduced$" "$prog"; then target="## Part ${romans[$k]} concepts introduced"; break; fi
  done
  [ -n "$target" ] || { echo "no lower Part concepts header found"; exit 1; }
  awk -v t="$target" -v f="$block" 'BEGIN{while((getline l<f)>0) b=b l "\n"} $0==t && !d {printf "%s", b; d=1} {print}' "$prog" > "$prog.tmp" && mv "$prog.tmp" "$prog"
fi

# append Meridian rows after the last row of the Meridian table (the table ends before the "**Ferrite**" line)
if [ -s "$rows" ]; then
  awk -v f="$rows" 'BEGIN{while((getline l<f)>0) b=b l "\n"}
    /^## Running case study: Meridian/ {inm=1}
    inm && /^\*\*Ferrite\*\*/ && !d { sub(/\n$/, "", b); print b; print ""; d=1 }
    { if (inm && !d && /^$/ && prev ~ /^\|/) { held=1; next } if (held) { held=0 } print; prev=$0 }' "$prog" > "$prog.tmp" && mv "$prog.tmp" "$prog"
fi
echo "merged Part $roman: $(grep -c '^|' "$block") concept lines, $(wc -l < "$rows") Meridian rows"
rm -f "$block" "$rows"
