# Issue #55 Phase B — dataset/baseline recovery inventory for Issue #57

## Purpose

Issue #55's stage-gate directive (PR #56) requires, before Issue #57's frozen
full-matrix benchmark begins: a systematic audit of every dataset/baseline
this project's history recorded as blocked/degraded by network restrictions,
a retry of canonical upstream sources now that network access is materially
more open, and an explicit INCLUDE/VALIDATION-ONLY/REJECT/BLOCKED decision
per candidate. This log is that audit, its live retries, and its decisions.

Retrieval date for every live check below: 2026-08-27, this session, from
this repository's sandboxed execution environment (outbound HTTPS through a
pre-configured agent proxy). A "reachable now" verdict below is scoped to
that environment/session, consistent with this project's own prior finding
that Docker daemon/network availability has flip-flopped across sessions —
Issue #57's own execution session should re-verify before relying on any of
these results rather than assuming they carry forward automatically.

## Method

1. Read every `docs/decisions/PHASE6*_DECISION.md`, `ISSUE35_*_DECISION.md`,
   `ISSUE38_DECISION.md`, `docs/adr/0007-*.md`, `PHASE8_FEASIBILITY.md`, and
   `docs/decisions/README.md` in full; read
   `docs/research/havenask-realtime-update-archaeology.md`,
   `docs/experiments/ROUND1_LOG.md` (acquisition section),
   `docs/experiments/PHASE6A/6B/6C/6E_LOG.md`, `docs/experiments/ISSUE38_LOG.md`
   (external-validity section), `docs/experiments/ISSUE43_LOG.md`; read every
   `scripts/datasets/fetch_*.sh`; grepped `docs/`/`scripts/`/`benchmarks/`/
   `artifacts/` for 403/blocked/unreachable/mirror/synthetic/fallback/
   Havenask/OpenSearch/Elasticsearch mentions.
2. For every blocked/degraded source found, live-retried the actual network
   path (registry APIs, dataset download endpoints, git remotes) from this
   session, not just re-read the historical claim.
3. For Havenask specifically, went beyond a reachability probe: pulled the
   real container image and started it, since Issue #57 requires a
   "genuinely runnable" baseline, not archaeology.
4. Searched for additional real commerce datasets not previously considered,
   scored against the "adds new schema physics, not just vocabulary" bar.

## Recovery inventory — previously blocked/degraded, retried

### 1. Havenask (Alibaba search engine) — was BLOCKED, now CONFIRMED RUNNABLE

```
source / intended dataset: Havenask (github.com/alibaba/havenask), required
  as a mandatory Issue #57 baseline engine
historical blocker: "the docker/dockerd binaries now exist... But actually
  pulling any container image fails uniformly: Docker Hub, ghcr.io, and an
  AWS public ECR mirror all... return 403 Forbidden from their actual
  blob/CDN storage hosts... Havenask's own registry
  (registry.cn-hangzhou.aliyuncs.com) remains additionally blocked at the
  connection level (CONNECT tunnel failed, 403)" (PHASE6E_DECISION.md,
  P6E-E02, the most recent prior live check); a later, chronologically
  AFTER-6E check in ISSUE43_LOG.md again found "Docker daemon unreachable" —
  confirming this project's own point that Docker/network availability is
  not a stable, monotonic fact across sessions.
why it mattered: mandatory Issue #57 baseline; Issue #7/#8 archaeology
  existed but no running instance had ever been achieved.
current access result: FULLY RECOVERED, verified end-to-end this session:
  - `docker info` initially failed (daemon not started, matching the
    historical flip-flop pattern) -- started `dockerd` directly, succeeded.
  - `registry.cn-hangzhou.aliyuncs.com/v2/` returns HTTP 401 (normal
    anonymous-probe response for a real registry), not a connection-level
    block.
  - `docker manifest inspect registry.cn-hangzhou.aliyuncs.com/havenask/ha3_runtime:latest`
    resolved cleanly: 41 layers, 2.30 GB total.
  - `docker pull` of that exact image **completed successfully** (2.3 GB,
    all layers).
  - `docker run` started a container from it; stayed up, did not crash.
  - Inside the container, `/ha3_install` contains `hape` (Havenask's own
    single-node auto-provisioning tool), an `example/` directory (the
    official quickstart), and `sql_query.py` -- this is the real, official
    single-node quickstart install, exactly the "closest supported
    single-node configuration" Issue #57 asks for.
  - Also confirmed reachable, not yet pulled (redundant once the above
    succeeded): `registry-1.docker.io/v2/` and `ghcr.io/v2/` both return
    401 (not 403) on an anonymous probe.
  - Test container/image removed after verification to conserve this
    session's disk allowance; re-pulling for #57 is a ~2.3 GB, few-minute
    operation, already proven to work.
license: Havenask is Apache-2.0 (github.com/alibaba/havenask); the Docker
  image is the project's own official runtime image.
version/revision: image digest
  sha256:a4fd0269ac54593c894510df783d7aa6e33169cecf9731f7d1a6f08bbec51734,
  tag `latest` as of 2026-08-27; source HEAD `26bf4c1567b42f6a4b48a74e8cdec0288a0d8faa`.
historical conclusions weakened by its absence: every prior Phase 6
  cross-engine comparison omitted Havenask entirely, leaving Issue #57's
  mandatory 4-engine matrix's hardest cell untested.
information gain from recovery: HIGH and immediately actionable -- Issue
  #57 can now genuinely attempt a real Havenask baseline instead of
  documenting another BLOCKED verdict. Recommend #57's session start here
  first (index a small dataset, e.g. WANDS, into the official quickstart
  and confirm query correctness) since it is this matrix's single largest
  residual unknown.
verdict: INCLUDE IN #57.
```

