# Phase 6D Decision (Issue #21 Phase 6, extending 6A/6B/6C — the facet crossover closes, with a real qualifier)

**Decision: PROCEED**, with the single highest-value question P6C-E01
surfaced now answered: commerce-native's own facet counting *can* adopt
an ordinal-based technique, and for the field this project's own
repeated crossover finding was built on (`color`, a generic `Enum`
attribute), doing so does not merely narrow the long-standing crossover
against Solr — it reverses it decisively, holding across the full
WANDS-to-20x-replication scale range tested. **But this phase's own
adversarial follow-up (P6D-E02) found the technique is not a universal,
free win**: for fields whose existing naive baseline was already cheap
(the dedicated `brand`/`category`/`product_type` facets), the ordinal
method has a real crossover of its own — slower at small candidate
counts, faster only past a real threshold — because it trades a fixed,
dictionary-size-proportional per-call cost for per-candidate savings.
Both results are correctness-gated and are reported together, not
selectively.

## Recap: what this phase was asked to answer

P6C-E01 (`docs/experiments/PHASE6C_LOG.md`, `PHASE6C_DECISION.md`) found
that Lucene's own dedicated, ordinal-based facet module closes most of
this project's four-times-repeated facet-crossover finding (Phase 5, 6A,
6B, P6C-E00), and named the concrete, previously-untested next question
explicitly: **could commerce-native's own architecture adopt an
equivalent ordinal-based facet-counting approach, and by how much would
it close the native crossover?** This phase implements and measures
exactly that.

## Architecture tested

A new method, `CatalogIndex::facet_counts_ordinal`
(`crates/commerce-core/src/index/mod.rs`), alongside three new build-time
structures: a per-attribute value dictionary (`enum_dictionary:
HashMap<String, Vec<String>>`, each attribute's distinct values in
first-encountered order — the `Vec`'s index is the value's dense
ordinal), a reverse lookup used only at build time (`enum_value_ordinal`),
and a per-attribute `Vec<u32>` column (`enum_columns`, one entry per
variant ordinal, storing that variant's value-ordinal or a sentinel for
"no value"). Counting becomes: for each candidate ordinal, read its
value-ordinal from the column (`O(1)` array access, no string hashing),
increment a flat `Vec<u64>` counter array sized to the dictionary; only
the final, typically-few non-zero buckets get their `String` label
cloned. This is the same architectural family as Lucene's own
`SortedSetDocValuesFacetCounts` (P6C-E01) and Solr's own `facet.field`:
pre-build a dictionary once, count via integer array operations, defer
string work to the result only.

Existing structures (`enum_bitmaps`, `enum_values`, `facet_counts`,
`facet_counts_by_scan`) are unchanged — this is a purely additive change,
matching this project's own established pattern of adding new physical-
index strategies alongside, not in place of, existing ones (the same
pattern `facet_counts_by_scan` itself used when added in Phase 5).

## Measured results

**Correctness gate, checked before any timing claim**: a new unit test
asserts exact `BTreeMap` equality between `facet_counts_ordinal` and
`facet_counts_by_scan` across full/filtered/empty/never-indexed-attribute
inputs (all pass). The real-data benchmark additionally cross-checked the
new method's top-50 output against Solr's own live response at all 7
real WANDS `category_depth_1` checkpoints, in 3 repeated runs — **21/21
exact matches, 0 mismatches**.

**Three-way comparison (medians across 3 runs), color facet-scan under category filter**:

| Checkpoint | Candidates | Native scan (ms) | Native ordinal (ms) | Solr (ms) | Ordinal vs. scan | Ordinal vs. Solr |
|---|---|---|---|---|---|---|
| Rugs | 2,002 | 1.268 | 0.023 | 1.113 | 55.1x faster | 48.4x faster |
| Storage & Organization | 2,175 | 1.563 | 0.040 | 1.132 | 39.1x faster | 28.3x faster |
| Lighting | 2,072 | 1.340 | 0.015 | 1.047 | 89.3x faster | 69.8x faster |
| Outdoor | 3,394 | 2.668 | 0.077 | 1.167 | 34.6x faster | 15.2x faster |
| Décor & Pillows | 4,612 | 4.672 | 0.199 | 1.191 | 23.5x faster | 6.0x faster |
| Home Improvement | 4,686 | 3.891 | 0.083 | 1.127 | 46.9x faster | 13.6x faster |
| Furniture | 16,039 | 18.159 | 0.238 | 1.233 | 76.3x faster | 5.2x faster |

