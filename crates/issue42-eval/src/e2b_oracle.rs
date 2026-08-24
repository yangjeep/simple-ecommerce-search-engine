//! E2b's "blinded human/oracle mapping" baseline (4) and ground truth
//! (`docs/experiments/ISSUE42_PROTOCOL.md`'s E2b amendment 1): a
//! `Descriptor` hand-authored directly from each real key's name, real
//! sample values, and real per-key statistics (`e2b_workload`), *before*
//! any LLM proposal was generated and never shown to any proposing pass.
//! Every judgment call below is disclosed with its real evidence, not
//! merely asserted -- including several genuine, deliberately-preserved
//! ambiguities (a field statistically identifier-shaped but almost
//! certainly not what a shopper searches; a field whose real values mix
//! Boolean and free text; a "duration string" field that looks Numeric
//! but is not cleanly parseable as one) that this oracle resolves one
//! way but a reasonable second oracle author could resolve differently
//! -- the resolution matters less than making the reasoning inspectable.

use crate::e2b_schema::{
    Descriptor, Operator, PhysicalPrimitive, Scope, SemanticRole, Significance, ValueType,
};

/// A private, hand-authored-data convenience constructor for this
/// file's own 53 oracle entries (not a general public API another
/// caller could misuse with a hard-to-read positional call) -- the
/// large argument count reflects `Descriptor`'s own real field count,
/// not an avoidable design smell.
#[allow(clippy::too_many_arguments)]
fn d(
    key: &str,
    role: SemanticRole,
    value_type: ValueType,
    scope: Scope,
    ops: &[Operator],
    aliases: &[&str],
    relationship: Option<&str>,
    significance: Significance,
    primitive: PhysicalPrimitive,
    confidence: f64,
    evidence: &str,
) -> Descriptor {
    Descriptor {
        key: key.to_string(),
        real_key: None,
        semantic_role: role,
        value_type,
        scope,
        supported_operators: ops.to_vec(),
        aliases: aliases.iter().map(|s| s.to_string()).collect(),
        relationship_semantics: relationship.map(str::to_string),
        retrieval_significance: significance,
        candidate_physical_primitive: primitive,
        confidence,
        evidence: evidence.to_string(),
        abstain: false,
    }
}

use Operator::{Contains, Eq as OpEq, ExactLookup, Range};
use PhysicalPrimitive::{
    BitmapEnum, IdentifierDictionary as IdDict, LexicalPostings, None as NoPrimitive, NumericRange,
};
use Scope::{Product, Relationship as Rel};
use Significance::{Ignore as SigIgnore, RankingOnly, RetrievalSignificant};
use ValueType::{Boolean as VBool, Number, String as VString};

