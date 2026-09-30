# shellcheck shell=bash
# Issue #79 (Infra E3b): Docker-free equivalent of #77's frozen resource
# envelope, for hosts without Docker (see #79 section 2, "Environment
# amendment"). Sourced by scripts/issue77/provision_*.sh when
# I77_RUNTIME=scope, and by scripts/issue79/*.sh.
#
# Each engine runs as a transient user cgroup-v2 scope:
#   systemd-run --user --scope -p CPUQuota=<I77_CPUS*100>% \
#     -p MemoryMax=<I77_MEMORY> -p MemorySwapMax=0 taskset -c <I77_CPUSET> ...
# i.e. the same kernel knobs Docker's --cpus/--memory/--memory-swap(=memory)
# set; the cpuset is enforced by CPU affinity (taskset) because the cpuset
# controller is not delegated to user slices on this host. The unit name is
# the engine's #77 container name, so i77_measure finds the cgroup by name.
#
# Requires I77_* variables from benchmarks/configs/issue77/resource_envelope.env
# to be sourced first.

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
if [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" || "${DBUS_SESSION_BUS_ADDRESS}" == disabled:* ]]; then
  export DBUS_SESSION_BUS_ADDRESS="unix:path=${XDG_RUNTIME_DIR}/bus"
fi

E3B_ENGINE_ROOT="${E3B_ENGINE_ROOT:-$HOME/.cache/e3b-engines}"
E3B_DATA_ROOT="${E3B_DATA_ROOT:-$HOME/.cache/e3b-data}"

# Docker's --memory takes e.g. "6g"; systemd wants "6G".
e3b_systemd_bytes() {
  echo "$1" | tr '[:lower:]' '[:upper:]'
}

e3b_scope_stop() {
  local unit="$1"
  systemctl --user stop "${unit}.scope" >/dev/null 2>&1 || true
  # A scope whose processes already exited can linger as "failed".
  systemctl --user reset-failed "${unit}.scope" >/dev/null 2>&1 || true
}

# e3b_scope_start <unit> <logfile> <command...>
# Starts <command> detached inside a fresh scope and returns immediately.
e3b_scope_start() {
  local unit="$1" logfile="$2"
  shift 2
  e3b_scope_stop "$unit"
  local quota=$(( I77_CPUS * 100 ))
  # Issue #65: E3B_SLICE places the scope inside an aggregate serving slice
  # (whose own CPUQuota/MemoryMax bind all scopes in it together), and
  # E3B_SCOPE_MEMORY overrides the per-scope ceiling. Both unset for #77/#79.
  local slice_args=()
  [[ -n "${E3B_SLICE:-}" ]] && slice_args=(--slice="$E3B_SLICE")
  nohup systemd-run --user --scope --quiet --unit="$unit" "${slice_args[@]}" \
    -p "CPUQuota=${quota}%" \
    -p "MemoryMax=$(e3b_systemd_bytes "${E3B_SCOPE_MEMORY:-$I77_MEMORY}")" \
    -p "MemorySwapMax=0" \
    taskset -c "$I77_CPUSET" "$@" >"$logfile" 2>&1 &
  # Wait until the scope unit exists so callers can read its cgroup.
  local _
  for _ in $(seq 1 50); do
    if systemctl --user is-active --quiet "${unit}.scope"; then
      return 0
    fi
    sleep 0.2
  done
  echo "FATAL: scope ${unit}.scope did not start; log follows" >&2
  cat "$logfile" >&2
  return 1
}

e3b_scope_cgroup_dir() {
  local unit="$1"
  echo "/sys/fs/cgroup$(systemctl --user show -p ControlGroup --value "${unit}.scope")"
}
