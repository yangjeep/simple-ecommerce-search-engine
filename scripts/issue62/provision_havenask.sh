#!/usr/bin/env bash
# Issue #62 (Infra E2): attempt to provision Havenask (registry.cn-hangzhou.
# aliyuncs.com/havenask/ha3_runtime) with a WANDS-format catalog and measure
# on-disk index size, matching the calling convention of this directory's
# other provision_*.sh scripts.
#
# STATUS AS OF THIS SESSION: this script cannot reach PROVISION_OK on this
# host. It documents and fails fast on a genuine, reproduced, host-CPU
# architectural blocker -- see docs/experiments/ISSUE57_DATASET_RECOVERY_LOG.md
# for prior recon and the #62 session notes for the full investigation.
#
# What was actually verified this session (do not re-derive from scratch):
#   * The image's docker-registry block from earlier sessions is gone; the
#     image pulls and runs.
#   * hape's DEFAULT single-node config (hape_conf/default) uses
#     processorMode=docker, which shells out (via literal `ssh -t <own-ip>
#     '...'`, see hape_libs/utils/shell.py SSHShell) to a `docker` CLI that
#     does not exist inside this image at all (`which docker` -> not found).
#     That path is a dead end for a plain `docker run` of this one image
#     unless you additionally install a docker CLI, wire up localhost sshd
#     key auth, and mount the host's docker socket in -- a much heavier lift
#     than this experiment's other engines require.
#   * hape ships a SECOND, already-rendered config, hape_conf/proc
#     (processorMode=proc), that runs every Havenask/Swift/BuildService role
#     as a plain OS process inside the single container -- no nested docker
#     needed. This is the right config for a bounded single-container
#     footprint measurement. Using it (point any case's `hape_conf` at
#     /ha3_install/hape_conf/proc instead of .../default), `hape start
#     havenask` + `hape create table` successfully brought up zookeeper,
#     swift_broker, and a swift topic, and created real per-role working
#     directories under /root/havenask-sql-proc_*.
#   * The actual query-serving/indexing binary, ha_sql, crashes on THIS
#     host with SIGILL on every single invocation, including bare
#     `ha_sql --version` with no other setup (exit code 132, "Illegal
#     instruction (core dumped)"). Confirmed with gdb: the crash is inside
#     ailego::internal::CpuFeatures::CpuFlags::CpuFlags() itself (Alibaba's
#     own CPU-feature-detection constructor!) on a VEX-encoded `vpxor`
#     instruction -- i.e. the binary requires baseline AVX and has no
#     software fallback. `strings` on the binary additionally shows it was
#     built targeting "AVX2+FMA+BMI2+F16C" (x86-64-v3). This host's CPU
#     (`lscpu`) is a generic "QEMU Virtual CPU", flags capped at
#     sse4_2/popcnt/aes -- no avx, avx2, fma, or bmi2 at all.
#   * This is a host-CPU compatibility blocker, not a hape/schema/data
#     problem: every Havenask worker binary in this release links the same
#     ailego library and will crash identically, so no config or schema
#     change on this host can get past it. It would take either different
#     host/hypervisor CPU flags (outside this container's or this script's
#     control) or a from-source Havenask rebuild targeting this CPU (a much
#     larger undertaking than this experiment's scope).
#
# Because of that, this script performs the real container launch and then
# a REAL (not just /proc/cpuinfo-heuristic) compatibility probe -- it
# actually executes the Havenask query binary and inspects how it dies --
# before ever attempting schema/data work, so it fails in seconds with a
# precise, reproducible reason instead of hanging through hape's multi-
# minute crash-restart-loop retry budget. If this is ever run on a host
# whose CPU actually has the required baseline, the probe passes and the
# script stops with a clear "not implemented past this point" message: the
# WANDS schema/table-creation/data-load/measurement steps below were never
# reachable this session (blocked immediately at the probe on every host
# available here), so encoding them further without being able to test them
# end-to-end would risk shipping unverified logic as if it were proven --
# exactly what this experiment's rules say not to do. A future session with
# AVX-capable hardware should pick up from the "proc" hape_conf mechanism
# documented above, which IS proven to work up through table creation.
#
# Usage:
#   bash scripts/issue62/provision_havenask.sh <catalog_path> <expected_docs>
#
# On success (only possible on an AVX2/FMA/BMI2/F16C-capable host), the LAST
# line of stdout would be exactly:
#   PROVISION_OK container=<container-name> docs=<count> index_bytes=<bytes>
# On any failure: `FATAL: <reason>` to stderr, non-zero exit.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../benchmarks/configs/issue62/container_limits.env
source "$REPO_ROOT/benchmarks/configs/issue62/container_limits.env"

