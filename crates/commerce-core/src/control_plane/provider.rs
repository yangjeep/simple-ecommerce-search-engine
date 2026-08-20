use std::collections::HashMap;

use crate::ir::Candidate;

use super::observe::Observation;

/// A candidate lexicon entry proposed for one unresolved term. Applying a
/// proposal means inserting `candidate` into a lexicon under `term`; it is
/// not itself a promotion — see [`super::replay`] and
/// [`super::try_promote`].
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub term: String,
    pub candidate: Candidate,
}

/// The interface every semantic-mapping proposer must implement.
/// CLAUDE.md's hard rule ("no LLM/model call in the default query hot
/// path") is enforced by *where* this trait is called from, not by
/// anything in the trait itself: `ir::compile`/`CatalogIndex::execute`
/// never reference `ModelProvider` at all, so a hot-path query can never
/// reach a model call no matter which implementation is wired in here.
/// This offline control-plane flow is the only caller.
pub trait ModelProvider {
    /// Propose a mapping for one observed term, or `None` if this provider
    /// has no confident suggestion. Implementations backed by a real model
    /// API belong behind this trait too — never in `ir` or `index`.
    fn propose(&self, observation: &Observation) -> Option<Proposal>;
}

/// A deterministic, fixture-backed `ModelProvider`: a fixed
/// term-to-candidate table with no network access and no API key,
/// satisfying CLAUDE.md's "no test may require a real model API key"
/// hard rule. This is the only `ModelProvider` this repository ships;
/// standing up a real model-backed implementation is out of scope for
/// this gate (Gate 5 only requires the interface and a way to test
/// against it deterministically).
#[derive(Debug, Default)]
pub struct FixtureModelProvider {
    mappings: HashMap<String, Candidate>,
}

impl FixtureModelProvider {
    pub fn new(mappings: impl IntoIterator<Item = (&'static str, Candidate)>) -> Self {
        FixtureModelProvider {
            mappings: mappings
                .into_iter()
                .map(|(term, c)| (term.to_string(), c))
                .collect(),
        }
    }
}

impl ModelProvider for FixtureModelProvider {
    fn propose(&self, observation: &Observation) -> Option<Proposal> {
        self.mappings
            .get(&observation.term)
            .map(|candidate| Proposal {
                term: observation.term.clone(),
                candidate: candidate.clone(),
            })
    }
}
