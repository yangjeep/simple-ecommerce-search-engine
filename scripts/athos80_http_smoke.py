#!/usr/bin/env python3
"""Small loopback correctness smoke; not a load test or performance benchmark."""
import json, pathlib, socket, subprocess, sys, time, urllib.request, urllib.error
root = pathlib.Path(__file__).resolve().parents[1]
binary = pathlib.Path(sys.argv[1]) if len(sys.argv)>1 else root/'target/debug/athos80_server'
artifact_dir = pathlib.Path(sys.argv[2]) if len(sys.argv)>2 else root/'artifacts/athos80-v1.1'
artifact_dir.mkdir(parents=True,exist_ok=True)
artifact = artifact_dir/'http-smoke.json'
checks=[]
proc = subprocess.Popen([str(binary),str(root/'crates/athos80-eval/examples/catalog.json'),'--addr','127.0.0.1:0','--bm25','--trace'],stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,text=True)
try:
    startup=json.loads(proc.stderr.readline())
    host,port=startup['listening'].rsplit(':',1); port=int(port)
    base=f'http://{host}:{port}'
    def call(path,obj=None):
        data=None if obj is None else json.dumps(obj).encode()
        req=urllib.request.Request(base+path,data=data,headers={'Content-Type':'application/json'})
        try:
            with urllib.request.urlopen(req,timeout=10) as r: return r.status,json.load(r)
        except urllib.error.HTTPError as e: return e.code,json.load(e)
    def check(name,condition):
        checks.append({'name':name,'passed':bool(condition)})
        if not condition: raise AssertionError(name)
    status,res=call('/health');check('health',status==200 and res['bm25_built'] and not res['production'])
    req=json.loads((root/'crates/athos80-eval/examples/request.json').read_text())
    status,res=call('/search',req);check('happy product count and exact IDs',status==200 and res['num_found']==2 and [(h['product_id'],h['selected_variant_id']) for h in res['hits']]==[(2,21),(3,31)])
    status,res=call('/search',{'q':'shoes','lexical':{'profile':'bm25'}});check('bm25 happy',status==200 and res['num_found']==3 and all(h['relevance_bucket']==0 for h in res['hits']))
    status,trace=call('/debug/lexical',{'q':'shoes','lexical':{'profile':'bm25'}});check('complete lexical trace including out-of-stock',status==200 and len(trace['hits'])==5)
    status,res=call('/search',{'q':'shoes','lexical':{'profile':'supplied','hits':trace['hits']}});check('trace replay',status==200 and res['num_found']==3)
    for name,q,count in [('literal inch quotes','shoes""',3),('literal AND not operator','shoes AND',0),('literal colon not field','absentfield:shoes',0),('literal punctuation no implicit browse','""[]():^',0),('literal blank browse','   ',3)]:
        status,res=call('/search',{'q':q,'lexical':{'profile':'bm25_literal'}});check(name,status==200 and res['num_found']==count)
    status,literal_trace=call('/debug/lexical',{'q':'shoes""','lexical':{'profile':'bm25_literal'}});check('literal complete trace',status==200 and len(literal_trace['hits'])==5)
    status,literal_result=call('/search',{'q':'shoes""','lexical':{'profile':'bm25_literal'}})
    replay_status,replay_result=call('/search',{'q':'shoes""','lexical':{'profile':'supplied','hits':literal_trace['hits']}})
    check('literal trace exact hit replay',status==replay_status==200 and literal_result['hits']==replay_result['hits'])
    for name,bad in [('unknown schema',{'surprise':True}),('invalid pin',{'rules':[{'id':'bad','target':{'type':'products','ids':[1]},'action':{'type':'pin','position':99}}]}),('malformed bm25',{'q':'shoes AND (','lexical':{'profile':'bm25'}}),('semantic page cap',{'limit':1001}),('huge query boost',{'q':'shoes^1000000000000000','lexical':{'profile':'bm25'}})]:
        status,res=call('/search',bad);check(name,status==400 and 'error' in res)
    def raw(payload,delay=0):
        with socket.create_connection((host,port),timeout=10) as s:
            s.settimeout(10); s.sendall(payload)
            if delay: time.sleep(delay)
            data=b''
            while True:
                chunk=s.recv(65536)
                if not chunk: break
                data+=chunk
            return data
    for name,payload in [('body limit',b'POST /search HTTP/1.1\r\nHost: localhost\r\nContent-Length: 8388609\r\n\r\n'),('duplicate length',b'POST /search HTTP/1.1\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\n{}'),('transfer encoding',b'POST /search HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n')]:
        response=raw(payload);check(name,response.startswith(b'HTTP/1.1 400'))
    response=raw(b'POST /search HTTP/1.1\r\nHost: localhost\r\n',5.3);check('absolute read deadline',response.startswith(b'HTTP/1.1 400') and b'deadline' in response)
    status,res=call('/health');check('healthy after errors and timeout',status==200)
    out={'passed':True,'checks':checks,'count':len(checks),'server':startup,'scope':'local correctness smoke, no load/cost claim'}
except Exception as error:
    out={'passed':False,'checks':checks,'error':repr(error)}
    raise
finally:
    proc.terminate()
    try:proc.wait(timeout=5)
    except subprocess.TimeoutExpired:proc.kill();proc.wait()
    if 'out' in locals():artifact.write_text(json.dumps(out,indent=2)+'\n');print(json.dumps(out,indent=2))
