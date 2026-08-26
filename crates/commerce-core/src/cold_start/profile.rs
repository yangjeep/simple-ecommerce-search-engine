use std::collections::{BTreeMap, BTreeSet};

use super::alias;
use super::canonicalize::{CanonicalizationEvidence, VocabularyCanonicalizer};
use crate::domain::{
    AttributeMap, AttributeValue, Brand, BrandId, Catalog, Category, CategoryId, Constraint,
    ProductType, ProductTypeId,
};
use crate::ir::{Candidate, Preference, ResolvedConstraint, SemanticLexicon, StructuralConstraint};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EnumSource {
    attribute: String,
    value: String,
    is_multi: bool,
}

/// Everything Gate 6's cold-start profiler can extract from a catalog
/// snapshot without a model call: one pass over every product/variant,
/// deduplicated by value (CLAUDE.md: "do not perform one LLM call per
/// SKU" — this profiler makes zero calls at all, structural counting only).
/// Values are indexed by their lowercase form so [`compile_lexicon`] can
/// key lexicon entries the same way `ir::query::compile` looks them up,
/// but each source retains the *original* casing to build a correct
/// `Constraint`. Two different attributes sharing the same lowercase
/// value (e.g. "green" as both a color and a feature tag) fall out
/// naturally as multiple `EnumSource`s under one key — a genuine
/// ambiguity the profiler cannot and must not silently resolve.
#[derive(Debug, Default)]
pub struct CatalogProfile {
    brand_names: BTreeMap<String, BrandId>,
    product_type_names: BTreeMap<String, ProductTypeId>,
    category_names: BTreeMap<String, CategoryId>,
    boolean_attributes: BTreeSet<String>,
    enum_candidates: BTreeMap<String, BTreeSet<EnumSource>>,
    /// How many catalog attribute *occurrences* (not distinct sources) each
    /// lowercased enum value string was seen under, across every
    /// product/variant. Round 1 R1-E02/E02b (`docs/experiments/ROUND1_LOG.md`)
    /// found that on a real 1.2M-product catalog, naively trusting every
    /// distinct raw field value as a lexicon entry produces catastrophic
    /// filter recall (5.0% against real Exact-labeled relevant products):
    /// noisy, one-off data-entry values ("#2", "Without Lids", "10 Gallon")
    /// become hard-filter-worthy exactly as confidently as genuine,
    /// repeatedly-used categorical values ("Black", "Nike"). Occurrence
    /// frequency is used as a deterministic (zero model call), canonicalization
    /// signal in [`compile_lexicon`]: a real controlled-vocabulary value gets
    /// reused across many products; a one-off data-entry mistake typically
    /// does not.
    enum_occurrence: BTreeMap<String, usize>,
    /// How many products carry each lowercased brand name. Added by P2-E05
    /// (`docs/experiments/PHASE2_LOG.md`) after a real-data integration run
    /// found this project's own prior assumption wrong: `compile_lexicon`'s
    /// doc comment claimed brand vocabulary "comes from an already-curated
    /// registry," so it was never subject to `min_enum_frequency`
    /// filtering the way `enum_occurrence`-backed values are. On the real
    /// ESCI catalog, brand is populated the *same* way color was
    /// (`round1_eval::catalog::build_catalog` interns whatever raw string
    /// a product's source record puts in its brand field) --
    /// 206,227 distinct real "brands," 49.4% occurring on exactly one
    /// product, the great majority of those one-off values being seller
    /// junk ("funny musician gifts co") rather than genuine brand names.
    /// R1-E02/E02b's exact original failure mode, just never checked for
    /// this specific vocabulary.
    brand_occurrence: BTreeMap<String, usize>,
    numeric_values: BTreeMap<String, Vec<f64>>,
    price_cents: Vec<i64>,
}

impl CatalogProfile {
    pub fn build(
        catalog: &Catalog,
        brands: &[Brand],
        product_types: &[ProductType],
        categories: &[Category],
    ) -> Self {
        let mut profile = CatalogProfile::default();
        let mut brand_name_by_id: BTreeMap<BrandId, String> = BTreeMap::new();
        for b in brands {
            let lower = b.name.to_lowercase();
            profile.brand_names.insert(lower.clone(), b.id);
            brand_name_by_id.insert(b.id, lower);
        }
        for pt in product_types {
            profile
                .product_type_names
                .insert(pt.name.to_lowercase(), pt.id);
        }
        for c in categories {
            profile.category_names.insert(c.name.to_lowercase(), c.id);
        }

        for product in &catalog.products {
            if let Some(name) = brand_name_by_id.get(&product.brand) {
                *profile.brand_occurrence.entry(name.clone()).or_insert(0) += 1;
            }
            profile.index_attributes(&product.attributes);
            for variant in &product.variants {
                profile.index_attributes(&variant.attributes);
                profile.price_cents.push(variant.price.amount_cents);
            }
        }

        profile.price_cents.sort_unstable();
        profile.price_cents.dedup();
        for values in profile.numeric_values.values_mut() {
            values.sort_by(f64::total_cmp);
            values.dedup();
        }
        profile
    }

