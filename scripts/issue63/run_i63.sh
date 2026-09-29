#!/usr/bin/env bash
# Issue #63 (Infra E3, amendment 1): measurement phases in the preregistered
# order (amendment section 6). Each phase is invoked separately so the order
# is visible in the log/commit history:
#
#   bash scripts/issue63/run_i63.sh gate        # extended correctness gate (100k + 500k)
#   bash scripts/issue63/run_i63.sh micro       # B1/B2/C microbenchmarks, 3 runs
#   I63_NPLUS=<p0r|p1|p2|p2b|none> \
#   bash scripts/issue63/run_i63.sh confirm     # Part A Latin square, 3 runs
#   bash scripts/issue63/run_i63.sh solr_sensitivity  # post-review, NOT preregistered
#
# Every engine runs in #79's scope envelope (scripts/issue79/scope_runtime.sh,
# frozen #77 CPU/memory values); drivers are pinned to CPU 3, the microbench to
# CPU 0 inside its own scope. Nothing else CPU-heavy (e.g. cargo) may run on
# the host while a phase is running, and this script is not edited while a
# phase runs (#79 log, harness incident).
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue77/resource_envelope.env
source "$REPO_ROOT/benchmarks/configs/issue77/resource_envelope.env"
# shellcheck source=../issue79/scope_runtime.sh
source "$REPO_ROOT/scripts/issue79/scope_runtime.sh"
export I77_RUNTIME=scope

OUT="$REPO_ROOT/artifacts/issue63/results"
BIN="$REPO_ROOT/target/release"
CATALOG_500K="$REPO_ROOT/$I77_CATALOG_500K_PATH"
CATALOG_100K="$REPO_ROOT/$I77_CATALOG_100K_PATH"
# Frozen #79 constants (artifacts/issue79/results/planner_constants.json); no recalibration.
TAU_F=919.4972390657695
RHO_S=0.0833779131971903
ALL_STRUCTS="--sort-columns average_rating,review_count,rating_count --presence average_rating,review_count,rating_count"
CELLS=facet_low_cardinality_style,facet_medium_cardinality_primarymaterial,facet_high_cardinality_color,facet_disjunctive_multi_dim,numeric_range_sort,filter_depth_1,filter_depth_3,filter_depth_5
RUNS="${I63_RUNS:-1 2 3}"
phase="${1:?usage: run_i63.sh <gate|micro|confirm>}"
mkdir -p "$OUT/$phase"

log() { echo "=== $(date -u +%FT%TZ) $* ==="; }

# One JSON line of ambient host state (amendment section 2): disclosure only.
ambient() {
  local run="$1" arm="$2" when="$3" file="$4"
  local probe
  probe="$(taskset -c 0 "$BIN/i63_host_probe" 2>/dev/null || echo null)"
  python3 - "$run" "$arm" "$when" "$probe" >>"$file" <<'PY'
import json, sys, time
run, arm, when, probe = sys.argv[1:5]
def read(path):
    try:
        return open(path).read().strip()
    except OSError:
        return None
meminfo = {}
for line in (read("/proc/meminfo") or "").splitlines():
    key, _, value = line.partition(":")
    if key in ("MemAvailable", "MemFree", "SwapFree", "SwapTotal"):
        meminfo[key] = value.strip()
print(json.dumps({
    "run": int(run), "arm": arm, "when": when,
    "utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    "loadavg": read("/proc/loadavg"),
    "pressure": {k: read(f"/proc/pressure/{k}") for k in ("cpu", "memory", "io")},
    "meminfo": meminfo,
    "host_probe": json.loads(probe) if probe != "null" else None,
}))
PY
}