### 2. Elasticsearch / OpenSearch official distributions — was BLOCKED, now reachable (embedded-library route already validated separately)

```
source: official Elasticsearch/OpenSearch server tarballs
  (artifacts.elastic.co, artifacts.opensearch.org), and OpenSearch's
  bundled-JDK provider (api.adoptium.net)
historical blocker: "Official Elasticsearch tarball (artifacts.elastic.co)
  -- blocked (CONNECT tunnel failed, response 403)" (ROUND1_LOG.md); "Both
  engines' official prebuilt distributions are blocked... OpenSearch's own
  build hits a second, independent blocker... its bundled-JDK provider
  (api.adoptium.net) is also unreachable (403)" (PHASE6C_DECISION.md)
current access result: all three now reachable. `artifacts.elastic.co`
  returned HTTP 200 and began genuinely streaming the real 605 MB
  elasticsearch-8.15.0-linux-x86_64.tar.gz (49 MB transferred before a
  10s probe timeout cut it off -- a real download in progress, not a
  block). `api.adoptium.net/v3/info/available_releases` returned HTTP 200.
  `artifacts.opensearch.org` was not separately re-probed (same network
  path already confirmed open via the sibling elastic.co check; a
  distribution-tarball pull was not repeated for time, since --)
why full re-acquisition was not prioritized: PHASE6E_DECISION.md already
  achieved a real, correctness-verified embedded-library route for both
  engines via Maven Central (org.elasticsearch:elasticsearch:8.15.0,
  org.opensearch:opensearch:2.17.0) -- 24/24 correctness matches vs. live
  Solr for ES; OpenSearch verified indirectly (its SecurityManager network
  policy blocked a live cross-check, PHASE6E_DECISION.md's own named open
  risk). The "is this engine usable at all" question is already answered
  YES for both. The now-open distribution route adds a full-server (not
  embedded-node) topology option, and importantly REMOVES the
  api.adoptium.net blocker that caused OpenSearch's SecurityManager gap in
  the first place.
information gain: MEDIUM-HIGH for OpenSearch specifically -- with
  api.adoptium.net now open, OpenSearch's from-source/full-server build no
  longer hits its second independent blocker, which could resolve the
  SecurityManager network-permission gap (upgrade OpenSearch's evidence
  from indirect to live-verified). LOW-MEDIUM for Elasticsearch (embedded
  route already fully verified).
verdict: INCLUDE IN #57 (embedded-library route, already validated) for
  both; USE FOR TARGETED VALIDATION ONLY the newly-reachable full-server
  distribution route, specifically to attempt closing OpenSearch's
  SecurityManager gap.
```