    fn index_attributes(&mut self, attrs: &AttributeMap) {
        for (name, value) in attrs {
            match value {
                AttributeValue::Enum(v) => self.index_enum_source(name, v, false),
                AttributeValue::MultiEnum(vs) => {
                    for v in vs {
                        self.index_enum_source(name, v, true);
                    }
                }
                AttributeValue::Boolean(true) => {
                    self.boolean_attributes.insert(name.clone());
                }
                AttributeValue::Boolean(false) => {}
                AttributeValue::Numeric(n) => {
                    self.numeric_values
                        .entry(name.clone())
                        .or_default()
                        .push(*n);
                }
                // Free text has no discrete vocabulary to profile; Gate 3's
                // narrow-then-verify path already handles Text at query time.
                AttributeValue::Text(_) => {}
            }
        }
    }

    fn index_enum_source(&mut self, attribute: &str, value: &str, is_multi: bool) {
        let key = value.to_lowercase();
        *self.enum_occurrence.entry(key.clone()).or_insert(0) += 1;
        self.enum_candidates
            .entry(key)
            .or_default()
            .insert(EnumSource {
                attribute: attribute.to_string(),
                value: value.to_string(),
                is_multi,
            });
    }

    /// How many catalog attribute occurrences `value_lower` (already
    /// lowercased) was seen under. 0 if never seen.
    pub fn enum_occurrence_count(&self, value_lower: &str) -> usize {
        self.enum_occurrence.get(value_lower).copied().unwrap_or(0)
    }

    /// How many products carry `name_lower` (already lowercased) as their
    /// brand. 0 if never seen.
    pub fn brand_occurrence_count(&self, name_lower: &str) -> usize {
        self.brand_occurrence.get(name_lower).copied().unwrap_or(0)
    }

    pub fn product_type_names(&self) -> impl Iterator<Item = &str> {
        self.product_type_names.keys().map(String::as_str)
    }

    /// Paired with [`product_type_hyponym_groups`] so evaluation tooling
    /// can audit real hyponym groups (e.g. for cross-family false
    /// positives) without duplicating `CatalogProfile`'s internal name
    /// collection logic.
    pub fn product_type_names_with_ids(&self) -> impl Iterator<Item = (&str, ProductTypeId)> {
        self.product_type_names
            .iter()
            .map(|(name, id)| (name.as_str(), *id))
    }

    pub fn brand_names(&self) -> impl Iterator<Item = &str> {
        self.brand_names.keys().map(String::as_str)
    }

    pub fn boolean_attributes(&self) -> impl Iterator<Item = &str> {
        self.boolean_attributes.iter().map(String::as_str)
    }

    pub fn enum_value_keys(&self) -> impl Iterator<Item = &str> {
        self.enum_candidates.keys().map(String::as_str)
    }

    pub fn numeric_values(&self, attribute: &str) -> &[f64] {
        self.numeric_values
            .get(attribute)
            .map_or(&[], Vec::as_slice)
    }

    pub fn max_price_cents(&self) -> Option<i64> {
        self.price_cents.last().copied()
    }

    /// How many distinct raw values were seen (brands + product types +
    /// categories + boolean attributes + distinct enum/multi-enum value
    /// strings) versus how many catalog attribute occurrences were
    /// compressed into them — the "profile/compress semantic problems"
    /// half of Gate 6, reported as a ratio rather than asserted on, since
    /// the interesting number depends entirely on the catalog's own
    /// diversity.
    pub fn distinct_value_count(&self) -> usize {
        self.brand_names.len()
            + self.product_type_names.len()
            + self.category_names.len()
            + self.boolean_attributes.len()
            + self.enum_candidates.len()
    }
}