case "$phase" in
  gate)
    for catalog in "$CATALOG_100K" "$CATALOG_500K"; do
      tier=500k; [[ "$catalog" == "$CATALOG_100K" ]] && tier=100k
      log "gate tier=$tier"
      { time "$BIN/e3b_correctness_gate" --catalog "$catalog" --out "$OUT/gate/gate_${tier}.json" \
          --tau-f "$TAU_F" --rho-s "$RHO_S" --cand-modes p0,p0r,p1,p2,p2b \
          --include-reference true; } >"$OUT/gate/gate_${tier}.log" 2>&1
      echo "exit=$?" >>"$OUT/gate/gate_${tier}.log"
      grep -E 'E3B_GATE|CANDIDATE_FAILURE|exit=' "$OUT/gate/gate_${tier}.log"
    done
    ;;
  micro)
    quota=$(( I77_CPUS * 100 ))
    for run in $RUNS; do
      for tier in 500k 100k; do
        catalog="$CATALOG_500K"; [[ "$tier" == 100k ]] && catalog="$CATALOG_100K"
        log "micro run=$run tier=$tier"
        e3b_scope_stop i63-micro
        systemd-run --user --scope --quiet --unit=i63-micro \
          -p "CPUQuota=${quota}%" -p "MemoryMax=$(e3b_systemd_bytes "$I77_MEMORY")" -p "MemorySwapMax=0" \
          taskset -c 0 "$BIN/i63_primitives" --catalog "$catalog" --tier "$tier" --run "$run" \
          --out "$OUT/micro/${tier}_run${run}.json" \
          >"$OUT/micro/${tier}_run${run}.stdout" 2>"$OUT/micro/${tier}_run${run}.stderr"
        echo "exit=$?" >>"$OUT/micro/${tier}_run${run}.stdout"
        e3b_scope_stop i63-micro
        tail -2 "$OUT/micro/${tier}_run${run}.stdout"
      done
    done
    ;;
  confirm)
    NPLUS="${I63_NPLUS:?set I63_NPLUS to the frozen N+ cand_mode, or none}"
    FINAL_MODE="hybrid:hybrid"
    NPLUS_MODE="hybrid:hybrid:$NPLUS"
    SERVER_ARGS="$ALL_STRUCTS --tau-f $TAU_F --rho-s $RHO_S"
    # Only P1/P2/P2b read the prebuilt all-ordinals bitmap; P0r and FINAL do not.
    case "$NPLUS" in p1|p2|p2b) SERVER_ARGS="$SERVER_ARGS --prebuilt-all true" ;; esac
    # Balanced Latin square (amendment section 2); N/N+ order alternates.
    declare -A ORDER=([1]="native meilisearch solr" [2]="solr native meilisearch" [3]="meilisearch solr native")
    AMBIENT="$OUT/confirm/ambient.jsonl"
    for run in $RUNS; do
      RUN_DIR="$OUT/confirm/run${run}"
      mkdir -p "$RUN_DIR/dumps"
      modes="$FINAL_MODE"
      if [[ "$NPLUS" != none ]]; then
        if (( run % 2 == 0 )); then modes="$NPLUS_MODE,$FINAL_MODE"; else modes="$FINAL_MODE,$NPLUS_MODE"; fi
      fi
      for arm in ${ORDER[$run]}; do
        ambient "$run" "$arm" before "$AMBIENT"
        log "confirm run=$run arm=$arm"
        case "$arm" in
          native)
            I63_DUMP_DIR="$RUN_DIR/dumps" taskset -c "$I77_DRIVER_CPU" "$BIN/e3b_native_measure" \
              --repository-root "$REPO_ROOT" --server-binary "$BIN/e3b_native_plp_server" \
              --catalog "$CATALOG_500K" --label i63_confirm --run "$run" \
              --out "$RUN_DIR/native_500k.json" --cells "$CELLS" --modes "$modes" \
              --server-args "$SERVER_ARGS"
            ;;
          meilisearch)
            e3b_scope_stop "$I77_MEILISEARCH_CONTAINER"
            I77_MEILI_LIKE_FOR_LIKE=1 I63_DUMP_DIR="$RUN_DIR/dumps" taskset -c "$I77_DRIVER_CPU" \
              "$BIN/i77_measure" --engine meilisearch --tier 500k --run "$run" \
              --repository-root "$REPO_ROOT" --out "$RUN_DIR/meilisearch_500k.json" \
              --cells "$CELLS" --skip-throughput true
            e3b_scope_stop "$I77_MEILISEARCH_CONTAINER"
            ;;
          solr)
            e3b_scope_stop "$I77_SOLR_CONTAINER"
            I63_EQUAL_WORK=1 I63_DUMP_DIR="$RUN_DIR/dumps" taskset -c "$I77_DRIVER_CPU" \
              "$BIN/i77_measure" --engine solr --tier 500k --run "$run" \
              --repository-root "$REPO_ROOT" --out "$RUN_DIR/solr_500k.json" \
              --cells "$CELLS" --skip-throughput true
            e3b_scope_stop "$I77_SOLR_CONTAINER"
            ;;
        esac
        echo "arm_exit=$? run=$run arm=$arm"
        ambient "$run" "$arm" after "$AMBIENT"
      done
      log "equivalence run=$run"
      "$BIN/i63_facet_equivalence" --catalog "$CATALOG_500K" --dump-dir "$RUN_DIR/dumps" \
        --out "$RUN_DIR/equivalence.json" >"$RUN_DIR/equivalence.log" 2>&1
      echo "equivalence_exit=$?" >>"$RUN_DIR/equivalence.log"
      grep -E 'NOT_EQUIVALENT|UNREADABLE|I63_EQUIVALENCE|equivalence_exit' "$RUN_DIR/equivalence.log"
    done
    ;;
  solr_sensitivity)
    # Post-review Solr configuration sensitivity (NOT preregistered; the
    # preregistered Part A verdict is unchanged by it): equal-work Solr with
    # buckets in term order (I63_SOLR_FACET_SORT=index) instead of count
    # order, interleaved in one window with native FINAL + N+ (P0r), 3 pairs
    # in alternating order, FH1-FH4 only.
    FH=facet_low_cardinality_style,facet_medium_cardinality_primarymaterial,facet_high_cardinality_color,facet_disjunctive_multi_dim
    SERVER_ARGS="$ALL_STRUCTS --tau-f $TAU_F --rho-s $RHO_S"
    declare -A ORDER=([1]="solr native" [2]="native solr" [3]="solr native")
    AMBIENT="$OUT/solr_sensitivity/ambient.jsonl"
    for run in $RUNS; do
      RUN_DIR="$OUT/solr_sensitivity/run${run}"
      mkdir -p "$RUN_DIR/dumps"
      modes="hybrid:hybrid,hybrid:hybrid:p0r"
      (( run % 2 == 0 )) && modes="hybrid:hybrid:p0r,hybrid:hybrid"
      for arm in ${ORDER[$run]}; do
        ambient "$run" "$arm" before "$AMBIENT"
        log "solr_sensitivity run=$run arm=$arm"
        case "$arm" in
          native)
            I63_DUMP_DIR="$RUN_DIR/dumps" taskset -c "$I77_DRIVER_CPU" "$BIN/e3b_native_measure" \
              --repository-root "$REPO_ROOT" --server-binary "$BIN/e3b_native_plp_server" \
              --catalog "$CATALOG_500K" --label i63_solr_sensitivity --run "$run" \
              --out "$RUN_DIR/native_500k.json" --cells "$FH" --modes "$modes" \
              --server-args "$SERVER_ARGS"
            ;;
          solr)
            e3b_scope_stop "$I77_SOLR_CONTAINER"
            I63_EQUAL_WORK=1 I63_SOLR_FACET_SORT=index I63_DUMP_DIR="$RUN_DIR/dumps" \
              taskset -c "$I77_DRIVER_CPU" "$BIN/i77_measure" --engine solr --tier 500k --run "$run" \
              --repository-root "$REPO_ROOT" --out "$RUN_DIR/solr_500k.json" \
              --cells "$FH" --skip-throughput true
            e3b_scope_stop "$I77_SOLR_CONTAINER"
            ;;
        esac
        echo "arm_exit=$? run=$run arm=$arm"
        ambient "$run" "$arm" after "$AMBIENT"
      done
      "$BIN/i63_facet_equivalence" --catalog "$CATALOG_500K" --dump-dir "$RUN_DIR/dumps" \
        --out "$RUN_DIR/equivalence.json" >"$RUN_DIR/equivalence.log" 2>&1
      echo "equivalence_exit=$?" >>"$RUN_DIR/equivalence.log"
      grep -E 'NOT_EQUIVALENT|UNREADABLE|I63_EQUIVALENCE' "$RUN_DIR/equivalence.log"
    done
    ;;
  *)
    echo "unknown phase $phase" >&2
    exit 2
    ;;
esac
log "phase $phase done"