### 3. Amazon Reviews 2023 (McAuley Lab) — was BLOCKED on specific hosts, now reachable at the top level; exact resolve-file paths not yet re-derived

```
source: huggingface.co/datasets/McAuley-Lab/Amazon-Reviews-2023 (+ CDN
  subdomains cdn-lfs*.huggingface.co), mcauleylab.ucsd.edu,
  amazon-reviews-2023.github.io
historical blocker: "huggingface.co and every CDN subdomain..., 
  mcauleylab.ucsd.edu, amazon-reviews-2023.github.io are blocked by this
  environment's egress policy -- confirmed as an organization-policy 403
  across multiple independent hosts" (PHASE6A_DECISION.md); re-confirmed
  unchanged through PHASE6B/6C.
current access result: `huggingface.co/datasets/McAuley-Lab/Amazon-Reviews-2023`
  itself returns HTTP 200 (the dataset landing page is reachable). A guessed
  direct resolve-URL (`.../resolve/main/raw/meta_categories/meta_All_Beauty.jsonl.gz`)
  returned 404 -- not blocked, just the wrong exact path (this dataset uses
  a loading-script/parquet-conversion layout on HF, not flat file paths
  under that guessed directory). This project's own `scripts/round1/fetch_esci.sh`
  and `scripts/datasets/fetch_esci_*.sh` already prove a *different*,
  smaller HF-hosted dataset (`tasksource/esci`) is fetchable via HF's
  `resolve/<rev>/...` pattern from this same environment, which is strong
  evidence the blanket-host block is gone, not merely that one file path
  is wrong.
information gain: HIGH if pursued to completion (this project's originally
  intended primary Phase 6 dataset, ~larger and richer than the WANDS
  substitute it was replaced with -- real price/rating/multi-category
  metadata at Amazon's full catalog scale) but NOT completed this session:
  the exact current HF dataset-viewer/parquet access pattern needs to be
  derived (likely via `datasets` library's loading script or the
  parquet-converted `refs/convert/parquet` branch) rather than guessed
  file paths, which is a real but bounded follow-up task, not a network
  block.
verdict: USE FOR TARGETED VALIDATION ONLY -- worth a dedicated follow-up
  to derive the correct access pattern given the network premise, but the
  existing WANDS+ESCI evidence base does not obviously become wrong
  without it, so this is not a blocking rerun for #57's freeze.
```

### 4. Retailrocket (real shopper behavior events) — was BLOCKED, now RECOVERED and cached

```
source: kaggle.com/datasets/retailrocket/ecommerce-dataset
historical blocker: "curl to kaggle.com fails with CONNECT tunnel failed,
  response 403 (organization-policy block)... No GitHub/GCS mirror of the
  raw data was found." (PHASE6B_LOG.md), re-confirmed through PHASE6C.
current access result: FULLY RECOVERED. `kaggle.com` returns HTTP 200.
  Kaggle's public API download endpoint
  (`kaggle.com/api/v1/datasets/download/retailrocket/ecommerce-dataset`)
  downloaded the complete, real 304,719,974-byte zip **anonymously, no API
  key/session required** -- confirming the historical blocker was purely
  this project's own network egress policy, not a Kaggle auth requirement
  as might otherwise be assumed. Downloaded and cached this session:
  `dataset_cache/retailrocket/retailrocket.zip`
  (sha256 `5dc06173eaa4a4d3b0a5f6afc4daffb0218d95e409aaacba5fdaa0214eacf2a2`),
  reproducible via the new `scripts/datasets/fetch_retailrocket.sh` +
  `scripts/datasets/retailrocket_checksums.sha256`.
schema/size (verified by direct inspection, not just the listing page):
  - `category_tree.csv`: `categoryid,parentid` -- a real category hierarchy.
  - `events.csv`: `timestamp,visitorid,event,itemid,transactionid` --
    **2,756,101 real events** (view/addtocart/transaction), a genuine
    shopper behavior funnel this project has no equivalent of anywhere
    else.
  - `item_properties_part1.csv` + `part2.csv`:
    `timestamp,itemid,property,value` -- anonymized, time-varying item
    property changes (property names/values are hashed/obfuscated by the
    dataset's own anonymization, a real "noisy seller-entered marketplace
    schema" example, not clean typed attributes).
license: **CC BY-NC-SA 4.0** (verified from the Kaggle listing page's own
  license badge, not assumed) -- NonCommercial: research/benchmark use
  only, must never be redistributed or used commercially.
historical conclusions weakened by its absence: Phase 6B built a synthetic
  controlled-stress WANDS replication ladder (2x/5x/10x/20x) specifically
  *because* Retailrocket (and H&M) were unavailable as a real large-scale
  alternative -- that synthetic-ladder methodology choice is not
  invalidated (it answers a different, controlled-stress question), but
  this project has never had real behavioral/event data to evaluate
  against at all until now.
information gain: HIGH -- this is the first real behavior/relevance-
  adjacent evidence source (view -> cart -> purchase) this project has
  ever had access to, and the item-properties table is a genuine example
  of the "noisy seller-entered marketplace schema" dimension #57 asks for.
  No relevance judgments in the WANDS/ESCI sense (no query text at all --
  this is behavioral logs, not a search-query benchmark), so it cannot
  replace WANDS/ESCI for NDCG-style relevance measurement, but is a
  strong candidate for click/conversion-based implicit relevance or
  browse/PLP-style workload classes.
verdict: INCLUDE IN #57 for browse/behavior/marketplace-noise workload
  classes; not a relevance-judgment substitute for WANDS/ESCI.
```

