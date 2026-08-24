//! A diagnostic, not part of the preregistered GO-gate computation
//! (`docs/experiments/ISSUE45_PROTOCOL.md` never mentions it), added
//! after this checkpoint's own fresh adversarial review raised a real
//! question: does Treatment C's headline "100% full-descriptor
//! stability" reflect genuine evidence-based conflict resolution (R2's
//! plurality vote combined with R3/R4/R5/R7's real, non-vote-derivable
//! rules), or does most of the measurable gap over naive majority
//! voting (Treatment B) come from R1 (primitive is a deterministic
//! function of role) and R6 (scope defaults to Product) alone -- both
//! legitimate, principled, disclosed design choices, but structurally
//! different from "the canonicalizer resolved a real disagreement with
//! evidence": R1 collapses two comparison axes (primitive, and via
//! `value_type_for_role`, type) into role by construction; R6 is a
//! constant whenever `has_real_variant_grouping=false` (always true for
//! WANDS/automotive as ingested here), so it is tautologically 100%
//! stable rather than an empirically resolved conflict.
//!
//! Answers two questions directly from the real, frozen data:
//!
//! 1. Of the leave-one-out-stable canonicalizations for every real-key
//!    that was raw-unstable (role or primitive disagreed across raw
//!    runs), how many stabilized via genuine `Promoted`-`Promoted`
//!    agreement vs `Abstain`-`Abstain` agreement (the metric's own
//!    gameable convention, `e2c_metrics::pairwise_stability`)?
//! 2. For every (config, real key) where `e2b_validator::cross_run_type_conflict`
//!    fires on at least one pair of raw proposals (the only condition
//!    under which R3 can possibly engage, post-fix), does R3's resolved
//!    role ever differ from what plain R2 plurality alone (no R3-R8)
//!    would have produced?
//!
//! Reproduction: `cargo run --release -p issue42-eval --bin
//! e2c_r1_r6_attribution_diagnostic`.

use std::collections::BTreeMap;

use issue42_eval::e2b_pipeline::{self, CONFIGS};
use issue42_eval::e2b_schema::SemanticRole;
use issue42_eval::e2b_workload::{automotive_unified_stats, load_wands_feed, UnifiedFieldStats};
use issue42_eval::e2c_canonicalizer::canonicalize;
use issue42_eval::e2c_metrics::{group_by_real_key, leave_one_out_outcomes, Treatment};
use issue42_eval::e2c_schema::{CandidateDescriptor, CanonicalOutcome};

/// A local reimplementation of `e2c_canonicalizer`'s own private R2
/// plurality tie-break, for this review-only comparison -- never
/// consulted by the reviewed code path itself.
const ROLE_PRECEDENCE: [SemanticRole; 7] = [
    SemanticRole::Ignore,
    SemanticRole::FreeText,
    SemanticRole::Enum,
    SemanticRole::Numeric,
    SemanticRole::Boolean,
    SemanticRole::Identifier,
    SemanticRole::Relationship,
];

fn r2_only_plurality(non_abstain: &[&CandidateDescriptor]) -> SemanticRole {
    let mut best = ROLE_PRECEDENCE[0];
    let mut best_count = 0usize;
    for &candidate in &ROLE_PRECEDENCE {
        let count = non_abstain
            .iter()
            .filter(|d| d.semantic_role == candidate)
            .count();
        if count > best_count {
            best_count = count;
            best = candidate;
        }
    }
    best
}

