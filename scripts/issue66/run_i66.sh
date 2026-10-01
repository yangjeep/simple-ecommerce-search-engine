#!/usr/bin/env bash
# Issue #66 (Infra E6): fixed workload -> minimum CPU/RAM envelope.
# Reuses Issue #65's frozen serving treatments (B0 Solr-only; H1 native N1 +
# Solr delegate), pools (artifacts/issue65/workload/pools_frozen.json),
# load generator and slice (i65-serving.slice, CPUs 0-2, generator on CPU 3).
# The envelope is varied live on the slice (CPUQuota / MemoryMax).
#
#   bash scripts/issue66/run_i66.sh pilot   # protocol-feasibility pilot (no SLO verdicts)
#   bash scripts/issue66/run_i66.sh prepass     # Solr equivalence at non-3g heaps (1g, 512m)
#   bash scripts/issue66/run_i66.sh cpu         # CPU descents: primary mix x T1/T2/T3 x {b0,h1}
#   bash scripts/issue66/run_i66.sh cpusens     # CPU descents: structural/lexical mixes at T2 (descent only)
#   bash scripts/issue66/run_i66.sh mem         # RAM descents: T2, 3 cores, heap ladder x {b0,h1}
#   bash scripts/issue66/run_i66.sh cpuconfirm  # 3 fresh launches per candidate (+ one step-up round)
#   bash scripts/issue66/run_i66.sh memconfirm  # same for RAM candidates
#   bash scripts/issue66/run_i66.sh joint       # (min cores, min GiB, best heap) at T2
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

OUT="$REPO_ROOT/artifacts/issue66/results"
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
phase="${1:?usage: run_i66.sh <phase>}"
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
  # Clarification C1a: a search whose first point (20 QPS) fails brackets
  # downward (20/1.5^k to 1.8 QPS) to its first PASS; none -> Q* = 0.
  if [[ -z "$pass" ]]; then
    for r in 13.3 8.9 5.9 4.0 2.6 1.8; do
      res=$(point "$kind" "$mix" "$r" "$dir/rate_${r}.json" "$@")
      echo "down $kind $mix $r $res" | tee -a "$dir/search.log"
      if [[ "$res" == PASS ]]; then pass="$r"; break; fi
      fail="$r"
    done
    [[ -z "$pass" ]] && pass=0
  fi
  if [[ -n "$pass" && "$pass" != 0 && -n "$fail" ]]; then
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

# envelope <cores> <mem>: set the shared slice envelope live.
envelope() {
  systemctl --user set-property --runtime "$E3B_SLICE" CPUQuota="$(python3 -c "print(int(round($1*100)))")%" MemoryMax="$2" MemorySwapMax=0
}

# idle_probe <out> <seconds>: slice CPU (cores) in 10 s steps with no load.
idle_probe() {
  local out="$1" secs="$2" dir prev now t=0
  dir="$(cg "$E3B_SLICE")"
  prev=$(awk '/^usage_usec/{print $2}' "$dir/cpu.stat")
  while (( t < secs )); do
    sleep 10; t=$((t + 10))
    now=$(awk '/^usage_usec/{print $2}' "$dir/cpu.stat")
    python3 -c "import json,sys; print(json.dumps({'t_s': $t, 'cores': ($now-$prev)/1e6/10, 'memory_current': int(open('$dir/memory.current').read())}))" >>"$out"
    prev=$now
  done
}

CPU_LEVELS=(3 2.5 2 1.5 1 0.75 0.5)
MEM_LEVELS=(12 8 7 6 5 4 3 2 1.5 1)
JUDGE="python3 $REPO_ROOT/scripts/issue66/judge.py"
PLAN="python3 $REPO_ROOT/scripts/issue66/plan.py"
CAND="python3 $REPO_ROOT/scripts/issue66/candidates.py"

SETTLE="${I66_SETTLE:-60}"; WINDOW="${I66_WINDOW:-120}"; WARMUP="${I66_WARMUP:-300}"

mem_arg() { python3 -c "print(f'{int(round($1 * 1024))}M')"; }

alive() {
  local kind="$1"
  systemctl --user is-active --quiet "$I77_SOLR_CONTAINER.scope" || return 1
  if [[ "$kind" == h1 ]]; then systemctl --user is-active --quiet i65-native.scope || return 1; fi
  return 0
}

