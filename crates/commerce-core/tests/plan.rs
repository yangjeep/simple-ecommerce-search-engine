//! Issue #6 priority 5: the structural-plus-delegated-lexical execution
//! contract (`commerce_core::plan`). `commerce_core` never depends on a
//! real lexical engine, so these tests use a small deterministic mock
//! delegate that can deliberately misbehave (return out-of-restriction or
//! constraint-violating hits) to prove `execute_planned` re-verifies
//! everything itself rather than trusting the delegate.

use std::cell::RefCell;
use std::collections::BTreeSet;

use commerce_core::domain::{
    attributes, BrandId, Catalog, CategoryId, Constraint, Inventory, NumericOp, Price, Product,
    ProductId, ProductTypeId, Variant, VariantId,
};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{CommerceQuery, ResolvedConstraint, StructuralConstraint};
use commerce_core::plan::{
    execute_planned, plan, ExecutionOutcome, LexicalDelegate, LexicalHit, PlannerPolicy,
};

const NIKE: BrandId = BrandId(1);
const OTHER: BrandId = BrandId(2);
const TEST_POLICY: PlannerPolicy = PlannerPolicy {
    selectivity_threshold: 0.5,
    delegate_oversample: 20,
};

/// 11 products: exactly 1 Nike (a genuinely selective ~9% predicate) and
/// 10 Other-brand (a deliberately non-selective ~91% predicate), so a
/// single fixture can exercise both the `Hybrid` and `Punt` (non-selective)
/// routes. Every product has one variant priced at $10 except product 0
/// (Nike), priced at $50, so a price constraint can also be tested.
fn eleven_product_catalog() -> Catalog {
    let mut products = Vec::new();
    for i in 0..11u64 {
        let brand = if i == 0 { NIKE } else { OTHER };
        let price_cents = if i == 0 { 5_000 } else { 1_000 };
        products.push(Product {
            id: ProductId(i),
            product_type: ProductTypeId(1),
            brand,
            category: CategoryId(1),
            title: format!("Product {i}"),
            attributes: attributes([]),
            variants: vec![Variant {
                id: VariantId(100 + i),
                attributes: attributes([]),
                price: Price::usd(price_cents),
                inventory: Inventory::in_stock(10),
            }],
        });
    }
    Catalog { products }
}

fn query_with(constraints: Vec<ResolvedConstraint>, residual_lexical: Vec<&str>) -> CommerceQuery {
    CommerceQuery {
        constraints,
        preferences: vec![],
        ambiguous: vec![],
        residual_lexical: residual_lexical.into_iter().map(String::from).collect(),
    }
}

type RecordedCall = (Vec<String>, Option<BTreeSet<ProductId>>);

/// A delegate whose responses are configured per-test and which records
/// every call (terms + restriction) it received, so tests can assert both
/// on `execute_planned`'s final output and on what the planner actually
/// asked the delegate to do.
struct MockDelegate {
    responses: Vec<LexicalHit>,
    calls: RefCell<Vec<RecordedCall>>,
}

impl MockDelegate {
    fn new(responses: Vec<LexicalHit>) -> Self {
        MockDelegate {
            responses,
            calls: RefCell::new(Vec::new()),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.borrow().len()
    }
}

impl LexicalDelegate for MockDelegate {
    fn search(
        &self,
        terms: &[String],
        restrict_to: Option<&BTreeSet<ProductId>>,
        _limit: usize,
    ) -> Vec<LexicalHit> {
        self.calls
            .borrow_mut()
            .push((terms.to_vec(), restrict_to.cloned()));
        self.responses.clone()
    }
}

#[test]
fn fast_path_never_calls_the_delegate() {
    let catalog = eleven_product_catalog();
    let index = CatalogIndex::build(&catalog);
    let query = query_with(
        vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
            NIKE,
        ))],
        vec![],
    );
    let delegate = MockDelegate::new(vec![]);

    let (planned, hits) =
        execute_planned(&query, &catalog, &index, Some(&delegate), 10, &TEST_POLICY);

    assert_eq!(planned.outcome, ExecutionOutcome::FastPath);
    assert_eq!(planned.selectivity, None);
    assert_eq!(
        delegate.call_count(),
        0,
        "FastPath must never call the delegate"
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].product, ProductId(0));
}

