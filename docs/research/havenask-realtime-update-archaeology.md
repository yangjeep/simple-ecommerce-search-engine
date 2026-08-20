# Issue #8: Havenask/IndexLib realtime update archaeology

Source-level research (not a summary of marketing claims) into whether
Havenask/IndexLib already has the variant-scoped availability/inventory
overlay Issue #8 proposes, before implementing anything ourselves. Two of
three independent research passes are recorded here (a third, on realtime
segment/visibility/consistency semantics specifically, is in progress and
will be appended). Both completed passes independently converge on the
same classification.

**Method**: both passes cloned `github.com/alibaba/havenask` (commit
`26bf4c1567b42f6a4b48a74e8cdec0288a0d8faa`) and read actual IndexLib
source (`aios/storage/indexlib/`) plus shipped docs
(`docs/havenask_docs/`), citing specific files/line ranges for every claim.
Neither relied on inference from "how other search engines typically
work."

## Finding 1: single-value fixed-width attributes get true in-place mutation, by default

`AttributeConfig::Init` (`index/attribute/config/AttributeConfig.cpp:54-80`)
marks single-value non-string numeric fields (`int8`/`int32`/`double`/etc
— exactly the type an availability flag, inventory count, or price would
use) `updatable = true` **by default, with zero extra schema
configuration**. `InplaceAttributeModifier`'s call chain resolves to
`SingleValueAttributeUpdatableFormatter<T>::Set` (`format/
SingleValueAttributeUpdatableFormatter.h:61-69`):

```cpp
*(T*)(data + (int64_t)docId * this->_recordSize) = value;
```

A literal raw pointer write into the memory-mapped attribute file. No
document reconstruction, no segment rewrite, no reindexing. Reads
dereference the same mmap'd pointer, so the write is visible to the next
read in-process immediately — no reopen, no flush, no merge required for
visibility. Multi-value/string attributes and pack attributes do **not**
get this path (`InplaceAttributeModifier.cpp:172-175` explicitly rejects
pack attributes).

## Finding 2: bitmap/inverted-index term membership also gets true in-place mutation — but only when explicitly opted in

`InvertedIndexConfig::Check()` requires `index_updatable: true` (default
`false`) and restricts it to `NUMBER`/`STRING` index types with
`optionFlag == 0` (a bare filter/facet index, no position list — exactly
the shape of an availability or color/size-combo filter field, not a
full-text index). `BitmapLeafReader::TryUpdateInOriginalBitmap`
(`bitmap/BitmapLeafReader.cpp:188-210`) does a literal bit flip directly
on the memory-mapped bitmap posting file for a pre-existing docId. For
docIds beyond the original bitmap's size, or non-high-frequency terms, the
update goes to a separate, purpose-built "Dynamic Index" — a lock-free
concurrent search tree (`DynamicSearchTree`, using an
`EpochBasedReclaimManager`) held as a `ResourceFile` distinct from the
base dictionary/posting files. **This is a genuine overlay/sidecar
design** — but it is a generic filter-index update primitive, not a named
commerce/inventory subsystem.

**Real production evidence this exact shape is used for commerce fields**:
a shipped test schema (`aios/ha3/testdata/testSchema/termMatch/
mainse_excellent_search_schema.json` — field names `auction_tag`,
`coupon_business_id`, `combinetag`, `o2o_flag_c2c` indicate Taobao/Tmall
C2C marketplace provenance) configures `NUMBER`-typed filter/tag fields
with a high-frequency bitmap dictionary and `index_updatable: true` — the
precise shape needed for a fast, bitmap-backed, in-place-mutable
"is this SKU/variant available" flag.

## Finding 3: no purpose-built commerce/inventory overlay exists — it's a generic primitive someone would still assemble themselves

Both passes independently searched for a dedicated availability/inventory
index type, class, schema flag, or documented pattern, and found none.
What exists: (a) attribute storage is architecturally separate from the
inverted index for every field uniformly (not commerce-specific), and (b)
Havenask supports fully separate KV/KKV table types joinable via
`LookupJoin` at query time — closer in spirit to an "overlay," but a
generic building block (the docs' own worked examples are a `category`
lookup and a `company` lookup, nothing ecommerce-specific) that a schema
designer would still have to stand up and specialize themselves for
availability/inventory use. KV/KKV "updates" are also, separately, a
materially different and more expensive mechanism than attribute/bitmap
updates: a point-read of the old value, an in-memory merge, and a
full-value overwrite (read-merge-rewrite-as-`ADD_DOC`), not a lightweight
patch (`table/kv_table/KVTabletWriter.cpp:205-239`,
`RewriteDocUtil.cpp:23-47`).

## Finding 4: no credible update-latency/TPS benchmark exists — only inconsistent marketing claims

