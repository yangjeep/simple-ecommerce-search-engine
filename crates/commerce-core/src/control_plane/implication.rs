//! Issue #16 (Phase 4): learned semantic implication rules. A trigger
//! phrase (e.g. "air force 1") implies one or more typed commerce facts
//! (e.g. `Brand=NIKE`) that are not literal substrings of the trigger
//! itself -- this is a genuinely different mechanism from
//! `control_plane::provider::ModelProvider`/`ir::lexicon::Candidate`
//! ("propose a single resolved reading for an *unresolved* term") and
//! from `cold_start::prefill` ("infer a brand live, inline, per query,
//! from a title-phrase index"). An [`ImplicationRule`] is proposed
//! offline, replay-validated (see `docs/experiments/PHASE4_LOG.md`
//! P4-E01), and only a `Promoted` rule ever enters a compiled
//! [`ImplicationTable`] -- the online path (`apply_implications`) is a
//! deterministic map lookup with no model call and no live index access,
//! matching Issue #16's own required online/offline separation.
//!
//! **Scope, stated explicitly (`docs/experiments/PHASE4_LOG.md`)**: the
//! real ESCI catalog's `product_type`/`category` fields are always
//! sentinel, so this phase's rules only ever imply a single
//! `StructuralConstraint::Brand` fact each, even though [`ImplicationRule`]
//! itself supports multiple simultaneous implied facts (a catalog with
//! real structured multi-entity data could exercise that without a
//! redesign). The conflict-detection logic in [`apply_implications`] is
//! written generically over [`ResolvedConstraint`], but has only been
//! exercised against Brand-kind facts -- widening the fact kinds actually
//! promoted is future work, not assumed safe by this module alone.

use std::collections::HashMap;

use crate::ir::{CommerceQuery, ResolvedConstraint, StructuralConstraint};

/// Where a rule's supporting evidence came from. Mirrors the shape Issue
/// #16 itself asks for (`catalog | query-log | behavioral | model |
/// manual`); this phase only produces `Catalog`-provenance rules (real
/// title-phrase-to-brand co-occurrence, `cold_start::prefill`'s existing
/// signal) -- the other variants exist so a future proposer (e.g. a
/// `ModelProvider`-style offline model pass, per Issue #9's own
/// committed-static-file pattern) has somewhere to record itself without
/// a type change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleProvenance {
    Catalog,
    QueryLog,
    Behavioral,
    Model,
    Manual,
}

/// A rule's lifecycle state. Only `Promoted` rules are ever compiled into
/// an [`ImplicationTable`] -- see that type's own doc comment for why this
/// is enforced structurally (at compile-time construction) rather than by
/// a runtime status check on every lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleStatus {
    Candidate,
    Promoted,
    Withdrawn,
}

/// One learned semantic implication: `trigger` (a lowercased phrase)
/// implies every fact in `implies` simultaneously, whenever it is
/// recognized in a query -- unlike an [`crate::ir::lexicon::Candidate`],
/// which represents one *alternative reading* among several competing
/// ones for the same phrase, `implies` is always applied in full as a
/// conjunction, never treated as a menu to disambiguate.
#[derive(Debug, Clone, PartialEq)]
pub struct ImplicationRule {
    pub trigger: String,
    pub implies: Vec<ResolvedConstraint>,
    pub provenance: RuleProvenance,
    pub confidence: f64,
    pub status: RuleStatus,
}

impl ImplicationRule {
    /// A new, unvalidated candidate rule (`status: Candidate`). Use
    /// [`Self::promote`]/[`Self::withdraw`] to change status once replay
    /// validation (P4-E01) has run.
    pub fn candidate(
        trigger: &str,
        implies: Vec<ResolvedConstraint>,
        provenance: RuleProvenance,
        confidence: f64,
    ) -> Self {
        ImplicationRule {
            trigger: trigger.to_lowercase(),
            implies,
            provenance,
            confidence,
            status: RuleStatus::Candidate,
        }
    }

    pub fn promote(mut self) -> Self {
        self.status = RuleStatus::Promoted;
        self
    }

    pub fn withdraw(mut self) -> Self {
        self.status = RuleStatus::Withdrawn;
        self
    }
}

