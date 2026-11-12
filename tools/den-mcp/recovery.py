"""Fixed-profile private-pilot recovery controls; no arbitrary host/process commands."""
from __future__ import annotations
import asyncio, hashlib, json, os, shlex, sqlite3, subprocess, time
from pathlib import Path
from uuid import UUID
from mcp.server.mcpserver.exceptions import ToolError
from mcp.types import ToolAnnotations

HUB_HOST='jack@192.168.1.204'
HUB_BINARY='/Users/jack/hive-hub-home/hive'
HUB_SHA='e6ba418e62479672af3b9db9cab84540d4139d4948e39214ea697d1dea0e030c'
HUB_LABEL='media.happyjack.hive-hub'
HUB_SCRIPT=r'''
import hashlib,json,os,plistlib,re,signal,sqlite3,subprocess,sys,time,fcntl,stat
request=json.load(sys.stdin)
label='media.happyjack.hive-hub'
program='/Users/jack/hive-hub-home/hive'
expected='e6ba418e62479672af3b9db9cab84540d4139d4948e39214ea697d1dea0e030c'
config=plistlib.load(open('/Library/LaunchDaemons/'+label+'.plist','rb'))
assert config.get('UserName')=='jack' and config.get('KeepAlive') is True
assert program in config.get('ProgramArguments',[]) or config.get('Program')==program
sha=hashlib.sha256(open(program,'rb').read()).hexdigest()
assert sha==expected,'Hub binary changed; reconcile provenance before recovery'
def status():
    text=subprocess.check_output(['launchctl','print','system/'+label],text=True)
    match=re.search(r'^\s*pid = (\d+)\s*$',text,re.M)
    assert match,'Hub has no launchd PID'
    pid=int(match.group(1))
    uid=int(subprocess.check_output(['ps','-p',str(pid),'-o','uid='],text=True).strip())
    assert uid==os.getuid(),'Hub is not owned by this account'
    c=sqlite3.connect('file:/Users/jack/hive-hub-home/Library/Application Support/ohhive/vault-host.sqlite3?mode=ro',uri=True)
    counts={t:c.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ['projects','cards','vaults','vault_documents','applied_migrations']}
    c.close()
    return {'pid':pid,'binary_sha256':sha,'launchd_label':label,'counts':counts}
lock=None
if request.get('operation')=='restart':
    lock=os.open('/Users/jack/hive-hub-home/.owner-restart.lock',os.O_CREAT|os.O_RDWR|os.O_NOFOLLOW,0o600)
    info=os.fstat(lock)
    assert stat.S_ISREG(info.st_mode) and info.st_uid==os.getuid() and not info.st_mode & 0o077
    fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
before=status()
if request.get('operation')=='restart':
    assert before['pid']==request['expected_pid'],'Hub PID changed; no second restart performed'
    c=sqlite3.connect('file:/Users/jack/hive-hub-home/Library/Application Support/ohhive/vault-host.sqlite3?mode=ro',uri=True)
    active={row[0] for row in c.execute('SELECT DISTINCT card_id FROM leases WHERE expires>?',(int(time.time()),))};c.close()
    allowed={request['active_task_id']} if request.get('active_task_id') else set()
    assert active<=allowed,'Unrelated leased work exists; restart refused'
    os.kill(before['pid'],signal.SIGTERM)
    for _ in range(100):
        time.sleep(.2)
        try:
            after=status()
            if after['pid']!=before['pid']:break
        except Exception:continue
    else:raise RuntimeError('Hub did not restart; inspect launchd before retrying')
    print(json.dumps({'before':before,'after':after}))
else:print(json.dumps(before))
'''
SELECTION_SCRIPT=r'''
import json,pathlib,sys,urllib.parse
config=json.loads((pathlib.Path.home()/'Library/Application Support/ohhive/private-primary.json').read_text())
endpoint=urllib.parse.urlparse(config['endpoint'])
assert endpoint.scheme=='http' and endpoint.hostname=='192.168.1.204' and endpoint.port==8787, 'Primary selection differs from the fixed recovery profile'
print(json.dumps({'fixed_profile_matches':True}))
'''
PROBE_SCRIPT=r'''
import datetime,hashlib,json,os,pathlib,stat,subprocess,sys,uuid
request=json.load(sys.stdin);task=str(uuid.UUID(request['task_id']))
base=pathlib.Path.home()/'Library/Application Support/ohhive'
marker=base/'pilot-recovery-markers'/('mcp-'+task+'.json')
result={'task_id':task,'marker_exists':marker.exists()}
if marker.exists():
    assert not marker.is_symlink() and marker.stat().st_size<4096
    data=json.loads(marker.read_text());pid=data['pid'];assert type(pid)==int and pid>1
    started=subprocess.run(['ps','-p',str(pid),'-o','lstart='],capture_output=True,text=True).stdout.strip()
    result.update({'pid':pid,'same_process_alive':bool(started) and started==data['started'],'marker':{'pid':pid,'started':data['started']}})
    file=base/'sandbox/code-workspaces'/task/'recovery_probe.py'
    assert not file.is_symlink()
    if file.exists():
        assert file.is_file() and file.stat().st_size<8192
        content=file.read_bytes();result.update({'file_sha256':hashlib.sha256(content).hexdigest(),'file_content':content.decode()})
print(json.dumps(result))
'''
# SQLite's online backup API avoids copying a live WAL database inconsistently.
BACKUP_SCRIPT=r'''
import ctypes as C,sys,time
s=C.CDLL('/usr/lib/libsqlite3.dylib');ptr=C.c_void_p
s.sqlite3_open_v2.argtypes=[C.c_char_p,C.POINTER(ptr),C.c_int,C.c_char_p]
s.sqlite3_backup_init.argtypes=[ptr,C.c_char_p,ptr,C.c_char_p];s.sqlite3_backup_init.restype=ptr
s.sqlite3_backup_step.argtypes=[ptr,C.c_int];s.sqlite3_backup_finish.argtypes=[ptr]
s.sqlite3_serialize.argtypes=[ptr,C.c_char_p,C.POINTER(C.c_longlong),C.c_uint];s.sqlite3_serialize.restype=ptr
s.sqlite3_free.argtypes=[ptr];s.sqlite3_close.argtypes=[ptr]
source=ptr();dest=ptr()
assert s.sqlite3_open_v2(b'/Users/jack/hive-hub-home/Library/Application Support/ohhive/vault-host.sqlite3',C.byref(source),1,None)==0
assert s.sqlite3_open_v2(b':memory:',C.byref(dest),6,None)==0
backup=s.sqlite3_backup_init(dest,b'main',source,b'main');assert backup
deadline=time.monotonic()+20
while True:
    rc=s.sqlite3_backup_step(backup,256)
    if rc==101:break
    assert rc in (0,5,6) and time.monotonic()<deadline
    if rc in (5,6):time.sleep(.05)
assert s.sqlite3_backup_finish(backup)==0;s.sqlite3_close(source)
size=C.c_longlong();blob=s.sqlite3_serialize(dest,b'main',C.byref(size),0)
assert blob and 0<size.value<=512*1024*1024
sys.stdout.buffer.write(C.string_at(blob,size.value));s.sqlite3_free(blob);s.sqlite3_close(dest)
'''

