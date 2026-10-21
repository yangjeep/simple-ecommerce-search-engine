#!/usr/bin/env python3
"""Write deterministic synthetic correctness inputs, never start the engine."""
import argparse
import copy
import hashlib
import json
import random
from pathlib import Path

ROOT = Path(__file__).resolve().parent

def attr(kind, value): return {"type":kind, "value":value}
def target(kind, ids): return {"type":kind, "ids":ids}
def rule(name, action, to=None, scope=None, priority=0, conditions=None):
    return {"id":name, "scope":scope or {"type":"global"}, "priority":priority,
            "conditions":conditions or [], "target":to or {"type":"all"}, "action":action}
def pin(name, ids, position=0, kind="variants", **kwargs):
    return rule(name, {"type":"pin", "position":position}, target(kind, ids), **kwargs)
def group(name, ids, rank=1, order=0, kind="variants", **kwargs):
    return rule(name, {"type":"group", "rank":rank, "order":order}, target(kind, ids), **kwargs)
def enum_filter(name, value): return {"type":"enum", "attribute":name, "value":value}
def facet(name, kind="attribute"):
    return {"name":name, "field":{"type":kind, **({"name":name} if kind == "attribute" else {})}}
def variant(vid, price, color, size="small", units=3, stock=True, **extra):
    return dict(variant_id=vid, price_cents=price, available_units=units, in_stock=stock,
                attributes={"color":attr("enum",color), "size":attr("enum",size), **extra})
def product(pid, variants, title="red shoes", brand=1, category=1, **attrs):
    return dict(product_id=pid, title=title, brand_id=brand, category_id=category,
                product_type_id=1, attributes=attrs, variants=variants)