/// A compiled, versioned lookup table of **promoted rules only**.
/// [`Self::compile`] silently drops any `Candidate`/`Withdrawn` rule
/// passed to it -- the online serving path must be structurally incapable
/// of applying an unvalidated or retracted rule, mirroring
/// `control_plane::provider::ModelProvider`'s own doc comment: "enforced
/// by *where* this is called from, not by anything in the type itself."
/// A rule withdrawn after having been promoted is removed from the next
/// compiled table version, not flagged inside it -- there is deliberately
/// no way for a caller of [`Self::lookup`] to observe a withdrawn rule at
/// all.
///
/// **Same-trigger conflict handling, Issue #16's own "ambiguous product-
/// family name"/"merchant-specific naming conflict" adversarial case**:
/// if two or more *distinct* promoted rules share the same trigger and
/// disagree on `implies`, naively collecting them into a `HashMap` would
/// silently keep whichever one happened to iterate last -- an arbitrary,
/// iteration-order-dependent pick with no safety meaning at all (exactly
/// the hazard this project's own "actively try to kill every favorable
/// result" discipline exists to catch before it becomes a real serving
/// bug, not a hypothetical one: this was found by inspecting `compile`'s
/// own implementation, not by a bug report). `compile` excludes a
/// trigger entirely (abstains) whenever its promoted rules disagree,
/// mirroring `apply_implications`'s own cross-trigger disagreement
/// abstention -- but applied here at the compile boundary, one layer
/// earlier, so a caller merging rule sets from multiple offline sources
/// (multiple proposers, multiple verticals/merchants) never has to
/// implement this check themselves. Multiple promoted rules that happen
/// to agree exactly on `implies` collapse safely to one entry -- no
/// information is lost, since they represent the same fact.
#[derive(Debug, Clone, Default)]
pub struct ImplicationTable {
    pub version: u32,
    rules: HashMap<String, ImplicationRule>,
}

impl ImplicationTable {
    pub fn compile(version: u32, rules: impl IntoIterator<Item = ImplicationRule>) -> Self {
        let mut by_trigger: HashMap<String, Vec<ImplicationRule>> = HashMap::new();
        for rule in rules
            .into_iter()
            .filter(|r| r.status == RuleStatus::Promoted)
        {
            by_trigger
                .entry(rule.trigger.clone())
                .or_default()
                .push(rule);
        }
        let rules = by_trigger
            .into_iter()
            .filter_map(|(trigger, candidates)| {
                let first_implies = &candidates[0].implies;
                if candidates.iter().all(|r| &r.implies == first_implies) {
                    Some((trigger, candidates.into_iter().next().unwrap()))
                } else {
                    None // conflicting promoted rules for the same trigger -- abstain
                }
            })
            .collect();
        ImplicationTable { version, rules }
    }

