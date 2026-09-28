#!/usr/bin/env bash
# Issue #79 (Infra E3b): measurement phases, in the preregistered order
# (#79 section 3). Each phase is invoked separately so the order is visible
# in the commit/log history:
#
#   bash scripts/issue79/run_e3b.sh competitor    # corrected competitor baseline (first)
#   bash scripts/issue79/run_e3b.sh n0            # unchanged #77 native binary
#   bash scripts/issue79/run_e3b.sh calibration   # calibration cells only
#   bash scripts/issue79/run_e3b.sh headline      # held-out cells (needs E3B_TAU_F/E3B_RHO_S or none)
#   bash scripts/issue79/run_e3b.sh memory        # load/build/RSS launch configurations
#
# Every engine runs in the scope envelope (scripts/issue79/scope_runtime.sh,
# frozen #77 CPU/memory values); the driver is pinned to CPU 3. Nothing else
# CPU-heavy (e.g. cargo) may run on the host while a phase is running.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue77/resource_envelope.env
source "$REPO_ROOT/benchmarks/configs/issue77/resource_envelope.env"
# shellcheck source=scope_runtime.sh
source "$SCRIPT_DIR/scope_runtime.sh"
export I77_RUNTIME=scope

OUT="$REPO_ROOT/artifacts/issue79/results"
CATALOG="$REPO_ROOT/$I77_CATALOG_500K_PATH"
N0_BINARY="${E3B_N0_BINARY:-$E3B_ENGINE_ROOT/n0/i77_native_plp_server_9482f5d}"
N0_SHA256="91060ad12cd2ed8bcd5199776ca2ababe3438c48f2274ae3647d5b819916333b"
NEW_BINARY="$REPO_ROOT/target/release/e3b_native_plp_server"
DRIVER="$REPO_ROOT/target/release/e3b_native_measure"
ALL_STRUCTS="--sort-columns average_rating,review_count,rating_count --presence average_rating,review_count,rating_count"
RUNS="${E3B_RUNS:-1 2 3}"
phase="${1:?usage: run_e3b.sh <competitor|n0|calibration|headline|memory>}"
mkdir -p "$OUT/$phase"

log() { echo "=== $(date -u +%FT%TZ) $* ==="; }