# quiesce <out>: wait (<= 900 s) for slice idle CPU < 0.1 cores over 30 s.
quiesce() {
  local out="$1" dir a b c t=0
  dir="$(cg "$E3B_SLICE")"
  while (( t < 900 )); do
    a=$(awk '/^usage_usec/{print $2}' "$dir/cpu.stat"); sleep 30; t=$((t + 30))
    b=$(awk '/^usage_usec/{print $2}' "$dir/cpu.stat")
    c=$(python3 -c "print(($b - $a) / 1e6 / 30)")
    echo "{\"t_s\": $t, \"idle_cores\": $c}" >>"$out"
    python3 -c "import sys; sys.exit(0 if $c < 0.1 else 1)" && return 0
  done
  echo "{\"quiesce\": \"timeout\"}" >>"$out"
  return 0
}

# fresh <kind> <heap> <mix> <rate> <dir>: fresh launch at 3 cores / 12 GiB,
# quiescence check, 300 s unmeasured warm-up at the tier rate.
fresh() {
  local kind="$1" heap="$2" mix="$3" rate="$4" dir="$5"
  export I66_SOLR_HEAP="$heap"
  envelope 3 12G
  launch "$kind" 3 "$dir/launch" || return 1
  quiesce "$dir/quiesce.jsonl"
  point "$kind" "$mix" "$rate" "$dir/warmup_300.json" --warmup 0 --duration "$WARMUP" >/dev/null
}

# window <kind> <mix> <rate> <out>: 60 s settle + 120 s measured window at the
# current envelope; writes a sidecar (<out>.env.json) and prints the judgement.
window() {
  local kind="$1" mix="$2" rate="$3" out="$4" dir thr0 thr1 oom0 oom1 live=true
  dir="$(cg "$E3B_SLICE")"
  thr0=$(awk '/^throttled_usec/{print $2}' "$dir/cpu.stat")
  oom0=$(awk '/^oom_kill /{print $2}' "$dir/memory.events")
  point "$kind" "$mix" "$rate" "$out" --warmup "$SETTLE" --duration "$WINDOW" >/dev/null
  thr1=$(awk '/^throttled_usec/{print $2}' "$dir/cpu.stat")
  oom1=$(awk '/^oom_kill /{print $2}' "$dir/memory.events")
  alive "$kind" || live=false
  python3 - "$dir" "$thr0" "$thr1" "$oom0" "$oom1" "$live" "$((SETTLE + WINDOW))" >"${out%.json}.env.json" <<'PY'
import json, sys
d, thr0, thr1, oom0, oom1, live = sys.argv[1:7]
read = lambda f: open(f"{d}/{f}").read().strip()
stat = dict(l.split() for l in read("memory.stat").splitlines())
peak = None
try:
    peak = int(read("memory.peak"))
except OSError:
    pass
print(json.dumps({"cpu_max": read("cpu.max"), "memory_max": read("memory.max"),
                  "throttled_share": (int(thr1) - int(thr0)) / 1e6 / float(sys.argv[7]),
                  "oom_kill_delta": int(oom1) - int(oom0), "alive": live == "true",
                  "memory_current": int(read("memory.current")), "memory_peak": peak,
                  "anon": int(stat.get("anon", 0)), "file": int(stat.get("file", 0))}))
PY
  $JUDGE "$out"
}

# set_level <dim> <level>: cpu -> quota at 12 GiB; mem -> GiB at 3 cores.
set_level() {
  if [[ "$1" == cpu ]]; then envelope "$2" 12G; else envelope 3 "$(mem_arg "$2")"; fi
}

# descend <kind> <dim> <mix> <rate> <heap> <dir>
descend() {
  local kind="$1" dim="$2" mix="$3" rate="$4" heap="$5" dir="$6" levels fails=0 L r
  mkdir -p "$dir"; : >"$dir/descent.tsv"
  fresh "$kind" "$heap" "$mix" "$rate" "$dir" || { echo "LAUNCH_FAILED" >"$dir/INVALID"; return 1; }
  ambient "descend_${kind}_${dim}_${mix}_${rate}_${heap}" before "$AMBIENT"
  if [[ "$dim" == cpu ]]; then levels=("${CPU_LEVELS[@]}"); else levels=("${MEM_LEVELS[@]}"); fi
  for L in "${levels[@]}"; do
    set_level "$dim" "$L"
    r=$(window "$kind" "$mix" "$rate" "$dir/level_${L}.json")
    echo "$L $(echo "$r" | sed -E 's/S1=([A-Z]+) S2=([A-Z]+) mode=(.*)/\1 \2 \3/') level_${L}.json" >>"$dir/descent.tsv"
    echo "descend $kind $dim $mix t$rate heap$heap $L $r" | tee -a "$OUT/$phase/progress.log"
    if [[ "$r" == *"S1=FAIL S2=FAIL"* ]]; then fails=$((fails + 1)); else fails=0; fi
    alive "$kind" || break
    (( fails >= 2 )) && break
  done
  ambient "descend_${kind}_${dim}_${mix}_${rate}_${heap}" after "$AMBIENT"
  envelope 3 12G
}

