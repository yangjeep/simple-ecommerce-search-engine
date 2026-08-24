//! Issue #42's E2b serving-contract-closure pass, item 3 (repeated-run
//! stability re-run): dumps the EXACT bounded inputs each of the 4
//! perturbation configurations' original LLM passes were given --
//! deterministically reconstructed from the same real, committed
//! statistics `e2b_workload`/`e2b_key_mapping` already provide, never
//! re-derived by a second, independent computation. The original
//! session's own literal prompt WORDING was never committed (a real,
//! disclosed gap -- see `e2b_key_mapping`'s own doc comment for the
//! analogous key-mapping gap this pass already fixed); this binary
//! instead reproduces the one thing that WAS fully specified and
//! reproducible: the bounded input DATA itself (key/alias names, real
//! sample values, real parse/density/cardinality/uniqueness statistics),
//! per `docs/experiments/ISSUE42_PROTOCOL.md`'s E2b amendment 1 verbatim
//! ("Provide only: raw column names..., representative values,
//! parse/null/density/cardinality/uniqueness distributions..."). A new
//! stability-re-run prompt is then built from this data plus the
//! protocol's own descriptor schema and instructions text -- a faithful
//! reconstruction, disclosed as such, not a byte-identical replay.
//!
//! Output: one JSON file per configuration under the given output
//! directory, `bounded_inputs_<config>.json`, each a `Vec<BoundedField>`.

use std::collections::BTreeMap;
use std::fs;

use issue42_eval::e2b_key_mapping::{anonymized_mapping, noisy_mapping};
use issue42_eval::e2b_workload::{automotive_unified_stats, load_wands_feed, UnifiedFieldStats};

#[derive(serde::Serialize)]
struct BoundedField {
    shown_key: String,
    occurrences: usize,
    distinct_values: usize,
    uniqueness_ratio: f64,
    numeric_parseable_fraction: f64,
    mean_value_length: f64,
    variant_scoped: Option<bool>,
    sample_values: Vec<String>,
}

fn wands_unified() -> BTreeMap<String, UnifiedFieldStats> {
    let feed = load_wands_feed();
    feed.stats
        .iter()
        .map(|(k, s)| (k.clone(), UnifiedFieldStats::from(s)))
        .collect()
}

fn to_bounded(shown_key: String, stats: &UnifiedFieldStats) -> BoundedField {
    BoundedField {
        shown_key,
        occurrences: stats.occurrences,
        distinct_values: stats.distinct_values,
        uniqueness_ratio: stats.uniqueness_ratio,
        numeric_parseable_fraction: stats.numeric_parseable_fraction,
        mean_value_length: stats.mean_value_length,
        variant_scoped: stats.variant_scoped,
        sample_values: stats.sample_values.clone(),
    }
}

fn main() {
    let out_dir = std::env::args()
        .nth(1)
        .expect("usage: e2b_dump_bounded_inputs <output_dir>");
    fs::create_dir_all(&out_dir).expect("create output dir");

    let wands = wands_unified();
    let automotive = automotive_unified_stats(1500);
    let anon = anonymized_mapping();
    let noisy = noisy_mapping();

    // wands_baseline: real key names shown verbatim.
    let wands_baseline: Vec<BoundedField> = wands
        .iter()
        .map(|(k, s)| to_bounded(k.clone(), s))
        .collect();

    // wands_anonymized: feature_NNN shown; stats keyed by the real field
    // underneath, per the reconstructed anonymized_mapping.
    let wands_anonymized: Vec<BoundedField> = anon
        .iter()
        .map(|(shown, real)| to_bounded(shown.clone(), wands.get(real).expect("real key present")))
        .collect();

    // wands_noisy: noisy alias shown; same real-field lookup.
    let wands_noisy: Vec<BoundedField> = noisy
        .iter()
        .map(|(shown, real)| to_bounded(shown.clone(), wands.get(real).expect("real key present")))
        .collect();

    // automotive: real attribute names shown verbatim (never perturbed
    // per the protocol -- "used unperturbed as the synthetic control").
    let automotive_fields: Vec<BoundedField> = automotive
        .iter()
        .map(|(k, s)| to_bounded(k.clone(), s))
        .collect();

    for (name, fields) in [
        ("wands_baseline", &wands_baseline),
        ("wands_anonymized", &wands_anonymized),
        ("wands_noisy", &wands_noisy),
        ("automotive", &automotive_fields),
    ] {
        let path = format!("{out_dir}/bounded_inputs_{name}.json");
        fs::write(&path, serde_json::to_string_pretty(fields).unwrap())
            .unwrap_or_else(|e| panic!("write {path}: {e}"));
        println!("wrote {path} ({} fields)", fields.len());
    }
}
