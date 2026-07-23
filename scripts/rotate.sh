#!/bin/sh
# Leave-one-app-out rotation (PLAN 2.9.3): for each per-host dataset under
# <by-app-root>, train on the merged others and eval on the held-out app.
# Usage: scripts/rotate.sh <by-app-root> <work-dir> [epochs]
set -eu
ROOT=$1
WORK=$2
EPOCHS=${3:-30}
mkdir -p "$WORK"
SUMMARY="$WORK/rotation-summary.txt"
: > "$SUMMARY"

for HELD in "$ROOT"/*/; do
    APP=$(basename "$HELD")
    TRAIN="$WORK/train-minus-$APP"
    rm -rf "$TRAIN"
    OTHERS=$(ls -d "$ROOT"/*/ | grep -v "/$APP/\$")
    # shellcheck disable=SC2086 — word-splitting the dir list is the point
    cargo run --release -q -p verbivore -- dataset-merge "$TRAIN" $OTHERS >/dev/null
    echo "=== fold: heldout=$APP ==="
    OUT=$(cargo run --release -q -p verbivore-grounding --bin train-eval -- "$TRAIN" "$HELD" "$EPOCHS" | tail -1)
    echo "$APP: $OUT" | tee -a "$SUMMARY"
done

echo "--- rotation complete ---"
cat "$SUMMARY"
