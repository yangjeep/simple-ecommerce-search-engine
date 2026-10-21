#!/usr/bin/env python3
"""Gated black-box correctness validation; no engine start, build, or load test.

The caller must supply its isolated loopback server after explicit operator approval.
BM25 traces test execution integration, not independent lexical correctness.
"""
import argparse
import copy
import hashlib
import ipaddress
import json
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from oracle import search, differences, ScanCatalog, PROFILE_NAMES


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None

def loopback_opener():
    # Ambient proxies and redirects must not move data outside our owned server.
    return urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())

def call(url, request, timeout, raw=None):
    body = raw.encode() if raw is not None else json.dumps(request,ensure_ascii=False,allow_nan=False).encode()
    req=urllib.request.Request(url,data=body,headers={"Content-Type":"application/json"},method="POST")
    start=time.perf_counter()
    try:
        with loopback_opener().open(req,timeout=timeout) as reply:
            status=reply.status; data=reply.read()
    except urllib.error.HTTPError as error:
        status=error.code; data=error.read()
    try: response=json.loads(data)
    except (ValueError,UnicodeDecodeError): response={"_non_json_body":data[:2000].decode(errors="replace")}
    return status,response,(time.perf_counter()-start)*1000

def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def canonical_sha(value):return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(",",":"),allow_nan=False).encode()).hexdigest()
def stable_response_sha(response):
    stable=copy.deepcopy(response)
    del stable['metrics']['elapsed_micros']
    return canonical_sha(stable)
def validate_loopback(url):
    parsed=urllib.parse.urlparse(url)
    if parsed.scheme!="http":raise ValueError("Only isolated loopback HTTP endpoint is permitted")
    try:address=ipaddress.ip_address(parsed.hostname or "")
    except ValueError as error:raise ValueError("Endpoint must be literal loopback, not DNS") from error
    if not address.is_loopback or parsed.port is None:raise ValueError("Literal loopback and explicit port required")
    if parsed.username or parsed.password or parsed.query or parsed.fragment:
        raise ValueError("Unexpected endpoint credentials/query/fragment")
    return (str(address),parsed.port)

