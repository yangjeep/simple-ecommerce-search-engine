# Issue #55 — first exhaustive, reachability-verified hyponym audit: decision

Full log: `docs/experiments/ISSUE55_HYPONYM_REACHABILITY_AUDIT_LOG.md`.
Raw artifact: `docs/research/artifacts/i55_hyponym_reachability_audit/run1.txt`.

## Governing question

Every prior "false-family audit" of `product_type_hyponym_groups`
(checkpoint 11's original, checkpoint 14's re-audit, the later Priority
1A/2 session) checked specific, previously-named groups against the
current mechanism -- none had ever read through, and mechanically
verified the query-time reachability of, the entire live 149-group
candidate set at once. Does a genuinely complete audit find any
confirmed cross-family false positive beyond the already-disclosed
`"beds"`->pet-products case?

## Result: no new confirmed defect; the mechanism's real exposure is smaller than the raw candidate-group count suggests

**A visual read of the full `p9_e08` dump found a plausible-looking new
false positive** (`"plates"` admitting `"switch plates"`/`"burners &
hot plates"`) that did **not** survive direct mechanical verification:
compiling the literal query text produces no structural constraint at
all, because the actual registered "broader term" for that group is an
11-word synthesized ancestor-breadcrumb path no real shopper would type,
and even the exact path string resolves to a soft `Preference`, not the
`ProductTypeAny`. This is disclosed as a self-caught overclaim, not
smoothed over: the first-pass reading was wrong, and the fix was to
verify by execution before publishing, exactly the discipline this
project's own research process calls for.

**A systematic, mechanical sweep** (`i55_hyponym_reachability_audit`,
compiles every group's own literal broader-term text against the real
current-default lexicon) found:

- 79 of 149 groups (53%) are actually reachable via their own literal
  text; 70 (47%) are shadowed by a competing `Preference` candidate and
  are not reachable this way at all, regardless of their raw
  narrower-name list looking risky on paper.
- Every one of the 79 reachable groups' narrower-name lists was read in
  full (not sampled). Exactly one confirmed cross-family false positive
  exists: the already-known `"beds"` -> `"cat beds"`/`"dog beds & mats"`.
  No new confirmed violation.
- One low-practical-risk edge case: `"accent chests / cabinets"` ->
  `"dartboards and cabinets"`, reachable only via an exact literal
  taxonomy-label string (containing `"/"`) that free-text search would
  essentially never produce.

## Verdict: CONFIRMS checkpoint 14's KEEP verdict, on stronger evidence than it originally had; no production change

`ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md`'s KEEP verdict was based on
rechecking three previously-named false positives plus the one already-
disclosed residual risk -- not a full, independent re-audit. This
checkpoint supplies the full, independent re-audit that was actually
missing, and it **confirms** (does not overturn) the KEEP verdict: no
new production-relevant defect was found, and the disclosed residual
risk remains exactly as sized as before (one group, two narrower names).
No change to `compile_lexicon`'s default, `product_type_hyponym_groups`,
or any other production code.

**A genuinely new, useful finding, independent of the false-positive
question**: 47% of the raw candidate-group list is not reachable via
its own literal text at query time at all. This matters for how
`p9_e08`'s own dump should be read going forward -- a narrower-name
list under an unreachable broader term describes a real entry in the
`product_type_hyponym_groups` data structure, but not a live query-time
risk surface, until proven otherwise by direct compilation.

## Separately, a disclosed correction to a different log

While investigating this, found and corrected (dated addendum, not a
rewrite) a factual error in `ISSUE55_SEMANTIC_PROMOTION_LOG.md`: it
claimed the "currently live candidate set" was exactly two groups
(`"beds"`, `"recliners"`), and separately cited "the full 245-group
candidate set" elsewhere in the same document -- neither number is
correct. The real, continuously-stable count since checkpoint 14 is 149
groups / 317 pairs, confirmed byte-identical against checkpoint 14's own
saved artifact. This does not invalidate that log's own numeric
result for the two groups it actually tested, only its claim about how
representative that sample was; this session's own promotion-gate work
(`ISSUE55_PROMOTION_GATE_FULL_SET_DECISION.md`) already independently
used the correct, full 149-group set throughout, unaffected by the
other log's error.

## Real caveats, disclosed rather than smoothed over

- **Reachability was tested only via each group's own exact literal
  broader-term text**, matching how `p9_e08`'s dump presents each group.
  A real user query paraphrasing or partially matching a reachable
  group's text (e.g. plural/singular variants, word order) was not
  swept -- this audit answers "is the raw registered name itself a live
  trigger," not "is there any phrasing that could trigger it."
- **The `"accent chests / cabinets"` edge case is disclosed, not
  dismissed**: it is real and mechanically confirmed reachable via its
  exact string, just judged low-practical-risk for free-text search
  specifically. If this project ever exposes literal taxonomy labels as
  query input (e.g. a browse/facet click passing a category string
  through the same `compile()` path), this reopens as a real, live
  concern rather than a theoretical one.
- **77 "genuinely on-topic" groups were judged by direct human reading,
  not an automated correctness proof.** This is the same standard
  `p9_e08`'s own methodology has always used (human audit of a printed
  dump); it is not infallible, but it is now complete rather than
  targeted.

## What this does NOT establish

- Not a claim that `product_type_hyponym_groups`' underlying mechanism
  is provably free of any false positive under every possible future
  catalog change -- only that the current, live, real-WANDS-vocabulary
  instantiation has exactly one confirmed reachable violation, now
  verified exhaustively rather than assumed.
- Not a change to any preregistered gate, threshold, or production
  code. This is a verification checkpoint, not an experiment with a
  GO/REVISE/REJECT gate of its own beyond confirming the existing one.
