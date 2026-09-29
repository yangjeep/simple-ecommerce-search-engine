#!/usr/bin/env bash
# Issue #65 (Infra E5, amendment 1): mixed-workload capacity phases, in the
# preregistered order:
#
#   bash scripts/issue65/run_i65.sh calibrate     # transport ceilings (diagnostic)
#   bash scripts/issue65/run_i65.sh validate      # correctness pre-pass -> pools_frozen.json
#   bash scripts/issue65/run_i65.sh scaling       # N0 / N1 W=1,2,3 native scaling (diagnostic)
#   bash scripts/issue65/run_i65.sh search        # B0 then H1 capacity search, primary mix
#   bash scripts/issue65/run_i65.sh confirm       # 3 counterbalanced confirmations, primary mix
#   bash scripts/issue65/run_i65.sh sensitivity   # structural + lexical mixes, one search each
#   bash scripts/issue65/run_i65.sh control       # conditional: no-lexical decomposition
#
# Every serving process runs inside ONE user slice (i65-serving.slice:
# CPUQuota=300%, MemoryMax=12G, swap 0), pinned to CPUs 0-2; for H1 that is
# native N1 (also the router) AND Solr together. The load generator runs on
# CPU 3 outside the slice. No cargo while a phase runs; this script is not
# edited while a phase runs.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"
# shellcheck source=../../benchmarks/configs/issue77/resource_envelope.env
source "$REPO_ROOT/benchmarks/configs/issue77/resource_envelope.env"
# shellcheck source=../issue79/scope_runtime.sh
source "$REPO_ROOT/scripts/issue79/scope_runtime.sh"
export I77_RUNTIME=scope
export E3B_SLICE=i65-serving.slice
export E3B_SCOPE_MEMORY=12g

OUT="$REPO_ROOT/artifacts/issue65/results"
BIN="$REPO_ROOT/target/release"
CAT="$REPO_ROOT/$I77_CATALOG_500K_PATH"
POOLS_BASE="$REPO_ROOT/artifacts/issue65/workload/pools_base.json"
POOLS="$REPO_ROOT/artifacts/issue65/workload/pools_frozen.json"
NPORT=9965
NATIVE="http://127.0.0.1:$NPORT"
SOLR="http://127.0.0.1:$I77_SOLR_PORT"
TAU_F=919.4972390657695
RHO_S=0.0833779131971903
ALL_STRUCTS=(--sort-columns average_rating,review_count,rating_count --presence average_rating,review_count,rating_count)
phase="${1:?usage: run_i65.sh <calibrate|validate|scaling|search|confirm|sensitivity|control>}"
mkdir -p "$OUT/$phase"

log() { echo "=== $(date -u +%FT%TZ) $* ==="; }

systemctl --user set-property --runtime "$E3B_SLICE" CPUQuota=300% MemoryMax=12G MemorySwapMax=0

cg() { echo "/sys/fs/cgroup$(systemctl --user show -p ControlGroup --value "$1")"; }

stop_all() {
  e3b_scope_stop i65-native
  e3b_scope_stop "$I77_SOLR_CONTAINER"
}

wait_http() {
  local url="$1" tries="$2"
  for _ in $(seq 1 "$tries"); do
    curl -sf "$url" >/dev/null 2>&1 && return 0
    sleep 1
  done
  echo "FATAL: $url not ready" >&2
  return 1
}

ambient() {
  local arm="$1" when="$2" file="$3" probe
  probe="$(taskset -c "$I77_DRIVER_CPU" "$BIN/i63_host_probe" 2>/dev/null || echo null)"
  python3 - "$arm" "$when" "$probe" >>"$file" <<'PY'
import json, sys, time
arm, when, probe = sys.argv[1:4]
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
print(json.dumps({"arm": arm, "when": when, "utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                  "loadavg": read("/proc/loadavg"),
                  "pressure": {k: read(f"/proc/pressure/{k}") for k in ("cpu", "memory", "io")},
                  "meminfo": meminfo, "host_probe": json.loads(probe) if probe != "null" else None}))
PY
}