def fixtures():
    high = 2**32 + 7
    catalog = {"products":[
        product(1,[variant(11,100,"red",units=0), variant(12,200,"blue","large")],
                title="running shoe", description=attr("text","shared inherited mesh"),
                color=attr("enum","parent-red"), weight=attr("numeric",-0.0)),
        product(2,[variant(21,150,"red"),variant(22,120,"blue"),variant(23,130,"red",stock=False)],
                tags=attr("multi_enum",["sale","sale","light"]), weight=attr("numeric",0.0),
                description=attr("text","red shoes breathable")),
        product(3,[variant(31,400,"red","large",description=attr("text","variant-only suede"))],
                brand=2, category=2, description=attr("text","parent-only leather"),
                weight=attr("numeric",1.5), eco=attr("boolean",True)),
        product(4,[variant(41,300,"green","large"),variant(42,300,"green","large")],
                title="green shoes", brand=3, category=2, weight=attr("numeric",2.0)),
        product(high,[variant(high*10,250,"red","large")],title="red shoes", brand=2,
                weight=attr("numeric",3.0)),
        product(2**64-1,[variant(2**64-1,260,"red","small")],title="雪 靴",brand=4),
    ]}
    all_ids = [(p["product_id"],v["variant_id"]) for p in catalog["products"] for v in p["variants"]]
    supplied = {"profile":"supplied", "hits":[dict(product_id=p,variant_id=v,score=1.0,relevance_bucket=0) for p,v in all_ids]}
    common_facets = [facet("color"), facet("size"), facet("brand", "brand"), facet("weight"),facet("tags")]
    cases = []
    def add(name, request, error=False, purpose=""):
        cases.append({"id":name, "request":request, "expect_error":error, "purpose":purpose})
    add("empty_browse_products", {"facets":common_facets}, purpose="Default product grouping and deduplicated counts")
    add("empty_browse_variants", {"result_mode":"variants", "facets":common_facets})
    add("red_oos_blue_sibling", {"filters":[enum_filter("color","red"),enum_filter("size","large")],"facets":common_facets}, purpose="Never combine red from one sibling with size/stock from another")
    add("variant_overrides_product_text", {"q":"parent-only", "facets":common_facets}, purpose="Overridden shared Text must not leak into lexical candidate")
    add("variant_text_recall", {"q":"variant-only", "facets":common_facets})
    add("enum_not_lexical_text", {"q":"parent-red"})
    add("unicode_query", {"q":"雪 靴"})
    add("punctuation_browse", {"q":"--- !!!", "limit":30})
    add("typed_case_sensitive_filter", {"filters":[enum_filter("color","RED")]})
    add("typed_mismatch_not_coerced", {"filters":[{"type":"numeric","attribute":"color","op":"eq","value":0.0}]})
    add("numeric_signed_zero_facets", {"filters":[{"type":"numeric","attribute":"weight","op":"eq","value":-0.0}],"facets":[facet("weight"),facet("color")]})
    add("price_strict_bounds", {"filters":[{"type":"price_under_cents","value":200},{"type":"price_over_cents","value":120}],"facets":[facet("price_cents","price_cents")]})
    add("availability_disabled", {"in_stock_only":False,"result_mode":"variants","facets":common_facets})
    add("personal_cannot_create_recall", {"q":"no-match","personal_signals":[{"target":{"type":"all"},"factor":4.0}]})
    ranked = copy.deepcopy(supplied)
    for hit in ranked["hits"]:
        if hit["variant_id"] == 21: hit.update(score=0.1, relevance_bucket=10)
        if hit["variant_id"] == 31: hit.update(score=1000.0, relevance_bucket=0)
    add("bucket_dominates_merch_personal", {"lexical":ranked, "rules":[group("huge-low-bucket",[31],rank=100000)],"personal_signals":[{"target":target("variants",[31]),"factor":4.0}], "limit":30})
    add("ordered_merch_axes", {"lexical":supplied,"rules":[group("axis-0",[21],rank=1,order=0),group("axis-1",[31],rank=1000,order=1)]})
    add("negative_group_bury", {"lexical":supplied,"rules":[group("bury",[21],rank=-1)]})
    add("personal_inside_group", {"lexical":supplied,"rules":[group("first",[21],rank=1)],"personal_signals":[{"target":target("variants",[31]),"factor":4.0}]})
    add("neutral_missing_personal", {"lexical":supplied})
    add("factor_multiply_clamp_once", {"lexical":supplied,"personal_signals":[{"target":target("variants",[31]),"factor":f} for f in [4.0,4.0,.25]]})
    add("factor_order_invariant", {"lexical":supplied,"personal_signals":[{"target":target("variants",[31]),"factor":f} for f in [.25,4.0,4.0]]})
    add("factor_lower_bound_saturation", {"lexical":supplied,"personal_signals":[{"target":target("variants",[31]),"factor":.25}]*64})
    add("factor_upper_bound_saturation", {"lexical":supplied,"personal_signals":[{"target":target("variants",[31]),"factor":4.0}]*64})
    add("signals_over_http_cap", {"personal_signals":[{"target":{"type":"all"},"factor":1.0}]*65}, error=True)
    for count in [3,10]:
        add(f"neutral_inverse_pairs_{count}", {"lexical":supplied,"personal_signals":[{"target":target("variants",[31]),"factor":f} for f in [.25]*count+[4.0]*count]})
    exact_float=copy.deepcopy(supplied)
    exact_float["hits"][0]["score"]=9.842065267857133e-06
    exact_float["hits"][1]["score"]=9.842065267857133e-06
    add("binary64_score_roundtrip", {"lexical":exact_float,"limit":30})
    signed_zero=copy.deepcopy(supplied)
    signed_zero["hits"][1]["score"]=-0.0
    add("signed_score_zero_canonical", {"lexical":signed_zero,"limit":30})
    override_rules = [group("global",[21],rank=2),group("category",[31],rank=1,scope={"type":"category","category_id":2}),group("query",[999],rank=100,scope={"type":"query","query":"red shoes"})]
    add("whole_group_scope_replacement", {"q":"RED shoes!", "lexical":supplied,"context":{"category_id":2},"rules":override_rules})
    add("category_requires_context", {"lexical":supplied,"rules":override_rules})
    add("group_priority_id_ties", {"lexical":supplied,"rules":[group("z",[31],rank=7),group("a",[31],rank=2),group("priority",[21],rank=3,priority=1)]})
    hides=[rule("hide-query",{"type":"hide"},target("products",[2]),scope={"type":"query","query":"red shoes"},conditions=[{"type":"context_tag","tag":"sale"}]),
           rule("hide-category",{"type":"hide"},target("variants",[31]),scope={"type":"category","category_id":2},conditions=[{"type":"query_contains_all","tokens":["red"]}])]
    add("conditional_hide_inactive", {"lexical":supplied,"q":"red shoes","rules":hides,"facets":common_facets})
    add("conditional_hide_active", {"lexical":supplied,"q":"red shoes","context":{"category_id":2,"tags":["sale"]},"rules":hides,"facets":common_facets})
    add("hide_beats_pin", {"lexical":supplied,"rules":[rule("hide",{"type":"hide"},target("products",[2])),pin("resurrect",[2],kind="products")],"facets":common_facets})
    add("ineligible_pins_ignored", {"filters":[enum_filter("color","red")],"rules":[pin("oos",[11],position=999),pin("wrong-filter",[12],position=999),pin("unknown",[99999],position=999)]})
    add("variant_pin_exact_display", {"lexical":supplied,"rules":[pin("pin-blue",[22])]}, purpose="Must display exact eligible variant22, not comparator-best sibling21")
    add("losing_variant_pin_keeps_best", {"lexical":supplied,"rules":[pin("z-blue",[22]),pin("a-other",[31])]})
    add("product_pin_best_display", {"lexical":supplied,"rules":[pin("pin-product",[2],kind="products")]})
    add("pin_collision_id_priority", {"lexical":supplied,"rules":[pin("z",[21]),pin("a",[31])],"limit":30})
    add("product_pin_variants_collision", {"result_mode":"variants","lexical":supplied,"rules":[pin("product",[2],kind="products")],"limit":30})
    add("eligible_pin_outside_is_error", {"lexical":supplied,"rules":[pin("outside",[21],position=999)]}, error=True)
    add("explicit_shopper_sort", {"lexical":supplied,"sort":"price_desc","rules":[group("boost",[21]),pin("pin",[21])],"personal_signals":[{"target":{"type":"all"},"factor":4.0}]})
    add("merchant_price_subsort", {"lexical":supplied,"sort":"price_desc","sort_policy":"merch_overrides","rules":[group("boost",[21])],"personal_signals":[{"target":target("variants",[31]),"factor":4.0}]})
    for mode in ["products","variants"]:
        for facet_mode in [None,"products","variants"]:
            for offset in [0,1,3,20]:
                add(f"page_{mode}_{facet_mode}_{offset}", {"result_mode":mode,"facet_count_mode":facet_mode,"filters":[enum_filter("color","red")],"facets":common_facets,"limit":2,"offset":offset})
    # Invalid HTTP envelopes are independently rejected, not silently ignored.
    for name, request in [
        ("unknown_field", {"query":"red"}), ("unknown_mode",{"result_mode":"both"}),
        ("unknown_lexical",{"lexical":{"profile":"invented"}}),
        ("bad_factor_low",{"personal_signals":[{"target":{"type":"all"},"factor":.24}]}),
        ("bad_factor_high",{"personal_signals":[{"target":{"type":"all"},"factor":4.01}]}),
        ("factor_null",{"personal_signals":[{"target":{"type":"all"},"factor":None}]}),
        ("supplied_unknown_id",{"lexical":{"profile":"supplied","hits":[dict(product_id=1,variant_id=999,score=1,relevance_bucket=0)]}}),
        ("supplied_wrong_product",{"lexical":{"profile":"supplied","hits":[dict(product_id=1,variant_id=21,score=1,relevance_bucket=0)]}}),
        ("supplied_duplicate",{"lexical":{"profile":"supplied","hits":[dict(product_id=2,variant_id=21,score=1,relevance_bucket=0)]*2}}),
        ("supplied_negative_score",{"lexical":{"profile":"supplied","hits":[dict(product_id=2,variant_id=21,score=-1,relevance_bucket=0)]}}),
        ("supplied_over_score",{"lexical":{"profile":"supplied","hits":[dict(product_id=2,variant_id=21,score=1e13,relevance_bucket=0)]}}),
        ("duplicate_rule_id",{"rules":[group("dupe",[21]),group("dupe",[31])]}),
        ("duplicate_facet_name",{"facets":[facet("color"),facet("color")]}),
        ("blank_query_scope",{"rules":[group("blank",[21],scope={"type":"query","query":"!!!"})]}),
        ("u64_overflow_request",{"rules":[pin("overflow",[2**64])]}),
    ]: add(name,request,error=True)
    for name, request in [
        ("q_over_http_cap", {"q":"x"*4097}),
        ("limit_over_http_cap", {"limit":1001}),
        ("context_tags_over_http_cap", {"context":{"tags":["tag"]*65}}),
        ("filters_over_http_cap", {"filters":[enum_filter("color","red")]*65}),
        ("facets_over_http_cap", {"facets":[{"name":f"f{i}","field":{"type":"brand"}} for i in range(17)]}),
        ("rules_over_http_cap", {"rules":[group(f"g{i}",[21]) for i in range(65)]}),
        ("conditions_over_http_cap", {"rules":[group("too-many",[21],conditions=[{"type":"context_tag","tag":"sale"}]*17)]}),
        ("target_ids_over_http_cap", {"rules":[group("too-many",list(range(4097)))]}),
        ("invalid_factor_unmatched_target", {"personal_signals":[{"target":target("variants",[9999]),"factor":5.0}]}),
    ]: add(name,request,error=True)
    for name, raw in [("nan_factor_json", '{"personal_signals":[{"target":{"type":"all"},"factor":NaN}]}'),
                      ("infinite_numeric_json", '{"filters":[{"type":"numeric","attribute":"weight","op":"eq","value":Infinity}]}')]:
        cases.append({"id":name,"request":{},"raw_json":raw,"expect_error":True})
    # Deterministic differential cases; not a feature-coverage percentage.
    rng=random.Random(20261021)
    for i in range(150):
        req={"lexical":copy.deepcopy(supplied),"result_mode":rng.choice(["products","variants"]),
             "facet_count_mode":rng.choice([None,"products","variants"]),"facets":common_facets,
             "filters":[],"offset":rng.randrange(5),"limit":rng.randrange(1,7),
             "rules":[],"personal_signals":[],"context":{"category_id":rng.choice([1,2]),"tags":rng.choice([[],["sale"]])}}
        for hit in req["lexical"]["hits"]:
            hit["score"]=rng.choice([0.0,.5,1.0,2.0,10.0]); hit["relevance_bucket"]=rng.randrange(3)
        if rng.random()<.6: req["filters"].append(enum_filter("color",rng.choice(["red","blue","green"])))
        for j in range(rng.randrange(5)):
            vids=rng.sample([v for p,v in all_ids],rng.randrange(1,4))
            scope=rng.choice([{"type":"global"},{"type":"category","category_id":2},{"type":"query","query":"unused"}])
            req["rules"].append(group(f"g-{j}",vids,rank=rng.randrange(-2,4),order=rng.randrange(3),priority=rng.randrange(3),scope=scope))
        if rng.random()<.5: req["rules"].append(rule("hide",{"type":"hide"},target("variants",rng.sample([v for p,v in all_ids],2)), conditions=[{"type":"context_tag","tag":"sale"}]))
        for j in range(rng.randrange(4)):
            req["personal_signals"].append({"target":target("variants",rng.sample([v for p,v in all_ids],3)),"factor":rng.choice([.25,.5,1.,2.,4.])})
        add(f"seeded_{i:03d}",req)
    # Late best variant and many early siblings defeat oversample-before-group topK.
    stress_catalog={"products":[product(100,[variant(1000+i,100+i,"red") for i in range(100)]),product(200,[variant(9999,100,"red")])]}
    stress_hits=[dict(product_id=100,variant_id=1000+i,score=100-i,relevance_bucket=0) for i in range(100)] + [dict(product_id=200,variant_id=9999,score=.1,relevance_bucket=0)]
    stress_cases=[{"id":"global_rank_before_group_topk", "request":{"lexical":{"profile":"supplied","hits":stress_hits},"limit":2},"expect_error":False},
                  {"id":"late_merch_boost_not_presliced", "request":{"lexical":{"profile":"supplied","hits":stress_hits},"rules":[group("late",[9999],rank=1)],"limit":1},"expect_error":False}]
    return catalog,cases,stress_catalog,stress_cases