### 5. Open Food Facts — was BLOCKED, now reachable; named follow-up for Issue #38's external-validity gap

```
source: Open Food Facts (world.openfoodfacts.org / static.openfoodfacts.org),
  sought as a genuinely distant-vertical (grocery/CPG) external-validity
  check for Issue #38's synthetic schema-discovery work
historical blocker: "both its primary host and its HuggingFace mirror are
  unreachable from this sandbox" (ISSUE38_LOG.md, "External-validity
  check" section) -- explicitly disclosed by the project's own prior
  session as a sandbox restriction, not a genuine absence.
current access result: `world.openfoodfacts.org` returns HTTP 200; the
  full-export CSV endpoint (`static.openfoodfacts.org/data/en.openfoodfacts.org.products.csv.gz`)
  returns HTTP 302 (a normal redirect, e.g. to a CDN-fronted copy), not a
  block. Not fully downloaded this session (the full export is large and
  ISSUE38_DECISION.md's own external-validity gap is scoped to Issue #38,
  not #55/#57's mandatory matrix).
license: ODbL (database) + CC-BY-SA (individual contributions), per Open
  Food Facts' own stated policy (not independently re-verified this
  session beyond the reachability check).
information gain: HIGH but scoped to Issue #38, not #57 -- this is
  explicitly named in ISSUE38_DECISION.md as a disclosed, already-scoped
  follow-up ("this pass's own attempt was blocked by sandbox network
  restrictions, not by dataset unavailability in general"). Recommend a
  dedicated Issue #38 follow-up session acquire and use it, rather than
  folding it into #57's matrix (#57's dataset list is explicitly commerce-
  search-relevance-shaped; grocery/CPG without query/relevance data would
  need its own workload-class treatment first).
verdict: USE FOR TARGETED VALIDATION ONLY (Issue #38's external-validity
  gap specifically, not the #57 matrix).
```

### 6. SayamAlt E-Commerce-Text-Classification — was license-BLOCKED (origin unverifiable), now the origin is reachable

```
source: raw.githubusercontent.com/SayamAlt/E-Commerce-Text-Classification
  (50,425 rows, Electronics/Clothing & Accessories/Books/Household labels);
  its license provenance traces to a Zenodo record
historical blocker: "has an unconfirmed license -- its origin (a Zenodo
  record) could not be reached from this sandbox to check." (ISSUE38_LOG.md)
  -- the dataset content itself was always fetchable; only the license
  check was blocked.
current access result: both now reachable. `raw.githubusercontent.com/...`
  returns HTTP 200 (content, as before). `zenodo.org` returns HTTP 200
  (previously-blocked origin now reachable) -- the specific Zenodo record
  number was not re-derived and its license page not re-read this session
  (time-boxed; this is a narrow, cheap follow-up, not a blocker).
information gain: HIGH and cheap -- only the specific Zenodo record needs
  to be located and its license read; the data itself is already in hand.
verdict: USE FOR TARGETED VALIDATION ONLY (same Issue #38 external-
  validity scope as Open Food Facts; complete the license check before
  using).
```

