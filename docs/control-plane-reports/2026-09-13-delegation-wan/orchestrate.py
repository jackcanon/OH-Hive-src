from pathlib import Path
import subprocess,time,json,urllib.request,urllib.error
D=Path('/tmp/sif-delegation-live')
def sql(q):
 return subprocess.run(['/opt/homebrew/bin/supabase','db','query','--linked','--project-ref','pxfbnuxcnerulbvbmowz',q,'--output','json'],check=True,capture_output=True,text=True)
def ssh(q):return subprocess.check_output(['ssh','-o','BatchMode=yes','-o','ConnectTimeout=8','root@100.100.2.120',q],text=True).strip()
def ready():
 try:
  req=urllib.request.Request('https://chicago.ohghive.com/hive/ctl/1/ready',headers={'User-Agent':'HiveControlPilot/0.3'})
  with urllib.request.urlopen(req,timeout=4) as r:return r.status
 except urllib.error.HTTPError as e:return e.code
 except Exception:return 0
def wait_file(name):
 end=time.monotonic()+150
 while not (D/name).exists():
  if time.monotonic()>end:raise RuntimeError('client phase timeout '+name)
  time.sleep(.2)
result={};wait_file('database.waiting');pid=ssh('systemctl show hive-control-pilot -p MainPID --value')
try:
 sql("alter role hive_ctl_chicago_pilot nologin; select pg_terminate_backend(pid) from pg_stat_activity where usename='hive_ctl_chicago_pilot' and pid<>pg_backend_pid()")
 result['outage_readiness_status']=ready();assert result['outage_readiness_status']==503
 (D/'database.go').write_text('go');time.sleep(2)
 sql('alter role hive_ctl_chicago_pilot login')
 at=time.monotonic()
 while ready()!=200:
  assert time.monotonic()-at<45;time.sleep(.15)
 result['wan_ready_after_login_restored_seconds']=round(time.monotonic()-at,3)
 result['same_process_after_database_recovery']=pid==ssh('systemctl show hive-control-pilot -p MainPID --value')
 wait_file('restart.waiting')
 ssh('systemctl restart hive-control-pilot');(D/'restart.go').write_text('go')
 print(json.dumps(result,indent=2),flush=True)
finally:
 sql('alter role hive_ctl_chicago_pilot login')
