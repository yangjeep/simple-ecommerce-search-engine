//! Independent scan oracle: these tests deliberately do not call commerce-core
//! filtering, tokenization, ranking, or facet helpers to compute expectations.
use athos80_eval::*;
use std::collections::{BTreeMap, BTreeSet};

fn en(key: &str, value: &str) -> (String, Value) {
    (key.into(), Value::Enum(value.into()))
}
fn variant(id: u64, color: &str, size: &str, stock: bool, price: i64) -> VariantInput {
    VariantInput {
        variant_id: id,
        price_cents: price,
        available_units: u32::from(stock),
        in_stock: stock,
        attributes: [en("color", color), en("size", size)].into_iter().collect(),
    }
}
fn product(id: u64, variants: Vec<VariantInput>) -> ProductInput {
    ProductInput {
        product_id: id,
        title: "Everyday shoes".into(),
        brand_id: id as u32,
        category_id: 10,
        product_type_id: 20,
        attributes: BTreeMap::new(),
        variants,
    }
}
fn fixture() -> CatalogInput {
    CatalogInput {
        products: vec![
            product(
                1,
                vec![
                    variant(11, "red", "S", true, 100),
                    variant(12, "blue", "L", false, 50),
                ],
            ),
            product(2, vec![variant(21, "red", "L", true, 200)]),
            product(
                3,
                vec![
                    variant(31, "red", "L", true, 300),
                    variant(32, "blue", "L", true, 250),
                ],
            ),
        ],
    }
}
fn ef(attribute: &str, value: &str) -> Filter {
    Filter::Enum {
        attribute: attribute.into(),
        value: value.into(),
    }
}
fn facet(name: &str) -> FacetRequest {
    FacetRequest {
        name: name.into(),
        field: FacetField::Attribute { name: name.into() },
    }
}
fn rule(id: &str, target: Target, action: Action) -> Rule {
    Rule {
        id: id.into(),
        scope: Scope::Global,
        priority: 0,
        conditions: vec![],
        target,
        action,
    }
}
fn pt(ids: &[u64]) -> Target {
    Target::Products { ids: ids.to_vec() }
}
fn vt(ids: &[u64]) -> Target {
    Target::Variants { ids: ids.to_vec() }
}
fn ids(r: &Response) -> Vec<(u64, u64)> {
    r.hits
        .iter()
        .map(|h| (h.product_id, h.selected_variant_id))
        .collect()
}
fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .collect()
}
fn value<'a>(p: &'a ProductInput, v: &'a VariantInput, key: &str) -> Option<&'a Value> {
    v.attributes.get(key).or_else(|| p.attributes.get(key))
}
fn match_filter(f: &Filter, p: &ProductInput, v: &VariantInput) -> bool {
    match f {
        Filter::Enum {
            attribute,
            value: want,
        } => matches!(value(p,v,attribute), Some(Value::Enum(have)) if have == want),
        Filter::MultiEnumContains {
            attribute,
            value: want,
        } => matches!(value(p,v,attribute), Some(Value::MultiEnum(have)) if have.contains(want)),
        Filter::Boolean {
            attribute,
            value: want,
        } => matches!(value(p,v,attribute), Some(Value::Boolean(have)) if have == want),
        Filter::Numeric {
            attribute,
            op,
            value: want,
        } => match value(p, v, attribute) {
            Some(Value::Numeric(have)) => match op {
                NumberOp::Eq => have == want,
                NumberOp::Lt => have < want,
                NumberOp::Lte => have <= want,
                NumberOp::Gt => have > want,
                NumberOp::Gte => have >= want,
            },
            _ => false,
        },
        Filter::TextContains {
            attribute,
            value: want,
        } => matches!(value(p,v,attribute), Some(Value::Text(have)) if have.contains(want)),
        Filter::Brand { value } => p.brand_id == *value,
        Filter::Category { value } => p.category_id == *value,
        Filter::ProductType { value } => p.product_type_id == *value,
        Filter::PriceUnderCents { value } => v.price_cents < *value,
        Filter::PriceOverCents { value } => v.price_cents > *value,
    }
}
fn target_matches(t: &Target, p: &ProductInput, v: &VariantInput) -> bool {
    match t {
        Target::All => true,
        Target::Products { ids } => ids.contains(&p.product_id),
        Target::Variants { ids } => ids.contains(&v.variant_id),
        Target::Filters { filters } => filters.iter().all(|f| match_filter(f, p, v)),
    }
}
fn match_lexical(q: &str, p: &ProductInput, v: &VariantInput) -> bool {
    let mut merged = p.attributes.clone();
    merged.extend(v.attributes.clone());
    let mut have: BTreeSet<String> = tokens(&p.title).into_iter().collect();
    for attr in merged.values() {
        if let Value::Text(s) = attr {
            have.extend(tokens(s));
        }
    }
    tokens(q).iter().all(|t| have.contains(t))
}
fn filter_field(f: &Filter) -> FacetField {
    match f {
        Filter::Enum { attribute, .. }
        | Filter::MultiEnumContains { attribute, .. }
        | Filter::Boolean { attribute, .. }
        | Filter::Numeric { attribute, .. }
        | Filter::TextContains { attribute, .. } => FacetField::Attribute {
            name: attribute.clone(),
        },
        Filter::Brand { .. } => FacetField::Brand,
        Filter::Category { .. } => FacetField::Category,
        Filter::ProductType { .. } => FacetField::ProductType,
        Filter::PriceUnderCents { .. } | Filter::PriceOverCents { .. } => FacetField::PriceCents,
    }
}
fn facet_values(f: &FacetField, p: &ProductInput, v: &VariantInput) -> Vec<String> {
    match f {
        FacetField::Attribute { name } => match value(p, v, name) {
            Some(Value::Enum(s)) | Some(Value::Text(s)) => vec![s.clone()],
            Some(Value::MultiEnum(a)) => a.clone(),
            Some(Value::Boolean(b)) => vec![b.to_string()],
            Some(Value::Numeric(n)) => vec![n.to_string()],
            None => vec![],
        },
        FacetField::Brand => vec![p.brand_id.to_string()],
        FacetField::Category => vec![p.category_id.to_string()],
        FacetField::ProductType => vec![p.product_type_id.to_string()],
        FacetField::PriceCents => vec![v.price_cents.to_string()],
    }
}
fn scan(c: &CatalogInput, r: &Request, without: Option<&FacetField>) -> Vec<(u64, u64)> {
    c.products
        .iter()
        .flat_map(|p| {
            p.variants.iter().filter_map(move |v| {
                let hidden = r
                    .rules
                    .iter()
                    .filter(|x| matches!(x.action, Action::Hide))
                    .any(|x| target_matches(&x.target, p, v));
                let lexical = match &r.lexical {
                    LexicalInput::ExactTokenAnd => match_lexical(&r.q, p, v),
                    LexicalInput::Bm25 | LexicalInput::Bm25Literal => panic!("BM25 is validated through separate lexical fixtures, not this exact-token oracle"),
                    LexicalInput::Supplied { hits } => hits
                        .iter()
                        .any(|h| h.product_id == p.product_id && h.variant_id == v.variant_id),
                };
                (lexical
                    && (!r.in_stock_only || (v.in_stock && v.available_units > 0))
                    && !hidden
                    && r.filters
                        .iter()
                        .filter(|f| without != Some(&filter_field(f)))
                        .all(|f| match_filter(f, p, v)))
                .then_some((p.product_id, v.variant_id))
            })
        })
        .collect()
}
fn scan_facets(c: &CatalogInput, r: &Request) -> Vec<FacetResult> {
    let mode = r.facet_count_mode.unwrap_or(r.result_mode);
    r.facets
        .iter()
        .map(|f| {
            let eligible: BTreeSet<_> = scan(c, r, Some(&f.field)).into_iter().collect();
            let mut buckets: BTreeMap<String, BTreeSet<(u64, u64)>> = BTreeMap::new();
            for p in &c.products {
                for v in &p.variants {
                    if !eligible.contains(&(p.product_id, v.variant_id)) {
                        continue;
                    }
                    for val in facet_values(&f.field, p, v) {
                        buckets.entry(val).or_default().insert((
                            p.product_id,
                            if mode == ResultMode::Products {
                                0
                            } else {
                                v.variant_id
                            },
                        ));
                    }
                }
            }
            FacetResult {
                name: f.name.clone(),
                counts: buckets.into_iter().map(|(k, v)| (k, v.len())).collect(),
            }
        })
        .collect()
}

