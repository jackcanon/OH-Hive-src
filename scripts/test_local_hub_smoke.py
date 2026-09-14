import subprocess,tempfile,pathlib,json,uuid,socket,time,urllib.request,urllib.error
bin=str(pathlib.Path('target/debug/examples/local_hub').resolve())
def cmd(*a):return subprocess.check_output([bin,*map(str,a)],text=True).strip()
with tempfile.TemporaryDirectory(prefix='hive-local-smoke-') as td:
 d=pathlib.Path(td);db=d/'local.sqlite';creds=d/'owner.json';cmd('init',db,creds);owner=json.loads(creds.read_text());project=cmd('project',db,'Standalone smoke','Synthetic private task')
 card={'id':str(uuid.uuid4()),'project_id':project,'key':'smoke','title':'Smoke fixture','modality':'text','inputs':'Synthetic private task','acceptance':'fixture result','required_capabilities':{'loop':'single'}}
 fixture=d/'card.json';fixture.write_text(json.dumps(card));cmd('card',db,fixture)
 sock=socket.socket();sock.bind(('127.0.0.1',0));port=sock.getsockname()[1];sock.close();origin=f'http://127.0.0.1:{port}'
 server=subprocess.Popen([bin,'serve',str(db),f'127.0.0.1:{port}'],stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,text=True)
 try:
  for _ in range(50):
   try:
    with socket.create_connection(('127.0.0.1',port),timeout=.2):break
   except OSError:time.sleep(.1)
  def call(path,body,key=None):
   h={'content-type':'application/json'}
   if key:h['Authorization']='Bearer '+key
   req=urllib.request.Request(origin+path,data=json.dumps(body).encode(),headers=h)
   with urllib.request.urlopen(req,timeout=5) as r:return json.load(r)
  code=cmd('pair-code',db);other=call('/local/v1/pair',{'code':code,'name':'second-process'})
  caps={'hardware':{'cpu_model':'fixture','cpu_cores':2,'ram_bytes':1000000000,'gpu_vendor':'none','disk_free_bytes':1000000000},'modalities':['text'],'models':[],'allow_internet':False,'tools_level':'inference_only'}
  session1=str(uuid.uuid4());session2=str(uuid.uuid4())
  def rpc(c,s,m,p={}):return call('/local/v1/rpc',{'session':s,'method':m,'params':p},c['raw_key'])
  rpc(owner,session1,'check_in',{'caps':caps,'region':None});rpc(other,session2,'check_in',{'caps':caps,'region':None})
  assert rpc(owner,session1,'claim_card')['status']=='leased'
  assert rpc(other,session2,'claim_card')['status']=='nothing_to_do'
  usage={'tokens_in':1,'tokens_out':1,'compute_seconds':0}
  rpc(owner,session1,'checkpoint',{'card_id':card['id'],'step':3,'state':{'private_fixture':'saved'},'usage':usage})
  rpc(owner,session1,'release_card',{'card_id':card['id'],'reason':'smoke handoff'})
  leased=rpc(other,session2,'claim_card');assert leased['checkpoint']['state']['private_fixture']=='saved'
  rpc(other,session2,'complete_card',{'card_id':card['id'],'content':'standalone local result','model_id':None,'usage':usage})
  state=json.loads(cmd('inspect',db));assert state['outputs'][0]['content']=='standalone local result'
  print('Standalone processes: local pairing, competing claims, checkpoint handoff and completion PASS; no cloud endpoint used.')
 finally:
  server.terminate();server.wait(timeout=5)
 print('Persisted SQLite state after server exit:',json.loads(cmd('inspect',db))['cards'][0]['status'])