/// Deterministically derive a [`SemanticLexicon`] from a [`CatalogProfile`]
/// — no model call, no randomness. Every derived attribute-value mapping
/// becomes a *hard* constraint candidate: unlike the hand-curated
/// `fixtures::shoe_lexicon`, the profiler has no signal to distinguish a
/// decisive attribute (color, size) from a descriptive/soft one
/// (cushioned, breathable), so it cannot propose `ir::Preference`s the way
/// a human curator did in Gate 2/4 — see `docs/adr/0006-cold-start-fuzzing.md`.
///
/// `min_enum_frequency` is the Round 1 R1-E02/E02b canonicalization fix
/// (`docs/experiments/ROUND1_LOG.md`, `docs/adr/0008-narrow-to-structural-planning-layer.md`):
/// a value must have been seen at least this many times across the
/// catalog to become a trusted lexicon entry at all. `1` means "no
/// filtering" — every Phase 0 test fixture uses `1` and keeps its exact
/// pre-existing behavior.
///
/// Applies to brand names too, as of P2-E05
/// (`docs/experiments/PHASE2_LOG.md`). This function's own prior doc
/// comment claimed brand/product-type/category/boolean vocabulary came
/// from "an already-curated registry" and was exempt by design — true for
/// `ProductType`/`Category` (this project's own fixtures/ingestion define
/// a small, deliberate registry for those), but **not** actually true for
/// `Brand` on the real ESCI catalog: `round1_eval::catalog::build_catalog`
/// interns brand the same way it interns raw `color` values, from
/// whatever a source record's brand field contains, no validation.
/// P2-E05's real-data integration run found this the hard way (badly
/// degraded end-to-end relevance, traced to single-product "brand"
/// candidate sets built from one-off seller-junk strings) before it was
/// confirmed independently (206,227 distinct real "brand" strings,
/// 49.4% occurring on exactly one product). Product-type/category remain
/// unfiltered: nothing in this project's evidence base has shown them to
/// have the same raw-noise problem, since the real catalog ingestion
/// pipeline assigns every real product the same sentinel
/// `ProductTypeId(0)`/`CategoryId(0)` rather than deriving them from a
/// noisy per-product field at all (`round1_eval::catalog`'s own doc
/// comment) — there is no equivalent noisy source to canonicalize away.
pub fn compile_lexicon(profile: &CatalogProfile, min_enum_frequency: usize) -> SemanticLexicon {
    // Delegates to compile_lexicon_with_product_type_hyponyms(..., true)
    // rather than duplicating its brand-loop, so "true is byte-identical to
    // compile_lexicon" is compiler-enforced, not merely test-enforced (a
    // future edit to one loop and not the other could otherwise silently
    // break that invariant outside the narrow case the regression tests
    // below exercise -- flagged by adversarial review of
    // `docs/decisions/ISSUE55_PAIRED_COMPARATOR_DECISION.md`).
    compile_lexicon_with_product_type_hyponyms(profile, min_enum_frequency, true)
}

/// Issue #55 checkpoint-14 follow-up
/// (`docs/decisions/ISSUE55_PAIRED_COMPARATOR_DECISION.md`, Priority 1A
/// of the Issue #55 falsification loop): identical to [`compile_lexicon`]
/// except `ProductTypeAny` hyponym expansion
/// ([`product_type_hyponym_groups`]) can be switched off, so evaluation
/// tooling can build a "baseline" lexicon (plain per-id `ProductType`
/// matching only, the pre-checkpoint-14 production behavior) and a
/// "treatment" lexicon (current production, hyponym expansion on) from
/// the exact same `CatalogProfile` and `min_enum_frequency`, isolating
/// the one variable a paired before/after comparison needs held apart
/// from everything else compile_lexicon does. `enable_product_type_hyponyms:
/// true` is byte-identical to [`compile_lexicon`] (see
/// `product_type_hyponym_toggle_true_matches_compile_lexicon` below) --
/// this function does not change production behavior on its own; it
/// only exposes the switch [`compile_lexicon`] always sets to `true`.
pub fn compile_lexicon_with_product_type_hyponyms(
    profile: &CatalogProfile,
    min_enum_frequency: usize,
    enable_product_type_hyponyms: bool,
) -> SemanticLexicon {
    let mut lex = SemanticLexicon::new();
    for (name, id) in &profile.brand_names {
        if profile.brand_occurrence_count(name) < min_enum_frequency {
            continue;
        }
        lex.insert(
            name,
            vec![Candidate::constraint(
                ResolvedConstraint::Structural(StructuralConstraint::Brand(*id)),
                1.0,
            )],
        );
    }
    compile_non_brand_lexicon(
        profile,
        min_enum_frequency,
        enable_product_type_hyponyms,
        &mut lex,
    );
    lex
}

/// Issue #9: identical to [`compile_lexicon`] for every field except
/// brand, where inclusion is decided by a pluggable [`VocabularyCanonicalizer`]
/// (`docs/research/brand-adjudication-rubric.md`'s five-class taxonomy)
/// instead of the raw `min_enum_frequency` occurrence-count gate. Isolates
/// exactly the one variable Issue #9's real adjudication corpus/ground
/// truth actually covers -- brand vocabulary -- so a caller comparing this
/// against `compile_lexicon` measures the canonicalizer swap alone.
/// Enum-value canonicalization was not adjudicated and stays on the same
/// raw-threshold gate here as `compile_lexicon` uses, via the same
/// `min_enum_frequency` parameter.
///
/// `representative_titles` is a caller-supplied lookup (a handful of
/// titles per lowercased brand name, the same bounded evidence shape
/// `CanonicalizationEvidence` requires) rather than something this
/// function derives itself: `CatalogProfile` does not retain per-value
/// product titles (Gate 6's whole point is compressing a catalog into
/// counts/keys, not retaining per-SKU data), so the caller (which still
/// has the ingested `Catalog`) must provide it.
pub fn compile_lexicon_with_brand_canonicalizer(
    profile: &CatalogProfile,
    min_enum_frequency: usize,
    canonicalizer: &dyn VocabularyCanonicalizer,
    representative_titles: impl Fn(&str) -> Vec<String>,
) -> SemanticLexicon {
    let mut lex = SemanticLexicon::new();
    for (name, id) in &profile.brand_names {
        let titles = representative_titles(name);
        let evidence = CanonicalizationEvidence {
            value: name,
            occurrence_count: profile.brand_occurrence_count(name),
            representative_titles: &titles,
        };
        if !canonicalizer.classify(&evidence).trusted_as_structural() {
            continue;
        }
        lex.insert(
            name,
            vec![Candidate::constraint(
                ResolvedConstraint::Structural(StructuralConstraint::Brand(*id)),
                1.0,
            )],
        );
    }
    compile_non_brand_lexicon(profile, min_enum_frequency, true, &mut lex);
    lex
}