# runjobs <stage>: 3 counterbalanced rounds (round 2 reversed) of fresh launches.
runjobs() {
  local stage="$1" jobs="$OUT/$1/jobs.tsv" round order id kind dim mix rate heap cores mem slos st r d
  [[ -s "$jobs" ]] || { echo "no jobs for $stage"; return 0; }
  for round in 1 2 3; do
    if [[ $round == 2 ]]; then order=$(tac "$jobs"); else order=$(cat "$jobs"); fi
    while IFS=$'\t' read -r id kind dim mix rate heap cores mem slos st; do
      d="$OUT/$stage/$id"; mkdir -p "$d"
      fresh "$kind" "$heap" "$mix" "$rate" "$d/launch$round" || { echo "$id run$round LAUNCH_FAILED" | tee -a "$OUT/$stage/progress.log"; continue; }
      envelope "$cores" "$(mem_arg "$mem")"
      r=$(window "$kind" "$mix" "$rate" "$d/run$round.json")
      echo "$stage $id run$round cores=$cores mem=$mem heap=$heap $r" | tee -a "$OUT/$stage/progress.log"
      envelope 3 12G
    done <<<"$order"
  done
}

throttle() { grep -E "nr_throttled|throttled_usec" "$(cg "$E3B_SLICE")/cpu.stat" | tr '\n' ' '; echo; }