    pub fn lookup(&self, trigger: &str) -> Option<&ImplicationRule> {
        self.rules.get(&trigger.to_lowercase())
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

fn implied_brand(constraint: &ResolvedConstraint) -> Option<crate::domain::BrandId> {
    match constraint {
        ResolvedConstraint::Structural(StructuralConstraint::Brand(id)) => Some(*id),
        _ => None,
    }
}

fn query_already_has_explicit_brand(query: &CommerceQuery) -> bool {
    query.constraints.iter().any(|c| {
        matches!(
            c,
            ResolvedConstraint::Structural(
                StructuralConstraint::Brand(_) | StructuralConstraint::BrandAny(_)
            )
        )
    })
}

/// Scan `raw_text` for word windows (2 up to `max_window_words` words)
/// matching a promoted rule's trigger in `table`, and add every implied
/// fact to `query` -- additively, never replacing or removing anything
/// (ADR 0010's own discipline, reused unchanged from
/// `cold_start::prefill::apply_predictive_prefill`). Returns the triggers
/// actually applied (empty if none matched, or if abstention fired).
///
/// Two non-negotiable safety rules, both required by Issue #16's own
/// adversarial-safety list:
///
/// 1. **Never override explicit signal.** If `query` already carries an
///    explicit `Brand`/`BrandAny` constraint, no Brand-implying rule is
///    applied at all, regardless of confidence -- identical to
///    `apply_predictive_prefill`'s own rule 1.
/// 2. **Abstain on internal disagreement, never guess.** If two or more
///    matched triggers in the same query imply *different* Brand values
///    (Issue #16's required "one query span implying mutually
///    incompatible facts" case, generalized here to "multiple spans in
///    the same query disagreeing" since a single trigger's own `implies`
///    is always internally consistent by construction), nothing is
///    applied for this query at all: abstention plus the existing Solr
///    fallback is strictly safer than picking either conflicting value.
pub fn apply_implications(
    query: &mut CommerceQuery,
    raw_text: &str,
    table: &ImplicationTable,
    max_window_words: usize,
) -> Vec<String> {
    if table.is_empty() {
        return Vec::new();
    }

    let tokens: Vec<String> = raw_text.split_whitespace().map(str::to_lowercase).collect();
    let mut matched_triggers: Vec<String> = Vec::new();
    let mut implied_brands: Vec<crate::domain::BrandId> = Vec::new();
    let mut other_implied: Vec<ResolvedConstraint> = Vec::new();

    for window_len in 2..=max_window_words.max(2) {
        if tokens.len() < window_len {
            continue;
        }
        for window in tokens.windows(window_len) {
            let phrase = window.join(" ");
            let Some(rule) = table.lookup(&phrase) else {
                continue;
            };
            matched_triggers.push(rule.trigger.clone());
            for fact in &rule.implies {
                match implied_brand(fact) {
                    Some(brand_id) => implied_brands.push(brand_id),
                    None => other_implied.push(fact.clone()),
                }
            }
        }
    }

    if matched_triggers.is_empty() {
        return Vec::new();
    }

    implied_brands.dedup();
    let brands_agree = {
        let mut sorted = implied_brands.clone();
        sorted.sort_by_key(|b| b.0);
        sorted.dedup();
        sorted.len() <= 1
    };
    if !brands_agree {
        // Abstain entirely -- do not apply any fact from this query's
        // matches, even the ones that didn't conflict, since a caller
        // seeing a partial application couldn't distinguish "abstained
        // deliberately" from "only found one matching rule."
        return Vec::new();
    }

    let mut applied = false;
    if let Some(&brand_id) = implied_brands.first() {
        if !query_already_has_explicit_brand(query) {
            query
                .constraints
                .push(ResolvedConstraint::Structural(StructuralConstraint::Brand(
                    brand_id,
                )));
            applied = true;
        }
    }
    for fact in other_implied {
        query.constraints.push(fact);
        applied = true;
    }

    if applied {
        matched_triggers.sort();
        matched_triggers.dedup();
        matched_triggers
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::BrandId;

    fn nike() -> BrandId {
        BrandId(1)
    }
    fn adidas() -> BrandId {
        BrandId(2)
    }

    fn brand_rule(trigger: &str, brand: BrandId) -> ImplicationRule {
        ImplicationRule::candidate(
            trigger,
            vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                brand,
            ))],
            RuleProvenance::Catalog,
            0.95,
        )
    }

