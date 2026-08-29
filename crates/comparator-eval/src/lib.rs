//! Issue #55 A3 (PR #56 closure directive): centralized, hardened
//! comparator infrastructure, extracted and generalized from the two
//! independent partial implementations the repository had already
//! converged on -- `issue35_eval::eval`'s `SolrLookup`/`solr_search`
//! (transport-hardened, but crate-private and narrow fq coverage) and
//! `round1_eval::solr` (widely reused, but collapses every failure mode
//! into one `Option::None` indistinguishable from a real empty result).
//!
//! An audit of every Solr-calling eval binary in this workspace
//! (`docs/decisions/ISSUE55_COMPARATOR_CENTRALIZATION_DECISION.md`) found
//! the exact defect class this crate exists to close, live, in
//! decision-relevant code: `phase9-eval/p9_e02_wands_physical_advantage.rs`
//! silently turned a Solr transport/parse failure into a scored
//! `NDCG=0.0` folded into its headline traffic-weighted verdict; a stale
//! copy of its own fq-translation function in
//! `p9_e07_ambiguous_routing_diagnostic.rs` was missing the
//! `ProductTypeAny` arm entirely, asymmetrically widening Solr's
//! candidate pool relative to native's; and three more binaries dropped
//! failed queries from their sample with no counter or trace at all.
//!
//! Three modules implement the contract:
//!
//! - [`outcome`] -- a 4-way lookup outcome (`Success` / `TransportError`
//!   / `QueryError` / `ParseError`) so a legitimate empty result can
//!   never be confused with any failure to answer, and a transport
//!   failure can never be confused with the search engine itself
//!   rejecting the query.
//! - [`solr`] -- the hardened Solr transport, implementing the
//!   [`EngineComparator`](solr::EngineComparator) trait so an
//!   Elasticsearch or Havenask adapter (Issue #57) can implement the same
//!   trait and reuse every other module unchanged.
//! - [`translate`] -- one exhaustive (no wildcard arm) translation from
//!   every `commerce_core::ir::ResolvedConstraint` shape to a Solr `fq`
//!   clause, so a newly added constraint variant fails to *compile* here
//!   instead of silently falling through an unmatched catch-all -- the
//!   exact mechanism by which the `ProductTypeAny` omission above
//!   happened.
//! - [`compare`] -- a paired-comparison accumulator that structurally
//!   forbids a failed lookup from contributing a scored metric, and
//!   requires the caller to make an explicit choice between "abort
//!   before publishing any number" (the discipline `issue35_eval`
//!   already had) and "report a disclosed partial sample" (the
//!   discipline `i55_e14_paired_comparator_freeze` already had) rather
//!   than silently defaulting to neither.
//!
//! Issue #57 adds three more backends behind the same [`EngineComparator`]
//! trait, each with its own translator module (clause syntax differs per
//! engine; field-name resolution does not -- every translator reuses
//! [`translate::SolrFieldMap`]/[`translate::StructuralNames`] unchanged):
//! [`elasticsearch`] ([`ElasticsearchComparator`]/[`OpenSearchComparator`]
//! plus [`translate_es`]) and [`havenask`] ([`HavenaskComparator`] plus
//! [`translate_havenask`]).

pub mod compare;
pub mod elasticsearch;
pub mod havenask;
pub mod outcome;
pub mod solr;
pub mod translate;
pub mod translate_es;
pub mod translate_havenask;

pub use elasticsearch::{ElasticsearchComparator, OpenSearchComparator};
pub use havenask::HavenaskComparator;
pub use outcome::EngineLookup;
pub use solr::{case_insensitive_field_regex, EngineComparator, SolrComparator};
pub use translate::{translate_constraint, SolrFieldMap, StructuralNames, Translation};
pub use translate_es::{translate_all_es, translate_constraint_es, EsTranslation};
pub use translate_havenask::{
    translate_all_havenask, translate_constraint_havenask, HavenaskTranslation,
};
