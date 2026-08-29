#!/usr/bin/env python3
"""Issue #57 Revision 2 gap closure: index size / build time / startup
time / memory footprint across every (dataset, engine) cell -- the
frozen protocol's own §11 requirement, not systematically instrumented
in Revision 1 (adversarial review, confirmed limitation).

Build time is read back from this session's own indexing run logs
(shell `time` wrappers around each `scripts/datasets/*_index_*.py`
invocation -- see docs/experiments/ISSUE57_HAVENASK_REVISION2_LOG.md's
sibling artifacts) rather than re-running the indexer, so this script
does not re-ingest data. Index size is read from each engine's own
admin API (Solr core STATUS, Elasticsearch/OpenSearch `_stats/store`) --
a real reported figure, not a directory `du` estimate that could double-
count replicas or shared segment files. Startup time is measured by a
real stop/start cycle timed to first successful health response.
Peak RSS is a steady-state snapshot at measurement time (`/proc/<pid>/
status` VmRSS), disclosed as such -- not a continuously-sampled true
peak, since retrofitting continuous sampling into an already-running
indexing pass was out of scope this revision.

Usage: python3 scripts/datasets/measure_footprint.py
"""
import json
import re
import subprocess
import time
from pathlib import Path

import requests

ROOT = Path(__file__).resolve().parents[2]
LOG_DIR = Path("/tmp/gap3_logs")
OUT_DIR = ROOT / "docs" / "research" / "artifacts" / "issue57_footprint"
OUT_DIR.mkdir(parents=True, exist_ok=True)

DATASETS = ["wands", "esci_electronics", "esci_automotive", "esci_beauty", "magento"]
CORE_OR_INDEX = {
    "wands": "wands_bench",
    "esci_electronics": "esci_electronics_bench",
    "esci_automotive": "esci_automotive_bench",
    "esci_beauty": "esci_beauty_bench",
    "magento": "magento_bench",
}
BUILD_LOGS = {
    ("wands", "solr"): "idx_solr_wands.log",
    ("wands", "elasticsearch"): "idx_es_wands.log",
    ("wands", "opensearch"): "idx_os_wands.log",
    ("esci_electronics", "solr"): "idx_solr_esci_electronics.log",
    ("esci_electronics", "elasticsearch"): "idx_es_esci_electronics.log",
    ("esci_electronics", "opensearch"): "idx_os_esci_electronics.log",
    ("esci_automotive", "solr"): "idx_solr_esci_automotive.log",
    ("esci_automotive", "elasticsearch"): "idx_es_esci_automotive.log",
    ("esci_automotive", "opensearch"): "idx_os_esci_automotive.log",
    ("esci_beauty", "solr"): "idx_solr_esci_beauty.log",
    ("esci_beauty", "elasticsearch"): "idx_es_esci_beauty.log",
    ("esci_beauty", "opensearch"): "idx_os_esci_beauty.log",
    ("magento", "solr"): "idx_magento_all.log",
    ("magento", "elasticsearch"): "idx_magento_all.log",
    ("magento", "opensearch"): "idx_magento_all.log",
}


def parse_real_seconds(log_path):
    """Parses GNU `time`'s `real\tXmY.Zs` line into milliseconds."""
    if not log_path.exists():
        return None
    text = log_path.read_text()
    m = re.search(r"real\s+(\d+)m([\d.]+)s", text)
    if not m:
        return None
    minutes, seconds = int(m.group(1)), float(m.group(2))
    return (minutes * 60 + seconds) * 1000.0


def solr_index_bytes(core):
    try:
        resp = requests.get(
            "http://localhost:8983/solr/admin/cores",
            params={"action": "STATUS", "core": core, "indexInfo": "true"},
            timeout=10,
        ).json()
        return resp["status"][core]["index"]["sizeInBytes"]
    except Exception as e:
        return f"ERROR: {e}"


def es_family_index_bytes(base_url, index):
    try:
        resp = requests.get(f"{base_url}/{index}/_stats/store", timeout=10).json()
        return resp["_all"]["primaries"]["store"]["size_in_bytes"]
    except Exception as e:
        return f"ERROR: {e}"


def process_rss_kb(pid):
    try:
        status = Path(f"/proc/{pid}/status").read_text()
        for line in status.splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1])
    except Exception:
        return None
    return None


def find_pid(pattern):
    try:
        out = subprocess.run(["pgrep", "-f", pattern], capture_output=True, text=True).stdout
        pids = [int(p) for p in out.split()]
        return pids[0] if pids else None
    except Exception:
        return None