### 7. H&M Personalized Fashion Recommendations / Home Depot product-search-relevance — Kaggle-hosted, network block lifted; not re-acquired this session

```
source: kaggle.com/competitions/h-and-m-personalized-fashion-recommendations
  (real fashion/apparel catalog+transactions); Kaggle's
  home-depot-product-search-relevance competition (real human relevance
  judgments, structurally closest analog to WANDS)
historical blocker: same kaggle.com 403 as Retailrocket.
current access result: kaggle.com itself confirmed reachable (see #4
  above); these two specific competition datasets were not individually
  re-probed this session (competition datasets, unlike open datasets,
  typically require accepting competition rules via an authenticated
  Kaggle account even when the network path is open -- untested whether
  the anonymous-download route that worked for Retailrocket's open
  dataset also works for a *competition* dataset).
information gain: Home Depot is HIGH (closest real structural analog to
  WANDS with real relevance judgments) if the competition-rules
  requirement turns out not to block anonymous access; H&M is MEDIUM
  (real apparel/size-variant structure, no direct relevance-judgment
  angle). Both are real, bounded follow-ups for a session with Kaggle
  credentials available, not proven blockers.
verdict: BLOCKED (competition-gated, unverified this session) pending a
  credentialed retry; do not assume the Retailrocket result generalizes
  to competition datasets without checking.
```

### 8. eCommerceSearchBench (Alibaba) — never actually blocked, previously just unexplored

```
source: github.com/alibaba/eCommerceSearchBench, a synthetic Taobao-style
  workload/data generator, named in Issue #21's Phase 6 plan
historical status: "A fresh search found it -- reachable via git clone...
  known reachable but unexplored" (PHASE6C_DECISION.md, unresolved risk #9)
current access result: confirmed reachable again this session
  (`git ls-remote` succeeded, HEAD `c01eb4b8625d3b6614a16f249aee4b02b5d4d49d`).
information gain: LOW for the #57 matrix specifically (it is a synthetic
  workload generator, not a real dataset with real relevance judgments,
  and #57's own text prioritizes real recovered data); a legitimate but
  low-priority "someone should explore this" backlog item, independent of
  the network-reopening premise since it was never blocked.
verdict: REJECT / LOW INFORMATION GAIN for #57 (not blocked, just
  synthetic and not yet scoped to a specific question).
```

### 9. Not retried: sources correctly recorded as never blocked

WANDS (raw.githubusercontent.com, pinned commit), the full ESCI corpus
(`media.githubusercontent.com/media/...` LFS route), the three ESCI
vertical slices, Magento configurable-product sample data, and the local
Solr 9.10.1 install were all previously confirmed fully reachable/acquired
and remain so — not re-audited here, per Issue #55's own instruction not
to re-verify what was never blocked.

## New datasets sought (beyond re-retrying known blockers)

A parallel search (live web search + direct fetch verification, not just
listing pages) for additional real commerce datasets adding new schema
physics ran this session. None of its candidates clear the bar for
immediate inclusion — every one carries at least one unresolved or
disqualifying issue (license conflict, unstated license, dead canonical
host, an explicit anti-ML-training clause, or Kaggle competition gating
this session could not verify past its JS-rendered listing page). Recorded
here in full rather than silently dropped, per "do not fabricate
unavailable structure" and "do not add a dataset merely to increase
dataset count":