# --- 0. argument validation -------------------------------------------------
if [[ $# -ne 2 ]]; then
  echo "FATAL: usage: provision_havenask.sh <catalog_path> <expected_docs>" >&2
  exit 2
fi

CATALOG="$1"
EXPECTED_DOCS="$2"

if [[ ! -f "$CATALOG" ]]; then
  echo "FATAL: catalog not found or not a regular file: $CATALOG" >&2
  exit 2
fi

if ! [[ "$EXPECTED_DOCS" =~ ^[1-9][0-9]*$ ]]; then
  echo "FATAL: expected_docs must be a positive integer, got: $EXPECTED_DOCS" >&2
  exit 2
fi

CONTAINER="${I62_HAVENASK_CONTAINER:-i62-havenask}"
IMAGE="${I62_HAVENASK_IMAGE:?I62_HAVENASK_IMAGE not set in container_limits.env}"
# No port was pre-assigned for Havenask in container_limits.env as of this
# session. Havenask's own hape_conf/*/global.conf templates hardcode the QRS
# HTTP (SQL query) port at 45800 -- reusing that default here rather than
# inventing a new one. If this script ever reaches PROVISION_OK, add
# I62_HAVENASK_PORT=45800 to container_limits.env explicitly.
HAVENASK_PORT="${I62_HAVENASK_PORT:-45800}"

cleanup() {
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
}
trap cleanup EXIT

# --- 1. clean slate + launch ------------------------------------------------
echo "==> removing any stale $CONTAINER"
docker rm -f "$CONTAINER" >/dev/null 2>&1 || true

echo "==> starting $CONTAINER from $IMAGE (cpus=$I62_CPUS mem=$I62_MEMORY)"
docker run -d --name "$CONTAINER" \
  --cpus="$I62_CPUS" ${I62_CPUSET:+--cpuset-cpus="$I62_CPUSET"} \
  --memory="$I62_MEMORY" --memory-swap="$I62_MEMORY_SWAP" \
  "$IMAGE" sleep infinity >/dev/null

# The image's default CMD is a bare `/bin/bash` (no ENTRYPOINT), which exits
# immediately without a tty/`-it` -- hence launching with an explicit
# `sleep infinity` override so the container stays up to exec into, matching
# how this session's exploration container was run.

# --- 2. real (executed, not heuristic) CPU-baseline compatibility probe ----
# Run the actual Havenask query-serving binary with a trivial argument and
# see how it dies. This is real evidence (gdb-confirmed this session, see the
# header above), not a /proc/cpuinfo flag guess: it is possible in principle
# for a flag guess to be wrong in either direction, but actually invoking the
# binary cannot be.
echo "==> probing Havenask worker binary (ha_sql) compatibility with this host's CPU"
set +e
PROBE_OUT=$(docker exec "$CONTAINER" bash -c '
  export LD_LIBRARY_PATH=/ha3_install/usr/local/lib:/ha3_install/usr/local/lib64:/usr/local/lib64/ssl/lib64:/usr/lib:/usr/lib64:/opt/taobao/java/jre/lib/amd64/server
  export LD_PRELOAD=/usr/local/lib64/lockless/libllalloc.so
  timeout 10 /ha3_install/usr/local/bin/ha_sql --version
' 2>&1)
PROBE_RC=$?
set -e

if [[ $PROBE_RC -eq 132 ]] || echo "$PROBE_OUT" | grep -qi "illegal instruction"; then
  echo "FATAL: Havenask worker binary (ha_sql) crashes with SIGILL (\"Illegal instruction\", exit 132) on this host's CPU before it can serve or index anything. gdb backtrace this session pinpointed the crash inside ailego::internal::CpuFeatures::CpuFlags::CpuFlags() on a VEX-encoded 'vpxor' instruction -- i.e. the official ha3_runtime binaries require baseline AVX (and per \`strings ha_sql\`, are built targeting AVX2+FMA+BMI2+F16C) with no software fallback, and this host's CPU (QEMU Virtual CPU, per lscpu) exposes no avx/avx2/fma/bmi2 flags at all. This is a genuine host-CPU architectural incompatibility affecting every Havenask worker role (searcher/qrs/build_service all link the same ailego library), not a hape config, schema, or WANDS-data problem -- hape's processorMode=proc single-process config (hape_conf/proc) DOES successfully bring up zookeeper/swift/table scaffolding on this host, it is only the actual serving/indexing binaries that cannot execute here. Probe output: ${PROBE_OUT}" >&2
  exit 3
fi

# --- unreached on this host: the probe above has always failed here. -------
# A future session on AVX2/FMA/BMI2/F16C-capable hardware that gets past the
# probe should implement, in order (none of this is proven working yet):
#   1. Render a proc-mode hape_conf for a real run (the /ha3_install/example
#      cases customize a template with `hape customize -t <template_dir> -i
#      <image>`; hape_conf/proc already exists pre-rendered and was used
#      successfully this session for `hape start havenask` / `hape create
#      table` up to the point the probe above would have failed).
#   2. Translate the WANDS field list (id, title, description, product_class,
#      category_leaf, category_depth_1..6, color, style, primarymaterial,
#      material, shape, rating_count, average_rating, review_count -- see
#      dataset_cache/wands/catalog.jsonl's first line for exact
#      shapes/nullability) into a Havenask table schema JSON modeled on
#      /ha3_install/example/cases/normal/in0_schema.json (TEXT+analyzer for
#      title/description, ATTRIBUTE/STRING for the rest, PRIMARY_KEY64 or
#      PRIMARY_KEY128 on id depending on id's actual type, SUMMARY for
#      display fields).
#   3. `hape create table -t <table> -s <schema.json>` then either (a) direct
#      per-row SQL INSERT over HTTP :45800 (small/medium tiers, mirroring
#      example/common/case.py's HavenaskDataSet.to_sqls path) or (b) the
#      offline/full `-f <full_data_file>` buildservice path for larger
#      tiers -- HavenaskDataSet's expected on-disk input format is the
#      `CMD=add\nfield=value\n...` blocks seen in example/data/test.data, not
#      raw JSON, so the WANDS catalog.jsonl needs a format conversion step.
#   4. Verify count via `sql_query.py`/`hape gs havenask`, then find the real
#      on-disk index path (this session got as far as
#      /root/havenask-sql-proc_database.database_partition_0 existing, but
#      never confirmed built index files under it, since the query binary
#      that would perform the build never ran) and `du -sb` it.
echo "FATAL: unreachable -- provisioning logic past the CPU probe is undocumented/untested on this host and intentionally not implemented; see script header and probe FATAL above" >&2
exit 3