def measure_startup(name, stop_cmd, start_cmd, health_check, timeout_s=180):
    """Stops the engine, restarts it, and times until `health_check()`
    first returns True -- a real cold-start-to-serving figure, not an
    estimate."""
    subprocess.run(stop_cmd, shell=True, capture_output=True)
    time.sleep(2)
    subprocess.Popen(start_cmd, shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    t0 = time.time()
    while time.time() - t0 < timeout_s:
        if health_check():
            return (time.time() - t0) * 1000.0
        time.sleep(1)
    return None


def solr_healthy():
    try:
        return requests.get("http://localhost:8983/solr/admin/cores", timeout=2).status_code == 200
    except Exception:
        return False


def es_healthy(port):
    try:
        return requests.get(f"http://localhost:{port}", timeout=2).status_code == 200
    except Exception:
        return False


def main():
    rows = []

    # ---- index size + build time (per dataset x engine, no restart needed) ----
    for dataset in DATASETS:
        core_or_index = CORE_OR_INDEX[dataset]
        solr_bytes = solr_index_bytes(core_or_index)
        es_bytes = es_family_index_bytes("http://localhost:9200", core_or_index)
        os_bytes = es_family_index_bytes("http://localhost:9201", core_or_index)
        for engine, index_bytes in [
            ("solr", solr_bytes),
            ("elasticsearch", es_bytes),
            ("opensearch", os_bytes),
        ]:
            build_ms = parse_real_seconds(LOG_DIR / BUILD_LOGS[(dataset, engine)])
            rows.append(
                {
                    "dataset": dataset,
                    "engine": engine,
                    "build_ms": build_ms,
                    "index_bytes": index_bytes,
                }
            )

    # ---- startup time + peak (steady-state) RSS, one real restart per engine ----
    solr_pid = find_pid("start.jar")
    es_pid = find_pid("elasticsearch")
    os_pid = find_pid("opensearch")
    print(f"pre-restart pids: solr={solr_pid} es={es_pid} os={os_pid}")

    solr_startup_ms = measure_startup(
        "solr",
        "/home/yangjeep/engines/solr-9.10.1/bin/solr stop -all",
        "cd /home/yangjeep/engines && JAVA_HOME=/home/yangjeep/engines/jdk-21.0.12.1+1 "
        "PATH=/home/yangjeep/engines/jdk-21.0.12.1+1/bin:$PATH "
        "./solr-9.10.1/bin/solr start -force",
        solr_healthy,
    )
    solr_pid_after = find_pid("start.jar")
    solr_rss = process_rss_kb(solr_pid_after) if solr_pid_after else None

    es_startup_ms = measure_startup(
        "elasticsearch",
        f"kill {es_pid}" if es_pid else "true",
        "cd /home/yangjeep/engines && nohup ./elasticsearch-8.15.0/bin/elasticsearch "
        "> es_boot_footprint.log 2>&1 &",
        lambda: es_healthy(9200),
    )
    es_pid_after = find_pid("elasticsearch")
    es_rss = process_rss_kb(es_pid_after) if es_pid_after else None

    os_startup_ms = measure_startup(
        "opensearch",
        f"kill {os_pid}" if os_pid else "true",
        "cd /home/yangjeep/engines && nohup ./opensearch-2.17.0/bin/opensearch "
        "> os_boot_footprint.log 2>&1 &",
        lambda: es_healthy(9201),
    )
    os_pid_after = find_pid("opensearch")
    os_rss = process_rss_kb(os_pid_after) if os_pid_after else None

    for row in rows:
        if row["engine"] == "solr":
            row["startup_ms"] = solr_startup_ms
            row["peak_rss_kb"] = solr_rss
        elif row["engine"] == "elasticsearch":
            row["startup_ms"] = es_startup_ms
            row["peak_rss_kb"] = es_rss
        elif row["engine"] == "opensearch":
            row["startup_ms"] = os_startup_ms
            row["peak_rss_kb"] = os_rss

    csv_lines = ["dataset,engine,build_ms,index_bytes,startup_ms,peak_rss_kb"]
    for r in rows:
        csv_lines.append(
            f"{r['dataset']},{r['engine']},{r['build_ms']},{r['index_bytes']},"
            f"{r['startup_ms']},{r['peak_rss_kb']}"
        )
    (OUT_DIR / "footprint.csv").write_text("\n".join(csv_lines) + "\n")
    (OUT_DIR / "footprint.json").write_text(json.dumps(rows, indent=2))
    print(f"wrote {len(rows)} rows to {OUT_DIR / 'footprint.csv'}")
    for r in rows:
        print(r)


if __name__ == "__main__":
    main()