/// Issue #6 P1-B: tests the *enforcement* mechanism around an
/// already-trusted brand match, not which strings are trusted -- Issue #9
/// already answered that question three independent ways
/// (`docs/experiments/PHASE2_LOG.md` P2-E07-E10, decision: CANONICALIZATION
/// FRONTIER IS FUNDAMENTAL). Same `min_enum_frequency` trust gate
/// [`compile_lexicon`] uses, applied identically here, so a caller
/// comparing this against `compile_lexicon` measures the enforcement swap
/// alone -- the same isolation discipline
/// [`compile_lexicon_with_brand_canonicalizer`]'s own doc comment
/// establishes for swapping the canonicalizer instead.
///
/// Two independent, confidence-tiered enforcement mechanisms:
///
/// - **Tier 1 (hard `Constraint`, deterministic)**: trusted brand names
///   that collapse to the same [`alias::alias_key`] (punctuation/
///   corporate-suffix normalized -- "Nike", "Nike Inc", "Nike, Inc.") are
///   grouped, and the compiled constraint matches *any* `BrandId` in that
///   group ([`StructuralConstraint::BrandAny`]) instead of requiring one
///   exact `BrandId`. A trusted name with no alias siblings behaves
///   identically to [`compile_lexicon`]'s plain `Brand(id)`.
/// - **Tier 2 (soft [`Preference::StructuralBoost`], fuzzy)**: a
///   brand-shaped string in `fuzzy_candidates` that does *not* pass the
///   trust gate on its own, but whose alias key is within
///   `fuzzy_max_edit_distance` of a trusted group's alias key, gets a
///   ranking-only boost toward that group instead of either a hard filter
///   (too risky at this confidence -- a fuzzy match can be a genuinely
///   different brand, e.g. "Nike"/"Nikon") or nothing at all (today's
///   `compile_lexicon` behavior: the term falls through to
///   `residual_lexical` unresolved).
///
/// `fuzzy_candidates` is caller-bounded (e.g. real query vocabulary)
/// rather than every untrusted raw catalog string: fuzzy-matching all
/// ~200K real raw brand strings (`docs/adr/0009-structural-lexical-execution-contract.md`)
/// against every trusted group is neither necessary (most never appear in
/// a real query) nor cheap, and this project's own precedent
/// (`scripts/phase2/build_query_relevant_brand_sample.py`, P2-E10) already
/// established bounding by real query relevance rather than raw catalog
/// size. `fuzzy_max_edit_distance == 0` disables tier 2 entirely (no
/// string is within edit distance 0 of a different string), isolating
/// tier 1's effect alone.
pub fn compile_lexicon_with_alias_enforcement(
    profile: &CatalogProfile,
    min_enum_frequency: usize,
    fuzzy_candidates: &[String],
    fuzzy_max_edit_distance: usize,
) -> SemanticLexicon {
    let mut lex = SemanticLexicon::new();

    let trusted: BTreeMap<&str, BrandId> = profile
        .brand_names
        .iter()
        .filter(|(name, _)| profile.brand_occurrence_count(name) >= min_enum_frequency)
        .map(|(name, id)| (name.as_str(), *id))
        .collect();

    let mut groups: BTreeMap<String, Vec<BrandId>> = BTreeMap::new();
    for (name, id) in &trusted {
        groups.entry(alias::alias_key(name)).or_default().push(*id);
    }

    for name in trusted.keys() {
        let key = alias::alias_key(name);
        let ids = groups.get(&key).cloned().unwrap_or_default();
        lex.insert(
            name,
            vec![Candidate::constraint(
                ResolvedConstraint::Structural(StructuralConstraint::BrandAny(ids)),
                1.0,
            )],
        );
    }

    if fuzzy_max_edit_distance > 0 {
        for candidate in fuzzy_candidates {
            let candidate_lower = candidate.to_lowercase();
            if trusted.contains_key(candidate_lower.as_str()) {
                continue; // already resolved as tier 1, do not downgrade it
            }
            let candidate_key = alias::alias_key(&candidate_lower);
            let candidate_len = candidate_key.chars().count();
            let mut best: Option<(usize, &String)> = None;
            for group_key in groups.keys() {
                // Levenshtein distance is always >= the length difference,
                // so this is a correct, cheap prefilter, not an
                // approximation -- skips the O(len_a * len_b) DP entirely
                // for pairs that cannot possibly be close enough. This
                // matters in practice: without it, this loop was measured
                // taking ~2.3x longer end to end on the real 22,458-query
                // corpus (`docs/experiments/PHASE2_LOG.md` P2-E11) purely
                // from computing edit distances that were always going to
                // exceed the threshold.
                let group_len = group_key.chars().count();
                let len_diff = candidate_len.abs_diff(group_len);
                if len_diff > fuzzy_max_edit_distance {
                    continue;
                }
                let d = alias::edit_distance(&candidate_key, group_key);
                if d <= fuzzy_max_edit_distance && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, group_key));
                }
            }
            if let Some((_, group_key)) = best {
                let ids = groups.get(group_key).cloned().unwrap_or_default();
                lex.insert(
                    &candidate_lower,
                    vec![Candidate::preference(
                        Preference::StructuralBoost(StructuralConstraint::BrandAny(ids), 1.0),
                        0.5,
                    )],
                );
            }
        }
    }

    compile_non_brand_lexicon(profile, min_enum_frequency, true, &mut lex);
    lex
}