#[test]
fn hybrid_routes_when_the_structural_predicate_is_selective() {
    let catalog = eleven_product_catalog();
    let index = CatalogIndex::build(&catalog);
    let query = query_with(
        vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
            NIKE,
        ))],
        vec!["waterproof"],
    );
    let delegate = MockDelegate::new(vec![LexicalHit {
        product: ProductId(0),
        score: 1.0,
    }]);

    let (planned, hits) =
        execute_planned(&query, &catalog, &index, Some(&delegate), 10, &TEST_POLICY);

    assert_eq!(planned.outcome, ExecutionOutcome::Hybrid);
    let selectivity = planned
        .selectivity
        .expect("Hybrid always reports selectivity");
    assert!(
        (selectivity - 1.0 / 11.0).abs() < 1e-9,
        "expected ~9% selectivity, got {selectivity}"
    );
    assert_eq!(delegate.call_count(), 1);
    let (terms, restrict_to) = &delegate.calls.borrow()[0];
    assert_eq!(terms, &["waterproof".to_string()]);
    assert_eq!(
        restrict_to
            .as_ref()
            .expect("Hybrid must restrict the delegate"),
        &BTreeSet::from([ProductId(0)]),
        "Hybrid must restrict the delegate to exactly the structural candidates"
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].product, ProductId(0));
}

#[test]
fn punt_routes_when_the_structural_predicate_is_not_selective() {
    let catalog = eleven_product_catalog();
    let index = CatalogIndex::build(&catalog);
    // Brand=Other matches 10/11 products (~91%), above TEST_POLICY.selectivity_threshold.
    let query = query_with(
        vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
            OTHER,
        ))],
        vec!["cushioned"],
    );
    let delegate = MockDelegate::new(vec![LexicalHit {
        product: ProductId(1),
        score: 1.0,
    }]);

    let (planned, hits) =
        execute_planned(&query, &catalog, &index, Some(&delegate), 10, &TEST_POLICY);

    assert_eq!(planned.outcome, ExecutionOutcome::Punt);
    let selectivity = planned.selectivity.expect(
        "Punt via non-selectivity still reports the measured selectivity, unlike Punt via no constraints at all",
    );
    assert!(
        (selectivity - 10.0 / 11.0).abs() < 1e-9,
        "expected ~91% selectivity, got {selectivity}"
    );
    assert_eq!(delegate.call_count(), 1);
    let (_, restrict_to) = &delegate.calls.borrow()[0];
    assert_eq!(
        *restrict_to, None,
        "Punt must never hand the delegate a near-universal candidate set to filter against"
    );
    // The Brand=Other constraint is still enforced -- against the small
    // delegate-returned hit set, not via a bitmap intersection.
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].product, ProductId(1));
}

#[test]
fn punt_routes_when_there_is_no_structural_constraint_at_all() {
    let catalog = eleven_product_catalog();
    let index = CatalogIndex::build(&catalog);
    let query = query_with(vec![], vec!["running", "shoes"]);
    let delegate = MockDelegate::new(vec![]);

    let (planned, _hits) =
        execute_planned(&query, &catalog, &index, Some(&delegate), 10, &TEST_POLICY);

    assert_eq!(planned.outcome, ExecutionOutcome::Punt);
    assert_eq!(
        planned.selectivity, None,
        "no structural constraint means no candidate set is ever computed"
    );
    assert_eq!(delegate.call_count(), 1);
}

