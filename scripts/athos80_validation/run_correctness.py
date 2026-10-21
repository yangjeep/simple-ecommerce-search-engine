#!/usr/bin/env python3
"""Own isolated, short-lived Rust server for correctness only; never compile/load."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import shutil
import subprocess
import sys
import time
import urllib.request
import uuid
from validate_http import loopback_opener
ROOT=Path(__file__).resolve().parent

def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def listener_owned(pid,port):
    try:
        inodes={p.readlink().as_posix()[8:-1] for p in Path(f'/proc/{pid}/fd').iterdir() if p.is_symlink() and p.readlink().as_posix().startswith('socket:[')}
        for table in ['/proc/net/tcp','/proc/net/tcp6']:
            for line in Path(table).read_text().splitlines()[1:]:
                fields=line.split()
                if int(fields[1].split(':')[1],16)==port and fields[3]=='0A' and fields[9] in inodes:return True
    except (OSError,ValueError,IndexError):pass
    return False
def main():
    ap=argparse.ArgumentParser();ap.add_argument('--run-approved',action='store_true');ap.add_argument('--binary',type=Path,required=True)
    ap.add_argument('--phase',choices=['synthetic','global','wands'],required=True);ap.add_argument('--port',type=int,required=True)
    ap.add_argument('--data-root',type=Path);ap.add_argument('--output',type=Path,required=True);ap.add_argument('--max-cases',type=int,default=0);ap.add_argument('--expect-binary-sha256',required=True)
    args=ap.parse_args()
    if not args.run_approved:ap.error('Explicit local-run approval required')
    if sha(args.binary)!=args.expect_binary_sha256:ap.error('Frozen binary hash changed')
    if args.output.exists():ap.error('Output must be a fresh run directory')
    with socket.socket() as s:
        try:s.bind(('127.0.0.1',args.port))
        except OSError:ap.error('Port occupied; will not touch existing process')
    args.output.mkdir(parents=True)
    if args.phase=='synthetic':catalog=ROOT/'fixtures/synthetic_catalog.json';suites=[('supplied-exact',ROOT/'fixtures/synthetic_cases.json',False),('bm25',ROOT/'fixtures/bm25_cases.json',True),('bm25-literal',ROOT/'fixtures/bm25_literal_cases.json',True)]
    elif args.phase=='global':catalog=ROOT/'fixtures/global_topk_catalog.json';suites=[('global-topk',ROOT/'fixtures/global_topk_cases.json',False)]
    else:
        data=args.data_root or ROOT/'wands';catalog=data/'catalog.json';suites=[('wands-all-arms',data/'workload.json',True)]
    command=[str(args.binary.resolve()),str(catalog),'--addr',f'127.0.0.1:{args.port}','--workers','1','--bm25','--trace']
    env=dict(os.environ);env['ATHOS80_CORRECTNESS_OWNER']=uuid.uuid4().hex
    manifest={'phase':args.phase,'command':command,'catalog_sha256':sha(catalog),'binary_sha256':sha(args.binary),'owner_nonce':env['ATHOS80_CORRECTNESS_OWNER'],'purpose':'HTTP correctness only, not performance','suites':[]}
    frozen=[args.binary,catalog,*[case for _,case,_ in suites],*[ROOT/name for name in ['run_correctness.py','validate_http.py','oracle.py']]]
    manifest['pre_hashes']={str(p):sha(p) for p in frozen}
    (args.output/'sources').mkdir()
    for source in [ROOT/name for name in ['run_correctness.py','validate_http.py','oracle.py']]:shutil.copy2(source,args.output/'sources'/source.name)
    (args.output/'inputs').mkdir()
    for name,case,_ in suites:shutil.copy2(case,args.output/'inputs'/(name+'-cases.json'))
    process=None;code=1
    try:
        with (args.output/'server.log').open('wb') as log:
            process=subprocess.Popen(command,stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT,env=env,start_new_session=True)
            manifest['pid']=process.pid
            deadline=time.monotonic()+180
            while True:
                if process.poll() is not None:raise RuntimeError(f'Server exited {process.returncode}')
                try:
                    if not listener_owned(process.pid,args.port):raise RuntimeError('Owned listener not ready')
                    with loopback_opener().open(f'http://127.0.0.1:{args.port}/health',timeout=1) as r:manifest['health']=json.load(r)
                    break
                except Exception:
                    if time.monotonic()>deadline:raise RuntimeError('Server readiness timed out')
                    time.sleep(.1)
            for name,cases,trace in suites:
                run=[sys.executable,str(ROOT/'validate_http.py'),'--run-approved','--catalog',str(catalog),'--cases',str(cases),'--endpoint',f'http://127.0.0.1:{args.port}/search','--trace-endpoint',f'http://127.0.0.1:{args.port}/debug/lexical','--output',str(args.output/name)]
                if trace:run.append('--fulltext-trace')
                if args.phase=='wands':run+=['--arms','base','merch','personal','both']
                if args.max_cases:run+=['--max-cases',str(args.max_cases)]
                completed=subprocess.run(run)
                manifest['suites'].append({'name':name,'cases_sha256':sha(cases),'exit_code':completed.returncode})
                if completed.returncode!=0:raise RuntimeError(f'Correctness suite {name} failed; stopping before subsequent suites')
            code=0
    except Exception as error:manifest['error']=repr(error);print(repr(error),file=sys.stderr)
    finally:
        # Popen is our live child (not a PID lookup). Reap only our own Rust server;
        # this reviewed thread-only server does not launch adapter/daemon children.
        if process is not None:
            if process.poll() is None:
                process.terminate()
                try:process.wait(timeout=10)
                except subprocess.TimeoutExpired:process.kill();process.wait(timeout=10)
            manifest['server_exit_code']=process.returncode
        manifest['post_hashes']={str(p):sha(p) for p in frozen}
        manifest['frozen_inputs_unchanged']=manifest['pre_hashes']==manifest['post_hashes']
        if not manifest['frozen_inputs_unchanged']:code=1;manifest['error']='Frozen code/input/binary changed during correctness run'
        manifest['owned_listener_remaining']=False if process is None else listener_owned(process.pid,args.port)
        if manifest['owned_listener_remaining']:code=1;manifest['error']='Owned listener unexpectedly remains after cleanup'
        manifest['passed']=code==0
        (args.output/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    return code
if __name__=='__main__':sys.exit(main())