/// A product-type name's own trailing path segment: for a name built
/// from `effective_product_class`'s ancestor-breadcrumb-path fallback
/// (e.g. `"furniture / bedroom furniture / beds & headboards / beds"`,
/// `" / "`-joined -- WANDS's own raw `category_depth_N` format), this is
/// the leaf (`"beds"`); for a clean `product_class`-derived name with no
/// `" / "` at all (e.g. `"recliners"`), this is the whole name,
/// unchanged.
fn leaf_segment(name: &str) -> &str {
    name.rsplit(" / ").next().unwrap_or(name)
}

/// Issue #55 (`docs/experiments/ISSUE55_HYPONYM_LEAF_ONLY_PROTOCOL.md`,
/// superseding the first, REJECTed attempt in
/// `docs/experiments/ISSUE55_PRODUCT_TYPE_HYPONYM_PROTOCOL.md`):
/// product type `B` is a whole-word hyponym of `A` when every
/// whitespace-separated word of `A`'s name also appears in **`B`'s own
/// leaf segment** ([`leaf_segment`]) and that leaf has at least one
/// additional word -- e.g. `"recliners"` is a hyponym-parent of
/// `".../ recliners / gray recliners"` (leaf `"gray recliners"`, real
/// WANDS vocabulary: a query resolving to the broader "recliners" type
/// should also admit the more specific "gray recliners" products).
///
/// **Leaf-only, not full-path, comparison**: the first version of this
/// function compared full names, which let a word appearing only in an
/// *ancestor* breadcrumb segment (e.g. `"candles"` in the parent
/// category `"candles & holders"`) spuriously match an unrelated
/// sibling leaf (`"scented oils & diffusers"`) -- a confirmed,
/// real-vocabulary cross-family false positive
/// (`docs/decisions/ISSUE55_PRODUCT_TYPE_HYPONYM_DECISION.md`).
/// Restricting to each name's own leaf segment removes that whole class
/// of false positive by construction (an ancestor segment's words no
/// longer participate in the comparison at all) while preserving
/// genuine within-branch hyponymy. **Deliberately not a complete fix**:
/// two *clean* (non-path) names can still collide on genuine
/// cross-vertical lexical polysemy (e.g. `"beds"` vs. pet `"cat
/// beds"`) -- `leaf_segment` is the identity for either, so this
/// specific risk is unchanged and must be measured/disclosed
/// separately, not assumed away.
///
/// Still deliberately **whole-word**, computed via `split_whitespace`
/// on each leaf, not raw substring containment: `"table"` must never
/// match inside `"turntable"` (one word, no space) the way substring
/// containment would allow, since a turntable is not a kind of table.
/// Built once from real catalog vocabulary already collected by
/// `CatalogProfile` -- no new data, no model call, fully deterministic.
/// Exposed (beyond `compile_non_brand_lexicon`'s internal use) so
/// evaluation tooling can audit which pairs a catalog's real vocabulary
/// actually produces -- e.g. to check for cross-family false positives
/// before trusting a `ProductTypeAny` recall improvement.
pub fn product_type_hyponym_groups(
    product_type_names: &BTreeMap<String, ProductTypeId>,
) -> BTreeMap<ProductTypeId, Vec<ProductTypeId>> {
    let word_sets: Vec<(ProductTypeId, BTreeSet<&str>)> = product_type_names
        .iter()
        .map(|(name, id)| (*id, leaf_segment(name).split_whitespace().collect()))
        .collect();
    let mut groups: BTreeMap<ProductTypeId, Vec<ProductTypeId>> = BTreeMap::new();
    for (a_id, a_words) in &word_sets {
        for (b_id, b_words) in &word_sets {
            if a_id == b_id {
                continue;
            }
            // Strict subset (not equal): `b_words` must have strictly
            // more words than `a_words`, so two distinct names with the
            // exact same leaf word set (rare, but not ruled out by the
            // domain model) never form a spurious hyponym pair either way.
            if a_words.len() < b_words.len() && a_words.is_subset(b_words) {
                groups.entry(*a_id).or_default().push(*b_id);
            }
        }
    }
    groups
}