def load_cases(package,max_cases):
    if max_cases<0:raise ValueError("max-cases must be nonnegative")
    cases=package.get("cases")
    if not isinstance(cases,list) or not cases:raise ValueError("At least one case is required; zero-check pass forbidden")
    return cases[:max_cases or None]

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("--run-approved",action="store_true",help="Explicit local-run approval required; review the run scope first")
    ap.add_argument("--catalog",type=Path,required=True)
    ap.add_argument("--cases",type=Path,required=True)
    ap.add_argument("--endpoint",default="http://127.0.0.1:18880/search")
    ap.add_argument("--trace-endpoint",default="http://127.0.0.1:18880/debug/lexical")
    ap.add_argument("--fulltext-trace",action="store_true")
    ap.add_argument("--arms",nargs="+",choices=["base","merch","personal","both"],default=["both"])
    ap.add_argument("--max-cases",type=int,default=0)
    ap.add_argument("--keep-going",action="store_true",help="Continue after a mismatch only when explicitly reviewing multiple failures")
    ap.add_argument("--timeout",type=float,default=60.0)
    ap.add_argument("--output",type=Path,required=True)
    args=ap.parse_args()
    if not args.run_approved:ap.error("Refusing execution without --run-approved after operator approval")
    if validate_loopback(args.endpoint)!=validate_loopback(args.trace_endpoint):ap.error("Search and trace must be on the same loopback origin")
    catalog=json.loads(args.catalog.read_text()); package=json.loads(args.cases.read_text())
    prepared=ScanCatalog(catalog)
    cases=load_cases(package,args.max_cases)
    if args.output.exists():ap.error("Output directory exists; preserve earlier evidence and use a fresh directory")
    args.output.mkdir(parents=True)
    (args.output/"traces").mkdir()
    results=[]; failures=[]; trace_cache={}
    with (args.output/"checks.jsonl").open("w") as logfile, (args.output/"expected.jsonl").open("w") as goldenfile, (args.output/"validated_responses.jsonl").open("w") as nativefile:
        for case in cases:
            arms=args.arms if "merch_rules" in case else ["fixture"]
            for arm in arms:
                request=copy.deepcopy(case["request"])
                if arm!="fixture":
                    request["rules"]=copy.deepcopy(case.get("merch_rules",[])) if arm in ("merch","both") else []
                    request["personal_signals"]=copy.deepcopy(case.get("personal_signals",[])) if arm in ("personal","both") else []
                record={"id":case["id"],"arm":arm,"expected_error":case.get("expect_error",False)}
                try:
                    expected=None
                    if not record["expected_error"]:
                        oracle_request=copy.deepcopy(request)
                        profile=request.get("lexical",{}).get("profile","exact_token_and")
                        if profile not in ("exact_token_and","supplied"):
                            if not args.fulltext_trace: raise ValueError("Fulltext requires --fulltext-trace; no independent BM25 oracle is claimed")
                            # Trace is full prefilter authority. User filters must never truncate it.
                            trace_request={"q":request.get("q",""),"lexical":request["lexical"]}
                            key=json.dumps(trace_request,sort_keys=True)
                            if key not in trace_cache:
                                status,trace,_=call(args.trace_endpoint,trace_request,args.timeout)
                                if status!=200:raise RuntimeError(f"lexical trace HTTP {status}: {trace}")
                                hits=trace if isinstance(trace,list) else trace.get("hits")
                                if not isinstance(hits,list):raise ValueError("Unexpected lexical trace response; expected array or {hits: []}")
                                trace_cache[key]=hits
                                trace_name=hashlib.sha256(key.encode()).hexdigest()+".json"
                                (args.output/"traces"/trace_name).write_text(json.dumps({"request":trace_request,"response":trace},ensure_ascii=False,allow_nan=False)+"\n")
                            if "expected_lexical_variant_ids" in case:
                                observed=sorted(h["variant_id"] for h in trace_cache[key])
                                if observed!=sorted(case["expected_lexical_variant_ids"]):raise AssertionError(f"Hand-calculated lexical IDs differ: {observed}")
                            oracle_request["lexical"]={"profile":"supplied","hits":trace_cache[key]}
                            record["lexical_trace_count"]=len(trace_cache[key])
                            record["lexical_trace_sha256"]=hashlib.sha256(json.dumps(trace_cache[key],sort_keys=True).encode()).hexdigest()
                        expected=search(catalog,oracle_request,prepared=prepared)
                        expected["lexical_profile"]=PROFILE_NAMES[profile]
                        goldenfile.write(json.dumps({"id":case["id"],"arm":arm,"request":request,"expected":expected},ensure_ascii=False,allow_nan=False)+"\n");goldenfile.flush()
                    status,actual,elapsed=call(args.endpoint,request,args.timeout,raw=case.get("raw_json"))
                    record.update(status=status,http_ms=elapsed)
                    if record["expected_error"]:
                        passed=400<=status<500 and isinstance(actual,dict) and "error" in actual
                        if not passed:record["differences"]=[("expected 4xx JSON error",actual)]
                    else:
                        diff=differences(expected,actual) if status==200 else [("http_status",200,status),("response",None,actual)]
                        passed=not diff
                        if diff:record["differences"]=diff[:30]
                    record["passed"]=passed
                    if passed and not record["expected_error"]:
                        nativefile.write(json.dumps({"id":case["id"],"arm":arm,"request":request,"response":actual,"request_sha256":canonical_sha(request),"stable_response_sha256":stable_response_sha(actual)},ensure_ascii=False,allow_nan=False)+"\n");nativefile.flush()
                except Exception as error:
                    record.update(passed=False,exception=repr(error))
                results.append(record)
                if not record["passed"]:failures.append(record)
                logfile.write(json.dumps(record,ensure_ascii=False,allow_nan=False)+"\n"); logfile.flush()
                if not record["passed"] and not args.keep_going:break
            if failures and not args.keep_going:break
    summary={"passed":not failures,"checks":len(results),"passed_checks":len(results)-len(failures),"failed_checks":len(failures),
             "catalog_sha256":sha(args.catalog),"cases_sha256":sha(args.cases),"oracle_sha256":sha(Path(__file__).with_name("oracle.py")),"validator_sha256":sha(Path(__file__)),
             "endpoint":args.endpoint,"fulltext_trace_used":bool(trace_cache),"lexical_claim":"Tantivy traces verify pipeline integration, not independent BM25/tokenizer/relevance parity.",
             "failures":failures[:20],"not_claims":["80% Athos coverage","20% Athos total cost"]}
    (args.output/"summary.json").write_text(json.dumps(summary,indent=2,ensure_ascii=False)+"\n")
    print(json.dumps({key:summary[key] for key in ("passed","checks","passed_checks","failed_checks")},indent=2))
    return 0 if not failures else 1
if __name__=="__main__":sys.exit(main())