#[test]
fn exhaustive_scan_oracle_modes_filters_self_excluding_facets_and_hides() {
    let c = fixture();
    let engine = Engine::new(c.clone()).unwrap();
    let mut cases = 0;
    for mode in [ResultMode::Products, ResultMode::Variants] {
        for facet_mode in [None, Some(ResultMode::Products), Some(ResultMode::Variants)] {
            for color in [None, Some("red"), Some("blue"), Some("green")] {
                for size in [None, Some("S"), Some("L")] {
                    for in_stock_only in [false, true] {
                        for hide in [false, true] {
                            let mut r = Request {
                                result_mode: mode,
                                facet_count_mode: facet_mode,
                                in_stock_only,
                                facets: vec![facet("color"), facet("size")],
                                ..Request::default()
                            };
                            if let Some(v) = color {
                                r.filters.push(ef("color", v));
                            }
                            if let Some(v) = size {
                                r.filters.push(ef("size", v));
                            }
                            if hide {
                                r.rules.push(rule("hide-r-red", vt(&[31]), Action::Hide));
                            }
                            let all = scan(&c, &r, None);
                            let expected: BTreeSet<_> = all
                                .iter()
                                .map(|(p, v)| {
                                    (*p, if mode == ResultMode::Products { 0 } else { *v })
                                })
                                .collect();
                            let actual = engine.search(&r).unwrap();
                            assert_eq!(actual.num_found, expected.len(), "case {cases}");
                            let actual_ids: BTreeSet<_> = ids(&actual)
                                .iter()
                                .map(|(p, v)| {
                                    (*p, if mode == ResultMode::Products { 0 } else { *v })
                                })
                                .collect();
                            assert_eq!(actual_ids, expected, "case {cases}");
                            assert_eq!(actual.facets, scan_facets(&c, &r), "case {cases}");
                            for h in &actual.hits {
                                assert!(
                                    all.contains(&(h.product_id, h.selected_variant_id)),
                                    "selected ineligible sibling"
                                );
                                let n = if mode == ResultMode::Products {
                                    all.iter().filter(|(p, _)| *p == h.product_id).count()
                                } else {
                                    1
                                };
                                assert_eq!(h.matching_variant_count, n);
                            }
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 288);
}

#[test]
fn no_cross_variant_filter_or_stock_leakage() {
    let e = Engine::new(fixture()).unwrap();
    let red_l = e
        .search(&Request {
            filters: vec![ef("color", "red"), ef("size", "L")],
            ..Request::default()
        })
        .unwrap();
    assert_eq!(ids(&red_l), vec![(2, 21), (3, 31)]);
    let blue = e
        .search(&Request {
            filters: vec![ef("color", "blue")],
            ..Request::default()
        })
        .unwrap();
    assert_eq!(ids(&blue), vec![(3, 32)]);
}

#[test]
fn product_can_count_in_multiple_buckets_but_once_per_bucket() {
    let e = Engine::new(fixture()).unwrap();
    let r = e
        .search(&Request {
            facets: vec![facet("color"), facet("size")],
            ..Request::default()
        })
        .unwrap();
    assert_eq!(r.num_found, 3);
    assert_eq!(
        r.facets[0].counts,
        BTreeMap::from([("blue".into(), 1), ("red".into(), 3)])
    );
    assert_eq!(
        r.facets[1].counts,
        BTreeMap::from([("L".into(), 2), ("S".into(), 1)])
    );
}

#[test]
fn boosted_sibling_selected_only_after_complete_variant_ranking() {
    let e = Engine::new(fixture()).unwrap();
    let r = e
        .search(&Request {
            personal_signals: vec![PersonalSignal {
                target: vt(&[32]),
                factor: 4.0,
            }],
            ..Request::default()
        })
        .unwrap();
    assert_eq!(ids(&r)[0], (3, 32));
    assert_eq!(r.hits[0].matching_variant_count, 2);
    let filtered = e
        .search(&Request {
            filters: vec![ef("color", "red")],
            personal_signals: vec![PersonalSignal {
                target: vt(&[32]),
                factor: 4.0,
            }],
            ..Request::default()
        })
        .unwrap();
    assert_eq!(ids(&filtered), vec![(1, 11), (2, 21), (3, 31)]);
}

#[test]
fn global_rule_can_bring_organic_rank_100_to_first_before_top_k() {
    let c = CatalogInput {
        products: (1..=100)
            .map(|i| product(i, vec![variant(i, "red", "L", true, 100)]))
            .collect(),
    };
    let e = Engine::new(c).unwrap();
    let baseline = e
        .search(&Request {
            limit: 100,
            ..Request::default()
        })
        .unwrap();
    assert_eq!(baseline.hits[99].product_id, 100);
    let r = e
        .search(&Request {
            limit: 1,
            rules: vec![rule(
                "last-to-first",
                pt(&[100]),
                Action::Group { order: 0, rank: 1 },
            )],
            ..Request::default()
        })
        .unwrap();
    assert_eq!(r.hits[0].product_id, 100);
    assert_eq!(r.num_found, 100);
    assert_eq!(r.metrics.ranked_variants, 100);
}

#[test]
fn relevance_then_ordered_groups_cannot_be_overturned_by_personalization() {
    let e = Engine::new(fixture()).unwrap();
    let supplied = LexicalInput::Supplied {
        hits: vec![
            LexicalHit {
                product_id: 1,
                variant_id: 11,
                score: 1.0,
                relevance_bucket: 2,
            },
            LexicalHit {
                product_id: 2,
                variant_id: 21,
                score: 100.0,
                relevance_bucket: 1,
            },
            LexicalHit {
                product_id: 3,
                variant_id: 31,
                score: 1.0,
                relevance_bucket: 1,
            },
        ],
    };
    let r = e
        .search(&Request {
            lexical: supplied,
            personal_signals: vec![PersonalSignal {
                target: pt(&[2]),
                factor: 4.0,
            }],
            rules: vec![
                rule("first-axis", pt(&[3]), Action::Group { order: 0, rank: 1 }),
                rule(
                    "second-axis",
                    pt(&[2]),
                    Action::Group {
                        order: 1,
                        rank: i32::MAX,
                    },
                ),
            ],
            ..Request::default()
        })
        .unwrap();
    assert_eq!(ids(&r), vec![(1, 11), (3, 31), (2, 21)]);
}

#[test]
fn lexical_score_coexists_with_merch_and_bounded_personal_factor() {
    let e = Engine::new(fixture()).unwrap();
    let lexical = LexicalInput::Supplied {
        hits: vec![
            LexicalHit {
                product_id: 1,
                variant_id: 11,
                score: 3.0,
                relevance_bucket: 1,
            },
            LexicalHit {
                product_id: 2,
                variant_id: 21,
                score: 1.0,
                relevance_bucket: 1,
            },
            LexicalHit {
                product_id: 3,
                variant_id: 31,
                score: 1.0,
                relevance_bucket: 1,
            },
        ],
    };
    let r = e
        .search(&Request {
            lexical: lexical.clone(),
            personal_signals: vec![PersonalSignal {
                target: pt(&[2]),
                factor: 2.0,
            }],
            ..Request::default()
        })
        .unwrap();
    assert_eq!(ids(&r), vec![(1, 11), (2, 21), (3, 31)]);
    assert_eq!(r.hits[1].lexical_score, 1.0);
    assert_eq!(r.hits[1].composed_score, 2.0);
    let boosted = e
        .search(&Request {
            lexical,
            personal_signals: vec![PersonalSignal {
                target: pt(&[2]),
                factor: 4.0,
            }],
            ..Request::default()
        })
        .unwrap();
    assert_eq!(boosted.hits[0].product_id, 2);
}

#[test]
fn factors_are_bounded_finite_and_composed_without_overflow() {
    let e = Engine::new(fixture()).unwrap();
    for factor in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        0.0,
        -1.0,
        0.249,
        4.001,
    ] {
        assert!(
            e.search(&Request {
                personal_signals: vec![PersonalSignal {
                    target: Target::All,
                    factor
                }],
                ..Request::default()
            })
            .is_err(),
            "accepted {factor}"
        );
    }
    let r = e
        .search(&Request {
            personal_signals: (0..100)
                .map(|_| PersonalSignal {
                    target: Target::All,
                    factor: 4.0,
                })
                .collect(),
            ..Request::default()
        })
        .unwrap();
    assert!(r
        .hits
        .iter()
        .all(|h| h.personal_factor == 4.0 && h.composed_score.is_finite()));
    let plain = e.search(&Request::default()).unwrap();
    assert!(plain.hits.iter().all(|h| h.personal_factor == 1.0));
}

#[test]
fn pins_are_eligible_only_hide_wins_conflicts_are_deterministic_and_pagination_is_global() {
    let e = Engine::new(fixture()).unwrap();
    let mut weak = rule("weak", pt(&[1]), Action::Pin { position: 0 });
    weak.priority = 1;
    let mut strong = rule("strong", pt(&[2]), Action::Pin { position: 0 });
    strong.priority = 2;
    let rules = vec![
        weak,
        strong,
        rule("oos", vt(&[12]), Action::Pin { position: 0 }),
        rule("missing", pt(&[999]), Action::Pin { position: 0 }),
    ];
    let mut request = Request {
        rules,
        limit: 1,
        ..Request::default()
    };
    let first = e.search(&request).unwrap();
    assert_eq!(ids(&first), vec![(2, 21)]);
    request.offset = 1;
    assert_eq!(ids(&e.search(&request).unwrap()), vec![(1, 11)]);
    request.offset = 2;
    assert_eq!(ids(&e.search(&request).unwrap()), vec![(3, 31)]);
    request.offset = 0;
    request.limit = 20;
    request.rules.push(rule("hide-q", pt(&[2]), Action::Hide));
    assert_eq!(ids(&e.search(&request).unwrap()), vec![(1, 11), (3, 31)]);
    request.filters = vec![ef("size", "L")];
    assert_eq!(ids(&e.search(&request).unwrap()), vec![(3, 31)]);
}

#[test]
fn explicit_variant_pin_displays_that_eligible_variant_in_products_mode() {
    let e = Engine::new(fixture()).unwrap();
    let r = e
        .search(&Request {
            rules: vec![rule("pin-blue", vt(&[32]), Action::Pin { position: 0 })],
            ..Request::default()
        })
        .unwrap();
    assert_eq!(ids(&r)[0], (3, 32));
    assert_eq!(r.hits[0].pin_rule_id.as_deref(), Some("pin-blue"));
}

#[test]
fn category_and_query_scopes_replace_whole_global_group_set() {
    let e = Engine::new(fixture()).unwrap();
    let global = rule(
        "global",
        pt(&[1]),
        Action::Group {
            order: 0,
            rank: 999,
        },
    );
    let mut category = rule("category", pt(&[2]), Action::Group { order: 1, rank: 1 });
    category.scope = Scope::Category { category_id: 10 };
    let mut query = rule("query", pt(&[3]), Action::Group { order: 2, rank: 1 });
    query.scope = Scope::Query {
        query: "SHOES!!!".into(),
    };
    let rules = vec![global, category, query];
    let base = Request {
        rules: rules.clone(),
        ..Request::default()
    };
    assert_eq!(e.search(&base).unwrap().hits[0].product_id, 1);
    let cat = Request {
        context: Context {
            category_id: Some(10),
            tags: vec![],
        },
        ..base.clone()
    };
    assert_eq!(e.search(&cat).unwrap().hits[0].product_id, 2);
    let specific = Request {
        q: "shoes".into(),
        ..cat
    };
    assert_eq!(e.search(&specific).unwrap().hits[0].product_id, 3);
    // A filter is not a category-page context and must not fire that scope.
    let filter_only = Request {
        filters: vec![Filter::Category { value: 10 }],
        ..base
    };
    assert_eq!(e.search(&filter_only).unwrap().hits[0].product_id, 1);
}

#[test]
fn rule_conditions_are_conjunctive_and_per_axis_conflict_uses_priority_then_id() {
    let e = Engine::new(fixture()).unwrap();
    let mut a = rule("a", pt(&[2]), Action::Group { order: 0, rank: 2 });
    a.conditions = vec![
        Condition::QueryContainsAll {
            tokens: vec!["SHOES".into()],
        },
        Condition::ContextTag { tag: "vip".into() },
    ];
    let mut b = rule("b", pt(&[2]), Action::Group { order: 0, rank: -2 });
    b.conditions = a.conditions.clone();
    let r = Request {
        q: "shoes".into(),
        rules: vec![b.clone(), a.clone()],
        ..Request::default()
    };
    assert_eq!(e.search(&r).unwrap().hits[0].product_id, 1);
    let active = Request {
        context: Context {
            category_id: None,
            tags: vec!["vip".into()],
        },
        ..r
    };
    let found = e.search(&active).unwrap();
    assert_eq!(found.hits[0].product_id, 2);
    assert_eq!(found.hits[0].group_rule_ids, vec![Some("a".into())]);
    b.priority = 1;
    let stronger = Request {
        rules: vec![a, b],
        ..active
    };
    assert_eq!(
        e.search(&stronger).unwrap().hits.last().unwrap().product_id,
        2
    );
}

#[test]
fn shopper_sort_and_merch_override_are_explicit_distinct_policies() {
    let e = Engine::new(fixture()).unwrap();
    let mut r = Request {
        sort: Sort::PriceAsc,
        rules: vec![
            rule("promote-r", pt(&[3]), Action::Group { order: 0, rank: 1 }),
            rule("pin-r", pt(&[3]), Action::Pin { position: 0 }),
        ],
        personal_signals: vec![PersonalSignal {
            target: pt(&[3]),
            factor: 4.0,
        }],
        ..Request::default()
    };
    assert_eq!(ids(&e.search(&r).unwrap()), vec![(1, 11), (2, 21), (3, 32)]);
    r.sort_policy = SortPolicy::MerchOverrides;
    assert_eq!(ids(&e.search(&r).unwrap())[0], (3, 32));
    r.rules.push(rule("hide-r", pt(&[3]), Action::Hide));
    assert_eq!(ids(&e.search(&r).unwrap()), vec![(1, 11), (2, 21)]);
}

#[test]
fn deterministic_ids_survive_catalog_and_rule_permutation_and_page_replay() {
    let c = fixture();
    let mut reversed = c.clone();
    reversed.products.reverse();
    for p in &mut reversed.products {
        p.variants.reverse();
    }
    let a = Engine::new(c).unwrap();
    let b = Engine::new(reversed).unwrap();
    for mode in [ResultMode::Products, ResultMode::Variants] {
        for offset in 0..=6 {
            let r = Request {
                result_mode: mode,
                offset,
                limit: 1,
                ..Request::default()
            };
            assert_eq!(ids(&a.search(&r).unwrap()), ids(&b.search(&r).unwrap()));
        }
    }
}

#[test]
fn supplied_hits_reverify_filters_and_reject_invalid_identity_duplicate_and_scores() {
    let e = Engine::new(fixture()).unwrap();
    let hit = |p, v, score| LexicalHit {
        product_id: p,
        variant_id: v,
        score,
        relevance_bucket: 0,
    };
    let r = Request {
        lexical: LexicalInput::Supplied {
            hits: vec![hit(1, 11, 1.0), hit(1, 12, 999.0), hit(2, 21, 2.0)],
        },
        filters: vec![ef("size", "L")],
        ..Request::default()
    };
    assert_eq!(ids(&e.search(&r).unwrap()), vec![(2, 21)]);
    for hits in [
        vec![hit(999, 999, 1.0)],
        vec![hit(2, 11, 1.0)],
        vec![hit(1, 11, 1.0), hit(1, 11, 2.0)],
        vec![hit(1, 11, f64::NAN)],
        vec![hit(1, 11, f64::INFINITY)],
        vec![hit(1, 11, -1.0)],
    ] {
        assert!(e
            .search(&Request {
                lexical: LexicalInput::Supplied { hits },
                ..Request::default()
            })
            .is_err());
    }
}

#[test]
fn exact_and_lexical_profile_matches_independent_scan_and_effective_text_scope() {
    let mut c = fixture();
    c.products[0].title = "Waterproof Running Shoe".into();
    c.products[1].title = "Water Shoe".into();
    c.products[2].title = "Running Shoe".into();
    c.products[2].attributes.insert(
        "description".into(),
        Value::Text("shared waterproof".into()),
    );
    c.products[2].variants[1]
        .attributes
        .insert("description".into(), Value::Text("blue lightweight".into()));
    let e = Engine::new(c.clone()).unwrap();
    for q in [
        "",
        "   ",
        "SHOE",
        "shoe running",
        "water",
        "waterproof",
        "shoe waterproof",
        "lightweight",
        "missing",
        "waterproof waterproof",
    ] {
        let r = Request {
            q: q.into(),
            result_mode: ResultMode::Variants,
            ..Request::default()
        };
        let actual = e.search(&r).unwrap();
        assert_eq!(
            ids(&actual).into_iter().collect::<BTreeSet<_>>(),
            scan(&c, &r, None).into_iter().collect(),
            "query {q:?}"
        );
    }
}

#[test]
fn typed_filters_and_multivalue_facets_use_variant_override_and_deduplicate_values() {
    let mut c = fixture();
    for p in &mut c.products {
        p.attributes.insert(
            "tags".into(),
            Value::MultiEnum(vec!["sale".into(), "sale".into(), "all".into()]),
        );
        p.attributes.insert("active".into(), Value::Boolean(true));
        p.attributes.insert("rating".into(), Value::Numeric(4.0));
        p.attributes
            .insert("description".into(), Value::Text("soft sole".into()));
    }
    c.products[2].variants[1]
        .attributes
        .insert("tags".into(), Value::MultiEnum(vec!["new".into()]));
    c.products[2].variants[1]
        .attributes
        .insert("rating".into(), Value::Numeric(5.0));
    let e = Engine::new(c.clone()).unwrap();
    let all_filters = vec![
        Filter::MultiEnumContains {
            attribute: "tags".into(),
            value: "sale".into(),
        },
        Filter::Boolean {
            attribute: "active".into(),
            value: true,
        },
        Filter::Numeric {
            attribute: "rating".into(),
            op: NumberOp::Gte,
            value: 4.5,
        },
        Filter::TextContains {
            attribute: "description".into(),
            value: "soft".into(),
        },
        Filter::PriceUnderCents { value: 300 },
        Filter::PriceOverCents { value: 100 },
        Filter::Brand { value: 3 },
        Filter::Category { value: 10 },
        Filter::ProductType { value: 20 },
    ];
    for filter in all_filters {
        for mode in [ResultMode::Products, ResultMode::Variants] {
            let r = Request {
                filters: vec![filter.clone()],
                result_mode: mode,
                facets: vec![
                    facet("tags"),
                    facet("active"),
                    FacetRequest {
                        name: "brand".into(),
                        field: FacetField::Brand,
                    },
                ],
                ..Request::default()
            };
            let actual = e.search(&r).unwrap();
            let expected = scan(&c, &r, None);
            assert_eq!(
                actual.num_found,
                expected
                    .iter()
                    .map(|(p, v)| (*p, if mode == ResultMode::Products { 0 } else { *v }))
                    .collect::<BTreeSet<_>>()
                    .len()
            );
            assert_eq!(actual.facets, scan_facets(&c, &r));
        }
    }
}

#[test]
fn defaults_are_explicit_and_unknown_json_fields_are_rejected() {
    let r: Request = serde_json::from_str("{}").unwrap();
    assert_eq!(r.result_mode, ResultMode::Products);
    assert_eq!(r.facet_count_mode, None);
    assert!(r.in_stock_only);
    assert_eq!(r.sort_policy, SortPolicy::ShopperOverrides);
    assert!(serde_json::from_str::<Request>(r#"{"unknown":true}"#).is_err());
}

#[test]
fn availability_requires_consistent_status_and_positive_units() {
    let mut c = fixture();
    c.products[0].variants[0].available_units = 0;
    c.products[0].variants[1].available_units = 100;
    let e = Engine::new(c).unwrap();
    assert_eq!(e.search(&Request::default()).unwrap().num_found, 2);
    let with_oos = e
        .search(&Request {
            in_stock_only: false,
            ..Request::default()
        })
        .unwrap();
    assert_eq!(with_oos.num_found, 3);
}

#[test]
fn zero_limit_large_offset_empty_catalog_and_empty_lexical_hits_have_exact_totals() {
    let e = Engine::new(fixture()).unwrap();
    for (offset, limit) in [(0, 0), (usize::MAX, 20), (3, 20)] {
        let r = e
            .search(&Request {
                offset,
                limit,
                facets: vec![facet("color")],
                ..Request::default()
            })
            .unwrap();
        assert!(r.hits.is_empty());
        assert_eq!(r.num_found, 3);
        assert_eq!(r.facets[0].counts.get("red"), Some(&3));
    }
    let empty = e
        .search(&Request {
            lexical: LexicalInput::Supplied { hits: vec![] },
            facets: vec![facet("color")],
            ..Request::default()
        })
        .unwrap();
    assert_eq!(empty.num_found, 0);
    assert!(empty.facets[0].counts.is_empty());
    let empty_engine = Engine::new(CatalogInput { products: vec![] }).unwrap();
    assert_eq!(
        empty_engine.search(&Request::default()).unwrap().num_found,
        0
    );
}

#[test]
fn pin_collision_same_priority_uses_stable_rule_id_not_input_order() {
    let e = Engine::new(fixture()).unwrap();
    let a = rule("a", pt(&[3]), Action::Pin { position: 0 });
    let b = rule("b", pt(&[2]), Action::Pin { position: 0 });
    for rules in [vec![a.clone(), b.clone()], vec![b, a]] {
        let r = e
            .search(&Request {
                rules,
                ..Request::default()
            })
            .unwrap();
        assert_eq!(ids(&r), vec![(3, 31), (1, 11), (2, 21)]);
        assert_eq!(r.hits[0].pin_rule_id.as_deref(), Some("a"));
    }
}

#[test]
fn out_of_range_eligible_pin_position_is_rejected_instead_of_silently_clamped() {
    let e = Engine::new(fixture()).unwrap();
    let r = e.search(&Request {
        rules: vec![rule(
            "last",
            pt(&[1]),
            Action::Pin {
                position: usize::MAX,
            },
        )],
        ..Request::default()
    });
    assert!(r.is_err());
    let absent = e
        .search(&Request {
            rules: vec![rule(
                "absent",
                pt(&[999]),
                Action::Pin {
                    position: usize::MAX,
                },
            )],
            ..Request::default()
        })
        .unwrap();
    assert_eq!(ids(&absent), vec![(1, 11), (2, 21), (3, 31)]);
    let out_of_stock = e
        .search(&Request {
            rules: vec![rule(
                "oos",
                vt(&[12]),
                Action::Pin {
                    position: usize::MAX,
                },
            )],
            ..Request::default()
        })
        .unwrap();
    assert_eq!(ids(&out_of_stock), vec![(1, 11), (2, 21), (3, 31)]);
}

#[test]
fn duplicate_catalog_and_rule_identities_are_rejected_before_execution() {
    let mut duplicate_product = fixture();
    duplicate_product.products[1].product_id = 1;
    assert!(Engine::new(duplicate_product).is_err());
    let mut duplicate_variant = fixture();
    duplicate_variant.products[1].variants[0].variant_id = 11;
    assert!(Engine::new(duplicate_variant).is_err());
    let e = Engine::new(fixture()).unwrap();
    let dup = rule("same-id", Target::All, Action::Hide);
    assert!(e
        .search(&Request {
            rules: vec![dup.clone(), dup],
            ..Request::default()
        })
        .is_err());
}

#[test]
fn factor_order_is_irrelevant_and_opposing_factors_are_composed_before_clamping() {
    let e = Engine::new(fixture()).unwrap();
    let factors = [4.0, 4.0, 0.25, 0.25];
    let signals = |xs: &[f64]| {
        xs.iter()
            .map(|factor| PersonalSignal {
                target: pt(&[2]),
                factor: *factor,
            })
            .collect()
    };
    let a = e
        .search(&Request {
            personal_signals: signals(&factors),
            ..Request::default()
        })
        .unwrap();
    let b = e
        .search(&Request {
            personal_signals: signals(&factors.into_iter().rev().collect::<Vec<_>>()),
            ..Request::default()
        })
        .unwrap();
    assert_eq!(ids(&a), ids(&b));
    let q = a.hits.iter().find(|h| h.product_id == 2).unwrap();
    assert!((q.personal_factor - 1.0).abs() < 1e-12);
}

#[test]
fn nonfinite_typed_values_and_filters_are_rejected_including_nested_rule_targets() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut c = fixture();
        c.products[0]
            .attributes
            .insert("bad".into(), Value::Numeric(bad));
        assert!(Engine::new(c).is_err());
        let e = Engine::new(fixture()).unwrap();
        let filter = Filter::Numeric {
            attribute: "rating".into(),
            op: NumberOp::Gt,
            value: bad,
        };
        assert!(e
            .search(&Request {
                filters: vec![filter.clone()],
                ..Request::default()
            })
            .is_err());
        assert!(e
            .search(&Request {
                rules: vec![rule(
                    "invalid-target",
                    Target::Filters {
                        filters: vec![filter]
                    },
                    Action::Hide
                )],
                ..Request::default()
            })
            .is_err());
    }
}

#[test]
fn pin_absolute_position_is_validated_against_full_results_not_page_size() {
    let e = Engine::new(fixture()).unwrap();
    let r = Request {
        limit: 1,
        rules: vec![rule("last", pt(&[1]), Action::Pin { position: 2 })],
        ..Request::default()
    };
    assert_eq!(ids(&e.search(&r).unwrap()), vec![(2, 21)]);
    assert_eq!(
        ids(&e.search(&Request { offset: 2, ..r }).unwrap()),
        vec![(1, 11)]
    );
}

#[test]
fn failed_variant_pin_slot_claim_restores_organic_best_variant() {
    let e = Engine::new(fixture()).unwrap();
    let mut stronger = rule("stronger", pt(&[2]), Action::Pin { position: 0 });
    stronger.priority = 2;
    let weaker = rule("weaker", vt(&[32]), Action::Pin { position: 0 });
    let r = e
        .search(&Request {
            rules: vec![weaker, stronger],
            ..Request::default()
        })
        .unwrap();
    assert_eq!(ids(&r), vec![(2, 21), (1, 11), (3, 31)]);
    assert_eq!(r.hits[2].pin_rule_id, None);
}

#[cfg(feature = "bm25")]
mod bm25_acceptance {
    use super::*;

    #[test]
    fn bm25_uses_effective_variant_text_not_sibling_or_overridden_shared_text() {
        let mut c = fixture();
        c.products[2].attributes.insert(
            "description".into(),
            Value::Text("waterproof durable".into()),
        );
        c.products[2].variants[1]
            .attributes
            .insert("description".into(), Value::Text("lightweight blue".into()));
        let e = Engine::new_with_bm25(c).unwrap();
        let search = |q: &str| {
            e.search(&Request {
                q: q.into(),
                lexical: LexicalInput::Bm25,
                result_mode: ResultMode::Variants,
                ..Request::default()
            })
            .unwrap()
        };
        assert_eq!(ids(&search("waterproof")), vec![(3, 31)]);
        assert_eq!(ids(&search("lightweight")), vec![(3, 32)]);
        assert_eq!(ids(&search("waterproof lightweight")), vec![]);
        assert_eq!(search("shoes").num_found, 4);
    }

    #[test]
    fn bm25_round_trips_full_width_identifiers_without_u32_narrowing() {
        let pid = u64::from(u32::MAX) + 100;
        let vid = u64::MAX - 3;
        let e = Engine::new_with_bm25(CatalogInput {
            products: vec![product(pid, vec![variant(vid, "red", "L", true, 100)])],
        })
        .unwrap();
        let r = e
            .search(&Request {
                q: "shoes".into(),
                lexical: LexicalInput::Bm25,
                ..Request::default()
            })
            .unwrap();
        assert_eq!(ids(&r), vec![(pid, vid)]);
        let trace = e
            .lexical_trace(&Request {
                q: "shoes".into(),
                lexical: LexicalInput::Bm25,
                ..Request::default()
            })
            .unwrap();
        assert_eq!(trace[0].product_id, pid);
        assert_eq!(trace[0].variant_id, vid);
    }

    #[test]
    fn bm25_parser_errors_are_not_silently_converted_to_empty_success() {
        let e = Engine::new_with_bm25(fixture()).unwrap();
        assert!(e
            .search(&Request {
                q: "\"unterminated".into(),
                lexical: LexicalInput::Bm25,
                ..Request::default()
            })
            .is_err());
        let plain = Engine::new(fixture()).unwrap();
        assert!(plain
            .search(&Request {
                q: "shoes".into(),
                lexical: LexicalInput::Bm25,
                ..Request::default()
            })
            .is_err());
    }

    #[test]
    fn bm25_lexical_universe_is_not_narrowed_before_self_excluding_facets() {
        let e = Engine::new_with_bm25(fixture()).unwrap();
        for lexical in [LexicalInput::Bm25, LexicalInput::Bm25Literal] {
            let r = e
                .search(&Request {
                    q: "shoes".into(),
                    lexical,
                    filters: vec![ef("color", "red")],
                    facets: vec![facet("color")],
                    ..Request::default()
                })
                .unwrap();
            assert_eq!(r.num_found, 3);
            assert_eq!(
                r.facets[0].counts,
                BTreeMap::from([("blue".into(), 1), ("red".into(), 3)])
            );
            assert!(r
                .hits
                .iter()
                .all(|h| [11, 21, 31].contains(&h.selected_variant_id)));
        }
    }

    #[test]
    fn bm25_complete_candidate_set_allows_rank_100_merch_promotion_before_top_k() {
        let c = CatalogInput {
            products: (1..=100)
                .map(|i| product(i, vec![variant(i, "red", "L", true, 100)]))
                .collect(),
        };
        let e = Engine::new_with_bm25(c).unwrap();
        for lexical in [LexicalInput::Bm25, LexicalInput::Bm25Literal] {
            let req = Request {
                q: "shoes".into(),
                lexical,
                limit: 100,
                ..Request::default()
            };
            assert_eq!(e.search(&req).unwrap().hits[99].product_id, 100);
            let r = e
                .search(&Request {
                    limit: 1,
                    rules: vec![rule(
                        "global-100",
                        pt(&[100]),
                        Action::Group { order: 0, rank: 1 },
                    )],
                    ..req
                })
                .unwrap();
            assert_eq!(r.hits[0].product_id, 100);
            assert_eq!(r.metrics.lexical_candidates, 100);
            assert_eq!(r.metrics.ranked_variants, 100);
        }
    }

    #[test]
    fn bm25_trace_replay_preserves_lexical_scores_and_composite_results_exactly() {
        let mut c = fixture();
        c.products[0].title = "running shoes running running".into();
        c.products[1].title = "running shoes".into();
        c.products[2].title =
            "very comfortable running shoes for every day outdoor adventures".into();
        let e = Engine::new_with_bm25(c).unwrap();
        let req = Request {
            q: "running shoes".into(),
            lexical: LexicalInput::Bm25,
            facets: vec![facet("color")],
            personal_signals: vec![PersonalSignal {
                target: pt(&[2]),
                factor: 2.0,
            }],
            rules: vec![rule(
                "r-group",
                pt(&[3]),
                Action::Group { order: 0, rank: 1 },
            )],
            ..Request::default()
        };
        let trace = e.lexical_trace(&req).unwrap();
        assert_eq!(trace.len(), 5); // lexical trace deliberately precedes stock filter.
        assert!(trace.iter().all(|h| h.score.is_finite() && h.score >= 0.0));
        let actual = e.search(&req).unwrap();
        let replay = e
            .search(&Request {
                lexical: LexicalInput::Supplied {
                    hits: trace.clone(),
                },
                ..req
            })
            .unwrap();
        assert_eq!(actual.hits, replay.hits);
        assert_eq!(actual.facets, replay.facets);
        assert_eq!(actual.num_found, replay.num_found);
        for h in actual.hits {
            let source = trace
                .iter()
                .find(|x| x.variant_id == h.selected_variant_id)
                .unwrap();
            assert_eq!(h.lexical_score, source.score);
            assert_eq!(h.composed_score, source.score * h.personal_factor);
        }
    }
}

#[test]
fn signed_zero_lexical_scores_tie_by_stable_ids_not_float_sign_bit() {
    let e = Engine::new(fixture()).unwrap();
    let r = e
        .search(&Request {
            lexical: LexicalInput::Supplied {
                hits: vec![
                    LexicalHit {
                        product_id: 2,
                        variant_id: 21,
                        score: 0.0,
                        relevance_bucket: 0,
                    },
                    LexicalHit {
                        product_id: 1,
                        variant_id: 11,
                        score: -0.0,
                        relevance_bucket: 0,
                    },
                ],
            },
            ..Request::default()
        })
        .unwrap();
    assert_eq!(ids(&r), vec![(1, 11), (2, 21)]);
    assert!(r
        .hits
        .iter()
        .all(|h| h.lexical_score.to_bits() == 0.0_f64.to_bits()));
}

#[test]
fn numeric_facet_canonicalizes_signed_zero_and_omits_missing_attributes() {
    let mut c = fixture();
    c.products[0]
        .attributes
        .insert("rating".into(), Value::Numeric(-0.0));
    c.products[1]
        .attributes
        .insert("rating".into(), Value::Numeric(0.0));
    let e = Engine::new(c).unwrap();
    for mode in [ResultMode::Products, ResultMode::Variants] {
        let r = e
            .search(&Request {
                facets: vec![facet("rating")],
                result_mode: mode,
                ..Request::default()
            })
            .unwrap();
        assert_eq!(r.facets[0].counts, BTreeMap::from([("0".into(), 2)]));
        assert!(!r.facets[0].counts.contains_key("-0"));
    }
    let zero_filter = e
        .search(&Request {
            filters: vec![Filter::Numeric {
                attribute: "rating".into(),
                op: NumberOp::Eq,
                value: 0.0,
            }],
            ..Request::default()
        })
        .unwrap();
    assert_eq!(ids(&zero_filter), vec![(1, 11), (2, 21)]);
}

#[test]
fn neutral_three_and_ten_inverse_factor_pairs_do_not_create_ranking_drift() {
    let e = Engine::new(fixture()).unwrap();
    for pairs in [3, 10] {
        let mut grouped = vec![0.25; pairs];
        grouped.extend(vec![4.0; pairs]);
        let reversed: Vec<_> = grouped.iter().rev().copied().collect();
        let alternating: Vec<_> = (0..pairs).flat_map(|_| [0.25, 4.0]).collect();
        for factors in [grouped, reversed, alternating] {
            let r = e
                .search(&Request {
                    personal_signals: factors
                        .into_iter()
                        .map(|factor| PersonalSignal {
                            target: pt(&[2]),
                            factor,
                        })
                        .collect(),
                    ..Request::default()
                })
                .unwrap();
            assert_eq!(
                ids(&r),
                vec![(1, 11), (2, 21), (3, 31)],
                "neutral {pairs} factor pairs changed order"
            );
            let q = r.hits.iter().find(|h| h.product_id == 2).unwrap();
            assert_eq!(
                q.personal_factor, 1.0,
                "neutral factors must be exactly neutral after documented precision quantization"
            );
            assert_eq!(q.composed_score, q.lexical_score);
        }
    }
}

#[cfg(feature = "bm25")]
#[test]
fn bm25_huge_query_boost_is_rejected_or_remains_losslessly_replayable_within_score_cap() {
    let e = Engine::new_with_bm25(fixture()).unwrap();
    let r = Request {
        q: "shoes^1000000000000000".into(),
        lexical: LexicalInput::Bm25,
        ..Request::default()
    };
    match e.lexical_trace(&r) {
        Err(error) => assert!(
            error.to_string().to_lowercase().contains("score"),
            "expected explicit score-domain error, got {error}"
        ),
        Ok(trace) => {
            assert!(
                !trace.is_empty(),
                "a large boost must not silently erase matching results"
            );
            assert!(trace
                .iter()
                .all(|h| h.score.is_finite() && (0.0..=1.0e12).contains(&h.score)));
            let original = e.search(&r).unwrap();
            let replay = e
                .search(&Request {
                    lexical: LexicalInput::Supplied { hits: trace },
                    ..r
                })
                .unwrap();
            assert_eq!(original.hits, replay.hits);
            assert!(original.hits.iter().any(|h| h.lexical_score > 0.0));
        }
    }
}

#[test]
fn json_float_ingestion_and_serialized_replay_preserve_full_f64_bits() {
    const EXPECTED: u64 = 0x3ee4a3eb2681c146;
    let value: Value =
        serde_json::from_str(r#"{"type":"numeric","value":9.842065267857133e-06}"#).unwrap();
    let Value::Numeric(numeric) = value else {
        panic!("expected numeric")
    };
    assert_eq!(numeric.to_bits(), EXPECTED);
    let request: Request = serde_json::from_str(r#"{
        "filters":[{"type":"numeric","attribute":"weight","op":"eq","value":9.842065267857133e-06}],
        "lexical":{"profile":"supplied","hits":[{"product_id":1,"variant_id":11,"score":9.842065267857133e-06,"relevance_bucket":0}]},
        "personal_signals":[{"target":{"type":"all"},"factor":1.0000000000000002}]
    }"#).unwrap();
    let replay: Request = serde_json::from_str(&serde_json::to_string(&request).unwrap()).unwrap();
    for r in [request, replay] {
        let Filter::Numeric { value, .. } = r.filters[0] else {
            panic!("expected numeric filter")
        };
        assert_eq!(value.to_bits(), EXPECTED);
        let LexicalInput::Supplied { hits } = r.lexical else {
            panic!("expected supplied lexical")
        };
        assert_eq!(hits[0].score.to_bits(), EXPECTED);
        assert_eq!(r.personal_signals[0].factor.to_bits(), 0x3ff0000000000001);
    }
}

#[test]
fn conditional_hide_scope_controls_eligibility_and_remains_applied_after_facet_self_exclusion() {
    let e = Engine::new(fixture()).unwrap();
    let mut inactive_query = rule("inactive-query", pt(&[1]), Action::Hide);
    inactive_query.scope = Scope::Query {
        query: "boots".into(),
    };
    let mut conditional_category = rule("category-vip", vt(&[32]), Action::Hide);
    conditional_category.scope = Scope::Category { category_id: 10 };
    conditional_category.conditions = vec![
        Condition::ContextTag { tag: "vip".into() },
        Condition::QueryContainsAll {
            tokens: vec!["shoes".into()],
        },
    ];
    let base = Request {
        q: "shoes".into(),
        filters: vec![ef("color", "red")],
        facets: vec![facet("color")],
        rules: vec![inactive_query, conditional_category],
        ..Request::default()
    };
    for (category_id, tags, active) in [
        (Some(10), vec![], false),
        (Some(11), vec!["vip".into()], false),
        (Some(10), vec!["vip".into()], true),
    ] {
        let r = e
            .search(&Request {
                context: Context { category_id, tags },
                ..base.clone()
            })
            .unwrap();
        assert_eq!(
            ids(&r),
            vec![(1, 11), (2, 21), (3, 31)],
            "inactive query hide must not remove P"
        );
        assert_eq!(r.num_found, 3);
        let expected = if active {
            BTreeMap::from([("red".into(), 3)])
        } else {
            BTreeMap::from([("blue".into(), 1), ("red".into(), 3)])
        };
        assert_eq!(
            r.facets[0].counts, expected,
            "self-excluding color filter must not remove active hide"
        );
        assert_eq!(
            r.active_rule_ids,
            if active {
                vec!["category-vip".to_string()]
            } else {
                vec![]
            }
        );
    }
}

#[cfg(feature = "bm25")]
mod bm25_literal_acceptance {
    use super::*;

    fn catalog(titles: &[&str]) -> CatalogInput {
        CatalogInput {
            products: titles
                .iter()
                .enumerate()
                .map(|(index, title)| {
                    let id = index as u64 + 1;
                    let mut p = product(id, vec![variant(id + 100, "red", "L", true, 100)]);
                    p.title = (*title).into();
                    p
                })
                .collect(),
        }
    }
    fn request(q: &str) -> Request {
        Request {
            q: q.into(),
            lexical: LexicalInput::Bm25Literal,
            ..Request::default()
        }
    }
    fn trace(e: &Engine, q: &str) -> Vec<(u64, u64, u64, i32)> {
        e.lexical_trace(&request(q))
            .unwrap()
            .iter()
            .map(|h| {
                (
                    h.product_id,
                    h.variant_id,
                    h.score.to_bits(),
                    h.relevance_bucket,
                )
            })
            .collect()
    }
    fn found(e: &Engine, q: &str) -> BTreeSet<u64> {
        e.search(&request(q))
            .unwrap()
            .hits
            .iter()
            .map(|h| h.product_id)
            .collect()
    }

    #[test]
    fn legacy_naive_tsv_strings_remain_supported_as_robustness_inputs() {
        let e = Engine::new_with_bm25(catalog(&[
            "fawkes 36 blue vanity",
            "fawkes blue vanity",
            "fawkes 36 blue",
            "48 sliding single track barn door for laundry",
            "sliding single track barn door for laundry",
            "writing desk 48",
            "writing desk",
        ]))
        .unwrap();
        // Historical fixture is retained byte-for-byte. These are naive-TSV
        // serialized field strings, NOT official decoded shopper query values.
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/wands_raw_query_regressions.json"))
                .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let raw = case["q"].as_str().unwrap();
            let normalized = case["normalized"].as_str().unwrap();
            let pid = case["expected_product_id"].as_u64().unwrap();
            assert_eq!(
                found(&e, raw),
                BTreeSet::from([pid]),
                "legacy naive-TSV robustness input {raw:?}"
            );
            assert_eq!(
                trace(&e, raw),
                trace(&e, normalized),
                "punctuation must not alter analyzed-token scores"
            );
            let response = e.search(&request(raw)).unwrap();
            assert!(response.lexical_profile.contains("literal"));
            if pid == 1 {
                assert!(
                    e.search(&Request {
                        lexical: LexicalInput::Bm25,
                        ..request(raw)
                    })
                    .is_err(),
                    "the legacy naive-TSV208 artifact must still fail the strict DSL profile"
                );
            }
        }
    }

    #[test]
    fn official_csv_decoded_query_values_preserve_inch_quotes_without_transport_escaping() {
        let e = Engine::new_with_bm25(catalog(&[
            "fawkes 36 blue vanity",
            "fawkes blue vanity",
            "fawkes 36 blue",
            "48 sliding single track barn door for laundry",
            "sliding single track barn door for laundry",
            "writing desk 48",
            "writing desk",
        ]))
        .unwrap();
        let official: serde_json::Value = serde_json::from_str(include_str!(
            "fixtures/wands_official_decoded_query_regressions.json"
        ))
        .unwrap();
        let legacy: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/wands_raw_query_regressions.json"))
                .unwrap();
        for (case, historical) in official["cases"]
            .as_array()
            .unwrap()
            .iter()
            .zip(legacy["cases"].as_array().unwrap())
        {
            let q = case["q"].as_str().unwrap();
            let normalized = case["normalized"].as_str().unwrap();
            let expected = case["expected_product_id"].as_u64().unwrap();
            assert_ne!(
                q,
                historical["q"].as_str().unwrap(),
                "decoded value must not retain TSV wrapper/doubled quote serialization"
            );
            assert!(!q.starts_with('"'));
            assert_eq!(
                q.matches('"').count(),
                1,
                "one genuine inch-quote remains in each official value"
            );
            assert_eq!(found(&e, q), BTreeSet::from([expected]));
            assert_eq!(trace(&e, q), trace(&e, normalized));
            assert_eq!(
                request(q).q,
                q,
                "transport decoding must not become engine-side query rewriting"
            );
        }
    }

    #[test]
    fn literal_operator_keywords_and_field_syntax_are_required_terms_not_query_dsl() {
        let e = Engine::new_with_bm25(catalog(&[
            "red blue",
            "red or blue",
            "title shoes",
            "shoes",
            "the and shoes",
            "and shoes",
            "title shoes 10",
        ]))
        .unwrap();
        for (q, expected) in [
            ("red OR blue", vec![2]),
            ("title:shoes", vec![3, 7]),
            ("title:shoes^10", vec![7]),
            ("[red]", vec![1, 2]),
            ("shoes AND", vec![5, 6]),
            ("the", vec![5]),
            ("NOT shoes", vec![]),
        ] {
            assert_eq!(
                found(&e, q),
                expected.into_iter().collect(),
                "literal query {q:?}"
            );
        }
        assert_eq!(trace(&e, "title:shoes"), trace(&e, "title shoes"));
        assert_eq!(trace(&e, "red OR blue"), trace(&e, "red or blue"));
    }

    #[test]
    fn literal_apostrophes_and_unicode_use_the_index_default_analyzer() {
        let e = Engine::new_with_bm25(catalog(&[
            "kid s chair",
            "kids chair",
            "café 東京",
            "cafe 東京",
            "café Kyoto",
        ]))
        .unwrap();
        assert_eq!(found(&e, "kid's chair"), BTreeSet::from([1]));
        assert_eq!(trace(&e, "kid's chair"), trace(&e, "kid s chair"));
        assert_eq!(found(&e, "CAFÉ 東京"), BTreeSet::from([3]));
        assert_eq!(trace(&e, "CAFÉ—東京"), trace(&e, "café 東京"));
    }

    #[test]
    fn literal_repeated_tokens_are_deduplicated_without_score_inflation() {
        let e =
            Engine::new_with_bm25(catalog(&["red blue", "red red blue", "red", "blue"])).unwrap();
        assert_eq!(found(&e, "red red blue"), BTreeSet::from([1, 2]));
        assert_eq!(trace(&e, "RED red blue BLUE red"), trace(&e, "red blue"));
        let result = e.search(&request("red red blue")).unwrap();
        let supplied = e.lexical_trace(&request("red red blue")).unwrap();
        let replay = e
            .search(&Request {
                lexical: LexicalInput::Supplied { hits: supplied },
                ..request("red red blue")
            })
            .unwrap();
        assert_eq!(result.hits, replay.hits);
        assert!(result.contract.contains("v1.1"));
        assert!(result.lexical_profile.contains("literal"));
    }

    #[test]
    fn literal_blank_is_browse_but_nonblank_without_analyzed_tokens_is_empty() {
        let e = Engine::new_with_bm25(catalog(&["shoes", "boots"])).unwrap();
        for q in ["", " \t\n "] {
            assert_eq!(found(&e, q), BTreeSet::from([1, 2]));
        }
        for q in ["!!!", "\"\"\"", "[] : ^ ()", "—…", "*"] {
            assert!(
                found(&e, q).is_empty(),
                "nonblank punctuation-only input {q:?} must not become browse"
            );
            assert!(trace(&e, q).is_empty());
        }
        assert!(found(&e, &"a".repeat(40)).is_empty());
    }

    #[test]
    fn literal_long_token_cutoff_uses_same_strict_40_byte_boundary_as_index() {
        let short_ascii = "a".repeat(39);
        let long_ascii = "a".repeat(40);
        let short_unicode = "é".repeat(19);
        let long_unicode = "é".repeat(20);
        let e = Engine::new_with_bm25(catalog(&[
            &short_ascii,
            &long_ascii,
            &short_unicode,
            &long_unicode,
            "shoes",
        ]))
        .unwrap();
        assert_eq!(found(&e, &short_ascii), BTreeSet::from([1]));
        assert!(found(&e, &long_ascii).is_empty());
        assert_eq!(found(&e, &short_unicode), BTreeSet::from([3]));
        assert!(found(&e, &long_unicode).is_empty());
        assert_eq!(
            trace(&e, &format!("{long_ascii} shoes")),
            trace(&e, "shoes")
        );
    }

    #[test]
    fn literal_terms_can_span_title_and_effective_variant_text_but_not_siblings() {
        let mut c = catalog(&["blue", "blue", "red"]);
        c.products[0]
            .attributes
            .insert("description".into(), Value::Text("vanity".into()));
        c.products[0]
            .variants
            .push(variant(999, "blue", "S", true, 100));
        c.products[0].variants[1]
            .attributes
            .insert("description".into(), Value::Text("cabinet".into()));
        let e = Engine::new_with_bm25(c).unwrap();
        let r = e
            .search(&Request {
                result_mode: ResultMode::Variants,
                ..request("blue vanity")
            })
            .unwrap();
        assert_eq!(ids(&r), vec![(1, 101)]);
        assert_eq!(
            ids(&e
                .search(&Request {
                    result_mode: ResultMode::Variants,
                    ..request("blue cabinet")
                })
                .unwrap()),
            vec![(1, 999)]
        );
        assert!(found(&e, "vanity cabinet").is_empty());
    }
}

#[test]
fn literal_profile_json_is_explicit_and_preserves_raw_query_text() {
    let r: Request = serde_json::from_str(
        r#"{"q":"fawkes 36\"\" blue vanity","lexical":{"profile":"bm25_literal"}}"#,
    )
    .unwrap();
    assert!(matches!(r.lexical, LexicalInput::Bm25Literal));
    assert_eq!(r.q, "fawkes 36\"\" blue vanity");
    assert!(matches!(
        Request::default().lexical,
        LexicalInput::ExactTokenAnd
    ));
}