fn compile_non_brand_lexicon(
    profile: &CatalogProfile,
    min_enum_frequency: usize,
    enable_product_type_hyponyms: bool,
    lex: &mut SemanticLexicon,
) {
    // Issue #55 (`docs/experiments/ISSUE55_HYPONYM_LEAF_ONLY_PROTOCOL.md`):
    // re-wired to the leaf-only-restricted `product_type_hyponym_groups`
    // after the original full-path-comparison version was REJECTed
    // (`docs/decisions/ISSUE55_PRODUCT_TYPE_HYPONYM_DECISION.md`) for
    // confirmed cross-family false positives. The leaf restriction
    // removes that class of false positive by construction; see
    // `docs/decisions/ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md` for the
    // re-audit this checkpoint's own verdict rests on.
    //
    // `enable_product_type_hyponyms=false` (only reachable via
    // `compile_lexicon_with_product_type_hyponyms`, never via
    // `compile_lexicon` itself) reproduces the pre-checkpoint-14 baseline
    // -- plain per-id `ProductType` matching only -- for the paired
    // comparator experiment (`docs/decisions/ISSUE55_PAIRED_COMPARATOR_DECISION.md`).
    let hyponym_groups = if enable_product_type_hyponyms {
        product_type_hyponym_groups(&profile.product_type_names)
    } else {
        BTreeMap::new()
    };
    for (name, id) in &profile.product_type_names {
        let structural = match hyponym_groups.get(id) {
            Some(hyponyms) if !hyponyms.is_empty() => {
                let mut ids = vec![*id];
                ids.extend(hyponyms.iter().copied());
                StructuralConstraint::ProductTypeAny(ids)
            }
            _ => StructuralConstraint::ProductType(*id),
        };
        lex.insert(
            name,
            vec![Candidate::constraint(
                ResolvedConstraint::Structural(structural),
                1.0,
            )],
        );
    }
    for (name, id) in &profile.category_names {
        lex.insert(
            name,
            vec![Candidate::constraint(
                ResolvedConstraint::Structural(StructuralConstraint::Category(*id)),
                1.0,
            )],
        );
    }
    for attribute in &profile.boolean_attributes {
        lex.insert(
            attribute,
            vec![Candidate::constraint(
                ResolvedConstraint::Attribute(Constraint::Boolean {
                    attribute: attribute.clone(),
                    value: true,
                }),
                1.0,
            )],
        );
    }
    for (value_lower, sources) in &profile.enum_candidates {
        if profile.enum_occurrence_count(value_lower) < min_enum_frequency {
            continue;
        }
        let candidates: Vec<Candidate> = sources
            .iter()
            .map(|source| {
                let constraint = if source.is_multi {
                    Constraint::MultiEnumContains {
                        attribute: source.attribute.clone(),
                        value: source.value.clone(),
                    }
                } else {
                    Constraint::Enum {
                        attribute: source.attribute.clone(),
                        value: source.value.clone(),
                    }
                };
                Candidate::constraint(ResolvedConstraint::Attribute(constraint), 1.0)
            })
            .collect();
        lex.insert(value_lower, candidates);
    }
}

#[cfg(test)]
mod hyponym_tests {
    use super::*;

    fn names(pairs: &[(&str, u32)]) -> BTreeMap<String, ProductTypeId> {
        pairs
            .iter()
            .map(|(name, id)| (name.to_string(), ProductTypeId(*id)))
            .collect()
    }

    #[test]
    fn broader_term_admits_a_more_specific_whole_word_superset() {
        let types = names(&[("beds", 1), ("kids beds", 2), ("sofas", 3)]);
        let groups = product_type_hyponym_groups(&types);
        assert_eq!(groups.get(&ProductTypeId(1)), Some(&vec![ProductTypeId(2)]));
        assert_eq!(
            groups.get(&ProductTypeId(2)),
            None,
            "the more specific term has no hyponyms of its own here"
        );
        assert_eq!(
            groups.get(&ProductTypeId(3)),
            None,
            "unrelated term stays untouched"
        );
    }

    /// The adversarial case the whole-word design exists specifically to
    /// reject: "table" must never be treated as a hyponym-parent of
    /// "turntable" just because the substring "table" appears inside it --
    /// a turntable is not a kind of table. `split_whitespace` treats
    /// "turntable" as a single, different word from "table", so no
    /// subset relation holds either direction.
    #[test]
    fn whole_word_matching_rejects_the_turntable_false_positive() {
        let types = names(&[("table", 1), ("turntable", 2)]);
        let groups = product_type_hyponym_groups(&types);
        assert!(
            groups.get(&ProductTypeId(1)).is_none_or(|v| v.is_empty()),
            "\"table\" must not admit \"turntable\" as a hyponym"
        );
        assert!(
            groups.get(&ProductTypeId(2)).is_none_or(|v| v.is_empty()),
            "\"turntable\" must not admit \"table\" as a hyponym either"
        );
    }