# launch <b0|h1|native|n0> <workers> <logdir>: fresh serving processes.
launch() {
  local kind="$1" workers="$2" dir="$3"
  stop_all
  mkdir -p "$dir"
  if [[ "$kind" == b0 || "$kind" == h1 ]]; then
    log "provision solr (fresh index) for $kind"
    bash "$REPO_ROOT/scripts/issue77/provision_solr.sh" "$CAT" "$I77_TIER_500K_DOCS" >"$dir/solr_provision.log" 2>&1 \
      || { echo "FATAL: solr provisioning failed"; tail -20 "$dir/solr_provision.log"; return 1; }
  fi
  case "$kind" in
    h1) e3b_scope_start i65-native "$dir/native.log" "$BIN/i65_server" --catalog "$CAT" --port "$NPORT" \
          --workers 3 --solr-url "$SOLR" ;;
    native) e3b_scope_start i65-native "$dir/native.log" "$BIN/i65_server" --catalog "$CAT" --port "$NPORT" \
          --workers "$workers" ;;
    n0) e3b_scope_start i65-native "$dir/native.log" "$BIN/e3b_native_plp_server" --catalog "$CAT" \
          --dataset wands --port "$NPORT" "${ALL_STRUCTS[@]}" --tau-f "$TAU_F" --rho-s "$RHO_S" ;;
  esac
  if [[ "$kind" == h1 || "$kind" == native ]]; then
    wait_http "$NATIVE/noop" 900 || return 1
    curl -s "$NATIVE/correctness" >"$dir/fixture.json"
  elif [[ "$kind" == n0 ]]; then
    wait_http "$NATIVE/ping" 900 || return 1
  fi
  systemctl --user show -p ControlGroup "$E3B_SLICE" >"$dir/slice.txt"
  cat "$(cg "$E3B_SLICE")/cpu.max" "$(cg "$E3B_SLICE")/memory.max" >>"$dir/slice.txt"
}

cgroup_args() {
  local args=(--cgroup "total=$(cg "$E3B_SLICE")")
  for unit in "$I77_SOLR_CONTAINER" i65-native; do
    if systemctl --user is-active --quiet "$unit.scope"; then
      args+=(--cgroup "$unit=$(cg "$unit.scope")")
    fi
  done
  echo "${args[@]}"
}

# point <launch-kind> <mix> <rate> <out> [extra i65_load args]; prints PASS/FAIL.
point() {
  local kind="$1" mix="$2" rate="$3" out="$4"
  shift 4
  local treatment="$kind" extra=()
  [[ "$kind" == native ]] && treatment=native
  [[ "$kind" == n0 ]] && treatment=native && extra=(--close-each true)
  # shellcheck disable=SC2046
  taskset -c "$I77_DRIVER_CPU" "$BIN/i65_load" --pools "$POOLS" --mix "$mix" --rate "$rate" \
    --treatment "$treatment" --native-url "$NATIVE" --solr-url "$SOLR" --out "$out" \
    $(cgroup_args) "${extra[@]}" "$@" >"${out%.json}.stdout" 2>&1
  python3 -c "import json,sys; d=json.load(open(sys.argv[1])); print('PASS' if d['pass'] else 'FAIL')" "$out" 2>/dev/null || echo FAIL
}

# search <kind> <mix> <dir> [extra]: bracket x1.5 from 20 QPS, then bisect to 5%.
search() {
  local kind="$1" mix="$2" dir="$3"
  shift 3
  mkdir -p "$dir"
  point "$kind" "$mix" 20 "$dir/precondition_20.json" --duration 30 "$@" >/dev/null
  local rate=20 pass="" fail=""
  while :; do
    local r res
    r=$(python3 -c "print(round($rate, 1))")
    res=$(point "$kind" "$mix" "$r" "$dir/rate_${r}.json" "$@")
    echo "search $kind $mix $r $res" | tee -a "$dir/search.log"
    if [[ "$res" == PASS ]]; then
      pass="$r"
      rate=$(python3 -c "print($r * 1.5)")
      if python3 -c "import sys; sys.exit(0 if $rate > 20000 else 1)"; then break; fi
    else
      fail="$r"
      break
    fi
  done
  if [[ -n "$pass" && -n "$fail" ]]; then
    while python3 -c "import sys; sys.exit(0 if ($fail - $pass) / $pass > 0.05 else 1)"; do
      local mid res
      mid=$(python3 -c "print(round(($pass + $fail) / 2, 1))")
      res=$(point "$kind" "$mix" "$mid" "$dir/rate_${mid}.json" "$@")
      echo "refine $kind $mix $mid $res" | tee -a "$dir/search.log"
      if [[ "$res" == PASS ]]; then pass="$mid"; else fail="$mid"; fi
    done
  fi
  echo "${pass:-none}" >"$dir/qstar.txt"
  echo "Q* $kind $mix = ${pass:-none} (first fail ${fail:-none})" | tee -a "$dir/search.log"
}

AMBIENT="$OUT/$phase/ambient.jsonl"