**The ordinal-based method beats Solr at every single checkpoint (5.2x-
69.8x), with no exceptions** — a materially larger and more consistent
win than P6C-E01 found for Lucene's own facet module against Solr (up to
3.0x, and still trailing at the 2 largest checkpoints). This is also
substantially faster than commerce-native's own existing
`facet_counts_by_scan` (23.5x-89.3x).

**This confirms P6D-E00's hypothesis, and confirms the mechanistic
explanation P6C-E01 offered was directionally correct and generalizable
beyond Lucene**: the facet crossover this project has now characterized
five times (Phase 5, 6A, 6B, P6C-E00, and this phase's own prior scan
baseline) was a property of *naive per-candidate scanning specifically*
— string hashing, map cloning/insertion, and (in commerce-native's own
case) a full attribute-map clone per candidate — not an inherent
limitation of doing structural retrieval in-process rather than through
a mature generic engine. Given a comparably specialized counting
strategy, commerce-native's own architecture does not merely close the
gap to a mature, decades-tuned engine — it substantially exceeds it, on
this real workload.

**Why the margin here is larger than Lucene's own module's margin over
Solr**: `facet_counts_by_scan`'s naive baseline paid a genuinely more
expensive per-candidate cost than Lucene's own naive scan did (a full
`BTreeMap<String, AttributeValue>` clone via `effective_attributes` on
every iteration, versus Lucene's plain ordinal lookup with no map-clone
equivalent) — so removing that cost via the ordinal design had more
room to help. A disclosed, mechanistic explanation, not independently
profiled.

**Confirmed across the entire Phase 6B scale ladder, not just WANDS'
natural 1x scale (P6D-E01)**: extending the same measurement to Phase
6B's own controlled-stress replication (2x/5x/10x/20x, up to 859,880
products / 320,780 candidates at the largest checkpoint), the ordinal
method beat Solr at every one of 35 checkpoint x tier combinations, with
zero exceptions — margins from 2.5x to 72.6x. **The margin is not
scale-invariant: it narrows, converging toward roughly 2.5x-3x at the
largest candidate counts tested, rather than growing indefinitely** (at
Furniture, the largest checkpoint: 5.2x at 1x → 4.4x at 2x → 3.4x at 5x
→ 2.5x at 10x and 20x). By contrast, the ordinal method's margin over
commerce-native's own scan method *grows* sharply with scale (20.6x-99.6x
at 1x, up to 118.2x-327.0x at 10x/20x) — the mirror image, consistent
with the scan method's `BTreeMap`-clone cost getting relatively worse at
scale while both other methods scale closer to linearly. All 85 rows
(17 x 5 tiers) and 35 top-50-facet checks passed correctness with zero
mismatches. See "P6D-E01" in `docs/experiments/PHASE6D_LOG.md` for the
full table and mechanism discussion.

**A real crossover found in the technique itself, not just in the
comparison against Solr (P6D-E02, an adversarial test of the earlier
finding's generality)**: extending the ordinal technique to the three
dedicated typed-ID facets (`brand`/`category`/`product_type`) tested
whether it still helps when the naive baseline never paid the expensive
attribute-map clone `color`'s baseline paid (these dedicated `_by_scan`
methods read the typed ID directly, no clone, no string hash). Result:
**the ordinal method is SLOWER than the existing scan method at small
candidate counts** (4.8x-5.2x slower at n=2, 1.9x-2.4x slower at n=13),
**and only becomes faster past a real threshold** (2.3x faster at
n=121, 6.2x faster at n=1,103) — a genuine crossover, not a uniform win,
caused by the ordinal method's own fixed cost (allocating and zeroing a
counter array sized to the full attribute dictionary, ~860 distinct
`product_class` values in WANDS) dominating when there are too few
candidates to amortize it. Against Solr the ordinal method still wins
by a wide margin (142x-944x) at every candidate count tested here, but
this specific comparison is not very informative at these particular
counts (2-1,103) since Solr's own cost is consistent with being
network/HTTP-bound rather than facet-algorithm-bound at this scale. See
"P6D-E02" in `docs/experiments/PHASE6D_LOG.md` for the full table and
mechanism discussion.

