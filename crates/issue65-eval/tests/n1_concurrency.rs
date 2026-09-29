//! Issue #65 (amendment 1 section 5.4): N1 under concurrency returns exactly
//! the single-threaded executor's answers -- no request sees another
//! request's candidate set or facet counters, for identical and mixed
//! request streams.

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::Arc;

use commerce_core::domain::{
    attributes, AttributeValue, BrandId, Catalog, CategoryId, Inventory, Price, Product, ProductId,
    ProductTypeId, Variant, VariantId,
};
use commerce_core::index::CatalogIndex;
use issue65_eval::observe::{parse, Observed};
use issue65_eval::server::{handle_lookup, handle_plp, serve, State};
use issue79_eval::plp::SortStructures;

fn catalog() -> Catalog {
    let mut state = 65u64;
    let mut next = move |m: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        (state >> 33) % m
    };
    let colors = ["black", "white", "red", "green", "blue"];
    let styles = ["modern", "rustic", "classic"];
    let mut products = Vec::new();
    let mut vid = 1u64;
    for p in 0..300u64 {
        let mut pattrs = vec![(
            "style",
            AttributeValue::Enum(styles[next(3) as usize].to_owned()),
        )];
        pattrs.push((
            "category_depth_1",
            AttributeValue::Enum(format!("dept{}", next(3))),
        ));
        let variants = (0..1 + next(3))
            .map(|_| {
                vid += 1;
                Variant {
                    id: VariantId(vid),
                    attributes: attributes([
                        (
                            "color",
                            AttributeValue::Enum(colors[next(5) as usize].to_owned()),
                        ),
                        ("rating", AttributeValue::Numeric(next(10) as f64 / 2.0)),
                    ]),
                    price: Price::usd(100),
                    inventory: Inventory::in_stock(1),
                }
            })
            .collect();
        products.push(Product {
            id: ProductId(p),
            product_type: ProductTypeId(1),
            brand: BrandId(1),
            category: CategoryId(0),
            title: format!("p{p}"),
            attributes: attributes(pattrs),
            variants,
        });
    }
    Catalog { products }
}

fn state() -> State {
    let catalog = catalog();
    let index = CatalogIndex::build(&catalog);
    let structures = SortStructures::build(&index, &["rating"], &["rating"]);
    let source_id_by_product: HashMap<ProductId, String> = catalog
        .products
        .iter()
        .map(|p| (p.id, format!("src-{}", p.id.0)))
        .collect();
    let product_by_source_id = source_id_by_product
        .iter()
        .map(|(p, s)| (s.clone(), *p))
        .collect();
    State {
        catalog,
        index,
        category_id_by_leaf: HashMap::new(),
        source_id_by_product,
        product_by_source_id,
        structures,
        tau_f: Some(919.497_239_065_769_5),
        rho_s: Some(0.083_377_913_197_190_3),
        solr_url: None,
        correctness: None,
    }
}

fn targets() -> Vec<String> {
    let modes = "facet_mode=hybrid&sort_mode=hybrid&cand_mode=p0r";
    let mut out = vec![
        format!("/plp?facets=color,style&topk=48&{modes}"),
        format!("/plp?filter=category_depth_1:dept1&facets=color,style&topk=48&{modes}"),
        format!("/plp?filter=category_depth_1:dept2&filter=color:red&facets=color,style&topk=48&{modes}"),
        format!("/plp?range=rating:gte:2&sort=rating:desc&topk=48&{modes}"),
        format!("/plp?filter=style:modern&sort=rating:desc&topk=48&{modes}"),
        format!("/plp?filter=color:black&filter=style:rustic&topk=48&{modes}"),
        format!("/plp?anyfilter=color:black|white&facets=color,style&topk=48&{modes}"),
    ];
    for p in [0, 7, 150, 299, 1000] {
        out.push(format!("/lookup?id=src-{p}"));
    }
    out
}

fn direct(state: &State, target: &str) -> Observed {
    let body = if target.starts_with("/lookup") {
        handle_lookup(state, target).expect("lookup")
    } else {
        handle_plp(state, target).expect("plp")
    };
    parse(&body, false, None).expect("parse")
}

#[test]
fn concurrent_n1_matches_single_threaded_answers() {
    let reference_state = state();
    let targets = targets();
    let expected: Vec<Observed> = targets
        .iter()
        .map(|t| direct(&reference_state, t))
        .collect();
    assert!(
        expected.iter().any(|o| !o.facets.is_empty()),
        "fixture must exercise facets"
    );

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let shared = Arc::new(state());
    std::thread::spawn(move || serve(listener, shared, 3));

    let targets = Arc::new(targets);
    let expected = Arc::new(expected);
    let handles: Vec<_> = (0..16)
        .map(|t| {
            let (targets, expected) = (Arc::clone(&targets), Arc::clone(&expected));
            std::thread::spawn(move || {
                let agent = ureq::AgentBuilder::new().build();
                let mut mismatches = 0;
                for k in 0..50 {
                    // Identical requests on some threads, mixed on others.
                    let i = if t % 4 == 0 {
                        0
                    } else {
                        (t * 7 + k * 3) % targets.len()
                    };
                    let body = agent
                        .get(&format!("http://127.0.0.1:{port}{}", targets[i]))
                        .call()
                        .expect("call")
                        .into_string()
                        .expect("body");
                    if parse(&body, false, None).expect("parse") != expected[i] {
                        mismatches += 1;
                    }
                }
                mismatches
            })
        })
        .collect();
    let mismatches: usize = handles.into_iter().map(|h| h.join().expect("join")).sum();
    assert_eq!(mismatches, 0);
}
