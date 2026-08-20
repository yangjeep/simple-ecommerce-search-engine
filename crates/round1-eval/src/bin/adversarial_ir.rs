//! R1-E03: run the Commerce IR stress-test queries named in the Round 1
//! brief verbatim through the actual compiler, against the Phase 0
//! hand-curated shoe lexicon (`commerce_core::fixtures::shoe_lexicon`,
//! chosen because most of the brief's examples are shoe-domain and this
//! is the one lexicon in the repo built with genuine, validated
//! categorical values — isolating "does the compiler's *syntax* handle
//! OR/NOT/ranges/numeric-words" from R1-E02's already-covered "is the
//! *vocabulary* clean"). Prints exactly what each query compiles to, so
//! the classification into handled/misinterpreted/safely-unresolved is
//! reproducible from the printed output, not asserted from memory.

use commerce_core::fixtures::shoe_lexicon;
use commerce_core::ir::compile;

const QUERIES: &[&str] = &[
    "black Nike waterproof running shoes size 9 under $150",
    "black or navy running shoes",
    "Nike shoes not red",
    "size nine men's trail shoes",
    "16 inch laptop with 32GB RAM",
    "lightweight waterproof shoes for winter running",
    "gift for a runner under $100",
    "TV between $500 and $900",
    "dress shoes that aren't leather",
    "black size 9",
];

fn main() {
    let lexicon = shoe_lexicon();
    for query in QUERIES {
        let compiled = compile(query, &lexicon);
        println!("QUERY: {query:?}");
        println!("  constraints: {:?}", compiled.constraints);
        println!("  preferences: {:?}", compiled.preferences);
        println!(
            "  ambiguous:   {:?}",
            compiled
                .ambiguous
                .iter()
                .map(|a| &a.text)
                .collect::<Vec<_>>()
        );
        println!("  residual:    {:?}", compiled.residual_lexical);
        println!();
    }
}
