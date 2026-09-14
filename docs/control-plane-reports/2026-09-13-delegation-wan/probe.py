from pathlib import Path
import json,urllib.request,urllib.error,time,hashlib,subprocess
D=Path('/tmp/sif-delegation-live');c=json.loads((D/'config.json').read_text());raw=(D/'node2.key').read_text()
def request(url,key=None,data=None,authority=False):
 headers={'User-Agent':'HiveControlPilot/0.3'}
 if key:headers['Authorization']='Bearer '+key
 if authority:headers['apikey']=c['anon_key']
 body=None
 if data is not None:body=json.dumps(data).encode();headers['Content-Type']='application/json'
 req=urllib.request.Request(url,data=body,headers=headers)
 try:
  with urllib.request.urlopen(req,timeout=10) as r:
   b=r.read();return r.status,json.loads(b) if b else None
 except urllib.error.HTTPError as e:return e.code,None
base=c['origin']+'/hive/ctl/1/'
assert request(base+'auth',raw,{})[0]==401
status,v=request(c['authority']+'/rest/v1/rpc/hive_control_delegate',c['anon_key'],{'raw_key':raw,'p_server':c['server_id'],'p_project':c['project_id']},True);assert status==200
delegate=v['delegation'];status,v=request(base+'auth',delegate,{});assert status==200;old=v['token']
status,v=request(base+'rpc',old,{'method':'heartbeat','params':{}});assert status==200;token=v['token'];assert token!=old
assert request(base+'rpc',old,{'method':'get_schedule','params':{}})[0]==401
status,v=request(base+'rpc',token,{'method':'recover_leases','params':{}});assert status==200 and v['result']==[];token=v['token']
assert request(base+'rpc',token,{'method':'release_card','params':{'card_id':'27f794a9-8159-467c-8cb3-d1e534199631'}})[0]==409
print('raw-key rejection, direct authority issuance, heartbeat token rotation, stale-token rejection, empty lease recovery and scope rejection PASS',flush=True)
results=[]
for i in range(3):
 subprocess.run(['/opt/homebrew/bin/supabase','db','query','--linked','--project-ref','pxfbnuxcnerulbvbmowz',"select pg_terminate_backend(pid) from pg_stat_activity where usename='hive_ctl_chicago_pilot' and pid<>pg_backend_pid()",'--output','json'],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 at=time.monotonic();seen=[]
 while True:
  status,_=request(base+'ready');seen.append(status)
  if status==200:break
  assert time.monotonic()-at<40;time.sleep(.1)
 results.append({'seconds_after_termination_ack':round(time.monotonic()-at,3),'observed_unready':503 in seen})
print('natural_disconnect_samples='+json.dumps(results),flush=True)
hash=hashlib.sha256(delegate.encode()).hexdigest()
subprocess.run(['/opt/homebrew/bin/supabase','db','query','--linked','--project-ref','pxfbnuxcnerulbvbmowz',f"update hive.ctl_delegations set revoked_at=now() where hash='{hash}'",'--output','json'],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
assert request(base+'auth',delegate,{})[0]==401
assert request(base+'rpc',token,{'method':'heartbeat','params':{}})[0]==503
print('revoked delegation auth 401 and heartbeat 503 PASS',flush=True)