def ssh_args(host,script):
    return ['ssh','-o','BatchMode=yes','-o','IdentitiesOnly=yes','-o','ConnectTimeout=5',host,'python3 -c '+shlex.quote(script)]

def remote(host,script,payload):
    result=subprocess.run(ssh_args(host,script),input=json.dumps(payload),text=True,capture_output=True,timeout=30)
    if result.returncode:raise ToolError('Recovery operation failed; inspect status before retrying. No automatic repeat was made.')
    return json.loads(result.stdout)

def guard_other_work(project_tasks,active_task_id):
    for task in project_tasks:
        run=task.get('run') or {}
        active=run.get('lease_active') or (run.get('state') in ['queued','running'] and not run.get('stop_requested'))
        if active and task['task_id']!=active_task_id:
            raise ToolError('Unrelated active work exists; hub restart refused')

def save_receipt(path,value):
    temp=path.with_suffix('.tmp');fd=os.open(temp,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
    with os.fdopen(fd,'w') as stream:
        json.dump(value,stream,indent=2);stream.flush();os.fsync(stream.fileno())
    os.replace(temp,path)

def backup_hub(path):
    with path.open('xb') as output:
        os.chmod(path,0o600)
        result=subprocess.run(ssh_args(HUB_HOST,BACKUP_SCRIPT),stdout=output,stderr=subprocess.PIPE,timeout=30)
    if result.returncode:raise ToolError('Hub backup failed; restart was not attempted')
    connection=sqlite3.connect('file:'+str(path)+'?mode=ro',uri=True)
    try:
        if connection.execute('PRAGMA integrity_check').fetchone()[0]!='ok':raise ToolError('Backup integrity failed; no restart attempted')
    finally:connection.close()
    return {'path':str(path),'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'integrity':'ok'}


def register(mcp,app_request,rpc,hosts):
    @mcp.tool(annotations=ToolAnnotations(readOnlyHint=True))
    async def den_hub_status() -> dict:
        """Read the fixed Asgard pilot hub's launchd PID, verified binary hash and aggregate database counts. No document contents or credentials."""
        await app_request('helheim','status')
        await asyncio.to_thread(remote,hosts['helheim'],SELECTION_SCRIPT,{})
        return await asyncio.to_thread(remote,HUB_HOST,HUB_SCRIPT,{'operation':'status'})

    @mcp.tool(annotations=ToolAnnotations(readOnlyHint=True))
    async def den_recovery_probe(project_id: UUID,task_id: UUID) -> dict:
        """Inspect an owned pilot task's fixed recovery marker, command PID identity and retained probe file. Cannot inspect arbitrary processes or paths."""
        tasks=await rpc('helheim','private_coding_tasks',{'project':str(project_id)})
        task=next((t for t in tasks if t['task_id']==str(task_id)),None)
        if task is None or task.get('target_name')!='Helheim':raise ToolError('Task is not assigned to this recovery host')
        return await asyncio.to_thread(remote,hosts['helheim'],PROBE_SCRIPT,{'task_id':str(task_id)})

    @mcp.tool(annotations=ToolAnnotations(destructiveHint=True))
    async def den_hub_restart(request_id: UUID,active_task_id: UUID | None=None) -> dict:
        """Restart only the verified Asgard pilot hub through its existing launchd supervisor. Refuses unrelated active coding runs, validates an online backup, and preserves an idempotent recovery receipt. No binary/config/database replacement. Reuse request_id after uncertain delivery; inspect pending outcomes instead of blindly restarting again."""
        status=await app_request('helheim','status')
        if status.get('shuttingDown'):raise ToolError('App is shutting down')
        root=Path(__file__).resolve().parents[3]/'backups/mcp-recovery'
        if not os.path.ismount('/Volumes/10TB JBOD'):raise ToolError('Required external volume is not mounted')
        await asyncio.to_thread(remote,hosts['helheim'],SELECTION_SCRIPT,{})
        root.mkdir(parents=True,exist_ok=True);os.chmod(root,0o700)
        receipt=root/(str(request_id)+'.json')
        if receipt.exists():
            previous=json.loads(receipt.read_text())
            if previous['active_task_id']!=(str(active_task_id) if active_task_id else None):raise ToolError('Recovery request conflicts with prior intent')
            if previous['state']=='complete':return previous
            now=await asyncio.to_thread(remote,HUB_HOST,HUB_SCRIPT,{'operation':'status'})
            if now['pid']!=previous['before']['pid']:
                previous.update(state='complete',after=now);save_receipt(receipt,previous);return previous
            raise ToolError('Recovery request is pending with the same PID; inspect outcome before authorizing a new request')
        projects=await rpc('helheim','private_repository_projects',{})
        for project in projects:
            tasks=await rpc('helheim','private_coding_tasks',{'project':project['id']})
            guard_other_work(tasks,str(active_task_id) if active_task_id else None)
        before=await asyncio.to_thread(remote,HUB_HOST,HUB_SCRIPT,{'operation':'status'})
        backup=await asyncio.to_thread(backup_hub,root/(str(request_id)+'.sqlite3'))
        # Recheck after backup, immediately before changing the service.
        await asyncio.to_thread(remote,hosts['helheim'],SELECTION_SCRIPT,{})
        for project in projects:
            guard_other_work(await rpc('helheim','private_coding_tasks',{'project':project['id']}),str(active_task_id) if active_task_id else None)
        intent={'state':'pending','request_id':str(request_id),'guard_scope':'Owner-visible private coding runs; other hub features briefly disconnect.','active_task_id':str(active_task_id) if active_task_id else None,'before':before,'backup':backup}
        save_receipt(receipt,intent)
        result=await asyncio.to_thread(remote,HUB_HOST,HUB_SCRIPT,{'operation':'restart','expected_pid':before['pid'],'active_task_id':str(active_task_id) if active_task_id else None})
        intent.update(state='complete',after=result['after']);save_receipt(receipt,intent)
        return intent