Full tables, raw CSVs, console logs: `docs/experiments/PHASE6D_LOG.md`,
`docs/research/artifacts/p6d_e00_ordinal_facet_run1/`,
`docs/research/artifacts/p6d_e01_scale_ladder_run1/`,
`docs/research/artifacts/p6d_e02_typed_facet_ordinal_run1/`.

## Failed / fixed experiments (preserved, not erased)

No build/API failures this pass (unlike P6C-E00/E01's Lucene-side Maven/
API issues) — this is native Rust code in an already-familiar crate. One
real compile-time borrow-checker fix during implementation: an initial
`index_enum_value` draft used a closure inside `or_insert_with` that
attempted to re-borrow `self.enum_dictionary` from within a closure
already holding a live borrow of it via an outer `let dictionary = ...`
binding — restructured to hold both the `enum_dictionary` and
`enum_value_ordinal` mutable borrows as plain sequential `let` bindings
(disjoint fields, no closure re-borrow) instead, which resolved cleanly
and is the version committed.

## Unresolved risks

1. **Resolved by P6D-E02, with a materially important qualifier**: the
   dedicated `brand`/`category`/`product_type` facets were measured.
   The ordinal technique does NOT win uniformly there — it has its own
   real crossover (slower than the existing scan at n=2/13, faster at
   n=121/1,103), because those baselines never paid the per-candidate
   allocation `color`'s baseline did. `brand`/`category` have unit-test
   correctness coverage but no real-data benchmark (only `product_type`
   had an existing Solr-comparable operation to extend); other generic
   `Enum` attributes beyond `color` remain untested in a real-data
   benchmark.
2. **Resolved by P6D-E01, with a real nuance**: the Phase 6B scale
   ladder (2x-20x) was repeated for the ordinal method. The margin over
   Solr holds (never crosses into a loss) across the whole 1x-20x range
   tested, but it narrows — not grows — at the largest candidate counts,
   converging toward roughly 2.5x-3x rather than an ever-widening
   advantage. Whether this narrowing continues, plateaus, or reverses
   beyond the ~320,780-candidate ceiling tested here is itself now the
   open question (see "What would be built next").