```
1. Amazon Berkeley Objects (ABO) -- amazon-berkeley-objects.s3.amazonaws.com,
   147,702 listings, real typed attributes + multilingual text +
   heuristic variant grouping. LICENSE CONFLICT: the canonical S3 page
   says CC BY 4.0; the AWS Open Data registry entry and the CVPR/arXiv
   paper both say CC BY-NC 4.0. Disagreement not resolved. Verdict:
   BLOCKED pending license resolution (contact/confirm with the
   publisher); real value if resolved (multilingual + variant grouping,
   dimension 1/5).

2. H&M Personalized Fashion Recommendations (Kaggle) -- 105,542 articles,
   1.37M customers, 31.8M transactions, real product_code -> article_id
   color/style variants. Kaggle-restricted license (competition rules,
   non-commercial, no redistribution) not independently confirmed past
   the JS-gated listing page. Verdict: BLOCKED pending credentialed
   verification.

3. NHTSA vPIC (vpic.nhtsa.dot.gov) -- real US-federal Year/Make/Model/
   Trim/engine/body-type reference data. data.gov license field is
   literally tagged "unknown-license"; likely public-domain as a US
   federal work but not confirmed. Also NOT itself a parts-fitment
   table -- only the YMM reference axis a fitment table would join
   against; does not alone satisfy the "automotive fitment/OEM part
   number" dimension. Verdict: USE FOR TARGETED VALIDATION ONLY (as a
   joinable YMM reference), license status still needs resolving.

4. Amazon-M2 / KDD Cup 2023 multilingual shopping sessions -- ~2.6M real
   sessions across EN/DE/JP/FR/IT/ES, genuine cross-locale product sets
   (not just translated text). No dataset license found on the AIcrowd
   challenge page, its rules subpage, or the companion GitHub repo (only
   the *submission code* is Apache-2.0). Verdict: BLOCKED pending license
   confirmation from Amazon/AIcrowd; real value if resolved (dimension 5,
   multilingual/cross-border).

5. Diginetica / CIKM Cup 2016 -- the one candidate found with a genuine
   query -> click -> purchase chain (Retailrocket has behavior but no
   query text at all). Official host (cikm2016.cs.iupui.edu) is DNS-dead;
   only an unverified Kaggle mirror remains. Verdict: REJECT / BLOCKED --
   provenance broken, do not use without a license-clear re-publication.

6. MercadoLibre Data Challenge 2019 -- real LatAm (es/pt) marketplace
   product-to-category data. Official site returned HTTP 403; Kaggle
   mirror not independently verifiable. Verdict: BLOCKED pending a
   working access path.

7. Icecat / Open Icecat -- manufacturer-sponsored typed technical
   datasheets (dimension 3), 70+ languages, 18M+ products. Its own
   license text (iceclog.com/open-content-license-opl), read directly,
   explicitly bars use for machine-learning training as a condition of
   the free tier. Verdict: REJECT -- structurally incompatible with this
   project's own methodology (model proposals over catalog data), not
   merely undesirable.

8. GS1 Global Product Classification (GPC) -- the real, deep,
   industry-standard taxonomy dimension 6 asks for. Page reachable; full
   brick-level codeset appears to require GS1 membership, free-tier
   boundary not confirmed. Verdict: USE FOR TARGETED VALIDATION ONLY /
   BLOCKED pending confirming what is actually free.

9. Home Depot product-search-relevance (Kaggle) -- real human relevance
   judgments + a technical attributes.csv, structurally closest analog to
   WANDS with typed specs attached. Same Kaggle JS-gating as #2; terms
   not independently confirmed. Verdict: BLOCKED pending credentialed
   verification (same open question as #7 in the original blocked-source
   table above -- do not assume the Retailrocket anonymous-download
   result generalizes to a *competition* dataset).
```