def write(path,data):
    path.write_text(json.dumps(data,ensure_ascii=False,indent=2,allow_nan=False)+"\n")
    return {"path":str(path),"sha256":hashlib.sha256(path.read_bytes()).hexdigest(),"bytes":path.stat().st_size}
def main():
    parser=argparse.ArgumentParser(); parser.add_argument("--out",type=Path,default=ROOT/"fixtures"); args=parser.parse_args()
    args.out.mkdir(parents=True,exist_ok=True)
    catalog,cases,stress_catalog,stress_cases=fixtures()
    files=[]
    for stem,cat,cs in [("synthetic",catalog,cases),("global_topk",stress_catalog,stress_cases)]:
        files.append(write(args.out/(stem+"_catalog.json"),cat))
        files.append(write(args.out/(stem+"_cases.json"),{"provenance":"Entirely synthetic catalog, requests, merchant rules and factors; no production or user behavior data.","cases":cs}))
        files.append(write(args.out/(stem+"_replay.json"),{"catalog":cat,"requests":[c["request"] for c in cs if not c.get("expect_error")]}))
    bm25_names={"empty_browse_products","empty_browse_variants","red_oos_blue_sibling","variant_overrides_product_text","variant_text_recall","enum_not_lexical_text","unicode_query","numeric_signed_zero_facets","availability_disabled","personal_cannot_create_recall","conditional_hide_inactive","conditional_hide_active","hide_beats_pin","ineligible_pins_ignored","variant_pin_exact_display","losing_variant_pin_keeps_best","product_pin_best_display","pin_collision_id_priority","product_pin_variants_collision","explicit_shopper_sort","merchant_price_subsort"}
    bm25=[]
    for case in cases:
        if case['id'] not in bm25_names and not case['id'].startswith('page_'):continue
        cloned=copy.deepcopy(case);cloned['id']='bm25_'+case['id'];cloned['request']['lexical']={'profile':'bm25'}
        bm25.append(cloned)
    bm25 += [
        {'id':'bm25_parse_error_unclosed_quote','request':{'q':'"unclosed','lexical':{'profile':'bm25'}},'expect_error':True},
        {'id':'bm25_parse_error_unknown_field','request':{'q':'absentfield:shoes','lexical':{'profile':'bm25'}},'expect_error':True},
        {'id':'bm25_self_exclusion_full_lexical','request':{'q':'shoes','lexical':{'profile':'bm25'},'filters':[enum_filter('color','red')],'facets':[facet('color'),facet('size')]},'expect_error':False},
        {'id':'bm25_strict_and_no_match','request':{'q':'shoes impossibleword987','lexical':{'profile':'bm25'}},'expect_error':False},
    ]
    files.append(write(args.out/'bm25_cases.json',{'provenance':'Entirely synthetic BM25 integration cases. Full prefilter trace is native lexical authority, not independent BM25 quality validation.','cases':bm25}))
    literal=[]
    for case in bm25:
        if case.get('expect_error'):continue
        cloned=copy.deepcopy(case);cloned['id']=cloned['id'].replace('bm25_','literal_',1);cloned['request']['lexical']={'profile':'bm25_literal'}
        literal.append(cloned)
    for name,q,ids in [
        ('quotes_are_literal_delimiters','"red shoes""',[21,22,23,31,(2**32+7)*10]),
        ('or_is_required_token','red OR shoes',[]),
        ('and_is_required_token','red AND shoes',[]),
        ('colon_is_not_field_syntax','absentfield:shoes',[]),
        ('punctuation_no_browse','!!! () []',[]),
        ('wildcard_no_browse','*',[]),
        ('boost_syntax_is_literal','red^2 shoes',[]),
        ('field_syntax_is_literal','title:red shoes',[]),
        ('minus_is_delimiter','-red shoes',[21,22,23,31,(2**32+7)*10]),
        ('removed_long_token_no_browse','x'*80,[]),
        ('whitespace_is_browse','   ',[v['variant_id'] for p in catalog['products'] for v in p['variants']]),
        ('dedup_normalized_tokens','red shoes RED shoes',[21,22,23,31,(2**32+7)*10]),
    ]:
        literal.append({'id':'literal_'+name,'request':{'q':q,'lexical':{'profile':'bm25_literal'},'result_mode':'variants','limit':30},'expect_error':False,'expected_lexical_variant_ids':ids})
    files.append(write(args.out/'bm25_literal_cases.json',{'provenance':'Synthetic literal-text BM25 integration and hand-calculated lexical candidate sets; raw query bytes unchanged.','cases':literal}))
    write(args.out/"manifest.json",{"seed":20261021,"contract":"athos80-experimental-v1.1","files":files,"case_count":len(cases)+len(stress_cases),"bm25_integration_cases":len(bm25),"bm25_literal_integration_cases":len(literal),"status":"prepared inputs, not executed"})
if __name__=="__main__": main()