Havenask's docs contain detailed, methodical **query**-performance
benchmarks (dataset size, machine specs, QPS, p90/p99, for general and
vector retrieval). No equivalent table exists for update/write throughput
or update-to-visible latency anywhere in the docs tree or wiki. The only
numbers found are two directly **conflicting** marketing one-liners with
no methodology: the README claims "秒级数据更新" (second-level data
update) while a separate wiki intro page claims "毫秒级" (millisecond
level) for the same capability. Real ecommerce usage (Taobao, Tmall,
Freshippo/Hema search, Cainiao real-time order retrieval) is repeatedly
named in primary sources, but no field-level technical detail on how
stock/price/flash-sale state is actually handled is documented in what
was accessible.

## Finding 5 (third independent pass): a real two-tier routing mechanism exists, and it directly falsifies "treats all field updates uniformly"

A third, independent research pass (different agent, same method: cloned
source + docs, no shared context with the first two) traced the exact
mechanism that decides whether an incoming write is cheap or expensive:
`AddToUpdateDocumentRewriter` (`document/normal/rewriter/
AddToUpdateDocumentRewriter.cpp`) inspects an incoming `ADD_DOC` at
ingestion time and, **if every modified field is on the per-field
"updatable" whitelist**, rewrites the operation to `UPDATE_FIELD` before
it reaches the builder — otherwise the doc stays a full `ADD_DOC`
(delete-then-readd, consuming a new docid, flowing through the realtime
segment → dump → merge pipeline like any new document). This is a real,
explicit, evidence-backed two-tier system: cheap in-place `UPDATE_FIELD`
for whitelisted fields (works even against already-built **disk**
segments, no dump/merge involved) vs. expensive `ADD_DOC`/`AlterTable` for
everything else (schema/taxonomy changes go through `Tablet::AlterTable`,
which seals the building segment and generates an async background
IndexTask — structurally the same machinery class as a merge task).