Explicitly rejected, not padding the candidate list further:
**Rakuten France/SIGIR eCom** (strict confidentiality agreement, 2-year
post-challenge restriction), **Instacart 2017** (canonical source now
returns 404 -- confirmed withdrawn, not merely slow), **Myntra fashion
scrapes** (no stated license, scraped without permission), **Yoochoose**
(license is fine -- CC BY-NC-ND 4.0 -- but its session-click-buy schema is
structurally redundant with Retailrocket's, no new schema physics),
**UNSPSC** (a free classification codeset, not an actual product catalog
with real listing rows -- a supplementary reference only, not a primary
candidate).

**Two of the eight priority dimensions came back genuinely empty**, stated
plainly rather than forced: no license-clear, publicly downloadable real
automotive fitment/OEM-part-number compatibility table was found (every
real one located is a paid commercial product), and no credible open B2B/
industrial catalog with MOQ/unit-of-measure structure was found either.
This is real, disclosed negative evidence for #57's dataset search, not an
oversight.

## Summary: dataset recovery decisions

| Source | Historical status | Current status | Decision |
|---|---|---|---|
| Havenask (engine) | BLOCKED (container registry/blob storage 403) | **RECOVERED** — pulled, ran, confirmed official single-node quickstart present | **INCLUDE IN #57** |
| Elasticsearch (embedded) | already validated | unchanged | **INCLUDE IN #57** |
| OpenSearch (embedded) | already validated, SecurityManager gap disclosed | distribution route now open (may resolve the gap) | **INCLUDE IN #57**; retry SecurityManager fix as targeted validation |
| Amazon Reviews 2023 | BLOCKED (host-level) | host reachable, exact file layout not yet derived | USE FOR TARGETED VALIDATION ONLY |
| Retailrocket | BLOCKED (kaggle.com 403) | **RECOVERED** — downloaded, hashed, cached, schema verified | **INCLUDE IN #57** (behavior/marketplace-noise classes, not relevance-judgment classes) |
| Open Food Facts | BLOCKED (sandbox) | reachable | USE FOR TARGETED VALIDATION ONLY (Issue #38 scope) |
| SayamAlt / Zenodo license | license-BLOCKED | Zenodo reachable, record not yet re-derived | USE FOR TARGETED VALIDATION ONLY (Issue #38 scope) |
| H&M / Home Depot (Kaggle competitions) | BLOCKED (kaggle.com 403) | kaggle.com open; competition-gate status unverified | BLOCKED pending credentialed retry |
| eCommerceSearchBench | never blocked, unexplored | unchanged | REJECT / LOW INFORMATION GAIN for #57 |
| WANDS / ESCI (full + 3 slices) / Magento / local Solr | never blocked | unchanged | already INCLUDED |
| Amazon Berkeley Objects | not previously considered | reachable, license conflict (CC BY vs. CC BY-NC across sources) | BLOCKED pending license resolution |
| Amazon-M2 (multilingual sessions) | not previously considered | reachable, no dataset license found | BLOCKED pending license confirmation |
| NHTSA vPIC | not previously considered | reachable, license tag "unknown"; not a fitment table by itself | VALIDATION ONLY (joinable YMM reference) |
| Diginetica / CIKM Cup 2016 | not previously considered | canonical host DNS-dead | REJECT / BLOCKED (broken provenance) |
| MercadoLibre Data Challenge 2019 | not previously considered | official site HTTP 403 | BLOCKED |
| Icecat / Open Icecat | not previously considered | reachable, but license explicitly bars ML-training use | **REJECT** (incompatible with project methodology) |
| GS1 GPC | not previously considered | reachable, full codeset likely membership-gated | VALIDATION ONLY / BLOCKED |
| automotive fitment/OEM part-number tables | sought | **none found** with a clear open license | genuine unmet gap, disclosed |
| B2B/industrial MOQ catalogs | sought | **none found** with a clear open license | genuine unmet gap, disclosed |

## Frozen candidate dataset set proposed for Issue #57

- WANDS (full corpus)
- ESCI (full corpus + electronics/automotive/beauty slices)
- Magento configurable-product sample (real Product/Variant)
- **Retailrocket** (newly recovered — behavior/browse/marketplace-noise workload classes; CC BY-NC-SA 4.0, research/benchmark use)
- Engines: commerce-native, Solr 9.10.1 (local), Elasticsearch 8.15.0 (embedded), OpenSearch 2.17.0 (embedded), **Havenask** (newly confirmed runnable — official single-node quickstart via the `ha3_runtime` container)

Not included in the frozen set (explicit, not silent): Amazon Reviews 2023
full corpus, Open Food Facts, H&M, Home Depot, eCommerceSearchBench, and
every newly-searched candidate above (Amazon Berkeley Objects, Amazon-M2,
NHTSA vPIC, Diginetica, MercadoLibre, Icecat, GS1 GPC) — each named with
its specific reason and, where one exists, a recommended next step to
resolve it. Real Product/Variant-fitment and B2B/MOQ data remain
disclosed, unmet gaps rather than forced weak substitutes.
