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
    compile_non_brand_lexicon(profile, min_enum_frequency, &mut lex);
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
    compile_non_brand_lexicon(profile, min_enum_frequency, &mut lex);
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

    compile_non_brand_lexicon(profile, min_enum_frequency, &mut lex);
    lex
}

fn compile_non_brand_lexicon(
    profile: &CatalogProfile,
    min_enum_frequency: usize,
    lex: &mut SemanticLexicon,
) {
    for (name, id) in &profile.product_type_names {
        lex.insert(
            name,
            vec![Candidate::constraint(
                ResolvedConstraint::Structural(StructuralConstraint::ProductType(*id)),
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
