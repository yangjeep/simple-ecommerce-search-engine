use crate::schema::*;
use commerce_core::domain::{
    effective_attributes, AttributeValue, Availability, Catalog, Product, Variant, VariantId,
};
use commerce_core::index::{tokenize, CatalogIndex};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Instant;

pub const CONTRACT: &str = "athos80-experimental-v1.1";
pub const MIN_PERSONAL_FACTOR: f64 = 0.25;
pub const MAX_PERSONAL_FACTOR: f64 = 4.0;
pub const MAX_LEXICAL_SCORE: f64 = 1.0e12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;
fn ensure(ok: bool, message: impl Into<String>) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error(message.into()))
    }
}

/// Owns a validated immutable catalog and its index, preventing stale-index use.
#[derive(Debug)]
pub struct Engine {
    catalog: Catalog,
    index: CatalogIndex,
    pub build_micros: u128,
    #[cfg(feature = "bm25")]
    bm25: Option<crate::bm25::Bm25Index>,
}
#[derive(Debug, Clone)]
struct Ranked {
    hit: Hit,
    pin: Option<Pin>,
}
#[derive(Debug, Clone)]
struct Pin {
    position: usize,
    rule_id: String,
    specificity: u8,
    priority: i32,
    exact_variant: Option<u64>,
}
impl Engine {
    pub fn new(input: CatalogInput) -> Result<Self> {
        let start = Instant::now();
        validate_catalog(&input)?;
        let catalog = input.native();
        let index = CatalogIndex::build(&catalog);
        Ok(Self {
            catalog,
            index,
            build_micros: start.elapsed().as_micros(),
            #[cfg(feature = "bm25")]
            bm25: None,
        })
    }
    /// Optional separate build cost. Normal new() never builds this index.
    #[cfg(feature = "bm25")]
    pub fn new_with_bm25(input: CatalogInput) -> Result<Self> {
        let start = Instant::now();
        let mut engine = Self::new(input)?;
        engine.bm25 = Some(crate::bm25::Bm25Index::build(&engine.catalog)?);
        engine.build_micros = start.elapsed().as_micros();
        Ok(engine)
    }
    /// Full pre-filter lexical trace, for public/synthetic offline validation.
    /// Includes every candidate; never call for uncontrolled/public traffic.
    pub fn lexical_trace(&self, request: &Request) -> Result<Vec<LexicalHit>> {
        validate_request(request)?;
        Ok(self
            .lexical(request)?
            .into_iter()
            .map(|(variant_id, (score, relevance_bucket))| {
                let (product, _) = self
                    .index
                    .lookup_variant(&self.catalog, VariantId(variant_id))
                    .expect("validated lexical ID");
                LexicalHit {
                    product_id: product.id.0,
                    variant_id,
                    score,
                    relevance_bucket,
                }
            })
            .collect())
    }
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }
    pub fn search(&self, request: &Request) -> Result<Response> {
        let start = Instant::now();
        validate_request(request)?;
        let mut metrics = Metrics {
            catalog_variants: self.index.ordinal_count(),
            ..Metrics::default()
        };
        let mut diagnostics =
            vec!["Experimental contract; no Athos coverage or cost parity claim.".into()];
        let active: Vec<&Rule> = request
            .rules
            .iter()
            .filter(|r| {
                scope_matches(&r.scope, request)
                    && r.conditions.iter().all(|c| condition_matches(c, request))
            })
            .collect();
        // Whole set override: one scope wins for grouping, NOT additive campaigns.
        let group_scope = active
            .iter()
            .filter(|r| matches!(r.action, Action::Group { .. }))
            .map(|r| specificity(&r.scope))
            .max();
        let group_rules: Vec<&Rule> = active
            .iter()
            .copied()
            .filter(|r| {
                matches!(r.action, Action::Group { .. })
                    && Some(specificity(&r.scope)) == group_scope
            })
            .collect();
        let axes: Vec<u32> = group_rules
            .iter()
            .filter_map(|r| match r.action {
                Action::Group { order, .. } => Some(order),
                _ => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let shopper_override =
            request.sort != Sort::Relevance && request.sort_policy == SortPolicy::ShopperOverrides;
        if shopper_override {
            diagnostics.push("Explicit shopper sort: pins, merchant groups, and personal ranking disabled; hard filters and hides retained.".into());
        }
        if request.personal_signals.is_empty() {
            diagnostics.push(
                "No supplied personal factors: neutral factor 1; no model or network call.".into(),
            );
        }
        if let Some(scope) = group_scope {
            diagnostics.push(format!("Merchant group set winning scope specificity={scope} (global=0/category=1/query=2); axes={axes:?}."));
        }
        if matches!(
            request.lexical,
            LexicalInput::Bm25 | LexicalInput::Bm25Literal
        ) {
            diagnostics.push("Variant-document Tantivy 0.22 BM25: default TEXT analyzer; bucket=0 for every hit, so merchandising can outrank any raw BM25 score. NOT Athos relevance-tier parity; prior product-document NDCG does not transfer.".into());
        }
        if matches!(request.lexical, LexicalInput::Bm25Literal) {
            diagnostics.push("Literal storefront query: same index analyzer, unique normalized tokens AND across title/text; punctuation is not query DSL; blank browses, nonblank zero-token input has zero hits. Raw q is not rewritten.".into());
        } else if matches!(request.lexical, LexicalInput::Bm25) {
            diagnostics.push("Strict diagnostic QueryParser profile: AND default; explicit query DSL interpreted; syntax errors fail, without literal fallback.".into());
        }
        if matches!(request.lexical, LexicalInput::Supplied { .. }) {
            diagnostics.push("Supplied hits define the complete lexical universe for this replay; completeness/recall cannot be verified by the engine.".into());
        }
        let lexical = self.lexical(request)?;
        metrics.lexical_candidates = lexical.len();
        let hard_query = query(&request.filters);
        let candidates = self.index.indexed_candidates(&hard_query.constraints);
        metrics.structural_candidates = candidates.len() as usize;
        let mut ranked = Vec::new();
        for ordinal in candidates {
            let id = self
                .index
                .variant_id_at(ordinal)
                .expect("validated index ordinal");
            let Some(&(score, bucket)) = lexical.get(&id.0) else {
                continue;
            };
            let (product, variant) = self
                .index
                .lookup_variant(&self.catalog, id)
                .expect("validated index location");
            metrics.verified_variants += 1;
            // Reverify every candidate, including supplied hits, using CommerceQuery.
            if !hard_query.matches_variant(product, variant)
                || !available(request, variant)
                || hidden(&active, product, variant)
            {
                continue;
            }
            let mut groups = Vec::new();
            let mut group_ids = Vec::new();
            if !shopper_override {
                for order in &axes {
                    let winning = group_rules
                        .iter()
                        .copied()
                        .filter(|r| {
                            matches!(r.action, Action::Group { order: o, .. } if o == *order)
                                && target_matches(&r.target, product, variant)
                        })
                        .min_by(|a, b| rule_cmp(a, b));
                    groups.push(winning.map_or(0, |r| match r.action {
                        Action::Group { rank, .. } => rank,
                        _ => unreachable!(),
                    }));
                    group_ids.push(winning.map(|r| r.id.clone()));
                }
            }
            // Multiply in canonical numeric order, then clamp ONCE. Per-step
            // clamping would make a request's factor order change its answer.
            let mut factors: Vec<f64> = if shopper_override {
                vec![]
            } else {
                request
                    .personal_signals
                    .iter()
                    .filter(|s| target_matches(&s.target, product, variant))
                    .map(|s| s.factor)
                    .collect()
            };
            factors.sort_by(f64::total_cmp);
            // Compensated log accumulation, followed by explicitly frozen
            // 1e-12 factor quantization, prevents floating cancellation residue
            // (e.g. three .25 factors and three 4 factors) from changing ID ties.
            let mut sum = 0.0_f64;
            let mut correction = 0.0_f64;
            for value in factors.iter().map(|v| v.ln()) {
                let next = sum + value;
                correction += if sum.abs() >= value.abs() {
                    (sum - next) + value
                } else {
                    (value - next) + sum
                };
                sum = next;
            }
            let raw_factor = (sum + correction)
                .clamp(MIN_PERSONAL_FACTOR.ln(), MAX_PERSONAL_FACTOR.ln())
                .exp();
            let factor = ((raw_factor * 1e12).round() / 1e12)
                .clamp(MIN_PERSONAL_FACTOR, MAX_PERSONAL_FACTOR);
            let pin = if shopper_override {
                None
            } else {
                active
                    .iter()
                    .copied()
                    .filter(|r| {
                        matches!(r.action, Action::Pin { .. })
                            && target_matches(&r.target, product, variant)
                    })
                    .min_by(|a, b| rule_cmp(a, b))
                    .map(|r| pin_from_rule(r, variant.id.0))
            };
            ranked.push(Ranked {
                hit: Hit {
                    product_id: product.id.0,
                    selected_variant_id: variant.id.0,
                    matching_variant_count: 1,
                    lexical_score: score,
                    relevance_bucket: bucket,
                    merch_groups: groups,
                    personal_factor: factor,
                    composed_score: score * factor,
                    price_cents: variant.price.amount_cents,
                    group_rule_ids: group_ids,
                    pin_rule_id: None,
                },
                pin,
            });
        }
        metrics.eligible_variants = ranked.len();
        metrics.ranked_variants = ranked.len();
        metrics.eligible_products = ranked
            .iter()
            .map(|r| r.hit.product_id)
            .collect::<BTreeSet<_>>()
            .len();
        ranked.sort_by(|a, b| hit_cmp(&a.hit, &b.hit, request));
        // Preserve exact eligible display variants for successful variant pins.
        let eligible_hits: BTreeMap<u64, Hit> = ranked
            .iter()
            .map(|r| (r.hit.selected_variant_id, r.hit.clone()))
            .collect();
        // All eligible variants ranked before grouping. Product pins preserve
        // the comparator best variant; successful explicit variant pins display
        // their exact eligible variant, applied only AFTER slot conflict resolution.
        if request.result_mode == ResultMode::Products {
            let mut groups: BTreeMap<u64, Ranked> = BTreeMap::new();
            for item in ranked {
                match groups.entry(item.hit.product_id) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(item);
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        let best = entry.get_mut();
                        best.hit.matching_variant_count += 1;
                        if let Some(pin) = item.pin {
                            if best
                                .pin
                                .as_ref()
                                .is_none_or(|old| pin_cmp(&pin, old) == Ordering::Less)
                            {
                                best.pin = Some(pin);
                            }
                        }
                    }
                }
            }
            ranked = groups.into_values().collect();
            ranked.sort_by(|a, b| hit_cmp(&a.hit, &b.hit, request));
        }
        let num_found = ranked.len();
        let ordered = apply_pins(ranked, &eligible_hits, &mut diagnostics)?;
        let count_mode = request.facet_count_mode.unwrap_or(request.result_mode);
        let facets = self.facets(request, &active, &lexical, count_mode, &mut metrics);
        let hits = ordered
            .into_iter()
            .skip(request.offset)
            .take(request.limit)
            .map(|r| r.hit)
            .collect();
        metrics.elapsed_micros = start.elapsed().as_micros();
        Ok(Response {
            contract: CONTRACT.into(),
            lexical_profile: match request.lexical {
                LexicalInput::ExactTokenAnd => "exact_token_and_no_bm25",
                LexicalInput::Bm25 => "tantivy_0_22_variant_bm25_strict_parser_and_bucket0",
                LexicalInput::Bm25Literal => "tantivy_0_22_variant_bm25_literal_and_bucket0_v1",
                LexicalInput::Supplied { .. } => "supplied_fixture_hits_unverified_completeness",
            }
            .into(),
            result_mode: request.result_mode,
            facet_count_mode: count_mode,
            num_found,
            hits,
            facets,
            active_rule_ids: active
                .iter()
                .map(|r| r.id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            diagnostics,
            metrics,
        })
    }
    fn lexical(&self, request: &Request) -> Result<BTreeMap<u64, (f64, i32)>> {
        match &request.lexical {
            LexicalInput::Bm25 | LexicalInput::Bm25Literal => {
                #[cfg(feature = "bm25")]
                {
                    let index = self.bm25.as_ref().ok_or_else(|| {
                        Error("BM25 index not built: use new_with_bm25 / server --bm25".into())
                    })?;
                    if matches!(request.lexical, LexicalInput::Bm25Literal) {
                        index.search_literal(&request.q)
                    } else {
                        index.search(&request.q)
                    }
                }
                #[cfg(not(feature = "bm25"))]
                {
                    Err(Error(
                        "BM25 feature not enabled; rebuild with --features bm25".into(),
                    ))
                }
            }
            LexicalInput::ExactTokenAnd => {
                let tokens: Vec<String> = tokenize(&request.q).collect();
                let ordinals = if tokens.is_empty() {
                    self.index.indexed_candidates(&[])
                } else {
                    self.index.lexical_and_candidates(&tokens)
                };
                Ok(ordinals
                    .into_iter()
                    .map(|o| {
                        (
                            self.index.variant_id_at(o).expect("valid ordinal").0,
                            (1.0, 0),
                        )
                    })
                    .collect())
            }
            LexicalInput::Supplied { hits } => {
                let mut result = BTreeMap::new();
                for hit in hits {
                    ensure(
                        hit.score.is_finite() && (0.0..=MAX_LEXICAL_SCORE).contains(&hit.score),
                        "supplied lexical score must be finite in [0, 1e12]",
                    )?;
                    let (product, _) = self
                        .index
                        .lookup_variant(&self.catalog, VariantId(hit.variant_id))
                        .ok_or_else(|| {
                            Error(format!("unknown supplied variant {}", hit.variant_id))
                        })?;
                    ensure(
                        product.id.0 == hit.product_id,
                        "supplied product/variant ID mismatch",
                    )?;
                    ensure(
                        result
                            .insert(
                                hit.variant_id,
                                (
                                    if hit.score == 0.0 { 0.0 } else { hit.score },
                                    hit.relevance_bucket,
                                ),
                            )
                            .is_none(),
                        "duplicate supplied variant hit",
                    )?;
                }
                Ok(result)
            }
        }
    }
    fn facets(
        &self,
        request: &Request,
        active: &[&Rule],
        lexical: &BTreeMap<u64, (f64, i32)>,
        mode: ResultMode,
        metrics: &mut Metrics,
    ) -> Vec<FacetResult> {
        request
            .facets
            .iter()
            .map(|facet| {
                let filters = request
                    .filters
                    .iter()
                    .filter(|f| f.field() != facet.field)
                    .cloned()
                    .collect::<Vec<_>>();
                let hard = query(&filters);
                let mut buckets: BTreeMap<String, BTreeSet<u64>> = BTreeMap::new();
                for ordinal in self.index.indexed_candidates(&hard.constraints) {
                    let id = self.index.variant_id_at(ordinal).expect("valid ordinal");
                    if !lexical.contains_key(&id.0) {
                        continue;
                    }
                    let (product, variant) = self
                        .index
                        .lookup_variant(&self.catalog, id)
                        .expect("valid location");
                    metrics.facet_verified_variants += 1;
                    if !hard.matches_variant(product, variant)
                        || !available(request, variant)
                        || hidden(active, product, variant)
                    {
                        continue;
                    }
                    for value in facet_values(&facet.field, product, variant) {
                        buckets.entry(value).or_default().insert(match mode {
                            ResultMode::Products => product.id.0,
                            ResultMode::Variants => variant.id.0,
                        });
                    }
                }
                FacetResult {
                    name: facet.name.clone(),
                    counts: buckets.into_iter().map(|(k, ids)| (k, ids.len())).collect(),
                }
            })
            .collect()
    }
}
fn available(request: &Request, variant: &Variant) -> bool {
    !request.in_stock_only
        || (variant.inventory.status == Availability::InStock
            && variant.inventory.available_units > 0)
}
fn normalized(q: &str) -> String {
    tokenize(q).collect::<Vec<_>>().join(" ")
}
fn specificity(scope: &Scope) -> u8 {
    match scope {
        Scope::Global => 0,
        Scope::Category { .. } => 1,
        Scope::Query { .. } => 2,
    }
}
fn scope_matches(scope: &Scope, request: &Request) -> bool {
    match scope {
        Scope::Global => true,
        Scope::Category { category_id } => request.context.category_id == Some(*category_id),
        Scope::Query { query } => normalized(&request.q) == normalized(query),
    }
}
fn condition_matches(condition: &Condition, request: &Request) -> bool {
    match condition {
        Condition::QueryEquals { query } => normalized(&request.q) == normalized(query),
        Condition::QueryContainsAll { tokens } => {
            let actual: BTreeSet<String> = tokenize(&request.q).collect();
            tokens
                .iter()
                .flat_map(|t| tokenize(t))
                .all(|t| actual.contains(&t))
        }
        Condition::ContextCategory { category_id } => {
            request.context.category_id == Some(*category_id)
        }
        Condition::ContextTag { tag } => request.context.tags.contains(tag),
    }
}
fn target_matches(target: &Target, product: &Product, variant: &Variant) -> bool {
    match target {
        Target::All => true,
        Target::Products { ids } => ids.contains(&product.id.0),
        Target::Variants { ids } => ids.contains(&variant.id.0),
        Target::Filters { filters } => query(filters).matches_variant(product, variant),
    }
}
fn hidden(active: &[&Rule], product: &Product, variant: &Variant) -> bool {
    active
        .iter()
        .any(|r| matches!(r.action, Action::Hide) && target_matches(&r.target, product, variant))
}
fn rule_cmp(a: &Rule, b: &Rule) -> Ordering {
    specificity(&b.scope)
        .cmp(&specificity(&a.scope))
        .then_with(|| b.priority.cmp(&a.priority))
        .then_with(|| a.id.cmp(&b.id))
}
fn pin_from_rule(rule: &Rule, variant_id: u64) -> Pin {
    Pin {
        position: match rule.action {
            Action::Pin { position } => position,
            _ => unreachable!(),
        },
        rule_id: rule.id.clone(),
        specificity: specificity(&rule.scope),
        priority: rule.priority,
        exact_variant: if matches!(rule.target, Target::Variants { .. }) {
            Some(variant_id)
        } else {
            None
        },
    }
}
fn pin_cmp(a: &Pin, b: &Pin) -> Ordering {
    b.specificity
        .cmp(&a.specificity)
        .then_with(|| b.priority.cmp(&a.priority))
        .then_with(|| a.rule_id.cmp(&b.rule_id))
}
fn price_cmp(a: &Hit, b: &Hit, sort: Sort) -> Ordering {
    match sort {
        Sort::PriceAsc => a.price_cents.cmp(&b.price_cents),
        Sort::PriceDesc => b.price_cents.cmp(&a.price_cents),
        Sort::Relevance => Ordering::Equal,
    }
}
fn hit_cmp(a: &Hit, b: &Hit, request: &Request) -> Ordering {
    let ranking =
        if request.sort != Sort::Relevance && request.sort_policy == SortPolicy::ShopperOverrides {
            price_cmp(a, b, request.sort)
        } else {
            b.relevance_bucket
                .cmp(&a.relevance_bucket)
                .then_with(|| b.merch_groups.cmp(&a.merch_groups))
                .then_with(|| b.composed_score.total_cmp(&a.composed_score))
                .then_with(|| price_cmp(a, b, request.sort))
        };
    ranking
        .then_with(|| a.product_id.cmp(&b.product_id))
        .then_with(|| a.selected_variant_id.cmp(&b.selected_variant_id))
}
fn apply_pins(
    mut ranked: Vec<Ranked>,
    eligible_hits: &BTreeMap<u64, Hit>,
    diagnostics: &mut Vec<String>,
) -> Result<Vec<Ranked>> {
    if ranked.is_empty() {
        return Ok(ranked);
    }
    let len = ranked.len();
    let mut claims: Vec<usize> = (0..len).filter(|i| ranked[*i].pin.is_some()).collect();
    claims.sort_by(|a, b| {
        pin_cmp(
            ranked[*a].pin.as_ref().unwrap(),
            ranked[*b].pin.as_ref().unwrap(),
        )
        .then_with(|| a.cmp(b))
    });
    let mut slots = BTreeMap::new();
    let mut selected = BTreeSet::new();
    for index in claims {
        let pin = ranked[index].pin.as_ref().unwrap().clone();
        ensure(
            pin.position < len,
            format!(
                "eligible pin {} position {} outside result count {}",
                pin.rule_id, pin.position, len
            ),
        )?;
        let slot = pin.position;
        if let std::collections::btree_map::Entry::Vacant(entry) = slots.entry(slot) {
            entry.insert(index);
            selected.insert(index);
            if let Some(variant_id) = pin.exact_variant {
                let count = ranked[index].hit.matching_variant_count;
                ranked[index].hit = eligible_hits
                    .get(&variant_id)
                    .expect("pin came from eligible variant")
                    .clone();
                ranked[index].hit.matching_variant_count = count;
            }
            ranked[index].hit.pin_rule_id = Some(pin.rule_id.clone());
        } else {
            diagnostics.push(format!(
                "Pin {} lost slot {} conflict; candidate remains organic.",
                pin.rule_id, slot
            ));
        }
    }
    let mut organic = (0..len).filter(|i| !selected.contains(i));
    let order: Vec<usize> = (0..len)
        .map(|slot| {
            slots
                .get(&slot)
                .copied()
                .unwrap_or_else(|| organic.next().expect("unfilled organic slot"))
        })
        .collect();
    let mut items: Vec<Option<Ranked>> = ranked.into_iter().map(Some).collect();
    Ok(order
        .into_iter()
        .map(|i| items[i].take().expect("unique candidate"))
        .collect())
}
fn facet_values(field: &FacetField, product: &Product, variant: &Variant) -> Vec<String> {
    match field {
        FacetField::Brand => vec![product.brand.0.to_string()],
        FacetField::Category => vec![product.category.0.to_string()],
        FacetField::ProductType => vec![product.product_type.0.to_string()],
        FacetField::PriceCents => vec![variant.price.amount_cents.to_string()],
        FacetField::Attribute { name } => match effective_attributes(product, variant).get(name) {
            Some(AttributeValue::Enum(v)) | Some(AttributeValue::Text(v)) => vec![v.clone()],
            Some(AttributeValue::MultiEnum(v)) => v.clone(),
            Some(AttributeValue::Boolean(v)) => vec![v.to_string()],
            Some(AttributeValue::Numeric(v)) => {
                vec![if *v == 0.0 { "0".into() } else { v.to_string() }]
            }
            None => vec![],
        },
    }
}
fn validate_filters(filters: &[Filter]) -> Result<()> {
    for f in filters {
        if let Filter::Numeric { value, .. } = f {
            ensure(value.is_finite(), "numeric filter must be finite")?;
        }
    }
    Ok(())
}
fn validate_target(target: &Target) -> Result<()> {
    if let Target::Filters { filters } = target {
        validate_filters(filters)?;
    }
    Ok(())
}
fn validate_request(request: &Request) -> Result<()> {
    validate_filters(&request.filters)?;
    let mut rule_ids = BTreeSet::new();
    for r in &request.rules {
        ensure(
            !r.id.is_empty() && rule_ids.insert(&r.id),
            "rule IDs must be unique and nonempty",
        )?;
        validate_target(&r.target)?;
        if let Scope::Query { query } = &r.scope {
            ensure(
                !normalized(query).is_empty(),
                "query scope must contain a token",
            )?;
        }
        for c in &r.conditions {
            match c {
                Condition::QueryEquals { query } => ensure(
                    !normalized(query).is_empty(),
                    "query condition must contain a token",
                )?,
                Condition::QueryContainsAll { tokens } => ensure(
                    !tokens.is_empty() && tokens.iter().all(|t| !normalized(t).is_empty()),
                    "query contains condition must contain tokens",
                )?,
                Condition::ContextTag { tag } => {
                    ensure(!tag.is_empty(), "context tag must not be empty")?
                }
                _ => {}
            }
        }
    }
    for signal in &request.personal_signals {
        ensure(
            signal.factor.is_finite()
                && (MIN_PERSONAL_FACTOR..=MAX_PERSONAL_FACTOR).contains(&signal.factor),
            "personal factor must be finite in [0.25, 4]",
        )?;
        validate_target(&signal.target)?;
    }
    let mut facet_names = BTreeSet::new();
    for f in &request.facets {
        ensure(
            !f.name.is_empty() && facet_names.insert(&f.name),
            "facet names must be unique and nonempty",
        )?;
    }
    Ok(())
}
fn validate_catalog(input: &CatalogInput) -> Result<()> {
    let mut products = BTreeSet::new();
    let mut variants = BTreeSet::new();
    for p in &input.products {
        ensure(products.insert(p.product_id), "duplicate product ID")?;
        validate_attributes(&p.attributes)?;
        for v in &p.variants {
            ensure(
                variants.insert(v.variant_id),
                "variant IDs must be globally unique",
            )?;
            ensure(v.price_cents >= 0, "negative catalog price unsupported")?;
            validate_attributes(&v.attributes)?;
        }
    }
    ensure(
        variants.len() <= u32::MAX as usize,
        "catalog exceeds u32 ordinal capacity",
    )
}
fn validate_attributes(attrs: &BTreeMap<String, Value>) -> Result<()> {
    for (key, value) in attrs {
        ensure(!key.is_empty(), "attribute name must not be empty")?;
        if let Value::Numeric(v) = value {
            ensure(v.is_finite(), "catalog numeric attributes must be finite")?;
        }
    }
    Ok(())
}
