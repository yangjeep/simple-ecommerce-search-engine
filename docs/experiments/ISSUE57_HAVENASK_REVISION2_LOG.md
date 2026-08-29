# Issue #57 Revision 2 — Havenask retry log (gap 4)

Per Issue #57's adversarial review (`ISSUE57_ADVERSARIAL_REVIEW.md`, Lens 4):
Revision 1's Havenask disclosure was a single line ("Docker-in-Docker via
`hape`'s `default` domain is not available in this sandbox... denied by
this session's own safety guardrails") backed by a fallback to `hape`'s
`proc` domain, which succeeded. This session, on a fresh host, retried
both modes far more thoroughly. Neither reached a serving-ready state.
This log records exactly what was tried, what was found, and why further
effort was not spent, per CLAUDE.md's "preserve corrected and superseded
evidence" and "record GO/REVISE/STOP without changing thresholds"
discipline.

## Session context

Unlike Revision 1's session (Docker socket access denied outright, ~12 GB
disk quota), this session's host has full local Docker access (not
nested/sandboxed) and ~156 GB free disk. The Revision 1 blocker
(Docker-in-Docker forbidden) does **not** reproduce here — a materially
different environment, which is exactly why the retry was attempted at
this depth rather than re-citing Revision 1's disclosure unchanged.

## Attempt 1: `hape`'s `default` domain (sibling-container mode)

