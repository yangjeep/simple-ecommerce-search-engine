#!/usr/bin/env python3
"""Convert public WANDS catalog/query inputs, with explicit synthetic serving fields.

No catalog/user/merchant data is downloaded and no service is started. Prices
are zero placeholders, never derived from rating counts or dimensions.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path

ROOT=Path(__file__).resolve().parent
WORKSPACE=ROOT.parent

def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def write(path,value): path.write_text(json.dumps(value,ensure_ascii=False,separators=(",",":"),allow_nan=False)+"\n")
def typed(value):
    if isinstance(value,bool): return {"type":"boolean","value":value}
    if isinstance(value,(float,int)): return {"type":"numeric","value":value}
    return {"type":"enum","value":str(value)}
def read_queries(path):
    with path.open(newline="",encoding="utf-8") as stream:
        reader=csv.DictReader(stream,delimiter="\t",strict=True)
        fields=reader.fieldnames
        if not fields or len(fields)!=len(set(fields)) or not {"query_id","query"}.issubset(fields):
            raise ValueError("Official query TSV schema missing/duplicate query_id/query")
        rows=list(reader)
    if not rows:raise ValueError("Official query TSV contains no query rows")
    output=[];seen=set()
    for row in rows:
        if None in row or any(value is None for value in row.values()):
            raise ValueError("Malformed official query TSV row width")
        qid=row["query_id"]
        if not qid.isascii() or not qid.isdecimal() or str(int(qid))!=qid:
            raise ValueError("Official query ID must be a canonical nonnegative decimal integer")
        if qid in seen:raise ValueError("Duplicate official query IDs")
        if not row["query"].strip():raise ValueError("Official query text must not be empty")
        seen.add(qid);output.append({"query_id":qid,"text":row["query"]})
    return output

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("--source",type=Path,required=True)
    ap.add_argument("--queries",type=Path,required=True)
    ap.add_argument("--out",type=Path,default=ROOT/"wands-official-decoded")
    ap.add_argument("--expected-query-sha256",default="63b61660560fecc33ec490804c7e2b81402ee3e7c31a9cbb5e03736639f68e95")
    ap.add_argument("--variants-per-product",type=int,choices=[1,3],default=1)
    args=ap.parse_args(); args.out.mkdir(parents=True,exist_ok=True)
    source=[json.loads(line) for line in args.source.read_text().splitlines() if line.strip()]
    categories={name:i+1 for i,name in enumerate(sorted({r.get("category_leaf") or "<missing>" for r in source}))}
    types={name:i+1 for i,name in enumerate(sorted({r.get("product_class") or "<missing>" for r in source}))}
    products=[]
    for row in source:
        pid=int(row["id"])
        attrs={k:typed(v) for k,v in row.items() if k not in {"id","title","description"} and v is not None}
        attrs["description"]={"type":"text","value":row.get("description") or ""}
        variants=[]
        for n in range(args.variants_per_product):
            # No real WANDS price/inventory/variants exist. These are declared placeholders.
            attrs_v={} if n==0 else {"validation_synthetic_variant_label":{"type":"enum","value":f"v{n}"}}
            variants.append({"variant_id":pid*args.variants_per_product+n,"price_cents":0,
                             "available_units":1,"in_stock":True,"attributes":attrs_v})
        products.append({"product_id":pid,"title":row.get("title") or "",
                         "brand_id":pid%17+1,"category_id":categories[row.get("category_leaf") or "<missing>"],
                         "product_type_id":types[row.get("product_class") or "<missing>"],
                         "attributes":attrs,"variants":variants})
    catalog={"products":products}
    if sha(args.queries)!=args.expected_query_sha256:ap.error("Official query source hash differs from frozen expected hash")
    query_rows=read_queries(args.queries)
    if len(query_rows)!=480:ap.error("Official WANDS query source must contain exactly 480 unique query IDs")
    inputs=[("synthetic-browse","", "synthetic browse request")]+[(str(q["query_id"]),q["text"],"public WANDS query text") for q in query_rows]
    cases=[]
    for i,(qid,q,origin) in enumerate(inputs):
        request={"q":q,"lexical":{"profile":"bm25_literal"},"limit":20,"result_mode":"products",
                 "facets":[{"name":"color","field":{"type":"attribute","name":"color"}},
                           {"name":"product_class","field":{"type":"attribute","name":"product_class"}}],
                 "filters":[],"rules":[],"personal_signals":[]}
        if i%5==1:
            request["filters"]=[{"type":"numeric","attribute":"average_rating","op":"gte","value":3.0}]
        if i%5==2: request["result_mode"]="variants"
        if i%5==3: request["offset"]=20
        # Group/factor only: no hides/stock edits; candidate set identical across four arms.
        merch=[{"id":"synthetic-brand-group","scope":{"type":"global"},"target":{"type":"filters","filters":[{"type":"brand","value":i%17+1}]},"action":{"type":"group","order":0,"rank":1}},
               {"id":"synthetic-real-rating-group","scope":{"type":"global"},"target":{"type":"filters","filters":[{"type":"numeric","attribute":"average_rating","op":"gte","value":4.5}]},"action":{"type":"group","order":1,"rank":1}}]
        signals=[{"target":{"type":"filters","filters":[{"type":"brand","value":(i+3)%17+1}]},"factor":2.0},
                 {"target":{"type":"filters","filters":[{"type":"numeric","attribute":"review_count","op":"gte","value":50.0}]},"factor":1.25}]
        cases.append({"id":"wands-"+qid,"query_provenance":origin,"request":request,"merch_rules":merch,"personal_signals":signals})
    provenance={"catalog_source":str(args.source),"catalog_source_sha256":sha(args.source),
                "query_source":str(args.queries),"query_source_sha256":sha(args.queries),"query_source_commit":"3b74dcf4ba29ab8ff3e6a50b5b09fc627cb882b5","query_parsing":"csv.DictReader(newline=empty,delimiter=TAB), standard quoted-field decoding; no manual rewrites","workload_version":"wands-official-decoded-v2","experiment_contract":"athos80-experimental-v1.1","lexical_request_profile":"bm25_literal","query_handling":"Official WANDS query fields decoded with the standard TSV/CSV reader, then unchanged. Native index analyzer terms ANDed literally; strict parser profile is separate diagnostic.",
                "actual_public_fields":"WANDS product IDs, title, description, product class, category paths, parsed product attributes and rating/review numerics.",
                "synthetic_fields":{"brand_id":"source product_id modulo17 +1 (not real brands)",
                    "variants":"one synthetic serving row per product; if --variants-per-product3, duplicate text plus synthetic labels; not real variants",
                    "price_cents":"constant0 missing-price placeholder; no WANDS price-sorting claim",
                    "inventory":"constant in_stock=true, available_units=1; not real inventory",
                    "merch_rules":"deterministic synthetic test configurations; no merchant usage data",
                    "personal_signals":"deterministic synthetic supplied factors; no shoppers/behavior/model calls",
                    "traffic":"uniform deterministic round robin, not production traffic distribution"},
                "excluded":"ETL/connector upkeep, real catalog updates, profile/factor generation and fetch/cache, tracking, recommendation profiles/carousels/bundles, merchant control plane, deployment/operational total cost.",
                "not_claims":["80% Athos capability coverage","20% Athos total cost","Athos lexical or relevance parity","real merchant/user behavior"]}
    write(args.out/"catalog.json",catalog)
    write(args.out/"workload.json",{"metadata":provenance,"cases":cases})
    write(args.out/"manifest.json",{**provenance,"products":len(products),"variants":sum(len(p["variants"]) for p in products),"cases":len(cases),
                                 "files":{name:{"sha256":sha(args.out/name),"bytes":(args.out/name).stat().st_size} for name in ["catalog.json","workload.json"]},
                                 "status":"prepared input only; not executed"})
if __name__=="__main__":main()