**A real production schema fixture confirms this pattern is used for
exactly this class of field**: `mainse_excellent_search_schema.json`
(named for Alibaba's internal Taobao/Tmall main-search codename) marks
`promote`, `is_vertical`, `isprepay`, `coupon_business_id`, `region` —
fast-changing, filterable operational flags directly analogous to a
per-variant OOS flag — as `index_updatable: true`.

**Query consistency during mutation**: no snapshot isolation exists for
in-place updates. A single scalar field update is a single in-place
memory write, effectively atomic per field-slot from a reader's
perspective. But a logical update touching multiple fields (or both an
attribute and an inverted-index term) for one document has **no
cross-field transaction** — a concurrent reader could observe field A
updated and field B not yet updated. This is a real correctness
constraint our own design needs to either match or explicitly improve on,
not silently ignore.

**Correction to this issue's own premise, reported rather than smoothed
over**: the issue's briefing claimed "its 1.0 roadmap explicitly calls out
unified read/write ownership and realtime index organization." A
case-insensitive grep for this phrase and close variants (English and
Chinese) across the *entire* cloned repository — source, docs, README,
comments — found **zero matches**. No `ROADMAP.md` exists in the
repository at all. This claim is not substantiated by the primary source
repository (the wiki was unreachable due to this session's network
policy, so it cannot be fully ruled out there, but it is not present in
what was actually inspected). What *is* independently, architecturally
true: `Tablet` does literally own both writer and reader as sibling
members opened against the same shared in-memory segment objects
(`Tablet::OpenWriterAndReader`) — a real "single owner, shared live
state" design — but this specific terminology is not how Havenask's own
material describes it.

**Numbers**: still nothing rigorous. The only end-to-end figures anywhere
in the repository are the README's headline marketing claim ("millions of
QPS... millions of TPS... queries in milliseconds and updates data in
seconds") — no breakdown of how much of that "seconds" figure is
attributable to the (structurally much faster) `UPDATE_FIELD` path versus
the `ADD_DOC`/realtime-segment path.

## Classification: **B**, moderate-to-high confidence, from three independent passes

> B. Havenask supports fast generic partial updates but not a
> commerce-specific state overlay → benchmark generic Havenask update cost
> against a specialized overlay.

All three independent research passes reached this same classification
without coordination. High confidence in the positive finding (a
genuinely cheap, low-amplification, in-place update mechanism exists for
exactly the field types an availability/inventory/price signal needs, with
an explicit auto-routing mechanism distinguishing it from expensive
schema/taxonomy changes); necessarily somewhat lower confidence in the
negative finding (absence of a dedicated commerce-specific overlay), since
absence-of-evidence claims are weaker than positive ones, though the
search was thorough across source, docs, and (for two of three passes) a
direct full-repository grep.

## What this means for our implementation (not wheel-reinvention, convergent validation)

Havenask's proven mechanism — **in-place mutation of a bitmap or a
fixed-width column, bypassing full reindexing** — is the *same
algorithmic idea* our proposed variant-state overlay was going to use
(`RoaringBitmap` bit-flip for availability, a mutable column for
inventory), independently arrived at. This is not evidence to copy
Havenask's C++ implementation (we won't, and don't need to — clean-room
per Issue #5's own constraint). It is evidence that the *approach* is
sound and already validated by a mature system operating at far larger
scale, which changes what the actual open experimental question is: not
"does in-place bitmap/column mutation work" (Havenask already answers
that), but "does implementing that same proven pattern, scoped to our
typed `VariantId`-keyed commerce domain and integrated with our
`FastPath`/`Hybrid`/`Punt` planner, beat *our own* current baseline — a
full `CatalogIndex::build` rebuild for any state change (measured at
~64s/1.2M products, R1-E01) — by the material margin Issue #7's revised
performance thesis requires (>=5x)." That is the experiment Issue #8's
remaining work runs.

**Next**: implement the smallest mechanism that answers the performance
question — a variant-scoped mutable availability overlay in
`commerce_core`, matching the same algorithmic pattern (in-place bitmap
mutation, no reindex) Havenask's own `InplaceInvertedIndexModifier`/
`DynamicMemIndexer` independently validate — and benchmark it against our
own current baseline (a full `CatalogIndex::build` rebuild for any state
change, measured at ~64s/1.2M products, R1-E01), targeting Issue #7's
revised performance thesis bar (>=5x P50/P95 improvement) rather than a
generic "it works" claim. Must also address the multi-field
non-atomicity finding above: our design should decide explicitly whether
a variant-state delta is single-field (naturally atomic) or needs its own
consistency guarantee, not inherit Havenask's gap silently.

## Follow-up: can a real Havenask update-latency/TPS number actually be obtained in this environment?

Finding 4 above established that Havenask's own published material has no
credible number for this (only two conflicting marketing one-liners). The
open question this leaves is whether an *independent* measurement is
obtainable here, rather than relying on absence-of-evidence from
Havenask's own docs alone. This was genuinely attempted, not assumed
impossible, matching R1-E04's own precedent ("Docker being unavailable is
not sufficient reason to skip the baseline. Try native/JVM distributions,
CI runners, another environment, or a standalone service.").

**What was checked**:
- Havenask's own `README.md` documents exactly one supported distribution
  path: `docker pull registry.cn-hangzhou.aliyuncs.com/havenask/ha3_runtime:latest`.
  No prebuilt binary release, no native/JVM alternative package, no
  standalone benchmark tool independent of the full server image.
- `docker info` on this environment: the `docker` client binary and
  `dockerd` binary both exist, but `/var/run/docker.sock` does not, and
  the daemon is not running (`failed to connect to the docker API...
  no such file or directory`) — consistent with an unprivileged sandbox
  that cannot run a container runtime.
- Even setting the daemon aside: `registry.cn-hangzhou.aliyuncs.com` is
  unreachable through this environment's outbound proxy (`curl` to its
  v2 API endpoint returns `CONNECT tunnel failed, response 403`) — a
  second, independent blocker beyond the missing daemon.
- Building from source: the repository is a `bazel`-based build
  (`WORKSPACE`, `bazel/BUILD`) for a large-scale distributed C++ system
  (real production scale per its own README: "hundreds of billions of
  data records... millions of QPS"). `bazel` is not installed in this
  environment, and building it plus the full dependency graph
  (`third_party/` lists JVM, HDFS, and many other heavyweight
  dependencies) from scratch is a materially larger undertaking than
  R1-E04's Solr build (a single JVM `.tgz`, already itself the largest
  external-baseline effort in this project) — not attempted, given both
  the Docker path's double failure and the scale mismatch.

**Conclusion**: this is a genuine, concrete external blocker, not an
assumption. A real, independently-measured Havenask update-latency/TPS
number is not obtainable in this environment. The comparison this issue
asks for ("benchmark generic Havenask update cost against a specialized
overlay") therefore remains **algorithmic/mechanistic, not
head-to-head-quantitative**, on the Havenask side specifically: Findings
1-2 establish *what* Havenask's in-place mechanism does (a single raw
pointer write / bitmap bit-flip, immediately visible, no reopen/flush/
reindex) with source-level precision, which is sufficient to establish
that our own overlay (Issue #8's `commerce_core::state`,
`docs/experiments/REALTIME_LOG.md` R-E01) implements the *same
algorithmic pattern* — so the real, quantified numbers R-E01 measured
(342ns p50 `apply()`, ~2.6M updates/sec sustained) are evidence that this
pattern class performs at the expected order of magnitude, corroborated
by (not independently re-verified against) Havenask's own architecture,
not proof that our specific Rust implementation matches Havenask's
specific C++ implementation's exact number. This distinction is recorded
explicitly so it is never mistaken for a real head-to-head benchmark.