/// automotive's real, native 17-attribute set (R3's own `sku_code`/
/// `product_fingerprint` extensions are R3 fixture additions, not part of
/// automotive's own generator -- excluded here since they are not real
/// merchant-feed columns).
pub fn automotive_oracle() -> Vec<Descriptor> {
    vec![
        d("part_number", SemanticRole::Identifier, VString, Scope::Variant, &[ExactLookup], &[], None, RetrievalSignificant, IdDict, 1.0,
            "R3 (docs/experiments/ISSUE42_LOG.md#i42-r3): uniqueness_ratio=0.998, variant_scoped=true, the real production identifier field."),
        d("warranty_months", SemanticRole::Numeric, Number, Scope::Variant, &[Range, OpEq], &["year warranty", "warranty"], None, RetrievalSignificant, NumericRange, 0.85,
            "variant_scoped=true, distinct=4 (real duration buckets), a genuine real shopper filter term (\"5 year warranty\")."),
        d("oem_or_aftermarket", SemanticRole::Enum, VString, Product, &[OpEq], &["oem", "aftermarket"], None, RetrievalSignificant, BitmapEnum, 0.8,
            "distinct=2 (oem/aftermarket), a real, commonly-searched automotive-parts distinction."),
        d("amperage_output", SemanticRole::Numeric, Number, Product, &[Range], &["amp alternator"], None, RetrievalSignificant, NumericRange, 0.7,
            "distinct=72 numeric-shaped values, a real spec shoppers filter on for alternators."),
        d("bulb_type", SemanticRole::Enum, VString, Product, &[OpEq], &["bulb type"], None, RetrievalSignificant, BitmapEnum, 0.75,
            "distinct=4 low-cardinality categorical values."),
        d("cold_cranking_amps", SemanticRole::Numeric, Number, Product, &[Range], &["cold cranking amps", "cca"], None, RetrievalSignificant, NumericRange, 0.75,
            "distinct=131, a real, commonly-quoted battery spec shoppers filter/sort by."),
        d("filter_type", SemanticRole::Enum, VString, Product, &[OpEq], &["filter type"], None, RetrievalSignificant, BitmapEnum, 0.75,
            "distinct=2 low-cardinality categorical values."),
        d("group_size", SemanticRole::Enum, VString, Product, &[OpEq], &["battery group size"], None, RetrievalSignificant, BitmapEnum, 0.75,
            "distinct=3, a real battery-fitment categorical spec."),
        d("heat_range", SemanticRole::Enum, VString, Product, &[OpEq], &[], None, RankingOnly, BitmapEnum, 0.45,
            "distinct=5, a genuine but niche technical spark-plug spec -- real but rarely a direct shopper search term, hence RankingOnly not RetrievalSignificant (a lower-confidence call, disclosed)."),
        d("lumens", SemanticRole::Numeric, Number, Product, &[Range], &["lumens", "brightness"], None, RetrievalSignificant, NumericRange, 0.8,
            "distinct=141, R3's own documented near-miss field (uniqueness_ratio=0.94 -- genuinely high but NOT an identifier: a real, product-scoped brightness spec, not a per-item unique code)."),
        d("material", SemanticRole::Enum, VString, Product, &[OpEq], &["material"], None, RetrievalSignificant, BitmapEnum, 0.75,
            "distinct=2 low-cardinality categorical values."),
        d("material_grade", SemanticRole::Enum, VString, Product, &[OpEq], &[], None, RankingOnly, BitmapEnum, 0.4,
            "distinct=3, a grading spec rarely searched directly by a shopper (lower-confidence RankingOnly call, disclosed)."),
        d("micron_rating", SemanticRole::Numeric, Number, Product, &[Range], &["micron rating"], None, RetrievalSignificant, NumericRange, 0.7,
            "distinct=31, a real filter-spec spec for oil/air filters."),
        d("position", SemanticRole::Enum, VString, Product, &[OpEq], &["front", "rear"], None, RetrievalSignificant, BitmapEnum, 0.75,
            "distinct=2 (front/rear), a real, commonly-searched automotive-parts position distinction."),
        d("size", SemanticRole::Numeric, Number, Product, &[Range, OpEq], &["size"], None, RetrievalSignificant, NumericRange, 0.7,
            "distinct=13 numeric-shaped values."),
        d("thread_size", SemanticRole::Enum, VString, Product, &[OpEq], &["thread size"], None, RetrievalSignificant, BitmapEnum, 0.7,
            "distinct=3, a real fitment spec, categorical rather than continuous (thread sizes are named, not a smooth range)."),
        d("voltage", SemanticRole::Numeric, Number, Product, &[OpEq], &[], None, SigIgnore, NoPrimitive, 0.5,
            "distinct=1 (constant 12.0V across this whole catalog): technically Numeric-shaped, but a constant field discriminates nothing and should not be built into any physical index -- the honest oracle call is Ignore despite being cleanly typed, a real \"typed but useless for retrieval\" case."),
    ]
}