3. **Resolved by P6D-E03**: the earlier ~172 KB figure was an
   unmeasured analytical estimate, and `approximate_size_bytes()` — this
   whole campaign's canonical, 21-file-referenced memory-size metric —
   had silently omitted every Phase 6D ordinal-facet structure entirely
   (a real accounting gap, not just an unmeasured one). Both are now
   fixed: `approximate_size_bytes()` accounts for the new structures, and
   a new `approximate_ordinal_facet_bytes()` method isolates their
   contribution. Measured on the real WANDS catalog (42,994 products,
   `brand`/`category`/`product_type` columns plus any `Enum` attribute
   columns actually indexed): **2,876,248 bytes total for the ordinal-
   facet structures, 26.2% of the whole index's 10,984,302 measured
   bytes, 66.90 bytes/product** — roughly 16.7x the earlier ~172 KB
   estimate, still small in absolute terms but a materially larger share
   of the index than the estimate implied. This is not a dedicated
   RSS/allocator-level benchmark (Phase 7's own methodology); it is the
   same flat byte-size/string-length approximation
   `approximate_size_bytes()` has always used, not allocator bookkeeping
   or `HashMap` bucket overhead.
4. **`MultiEnum` attributes are not supported by the ordinal path at
   all**, by design (matching `facet_counts_by_scan`'s existing scope,
   which also silently skips `MultiEnum`) — a real, disclosed scope
   limitation on which attributes this technique currently covers, not a
   correctness bug.
5. **The specific mechanistic explanation (removing a per-candidate
   `BTreeMap` clone) is inferred, not profiled** — no JFR/perf/valgrind
   run confirms this is the dominant cost `facet_counts_by_scan` was
   paying, though it is the standard, expected cost of the code as
   written.
6. **This result is measured against Solr's `facet.field`, not directly
   against Lucene's own facet module** — a genuine three-way native-
   scan/native-ordinal/Lucene-module comparison (all three in the same
   session, same checkpoints) was not run; P6C-E01's Lucene numbers are
   referenced for context from a separate binary/session, not
   re-measured here.
7. **No integration with `CatalogIndex::execute`/`execute_ranked` or any
   real query-serving path was changed** — this phase adds new,
   additive counting methods next to the existing ones. P6D-E02's own
   crossover finding means "prefer ordinal by default" is now known to
   be the WRONG design for a real planner — any future integration needs
   a candidate-count-aware (or field-cardinality-aware) choice between
   strategies, not a blanket default, for the typed-ID facets at least.
8. **P6D-E02's crossover point (somewhere between n=13 and n=121 for
   `product_type`'s ~860-value dictionary) is bracketed, not pinpointed**
   — the exact threshold, and whether it scales predictably with
   dictionary size (a smaller-cardinality field like `brand` would be
   expected to cross over at a smaller n, since its fixed cost is
   smaller), is untested.

## What would be built next if scaling up

1. **Design and measure a candidate-count/dictionary-size-aware strategy
   selector** (not a blanket default) for any future query-serving/
   planner code — P6D-E00/E01 show the ordinal method should be
   preferred for `color`-like fields at essentially any realistic scale,
   but P6D-E02 shows the dedicated typed-ID facets need the scan method
   preferred below their own crossover point and the ordinal method
   above it. This is now a concrete design question with real data
   behind it, not a speculative one.
2. **Pinpoint P6D-E02's own crossover point precisely** (currently
   bracketed between n=13 and n=121 for `product_type`) and test whether
   it scales with dictionary size as expected (`brand`/`category`, with
   presumably smaller dictionaries than `product_type`'s ~860 values,
   would be predicted to cross over at a smaller n) — real-data
   benchmarks for `brand`/`category` specifically, and at candidate
   counts large enough to be Solr-comparison-informative (matching
   color's 2,002+ range), were not run in this pass.
3. **Extend past P6D-E01's own 320,780-candidate ceiling** to determine
   whether the observed margin-narrowing trend continues, plateaus, or
   reverses at organically larger (not just replication-scaled)
   candidate counts and facet cardinalities — the Phase 6B replication
   methodology deliberately holds facet cardinality fixed, so this
   would need a genuinely larger real catalog, not further replication.
4. **A dedicated RSS/allocator-level memory measurement** for the new
   per-attribute dictionary/column structures, using Phase 7's own
   established RSS methodology — P6D-E03 replaced the analytical
   estimate with a real, measured flat-byte-size number
   (`approximate_ordinal_facet_bytes()`), but a true RSS delta
   (`/proc` before/after, matching Phase 7's own convention) has still
   not been run for this specific structure set.
5. **Profile the mechanism** (JFR/perf/valgrind) to confirm the
   `BTreeMap`-clone-removal explanation, and to characterize how much of
   the remaining ~15-238μs the ordinal method itself still costs (array
   bounds-checked access, `RoaringBitmap` iteration overhead) versus
   theoretical minimum.
6. **A genuine three-way native-ordinal/Lucene-facet-module/Solr
   comparison**, all measured in the same session, to determine whether
   commerce-native's ordinal approach and Lucene's own are comparably
   fast, or whether one still meaningfully beats the other.

## What should explicitly not be built yet

- **Wiring any of the ordinal methods (`color`, `brand`, `category`,
  `product_type`) as an unconditional default facet-counting path** —
  P6D-E02 found this would be an outright regression for the typed-ID
  facets below their own real crossover point (n<~100-1,000 depending on
  field), and `MultiEnum` attributes have no ordinal path at all yet. Any
  real integration needs the candidate-count/dictionary-size-aware
  selection named above, not a blanket switch.
- **A distributed/sharded ordinal-dictionary design** (per-shard local
  ordinals needing global reconciliation) — this phase's `CatalogIndex`
  remains single-node/single-process, consistent with CLAUDE.md's
  "avoid distributed systems work until the single-node thesis has been
  measured," and nothing in this result changes that sequencing.
- **Declaring the facet-crossover question fully closed campaign-wide**
  — this phase closes it decisively for the specific operation measured
  (color facet-scan under category filter, across WANDS' natural 1x
  scale and the full Phase 6B 2x-20x controlled-stress ladder); the
  named unresolved risks above (other fields, memory cost, organic
  growth beyond the replication ladder) are real, not merely formal,
  caveats.
- **Assuming the margin over Solr grows without bound as scale
  increases** — P6D-E01 found the opposite: it narrows at the largest
  candidate counts tested. Any future capacity/scaling claim should use
  the observed ~2.5x-3x floor at large candidate counts, not the larger
  margins seen at small-to-medium ones, as the conservative planning
  number.

## What this decision does and does not claim

**Does claim**: for the generic `Enum` field this project's own
repeated facet-crossover finding was built on (`color`), an ordinal/
dictionary-based facet-counting method, correctness-gated exactly
against both `facet_counts_by_scan` (unit test) and Solr's own live
facet response (21/21 real-data matches at WANDS' natural 1x scale,
plus 35/35 more across the full Phase 6B 2x-20x scale ladder — 56/56
total), beats Solr at every checkpoint tested across the *entire* 1x-20x
range (2,002-320,780 candidates), by 2.5x to 72.6x with zero exceptions,
and beats commerce-native's own existing scan-based method by
20.6x-327.0x. This confirms the facet crossover this project has
repeatedly measured (Phase 5, 6A, 6B, P6C-E00) is a property of naive
per-candidate scanning specifically, not an inherent ceiling on
commerce-native's own architecture, for fields whose naive baseline pays
a real per-candidate allocation cost.

**Also does claim (P6D-E02, an adversarial finding, not a coverage
extension)**: the ordinal technique is not a universal, unconditional
win. Extended to the dedicated `brand`/`category`/`product_type` facets
— whose existing `_by_scan` baselines never paid the attribute-map-clone
cost `color`'s baseline did — the ordinal method has its own real
crossover, correctness-gated the same way: **slower than the existing
scan method at small candidate counts** (4.8x-5.2x slower at n=2,
1.9x-2.4x slower at n=13) **and faster only past a real threshold**
(2.3x faster at n=121, 6.2x faster at n=1,103), because it trades a
fixed, dictionary-size-proportional per-call cost (zeroing a counter
array sized to the full attribute dictionary) for per-candidate savings
that are smaller when there's no clone to remove.

**Does not claim**: that the margin over Solr is scale-invariant or
grows with scale for `color` — it narrows, converging toward roughly
2.5x-3x at the largest candidate counts tested (P6D-E01's own real,
disclosed nuance); that the ordinal technique wins unconditionally for
any facet field — P6D-E02 shows it does not, for the dedicated typed-ID
facets below their own crossover point; that this crossover point is
precisely known (bracketed between n=13 and n=121 for `product_type`,
not pinpointed); that `brand`/`category` have real-data (not just
unit-test) benchmark coverage (they do not); that it holds beyond the
~320,780 candidates tested for `color`, or under organic (not
replication-scaled) facet cardinality growth; that the new structures'
memory cost is negligible via a dedicated RSS/allocator-level
benchmark — P6D-E03 replaced the earlier unmeasured estimate with a
real flat-byte-size measurement (2,876,248 bytes, 26.2% of the whole
index, on the real WANDS catalog), but that is not the same thing as an
RSS delta; that `MultiEnum`
attributes are covered (explicitly out of scope); that any real
query-serving path has been changed to prefer either method by default
(it has not, and P6D-E02 shows an unconditional default would be wrong
for the typed-ID facets specifically); or that commerce-native's ordinal
approach is faster or slower than Lucene's own equivalent module
specifically (not directly compared in the same session).

**Decision: PROCEED.** This phase answers the single highest-value
question P6C-E01 surfaced, with a result more decisive than that
question's own framing anticipated for the field it was tested on:
commerce-native's own architecture, given a comparably specialized
counting strategy, substantially exceeds both Solr and Lucene's own
margin over Solr for `color`-like generic `Enum` attributes. But this
phase's own adversarial follow-up (P6D-E02) earned its keep by finding
the real boundary of that result: the technique is not free, and not
universal — it has its own crossover for fields whose naive baseline
was already cheap. That is not a weaker result; it is a more honest and
more useful one, because it tells a future implementer exactly when to
reach for this technique and when not to, rather than leaving them to
discover the crossover the hard way in production. The unresolved risks
above are real scope boundaries on a genuinely positive, now
better-characterized result, not reasons to doubt it — every claim
above is correctness-gated, not asserted from timing alone.