fn main() {
    let per_config_runs = e2b_pipeline::load_all_runs(CONFIGS);
    let anon_mapping = issue42_eval::e2b_key_mapping::anonymized_mapping();
    let noisy_mapping = issue42_eval::e2b_key_mapping::noisy_mapping();

    let wands_feed = load_wands_feed();
    let wands_unified: BTreeMap<String, UnifiedFieldStats> = wands_feed
        .stats
        .iter()
        .map(|(k, s)| (k.clone(), UnifiedFieldStats::from(s)))
        .collect();
    let mut all_unified: BTreeMap<String, UnifiedFieldStats> = wands_unified.clone();
    all_unified.extend(automotive_unified_stats(1500));

    let wands_queries_text = issue42_eval::e2b_validator::wands_query_texts();

    let mut stabilized_via_promoted = 0usize;
    let mut stabilized_via_abstain = 0usize;
    let mut not_stabilized = 0usize;
    let mut r3_ever_flips_r2 = 0usize;
    let mut r3_fires_at_all = 0usize;
    let mut total_groups_with_conflict = 0usize;

    for (config, runs) in &per_config_runs {
        let by_key = group_by_real_key(config, runs, &anon_mapping, &noisy_mapping);
        for (real_key, runs_for_key) in &by_key {
            if runs_for_key.len() < 2 {
                continue;
            }
            let first_role = runs_for_key[0].1.semantic_role;
            let first_prim = runs_for_key[0].1.candidate_physical_primitive;
            let was_unstable = runs_for_key.iter().any(|(_, d)| {
                d.semantic_role != first_role || d.candidate_physical_primitive != first_prim
            });
            if !was_unstable {
                continue;
            }
            let Some(stats) = all_unified.get(real_key) else {
                continue;
            };

            // Q1: leave-one-out stabilization mechanism.
            let outcomes = leave_one_out_outcomes(
                Treatment::C,
                runs_for_key,
                real_key,
                Some(stats),
                &wands_queries_text,
                false,
            );
            let all_promoted = outcomes.iter().all(|o| o.is_promoted());
            let all_abstain = outcomes.iter().all(|o| !o.is_promoted());
            let all_same_promoted = all_promoted
                && outcomes.windows(2).all(|w| {
                    let (CanonicalOutcome::Promoted(a), CanonicalOutcome::Promoted(b)) =
                        (&w[0], &w[1])
                    else {
                        return false;
                    };
                    a.semantic_role == b.semantic_role
                        && a.value_type == b.value_type
                        && a.scope == b.scope
                        && a.canonical_physical_primitive == b.canonical_physical_primitive
                });
            if all_same_promoted {
                stabilized_via_promoted += 1;
            } else if all_abstain {
                stabilized_via_abstain += 1;
                println!("ABSTAIN-STABILIZED: {config}/{real_key}");
            } else {
                not_stabilized += 1;
                println!("NOT UNIFORMLY STABLE: {config}/{real_key}");
            }

            // Q2: does R3's evidence override ever change the role vs
            // plain R2 plurality, on the full (non-leave-one-out) run?
            let non_abstain: Vec<&CandidateDescriptor> = runs_for_key
                .iter()
                .filter(|(_, d)| !d.abstain)
                .map(|(_, d)| d)
                .collect();
            if non_abstain.is_empty() {
                continue;
            }
            let r2_only_role = r2_only_plurality(&non_abstain);
            let full_outcome = canonicalize(
                runs_for_key,
                real_key,
                stats,
                &wands_queries_text,
                false,
                false,
            );
            let any_conflict = (0..runs_for_key.len()).any(|i| {
                ((i + 1)..runs_for_key.len()).any(|j| {
                    issue42_eval::e2b_validator::cross_run_type_conflict(
                        &runs_for_key[i].1,
                        &runs_for_key[j].1,
                    )
                })
            });
            if any_conflict {
                total_groups_with_conflict += 1;
                r3_fires_at_all += 1;
                if let CanonicalOutcome::Promoted(d) = &full_outcome {
                    if d.semantic_role != r2_only_role {
                        r3_ever_flips_r2 += 1;
                        println!(
                            "R3 FLIPS R2: {config}/{real_key} r2_only={r2_only_role:?} full={:?}",
                            d.semantic_role
                        );
                    }
                }
            }
        }
    }

    println!(
        "\n=== Q1: leave-one-out stabilization mechanism (raw-unstable keys, all 4 configs) ==="
    );
    println!("stabilized via Promoted-Promoted: {stabilized_via_promoted}");
    println!("stabilized via Abstain-Abstain:   {stabilized_via_abstain}");
    println!("NOT uniformly stable:              {not_stabilized}");

    println!(
        "\n=== Q2: does R3's evidence override ever change the role vs plain R2 plurality? ==="
    );
    println!("(config,key) groups where cross_run_type_conflict fires at all: {total_groups_with_conflict}");
    println!("of those, R3 fires (any_conflict branch entered): {r3_fires_at_all}");
    println!("of those, final role DIFFERS from plain-R2-plurality: {r3_ever_flips_r2}");
}