This is the domain the shipped quickstart's own `example/cases/normal/
setup.py` targets by default (`self.hape_conf =
"/ha3_install/hape_conf/default"`) — i.e., official, not a fork.

**Root SSH was explicitly avoided per instruction.** The investigation
traced the actual blocker precisely rather than accepting "needs root":

1. `hape`'s own Python orchestration layer (`hape_libs/utils/shell.py`'s
   `SSHShell`) does not hardcode a user — it SSHes as whatever OS account
   runs the `hape`/`case.py` process. Running that process as the
   existing non-root `yangjeep` account (already possessing Docker-group
   membership) was sufficient here, using that account's own pre-existing
   SSH key (already self-trusted via its own `authorized_keys` on this
   host) — **zero changes to the host's SSH/root configuration** were
   needed for this layer.
2. `docker exec --user <name>` failed to resolve the account by name on
   this Docker daemon (`unable to find user yangjeep: no matching entries
   in passwd file`) despite `/etc/passwd` being correctly bind-mounted
   and confirmed present via a direct `cat`/`getent` check — a genuine,
   reproducible Docker-CLI quirk on this host, not a `hape` defect.
   Numeric UID (`--user 1000`) works. Worked around by patching
   `global_config.py`'s `DefaultVaribles.user` to resolve to
   `"<uid>:<gid>"` instead of a username string — an orchestration-layer
   fix, verified to leave the actual `bs`/`searcher`/`qrs` binaries and
   the `ha3_runtime` image untouched.
3. A bare-numeric `--user 1000` (no explicit group) defaults its primary
   group to `0`/root in this Docker version, breaking writes into
   group-owned mount paths — fixed by resolving the real primary GID via
   `pwd.getpwuid` in the same patch (`"1000:1000"`).
4. Docker's own `--workdir <path>` flag auto-creates a missing directory
   as root before the container's entrypoint runs — `docker_util.py`'s
   subsequent `docker exec --user <uid> ... mkdir` then fails with
   `Permission denied` against that root-owned parent. Fixed with a
   `chown` step (as root, the same context `cp /home/.passwd /etc/passwd`
   already runs in) immediately after container creation.
5. A **real shipped typo bug**, independent of any of the above: `user =
   self._global_config.default_variables.user,` in
   `docker_appmaster.py` (trailing comma) silently makes `user` a
   1-tuple, corrupting every `--user {}` string built from it. Fixed by
   removing the comma.
6. A **real shipped environment-propagation bug**: the container image's
   own baked-in `USER=root` environment variable is what `swift_admin`'s
   internal scheduler reads to decide its own self-SSH target, not the
   process's actual UID — meaning even with (1)-(5) fixed, the *worker
   process itself* still tried `ssh root@...` and failed
   (`Permission denied`, then eventually `kex_exchange_identification:
   Connection closed by remote host` after repeated attempts). Fixed by
   having `docker_util.py::start_process` explicitly prepend
   `USER={uid}:{gid}` to the process's own environment.

After all six fixes: `swift_admin` genuinely started (`Succeed to start
process swift_admin`, no SSH errors at all) — a materially further point
than Revision 1 ever reached in this domain (Revision 1 never got past
"denied" to try any of this). It then stalled indefinitely on `swift.py:
is_ready`'s "Broker count is not equals to 1, now: 0" — `swift_admin`'s
own internal hippo/carbon local-scheduler never deployed the broker
worker, with no further error surfaced in its logs (only benign
`kmonitor` connection-refused noise on an unrelated metrics port). Root-
causing *that* would mean reverse-engineering `swift_admin`'s internal
C++ scheduler with no error message to go on — exactly the "invoking
admin RPCs manually" / "excessive effort" line this investigation was
told not to cross. Stopped here.

## Attempt 2: `hape`'s `proc` domain (the mode Revision 1 used successfully)

Run cleanly on this fresh host, twice (once before an unplanned VM/session
restart destroyed the first attempt's containers, once after, from a
fully clean `hape delete` + `havenask_data_store` wipe). **Both times**:
`suez_admin_worker` reaches `RUNNING`/responsive
(`hape gs havenask` returns real, well-formed JSON), `hape create table`
successfully drives the table through admin/catalog/database/cluster
creation — but the `qrs`/`database` partition workers get stuck at
`"containerStatus": "RUNNING", "workerStatus": "WS_NOT_READY",
"readyForCurVersion": false` indefinitely (250s+ retry budget exhausted
both times), never converging to a servable table. No error in the
reachable logs beyond the same benign `kmonitor` noise.

This is a **new, reproducible finding**: `proc` mode, previously
successful in Revision 1's session on a different host, does not converge
on this session's host. Since both `default` and `proc` mode each reached
a *different* stall point (broker scheduling vs. worker-version
convergence) with no informative error either time, this is read as an
environment-specific characteristic of this host (resource, kernel, or
timing-sensitive local-scheduler behavior), not a defect discovered in
Havenask's engine/query logic — no commerce dataset was ever loaded far
enough to test query correctness in either attempt this revision.

## Disposition

**Havenask is UNAVAILABLE this revision.** Per the governing instruction's
explicit fallback ("If all three legitimate routes fail, keep the current
proc-domain Havenask result, document default-domain as BLOCKED BY
ORCHESTRATION ENVIRONMENT rather than by Havenask execution itself, and
proceed"):

- Route 1 (non-root account): achieved in full — no host SSH/root change
  was made or needed.
- Route 2 (minimal orchestration-only adaptation): attempted and
  substantially succeeded (six real, disclosed, orchestration-layer-only
  fixes, verified not to touch engine binaries), but the `default`
  domain's remaining stall (broker scheduling) was not resolved within
  that same minimal-effort bound.
- Route 3 (fully root-controlled ephemeral environment): not attempted —
  both discovered stall points are inside Havenask's/`hape`'s own
  process-liveness convergence logic, not access-control-shaped, so a
  differently-privileged environment would not obviously change the
  outcome, and attempting it would itself be a large, open-ended new
  environment-provisioning effort.

Every full-matrix and relevance binary this revision (`wands_full_matrix`,
`esci_full_matrix`, `magento_full_matrix`, `wands_relevance`,
`esci_relevance`) therefore probes Havenask once at startup
(`issue57_eval::havenask_available`) and runs its cells as a genuine
4-way (native/Solr/Elasticsearch/OpenSearch) comparison when it is down,
rather than fabricating a 5th data point or silently reusing Revision 1's
stale Havenask numbers as if they were re-verified this revision. Revision
1's own Havenask correctness/timing results (WANDS, the three ESCI
verticals, Magento Q8) remain on file, unchanged and dated to that
revision — they are Revision 1 evidence, not superseded by an inability to
reproduce them this revision, and not silently carried forward as if
re-confirmed. The final decision (`ISSUE57_FULL_MATRIX_DECISION.md`)
treats Havenask accordingly: real Revision 1 evidence where it exists, an
explicit availability gap for everything measured fresh this revision
(relevance metrics, randomized-order reruns, footprint instrumentation,
full-scale ESCI).

**Recommendation for a future revision**: `proc` mode's `qrs`/`database`
worker convergence failure is the more promising thread to pull first (it
is closer to a servable table than `default` mode's broker-scheduling
stall, and does not require any of this revision's six orchestration
patches) — specifically, obtaining `suez_admin_worker`'s own detailed
convergence/version-negotiation logs (not surfaced at the `hape gs`
summary level used here) before spending further effort on the `default`
domain's deeper scheduler internals.