/// The 36 real WANDS `product_features` keys, per
/// `e2b_workload::WANDS_SAMPLE_KEYS` -- every judgment below cites the
/// real statistic(s) that justify it.
pub fn wands_oracle() -> Vec<Descriptor> {
    vec![
        // Numeric (continuous)
        d("overallproductweight", SemanticRole::Numeric, Number, Product, &[Range], &["weight", "lbs"], None, RetrievalSignificant, NumericRange, 0.75,
            "occurrences=57110, numeric_parseable_fraction>0.95: a real continuous dimension, a genuine shopper filter (\"under 50 lbs\")."),
        d("overallwidth-sidetoside", SemanticRole::Numeric, Number, Product, &[Range], &["width", "wide"], None, RetrievalSignificant, NumericRange, 0.75,
            "occurrences=46687, numeric-shaped, a real dimensional filter (\"narrow bookshelf\")."),
        d("overallheight-toptobottom", SemanticRole::Numeric, Number, Product, &[Range], &["height", "tall"], None, RetrievalSignificant, NumericRange, 0.75,
            "occurrences=37136, numeric-shaped, a real dimensional filter."),
        d("overalldepth-fronttoback", SemanticRole::Numeric, Number, Product, &[Range], &["depth"], None, RetrievalSignificant, NumericRange, 0.7,
            "occurrences=29674, numeric-shaped dimensional field."),
        d("weightcapacity", SemanticRole::Numeric, Number, Product, &[Range], &["weight capacity", "holds up to"], None, RetrievalSignificant, NumericRange, 0.75,
            "occurrences=15125, distinct=531, a real, commonly-searched capacity spec."),
        d("estimatedtimetosetup", SemanticRole::Numeric, Number, Product, &[Range], &["quick assembly", "setup time"], None, RankingOnly, NumericRange, 0.5,
            "occurrences=8602, numeric-shaped (minutes), but rarely a direct filter term -- more useful as a ranking signal (lower-confidence RankingOnly call, disclosed)."),
        // Boolean (yes/no)
        d("commercialwarranty", SemanticRole::Boolean, VBool, Product, &[OpEq], &[], None, RankingOnly, BitmapEnum, 0.4,
            "distinct=2 exact yes/no, but a compliance/B2B attribute rarely a direct consumer search term."),
        d("adultassemblyrequired", SemanticRole::Boolean, VBool, Product, &[OpEq], &[], None, RankingOnly, BitmapEnum, 0.4,
            "distinct=2 exact yes/no, low real shopper search frequency."),
        d("organic", SemanticRole::Boolean, VBool, Product, &[OpEq], &["organic"], None, RetrievalSignificant, BitmapEnum, 0.7,
            "distinct=2 (yes/no), a real, commonly-searched shopper term (\"organic mattress\")."),
        d("firerated", SemanticRole::Boolean, VBool, Product, &[OpEq], &["fire rated", "fire resistant"], None, RetrievalSignificant, BitmapEnum, 0.65,
            "distinct=2 (yes/no), a real safety-conscious shopper term."),
        d("drawersincluded", SemanticRole::Boolean, VBool, Product, &[OpEq], &["with drawers"], None, RetrievalSignificant, BitmapEnum, 0.7,
            "distinct=2 (yes/no), a real, commonly-searched furniture feature."),
        d("upholstered", SemanticRole::Boolean, VBool, Product, &[OpEq], &["upholstered"], None, RetrievalSignificant, BitmapEnum, 0.7,
            "distinct=2 (yes/no), a real, commonly-searched furniture feature."),
        d("installationrequired", SemanticRole::Boolean, VBool, Product, &[OpEq], &[], None, RankingOnly, BitmapEnum, 0.4,
            "distinct=3 (yes/no/blank -- real missing-data noise), rarely a direct search term."),
        // Enum, low/medium cardinality
        d("style", SemanticRole::Enum, VString, Product, &[OpEq], &["modern", "traditional", "rustic"], None, RetrievalSignificant, BitmapEnum, 0.85,
            "distinct=70, one of the most common real shopper query terms in this whole vertical (\"modern sofa\")."),
        d("dsprimaryproductstyle", SemanticRole::Enum, VString, Product, &[OpEq], &["modern", "traditional", "glam"], None, RetrievalSignificant, BitmapEnum, 0.6,
            "distinct=16, real style values -- but the 'ds' prefix and near-total semantic overlap with `style` (also present in this sample) is a real, disclosed same-role-different-name ambiguity: likely an internal normalized/derived duplicate of `style`, not an independently distinct concept, so a real physical build might legitimately merge the two rather than index both -- confidence lowered accordingly, not silently treated as fully independent."),
        d("countryoforigin", SemanticRole::Enum, VString, Product, &[OpEq], &["made in usa"], None, RetrievalSignificant, BitmapEnum, 0.5,
            "distinct=71, a real but lower-frequency shopper term (a minority of shoppers filter on country of manufacture)."),
        d("levelofassembly", SemanticRole::Enum, VString, Product, &[OpEq], &["no assembly required", "assembly required"], None, RetrievalSignificant, BitmapEnum, 0.75,
            "distinct=10, a real, commonly-searched convenience term."),
        d("dswoodtone", SemanticRole::Enum, VString, Product, &[OpEq], &["light wood", "dark wood"], None, RetrievalSignificant, BitmapEnum, 0.7,
            "distinct=9, a real, commonly-searched wood-furniture term."),
        d("primarymaterial", SemanticRole::Enum, VString, Product, &[OpEq], &["wood", "metal", "leather"], None, RetrievalSignificant, BitmapEnum, 0.8,
            "distinct=250, a real, commonly-searched material term, bounded cardinality despite the count."),
        d("framematerial", SemanticRole::Enum, VString, Product, &[OpEq], &["solid wood frame", "steel frame"], None, RetrievalSignificant, BitmapEnum, 0.65,
            "distinct=77, a real but more specialized material term than `primarymaterial`."),
        d("shape", SemanticRole::Enum, VString, Product, &[OpEq], &["round", "rectangular"], None, RetrievalSignificant, BitmapEnum, 0.75,
            "distinct=99, a real, commonly-searched shape term (\"round coffee table\")."),
        d("pattern", SemanticRole::Enum, VString, Product, &[OpEq], &["striped", "solid color", "geometric"], None, RetrievalSignificant, BitmapEnum, 0.65,
            "distinct=93, a real pattern/design term, primarily for textiles/rugs."),
        // Enum, higher but bounded cardinality
        d("color", SemanticRole::Enum, VString, Product, &[OpEq], &["blue", "red", "white"], None, RetrievalSignificant, BitmapEnum, 0.9,
            "distinct=4686 -- high, but real and bounded (a real, finite color vocabulary, not per-item unique text): uniqueness_ratio=0.178, far below any identifier threshold. The single most obvious real shopper query term in this sample."),
        d("basecolor", SemanticRole::Enum, VString, Product, &[OpEq], &["white base", "natural base"], None, RetrievalSignificant, BitmapEnum, 0.55,
            "distinct=1613 -- real but likely a secondary/derived color facet (base vs. accent color for multi-tone items), overlapping `color`'s own role -- a real, disclosed likely-redundant field, confidence lowered accordingly."),
        d("finish", SemanticRole::Enum, VString, Product, &[OpEq], &["brushed nickel", "matte black"], None, RetrievalSignificant, BitmapEnum, 0.75,
            "distinct=1294, a real, commonly-searched hardware/surface-finish term."),
        d("upholsterymaterial", SemanticRole::Enum, VString, Product, &[OpEq], &["leather", "velvet", "linen"], None, RetrievalSignificant, BitmapEnum, 0.75,
            "distinct=113, a real, commonly-searched upholstery-specific material term."),
        d("upholsterycolor", SemanticRole::Enum, VString, Product, &[OpEq], &["blue upholstery"], None, RetrievalSignificant, BitmapEnum, 0.6,
            "distinct=2176, a real color term scoped specifically to upholstery -- likely overlaps `color`'s own role for upholstered items, confidence lowered accordingly."),
        // Ambiguous / messy real fields
        d("productwarranty", SemanticRole::FreeText, VString, Product, &[Contains], &[], None, RankingOnly, LexicalPostings, 0.3,
            "a genuinely messy real field: sample values mix clean yes/no with actual warranty-description free text (\"1 year warranty for car...\") in the SAME key -- a real same-key type conflict, not an artificial perturbation. No single value_type cleanly fits every real value; FreeText/Contains is the conservative, honest call, with low confidence disclosed rather than forcing a false Boolean/Enum fit."),
        d("fullorlimitedwarranty", SemanticRole::Enum, VString, Product, &[OpEq], &["limited warranty", "full warranty"], None, RankingOnly, BitmapEnum, 0.45,
            "distinct=12 including a real empty-string value (missing data, not a 13th real category) -- a genuine real-world null/blank case a build must handle without treating '' as a legitimate value."),
        d("warrantylength", SemanticRole::Enum, VString, Product, &[OpEq], &["lifetime warranty", "5 year warranty"], None, RetrievalSignificant, BitmapEnum, 0.55,
            "distinct=112, real values are duration STRINGS (\"5 years\", \"1 year\", \"lifetime\") -- looks Numeric-adjacent but 'lifetime' has no numeric parse, so the honest call is Enum/categorical, not Numeric-with-unit-normalization (a real trap: naively parsing the numeric-looking majority and dropping 'lifetime' as an outlier would silently lose real inventory)."),
        d("title", SemanticRole::Ignore, VString, Product, &[], &[], None, SigIgnore, NoPrimitive, 0.3,
            "occurrences=546 of 42994 products (1.3% density) -- real but sparse enough, and its role (a second, unexplained 'title' inside the feature blob, distinct from the catalog's own top-level product_name column) unclear enough, that treating it as retrieval-significant would be a guess, not evidence; Ignore with low confidence is the honest call, disclosed as a real, unresolved ambiguity rather than a confident classification either way."),
        // Free-text / lexical-only
        d("productcare", SemanticRole::FreeText, VString, Product, &[Contains], &[], None, SigIgnore, LexicalPostings, 0.6,
            "distinct=3500 out of 15802 occurrences -- real free-text care instructions, no clean typed role; a real product should not build a hard structural filter on this field, at most an optional lexical-postings entry, never a bitmap/range primitive."),
        d("piecesincluded", SemanticRole::FreeText, VString, Product, &[Contains], &[], None, SigIgnore, LexicalPostings, 0.6,
            "distinct=2706 out of 6881 occurrences -- real free-text kit-contents descriptions (\"2 statues\", \"1 log rack with 4 tools...\"), no clean typed role."),
        // Identifier-shaped (real trap)
        d("samplepartnumber", SemanticRole::Identifier, VString, Product, &[ExactLookup], &[], None, SigIgnore, NoPrimitive, 0.55,
            "uniqueness_ratio=0.9785 at count=2048 -- statistically identifier-shaped, matching every numeric signal `commerce_core::index::identifier::IdentifierClassifier` would key on. But the real key name ('SAMPLE part number') and WANDS's own domain (furniture/textile swatches) strongly suggest this identifies a fabric/material SAMPLE a shopper might order to test, not the product's own primary identity -- almost certainly not what a shopper searches for directly. This is the deliberate, real trap this sample exists to test: a classifier gating ONLY on statistical shape (uniqueness+scope) cannot see this distinction; the oracle's own low retrieval_significance here, despite an Identifier role and high confidence in the STATISTICAL classification, is exactly the signal a statistics-only baseline is expected to miss."),
        // Relationship fields
        d("compatibledrainassemblypartnumber", SemanticRole::Relationship, VString, Rel, &[ExactLookup], &[], Some("references a compatible replacement part's own identifier on a DIFFERENT product listing -- not this product's own identity"), SigIgnore, IdDict, 0.5,
            "distinct=78 of count=161 -- a real cross-reference field (a plumbing part's compatible drain-assembly part number), structurally a Relationship, not this product's own Identifier, and not a realistic direct shopper search term."),
        d("compatiblediningchairpartnumber", SemanticRole::Relationship, VString, Rel, &[ExactLookup], &[], Some("references a compatible dining chair's own part number on a DIFFERENT product listing"), SigIgnore, IdDict, 0.5,
            "distinct=71 of count=71 (every value distinct) -- a real cross-reference field, structurally a Relationship."),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automotive_oracle_covers_every_real_native_attribute() {
        let oracle = automotive_oracle();
        assert_eq!(
            oracle.len(),
            17,
            "automotive's real, native attribute count"
        );
        let stats = crate::e2b_workload::automotive_field_stats(1500);
        for desc in &oracle {
            assert!(
                stats.contains_key(&desc.key),
                "oracle key {:?} must be a real automotive attribute",
                desc.key
            );
        }
        assert_eq!(
            stats.len(),
            oracle.len(),
            "the oracle must cover every real automotive attribute, not a subset"
        );
    }

    #[test]
    fn wands_oracle_covers_every_sample_key_exactly_once() {
        let oracle = wands_oracle();
        assert_eq!(oracle.len(), crate::e2b_workload::WANDS_SAMPLE_KEYS.len());
        let oracle_keys: std::collections::BTreeSet<&str> =
            oracle.iter().map(|d| d.key.as_str()).collect();
        for key in crate::e2b_workload::WANDS_SAMPLE_KEYS {
            assert!(
                oracle_keys.contains(key),
                "oracle must cover sample key {key:?}"
            );
        }
    }

    #[test]
    fn samplepartnumber_is_identified_but_marked_low_retrieval_significance() {
        let oracle = wands_oracle();
        let d = oracle.iter().find(|d| d.key == "samplepartnumber").unwrap();
        assert_eq!(d.semantic_role, SemanticRole::Identifier);
        assert_eq!(d.retrieval_significance, Significance::Ignore);
    }

    #[test]
    fn relationship_descriptors_always_carry_relationship_semantics() {
        for desc in wands_oracle() {
            if desc.scope == Scope::Relationship {
                assert!(
                    desc.relationship_semantics.is_some(),
                    "{:?} has scope=Relationship but no relationship_semantics",
                    desc.key
                );
            } else {
                assert!(
                    desc.relationship_semantics.is_none(),
                    "{:?} is not scope=Relationship but has relationship_semantics set",
                    desc.key
                );
            }
        }
    }
}
