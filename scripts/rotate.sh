#!/bin/sh
# Leave-one-app-out rotation (PLAN 2.9.3): for each per-host dataset under
# <by-app-root>, train on the merged others and eval on the held-out app.
# Usage: scripts/rotate.sh <by-app-root> <work-dir> [epochs] [seeds]
#
# B.2 credit rule: training is bit-reproducible at fixed seed (B.1), so ALL
# run-to-run swing is init/shuffle seed variance — and it's fold-dependent
# (measured on v8, seeds 42-44: mediawiki mAP range 0.013, gitea 0.047,
# ghost 0.100; link-AP ranges 0.065-0.087). Judge levers on seed-MEAN
# deltas; a single-seed fold delta smaller than that fold's seed range is
# noise. Pass seeds as a space-separated list ("42 43 44") for a measured
# rotation; the single-seed default is for cheap smoke only.
set -eu
ROOT=$1
WORK=$2
EPOCHS=${3:-30}
SEEDS=${4:-42}
mkdir -p "$WORK"
SUMMARY="$WORK/rotation-summary.txt"
{
    echo "# seeds: $SEEDS — judge levers on mean deltas; single-seed fold"
    echo "# deltas inside the fold's seed range are noise (B.1/B.2)."
} > "$SUMMARY"

for HELD in "$ROOT"/*/; do
    APP=$(basename "$HELD")
    # Tiny apps stay in every TRAINING merge but measure nothing as folds.
    N=$(ls "$HELD/samples" 2>/dev/null | grep -c '\.json$' || echo 0)
    if [ "$N" -lt 100 ]; then
        echo "$APP: only $N samples, skipping as heldout fold" | tee -a "$SUMMARY"
        continue
    fi
    TRAIN="$WORK/train-minus-$APP"
    rm -rf "$TRAIN"
    OTHERS=$(ls -d "$ROOT"/*/ | grep -v "/$APP/\$")
    # shellcheck disable=SC2086 — word-splitting the dir list is the point
    cargo run --release -q -p verbivore -- dataset-merge "$TRAIN" $OTHERS >/dev/null
    echo "=== fold: heldout=$APP (seeds: $SEEDS) ==="
    # Full output (per-class + size-stratified AP) survives in the fold log;
    # the summary keeps only aggregates. Losing the per-class detail cost a
    # full re-train pass in v7 — don't slim this back down.
    VALS=""
    for SEED in $SEEDS; do
        LOG="$WORK/fold-$APP-seed$SEED.log"
        cargo run --release -q -p verbivore-grounding --bin train-eval -- \
            "$TRAIN" "$HELD" "$EPOCHS" "$SEED" > "$LOG"
        VALS="$VALS $(tail -1 "$LOG" | grep -o 'mAP@0.5=[0-9.]*' | cut -d= -f2)"
    done
    STATS=$(echo "$VALS" | tr ' ' '\n' | awk 'NF { s += $1; n++;
            if (min == "" || $1 < min) min = $1; if ($1 > max) max = $1 }
        END { printf "mAP mean=%.3f range=%.3f [%.3f, %.3f] n=%d",
            s / n, max - min, min, max, n }')
    echo "$APP: $STATS" | tee -a "$SUMMARY"
done

echo "--- rotation complete ---"
cat "$SUMMARY"