/// Note: this test's misbehaving-delegate scenario happens to also
/// violate the query's own Brand constraint, so `matches_variant` alone
/// would already drop product 5 here -- it does not, by itself, isolate
/// `restrict_to`'s independent contribution from the constraint check's.
/// `commerce_core::plan::tests::restrict_to_independently_excludes_a_constraint_satisfying_hit`
/// (a white-box unit test in `plan/mod.rs`) isolates that specifically,
/// with a constraint-free query where only `restrict_to` can be
/// responsible for the exclusion. Kept here too because it still validates
/// real end-to-end behavior through the public `execute_planned` API, just
/// not in isolation from the constraint check.
#[test]
fn verify_and_truncate_drops_a_delegate_hit_outside_restrict_to() {
    let catalog = eleven_product_catalog();
    let index = CatalogIndex::build(&catalog);
    let query = query_with(
        vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
            NIKE,
        ))],
        vec!["waterproof"],
    );
    // A misbehaving delegate: returns a hit for product 5 (Other brand,
    // NOT in the Nike-only restrict_to set) alongside the correct one.
    let delegate = MockDelegate::new(vec![
        LexicalHit {
            product: ProductId(5),
            score: 2.0,
        },
        LexicalHit {
            product: ProductId(0),
            score: 1.0,
        },
    ]);

    let (planned, hits) =
        execute_planned(&query, &catalog, &index, Some(&delegate), 10, &TEST_POLICY);

    assert_eq!(planned.outcome, ExecutionOutcome::Hybrid);
    assert_eq!(
        hits.len(),
        1,
        "the out-of-restriction hit must be dropped even though the delegate returned it"
    );
    assert_eq!(hits[0].product, ProductId(0));
}

#[test]
fn verify_and_truncate_drops_a_delegate_hit_that_fails_a_hard_constraint() {
    let catalog = eleven_product_catalog();
    let index = CatalogIndex::build(&catalog);
    // No structural constraint (so restrict_to is None / Punt), but a hard
    // price constraint the delegate cannot see or enforce itself.
    let query = query_with(
        vec![ResolvedConstraint::Attribute(Constraint::Numeric {
            attribute: "price_cents_placeholder".to_string(),
            op: NumericOp::Lt,
            value: 0.0, // impossible attribute constraint, always false for every product
        })],
        vec!["cheap"],
    );
    let delegate = MockDelegate::new(vec![LexicalHit {
        product: ProductId(1),
        score: 1.0,
    }]);

    let (_planned, hits) =
        execute_planned(&query, &catalog, &index, Some(&delegate), 10, &TEST_POLICY);

    assert!(
        hits.is_empty(),
        "a delegate hit that fails a hard constraint must never be returned, \
         regardless of how the delegate ranked it"
    );
}

#[test]
fn verify_and_truncate_respects_k() {
    let catalog = eleven_product_catalog();
    let index = CatalogIndex::build(&catalog);
    let query = query_with(vec![], vec!["product"]);
    let delegate = MockDelegate::new(
        (1..11u64)
            .map(|i| LexicalHit {
                product: ProductId(i),
                score: 1.0 / i as f64,
            })
            .collect(),
    );

    let (_planned, hits) =
        execute_planned(&query, &catalog, &index, Some(&delegate), 3, &TEST_POLICY);

    assert_eq!(hits.len(), 3, "k must be respected after verification");
    assert_eq!(
        hits.iter().map(|h| h.product).collect::<Vec<_>>(),
        vec![ProductId(1), ProductId(2), ProductId(3)],
        "the delegate's original order must be preserved through verification"
    );
}

#[test]
fn no_delegate_degrades_hybrid_and_punt_to_empty_but_fast_path_still_works() {
    let catalog = eleven_product_catalog();
    let index = CatalogIndex::build(&catalog);

    let fast_path_query = query_with(
        vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
            NIKE,
        ))],
        vec![],
    );
    let (planned, hits) =
        execute_planned(&fast_path_query, &catalog, &index, None, 10, &TEST_POLICY);
    assert_eq!(planned.outcome, ExecutionOutcome::FastPath);
    assert_eq!(hits.len(), 1);

    let hybrid_query = query_with(
        vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
            NIKE,
        ))],
        vec!["waterproof"],
    );
    let (planned, hits) = execute_planned(&hybrid_query, &catalog, &index, None, 10, &TEST_POLICY);
    assert_eq!(planned.outcome, ExecutionOutcome::Hybrid);
    assert!(
        hits.is_empty(),
        "no delegate means no lexical hits, not a panic"
    );
}

#[test]
fn plan_alone_does_not_require_a_delegate_or_execute_anything() {
    let catalog = eleven_product_catalog();
    let index = CatalogIndex::build(&catalog);
    let query = query_with(
        vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
            OTHER,
        ))],
        vec!["cushioned"],
    );
    let planned = plan(&query, &index, catalog.products.len(), &TEST_POLICY);
    assert_eq!(planned.outcome, ExecutionOutcome::Punt);
}