case "$phase" in
  pilot)
    # Protocol-feasibility pilot (declared before the #66 amendment; no SLO
    # verdict is read from it). Questions: (1) does a freshly provisioned,
    # force-merged Solr burn CPU at idle, and for how long; (2) do live
    # CPUQuota changes bind; (3) does Solr start/serve correctly at a 1g heap.
    P="$OUT/pilot"
    for heap in 3g 1g; do
      export I66_SOLR_HEAP=$heap
      envelope 3 12G
      launch b0 3 "$P/b0_heap$heap/launch" || exit 1
      ambient "pilot_b0_$heap" before "$AMBIENT"
      log "idle probe after provisioning (heap $heap)"
      idle_probe "$P/b0_heap$heap/idle_after_provision.jsonl" 300
      point b0 primary 50 "$P/b0_heap$heap/load_50.json" --duration 60 >/dev/null
      log "idle probe after load (heap $heap)"
      idle_probe "$P/b0_heap$heap/idle_after_load.jsonl" 180
      if [[ $heap == 3g ]]; then
        for q in 1 0.5; do
          envelope "$q" 12G
          echo "quota $q before: $(throttle)" >>"$P/b0_heap$heap/throttle.txt"
          point b0 primary 20 "$P/b0_heap$heap/quota_${q}_20.json" --duration 30 >/dev/null
          echo "quota $q after: $(throttle)" >>"$P/b0_heap$heap/throttle.txt"
          cat "$(cg "$E3B_SLICE")/cpu.max" >>"$P/b0_heap$heap/throttle.txt"
        done
        envelope 3 12G
      else
        taskset -c "$I77_DRIVER_CPU" "$BIN/i65_validate" --pools "$POOLS" --solr "$SOLR" \
          --report "$P/b0_heap$heap/validate_solr.json" >"$P/b0_heap$heap/validate_solr.txt" 2>&1; echo "validate exit $?" >>"$P/b0_heap$heap/validate_solr.txt"
      fi
      ambient "pilot_b0_$heap" after "$AMBIENT"
    done
    unset I66_SOLR_HEAP
    ;;
  smoke)
    # Harness smoke test only (no verdict): short windows, 2 CPU levels and
    # 2 memory levels on h1, then one confirmation job through runjobs.
    export I66_SETTLE=5 I66_WINDOW=10 I66_WARMUP=10
    SETTLE=5; WINDOW=10; WARMUP=10
    CPU_LEVELS=(3 1); MEM_LEVELS=(12 3)
    descend h1 cpu primary 50 3g "$OUT/smoke/cpu/h1"
    descend h1 mem primary 50 3g "$OUT/smoke/mem/h1"
    mkdir -p "$OUT/smoke_jobs"
    printf 'j1\th1\tcpu\tprimary\t50\t3g\t1\t12\tS1,S2\tsmoke_jobs\n' >"$OUT/smoke_jobs/jobs.tsv"
    runjobs smoke_jobs
    ;;
  prepass)
    # Section 8 + C7a: at every heap (3g = fresh-index control), the #65
    # structural Solr pre-pass plus a tie-aware F record (ids + scores). A
    # non-3g heap is EQUIVALENT when its structural pre-pass is clean and its
    # F responses are tie-aware equivalent to the fresh 3g control's.
    FEQ="python3 $REPO_ROOT/scripts/issue66/f_equiv.py"
    for heap in 3g 1g 512m; do
      d="$OUT/prepass/heap$heap"; mkdir -p "$d"
      export I66_SOLR_HEAP=$heap
      envelope 3 12G
      launch b0 3 "$d/launch" || { echo "EXCLUDED launch_failed" >"$d/RESULT"; cat "$d/RESULT"; continue; }
      taskset -c "$I77_DRIVER_CPU" "$BIN/i65_validate" --pools "$POOLS" --solr "$SOLR" \
        --report "$d/validate.json" >"$d/validate.txt" 2>&1
      taskset -c "$I77_DRIVER_CPU" $FEQ record "$SOLR" "$POOLS" "$d/f_scores.json" >>"$d/validate.txt" 2>&1
      $FEQ frozen "$d/f_scores.json" "$POOLS" >"$d/vs_frozen.txt" 2>&1
      bad=$(python3 -c "import json; r=json.load(open('$d/validate.json')); print(len(r['solr_not_equivalent']) + len(r['excluded']))" 2>/dev/null || echo 999)
      if [[ $heap == 3g ]]; then
        echo "CONTROL structural_not_equivalent=$bad $(cat "$d/vs_frozen.txt")" >"$d/RESULT"
      else
        f=$($FEQ compare "$OUT/prepass/heap3g/f_scores.json" "$d/f_scores.json" 2>&1)
        if [[ "$bad" == 0 && "$f" == EQUIVALENT* ]]; then v=EQUIVALENT; else v=EXCLUDED; fi
        echo "$v structural_not_equivalent=$bad f: $f" >"$d/RESULT"
      fi
      cat "$d/RESULT"
    done
    unset I66_SOLR_HEAP
    ;;
  cpu)
    # Counterbalanced treatment order per tier.
    descend b0 cpu primary 50 3g "$OUT/cpu/primary/t50/b0"
    descend h1 cpu primary 50 3g "$OUT/cpu/primary/t50/h1"
    descend h1 cpu primary 100 3g "$OUT/cpu/primary/t100/h1"
    descend b0 cpu primary 100 3g "$OUT/cpu/primary/t100/b0"
    descend b0 cpu primary 200 3g "$OUT/cpu/primary/t200/b0"
    descend h1 cpu primary 200 3g "$OUT/cpu/primary/t200/h1"
    ;;
  cpusens)
    descend h1 cpu structural 100 3g "$OUT/cpu/structural/t100/h1"
    descend b0 cpu structural 100 3g "$OUT/cpu/structural/t100/b0"
    descend b0 cpu lexical 100 3g "$OUT/cpu/lexical/t100/b0"
    descend h1 cpu lexical 100 3g "$OUT/cpu/lexical/t100/h1"
    ;;
  mem)
    for heap in 3g 1g 512m; do
      if [[ $heap != 3g ]] && ! grep -q '^EQUIVALENT' "$OUT/prepass/heap$heap/RESULT" 2>/dev/null; then
        for k in b0 h1; do mkdir -p "$OUT/mem/heap$heap/$k"; echo "prepass not EQUIVALENT" >"$OUT/mem/heap$heap/$k/HEAP_EXCLUDED"; done
        continue
      fi
      if [[ $heap == 1g ]]; then order=(h1 b0); else order=(b0 h1); fi
      for k in "${order[@]}"; do descend "$k" mem primary 100 "$heap" "$OUT/mem/heap$heap/$k"; done
    done
    ;;
  cpuconfirm)
    mkdir -p "$OUT/cpu_confirm"
    $PLAN cpu-confirm "$OUT" >"$OUT/cpu_confirm/jobs.tsv"
    runjobs cpu_confirm
    mkdir -p "$OUT/cpu_confirm_stepup"
    $PLAN stepup cpu_confirm "$OUT" >"$OUT/cpu_confirm_stepup/jobs.tsv"
    runjobs cpu_confirm_stepup
    ;;
  memconfirm)
    mkdir -p "$OUT/mem_confirm"
    $PLAN mem-confirm "$OUT" >"$OUT/mem_confirm/jobs.tsv"
    runjobs mem_confirm
    mkdir -p "$OUT/mem_confirm_stepup"
    $PLAN stepup mem_confirm "$OUT" >"$OUT/mem_confirm_stepup/jobs.tsv"
    runjobs mem_confirm_stepup
    ;;
  joint)
    mkdir -p "$OUT/joint"
    $PLAN joint "$OUT" >"$OUT/joint/jobs.tsv"
    runjobs joint
    $PLAN summary "$OUT" >/dev/null
    ;;
  *)
    echo "unknown phase $phase" >&2
    exit 2
    ;;
esac
stop_all
envelope 3 12G
log "phase $phase done"
