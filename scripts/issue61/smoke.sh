#!/usr/bin/env bash
# Issue #61 (Infra E1) scenario S6 -- live smoke test.
#
# This is the evidence that the campaign's central measurement mechanism
# actually works on this host, not just in unit tests against fixture trees.
# Unit tests can prove `cpu.stat` is PARSED correctly; only this can prove the
# right cgroup is being READ and that it moves when the engine does work.
#
# Deliberately NOT a `cargo test`: this repository forbids tests that require a
# live service (no CI path may depend on Solr being up). It is run manually and
# its output is archived with the experiment.
#
# Asserts, in order:
#   1. the provisioned container exists and is running
#   2. its cgroup v2 path resolves from /proc/<pid>/cgroup
#   3. the frozen resource limits are ACTUALLY applied (not silently ignored)
#   4. swap is genuinely disabled for the container
#   5. a real query returns a plausible result set
#   6. cgroup CPU accounting MOVES when the engine does work
#
# Usage: bash scripts/issue61/smoke.sh [core]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue61/container_limits.env
source "$REPO_ROOT/benchmarks/configs/issue61/container_limits.env"

CORE="${1:-i61_wands}"
CORE_URL="http://localhost:${I61_SOLR_PORT}/solr/$CORE"
FAILURES=0

fail() { echo "  FAIL: $*" >&2; FAILURES=$((FAILURES + 1)); }
ok()   { echo "  ok: $*"; }

# --- 1. container running ---------------------------------------------------
echo "==> [1/6] container $I61_SOLR_CONTAINER is running"
if ! docker ps --format '{{.Names}}' | grep -qx "$I61_SOLR_CONTAINER"; then
  echo "FATAL: container $I61_SOLR_CONTAINER is not running." >&2
  echo "  run: bash scripts/issue61/provision_solr.sh wands" >&2
  exit 2
fi
PID="$(docker inspect --format '{{.State.Pid}}' "$I61_SOLR_CONTAINER")"
ok "pid=$PID"

# --- 2. cgroup path resolution ----------------------------------------------
# Resolved from /proc/<pid>/cgroup rather than a guessed path template, because
# the layout differs between Docker's cgroupfs and systemd drivers.
echo "==> [2/6] cgroup v2 path resolves"
CG_REL="$(grep '^0::' "/proc/$PID/cgroup" | cut -d: -f3)"
if [[ -z "$CG_REL" ]]; then
  echo "FATAL: no cgroup v2 (0::) line in /proc/$PID/cgroup -- host is not cgroup v2" >&2
  exit 3
fi
CG="/sys/fs/cgroup$CG_REL"
[[ -r "$CG/cpu.stat" ]] || fail "cpu.stat unreadable at $CG"
ok "$CG"

# --- 3. frozen limits are actually applied ----------------------------------
# Docker silently ignoring a limit flag would make every subsequent number a
# measurement of an unconstrained engine.
echo "==> [3/6] frozen limits applied"
CPU_MAX="$(cat "$CG/cpu.max")"
EXPECTED_QUOTA=$(awk -v c="$I61_CPUS" 'BEGIN{printf "%d", c*100000}')
if [[ "$CPU_MAX" != "$EXPECTED_QUOTA 100000" ]]; then
  fail "cpu.max is '$CPU_MAX', expected '$EXPECTED_QUOTA 100000' (I61_CPUS=$I61_CPUS)"
else
  ok "cpu.max=$CPU_MAX (= $I61_CPUS CPUs)"
fi
MEM_MAX="$(cat "$CG/memory.max")"
EXPECTED_MEM=$(( ${I61_MEMORY%g} * 1024 * 1024 * 1024 ))
if [[ "$MEM_MAX" != "$EXPECTED_MEM" ]]; then
  fail "memory.max is $MEM_MAX, expected $EXPECTED_MEM ($I61_MEMORY)"
else
  ok "memory.max=$MEM_MAX ($I61_MEMORY)"
fi
CPUSET="$(cat "$CG/cpuset.cpus.effective")"
if [[ "$CPUSET" != "$I61_CPUSET" ]]; then
  fail "cpuset.cpus.effective is '$CPUSET', expected '$I61_CPUSET'"
else
  ok "cpuset=$CPUSET"
fi

# --- 4. swap genuinely disabled ---------------------------------------------
# The host has 4 GiB of swap. If the container can swap, every RSS number the
# campaign's >=25% materiality bar depends on is meaningless.
echo "==> [4/6] swap disabled for the container"
SWAP_MAX="$(cat "$CG/memory.swap.max" 2>/dev/null || echo "unavailable")"
if [[ "$SWAP_MAX" != "0" ]]; then
  fail "memory.swap.max is '$SWAP_MAX', expected 0 -- swap would mask memory pressure"
else
  ok "memory.swap.max=0"
fi

# --- 5. a real query answers -------------------------------------------------
echo "==> [5/6] a real query returns results"
NUM_FOUND="$(curl -sf "$CORE_URL/select?q=*:*&rows=0&wt=json" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["response"]["numFound"])')"
if [[ "$NUM_FOUND" -le 0 ]]; then
  fail "numFound=$NUM_FOUND -- the core is empty"
else
  ok "numFound=$NUM_FOUND"
fi

# --- 6. cgroup CPU accounting moves under load ------------------------------
# THE critical assertion. Everything above could pass while cpu.stat stayed
# frozen -- which would mean the campaign is reading the wrong cgroup and every
# CPU/query number would be zero or nonsense.
echo "==> [6/6] cgroup CPU accounting moves under load"
BEFORE="$(awk '/^usage_usec/{print $2}' "$CG/cpu.stat")"
for _ in $(seq 1 200); do
  curl -sf "$CORE_URL/select?q=chair&rows=10&wt=json" >/dev/null
done
AFTER="$(awk '/^usage_usec/{print $2}' "$CG/cpu.stat")"
DELTA=$(( AFTER - BEFORE ))
echo "  cpu_delta_usec=$DELTA (200 queries)"
if [[ "$DELTA" -le 0 ]]; then
  fail "cpu_delta_usec=$DELTA -- CPU accounting did not move; wrong cgroup?"
else
  ok "cpu_delta_usec>0"
  echo "  cpu_usec_per_query=$(( DELTA / 200 ))"
fi

MEM_CURRENT="$(cat "$CG/memory.current")"
MEM_PEAK="$(cat "$CG/memory.peak" 2>/dev/null || echo 0)"
echo "  memory.current=$MEM_CURRENT memory.peak=$MEM_PEAK"

echo
if [[ "$FAILURES" -ne 0 ]]; then
  echo "SMOKE_FAILED failures=$FAILURES" >&2
  exit 1
fi
echo "SMOKE_OK core=$CORE docs=$NUM_FOUND cpu_delta_usec=$DELTA cgroup=$CG"