    #[test]
    fn a_promoted_rule_implies_its_fact_when_its_trigger_is_recognized() {
        let table = ImplicationTable::compile(1, [brand_rule("air force 1", nike()).promote()]);
        let mut query = CommerceQuery {
            residual_lexical: vec!["air".into(), "force".into(), "1".into(), "white".into()],
            ..Default::default()
        };

        let applied = apply_implications(&mut query, "air force 1 white", &table, 3);

        assert_eq!(applied, vec!["air force 1"]);
        assert_eq!(
            query.constraints,
            vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                nike()
            ))]
        );
    }

    #[test]
    fn a_candidate_rule_never_promoted_is_never_applied() {
        // RED-first: compiling a table from an unpromoted rule must
        // silently drop it, not apply it.
        let table = ImplicationTable::compile(1, [brand_rule("air force 1", nike())]);
        assert!(table.is_empty());

        let mut query = CommerceQuery::default();
        let applied = apply_implications(&mut query, "air force 1 white", &table, 3);

        assert!(applied.is_empty());
        assert!(query.constraints.is_empty());
    }

    #[test]
    fn a_withdrawn_rule_is_never_applied_even_after_having_been_promoted() {
        let rule = brand_rule("air force 1", nike()).promote().withdraw();
        let table = ImplicationTable::compile(1, [rule]);
        assert!(table.is_empty());

        let mut query = CommerceQuery::default();
        let applied = apply_implications(&mut query, "air force 1 white", &table, 3);

        assert!(applied.is_empty());
        assert!(query.constraints.is_empty());
    }

    #[test]
    fn an_explicit_brand_constraint_already_present_suppresses_the_implication_entirely() {
        let table = ImplicationTable::compile(1, [brand_rule("air force 1", nike()).promote()]);
        let mut query = CommerceQuery {
            constraints: vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                adidas(),
            ))],
            residual_lexical: vec!["air".into(), "force".into(), "1".into()],
            ..Default::default()
        };

        let applied = apply_implications(&mut query, "air force 1", &table, 3);

        assert!(applied.is_empty());
        assert_eq!(
            query.constraints,
            vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                adidas()
            ))]
        );
    }

    #[test]
    fn two_matched_triggers_implying_different_brands_abstain_entirely() {
        // Issue #16's required adversarial case: one query span (here,
        // two overlapping/adjacent triggers within the same query)
        // implying mutually incompatible facts must abstain, not guess.
        let table = ImplicationTable::compile(
            1,
            [
                brand_rule("air force 1", nike()).promote(),
                brand_rule("force 1 retro", adidas()).promote(),
            ],
        );
        let mut query = CommerceQuery::default();

        let applied = apply_implications(&mut query, "air force 1 retro", &table, 3);

        assert!(
            applied.is_empty(),
            "expected abstention on brand disagreement, got {applied:?}"
        );
        assert!(query.constraints.is_empty());
    }

    #[test]
    fn two_matched_triggers_agreeing_on_the_same_brand_apply_normally() {
        let table = ImplicationTable::compile(
            1,
            [
                brand_rule("air force 1", nike()).promote(),
                brand_rule("force 1 07", nike()).promote(),
            ],
        );
        let mut query = CommerceQuery::default();

        let applied = apply_implications(&mut query, "air force 1 07", &table, 3);

        assert_eq!(applied, vec!["air force 1", "force 1 07"]);
        assert_eq!(
            query.constraints,
            vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                nike()
            ))]
        );
    }

    #[test]
    fn no_matching_trigger_applies_nothing() {
        let table = ImplicationTable::compile(1, [brand_rule("air force 1", nike()).promote()]);
        let mut query = CommerceQuery::default();

        let applied = apply_implications(&mut query, "completely unrelated text", &table, 3);

        assert!(applied.is_empty());
        assert!(query.constraints.is_empty());
    }

    #[test]
    fn an_empty_table_short_circuits_without_scanning() {
        let table = ImplicationTable::compile(1, []);
        let mut query = CommerceQuery::default();

        let applied = apply_implications(&mut query, "air force 1", &table, 3);

        assert!(applied.is_empty());
        assert!(query.constraints.is_empty());
    }

    #[test]
    fn conflicting_promoted_rules_for_the_same_trigger_abstain_at_compile_time() {
        // Issue #16's "ambiguous product-family name" / "merchant-specific
        // naming conflict" adversarial case: two distinct promoted rules
        // (e.g. proposed from two different offline sources/verticals)
        // both claim the same trigger phrase but disagree on the implied
        // brand. Compiling them together must not silently pick either
        // one -- the trigger must not resolve to anything at all.
        let table = ImplicationTable::compile(
            1,
            [
                brand_rule("max", nike()).promote(),
                brand_rule("max", adidas()).promote(),
            ],
        );

        assert!(
            table.lookup("max").is_none(),
            "a trigger with conflicting promoted rules must not be applied"
        );

        let mut query = CommerceQuery::default();
        let applied = apply_implications(&mut query, "max", &table, 3);
        assert!(applied.is_empty());
        assert!(query.constraints.is_empty());
    }

    #[test]
    fn agreeing_promoted_rules_for_the_same_trigger_collapse_safely() {
        // Two promoted rules for the same trigger that agree on the
        // implied fact (e.g. proposed independently by two sources that
        // both arrived at the same real conclusion) must still apply --
        // agreement is not the same hazard as conflict.
        let table = ImplicationTable::compile(
            1,
            [
                brand_rule("air force 1", nike()).promote(),
                brand_rule("air force 1", nike()).promote(),
            ],
        );

        assert!(table.lookup("air force 1").is_some());
        let mut query = CommerceQuery::default();
        let applied = apply_implications(&mut query, "air force 1", &table, 3);
        assert_eq!(applied, vec!["air force 1"]);
        assert_eq!(
            query.constraints,
            vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                nike()
            ))]
        );
    }
}
