#!/usr/bin/env python3
"""Synthetic Rust host + independent Python reader on a second physical machine via SSH.
No private keys or node credentials are written to reports. Remote reader retains its key in RAM.
"""
import json, pathlib, selectors, shlex, subprocess, sys
REMOTE = r'''
import json,sys,uuid,urllib.request,urllib.error
key=None
session=str(uuid.uuid4())
for line in sys.stdin:
    c=json.loads(line)
    path='/local/v1/pair' if c['op']=='pair' else '/local/v1/rpc'
    body={'code':c['code'],'name':'synthetic-vault-reader'} if c['op']=='pair' else {'session':session,'method':c['method'],'params':c.get('params',{})}
    headers={'Content-Type':'application/json'}
    if key: headers['Authorization']='Bearer '+key
    try:
        req=urllib.request.Request(c['origin']+path,data=json.dumps(body).encode(),headers=headers)
        opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
        with opener.open(req,timeout=10) as response: data=json.load(response)
        if c['op']=='pair': key=data.pop('raw_key')
        print(json.dumps({'status':200,'data':data}),flush=True)
    except urllib.error.HTTPError as e: print(json.dumps({'status':e.code}),flush=True)
    except urllib.error.URLError: print(json.dumps({'status':'unreachable'}),flush=True)
'''
def read(p):
    with selectors.DefaultSelector() as selector:
        selector.register(p.stdout,selectors.EVENT_READ)
        if not selector.select(30): raise TimeoutError('validation process timed out')
    line=p.stdout.readline()
    if not line: raise RuntimeError('validation process exited')
    return json.loads(line)
def send(p,c):
    p.stdin.write(json.dumps(c)+'\n');p.stdin.flush();return read(p)
def main():
    binary,bind,ssh_host=sys.argv[1:4]
    host=subprocess.Popen([binary,bind],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    reader=None
    try:
        setup=read(host);origin=setup['origin'];vault=setup['vault']
        reader=subprocess.Popen(['ssh','-o','BatchMode=yes','-o','ConnectTimeout=8',ssh_host,'python3 -u -c '+shlex.quote(REMOTE)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        pair=send(reader,{'op':'pair','origin':origin,'code':setup['pair_code']});assert pair['status']==200
        node=pair['data']['node_id']
        def rpc(method,**params):return send(reader,{'op':'rpc','origin':origin,'method':method,'params':params})
        assert rpc('vault_list')['data']==[]
        assert rpc('vault_search',vault_id=vault,query='acceptance',limit=10)['status']==409
        send(host,{'op':'grant','node':node,'enabled':True})
        hit=rpc('vault_search',vault_id=vault,query='acceptance',limit=10)['data'][0]
        assert rpc('vault_read',vault_id=vault,document_id=hit['id'],revision=hit['revision'])['data']['content']=='# First\nacceptance alpha'
        send(host,{'op':'edit'})
        updated=rpc('vault_search',vault_id=vault,query='acceptance',limit=10)['data'][0]
        assert updated['id']==hit['id'] and updated['revision']!=hit['revision'] and updated['path']=='renamed.md'
        assert rpc('vault_read',vault_id=vault,document_id=hit['id'],revision=hit['revision'])['status']==409
        send(host,{'op':'missing'})
        assert rpc('vault_search',vault_id=vault,query='acceptance',limit=10)['status']==503
        send(host,{'op':'restore'})
        assert len(rpc('vault_search',vault_id=vault,query='acceptance',limit=10)['data'])==1
        send(host,{'op':'grant','node':node,'enabled':False})
        assert rpc('vault_search',vault_id=vault,query='acceptance',limit=10)['status']==409
        send(host,{'op':'grant','node':node,'enabled':True})
        send(host,{'op':'revoke','node':node})
        assert rpc('vault_list')['status']==401
        send(host,{'op':'stop'});host.wait(timeout=10)
        assert rpc('vault_list')['status']=='unreachable'
        print(json.dumps({'result':'PASS','host':origin,'reader':ssh_host,'checks':['pairing','default-deny','grant','search','revision-read','rename-identity','stale-read-rejection','missing-source-503','source-recovery','grant-revocation','node-revocation','host-offline-error'],'cleanup':'host exited; synthetic folder removed; remote credentials remained in RAM'}))
    finally:
        if host.poll() is None:
            host.stdin.close()
            try: host.wait(timeout=10)
            except subprocess.TimeoutExpired: host.terminate();host.wait(timeout=10)
        if reader:
            reader.stdin.close();reader.wait(timeout=15)
if __name__=='__main__':main()