case "$phase" in
  calibrate)
    # Transport-only probes: no expectation is checked, so the base pools
    # (which only supply the schedule) are enough.
    [[ -f "$POOLS" ]] || POOLS="$POOLS_BASE"
    launch h1 3 "$OUT/calibrate/launch" || exit 1
    ambient calibrate before "$AMBIENT"
    # Diagnostic only: shorter points (5 s warm-up + 20 s window).
    CAL=(--warmup 5 --duration 20)
    search h1 primary "$OUT/calibrate/native_noop" --noop /noop "${CAL[@]}"
    search b0 primary "$OUT/calibrate/solr_health" --noop /solr/admin/info/health "${CAL[@]}"
    search h1 primary "$OUT/calibrate/router_to_solr_health" --noop /solr/admin/info/health "${CAL[@]}"
    ambient calibrate after "$AMBIENT"
    ;;
  validate)
    launch h1 3 "$OUT/validate/launch" || exit 1
    "$BIN/i65_validate" --pools "$POOLS_BASE" --out-pools "$POOLS" --report "$OUT/validate/report.json" \
      --solr "$SOLR" --native "$NATIVE" --router "$NATIVE" --threads 32 --rounds 3 | tee "$OUT/validate/validate.log"
    echo "validate_exit=${PIPESTATUS[0]}" | tee -a "$OUT/validate/validate.log"
    ;;
  scaling)
    for cfg in n0 w1 w2 w3; do
      dir="$OUT/scaling/$cfg"
      if [[ "$cfg" == n0 ]]; then launch n0 1 "$dir/launch" || exit 1; kind=n0
      else launch native "${cfg#w}" "$dir/launch" || exit 1; kind=native; fi
      ambient "scaling_$cfg" before "$AMBIENT"
      search "$kind" native_plp "$dir"
      ambient "scaling_$cfg" after "$AMBIENT"
    done
    ;;
  search)
    for t in b0 h1; do
      launch "$t" 3 "$OUT/search/$t/launch" || exit 1
      ambient "search_$t" before "$AMBIENT"
      search "$t" primary "$OUT/search/$t"
      ambient "search_$t" after "$AMBIENT"
    done
    ;;
  confirm)
    QB0=$(cat "$OUT/search/b0/qstar.txt"); QH1=$(cat "$OUT/search/h1/qstar.txt")
    RC=$(python3 -c "print(round(min($QB0, $QH1), 1))")
    echo "Q*_B0=$QB0 Q*_H1=$QH1 R_c=$RC" | tee "$OUT/confirm/plan.txt"
    declare -A ORDER=([1]="b0 h1" [2]="h1 b0" [3]="b0 h1")
    for run in 1 2 3; do
      for t in ${ORDER[$run]}; do
        dir="$OUT/confirm/run$run/$t"
        launch "$t" 3 "$dir/launch" || exit 1
        ambient "confirm_${run}_$t" before "$AMBIENT"
        point "$t" primary 20 "$dir/precondition_20.json" --duration 30 >/dev/null
        q=$QB0; [[ "$t" == h1 ]] && q=$QH1
        for label in rc lo mid hi; do
          case "$label" in
            rc) r=$RC ;;
            lo) r=$(python3 -c "print(round($q * 0.95, 1))") ;;
            mid) r=$q ;;
            hi) r=$(python3 -c "print(round($q * 1.05, 1))") ;;
          esac
          res=$(point "$t" primary "$r" "$dir/${label}_${r}.json")
          echo "confirm run=$run $t $label $r $res" | tee -a "$OUT/confirm/confirm.log"
        done
        ambient "confirm_${run}_$t" after "$AMBIENT"
      done
    done
    ;;
  sensitivity)
    for mix in structural lexical; do
      for t in b0 h1; do
        launch "$t" 3 "$OUT/sensitivity/$mix/$t/launch" || exit 1
        ambient "sens_${mix}_$t" before "$AMBIENT"
        search "$t" "$mix" "$OUT/sensitivity/$mix/$t"
        ambient "sens_${mix}_$t" after "$AMBIENT"
      done
    done
    ;;
  control)
    for t in b0 h1; do
      launch "$t" 3 "$OUT/control/$t/launch" || exit 1
      ambient "control_$t" before "$AMBIENT"
      search "$t" no_lexical "$OUT/control/$t"
      ambient "control_$t" after "$AMBIENT"
    done
    ;;
  *)
    echo "unknown phase $phase" >&2
    exit 2
    ;;
esac
stop_all
log "phase $phase done"