    #[test]
    fn identical_word_sets_never_form_a_hyponym_pair() {
        // Two distinct product-type ids that happen to normalize to the
        // same word set in a different order ("chairs dining" is not a
        // real WANDS shape, but the domain model does not forbid it) --
        // neither is a strict word-count superset of the other.
        let types = names(&[("dining chairs", 1), ("chairs dining", 2)]);
        let groups = product_type_hyponym_groups(&types);
        assert!(groups.get(&ProductTypeId(1)).is_none_or(|v| v.is_empty()));
        assert!(groups.get(&ProductTypeId(2)).is_none_or(|v| v.is_empty()));
    }

    #[test]
    fn empty_vocabulary_produces_no_groups() {
        assert!(product_type_hyponym_groups(&BTreeMap::new()).is_empty());
    }

    #[test]
    fn leaf_segment_returns_the_whole_name_when_there_is_no_path_separator() {
        assert_eq!(leaf_segment("recliners"), "recliners");
        assert_eq!(leaf_segment("kids beds"), "kids beds");
    }

    #[test]
    fn leaf_segment_returns_only_the_trailing_path_component() {
        assert_eq!(
            leaf_segment("furniture / bedroom furniture / beds & headboards / beds"),
            "beds"
        );
        assert_eq!(
            leaf_segment("décor & pillows / candles & holders / scented oils & diffusers"),
            "scented oils & diffusers"
        );
    }

    /// A clean (non-path) broader term must still admit a path-derived
    /// narrower name when the broader term's words are a subset of the
    /// narrower name's own *leaf* segment -- the flagship real-WANDS
    /// case (`docs/decisions/ISSUE55_PRODUCT_TYPE_HYPONYM_DECISION.md`'s
    /// "recliners" example) that the leaf-only restriction must
    /// preserve, not merely avoid regressing.
    #[test]
    fn clean_broader_term_still_admits_a_path_names_matching_leaf() {
        let types = names(&[
            ("recliners", 1),
            (
                "furniture / living room furniture / chairs & seating / recliners / gray recliners",
                2,
            ),
        ]);
        let groups = product_type_hyponym_groups(&types);
        assert_eq!(
            groups.get(&ProductTypeId(1)),
            Some(&vec![ProductTypeId(2)]),
            "\"recliners\" must still admit the path name whose leaf is \"gray recliners\""
        );
    }

    /// The regression this checkpoint exists to fix
    /// (`docs/decisions/ISSUE55_PRODUCT_TYPE_HYPONYM_DECISION.md`'s
    /// "candles" -> "scented oils & diffusers" real-vocabulary false
    /// positive): a clean broader term whose word only appears in a
    /// path name's *ancestor* segment (not its leaf) must NOT be
    /// admitted, even though the full-path version of this function
    /// (checkpoint 11's rejected implementation) admitted it.
    #[test]
    fn clean_broader_term_does_not_match_a_path_names_ancestor_segment() {
        let types = names(&[
            ("candles", 1),
            (
                "décor & pillows / candles & holders / scented oils & diffusers",
                2,
            ),
        ]);
        let groups = product_type_hyponym_groups(&types);
        assert!(
            groups.get(&ProductTypeId(1)).is_none_or(|v| v.is_empty()),
            "\"candles\" must not admit a product whose leaf is \"scented oils & diffusers\" \
             just because \"candles\" appears in that product's ancestor breadcrumb"
        );
    }

    /// A second real-vocabulary regression case
    /// (`"bed accessories"`/`"bath accessories"` both spuriously
    /// admitting `"...shower curtain hooks"` under full-path
    /// comparison, because "bed"/"bath" and "accessories" each appeared
    /// in different, non-adjacent ancestor segments): neither must be
    /// admitted once only the leaf participates in the comparison.
    #[test]
    fn scattered_ancestor_words_no_longer_form_a_false_hyponym_pair() {
        let types = names(&[
            ("bed accessories", 1),
            ("bath accessories", 2),
            (
                "bed & bath / shower curtains & accessories / shower curtain hooks",
                3,
            ),
        ]);
        let groups = product_type_hyponym_groups(&types);
        assert!(groups.get(&ProductTypeId(1)).is_none_or(|v| v.is_empty()));
        assert!(groups.get(&ProductTypeId(2)).is_none_or(|v| v.is_empty()));
    }