case "$phase" in
  competitor)
    # #77's corrected harness (i77_measure), facet cells + the sort cell only.
    CELLS=facet_low_cardinality_style,facet_medium_cardinality_primarymaterial,facet_high_cardinality_color,facet_disjunctive_multi_dim,numeric_range_sort
    ENGINES="${E3B_ENGINES:-meilisearch typesense solr}"
    for engine in $ENGINES; do
      container_var="I77_$(echo "$engine" | tr '[:lower:]' '[:upper:]')_CONTAINER"
      [[ "$engine" == "elasticsearch" ]] && container_var=I77_ES_CONTAINER
      for run in $RUNS; do
        out="$OUT/competitor/${engine}_500k_run${run}.json"
        log "competitor engine=$engine run=$run"
        e3b_scope_stop "${!container_var}"
        taskset -c "$I77_DRIVER_CPU" "$REPO_ROOT/target/release/i77_measure" \
          --engine "$engine" --tier 500k --run "$run" --repository-root "$REPO_ROOT" \
          --out "$out" --cells "$CELLS" --skip-throughput true
        e3b_scope_stop "${!container_var}"
      done
    done
    ;;
  n0)
    echo "$N0_SHA256  $N0_BINARY" | sha256sum -c - || exit 1
    for run in $RUNS; do
      log "n0 run=$run"
      taskset -c "$I77_DRIVER_CPU" "$DRIVER" --repository-root "$REPO_ROOT" \
        --server-binary "$N0_BINARY" --catalog "$CATALOG" --label n0 --run "$run" \
        --out "$OUT/n0/n0_500k_run${run}.json" --cells all --modes none
    done
    ;;
  calibration)
    for run in $RUNS; do
      log "calibration run=$run"
      taskset -c "$I77_DRIVER_CPU" "$DRIVER" --repository-root "$REPO_ROOT" \
        --server-binary "$NEW_BINARY" --catalog "$CATALOG" --label calibration --run "$run" \
        --out "$OUT/calibration/calibration_500k_run${run}.json" --cells calibration \
        --facet-cell-modes legacy:legacy,ordinal:legacy,bitmap:legacy \
        --sort-cell-modes legacy:legacy,legacy:topk,legacy:presorted \
        --server-args "$ALL_STRUCTS"
    done
    ;;
  headline)
    : "${E3B_FACET_FINAL:?set E3B_FACET_FINAL (hybrid or the calibration winner)}"
    : "${E3B_SORT_FINAL:?set E3B_SORT_FINAL (hybrid or the calibration winner)}"
    CONSTS=""
    [[ -n "${E3B_TAU_F:-}" ]] && CONSTS="$CONSTS --tau-f $E3B_TAU_F"
    [[ -n "${E3B_RHO_S:-}" ]] && CONSTS="$CONSTS --rho-s $E3B_RHO_S"
    FACET_MODES="legacy:legacy,ordinal:legacy,bitmap:legacy"
    [[ -n "${E3B_TAU_F:-}" ]] && FACET_MODES="$FACET_MODES,hybrid:legacy"
    # legacy facet + final result path isolates bounded assembly alone.
    FACET_MODES="$FACET_MODES,legacy:$E3B_SORT_FINAL,ordinal:$E3B_SORT_FINAL,bitmap:$E3B_SORT_FINAL"
    [[ "$E3B_FACET_FINAL" == hybrid ]] && FACET_MODES="$FACET_MODES,hybrid:$E3B_SORT_FINAL"
    SORT_MODES="legacy:legacy,legacy:topk,legacy:presorted"
    [[ -n "${E3B_RHO_S:-}" ]] && SORT_MODES="$SORT_MODES,legacy:hybrid"
    SORT_MODES="$SORT_MODES,$E3B_FACET_FINAL:$E3B_SORT_FINAL"
    for run in $RUNS; do
      log "headline run=$run facet_modes=$FACET_MODES sort_modes=$SORT_MODES"
      taskset -c "$I77_DRIVER_CPU" "$DRIVER" --repository-root "$REPO_ROOT" \
        --server-binary "$NEW_BINARY" --catalog "$CATALOG" --label headline --run "$run" \
        --out "$OUT/headline/headline_500k_run${run}.json" --cells headline \
        --facet-cell-modes "$FACET_MODES" --sort-cell-modes "$SORT_MODES" \
        --server-args "$ALL_STRUCTS$CONSTS"
    done
    ;;
  memory)
    # Launch configurations (#79 section 9). F1/F2 add no new structure
    # (they reuse enum_columns/enum_bitmaps already in the base index), so
    # "facet structures only" is the base configuration by construction;
    # it is still launched separately so that claim is measured, not assumed.
    declare -A CONFIGS=(
      [base]="--sort-columns none --presence none"
      [facet_only]="--sort-columns none --presence none"
      [sort_columns_only]="--sort-columns average_rating,review_count,rating_count --presence none"
      [presence_only]="--sort-columns none --presence average_rating,review_count,rating_count"
      [combined]="$ALL_STRUCTS"
    )
    for run in $RUNS; do
      for config in base facet_only sort_columns_only presence_only combined; do
        log "memory config=$config run=$run"
        taskset -c "$I77_DRIVER_CPU" "$DRIVER" --repository-root "$REPO_ROOT" \
          --server-binary "$NEW_BINARY" --catalog "$CATALOG" --label "mem_$config" --run "$run" \
          --out "$OUT/memory/${config}_500k_run${run}.json" --cells none --modes none \
          --server-args "${CONFIGS[$config]}"
      done
    done
    ;;
  *)
    echo "unknown phase $phase" >&2
    exit 2
    ;;
esac
log "phase $phase done"
