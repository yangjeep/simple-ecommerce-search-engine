#!/usr/bin/env bash
# Issue #64 (Infra E4, amendment 1): facet-heavy economics phases, in the
# preregistered order (amendment section 4):
#
#   bash scripts/issue64/run_i64.sh gate      # correctness gate incl. the 44 #64 cells
#   bash scripts/issue64/run_i64.sh confirm   # 3x3 Latin square, 3 runs
#
# #79's scope envelope; drivers on CPU 3. No cargo while a phase runs; this
# script is not edited while a phase runs.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue77/resource_envelope.env
source "$REPO_ROOT/benchmarks/configs/issue77/resource_envelope.env"
# shellcheck source=../issue79/scope_runtime.sh
source "$REPO_ROOT/scripts/issue79/scope_runtime.sh"
export I77_RUNTIME=scope
export I64_CELLS=1

OUT="$REPO_ROOT/artifacts/issue64/results"
BIN="$REPO_ROOT/target/release"
CATALOG_500K="$REPO_ROOT/$I77_CATALOG_500K_PATH"
TAU_F=919.4972390657695
RHO_S=0.0833779131971903
ALL_STRUCTS="--sort-columns average_rating,review_count,rating_count --presence average_rating,review_count,rating_count"
NATIVE_MODE="hybrid:hybrid:p0r"   # N+ = #79 FINAL + #63 P0r
RUNS="${I64_RUNS:-1 2 3}"
phase="${1:?usage: run_i64.sh <gate|confirm>}"
mkdir -p "$OUT/$phase"

log() { echo "=== $(date -u +%FT%TZ) $* ==="; }

# All 44 #64 cell names, from the shared definition (via the native driver's
# own selection, so both harnesses use the same list).
CELLS="$(python3 - <<'PY'
scopes = {"s0": 0, "s1": 1, "s2": 1, "s3": 2, "s4": 3, "s5": 3}
names = []
for s, d in scopes.items():
    kmax = 11 if s == "s0" else 5 + 6 - d
    ks = sorted({k for k in (1, 3, 5, 8, kmax) if k <= kmax})
    names += [f"i64_{s}_k0"] + [f"i64_{s}_k{k}" for k in ks] + [f"i64_{s}_color"]
    if s in ("s2", "s3"):
        names += [f"i64_{s}_k5_style1", f"i64_{s}_k5_style2"]
print(",".join(names))
PY
)"
[[ "$(tr ',' '\n' <<<"$CELLS" | wc -l)" == 44 ]] || { echo "FATAL: expected 44 cells"; exit 1; }

ambient() {
  local run="$1" arm="$2" when="$3" file="$4" probe
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
    log "gate tier=500k"
    { time "$BIN/e3b_correctness_gate" --catalog "$CATALOG_500K" --out "$OUT/gate/gate_500k.json" \
        --tau-f "$TAU_F" --rho-s "$RHO_S" --cand-modes p0,p0r --include-i64 true; } \
      >"$OUT/gate/gate_500k.log" 2>&1
    echo "exit=$?" >>"$OUT/gate/gate_500k.log"
    grep -E 'E3B_GATE|CANDIDATE_FAILURE|exit=' "$OUT/gate/gate_500k.log"
    ;;
  confirm)
    declare -A ORDER=([1]="native meilisearch solr" [2]="solr native meilisearch" [3]="meilisearch solr native")
    AMBIENT="$OUT/confirm/ambient.jsonl"
    for run in $RUNS; do
      RUN_DIR="$OUT/confirm/run${run}"
      mkdir -p "$RUN_DIR"
      for arm in ${ORDER[$run]}; do
        DUMPS="$RUN_DIR/dumps_${arm}"
        mkdir -p "$DUMPS"
        ambient "$run" "$arm" before "$AMBIENT"
        log "confirm run=$run arm=$arm"
        case "$arm" in
          native)
            I63_DUMP_DIR="$DUMPS" taskset -c "$I77_DRIVER_CPU" "$BIN/e3b_native_measure" \
              --repository-root "$REPO_ROOT" --server-binary "$BIN/e3b_native_plp_server" \
              --catalog "$CATALOG_500K" --label i64_confirm --run "$run" \
              --out "$RUN_DIR/native_500k.json" --cells "$CELLS" --modes "$NATIVE_MODE" \
              --server-args "$ALL_STRUCTS --tau-f $TAU_F --rho-s $RHO_S"
            ;;
          meilisearch)
            e3b_scope_stop "$I77_MEILISEARCH_CONTAINER"
            I77_MEILI_LIKE_FOR_LIKE=1 I63_DUMP_DIR="$DUMPS" taskset -c "$I77_DRIVER_CPU" \
              "$BIN/i77_measure" --engine meilisearch --tier 500k --run "$run" \
              --repository-root "$REPO_ROOT" --out "$RUN_DIR/meilisearch_500k.json" \
              --cells "$CELLS" --skip-throughput true
            e3b_scope_stop "$I77_MEILISEARCH_CONTAINER"
            ;;
          solr)
            e3b_scope_stop "$I77_SOLR_CONTAINER"
            I63_EQUAL_WORK=1 I63_DUMP_DIR="$DUMPS" taskset -c "$I77_DRIVER_CPU" \
              "$BIN/i77_measure" --engine solr --tier 500k --run "$run" \
              --repository-root "$REPO_ROOT" --out "$RUN_DIR/solr_500k.json" \
              --cells "$CELLS" --skip-throughput true
            e3b_scope_stop "$I77_SOLR_CONTAINER"
            ;;
        esac
        echo "arm_exit=$? run=$run arm=$arm"
        ambient "$run" "$arm" after "$AMBIENT"
        # Equal-work verification of this arm before the next arm starts.
        "$BIN/i63_facet_equivalence" --catalog "$CATALOG_500K" --dump-dir "$DUMPS" \
          --out "$RUN_DIR/equivalence_${arm}.json" >"$RUN_DIR/equivalence_${arm}.log" 2>&1
        echo "equivalence_exit=$? run=$run arm=$arm" >>"$RUN_DIR/equivalence_${arm}.log"
        grep -E 'NOT_EQUIVALENT|UNREADABLE|I63_EQUIVALENCE|equivalence_exit' "$RUN_DIR/equivalence_${arm}.log"
      done
    done
    ;;
  *)
    echo "unknown phase $phase" >&2
    exit 2
    ;;
esac
log "phase $phase done"