    /// RED/property test: for 500 randomized synthetic vocabularies, every
    /// entry `product_type_hyponym_groups` produces must satisfy the
    /// safety property by direct re-derivation from the *names* themselves
    /// (not the function's own internal logic) -- a hyponym's word set is
    /// always a genuine, strict word-count superset of its parent's, and a
    /// product type is never its own hyponym. This is the RED baseline the
    /// implementation above is checked against, not merely argued to be
    /// correct.
    #[test]
    fn hyponym_groups_are_always_genuine_strict_whole_word_supersets_across_randomized_vocabularies(
    ) {
        use rand::seq::SliceRandom;
        use rand::{Rng, SeedableRng};
        use rand_chacha::ChaCha8Rng;

        let vocab = [
            "beds",
            "kids",
            "sofas",
            "chairs",
            "dining",
            "tables",
            "end",
            "lamps",
            "table",
            "turntable",
            "rugs",
            "outdoor",
        ];
        let mut rng = ChaCha8Rng::seed_from_u64(255);
        for trial in 0..500u32 {
            let type_count = rng.gen_range(1..=10);
            let mut names_by_id: BTreeMap<String, ProductTypeId> = BTreeMap::new();
            let mut next_id = 1u32;
            let mut seen_names = std::collections::BTreeSet::new();
            for _ in 0..type_count {
                let word_count = rng.gen_range(1..=3);
                let mut shuffled = vocab;
                shuffled.shuffle(&mut rng);
                let name = shuffled[..word_count].join(" ");
                if seen_names.insert(name.clone()) {
                    names_by_id.insert(name, ProductTypeId(next_id));
                    next_id += 1;
                }
            }

            let groups = product_type_hyponym_groups(&names_by_id);
            let word_set_by_id: BTreeMap<ProductTypeId, BTreeSet<&str>> = names_by_id
                .iter()
                .map(|(name, id)| (*id, name.split_whitespace().collect()))
                .collect();

            for (parent_id, hyponym_ids) in &groups {
                let parent_words = &word_set_by_id[parent_id];
                for hyponym_id in hyponym_ids {
                    assert_ne!(
                        hyponym_id, parent_id,
                        "trial {trial}: a product type must never be its own hyponym"
                    );
                    let hyponym_words = &word_set_by_id[hyponym_id];
                    assert!(
                        parent_words.len() < hyponym_words.len()
                            && parent_words.is_subset(hyponym_words),
                        "trial {trial}: {parent_id:?} ({parent_words:?}) -> {hyponym_id:?} \
                         ({hyponym_words:?}) is not a genuine strict whole-word superset"
                    );
                }
            }

            // Completeness in the other direction: every genuine strict
            // whole-word superset pair the vocabulary contains must show
            // up in `groups`, not just the ones that do appear being valid.
            for (a_id, a_words) in &word_set_by_id {
                for (b_id, b_words) in &word_set_by_id {
                    if a_id == b_id {
                        continue;
                    }
                    if a_words.len() < b_words.len() && a_words.is_subset(b_words) {
                        assert!(
                            groups.get(a_id).is_some_and(|v| v.contains(b_id)),
                            "trial {trial}: {a_id:?} ({a_words:?}) should admit {b_id:?} \
                             ({b_words:?}) as a hyponym but does not"
                        );
                    }
                }
            }
        }
    }

    /// Issue #55 checkpoint-14 follow-up
    /// (`docs/decisions/ISSUE55_PAIRED_COMPARATOR_DECISION.md`): the new
    /// `compile_lexicon_with_product_type_hyponyms` toggle must not change
    /// production behavior on its own -- `enable_product_type_hyponyms:
    /// true` has to be byte-identical (via `Debug` formatting, since
    /// `SemanticLexicon` has no `PartialEq`) to plain `compile_lexicon`,
    /// which always enables hyponym expansion. This is the regression
    /// guard for the refactor that threaded the new bool parameter through
    /// `compile_non_brand_lexicon`'s three existing call sites.
    #[test]
    fn product_type_hyponym_toggle_true_matches_compile_lexicon() {
        let types = names(&[("recliners", 1), ("gray recliners", 2), ("sofas", 3)]);
        let profile = CatalogProfile {
            product_type_names: types,
            ..Default::default()
        };
        let via_compile_lexicon = format!("{:?}", super::compile_lexicon(&profile, 1));
        let via_toggle_true = format!(
            "{:?}",
            super::compile_lexicon_with_product_type_hyponyms(&profile, 1, true)
        );
        assert_eq!(via_compile_lexicon, via_toggle_true);
    }

    /// The other half of the same guard: `enable_product_type_hyponyms:
    /// false` must produce plain per-id `ProductType` matching only --
    /// never a `ProductTypeAny`, even where the vocabulary contains a
    /// genuine hyponym pair the `true` path would expand.
    #[test]
    fn product_type_hyponym_toggle_false_never_produces_product_type_any() {
        let types = names(&[("recliners", 1), ("gray recliners", 2), ("sofas", 3)]);
        let profile = CatalogProfile {
            product_type_names: types,
            ..Default::default()
        };
        let baseline = super::compile_lexicon_with_product_type_hyponyms(&profile, 1, false);
        let debug = format!("{baseline:?}");
        assert!(
            !debug.contains("ProductTypeAny"),
            "baseline (hyponyms disabled) must never emit ProductTypeAny: {debug}"
        );
        assert!(
            debug.contains("ProductType(ProductTypeId(1))"),
            "baseline must still resolve \"recliners\" via plain ProductType matching: {debug}"
        );
        // Contrast: the same profile with hyponyms enabled DOES produce a
        // ProductTypeAny for "recliners" (its own established behavior,
        // `broader_term_admits_a_more_specific_whole_word_superset` above)
        // -- confirms the two lexicons are actually different, not that
        // this profile happens to never trigger the mechanism either way.
        let treatment = super::compile_lexicon_with_product_type_hyponyms(&profile, 1, true);
        assert!(format!("{treatment:?}").contains("ProductTypeAny"));
    }
}
